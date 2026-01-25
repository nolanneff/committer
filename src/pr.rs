//! Pull request generation and GitHub CLI integration.
//!
//! This module handles the `committer pr` subcommand workflow:
//!
//! 1. Validates GitHub CLI is installed and authenticated
//! 2. Detects base branch automatically (or uses `--base`)
//! 3. Handles uncommitted changes (commit, skip, or quit)
//! 4. Generates PR title and description using LLM
//! 5. Pushes branch and creates PR via GitHub CLI
//!
//! # Example
//!
//! ```bash
//! committer pr              # Interactive PR creation
//! committer pr --yes        # Auto-create without confirmation
//! committer pr --draft      # Create as draft PR
//! committer pr --dry-run    # Preview without creating
//! ```

use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use tokio::process::Command;

use crate::api::{stream_commit_message, stream_pr_content};
use crate::branch::PROTECTED_BRANCHES;
use crate::cli::PrArgs;
use crate::config::{get_api_key, Config};
use crate::git::{
    branch_has_merge_base, get_branch_commits, get_branch_diff, get_cached_remote_head,
    get_current_branch, get_git_diff, get_pr_changed_files, get_remote_default_branch,
    get_staged_files, get_uncommitted_changes, get_upstream_remote, push_branch_with_spinner,
    run_git_commit, stage_all_changes,
};
use crate::ui::{
    prompt_commit, prompt_pr, prompt_uncommitted_changes, CommitAction, PrAction,
    UncommittedAction,
};

/// Checks if the GitHub CLI (`gh`) is installed.
pub async fn check_gh_installed() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("gh").args(["--version"]).output().await;

    match output {
        Ok(o) if o.status.success() => Ok(()),
        _ => {
            Err("GitHub CLI (gh) is not installed.\n\
                 Install it from: https://cli.github.com/\n\
                 Then run: gh auth login"
                .into())
        }
    }
}

/// Detects the default base branch for the PR.
///
/// Tries multiple strategies: GitHub CLI, cached origin/HEAD, remote query,
/// and common branch name fallbacks.
pub async fn get_default_base_branch(verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
    // Strategy 1: Try gh CLI (works for GitHub repos)
    let gh_output = Command::new("gh")
        .args(["repo", "view", "--json", "defaultBranchRef", "-q", ".defaultBranchRef.name"])
        .output()
        .await;

    if let Ok(output) = gh_output {
        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !branch.is_empty() && branch_has_merge_base(&branch).await {
                if verbose {
                    eprintln!("— Base branch detection: gh CLI (GitHub API)");
                }
                return Ok(branch);
            }
            // gh returned a branch but no merge base - try with origin/ prefix
            let origin_branch = format!("origin/{}", branch);
            if branch_has_merge_base(&origin_branch).await {
                if verbose {
                    eprintln!("— Base branch detection: gh CLI (GitHub API, using origin/)");
                }
                return Ok(origin_branch);
            }
        }
    }

    // Strategy 2: Try cached git symbolic-ref for origin/HEAD
    if let Some(branch) = get_cached_remote_head().await {
        if branch_has_merge_base(&branch).await {
            if verbose {
                eprintln!("— Base branch detection: cached origin/HEAD ref");
            }
            return Ok(branch);
        }
        let origin_branch = format!("origin/{}", branch);
        if branch_has_merge_base(&origin_branch).await {
            if verbose {
                eprintln!("— Base branch detection: cached origin/HEAD ref (using origin/)");
            }
            return Ok(origin_branch);
        }
    }

    // Strategy 3: Query remote directly (works for any git host)
    if let Some(branch) = get_remote_default_branch().await {
        if branch_has_merge_base(&branch).await {
            if verbose {
                eprintln!("— Base branch detection: git ls-remote (queried remote)");
            }
            return Ok(branch);
        }
        let origin_branch = format!("origin/{}", branch);
        if branch_has_merge_base(&origin_branch).await {
            if verbose {
                eprintln!("— Base branch detection: git ls-remote (queried remote, using origin/)");
            }
            return Ok(origin_branch);
        }
    }

    // Strategy 4: Last resort - check common default branch names
    let common_branches = [
        "origin/main", "origin/master", "origin/mainline", "origin/develop",
        "main", "master", "mainline", "develop",
    ];

    for branch in common_branches {
        if branch_has_merge_base(branch).await {
            if verbose {
                eprintln!("— Base branch detection: fallback (checked common names)");
            }
            return Ok(branch.to_string());
        }
    }

    Err("Could not determine default base branch. Use --base <branch> to specify manually.".into())
}

/// Creates a pull request via GitHub CLI.
///
/// Returns the PR URL on success.
pub async fn create_pr(title: &str, body: &str, draft: bool) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["pr", "create", "--title", title, "--body", body];
    if draft {
        args.push("--draft");
    }

    let output = Command::new("gh").args(&args).output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("auth") {
            return Err(format!(
                "GitHub authentication failed.\nRun: gh auth login\n\nError: {}",
                stderr
            )
            .into());
        }
        return Err(format!("Failed to create PR: {}", stderr).into());
    }

    // gh pr create outputs the PR URL on success
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(url)
}

