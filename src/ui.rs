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
