//! Git operations and diff processing.
//!
//! This module handles all interactions with git, including:
//!
//! - **Diff retrieval**: [`get_git_diff`], [`get_branch_diff`]
//! - **Diff filtering**: Excludes lock files, minified code, build artifacts
//! - **Diff truncation**: Limits size to stay within LLM token limits
//! - **Status queries**: [`get_staged_files`], [`get_uncommitted_changes`]
//! - **Branch operations**: [`get_current_branch`], [`create_and_switch_branch`]
//! - **Commit operations**: [`run_git_commit`], [`stage_all_changes`]
//! - **Push operations**: [`push_branch_with_spinner`]
//!
//! # Diff Filtering
//!
//! Files matching [`EXCLUDED_FROM_DIFF`] patterns are automatically removed
//! from diffs to reduce noise and token usage. This includes lock files,
//! minified code, and build directories.
//!
//! # Size Limits
//!
//! Diffs are truncated at [`MAX_DIFF_CHARS`] (300KB) to stay within LLM
//! context limits while preserving file headers for context.

use console::{style, Term};
use indicatif::{ProgressBar, ProgressStyle};
use tokio::process::Command;

/// File patterns excluded from diffs to reduce noise.
pub const EXCLUDED_FROM_DIFF: &[&str] = &[
    // Lock files
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "composer.lock",
    "Gemfile.lock",
    "poetry.lock",
    "bun.lockb",
    "uv.lock",
    // Minified/generated
    ".min.js",
    ".min.css",
    ".map",
    // Build directories (safety net if staged)
    "target/",
    "node_modules/",
    "dist/",
    "build/",
    ".next/",
    "__pycache__/",
];

/// Maximum diff size in characters before truncation.
///
/// Set to 300KB to stay within typical LLM context limits while leaving
/// room for the prompt and response.
pub const MAX_DIFF_CHARS: usize = 300_000;

/// Checks if a file should be excluded from the diff based on [`EXCLUDED_FROM_DIFF`] patterns.
pub fn should_exclude_from_diff(filename: &str) -> bool {
    EXCLUDED_FROM_DIFF.iter().any(|pattern| {
        if pattern.ends_with('/') {
            // Directory pattern - check if file is inside this directory
            let dir_name = pattern.trim_end_matches('/');
            filename.contains(&format!("/{}/", dir_name))
                || filename.starts_with(&format!("{}/", dir_name))
        } else if pattern.starts_with('.') {
            // Extension pattern
            filename.ends_with(pattern)
        } else {
            // Exact filename match
            filename.ends_with(pattern) || filename.ends_with(&format!("/{}", pattern))
        }
    })
}

fn extract_filename_from_diff_header(header: &str) -> Option<&str> {
    header
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("diff --git a/"))
        .and_then(|rest| rest.split(" b/").next())
}

/// Removes excluded files from a diff based on [`EXCLUDED_FROM_DIFF`] patterns.
///
/// In verbose mode, prints excluded files to stderr.
pub fn filter_excluded_diffs(diff: &str, verbose: bool) -> String {
    if diff.is_empty() {
        return diff.to_string();
    }

    let mut chunks: Vec<&str> = diff.split("\ndiff --git ").collect();
    if chunks.is_empty() {
        return diff.to_string();
    }

    let first = chunks.remove(0);
    let mut file_diffs: Vec<String> = vec![];
    let mut excluded_files: Vec<String> = vec![];

    if !first.is_empty() {
        if let Some(filename) = extract_filename_from_diff_header(&format!("diff --git {}", first))
        {
            if should_exclude_from_diff(filename) {
                excluded_files.push(filename.to_string());
            } else {
                file_diffs.push(first.to_string());
            }
        } else {
            file_diffs.push(first.to_string());
        }
    }

    for chunk in chunks {
        let full_header = format!("diff --git {}", chunk);
        if let Some(filename) = extract_filename_from_diff_header(&full_header) {
            if should_exclude_from_diff(filename) {
                excluded_files.push(filename.to_string());
            } else {
                file_diffs.push(format!("\ndiff --git {}", chunk));
            }
        }
    }

    if verbose && !excluded_files.is_empty() {
        eprintln!("— Excluded from diff ({} files):", excluded_files.len());
        for file in &excluded_files {
            eprintln!("    {}", file);
        }
    }

    file_diffs.join("")
}

