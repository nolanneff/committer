use clap::{Parser, Subcommand};
use console::Term;
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use tokio::process::Command;

// ============================================================================
// Configuration
// ============================================================================

const DEFAULT_MODEL: &str = "google/gemini-2.0-flash-001";
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

    /// Auto-create branch if commit doesn't match current branch
    #[arg(short = 'b', long)]
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

// ============================================================================
// OpenRouter API
// ============================================================================

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
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

CRITICAL: You MUST mention ALL changed files. Do not skip or summarize any changes.

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
- For multiple changes, ALWAYS add bullet points after a blank line
- Each bullet describes WHAT the change does semantically, not which file changed
- Focus on behavior and functionality, not file operations
- Keep bullet descriptions concise (5-10 words each)
- No quotes around the message

Files changed:
{files}

Diff:
{diff}

Commit message:"#,
        files = files,
        diff = diff
    )
}

async fn stream_commit_message(
    client: &Client,
    api_key: &str,
    model: &str,
    diff: &str,
    files: &str,
    spinner: &ProgressBar,
) -> Result<String, Box<dyn std::error::Error>> {
    let prompt = build_prompt(diff, files);

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt,
        }],
        stream: true,
    };

    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&request)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        spinner.finish_and_clear();
        return Err(format!("API error ({}): {}", status, body).into());
    }

    let mut stream = response.bytes_stream();
    let mut full_message = String::new();
    let mut stdout = io::stdout();
    let mut first_chunk = true;

    'outer: while let Some(chunk_result) = stream.next().await {
        let chunk = chunk_result?;
        let text = String::from_utf8_lossy(&chunk);

        // SSE format: each line starts with "data: "
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    break 'outer;
                }

                if let Ok(parsed) = serde_json::from_str::<StreamChunk>(data) {
                    for choice in parsed.choices {
                        if let Some(content) = choice.delta.content {
                            if first_chunk {
                                spinner.set_style(
                                    ProgressStyle::default_spinner()
                                        .template("Generating commit message [x]")
                                        .unwrap(),
                                );
                                spinner.finish_and_clear();
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

    println!(); // Newline after streaming completes

    Ok(full_message.trim().to_string())
}

#[derive(Deserialize)]
struct BranchSuggestion {
    matches: bool,
    branch_name: Option<String>,
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

async fn check_branch_match(
    client: &Client,
    api_key: &str,
    model: &str,
    commit_message: &str,
    current_branch: &str,
) -> Result<BranchSuggestion, Box<dyn std::error::Error>> {
    let prompt = format!(
        r#"You are a git branch analyzer. Given a commit message and current branch name, determine if they match semantically.

Current branch: {current_branch}
Commit message: {commit_message}

Rules:
1. If on "main", "master", "develop", or "dev" - they NEVER match (always need a feature branch)
2. Otherwise, check if the commit type and general topic align with the branch name
3. For example: branch "feat/user-auth" matches commit "FEAT(auth): add login validation"
4. But: branch "refactor/ui" does NOT match commit "FEAT(api): add new endpoint"

If they don't match, suggest a new branch name in format: type/short-description
- type should be lowercase: feat, fix, refactor, docs, style, perf, test, chore
- description should be lowercase, hyphen-separated, 2-4 words max

Respond with ONLY valid JSON (no markdown, no explanation):
{{"matches": true}} or {{"matches": false, "branch_name": "feat/example-name"}}"#
    );

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt,
        }],
        stream: false,
    };

    let response = client
        .post(OPENROUTER_API_URL)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
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

    let suggestion: BranchSuggestion = serde_json::from_str(content).map_err(|e| {
        format!(
            "Failed to parse branch suggestion: {} - raw: {}",
            e, content
        )
    })?;

    Ok(suggestion)
}

// ============================================================================
// User Interaction
// ============================================================================

fn prompt_yes_no(prompt: &str) -> bool {
    print!("{} [y/N] ", prompt);
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();

    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut config = load_config();

    // Handle config subcommand
    if let Some(Commands::Config { action }) = cli.command {
        match action {
            ConfigAction::Show => {
                println!("Config file: {}", config_path().display());
                println!("auto_commit: {}", config.auto_commit);
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
    spinner.enable_steady_tick(std::time::Duration::from_millis(120));

    // Hide cursor while spinner is active
    let term = Term::stdout();
    let _ = term.hide_cursor();

    let message_result =
        stream_commit_message(&client, &api_key, model, &diff, &files, &spinner).await;

    // Show cursor again
    let _ = term.show_cursor();

    let message = message_result?;

    if message.is_empty() {
        spinner.finish_and_clear();
        println!("— Error: empty commit message generated");
        std::process::exit(1);
    }

    // Handle auto-branch logic
    if cli.auto_branch {
        let current_branch = get_current_branch().await?;

        let branch_spinner = ProgressBar::new_spinner();
        branch_spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("Checking branch match {spinner}")
                .unwrap(),
        );
        branch_spinner.enable_steady_tick(std::time::Duration::from_millis(120));

        let suggestion =
            check_branch_match(&client, &api_key, model, &message, &current_branch).await?;
        branch_spinner.finish_and_clear();

        if !suggestion.matches {
            if let Some(new_branch) = suggestion.branch_name {
                println!(
                    "— Branch '{}' doesn't match commit, creating '{}'",
                    current_branch, new_branch
                );
                create_and_switch_branch(&new_branch).await?;
                println!("— Switched to branch '{}'", new_branch);
            }
        }
    }

    // Handle commit logic
    if cli.dry_run {
        return Ok(());
    }

    let should_commit = cli.yes || config.auto_commit || prompt_yes_no("Commit?");

    if should_commit {
        run_git_commit(&message).await?;
        println!("— Committed");
    } else {
        println!("— Cancelled");
    }

    Ok(())
}
