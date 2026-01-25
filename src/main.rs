//! Committer - AI-powered git commit message generator.
//!
//! Committer uses LLMs via OpenRouter to generate conventional commit messages
//! from staged changes, with features like:
//!
//! - **Streaming output**: Watch messages generate token-by-token
//! - **Branch intelligence**: Detects misaligned branches and suggests alternatives
//! - **Interactive editing**: Edit messages before committing
//! - **Smart diff handling**: Filters noise and truncates large diffs
//! - **PR generation**: Create pull requests with AI-generated descriptions
//!
//! # Modules
//!
//! - [`api`]: OpenRouter API integration
//! - [`branch`]: Branch analysis and naming
//! - [`cli`]: Command-line interface
//! - [`config`]: Configuration management
//! - [`git`]: Git operations
//! - [`pr`]: Pull request generation
//! - [`ui`]: User interaction prompts
//!
//! # Quick Start
//!
//! ```bash
//! export OPENROUTER_API_KEY="sk-or-..."
//! git add .
//! committer
//! ```

use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use std::io::Write;
use tokio::process::Command;

mod api;
mod branch;
mod cli;
mod config;
mod git;
mod pr;
mod ui;

use api::stream_commit_message;
use branch::{
    analyze_branch_alignment, generate_branch_suggestion, generate_fallback_branch,
    BranchAction,
};
use cli::{Cli, Commands, ConfigAction};
use config::{config_path, get_api_key, load_config, save_config};
use git::{
    create_and_switch_branch, get_current_branch, get_git_diff,
    get_recent_commits, get_staged_files, run_git_commit, stage_all_changes,
};
use pr::handle_pr_command;
use ui::{prompt_branch_action, prompt_commit, CommitAction};

// ============================================================================
// Main
// ============================================================================

#[tokio::main(flavor = "current_thread")]
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
