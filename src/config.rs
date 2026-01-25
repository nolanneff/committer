//! Configuration management for Committer.
//!
//! This module handles persistent configuration stored in TOML format at
//! `~/.config/committer/config.toml`. It provides:
//!
//! - [`Config`] struct with all user preferences
//! - Functions to [`load_config`] and [`save_config`]
//! - API key retrieval via [`get_api_key`]
//!
//! # Example
//!
//! ```no_run
//! use committer::config::{load_config, save_config};
//!
//! let mut config = load_config();
//! config.auto_commit = true;
//! save_config(&config).unwrap();
//! ```

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Default LLM model used for commit message generation.
pub const DEFAULT_MODEL: &str = "google/gemini-3-flash-preview";

/// User configuration for Committer.
///
/// All fields have sensible defaults and are optional in the config file.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Skip confirmation prompts and commit automatically.
    #[serde(default)]
    pub auto_commit: bool,

    /// Automatically commit after creating a new branch via the `b` option.
    #[serde(default)]
    pub commit_after_branch: bool,

    /// LLM model identifier for OpenRouter (e.g., "anthropic/claude-sonnet-4").
    #[serde(default = "default_model")]
    pub model: String,

    /// Enable detailed logging of operations.
    #[serde(default)]
    pub verbose: bool,
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

/// Returns the path to the configuration file.
///
/// Typically `~/.config/committer/config.toml` on Linux/macOS.
pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("committer")
        .join("config.toml")
}

/// Loads configuration from disk, returning defaults if file doesn't exist.
pub fn load_config() -> Config {
    let path = config_path();
    if path.exists() {
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        toml::from_str(&contents).unwrap_or_default()
    } else {
        Config::default()
    }
}

/// Saves configuration to disk, creating parent directories if needed.
pub fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = toml::to_string_pretty(config)?;
    std::fs::write(&path, contents)?;
    Ok(())
}

/// Retrieves the OpenRouter API key from the `OPENROUTER_API_KEY` environment variable.
pub fn get_api_key() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY").ok()
}
