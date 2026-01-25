<div align="center">

# Committer

**Lightweight, terminal-native AI commit messages.**

[![Crates.io](https://img.shields.io/crates/v/committer.svg)](https://crates.io/crates/committer)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

[Installation](#installation) • [Quick Start](#quick-start) • [Usage](#usage) • [Configuration](#configuration)

</div>

---

A single binary that generates conventional commit messages from your staged changes. Zero config required—just run `committer` and you're done. Customize everything when you need to.

```bash
$ git add .
$ committer
✓ feat(auth): add JWT token refresh on expiration
```

## Why Committer?

- **Single binary** — One ~4MB executable, no runtime dependencies
- **Zero config** — Works immediately after install, sensible defaults
- **Fully automatic** — Run `committer -ay` and walk away
- **Or fully interactive** — Review, edit, and approve every message
- **Terminal-native** — Streaming output, no GUI, no browser, no electron
- **Customizable** — Change models, auto-commit behavior, verbosity when needed

## Features

- **Conventional commits** — Properly formatted `type(scope): description` messages
- **Real-time streaming** — Watch messages generate token-by-token
- **Branch protection** — Warns before committing to main/master/develop
- **Pull request generation** — Create PRs with AI-generated titles and descriptions
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

2. **Set up authentication:**
   ```bash
   export OPENROUTER_API_KEY="sk-or-..."
   ```

   Or save it permanently:
   ```bash
   committer config api-key sk-or-...
   ```

3. **Generate your first commit:**
   ```bash
   git add .
   committer
   ```

## Usage

### Fully automatic

Stage and commit in one command, no prompts:

```bash
committer -ay
```

That's it. Your changes are committed with an AI-generated message.

### Interactive (default)

Review and optionally edit before committing:

```bash
committer
```

You'll see the generated message and can accept, edit, or cancel.

### All options

```bash
committer              # Generate message, prompt for confirmation
committer -a           # Stage all changes first
committer -y           # Skip confirmation, commit immediately
committer -ay          # Stage all + auto-commit (fully automatic)
committer -d           # Dry run, preview message only
committer -m <model>   # Use a specific model
```

### Creating pull requests

```bash
# Create a PR with AI-generated title and description
committer pr

# Create as draft
committer pr --draft

# Preview without creating
committer pr -d
```

The PR command automatically:
- Detects the base branch
- Commits any staged changes
- Pushes the branch
- Generates title and description from your commits

**Requires:** [GitHub CLI](https://cli.github.com/) (`gh auth login`)

### Branch protection

Committer warns you when committing to protected branches (`main`, `master`, `develop`, `production`) and suggests creating a feature branch:

```bash
# Enable branch analysis
committer -b

# Automatically create suggested branches
committer -B
```

## Configuration

Configuration is **optional**. Committer works out of the box with sensible defaults. Customize only what you need.

Config file: `~/.config/committer/config.toml`

### Commands

```bash
committer config show              # View current settings
committer config api-key <key>     # Set API key
committer config model <model>     # Set default model
committer config auto-commit true  # Skip confirmations
committer config verbose true      # Enable debug output
```

### Options

| Option | Default | Description |
|--------|---------|-------------|
| `api_key` | — | OpenRouter API key |
| `model` | `google/gemini-2.0-flash-001` | Default model |
| `auto_commit` | `false` | Skip confirmation prompts |
| `verbose` | `false` | Show detailed logs |

### Environment variables

- `OPENROUTER_API_KEY` — API key (takes precedence over config file)

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

The entire process takes ~2 seconds with fast models. Use `--verbose` to see exactly what's happening.

## Requirements

- Git
- [OpenRouter API key](https://openrouter.ai/keys) (free tier available)
- [GitHub CLI](https://cli.github.com/) (only for `committer pr`)

No Node.js. No Python. No Docker. Just a single binary.

## License

MIT © [Nolan Neff](https://github.com/nolanneff)
