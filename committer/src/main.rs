use clap::{Parser, Subcommand};
use futures::StreamExt;
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

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    auto_commit: bool,
    #[serde(default = "default_model")]
    model: String,
}

fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_commit: false,
            model: default_model(),
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
}

// ============================================================================
// Git Operations
// ============================================================================

// Maximum characters to send (roughly 100K tokens * 4 chars/token = 400K chars)
// Leave headroom for prompt and response
const MAX_DIFF_CHARS: usize = 300_000;

async fn get_git_diff(staged_only: bool) -> Result<String, Box<dyn std::error::Error>> {
    let args = if staged_only {
        vec!["diff", "--staged"]
    } else {
        vec!["diff", "HEAD"]
    };

    let output = Command::new("git")
        .args(&args)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff failed: {}", stderr).into());
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(truncate_diff(&diff))
}

/// Truncates a diff to fit within token limits while preserving useful context.
/// Keeps the beginning (file headers, context) and end (recent changes).
fn truncate_diff(diff: &str) -> String {
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
        result.push_str(&format!(
            "\n[... diff truncated: showing {}/{} files to fit context limit ...]\n",
            included,
            file_diffs.len()
        ));
    }

    result
}

async fn get_staged_files() -> Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["diff", "--staged", "--name-status"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git diff --name-status failed: {}", stderr).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("\n{}", stdout);
    Ok(())
}

async fn stage_all_changes() -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["add", "-A"])
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git add failed: {}", stderr).into());
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
        r#"Generate a concise git commit message for the following changes.

Rules:
- Use conventional commit format: type(scope): description
- Types: feat, fix, docs, style, refactor, perf, test, chore
- Keep the first line under 72 characters
- Optionally add bullet points for significant changes
- Be specific about what changed, not why
- No quotes around the message

Files changed:
{}

Diff:
{}

Commit message:"#,
        files, diff
    )
}

async fn stream_commit_message(
    client: &Client,
    api_key: &str,
    model: &str,
    diff: &str,
    files: &str,
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
        return Err(format!("OpenRouter API error ({}): {}", status, body).into());
    }

    let mut stream = response.bytes_stream();
    let mut full_message = String::new();
    let mut stdout = io::stdout();

    // Print a newline before streaming starts
    println!();

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
        }
        return Ok(());
    }

    // Get API key
    let api_key = match get_api_key() {
        Some(key) => key,
        None => {
            eprintln!("Error: No API key found.");
            eprintln!("Set the OPENROUTER_API_KEY environment variable:\n");
            eprintln!("  Linux/macOS (bash/zsh):");
            eprintln!("    export OPENROUTER_API_KEY=\"your-api-key\"");
            eprintln!("    # Add to ~/.bashrc or ~/.zshrc to persist\n");
            eprintln!("  Windows (PowerShell):");
            eprintln!("    $env:OPENROUTER_API_KEY = \"your-api-key\"");
            eprintln!("    # Or set permanently via System Properties > Environment Variables\n");
            eprintln!("  Windows (Command Prompt):");
            eprintln!("    set OPENROUTER_API_KEY=your-api-key");
            std::process::exit(1);
        }
    };

    // Stage all changes if requested
    if cli.all {
        stage_all_changes().await?;
    }

    // Get diff and file list in parallel
    let (diff_result, files_result) = tokio::join!(
        get_git_diff(true),
        get_staged_files()
    );

    let diff = diff_result?;
    let files = files_result?;

    if diff.trim().is_empty() {
        eprintln!("No staged changes found.");
        eprintln!("Stage your changes with 'git add' or use 'committer --all'");
        std::process::exit(1);
    }

    // Determine which model to use
    let model = cli.model.as_ref().unwrap_or(&config.model);

    // Create HTTP client
    let client = Client::builder()
        .build()?;

    // Stream the commit message
    eprint!("Generating commit message...");
    let message = stream_commit_message(&client, &api_key, model, &diff, &files).await?;

    if message.is_empty() {
        eprintln!("Error: Empty commit message generated");
        std::process::exit(1);
    }

    // Handle commit logic
    if cli.dry_run {
        // Don't commit, just exit
        return Ok(());
    }

    let should_commit = cli.yes || config.auto_commit || prompt_yes_no("\nCommit with this message?");

    if should_commit {
        run_git_commit(&message).await?;
    } else {
        println!("\nCommit cancelled.");
    }

    Ok(())
}
