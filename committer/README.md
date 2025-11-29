# committer

A fast, AI-powered git commit message generator using OpenRouter.

## Features

- 🚀 **Fast** - Streams commit messages token-by-token as they generate
- 🤖 **AI-Powered** - Uses OpenRouter to access various LLMs
- ⚡ **Async** - Parallel git operations and non-blocking I/O
- 🔧 **Configurable** - Persistent settings for auto-commit and model selection

## Installation

```bash
# Clone and build
git clone <repo>
cd committer
cargo build --release

# Install to PATH
cargo install --path .
```

## Setup

Set your OpenRouter API key:

```bash
# Option 1: Environment variable (recommended)
export OPENROUTER_API_KEY="sk-or-..."

# Option 2: Store in config file
committer config api-key sk-or-...
```

## Usage

### Basic Usage

```bash
# Generate commit message for staged changes
committer

# Include all changes (stages everything first)
committer --all

# Auto-commit without confirmation
committer --yes

# Just print the message, don't commit
committer --dry-run

# Use a different model for this run
committer --model anthropic/claude-sonnet-4
```

### Configuration

```bash
# Show current config
committer config show

# Toggle auto-commit (skip confirmation prompt)
committer config auto-commit true
committer config auto-commit false

# Change default model
committer config model anthropic/claude-sonnet-4

# Store API key in config
committer config api-key sk-or-...
```

### Config File

Located at `~/.config/committer/config.toml`:

```toml
auto_commit = false
model = "x-ai/grok-4.1-fast:free"
# api_key = "sk-or-..."  # Optional, env var takes precedence
```

## Workflow

1. Make your code changes
2. Stage them with `git add` (or use `committer --all`)
3. Run `committer`
4. Watch the AI-generated message stream in real-time
5. Confirm to commit, or press N to cancel

## Default Model

The default model is `x-ai/grok-4.1-fast:free` - a fast, free model via OpenRouter.

You can change this permanently with:
```bash
committer config model your-preferred-model
```

Or per-invocation with:
```bash
committer --model your-preferred-model
```

## Options

| Flag | Short | Description |
|------|-------|-------------|
| `--yes` | `-y` | Auto-commit without asking |
| `--dry-run` | `-d` | Just print message, don't commit |
| `--all` | `-a` | Stage all changes before generating |
| `--model` | `-m` | Override model for this run |

## License

MIT
