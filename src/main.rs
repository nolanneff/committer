use clap::Parser;
use dialoguer::Input;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::io::{self, Write};
use tokio::process::Command;

mod api;
mod branch;
mod cli;
mod config;
mod git;

use api::{stream_commit_message, stream_pr_content};
use branch::{
    analyze_branch_alignment, generate_branch_suggestion, generate_fallback_branch,
    BranchAction, PROTECTED_BRANCHES,
};
use cli::{Cli, Commands, ConfigAction, PrArgs};
use config::{config_path, get_api_key, load_config, save_config, Config};
use git::{
    branch_has_merge_base, create_and_switch_branch, get_branch_commits, get_branch_diff,
    get_cached_remote_head, get_current_branch, get_git_diff, get_pr_changed_files,
    get_recent_commits, get_remote_default_branch, get_staged_files, get_uncommitted_changes,
    get_upstream_remote, push_branch_with_spinner, run_git_commit, stage_all_changes,
    UncommittedChanges,
};

// ============================================================================
// GitHub CLI Operations
// ============================================================================

async fn check_gh_installed() -> Result<(), Box<dyn std::error::Error>> {
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

/// Check if a branch exists and has a merge base with HEAD
async fn get_default_base_branch(verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
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

enum UncommittedAction {
    Commit,
    Skip,
    Quit,
}

fn prompt_uncommitted_changes(changes: &UncommittedChanges) -> UncommittedAction {
    println!();
    println!("⚠ Uncommitted changes won't be included in this PR");
    println!();

    if !changes.staged.is_empty() {
        println!("Staged:");
        for file in &changes.staged {
            println!("{}", file);
        }
        println!();
    }

    if !changes.unstaged.is_empty() {
        println!("Unstaged:");
        for file in &changes.unstaged {
            println!("{}", file);
        }
        println!();
    }

    loop {
        print!("[c]ommit first  [s]kip  [q]uit: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        match input.trim().to_lowercase().as_str() {
            "c" | "commit" => return UncommittedAction::Commit,
            "s" | "skip" => return UncommittedAction::Skip,
            "q" | "quit" => return UncommittedAction::Quit,
            _ => println!("Please enter c, s, or q"),
        }
    }
}


// ============================================================================
fn prompt_branch_action(
    current: &str,
    suggested: &str,
    reason: &str,
    show_mismatch_header: bool,
) -> BranchAction {
    if show_mismatch_header {
        println!();
        println!("⚠ Branch mismatch detected");
        println!("  Current: {}", current);
        println!("  Suggested: {}", suggested);
        println!("  Reason: {}", reason);
        println!();
    }

    let mut current_suggestion = suggested.to_string();

    loop {
        print!("Create branch? [y/n/e] ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => return BranchAction::Create(current_suggestion),
            "n" | "no" => return BranchAction::Skip,
            "e" | "edit" => {
                let edited: String = Input::new()
                    .with_prompt("Branch name")
                    .default(current_suggestion.clone())
                    .interact_text()
                    .unwrap();
                current_suggestion = edited;
                println!("  Branch: {}", current_suggestion);
            }
            _ => println!("Please enter y, n, or e"),
        }
    }
}

// ============================================================================
// User Interaction
// ============================================================================

enum CommitAction {
    Commit(String),
    Cancel,
    CreateBranch(String),
}

fn prompt_commit(message: &str, show_branch_option: bool) -> CommitAction {
    let mut current_message = message.to_string();

    let prompt_text = if show_branch_option {
        "Commit? [y/n/e/b] "
    } else {
        "Commit? [y/n/e] "
    };

    let invalid_msg = if show_branch_option {
        "Please enter y, n, e, or b"
    } else {
        "Please enter y, n, or e"
    };

    loop {
        print!("{}", prompt_text);
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => return CommitAction::Commit(current_message),
            "n" | "no" => return CommitAction::Cancel,
            "e" | "edit" => {
                let edited: String = dialoguer::Editor::new()
                    .extension(".txt")
                    .edit(&current_message)
                    .unwrap_or(None)
                    .unwrap_or_else(|| current_message.clone());
                current_message = edited;
                println!();
                println!("{}", current_message);
            }
            "b" | "branch" if show_branch_option => {
                return CommitAction::CreateBranch(current_message)
            }
            _ => println!("{}", invalid_msg),
        }
    }
}

enum PrAction {
    Create(String, String), // (title, body)
    Cancel,
}

fn prompt_pr(title: &str, body: &str) -> PrAction {
    let mut current_title = title.to_string();
    let mut current_body = body.to_string();

    // Calculate initial preview lines (title + blank + body + prompt line we're about to print)
    let initial_preview = format!("{}\n\n{}", title, body);
    let mut prev_lines: usize = initial_preview.lines().count() + 1; // +1 for prompt

    loop {
        print!("Create PR? [y/n/e] ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => return PrAction::Create(current_title, current_body),
            "n" | "no" => return PrAction::Cancel,
            "e" | "edit" => {
                let combined = format!("{}\n\n{}", current_title, current_body);
                let edited: String = dialoguer::Editor::new()
                    .extension(".md")
                    .edit(&combined)
                    .unwrap_or(None)
                    .unwrap_or_else(|| combined.clone());

                // Parse edited content back into title and body
                let mut lines = edited.lines();
                current_title = lines.next().unwrap_or("").trim().to_string();
                lines.next(); // Skip blank line
                current_body = lines.collect::<Vec<_>>().join("\n").trim().to_string();

                // Clear previous preview: move up and clear each line
                // +1 for the "Create PR?" prompt line, +1 for user input line
                for _ in 0..(prev_lines + 2) {
                    print!("\x1B[A\x1B[2K");
                }
                io::stdout().flush().unwrap();

                // Print new preview and count lines
                let preview = format!("{}\n\n{}\n", current_title, current_body);
                print!("{}", preview);
                io::stdout().flush().unwrap();
                prev_lines = preview.lines().count();
            }
            _ => println!("Please enter y, n, or e"),
        }
    }
}

async fn create_pr(title: &str, body: &str, draft: bool) -> Result<String, Box<dyn std::error::Error>> {
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

// ============================================================================
// PR Command Handler
// ============================================================================

async fn handle_pr_command(args: PrArgs, config: &Config) -> Result<(), Box<dyn std::error::Error>> {
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

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut config = load_config();

    // Handle subcommands
    if let Some(command) = cli.command {
        match command {
            Commands::Config { action } => {
                match action {
                    ConfigAction::Show => {
                        println!("Config file: {}", config_path().display());
                        println!("auto_commit: {}", config.auto_commit);
                        println!("commit_after_branch: {}", config.commit_after_branch);
                        println!("model: {}", config.model);
                        println!("verbose: {}", config.verbose);
                        println!(
                            "api_key: {}",
                            if std::env::var("OPENROUTER_API_KEY").is_ok() {
                                "[set via OPENROUTER_API_KEY env var]"
                            } else {
                                "[not set]"
                            }
                        );
                    }
                    ConfigAction::AutoCommit { value } => {
                        config.auto_commit = value.parse().unwrap_or(false);
                        save_config(&config)?;
                        println!("auto_commit set to: {}", config.auto_commit);
                    }
                    ConfigAction::CommitAfterBranch { value } => {
                        config.commit_after_branch = value.parse().unwrap_or(false);
                        save_config(&config)?;
                        println!("commit_after_branch set to: {}", config.commit_after_branch);
                    }
                    ConfigAction::Model { value } => {
                        config.model = value;
                        save_config(&config)?;
                        println!("model set to: {}", config.model);
                    }
                    ConfigAction::Verbose { value } => {
                        config.verbose = value.parse().unwrap_or(false);
                        save_config(&config)?;
                        println!("verbose set to: {}", config.verbose);
                    }
                }
                return Ok(());
            }
            Commands::Pr(args) => {
                return handle_pr_command(args, &config).await;
            }
        }
    }

    // Get API key
    let api_key = match get_api_key() {
        Some(key) => key,
        None => {
            println!("— No API key found");
            println!("  Set OPENROUTER_API_KEY environment variable");
            std::process::exit(1);
        }
    };

    // Stage all changes if requested
    if cli.all {
        stage_all_changes().await?;
    }

    // Determine verbose mode (CLI flag overrides config)
    let verbose = cli.verbose || config.verbose;

    // Get diff and file list in parallel
    let (diff_result, files_result) =
        tokio::join!(get_git_diff(true, verbose), get_staged_files(verbose));

    let diff = diff_result?;
    let files = files_result?;

    if diff.trim().is_empty() {
        // Check if there are any unstaged or untracked changes
        let status_output = Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .await?;

        let status = String::from_utf8_lossy(&status_output.stdout);

        if status.trim().is_empty() {
            println!("— Nothing to commit");
            std::process::exit(0);
        } else {
            println!("— No staged changes");
            println!("  Use 'git add' or --all");
            std::process::exit(1);
        }
    }

    // Determine which model to use
    let model = cli.model.as_ref().unwrap_or(&config.model);

    // Create HTTP client
    let client = Client::builder().build()?;

    // Stream the commit message with spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("Generating commit message {spinner}")
            .unwrap(),
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    // Ensure spinner renders before starting API call
    std::io::stdout().flush().ok();

    let message_result =
        stream_commit_message(&client, &api_key, model, &diff, &files, &spinner, verbose).await;

    let message = message_result?;

    if message.is_empty() {
        spinner.finish_and_clear();
        println!("— Error: empty commit message generated");
        std::process::exit(1);
    }

    // Track if branch was already handled via --branch or --auto-branch flags
    let mut branch_already_handled = false;

    if cli.branch || cli.auto_branch {
        let current_branch = get_current_branch().await?;
        let recent_commits = get_recent_commits(5).await.unwrap_or_default();

        let branch_spinner = ProgressBar::new_spinner();
        branch_spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("Analyzing branch alignment {spinner}")
                .unwrap(),
        );
        branch_spinner.enable_steady_tick(std::time::Duration::from_millis(120));

        let analysis = analyze_branch_alignment(
            &client,
            &api_key,
            model,
            &current_branch,
            &message,
            &files,
            &recent_commits,
        )
        .await?;

        branch_spinner.finish_and_clear();

        if verbose {
            eprintln!("[Branch Analysis]: {}\n", analysis.reason);
        }

        if !analysis.matches {
            let suggested = analysis
                .suggested_branch
                .unwrap_or_else(|| generate_fallback_branch(&message));

            if cli.auto_branch || cli.yes {
                println!(
                    "— Branch '{}' → '{}' ({})",
                    current_branch, suggested, analysis.reason
                );
                create_and_switch_branch(&suggested).await?;
                branch_already_handled = true;
            } else {
                match prompt_branch_action(&current_branch, &suggested, &analysis.reason, true) {
                    BranchAction::Create(name) => {
                        create_and_switch_branch(&name).await?;
                        println!("— Switched to branch '{}'", name);
                        branch_already_handled = true;
                    }
                    BranchAction::Skip => {
                        println!("— Continuing on '{}'", current_branch);
                        branch_already_handled = true;
                    }
                }
            }
        }
    }

    if cli.dry_run {
        return Ok(());
    }

    if cli.yes || config.auto_commit {
        run_git_commit(&message).await?;
        println!("— Committed");
    } else {
        let mut show_branch_option = !branch_already_handled;
        let mut current_message = message.clone();

        loop {
            match prompt_commit(&current_message, show_branch_option) {
                CommitAction::Commit(final_message) => {
                    run_git_commit(&final_message).await?;
                    println!("— Committed");
                    break;
                }
                CommitAction::Cancel => {
                    println!("— Cancelled");
                    break;
                }
                CommitAction::CreateBranch(msg) => {
                    current_message = msg;

                    let branch_spinner = ProgressBar::new_spinner();
                    branch_spinner.set_style(
                        ProgressStyle::default_spinner()
                            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                            .template("Generating branch name {spinner}")
                            .unwrap(),
                    );
                    branch_spinner.enable_steady_tick(std::time::Duration::from_millis(120));

                    let suggested = match generate_branch_suggestion(
                        &client,
                        &api_key,
                        model,
                        &current_message,
                    )
                    .await
                    {
                        Ok(name) => name,
                        Err(_) => generate_fallback_branch(&current_message),
                    };

                    branch_spinner.finish_and_clear();

                    let current_branch = get_current_branch().await.unwrap_or_default();
                    println!("🌿 Suggested branch: {}", suggested);
                    println!();

                    let branch_created =
                        match prompt_branch_action(&current_branch, &suggested, "", false) {
                            BranchAction::Create(name) => {
                                create_and_switch_branch(&name).await?;
                                println!("— Switched to branch '{}'", name);
                                true
                            }
                            BranchAction::Skip => {
                                println!("— Continuing on '{}'", current_branch);
                                false
                            }
                        };

                    // Auto-commit if config enabled and branch was created
                    if config.commit_after_branch && branch_created {
                        run_git_commit(&current_message).await?;
                        println!("— Committed");
                        break;
                    }

                    println!();
                    println!("{}", current_message);

                    // Disable branch option for next iteration
                    show_branch_option = false;
                }
            }
        }
    }

    Ok(())
}
