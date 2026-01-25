use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const DEFAULT_MODEL: &str = "google/gemini-3-flash-preview";

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub auto_commit: bool,
    #[serde(default)]
    pub commit_after_branch: bool,
    #[serde(default = "default_model")]
    pub model: String,
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

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("committer")
        .join("config.toml")
}

pub fn load_config() -> Config {
    let path = config_path();
    if path.exists() {
        let contents = std::fs::read_to_string(&path).unwrap_or_default();
        toml::from_str(&contents).unwrap_or_default()
    } else {
        Config::default()
    }
}

pub fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let contents = toml::to_string_pretty(config)?;
    std::fs::write(&path, contents)?;
    Ok(())
}

pub fn get_api_key() -> Option<String> {
    std::env::var("OPENROUTER_API_KEY").ok()
}
