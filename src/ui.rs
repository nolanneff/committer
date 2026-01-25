use dialoguer::Input;
use std::io::{self, Write};

use crate::branch::BranchAction;
use crate::git::UncommittedChanges;

pub enum UncommittedAction {
    Commit,
    Skip,
    Quit,
}

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

pub enum CommitAction {
    Commit(String),
    Cancel,
    CreateBranch(String),
}

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

pub enum PrAction {
    Create(String, String), // (title, body)
    Cancel,
}

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
