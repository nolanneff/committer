<div align="center">

# Committer

**AI-powered commits, branches, and pull requests.**

[![Crates.io](https://img.shields.io/crates/v/committer.svg)](https://crates.io/crates/committer)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

[Installation](#installation) • [Quick Start](#quick-start) • [Usage](#usage) • [Configuration](#configuration)

</div>

---

A fast, lightweight CLI that automates your git workflow. Generate commit messages, detect branch misalignment, create feature branches, and open pull requests—all powered by AI.

```bash
$ git add .
$ committer
✓ feat(auth): add JWT token refresh on expiration
```

## Why Committer?

- **Beyond commit messages** — Also handles branch creation and pull requests
- **Branch intelligence** — Detects when changes don't belong on your current branch
- **Fast** — Written in Rust, starts instantly, streams responses in real-time
- **Zero config** — Works immediately after install
- **Fully automatic or interactive** — Your choice

## What It Does

### Commit Messages

Generate conventional commits from your staged changes:

```bash
$ committer
✓ fix(api): handle timeout errors in retry logic
```

### Branch Detection

Catch mistakes before they happen. Committer analyzes your changes and warns if they don't match your current branch:

```bash
$ committer -b
⚠ These changes look like authentication work, but you're on main
→ Create branch feat/auth-jwt-refresh? [y/n/e]
```

### Pull Requests

Generate PR titles and descriptions from your commits, then create the PR:

```bash
$ committer pr
✓ Title: Add JWT token refresh on expiration
✓ Description: [generated from commits]
→ Create PR? [y/n/e]
```

## Features

- **Conventional commits** — Properly formatted `type(scope): description` messages
- **Real-time streaming** — Watch messages generate token-by-token
- **Smart diff filtering** — Automatically excludes lock files, build artifacts, minified code
- **Large diff handling** — Intelligently truncates at 300KB to stay within limits
- **Any model** — Use Claude, GPT-4, Gemini, Llama, or any model on OpenRouter

## Installation

### From crates.io

```bash
cargo install committer
```

### From source

```bash
git clone https://github.com/nolanneff/committer.git
cd committer
cargo install --path .
```

### Pre-built binaries

Download from the [releases page](https://github.com/nolanneff/committer/releases).

## Quick Start

1. **Get an API key** from [OpenRouter](https://openrouter.ai/keys)

2. **Set your API key:**
   ```bash
   export OPENROUTER_API_KEY="sk-or-..."
   ```

   Add to your shell profile (`~/.bashrc`, `~/.zshrc`) to persist across sessions.

3. **Generate your first commit:**
   ```bash
   git add .
   committer
   ```

## Usage

### Commits

```bash
committer              # Generate message, prompt for confirmation
committer -a           # Stage all changes first
committer -y           # Skip confirmation, commit immediately
committer -ay          # Stage all + auto-commit (fully automatic)
committer -d           # Dry run, preview message only
committer -m <model>   # Use a specific model
```

### Branches

```bash
committer -b           # Analyze branch alignment, prompt to create
committer -B           # Auto-create suggested branches
```

### Pull Requests

```bash
committer pr           # Create PR with AI-generated title/description
committer pr --draft   # Create as draft
committer pr -d        # Preview without creating
```

**Requires:** [GitHub CLI](https://cli.github.com/) (`gh auth login`)

## Configuration

Configuration is **optional**. Committer works out of the box with sensible defaults. Customize only what you need.

Config file: `~/.config/committer/config.toml`

### Commands

```bash
committer config show              # View current settings
committer config model <model>     # Set default model
committer config auto-commit true  # Skip confirmations
committer config verbose true      # Enable debug output
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `model` | `google/gemini-2.0-flash-001` | Default model |
| `auto_commit` | `false` | Skip confirmation prompts |
| `verbose` | `false` | Show detailed logs |

### Environment variables

- `OPENROUTER_API_KEY` — API key (required)

## CLI Reference

### `committer` (commit)

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Commit without confirmation |
| `--dry-run` | `-d` | Preview message only |
| `--all` | `-a` | Stage all changes first |
| `--model` | `-m` | Override default model |
| `--branch` | `-b` | Analyze branch alignment |
| `--auto-branch` | `-B` | Auto-create feature branches |
| `--verbose` | `-v` | Show debug output |

### `committer pr`

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Create without confirmation |
| `--dry-run` | `-d` | Preview PR only |
| `--draft` | `-D` | Create as draft |
| `--base` | `-b` | Override base branch |
| `--model` | `-m` | Override default model |
| `--verbose` | `-v` | Show debug output |

## How it works

1. **Reads staged diff** — Filters out noise (lock files, build artifacts, minified code)
2. **Sends to LLM** — Streams your diff to OpenRouter with commit conventions
3. **Returns message** — Displays the result, optionally prompts for confirmation
4. **Commits** — Runs `git commit` with the generated message

Startup is instant—no interpreter, no JIT warmup. Use `--verbose` to see exactly what's happening.

## Requirements

- Git
- [OpenRouter API key](https://openrouter.ai/keys) (free tier available)
- [GitHub CLI](https://cli.github.com/) (only for `committer pr`)

No Node.js. No Python. No Docker. Just a single binary.

## License

MIT © [Nolan Neff](https://github.com/nolanneff)
