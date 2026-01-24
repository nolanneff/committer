use clap::{Parser, Subcommand};
use dialoguer::Input;
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use tokio::process::Command;

// ============================================================================
// Configuration
// ============================================================================

const DEFAULT_MODEL: &str = "google/gemini-3-flash-preview";
const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

const EXCLUDED_FROM_DIFF: &[&str] = &[
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

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    auto_commit: bool,
    #[serde(default)]
    commit_after_branch: bool,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default)]
    verbose: bool,
}

fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_commit: false,
            commit_after_branch: false,
            model: default_model(),
            verbose: false,
        }
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("committer")
        .join("config.toml")
}

fn load_config() -> Config {
    let path = config_path();
    if path.exists() {
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        toml::from_str(&contents).unwrap_or_default()
    } else {
        Config::default()
    }
}

fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = toml::to_string_pretty(config)?;
    std::fs::write(&path, contents)?;
    Ok(())
}

fn get_api_key() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY").ok()
}

// ============================================================================
// CLI Interface
// ============================================================================

#[derive(Parser)]
#[command(name = "committer")]
#[command(about = "Fast AI-powered git commit message generator", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Auto-commit without asking
    #[arg(short = 'y', long)]
    yes: bool,

    /// Just print the message, don't commit
    #[arg(short, long)]
    dry_run: bool,

    /// Include unstaged changes
    #[arg(short, long)]
    all: bool,

    /// Override model for this run
    #[arg(short, long)]
    model: Option<String>,

    /// Interactive branch suggestion on mismatch [y/n/e]
    #[arg(short = 'b', long)]
    branch: bool,

    /// Auto-create branch on mismatch (non-interactive, just logs)
    #[arg(short = 'B', long)]
    auto_branch: bool,

    /// Show detailed operation logs (excluded files, truncation, etc.)
    #[arg(short = 'v', long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Generate and create a pull request
    Pr(PrArgs),
}

#[derive(Parser)]
struct PrArgs {
    /// Create PR without confirmation
    #[arg(short = 'y', long)]
    yes: bool,

    /// Show generated content, don't create PR
    #[arg(short, long)]
    dry_run: bool,

    /// Create as draft PR
    #[arg(short = 'D', long)]
    draft: bool,

    /// Override base branch (default: auto-detect)
    #[arg(short, long)]
    base: Option<String>,

    /// Show detailed operation logs
    #[arg(short = 'v', long)]
    verbose: bool,

    /// Override model for this run
    #[arg(short, long)]
    model: Option<String>,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration
    Show,
    /// Set auto-commit behavior
    AutoCommit {
        /// true or false
        value: String,
    },
    /// Auto-commit after creating branch via 'b' option
    CommitAfterBranch {
        /// true or false
        value: String,
    },
    /// Set default model
    Model {
        /// Model identifier (e.g., x-ai/grok-4.1-fast:free)
        value: String,
    },
    /// Enable verbose operation logs by default
    Verbose {
        /// true or false
        value: String,
    },
}

// ============================================================================
// Git Operations
// ============================================================================

// Maximum characters to send (roughly 100K tokens * 4 chars/token = 400K chars)
// Leave headroom for prompt and response
const MAX_DIFF_CHARS: usize = 300_000;

