<div align="center">

# Committer

**AI-powered git commit messages, done right.**

[![Crates.io](https://img.shields.io/crates/v/committer.svg)](https://crates.io/crates/committer)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

[Installation](#installation) • [Quick Start](#quick-start) • [Usage](#usage) • [Configuration](#configuration)

</div>

---

Committer generates conventional commit messages from your staged changes using LLMs via [OpenRouter](https://openrouter.ai). It streams responses in real-time, handles large diffs intelligently, and stays out of your way.

```bash
$ git add .
$ committer
✓ feat(auth): add JWT token refresh on expiration
```

## Features

- **Conventional commits** — Generates properly formatted `type(scope): description` messages
- **Streaming output** — Watch messages generate token-by-token
- **Branch intelligence** — Warns when committing directly to protected branches
- **Pull request generation** — Create PRs with AI-generated titles and descriptions
- **Smart diff filtering** — Excludes lock files, build artifacts, and minified code
- **Interactive editing** — Review and edit messages before committing
- **Model flexibility** — Use any model available on OpenRouter

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

### Generating commits

```bash
# Generate message for staged changes
committer

# Stage all changes and generate
committer -a

# Skip confirmation and commit immediately
committer -y

# Preview without committing
committer -d

# Use a specific model
committer -m anthropic/claude-sonnet-4
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

Configuration is stored at `~/.config/committer/config.toml`.

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
2. **Truncates if needed** — Large diffs are intelligently trimmed to 300KB
3. **Streams to LLM** — Sends diff with commit conventions to your chosen model
4. **Prompts for confirmation** — Review, edit, or cancel before committing

Use `--verbose` to see exactly what's being sent and processed.

## Requirements

- Git
- [OpenRouter API key](https://openrouter.ai/keys)
- [GitHub CLI](https://cli.github.com/) (for PR generation only)

## License

MIT © [Nolan Neff](https://github.com/nolanneff)
