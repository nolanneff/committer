# Module Refactor Design

**Date:** 2026-01-24
**Status:** Approved
**Goal:** Refactor `src/main.rs` (~2185 lines) into flat modules for maintainability, testability, and reduced merge conflicts.

## Module Structure

```
src/
  main.rs      (~50 lines)   Entry point, CLI parsing, command dispatch
  config.rs    (~90 lines)   Config struct, TOML load/save, defaults
  cli.rs       (~80 lines)   Clap structs (Cli, Commands, PrArgs, ConfigAction)
  git.rs       (~450 lines)  All git operations (diff, status, branch, commit)
  api.rs       (~400 lines)  OpenRouter client, streaming, prompts
  branch.rs    (~200 lines)  Branch analysis, alignment check, slugify
  ui.rs        (~150 lines)  User prompts, spinners, action enums
  pr.rs        (~190 lines)  PR command handler
```

## Module Responsibilities

### `config.rs`
- `Config` struct with serde derives
- `config_path()`, `load_config()`, `save_config()`
- `default_model()` helper

### `cli.rs`
- `Cli` struct with clap derives
- `Commands` enum (Config, Pr)
- `PrArgs`, `ConfigAction` structs

### `git.rs`
- Constants: `EXCLUDED_FROM_DIFF`
- Diff: `get_git_diff()`, `filter_excluded_diffs()`, `truncate_diff()`, `should_exclude_from_diff()`, `extract_filename_from_diff_header()`
- Status: `get_staged_files()`, `get_uncommitted_changes()`, `UncommittedChanges`
- Branch: `get_current_branch()`, `create_and_switch_branch()`, `get_recent_commits()`
- Commit: `run_git_commit()`, `stage_all_changes()`
- PR helpers: `get_branch_diff()`, `get_branch_commits()`, `get_pr_changed_files()`
- Push: `branch_needs_push()`, `push_branch_with_spinner()`

### `api.rs`
- Constants: `DEFAULT_MODEL`, `OPENROUTER_API_URL`, `MAX_DIFF_CHARS`
- Request/response types: `ChatRequest`, `Message`, `StreamChunk`, `NonStreamResponse`, etc.
- Prompt builders: `build_prompt()`, `build_pr_prompt()`
- Streaming: `stream_commit_message()`, `stream_pr_content()`
- `get_api_key()`

### `branch.rs`
- Constants: `PROTECTED_BRANCHES`, `FILLER_WORDS`
- Types: `BranchAnalysis`, `BranchAction`
- Functions: `slugify()`, `generate_fallback_branch()`, `analyze_branch_alignment()`, `generate_branch_suggestion()`

### `ui.rs`
- Types: `CommitAction`, `PrAction`, `UncommittedAction`
- Prompts: `prompt_commit()`, `prompt_pr()`, `prompt_branch_action()`, `prompt_uncommitted_changes()`
- Spinner helpers (if extracted)

### `pr.rs`
- `handle_pr_command()` - the entire PR subcommand flow
- GitHub CLI helpers: `check_gh_installed()`, `get_default_base_branch()`, `get_upstream_remote()`, `branch_has_merge_base()`, `get_cached_remote_head()`, `get_remote_default_branch()`, `create_pr()`

### `main.rs`
- `#[tokio::main]` entry point
- CLI parsing via clap
- Config subcommand handling (inline, simple)
- Commit flow orchestration
- Dispatch to `handle_pr_command()` for PR subcommand

## Dependency Flow

```
main.rs ─────┬──→ config.rs
             ├──→ cli.rs
             ├──→ git.rs
             ├──→ api.rs
             ├──→ branch.rs
             ├──→ ui.rs
             └──→ pr.rs

pr.rs ───────┬──→ config.rs
             ├──→ git.rs
             ├──→ api.rs
             └──→ ui.rs

branch.rs ───┴──→ api.rs
```

## Shared Types

| Type | Defined in | Used by |
|------|-----------|---------|
| `Config` | `config.rs` | `main.rs`, `pr.rs` |
| `Cli`, `Commands`, `PrArgs` | `cli.rs` | `main.rs`, `pr.rs` |
| `BranchAnalysis`, `BranchAction` | `branch.rs` | `main.rs` |
| `CommitAction`, `PrAction`, `UncommittedAction` | `ui.rs` | `main.rs`, `pr.rs` |
| `UncommittedChanges` | `git.rs` | `pr.rs`, `ui.rs` |

## Migration Steps

1. Create empty module files
2. Migrate one module at a time (config → cli → git → api → branch → ui → pr → main cleanup)
3. For each module:
   - Cut section from `main.rs`
   - Paste into new file
   - Add `pub` visibility as needed
   - Add `mod` and `use` statements to `main.rs`
   - `cargo build` to verify
   - Commit with committer
4. Final cleanup and full test

## Testing Strategy

Unit tests can be added to each module for pure functions:

- `config.rs`: `Config::default()` values
- `git.rs`: `should_exclude_from_diff()`, `filter_excluded_diffs()`, `truncate_diff()`
- `branch.rs`: `slugify()`, `generate_fallback_branch()`

Async functions and API calls remain integration-test territory.

## Notes

- Use `committer` for all commits during migration
- One commit per module keeps history clean
- No logic changes - pure restructuring