/// Truncates a diff to fit within token limits while preserving useful context.
/// Keeps the beginning (file headers, context) and end (recent changes).
pub fn truncate_diff(diff: &str, verbose: bool) -> String {
    if diff.len() <= MAX_DIFF_CHARS {
        return diff.to_string();
    }

    // Split into file chunks to truncate more intelligently
    let mut chunks: Vec<&str> = diff.split("\ndiff --git ").collect();

    if chunks.is_empty() {
        // Fallback: simple truncation with middle cut
        let keep_each = MAX_DIFF_CHARS / 2;
        let start = &diff[..keep_each];
        let end = &diff[diff.len() - keep_each..];
        if verbose {
            eprintln!(
                "— Diff truncated: {} chars removed (fallback mode)",
                diff.len() - MAX_DIFF_CHARS
            );
        }
        return format!(
            "{}\n\n[... {} characters truncated ...]\n\n{}",
            start,
            diff.len() - MAX_DIFF_CHARS,
            end
        );
    }

    // Reconstruct with "diff --git " prefix for all but first chunk
    let first = chunks.remove(0);
    let mut file_diffs: Vec<String> = vec![first.to_string()];
    for chunk in chunks {
        file_diffs.push(format!("diff --git {}", chunk));
    }

    // Try to fit as many complete file diffs as possible
    let mut result = String::new();
    let mut total_len = 0;
    let mut included = 0;

    for file_diff in &file_diffs {
        let chunk_len = file_diff.len();
        // Reserve space for truncation notice
        if total_len + chunk_len + 200 > MAX_DIFF_CHARS {
            break;
        }
        result.push_str(file_diff);
        result.push('\n');
        total_len += chunk_len + 1;
        included += 1;
    }

    if included < file_diffs.len() {
        if verbose {
            eprintln!(
                "— Diff truncated: showing {}/{} files ({} KB limit)",
                included,
                file_diffs.len(),
                MAX_DIFF_CHARS / 1024
            );
        }
        result.push_str(&format!(
            "\n[... diff truncated: showing {}/{} files to fit context limit ...]\n",
            included,
            file_diffs.len()
        ));
    }

    result
}

/// Retrieves the git diff, filtered and truncated for LLM consumption.
///
/// Applies [`filter_excluded_diffs`] and [`truncate_diff`] automatically.
pub async fn get_git_diff(
    staged_only: bool,
    verbose: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let args = if staged_only {
        vec!["diff", "--staged"]
    } else {
        vec!["diff", "HEAD"]
    };

    let output = Command::new("git").args(&args).output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr).into());
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    let filtered_diff = filter_excluded_diffs(&diff, verbose);
    Ok(truncate_diff(&filtered_diff, verbose))
}

/// Returns a list of staged files with their status (M/A/D).
///
/// Excluded files are annotated with `[excluded from diff]`.
pub async fn get_staged_files(verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["diff", "--staged", "--name-status"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff --name-status failed: {}", stderr).into());
    }

    let raw_output = String::from_utf8_lossy(&output.stdout).to_string();
    let mut excluded_count = 0;

    let annotated: Vec<String> = raw_output
        .lines()
        .map(|line| {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                let filename = parts[1];
                if should_exclude_from_diff(filename) {
                    excluded_count += 1;
                    format!("{}\t{} [excluded from diff]", parts[0], filename)
                } else {
                    line.to_string()
                }
            } else {
                line.to_string()
            }
        })
        .collect();

    if verbose {
        let total = annotated.len();
        eprintln!(
            "— Staged files: {} total, {} excluded from diff",
            total, excluded_count
        );
    }

    Ok(annotated.join("\n"))
}

/// Creates a git commit with the given message.
pub async fn run_git_commit(message: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git commit failed: {}", stderr).into());
    }

    Ok(())
}

/// Stages all changes (tracked and untracked) via `git add -A`.
pub async fn stage_all_changes() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git").args(["add", "-A"]).output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git add failed: {}", stderr).into());
    }

    Ok(())
}

/// Returns the name of the current git branch.
pub async fn get_current_branch() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git rev-parse failed: {}", stderr).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Creates a new branch and switches to it.
pub async fn create_and_switch_branch(branch_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["checkout", "-b", branch_name])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git checkout -b failed: {}", stderr).into());
    }

    Ok(())
}

/// Returns the subject lines of recent commits (for branch analysis context).
pub async fn get_recent_commits(limit: usize) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["log", "--oneline", &format!("-{}", limit), "--format=%s"])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(String::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Check if a branch exists and has a merge base with HEAD
pub async fn branch_has_merge_base(branch: &str) -> bool {
    let output = Command::new("git")
        .args(["merge-base", branch, "HEAD"])
        .output()
        .await;

    matches!(output, Ok(o) if o.status.success())
}

/// Try to get the default branch from the cached remote HEAD reference
pub async fn get_cached_remote_head() -> Option<String> {
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let full_ref = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // refs/remotes/origin/main -> main
    full_ref
        .strip_prefix("refs/remotes/origin/")
        .map(|s| s.to_string())
}

/// Query the remote directly for its default branch (works with any git remote)
pub async fn get_remote_default_branch() -> Option<String> {
    let output = Command::new("git")
        .args(["ls-remote", "--symref", "origin", "HEAD"])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // Output format: "ref: refs/heads/main\tHEAD\n..."
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.starts_with("ref: refs/heads/") && line.contains("HEAD") {
            // Extract branch name between "ref: refs/heads/" and the tab
            if let Some(rest) = line.strip_prefix("ref: refs/heads/") {
                if let Some(branch) = rest.split('\t').next() {
                    return Some(branch.to_string());
                }
            }
        }
    }
    None
}

