# Committer

[![Version](https://img.shields.io/badge/version-0.1.0-blue.svg)](https://github.com/your-repo/committer)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.56%2B-orange.svg)](https://www.rust-lang.org/)

A fast, AI-powered git commit message generator using OpenRouter.

## Features

- **AI-Powered Messages** - Generates conventional commit messages using LLMs via OpenRouter
- **Real-Time Streaming** - Watch commit messages generate token-by-token
- **Branch Intelligence** - Detects branch misalignment and suggests feature branches
- **Interactive Editing** - Edit generated messages and branch names before accepting
- **Smart Diff Handling** - Automatically excludes lock files, build artifacts, and minified code
- **Configurable** - Persistent settings for model selection, auto-commit, and more

## Quick Start

```bash
# Install
cargo install --path .

# Set API key
export OPENROUTER_API_KEY="sk-or-..."

# Generate a commit message
git add .
committer
```

## Installation

```bash
git clone <repo>
cd committer
cargo build --release
cargo install --path .
```

## Configuration

### API Key

```bash
# Environment variable (recommended)
export OPENROUTER_API_KEY="sk-or-..."

# Or store in config file
committer config api-key sk-or-...
```

### Config File

Located at `~/.config/committer/config.toml`:

```toml
auto_commit = false
model = "google/gemini-2.0-flash-001"
verbose = false
# api_key = "sk-or-..."  # Optional, env var takes precedence
```

### Config Commands

```bash
committer config show                    # Show current config
committer config auto-commit true        # Skip confirmation prompts
committer config model <model-name>      # Set default model
committer config verbose true            # Enable detailed logging
```

## Usage

### Basic Usage

```bash
# Generate message for staged changes
committer

# Stage all changes and generate
committer --all

# Auto-commit without confirmation
committer --yes

# Preview message without committing
committer --dry-run

# Use a different model
committer --model anthropic/claude-sonnet-4
```

### Branch Analysis

Committer can analyze whether your changes belong on the current branch and suggest creating a feature branch instead.

```bash
# Interactive branch suggestion
committer --branch

# Auto-create suggested branches
committer --auto-branch
```

Protected branches (main, master, develop, production) always trigger a branch suggestion when direct commits are detected.

When prompted, you can:
- `y` - Create the suggested branch
- `n` - Continue on current branch
- `e` - Edit the branch name before creating

## CLI Reference

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Auto-commit without confirmation |
| `--dry-run` | `-d` | Print message without committing |
| `--all` | `-a` | Stage all changes before generating |
| `--model` | `-m` | Override model for this run |
| `--branch` | `-b` | Enable interactive branch analysis |
| `--auto-branch` | `-B` | Auto-create misaligned branches |
| `--verbose` | `-v` | Show detailed operation logs |

## Smart Diff Handling

Committer automatically filters diffs to focus on meaningful changes:

**Excluded files:**
- Lock files: `Cargo.lock`, `package-lock.json`, `yarn.lock`, `poetry.lock`, etc.
- Minified code: `.min.js`, `.min.css`, `.map` files
- Build directories: `target/`, `node_modules/`, `dist/`, `build/`, `.next/`, `__pycache__/`

**Size limits:**
- Large diffs are intelligently truncated at 300KB to stay within token limits
- File headers and recent changes are preserved for context

Use `--verbose` to see what files are being excluded and how the diff is processed.

## Default Model

The default model is `google/gemini-2.0-flash-001`, a fast and capable model via OpenRouter.

Change permanently:
```bash
committer config model your-preferred-model
```

Override per-run:
```bash
committer --model your-preferred-model
```

## Requirements

- Rust 1.56+ (2021 edition)
- Git
- OpenRouter API key ([get one here](https://openrouter.ai))

## License

MIT
