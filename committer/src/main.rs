use clap::{Parser, Subcommand, ValueEnum};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::PathBuf;
use tokio::process::Command;

// ============================================================================
// Configuration
// ============================================================================

const DEFAULT_MODEL: &str = "x-ai/grok-4.1-fast:free";
const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

// ============================================================================
// Commit Message Formats
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum, Default)]
#[serde(rename_all = "lowercase")]
pub enum CommitFormat {
    /// Conventional Commits: type(scope): description
    #[default]
    Conventional,
    /// Simple one-line description
    Simple,
    /// Gitmoji style: 🎨 Add feature
    Gitmoji,
    /// Detailed multi-paragraph format
    Detailed,
    /// Minimal imperative style
    Imperative,
    /// Custom format using user-defined template
    Custom,
}

impl std::fmt::Display for CommitFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommitFormat::Conventional => write!(f, "conventional"),
            CommitFormat::Simple => write!(f, "simple"),
            CommitFormat::Gitmoji => write!(f, "gitmoji"),
            CommitFormat::Detailed => write!(f, "detailed"),
            CommitFormat::Imperative => write!(f, "imperative"),
            CommitFormat::Custom => write!(f, "custom"),
        }
    }
}

impl CommitFormat {
    fn get_template(&self) -> &'static str {
        match self {
            CommitFormat::Conventional => {
                r#"Generate a commit message using Conventional Commits format.

Format: type(scope): description

Types (pick the most appropriate):
- feat: A new feature
- fix: A bug fix
- docs: Documentation only changes
- style: Formatting, missing semicolons, etc (no code change)
- refactor: Code restructuring without changing behavior
- perf: Performance improvements
- test: Adding or updating tests
- build: Build system or dependency changes
- ci: CI/CD configuration changes
- chore: Maintenance tasks, tooling, etc

Rules:
- Scope is optional but encouraged (e.g., auth, api, ui)
- First line must be under 72 characters
- Use imperative mood ("add" not "added")
- No period at the end of the subject line
- Optionally add a blank line and bullet points for complex changes
- Output ONLY the commit message, no explanations"#
            }
            CommitFormat::Simple => {
                r#"Generate a simple, clear git commit message.

Rules:
- Single line, under 72 characters
- Start with a capital letter
- Use imperative mood ("Add" not "Added")
- No period at the end
- Be specific but concise
- Output ONLY the commit message, no explanations"#
            }
            CommitFormat::Gitmoji => {
                r#"Generate a commit message with a gitmoji prefix.

Format: <emoji> <description>

Common gitmojis:
✨ New feature
🐛 Bug fix
📝 Documentation
🎨 Style/formatting
♻️ Refactoring
⚡ Performance
✅ Tests
🔧 Configuration
🏗️ Architecture changes
🔥 Remove code/files
🚀 Deploy
📦 Dependencies
🔒 Security
🩹 Simple fix
➕ Add dependency
➖ Remove dependency
🚚 Move/rename files
💄 UI/style changes

Rules:
- Use exactly one emoji at the start
- Keep description under 60 characters
- Use imperative mood
- No period at the end
- Output ONLY the commit message, no explanations"#
            }
            CommitFormat::Detailed => {
                r#"Generate a detailed git commit message with subject and body.

Format:
<subject line>

<body paragraph explaining what and why>

<optional bullet points for specific changes>

Rules:
- Subject line under 72 characters, imperative mood
- Blank line between subject and body
- Body should explain WHAT changed and WHY (not how)
- Wrap body at 72 characters
- Use bullet points for multiple distinct changes
- Output ONLY the commit message, no explanations"#
            }
            CommitFormat::Imperative => {
                r#"Generate a minimal imperative commit message.

Rules:
- Start with a verb in imperative mood (Add, Fix, Update, Remove, Refactor)
- Single line, under 50 characters preferred, 72 max
- No type prefix, no scope, no emoji
- No period at the end
- Be direct and specific
- Output ONLY the commit message, no explanations"#
            }
            CommitFormat::Custom => {
                // Custom format uses user's template, this is fallback
                r#"Generate a git commit message.

Rules:
- Keep it concise and descriptive
- Use imperative mood
- Output ONLY the commit message, no explanations"#
            }
        }
    }

    fn get_examples(&self) -> &'static str {
        match self {
            CommitFormat::Conventional => {
                r#"Examples:
- feat(auth): add OAuth2 login support
- fix(api): handle null response from payment gateway
- refactor(ui): extract button component from form
- docs: update API endpoint documentation
- chore(deps): bump tokio to 1.35"#
            }
            CommitFormat::Simple => {
                r#"Examples:
- Add user authentication
- Fix crash on empty input
- Update README with examples
- Remove deprecated API calls"#
            }
            CommitFormat::Gitmoji => {
                r#"Examples:
- ✨ Add dark mode toggle
- 🐛 Fix memory leak in parser
- 📝 Update installation guide
- ♻️ Extract validation logic"#
            }
            CommitFormat::Detailed => {
                r#"Example:
Add rate limiting to API endpoints

Implement token bucket rate limiting to prevent abuse and ensure
fair usage across all API consumers. The default limit is set to
100 requests per minute per API key.

- Add RateLimiter middleware
- Configure limits in settings.toml
- Return 429 status with Retry-After header"#
            }
            CommitFormat::Imperative => {
                r#"Examples:
- Add caching layer
- Fix login redirect
- Update dependencies
- Remove dead code"#
            }
            CommitFormat::Custom => "",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    #[serde(default)]
    auto_commit: bool,
    #[serde(default = "default_model")]
    model: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    format: CommitFormat,
    #[serde(default)]
    custom_template: Option<String>,
    #[serde(default)]
    extra_instructions: Option<String>,
}