fn should_exclude_from_diff(filename: &str) -> bool {
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

fn filter_excluded_diffs(diff: &str, verbose: bool) -> String {
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

async fn get_git_diff(
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

/// Truncates a diff to fit within token limits while preserving useful context.
/// Keeps the beginning (file headers, context) and end (recent changes).
fn truncate_diff(diff: &str, verbose: bool) -> String {
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

async fn get_staged_files(verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
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

async fn run_git_commit(message: &str) -> Result<(), Box<dyn std::error::Error>> {
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

async fn stage_all_changes() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git").args(["add", "-A"]).output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git add failed: {}", stderr).into());
    }

    Ok(())
}

async fn get_current_branch() -> Result<String, Box<dyn std::error::Error>> {
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

async fn create_and_switch_branch(branch_name: &str) -> Result<(), Box<dyn std::error::Error>> {
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

async fn get_recent_commits(limit: usize) -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["log", "--oneline", &format!("-{}", limit), "--format=%s"])
        .output()
        .await?;

    if !output.status.success() {
        return Ok(String::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

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

async fn get_default_base_branch() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("gh")
        .args(["repo", "view", "--json", "defaultBranchRef", "-q", ".defaultBranchRef.name"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to get default branch: {}", stderr).into());
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        return Err("Could not determine default branch".into());
    }
    Ok(branch)
}

async fn get_upstream_remote() -> Result<Option<String>, Box<dyn std::error::Error>> {
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

async fn ensure_branch_pushed(branch: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check if branch has upstream tracking
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", &format!("{}@{{u}}", branch)])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => Ok(()), // Already has upstream
        _ => {
            // Push with -u to set upstream
            println!("— Pushing branch to origin...");
            let push_output = Command::new("git")
                .args(["push", "-u", "origin", branch])
                .output()
                .await?;

            if !push_output.status.success() {
                let stderr = String::from_utf8_lossy(&push_output.stderr);
                return Err(format!("Failed to push branch: {}", stderr).into());
            }
            Ok(())
        }
    }
}

async fn get_branch_diff(base: &str, verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
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

async fn get_branch_commits(base: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
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

async fn get_pr_changed_files(base: &str, verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
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

// ============================================================================
// Branch Analysis
// ============================================================================

const FILLER_WORDS: &[&str] = &[
    "add",
    "update",
    "fix",
    "remove",
    "delete",
    "change",
    "modify",
    "implement",
    "create",
    "make",
    "set",
    "get",
    "use",
    "handle",
    "support",
    "enable",
    "disable",
    "allow",
    "improve",
    "enhance",
    "the",
    "a",
    "an",
    "to",
    "for",
    "of",
    "in",
    "on",
    "with",
    "and",
    "or",
];

#[derive(Deserialize)]
struct BranchAnalysis {
    matches: bool,
    reason: String,
    suggested_branch: Option<String>,
}

enum BranchAction {
    Create(String),
    Skip,
}

fn slugify(text: &str, max_words: usize) -> String {
    let words: Vec<&str> = text
        .split_whitespace()
        .filter(|w| !FILLER_WORDS.contains(&w.to_lowercase().as_str()))
        .take(max_words)
        .collect();

    if words.is_empty() {
        let fallback: Vec<&str> = text.split_whitespace().take(max_words).collect();
        return fallback
            .join("-")
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect();
    }

    words
        .join("-")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect()
}

fn generate_fallback_branch(commit_message: &str) -> String {
    let first_line = commit_message.lines().next().unwrap_or(commit_message);

    let re = Regex::new(r"^([a-z]+)(?:\(([^)]+)\))?:\s*(.+)$").unwrap();
    if let Some(caps) = re.captures(first_line) {
        let commit_type = caps.get(1).map(|m| m.as_str()).unwrap_or("feat");
        let scope = caps.get(2).map(|m| m.as_str());
        let description = caps.get(3).map(|m| m.as_str()).unwrap_or("changes");

        let desc_slug = slugify(description, 3);
        match scope {
            Some(s) => format!("{}/{}-{}", commit_type, s, desc_slug),
            None => format!("{}/{}", commit_type, desc_slug),
        }
    } else {
        let slug = slugify(first_line, 3);
        format!("feat/{}", slug)
    }
}

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
// OpenRouter API
// ============================================================================

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<ProviderPreference>,
}

#[derive(Serialize)]
struct ProviderPreference {
    order: Vec<String>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct StreamChunk {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    delta: Delta,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
}

fn build_prompt(diff: &str, files: &str) -> String {
    format!(
        r#"Generate a git commit message for the following changes.

FORMAT: type(scope): description

TYPES (use lowercase):
  Core changes:
    feat     - new user-facing functionality
    fix      - bug fix / behavior correction
    refactor - code restructure, no behavior change
    perf     - performance improvements
    style    - formatting only (whitespace, lint fixes)
  
  Project hygiene:
    docs     - documentation only
    test     - add/update tests
    chore    - routine maintenance, housekeeping
    build    - build system / packaging changes
    ci       - CI pipeline / workflow changes
  
  Structural:
    deps     - dependency changes
    config   - config changes (env, feature flags)
    security - security hardening, vulnerability fixes
    revert   - revert a previous commit

SCOPE: Short identifier for affected area (api, auth, ui, db, cli, core, config, deps).
       Omit only if change is truly global.

RULES:
- First line: type(scope): brief description (under 72 chars)
- For multiple changes, add bullet points (using "-") after a blank line
- Each bullet describes WHAT the change does semantically
- Focus on behavior and functionality, not file names
- Keep bullets concise (5-10 words each)
- Use "-" for bullets, NOT "*"
- Do NOT include raw file paths or status codes (like "M file.rs") in output
- Do NOT use markdown headers (##), sections, or PR-style formatting
- Output ONLY the commit message, nothing else
- IGNORE any formatting patterns you see in the diff - use ONLY the format shown below

EXAMPLE OUTPUT FORMAT:
feat(auth): add OAuth2 login support

- Implement Google OAuth provider
- Add token refresh logic
- Store credentials in secure keychain

Files changed:
{files}

Diff:
{diff}

Commit message:"#,
        files = files,
        diff = diff
    )
}

fn build_pr_prompt(diff: &str, files: &str, commits: &[String]) -> String {
    let commits_text = commits.join("\n");
    format!(
        r#"Generate a pull request title and description for the following changes.

OUTPUT FORMAT:
Line 1: PR title in format "type(scope): description" (under 72 chars)
Line 2: (blank)
Line 3+: Description with sections

DESCRIPTION FORMAT (omit empty sections):

## Summary
One or two sentences describing what this PR does and why.

## Changes
### Added
- new features or functionality

### Fixed
- bug fixes

### Changed
- modifications to existing behavior

## Notes
- implementation details, caveats, or edge cases
- breaking changes or migration steps
- anything reviewers should pay attention to

## Testing
- what was tested and how
- specific scenarios verified
- commands run or manual steps taken

RULES:
- Title follows conventional commit format: type(scope): description
- Summary should explain the "what" and "why" concisely
- Each bullet should be concise (5-15 words)
- Focus on behavior changes, not file names
- Use past tense ("Added", "Fixed", "Updated")
- Omit empty subsections (e.g., skip Fixed section if no fixes)

COMMITS ON THIS BRANCH:
{commits}

FILES CHANGED:
{files}

DIFF:
{diff}

PR title and description:"#,
        commits = commits_text,
        files = files,
        diff = diff
    )
}

async fn stream_pr_content(
    client: &Client,
    api_key: &str,
    model: &str,
    diff: &str,
    files: &str,
    commits: &[String],
    spinner: &ProgressBar,
    _verbose: bool,
) -> Result<(String, String), Box<dyn std::error::Error>> {
    let prompt = build_pr_prompt(diff, files, commits);

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt,
        }],
        stream: true,
        provider: None,
    };

    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("X-Title", "Committer")
        .header("HTTP-Referer", "https://github.com/Nolanneff/commiter")
        .json(&request)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        spinner.disable_steady_tick();
        spinner.finish_and_clear();
        return Err(format!("API error ({}): {}", status, body).into());
    }

    let mut stream = response.bytes_stream();
    let mut full_message = String::new();
    let mut stdout = io::stdout();
    let mut first_chunk = true;
    let mut raw_response = String::new();

    'outer: while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        let text = String::from_utf8_lossy(&chunk);
        raw_response.push_str(&text);

        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    break 'outer;
                }

                if let Ok(parsed) = serde_json::from_str::<StreamChunk>(data) {
                    for choice in parsed.choices {
                        if let Some(content) = choice.delta.content {
                            if first_chunk {
                                spinner.disable_steady_tick();
                                spinner.finish_and_clear();
                                println!();
                                first_chunk = false;
                            }
                            print!("{}", content);
                            stdout.flush()?;
                            full_message.push_str(&content);
                        }
                    }
                }
            }
        }
    }

    // Fallback to non-streaming if needed
    if full_message.is_empty() && !raw_response.is_empty() {
        spinner.disable_steady_tick();
        spinner.finish_and_clear();

        if let Ok(parsed) = serde_json::from_str::<NonStreamResponse>(&raw_response) {
            if let Some(choice) = parsed.choices.first() {
                full_message = choice.message.content.clone();
                println!("{}", full_message);
            }
        }
    } else if !first_chunk {
        println!();
    } else {
        spinner.disable_steady_tick();
        spinner.finish_and_clear();
    }

    // Parse title and body from response
    let content = full_message.trim();
    let mut lines = content.lines();
    let title = lines.next().unwrap_or("").trim().to_string();

    // Skip blank line after title
    lines.next();

    let body: String = lines.collect::<Vec<_>>().join("\n").trim().to_string();

    if title.is_empty() {
        return Err("Failed to generate PR title".into());
    }

    Ok((title, body))
}

async fn stream_commit_message(
    client: &Client,
    api_key: &str,
    model: &str,
    diff: &str,
    files: &str,
    spinner: &ProgressBar,
    verbose: bool,
) -> Result<String, Box<dyn std::error::Error>> {
    let prompt = build_prompt(diff, files);

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt,
        }],
        stream: true,
        provider: None,
    };

    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("X-Title", "Committer")
        .header("HTTP-Referer", "https://github.com/Nolanneff/commiter")
        .json(&request)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        spinner.disable_steady_tick();
        spinner.finish_and_clear();
        return Err(format!("API error ({}): {}", status, body).into());
    }

    if verbose {
        eprintln!("[Stream] Starting to read response stream...");
    }

    let mut stream = response.bytes_stream();
    let mut full_message = String::new();
    let mut stdout = io::stdout();
    let mut first_chunk = true;
    let mut raw_response = String::new();
    let mut chunk_count = 0;
    let mut sse_lines_found = 0;

    'outer: while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        let text = String::from_utf8_lossy(&chunk);
        raw_response.push_str(&text);
        chunk_count += 1;

        if verbose {
            eprintln!("[Stream] Chunk {}: {} bytes, preview: {:?}",
                chunk_count,
                chunk.len(),
                text.chars().take(100).collect::<String>()
            );
        }

        // SSE format: each line starts with "data: "
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                sse_lines_found += 1;
                if data == "[DONE]" {
                    if verbose {
                        eprintln!("[Stream] Received [DONE] signal");
                    }
                    break 'outer;
                }

                match serde_json::from_str::<StreamChunk>(data) {
                    Ok(parsed) => {
                        for choice in parsed.choices {
                            if let Some(content) = choice.delta.content {
                                if first_chunk {
                                    if verbose {
                                        eprintln!("[Stream] First content chunk, clearing spinner");
                                    }
                                    spinner.disable_steady_tick();
                                    spinner.finish_and_clear();
                                    println!(); // Ensure clean line after spinner
                                    first_chunk = false;
                                }
                                print!("{}", content);
                                stdout.flush()?;
                                full_message.push_str(&content);
                            }
                        }
                    }
                    Err(e) => {
                        if verbose {
                            eprintln!("[Stream] Parse error: {} for data: {:?}", e, data.chars().take(100).collect::<String>());
                        }
                    }
                }
            }
        }
    }

    if verbose {
        eprintln!("[Stream] Stream ended. Total chunks: {}, SSE lines: {}, message length: {}",
            chunk_count, sse_lines_found, full_message.len());
    }

    // Fallback: if streaming produced nothing, try parsing as non-streaming response
    if full_message.is_empty() && !raw_response.is_empty() {
        if verbose {
            eprintln!("[Stream] No streaming content, trying non-streaming fallback...");
            eprintln!("[Stream] Raw response preview: {:?}", &raw_response.chars().take(300).collect::<String>());
        }

        spinner.disable_steady_tick();
        spinner.finish_and_clear();

        // Try parsing as a complete non-streaming response
        if let Ok(parsed) = serde_json::from_str::<NonStreamResponse>(&raw_response) {
            if let Some(choice) = parsed.choices.first() {
                full_message = choice.message.content.clone();
                println!("{}", full_message);
                if verbose {
                    eprintln!("[Stream] Fallback succeeded");
                }
            }
        } else if verbose {
            eprintln!("[Stream] Fallback parse failed");
        }
    } else if !first_chunk {
        // Only print newline if we actually printed content
        println!();
    } else {
        // Spinner still running but no content - clear it
        spinner.disable_steady_tick();
        spinner.finish_and_clear();
    }

    Ok(full_message.trim().to_string())
}