/// Main handler for the `committer pr` subcommand.
///
/// Orchestrates the full PR creation workflow.
pub async fn handle_pr_command(args: PrArgs, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    // Check gh CLI is installed
    check_gh_installed().await?;

    // Get API key
    let api_key = match get_api_key() {
        Some(key) => key,
        None => {
            println!("— No API key found");
            println!("  Set OPENROUTER_API_KEY environment variable");
            std::process::exit(1);
        }
    };

    let verbose = args.verbose || config.verbose;
    let model = args.model.as_ref().unwrap_or(&config.model);

    // Get current branch
    let current_branch = get_current_branch().await?;

    // Check if on protected branch
    if PROTECTED_BRANCHES.contains(&current_branch.as_str()) {
        // Check for upstream remote (fork workflow)
        if get_upstream_remote().await?.is_none() {
            println!("— Cannot create PR from protected branch '{}'", current_branch);
            println!("  Create a feature branch first: git checkout -b feat/your-feature");
            std::process::exit(1);
        }
    }

    // Determine base branch
    let base_branch = match &args.base {
        Some(base) => base.clone(),
        None => get_default_base_branch(verbose).await?,
    };

    if verbose {
        eprintln!("— Base branch: {}", base_branch);
        eprintln!("— Current branch: {}", current_branch);
    }

    // Check for uncommitted changes
    let uncommitted = get_uncommitted_changes().await?;
    if !uncommitted.staged.is_empty() || !uncommitted.unstaged.is_empty() {
        match prompt_uncommitted_changes(&uncommitted) {
            UncommittedAction::Commit => {
                // Stage all and run commit flow
                stage_all_changes().await?;

                let commit_diff = get_git_diff(true, verbose).await?;
                let commit_files = get_staged_files(verbose).await?;

                if commit_diff.trim().is_empty() {
                    println!("— No changes to commit");
                } else {
                    let client = Client::builder().build()?;

                    let spinner = ProgressBar::new_spinner();
                    spinner.set_style(
                        ProgressStyle::default_spinner()
                            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                            .template("Generating commit message {spinner}")
                            .unwrap(),
                    );
                    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

                    let commit_msg = stream_commit_message(
                        &client,
                        &api_key,
                        model,
                        &commit_diff,
                        &commit_files,
                        &spinner,
                        verbose,
                    )
                    .await?;

                    if !commit_msg.is_empty() {
                        match prompt_commit(&commit_msg, false) {
                            CommitAction::Commit(msg) => {
                                run_git_commit(&msg).await?;
                                println!("— Committed");
                                println!();
                            }
                            CommitAction::Cancel => {
                                println!("— Commit cancelled, continuing with PR...");
                                println!();
                            }
                            _ => {}
                        }
                    }
                }
            }
            UncommittedAction::Skip => {
                println!("— Skipping uncommitted changes");
                println!();
            }
            UncommittedAction::Quit => {
                println!("— Cancelled");
                std::process::exit(0);
            }
        }
    }

    // Get commits on this branch
    let commits = get_branch_commits(&base_branch).await?;
    if commits.is_empty() {
        println!("— No commits found between '{}' and '{}'", base_branch, current_branch);
        println!("  Make some commits first, or check your base branch");
        std::process::exit(1);
    }

    if verbose {
        eprintln!("— Found {} commits on branch", commits.len());
    }

    // Get diff and file list
    let (diff_result, files_result) = tokio::join!(
        get_branch_diff(&base_branch, verbose),
        get_pr_changed_files(&base_branch, verbose)
    );

    let diff = diff_result?;
    let files = files_result?;

    if diff.trim().is_empty() {
        println!("— No changes found between '{}' and '{}'", base_branch, current_branch);
        std::process::exit(1);
    }

    // Create HTTP client
    let client = Client::builder().build()?;

    // Stream PR content with spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("Generating PR content {spinner}")
            .unwrap(),
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let (title, body) = stream_pr_content(
        &client,
        &api_key,
        model,
        &diff,
        &files,
        &commits,
        &spinner,
        verbose,
    )
    .await?;

    if args.dry_run {
        println!();
        println!("— Dry run complete (PR not created)");
        return Ok(());
    }

    if args.yes {
        // Push branch if needed
        push_branch_with_spinner(&current_branch).await?;

        let url = create_pr(&title, &body, args.draft).await?;
        println!("— PR created: {}", url);
    } else {
        match prompt_pr(&title, &body) {
            PrAction::Create(final_title, final_body) => {
                // Push branch if needed
                push_branch_with_spinner(&current_branch).await?;

                let url = create_pr(&final_title, &final_body, args.draft).await?;
                println!("— PR created: {}", url);
            }
            PrAction::Cancel => {
                println!("— Cancelled");
            }
        }
    }

    Ok(())
}