fn default_model() -> String {
    DEFAULT_MODEL.to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto_commit: false,
            model: default_model(),
            api_key: None,
            format: CommitFormat::default(),
            custom_template: None,
            extra_instructions: None,
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

fn get_api_key(config: &Config) -> Option<String> {
    // Environment variable takes precedence
    std::env::var("OPENROUTER_API_KEY")
        .ok()
        .or_else(|| config.api_key.clone())
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

    /// Override commit format for this run
    #[arg(short, long, value_enum)]
    format: Option<CommitFormat>,

    /// Add extra instructions for this run only
    #[arg(short = 'i', long)]
    instructions: Option<String>,
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
    /// Set API key (stored in config file)
    ApiKey {
        /// OpenRouter API key
        value: String,
    },
    /// Set commit message format preset
    Format {
        /// Format preset: conventional, simple, gitmoji, detailed, imperative, custom
        #[arg(value_enum)]
        value: CommitFormat,
    },
    /// Set custom template for commit messages (used when format is 'custom')
    Template {
        /// Custom prompt template for the AI
        value: String,
    },
    /// Add extra instructions to append to any format
    Instructions {
        /// Additional instructions (or "clear" to remove)
        value: String,
    },
    /// Show available format presets with examples
    Formats,
}

// ============================================================================
// Git Operations
// ============================================================================

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

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

fn build_prompt(config: &Config, diff: &str, files: &str) -> String {
    // Get the base template
    let template = if config.format == CommitFormat::Custom {
        config
            .custom_template
            .as_deref()
            .unwrap_or(CommitFormat::Custom.get_template())
    } else {
        config.format.get_template()
    };

    // Get examples for the format
    let examples = config.format.get_examples();

    // Build the prompt
    let mut prompt = String::new();

    // Add the template
    prompt.push_str(template);
    prompt.push_str("\n\n");

    // Add examples if available
    if !examples.is_empty() {
        prompt.push_str(examples);
        prompt.push_str("\n\n");
    }

    // Add extra instructions if configured
    if let Some(ref instructions) = config.extra_instructions {
        prompt.push_str("Additional requirements:\n");
        prompt.push_str(instructions);
        prompt.push_str("\n\n");
    }

    // Add the context
    prompt.push_str(&format!(
        r#"Files changed:
{}

Diff:
{}

Commit message:"#,
        files, diff
    ));

    prompt
}

async fn stream_commit_message(
    client: &Client,
    api_key: &str,
    model: &str,
    config: &Config,
    diff: &str,
    files: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let prompt = build_prompt(config, diff, files);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UserAction {
    Commit,
    Cancel,
    Retry,
    Edit,
}

fn prompt_action(prompt: &str) -> UserAction {
    loop {
        print!("{} [y]es / [n]o / [r]etry / [e]dit: ", prompt);
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            return UserAction::Cancel;
        }

        match input.trim().to_lowercase().as_str() {
            "y" | "yes" => return UserAction::Commit,
            "n" | "no" => return UserAction::Cancel,
            "r" | "retry" => return UserAction::Retry,
            "e" | "edit" => return UserAction::Edit,
            "" => return UserAction::Cancel, // Enter defaults to cancel
            _ => {
                println!("Invalid choice. Please enter y, n, r, or e.");
                continue;
            }
        }
    }
}

fn get_editor() -> Option<String> {
    // Try environment variables first
    if let Ok(editor) = std::env::var("EDITOR") {
        if !editor.is_empty() {
            return Some(editor);
        }
    }
    if let Ok(visual) = std::env::var("VISUAL") {
        if !visual.is_empty() {
            return Some(visual);
        }
    }

    // Platform-specific fallbacks
    #[cfg(windows)]
    {
        // notepad is always available on Windows
        return Some("notepad".to_string());
    }

    #[cfg(not(windows))]
    {
        let fallbacks = ["vim", "vi", "nano"];
        for editor in fallbacks {
            if std::process::Command::new("which")
                .arg(editor)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return Some(editor.to_string());
            }
        }
        None
    }
}