#[derive(Deserialize)]
struct NonStreamChoice {
    message: NonStreamMessage,
}

#[derive(Deserialize)]
struct NonStreamMessage {
    content: String,
}

#[derive(Deserialize)]
struct NonStreamResponse {
    choices: Vec<NonStreamChoice>,
}

async fn analyze_branch_alignment(
    client: &Client,
    api_key: &str,
    model: &str,
    current_branch: &str,
    commit_message: &str,
    files_changed: &str,
    recent_commits: &str,
) -> Result<BranchAnalysis, Box<dyn std::error::Error>> {
    let prompt = format!(
        r#"You are a git branch analyzer. Determine if the current commit belongs on this branch.

CURRENT BRANCH: {current_branch}

RECENT COMMITS ON THIS BRANCH:
{recent_commits}

FILES BEING CHANGED IN THIS COMMIT:
{files_changed}

NEW COMMIT MESSAGE:
{commit_message}

ANALYSIS RULES:
1. Protected branches (main, master, develop, dev, staging, production) - NEVER match, always suggest a feature branch
2. The commit scope/module MUST relate to the branch name. Example: branch "feat/auth-login" should only have auth-related commits, NOT unrelated features like "feat(db): add migration"
3. Different commit TYPES (feat, fix, refactor, docs, test) on the SAME feature are fine - e.g., feat/auth can have "feat(auth): add login" then "fix(auth): handle edge case" then "docs(auth): add comments"
4. If the commit introduces a NEW scope/module not mentioned in the branch name, flag as MISMATCH
5. Be STRICT: when in doubt, flag as mismatch. It's better to suggest a new branch than pollute an existing one with unrelated work

BRANCH NAMING CONVENTION: <type>/<scope>-<short-description>
Examples: feat/auth-refresh-token, fix/ui-chat-scroll, refactor/server-ws-reconnect

Respond with ONLY valid JSON:
- If matches: {{"matches": true, "reason": "brief explanation"}}
- If mismatch: {{"matches": false, "reason": "brief explanation", "suggested_branch": "type/scope-description"}}"#
    );

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt,
        }],
        stream: false,
        provider: None,
    };

    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("X-Title", "Committer")
        .header("HTTP-Referer", "https://github.com/Nolanneff/commiter")
        .json(&request)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("API error ({}): {}", status, body).into());
    }

    let response_body: NonStreamResponse = response.json().await?;
    let content = response_body
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    let content = content.trim();
    let content = content.strip_prefix("```json").unwrap_or(content);
    let content = content.strip_prefix("```").unwrap_or(content);
    let content = content.strip_suffix("```").unwrap_or(content);
    let content = content.trim();

    let analysis: BranchAnalysis = serde_json::from_str(content)
        .map_err(|e| format!("Failed to parse branch analysis: {} - raw: {}", e, content))?;

    Ok(analysis)
}

