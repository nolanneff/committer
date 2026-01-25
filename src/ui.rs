//! User interaction and prompts.
//!
//! This module provides interactive prompts for user decisions during
//! commit and PR workflows. All prompts support:
//!
//! - Single-key responses (y/n/e)
//! - Full word responses (yes/no/edit)
//! - Editor integration for message editing
//!
//! # Prompts
//!
//! - [`prompt_commit`]: Confirm or edit commit message
//! - [`prompt_pr`]: Confirm or edit PR title/body
//! - [`prompt_branch_action`]: Create or skip branch creation
//! - [`prompt_uncommitted_changes`]: Handle uncommitted changes before PR

use console::style;
use dialoguer::Input;
use std::io::{self, Write};

use crate::branch::BranchAction;
use crate::git::UncommittedChanges;

/// User's choice when uncommitted changes are detected.
pub enum UncommittedAction {
    Commit,
    Skip,
    Quit,
}

/// Prompts user to handle uncommitted changes before creating a PR.
///
/// Displays staged and unstaged files, then asks user to commit, skip, or quit.
pub fn prompt_uncommitted_changes(changes: &UncommittedChanges) -> UncommittedAction {
    println!();
    println!("{} Uncommitted changes won't be included in this PR", style("⚠").yellow());
    println!();

    if !changes.staged.is_empty() {
        println!("{}:", style("Staged").green());
        for file in &changes.staged {
            println!("  {}", file);
        }
        println!();
    }

    if !changes.unstaged.is_empty() {
        println!("{}:", style("Unstaged").yellow());
        for file in &changes.unstaged {
            println!("  {}", file);
        }
        println!();
    }

    println!("  {} Commit changes first", style("[c]").cyan().bold());
    println!("  {} Skip and continue", style("[s]").cyan().bold());
    println!("  {} Quit", style("[q]").cyan().bold());
    println!();

    loop {
        print!("{} ", style("Choice:").bold());
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();

        match input.trim().to_lowercase().as_str() {
            "c" | "commit" => return UncommittedAction::Commit,
            "s" | "skip" => return UncommittedAction::Skip,
            "q" | "quit" => return UncommittedAction::Quit,
            _ => println!("  {} Please enter c, s, or q", style("→").dim()),
        }
    }
}

/// Prompts user to create a new branch or continue on current.
///
/// Options: `y` (create), `n` (skip), `e` (edit name then create).
pub fn prompt_branch_action(
    current: &str,
    suggested: &str,
    reason: &str,
    show_mismatch_header: bool,
) -> BranchAction {
    if show_mismatch_header {
        println!();
        println!("{} Branch mismatch detected", style("⚠").yellow());
        println!("  Current:   {}", style(current).dim());
        println!("  Suggested: {}", style(suggested).green());
        if !reason.is_empty() {
            println!("  Reason:    {}", style(reason).dim());
        }
        println!();
    }

    let mut current_suggestion = suggested.to_string();

    println!("  {} Create branch '{}'", style("[y]").cyan().bold(), style(&current_suggestion).green());
    println!("  {} Stay on '{}'", style("[n]").cyan().bold(), style(current).dim());
    println!("  {} Edit branch name", style("[e]").cyan().bold());
    println!();

    loop {
        print!("{} ", style("Choice:").bold());
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
                current_suggestion = edited.clone();
                // Reprint menu with updated branch name
                println!();
                println!("  {} Create branch '{}'", style("[y]").cyan().bold(), style(&current_suggestion).green());
                println!("  {} Stay on '{}'", style("[n]").cyan().bold(), style(current).dim());
                println!("  {} Edit branch name", style("[e]").cyan().bold());
                println!();
            }
            _ => println!("  {} Please enter y, n, or e", style("→").dim()),
        }
    }
}

/// User's choice after reviewing a commit message.
pub enum CommitAction {
    /// Proceed with commit using the (possibly edited) message.
    Commit(String),
    /// Cancel the commit.
    Cancel,
    /// Create a new branch first, then prompt again.
    CreateBranch(String),
}

/// Prompts user to confirm, edit, or cancel a commit.
///
/// Options: `y` (commit), `n` (cancel), `e` (edit in $EDITOR), `b` (create branch first).
pub fn prompt_commit(message: &str, show_branch_option: bool) -> CommitAction {
    let mut current_message = message.to_string();

    let print_menu = |show_branch: bool| {
        println!();
        println!("  {} Commit", style("[y]").cyan().bold());
        println!("  {} Cancel", style("[n]").cyan().bold());
        println!("  {} Edit in $EDITOR", style("[e]").cyan().bold());
        if show_branch {
            println!("  {} Create branch first", style("[b]").cyan().bold());
        }
        println!();
    };

    let invalid_msg = if show_branch_option {
        "Please enter y, n, e, or b"
    } else {
        "Please enter y, n, or e"
    };

    print_menu(show_branch_option);

    loop {
        print!("{} ", style("Choice:").bold());
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
                print_menu(show_branch_option);
            }
            "b" | "branch" if show_branch_option => {
                return CommitAction::CreateBranch(current_message)
            }
            _ => println!("  {} {}", style("→").dim(), invalid_msg),
        }
    }
}

/// User's choice after reviewing PR content.
pub enum PrAction {
    /// Create the PR with (title, body).
    Create(String, String),
    /// Cancel PR creation.
    Cancel,
}

/// Prompts user to confirm, edit, or cancel PR creation.
///
/// Options: `y` (create), `n` (cancel), `e` (edit in $EDITOR).
pub fn prompt_pr(title: &str, body: &str) -> PrAction {
    let mut current_title = title.to_string();
    let mut current_body = body.to_string();

    let print_menu = || {
        println!();
        println!("  {} Create PR", style("[y]").cyan().bold());
        println!("  {} Cancel", style("[n]").cyan().bold());
        println!("  {} Edit in $EDITOR", style("[e]").cyan().bold());
        println!();
    };

    print_menu();

    loop {
        print!("{} ", style("Choice:").bold());
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

                // Print updated preview
                println!();
                println!("{}", current_title);
                println!();
                println!("{}", current_body);
                print_menu();
            }
            _ => println!("  {} Please enter y, n, or e", style("→").dim()),
        }
    }
}
