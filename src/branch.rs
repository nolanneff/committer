//! Branch analysis and naming utilities.
//!
//! This module provides intelligent branch management:
//!
//! - **Branch alignment analysis**: Detects when commits don't match the current branch
//! - **Branch name generation**: Creates semantic branch names from commit messages
//! - **Protected branch detection**: Prevents accidental commits to main/master/etc.
//!
//! # Branch Naming Convention
//!
//! Generated branch names follow the pattern: `<type>/<scope>-<description>`
//!
//! Examples: `feat/auth-login`, `fix/ui-button-style`, `refactor/api-client`

use regex::Regex;
use reqwest::Client;
use serde::Deserialize;

use crate::api::{ChatRequest, Message, NonStreamResponse, OPENROUTER_API_URL};

/// Branches that should never receive direct commits.
pub const PROTECTED_BRANCHES: &[&str] = &["main", "master", "develop", "dev", "staging", "production"];

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

/// Result of analyzing whether a commit belongs on the current branch.
#[derive(Deserialize)]
pub struct BranchAnalysis {
    /// True if the commit aligns with the branch's purpose.
    pub matches: bool,
    /// Explanation of the analysis result.
    pub reason: String,
    /// Suggested branch name if there's a mismatch.
    pub suggested_branch: Option<String>,
}

/// User's choice when prompted about branch creation.
pub enum BranchAction {
    /// Create a new branch with the given name.
    Create(String),
    /// Continue on the current branch.
    Skip,
}

/// Converts text to a kebab-case slug suitable for branch names.
///
/// Filters out common filler words and limits to `max_words`.
pub fn slugify(text: &str, max_words: usize) -> String {
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

/// Generates a branch name from a commit message without LLM.
///
/// Parses conventional commit format to extract type/scope, falling back
/// to `feat/<slug>` if parsing fails.
pub fn generate_fallback_branch(commit_message: &str) -> String {
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

/// Analyzes whether a commit belongs on the current branch using LLM.
///
/// Returns analysis with match status, reason, and suggested branch name.
pub async fn analyze_branch_alignment(
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

/// Generates a branch name suggestion using LLM.
///
/// Falls back to [`generate_fallback_branch`] on error.
pub async fn generate_branch_suggestion(
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