async fn generate_branch_suggestion(
    client: &Client,
    api_key: &str,
    model: &str,
    commit_message: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let prompt = format!(
        r#"Given this commit message, suggest an appropriate git branch name.

COMMIT MESSAGE:
{commit_message}

BRANCH NAMING RULES:
1. Use format: <type>/<scope>-<short-description>
2. Type should match the commit type (feat, fix, docs, refactor, test, chore, etc.)
3. Scope is the area/module being changed (auth, ui, server, api, etc.)
4. Description should be kebab-case, concise (2-4 words)
5. Keep the full branch name under 50 characters when possible

BRANCH NAMING CONVENTION: <type>/<scope>-<short-description>
Examples: feat/auth-refresh-token, fix/ui-chat-scroll, refactor/server-ws-reconnect

Respond with ONLY the branch name, nothing else."#
    );

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt,
        }],
        stream: false,
        provider: None,
    };

    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header("X-Title", "Committer")
        .header("HTTP-Referer", "https://github.com/nolancui/committer")
        .json(&request)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(format!("API request failed: {}", response.status()).into());
    }

    let response_body: NonStreamResponse = response.json().await?;
    let content = response_body
        .choices
        .first()
        .map(|c| c.message.content.clone())
        .unwrap_or_default();

    let branch_name = content.trim().to_string();

    if branch_name.is_empty() {
        return Err("Empty branch name returned".into());
    }

    Ok(branch_name)
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

const PROTECTED_BRANCHES: &[&str] = &["main", "master", "develop", "dev", "staging", "production"];

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
        None => get_default_base_branch().await?,
    };

    if verbose {
        eprintln!("— Base branch: {}", base_branch);
        eprintln!("— Current branch: {}", current_branch);
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

    // Ensure branch is pushed
    if !args.dry_run {
        ensure_branch_pushed(&current_branch).await?;
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
        let url = create_pr(&title, &body, args.draft).await?;
        println!();
        println!("— PR created: {}", url);
    } else {
        match prompt_pr(&title, &body) {
            PrAction::Create(final_title, final_body) => {
                let url = create_pr(&final_title, &final_body, args.draft).await?;
                println!();
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
