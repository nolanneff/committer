//! OpenRouter API integration for LLM-powered content generation.
//!
//! This module handles all communication with the OpenRouter API, including:
//!
//! - **Streaming responses**: Real-time token-by-token output
//! - **Prompt construction**: Building prompts for commit messages and PRs
//! - **Response parsing**: Handling both streaming and non-streaming responses
//!
//! # Key Functions
//!
//! - [`stream_commit_message`]: Generate a commit message with streaming output
//! - [`stream_pr_content`]: Generate PR title and body with streaming output
//! - [`build_prompt`]: Construct the commit message prompt
//! - [`build_pr_prompt`]: Construct the PR generation prompt

use futures::StreamExt;
use indicatif::ProgressBar;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

/// OpenRouter API endpoint for chat completions.
pub const OPENROUTER_API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// Request body for OpenRouter chat completions API.
#[derive(Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderPreference>,
}

/// Provider ordering preferences for OpenRouter.
#[derive(Serialize)]
pub struct ProviderPreference {
    pub order: Vec<String>,
}

/// A single message in the chat conversation.
#[derive(Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

/// A chunk from the streaming response.
#[derive(Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<Choice>,
}

/// A single choice in a streaming chunk.
#[derive(Deserialize)]
pub struct Choice {
    pub delta: Delta,
}

/// Delta content in a streaming response.
#[derive(Deserialize)]
pub struct Delta {
    pub content: Option<String>,
}

/// A single choice in a non-streaming response.
#[derive(Deserialize)]
pub struct NonStreamChoice {
    pub message: NonStreamMessage,
}

/// Message content in a non-streaming response.
#[derive(Deserialize)]
pub struct NonStreamMessage {
    pub content: String,
}

/// Complete non-streaming response from OpenRouter.
#[derive(Deserialize)]
pub struct NonStreamResponse {
    pub choices: Vec<NonStreamChoice>,
}

/// Builds the prompt for commit message generation.
///
/// Includes instructions for conventional commit format and the diff/files context.
pub fn build_prompt(diff: &str, files: &str) -> String {
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

/// Builds the prompt for PR title and description generation.
///
/// Includes commit list, diff, and files for context.
pub fn build_pr_prompt(diff: &str, files: &str, commits: &[String]) -> String {
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

/// Streams PR title and body generation from the LLM.
///
/// Returns (title, body) tuple. Output is printed token-by-token as it streams.
#[allow(clippy::too_many_arguments)]
pub async fn stream_pr_content(
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

/// Streams commit message generation from the LLM.
///
/// Output is printed token-by-token as it streams. Falls back to non-streaming
/// parsing if the response doesn't use SSE format.
pub async fn stream_commit_message(
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
            eprintln!(
                "[Stream] Chunk {}: {} bytes, preview: {:?}",
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
                            eprintln!(
                                "[Stream] Parse error: {} for data: {:?}",
                                e,
                                data.chars().take(100).collect::<String>()
                            );
                        }
                    }
                }
            }
        }
    }

    if verbose {
        eprintln!(
            "[Stream] Stream ended. Total chunks: {}, SSE lines: {}, message length: {}",
            chunk_count,
            sse_lines_found,
            full_message.len()
        );
    }

    // Fallback: if streaming produced nothing, try parsing as non-streaming response
    if full_message.is_empty() && !raw_response.is_empty() {
        if verbose {
            eprintln!("[Stream] No streaming content, trying non-streaming fallback...");
            eprintln!(
                "[Stream] Raw response preview: {:?}",
                &raw_response.chars().take(300).collect::<String>()
            );
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
