# committer

A fast, AI-powered git commit message generator using OpenRouter.

## Features

- 🚀 **Fast** - Streams commit messages token-by-token as they generate
- 🤖 **AI-Powered** - Uses OpenRouter to access various LLMs
- ⚡ **Async** - Parallel git operations and non-blocking I/O
- 🔧 **Configurable** - Persistent settings for auto-commit, model selection, and message formats
- 📝 **Flexible Formats** - Multiple built-in commit message styles + custom templates
- 🔄 **Interactive** - Retry generation or edit messages before committing

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

# Use a different format for this run
committer --format gitmoji

# Add extra instructions for this run
committer --instructions "mention the ticket number PROJ-123"
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

## Commit Message Formats

Committer supports multiple commit message format presets. View all available formats with examples:

```bash
committer config formats
```

### Available Formats

| Format | Description | Example |
|--------|-------------|---------|
| `conventional` | Conventional Commits (default) | `feat(auth): add OAuth2 login support` |
| `simple` | Clean single-line messages | `Add user authentication` |
| `gitmoji` | Emoji-prefixed messages | `✨ Add dark mode toggle` |
| `detailed` | Multi-paragraph with body | Subject + body explaining what/why |
| `imperative` | Minimal verb-first style | `Add caching layer` |
| `custom` | Your own template | Whatever you define |

### Setting Your Preferred Format

```bash
# Set default format (persisted)
committer config format conventional
committer config format gitmoji
committer config format simple

# Override for a single run
committer --format detailed
committer -f imperative
```

### Extra Instructions

Add custom rules that apply to any format:

```bash
# Set persistent extra instructions
committer config instructions "Always mention the component being changed"
committer config instructions "Reference Jira tickets when applicable"

# Clear extra instructions
committer config instructions clear

# Add instructions for a single run only
committer --instructions "This is a hotfix for production"
committer -i "Keep the message under 50 chars"
```

### Custom Templates

For complete control, define your own prompt template:

```bash
# Set a custom template
committer config template "Generate a commit message that starts with an action verb, keeps the first line under 50 chars, and includes a brief explanation on a second line."

# Activate custom format
committer config format custom
```

### Config File

Located at `~/.config/committer/config.toml`:

```toml
auto_commit = false
model = "x-ai/grok-4.1-fast:free"
format = "conventional"
# api_key = "sk-or-..."  # Optional, env var takes precedence
# extra_instructions = "Always mention the module name"
# custom_template = "Your custom prompt here..."
```

## Workflow

1. Make your code changes
2. Stage them with `git add` (or use `committer --all`)
3. Run `committer`
4. Watch the AI-generated message stream in real-time
5. Choose from the interactive prompt:
   - **y** - Commit with this message
   - **n** - Cancel and exit
   - **r** - Regenerate a new message
   - **e** - Edit the message in your `$EDITOR`

### Interactive Options

After generating a commit message, you'll see:

```
Commit with this message? [y]es / [n]o / [r]etry / [e]dit:
```

| Option | Description |
|--------|-------------|
| `y` / `yes` | Accept the message and create the commit |
| `n` / `no` | Cancel without committing |
| `r` / `retry` | Generate a new message (useful if the AI missed something) |
| `e` / `edit` | Open the message in your editor for manual tweaking |

The edit option uses `$EDITOR` or `$VISUAL` environment variables, falling back to `vim`, `vi`, or `nano`.

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
| `--format` | `-f` | Override commit format for this run |
| `--instructions` | `-i` | Add extra instructions for this run |

## Config Commands

| Command | Description |
|---------|-------------|
| `config show` | Show current configuration |
| `config auto-commit <bool>` | Set auto-commit behavior |
| `config model <name>` | Set default model |
| `config api-key <key>` | Store API key in config |
| `config format <name>` | Set commit message format preset |
| `config template <text>` | Set custom template (for `custom` format) |
| `config instructions <text>` | Set extra instructions (use "clear" to remove) |
| `config formats` | Show all available formats with examples |

## License

MIT