/// Checks if an 'upstream' remote exists (for fork workflows).
pub async fn get_upstream_remote() -> Result<Option<String>, Box<dyn std::error::Error>> {
    // Check if 'upstream' remote exists (common fork workflow)
    let output = Command::new("git")
        .args(["remote", "get-url", "upstream"])
        .output()
        .await?;

    if output.status.success() {
        return Ok(Some("upstream".to_string()));
    }
    Ok(None)
}

/// Returns true if the branch needs to be pushed to origin.
pub async fn branch_needs_push(branch: &str) -> bool {
    // Check if branch has upstream tracking
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", &format!("{}@{{u}}", branch)])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            // Has upstream, check if we're ahead
            let status = Command::new("git").args(["status", "-sb"]).output().await;
            if let Ok(s) = status {
                let out = String::from_utf8_lossy(&s.stdout);
                out.contains("ahead")
            } else {
                false
            }
        }
        _ => true, // No upstream, needs push
    }
}

/// Uncommitted changes in the working directory.
pub struct UncommittedChanges {
    /// Files staged for commit.
    pub staged: Vec<String>,
    /// Modified or untracked files not yet staged.
    pub unstaged: Vec<String>,
}

/// Returns lists of staged and unstaged changes.
pub async fn get_uncommitted_changes() -> Result<UncommittedChanges, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .await?;

    if !output.status.success() {
        return Err("Failed to get git status".into());
    }

    let status = String::from_utf8_lossy(&output.stdout);
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();

    for line in status.lines() {
        if line.len() < 3 {
            continue;
        }
        let index_status = line.chars().next().unwrap_or(' ');
        let worktree_status = line.chars().nth(1).unwrap_or(' ');
        let file = &line[3..];

        // Staged changes (index has modifications)
        if index_status != ' ' && index_status != '?' {
            staged.push(format!("  {} {}", index_status, file));
        }
        // Unstaged changes (worktree has modifications) or untracked
        if worktree_status != ' ' {
            let status_char = if worktree_status == '?' {
                '?'
            } else {
                worktree_status
            };
            unstaged.push(format!("  {} {}", status_char, file));
        }
    }

    Ok(UncommittedChanges { staged, unstaged })
}

/// Pushes the branch to origin with a progress spinner.
///
/// Skips if branch is already up-to-date with upstream.
pub async fn push_branch_with_spinner(branch: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !branch_needs_push(branch).await {
        return Ok(());
    }

    let term = Term::stdout();
    let _ = term.hide_cursor();

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.cyan} Pushing branch to origin...")
            .unwrap(),
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let push_output = Command::new("git")
        .args(["push", "-u", "origin", branch])
        .output()
        .await?;

    spinner.finish_and_clear();
    let _ = term.show_cursor();

    if !push_output.status.success() {
        let stderr = String::from_utf8_lossy(&push_output.stderr);
        return Err(format!("Failed to push branch: {}", stderr).into());
    }

    println!("{} Pushed branch to origin", style("✓").green());
    Ok(())
}

/// Returns the diff between the base branch and HEAD (for PR generation).
pub async fn get_branch_diff(
    base: &str,
    verbose: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["diff", &format!("{}...HEAD", base)])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr).into());
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    let filtered_diff = filter_excluded_diffs(&diff, verbose);
    Ok(truncate_diff(&filtered_diff, verbose))
}

/// Returns commit subjects between base branch and HEAD.
pub async fn get_branch_commits(base: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["log", &format!("{}..HEAD", base), "--format=%s"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git log failed: {}", stderr).into());
    }

    let commits: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .collect();

    Ok(commits)
}

/// Returns files changed between base branch and HEAD with status.
pub async fn get_pr_changed_files(
    base: &str,
    verbose: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["diff", "--name-status", &format!("{}...HEAD", base)])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff --name-status failed: {}", stderr).into());
    }

    let raw_output = String::from_utf8_lossy(&output.stdout).to_string();
    let mut excluded_count = 0;

    let annotated: Vec<String> = raw_output
        .lines()
        .map(|line| {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                let filename = parts[1];
                if should_exclude_from_diff(filename) {
                    excluded_count += 1;
                    format!("{}\t{} [excluded from diff]", parts[0], filename)
                } else {
                    line.to_string()
                }
            } else {
                line.to_string()
            }
        })
        .collect();

    if verbose && excluded_count > 0 {
        eprintln!(
            "— PR files: {} total, {} excluded from diff",
            annotated.len(),
            excluded_count
        );
    }

    Ok(annotated.join("\n"))
}