fn edit_message(message: &str) -> Result<String, Box<dyn std::error::Error>> {
    let editor = get_editor().ok_or("No editor found. Set $EDITOR environment variable.")?;

    // Create temp file with the message
    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join("committer_msg.txt");

    // Write message with helpful comments
    let content = format!(
        "{}\n\n# Edit your commit message above.\n# Lines starting with '#' will be removed.\n# Save and close the editor to continue.\n# Leave empty to cancel the commit.\n",
        message
    );
    std::fs::write(&temp_path, &content)?;

    // Open editor and wait for it to close
    let status = std::process::Command::new(&editor)
        .arg(&temp_path)
        .status()?;

    if !status.success() {
        std::fs::remove_file(&temp_path).ok();
        return Err(format!("Editor '{}' exited with error", editor).into());
    }

    // Read the edited content
    let edited = std::fs::read_to_string(&temp_path)?;

    // Clean up temp file
    std::fs::remove_file(&temp_path).ok();

    // Remove comment lines and trim
    let cleaned: String = edited
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();

    Ok(cleaned)
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
                    if config.api_key.is_some() {
                        "[set in config]"
                    } else if std::env::var("OPENROUTER_API_KEY").is_ok() {
                        "[set via OPENROUTER_API_KEY env var]"
                    } else {
                        "[not set]"
                    }
                );
                println!("format: {}", config.format);
                if let Some(ref template) = config.custom_template {
                    println!("custom_template: {} chars", template.len());
                }
                if let Some(ref instructions) = config.extra_instructions {
                    println!("extra_instructions: {}", instructions);
                }
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
            ConfigAction::ApiKey { value } => {
                config.api_key = Some(value);
                save_config(&config)?;
                println!("API key saved to config");
            }
            ConfigAction::Format { value } => {
                config.format = value;
                save_config(&config)?;
                println!("format set to: {}", config.format);
            }
            ConfigAction::Template { value } => {
                config.custom_template = Some(value);
                save_config(&config)?;
                println!("Custom template saved. Set format to 'custom' to use it:");
                println!("  committer config format custom");
            }
            ConfigAction::Instructions { value } => {
                if value.to_lowercase() == "clear" {
                    config.extra_instructions = None;
                    save_config(&config)?;
                    println!("Extra instructions cleared");
                } else {
                    config.extra_instructions = Some(value);
                    save_config(&config)?;
                    println!("Extra instructions saved");
                }
            }
            ConfigAction::Formats => {
                println!("Available commit message formats:\n");
                for format in [
                    CommitFormat::Conventional,
                    CommitFormat::Simple,
                    CommitFormat::Gitmoji,
                    CommitFormat::Detailed,
                    CommitFormat::Imperative,
                    CommitFormat::Custom,
                ] {
                    println!("━━━ {} ━━━", format.to_string().to_uppercase());
                    let examples = format.get_examples();
                    if !examples.is_empty() {
                        println!("{}\n", examples);
                    } else {
                        println!("(User-defined template)\n");
                    }
                }
                println!("Set format with: committer config format <name>");
                println!("Override per-run: committer --format <name>");
            }
        }
        return Ok(());
    }

    // Get API key
    let api_key = match get_api_key(&config) {
        Some(key) => key,
        None => {
            eprintln!("Error: No API key found.");
            eprintln!("Set OPENROUTER_API_KEY environment variable or run:");
            eprintln!("  committer config api-key YOUR_API_KEY");
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

    // Apply CLI format override
    if let Some(format) = cli.format {
        config.format = format;
    }

    // Apply CLI instructions (append to existing)
    if let Some(ref cli_instructions) = cli.instructions {
        config.extra_instructions = Some(match config.extra_instructions {
            Some(existing) => format!("{}\n{}", existing, cli_instructions),
            None => cli_instructions.clone(),
        });
    }

    // Create HTTP client
    let client = Client::builder()
        .build()?;

    // Main generation loop - allows retry
    loop {
        // Stream the commit message
        eprint!("Generating commit message...");
        let message = stream_commit_message(&client, &api_key, model, &config, &diff, &files).await?;

        if message.is_empty() {
            eprintln!("Error: Empty commit message generated");
            std::process::exit(1);
        }

        // Handle commit logic
        if cli.dry_run {
            // Don't commit, just exit
            return Ok(());
        }

        // Auto-commit if --yes flag or config is set
        if cli.yes || config.auto_commit {
            run_git_commit(&message).await?;
            return Ok(());
        }

        // Interactive prompt
        match prompt_action("\nCommit with this message?") {
            UserAction::Commit => {
                run_git_commit(&message).await?;
                return Ok(());
            }
            UserAction::Cancel => {
                println!("Commit cancelled.");
                return Ok(());
            }
            UserAction::Retry => {
                println!("\nRegenerating...\n");
                continue;
            }
            UserAction::Edit => {
                match edit_message(&message) {
                    Ok(edited) => {
                        if edited.is_empty() {
                            println!("Empty message. Commit cancelled.");
                            return Ok(());
                        }
                        println!("\nEdited message:\n{}\n", edited);
                        // After editing, ask for confirmation (no retry option)
                        print!("Commit with this message? [y/N]: ");
                        io::stdout().flush().unwrap();
                        let mut input = String::new();
                        io::stdin().read_line(&mut input).unwrap();
                        if matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
                            run_git_commit(&edited).await?;
                        } else {
                            println!("Commit cancelled.");
                        }
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("Error editing message: {}", e);
                        println!("Continuing with original message...\n");
                        continue;
                    }
                }
            }
        }
    }
}
