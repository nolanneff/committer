<div align="center">

<img src="assets/title.png" alt="Committer" width="400">

**AI-powered commits, branches, and pull requests.**

[![Crates.io](https://img.shields.io/crates/v/committer.svg)](https://crates.io/crates/committer)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-windows%20%7C%20macos%20%7C%20linux-blue)](https://github.com/nolanneff/committer/releases)

[Installation](#installation) • [Quick Start](#quick-start) • [Usage](#usage) • [Configuration](#configuration)

</div>

---

A fast, Simple, lightweight CLI that automates your git workflow. Generate commit messages, detect branch misalignment, create feature branches, and open pull requests—all powered by AI.

Most AI commit tools are built on Node.js or Python, adding noticeable startup delay to every invocation. Committer is a native binary—it launches instantly and streams responses in real-time, so you're never waiting on the tool itself.

---

## What It Does

### Commit Messages

Generate conventional commits from your staged changes:

<p align="center">
<img src="assets/demos/commit.gif" alt="Committer generating a commit message">
</p>

### Branch Detection

Catch mistakes before they happen. Committer analyzes your changes and warns if they don't match your current branch:

<p align="center">
<img src="assets/demos/branch.gif" alt="Committer detecting branch misalignment">
</p>

### Pull Requests

Generate PR titles and descriptions from your commits, then create the PR:

<p align="center">
<img src="assets/demos/pr.gif" alt="Committer creating a pull request">
</p>

## Features

- **Conventional commits** — Properly formatted `type(scope): description` messages
- **Fast** — Starts instantly, streams responses in real-time
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

## Requirements

- Git
- [OpenRouter API key](https://openrouter.ai/keys) (free tier available)
- [GitHub CLI](https://cli.github.com/) (only for `committer pr`)

## Roadmap

This project is under active development. Planned features:

- [ ] Custom commit message formatting (templates, scopes, styles)
- [ ] More configuration options

## Contributing

1. Fork the repo
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Commit using conventional commits (Could use committer 🙂)
4. Open a PR against `main`

## License

MIT © [Nolan Neff](https://github.com/nolanneff)



