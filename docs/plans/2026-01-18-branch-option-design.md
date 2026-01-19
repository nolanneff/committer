# Add Branch Creation Option to Commit Prompt

## Summary

Add a `b` option to the commit confirmation prompt (`Commit? [y/n/e/b]`) that allows users to manually trigger branch creation before committing.

## User Experience

**New prompt:** `Commit? [y/n/e/b]`

When user presses `b`:

```
Commit? [y/n/e/b] b

⏳ Generating branch name...

🌿 Suggested branch: feat/diarization-accuracy-example

Create branch? [y/n/e] y

✓ Created and switched to branch: feat/diarization-accuracy-example

Commit? [y/n/e] _
```

**Key behaviors:**
- The `b` option only appears on the initial prompt
- After branch flow completes (create, edit, or skip), prompt returns as `[y/n/e]`
- If user skips branch creation, they return to `[y/n/e]` without creating a branch
- Invalid input shows: `Please enter y, n, e, or b`

## Implementation

### Files to modify

`src/main.rs`

### 1. Update `CommitAction` enum (lines 844-847)

```rust
enum CommitAction {
    Commit(String),
    Cancel,
    CreateBranch(String),  // NEW - carries the current message
}
```

### 2. Update `prompt_commit()` function (lines 849-875)

- Add `show_branch_option: bool` parameter
- Change prompt text based on parameter: `[y/n/e/b]` or `[y/n/e]`
- Add `b` match arm returning `CommitAction::CreateBranch(current_message)`
- Update invalid input message accordingly

### 3. Add new function: `generate_branch_suggestion()`

```rust
async fn generate_branch_suggestion(
    client: &Client,
    api_key: &str,
    model: &str,
    commit_message: &str,
) -> String
```

**AI Prompt:**
```
Given this commit message, suggest an appropriate git branch name.

COMMIT MESSAGE:
{commit_message}

BRANCH NAMING RULES:
1. Use format: <type>/<scope>-<short-description>
2. Type should match the commit type (feat, fix, docs, refactor, test, chore, etc.)
3. Scope is the area/module being changed (auth, ui, server, api, etc.)
4. Description should be kebab-case, concise (2-4 words)
5. Keep the full branch name under 50 characters when possible

BRANCH NAMING CONVENTION: <type>/<scope>-<short-description>
Examples: feat/auth-refresh-token, fix/ui-chat-scroll, refactor/server-ws-reconnect

Respond with ONLY the branch name, nothing else.
```

**Fallback:** If AI fails, use existing `generate_fallback_branch()`.

### 4. Update main flow (~line 1063)

```rust
// Initial prompt with branch option
let mut show_branch_option = true;

loop {
    match prompt_commit(&message, show_branch_option) {
        CommitAction::Commit(msg) => {
            // Execute commit
            break;
        }
        CommitAction::Cancel => {
            // Exit
            break;
        }
        CommitAction::CreateBranch(msg) => {
            // Generate branch suggestion
            let suggested = generate_branch_suggestion(...).await
                .unwrap_or_else(|| generate_fallback_branch(&msg));

            println!("🌿 Suggested branch: {}", suggested);

            // Use existing prompt_branch_action()
            match prompt_branch_action(&current_branch, &suggested, "") {
                BranchAction::Create(name) => {
                    // Create and switch to branch
                    create_and_switch_branch(&name);
                }
                BranchAction::Skip => {
                    // Continue without branch
                }
            }

            // Disable branch option for next iteration
            show_branch_option = false;
            // Loop continues to show commit prompt again
        }
    }
}
```

## Not in Scope

- Changing the existing `--branch` or `--auto-branch` flag behavior
- Adding branch analysis (checking if commit "belongs" on current branch)
- Modifying the branch creation prompt itself (`Create branch? [y/n/e]`)
