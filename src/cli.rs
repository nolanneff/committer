//! Command-line interface definitions for Committer.
//!
//! This module defines all CLI arguments, subcommands, and options using
//! the [clap](https://docs.rs/clap) crate with derive macros.
//!
//! # Commands
//!
//! - Default (no subcommand): Generate and create a commit
//! - `config`: Manage persistent configuration
//! - `pr`: Generate and create a pull request

use clap::{Parser, Subcommand};

/// Main CLI structure for Committer.
#[derive(Parser)]
#[command(name = "committer")]
#[command(about = "Fast AI-powered git commit message generator", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Auto-commit without asking
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Just print the message, don't commit
    #[arg(short, long)]
    pub dry_run: bool,

    /// Include unstaged changes
    #[arg(short, long)]
    pub all: bool,

    /// Override model for this run
    #[arg(short, long)]
    pub model: Option<String>,

    /// Interactive branch suggestion on mismatch [y/n/e]
    #[arg(short = 'b', long)]
    pub branch: bool,

    /// Auto-create branch on mismatch (non-interactive, just logs)
    #[arg(short = 'B', long)]
    pub auto_branch: bool,

    /// Show detailed operation logs (excluded files, truncation, etc.)
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

/// Available subcommands.
#[derive(Subcommand)]
pub enum Commands {
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Generate and create a pull request
    Pr(PrArgs),
}

/// Arguments for the `pr` subcommand.
#[derive(Parser)]
pub struct PrArgs {
    /// Create PR without confirmation
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Show generated content, don't create PR
    #[arg(short, long)]
    pub dry_run: bool,

    /// Create as draft PR
    #[arg(short = 'D', long)]
    pub draft: bool,

    /// Override base branch (default: auto-detect)
    #[arg(short, long)]
    pub base: Option<String>,

    /// Show detailed operation logs
    #[arg(short = 'v', long)]
    pub verbose: bool,

    /// Override model for this run
    #[arg(short, long)]
    pub model: Option<String>,
}

/// Configuration subcommand actions.
#[derive(Subcommand)]
pub enum ConfigAction {
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
