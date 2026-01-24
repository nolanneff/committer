# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Committer is a fast, AI-powered git commit message generator written in Rust. It uses OpenRouter API to generate conventional commit messages from staged changes, featuring streaming output, branch intelligence, and smart diff filtering.

## Build Commands

```bash
cargo build --release    # Build optimized binary
cargo install --path .   # Install to ~/.cargo/bin
cargo run -- --dry-run   # Test run without committing
```

## Architecture

**Single-file monolith**: All code lives in `src/main.rs` (~1,300 lines), organized into sections:

1. **Configuration** (lines 12-99) - TOML config at `~/.config/committer/config.toml`, env var support
2. **CLI Interface** (lines 101-175) - Clap-based argument parsing with subcommands
3. **Git Operations** (lines 176-459) - Async git commands via `tokio::process::Command`
4. **Diff Processing** (lines 182-349) - Filtering, truncation at 300KB, noise removal
5. **AI Integration** (lines 600-827) - OpenRouter streaming API calls
6. **Branch Intelligence** (lines 845-991) - Alignment analysis, protected branch detection
7. **User Interaction** (lines 996-1044) - Interactive prompts with edit capability
8. **Main Flow** (lines 1050-1315) - Orchestration of all components

## Key Constants

```rust
DEFAULT_MODEL = "google/gemini-3-flash-preview"
OPENROUTER_API_URL = "https://openrouter.ai/api/v1/chat/completions"
MAX_DIFF_CHARS = 300_000
PROTECTED_BRANCHES = [main, master, develop, dev, staging, production]
```

## Configuration

- **File**: `~/.config/committer/config.toml`
- **API Key**: `OPENROUTER_API_KEY` env var (required)
- **Options**: `auto_commit`, `commit_after_branch`, `model`, `verbose`

## Commit Message Convention

Format: `<type>(<scope>): <description>`
- Types: feat, fix, refactor, chore, docs, ci, config, test, perf, style, build, deps, revert
- Scopes: cli, core, config, docs
- Style: lowercase, imperative mood, concise

## Testing

No test suite exists. Manual testing via CLI:
```bash
export OPENROUTER_API_KEY="sk-or-..."
git add .
cargo run -- --dry-run --verbose
```

## Important Patterns

- All async operations use Tokio runtime
- Git commands spawn via `tokio::process::Command` with output parsing
- Streaming HTTP responses parsed token-by-token with fallback to non-streaming
- Diff filtering excludes lock files, minified code, build artifacts
- Branch analysis uses LLM to detect scope misalignment
