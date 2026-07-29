# Design: `committer clean` - AI-Powered Branch Cleanup

> **Status:** Superseded. The shipped command uses Git ancestry and exact merged-PR head SHAs to clean local branches safely. AI grouping, remote deletion, and branch combining were deliberately deferred.

## Overview

A command that analyzes all local branches using AI, groups related work, and provides interactive prompts to clean up merged branches and consolidate unmerged work.

```bash
committer clean          # Interactive branch cleanup
committer clean --dry-run  # Show analysis only, no actions
```

## Problem

When using committer's branch creation features, repositories accumulate many branches. These become hard to manage:
- Merged branches linger as clutter
- Related work is spread across multiple branches
- No easy way to understand what branches relate to what features

## Solution

AI-powered analysis that:
1. Groups related branches by feature/purpose
2. Safely deletes merged branches (local + remote)
3. Offers to combine related unmerged branches before merging to main

## User Flow

### Step 1: Gather Data

Collect for each local branch:
- Branch name
- Merge status (is it in main?)
- Remote tracking status
- Last commit date and message

### Step 2: AI Analysis

Send branch metadata to AI. Returns:
- Logical groupings with labels
- Brief reasoning for each group
- Separation of merged vs unmerged

### Step 3: Handle Merged Branches

```
committer clean

Analyzing 23 branches...

🧹 Merged branches (12 branches):

  📦 "Branch detection feature"
     • feat/core-branch-detection-strategies
     • feat/cli-auto-branching
     • feat/auto-branch

  📦 "Documentation updates"
     • docs/core-simplify-readme
     • docs/ui-brand-assets

  📦 "Bug fixes"
     • fix/cli-spinner-stability

Delete all 12 branches (local + remote)? [y]es / [n]o / [s]elect groups
```

User can delete all, none, or select specific groups.

### Step 4: Handle Unmerged Branches

```
📦 Related unmerged work: "Config improvements"
   • chore/config-project-cleanup
   • chore/core-update-ai-provider

   [c]ombine into new branch / [d]elete anyway / [s]kip

   Branch name: feat/config-improvements
   Combine and delete originals (local + remote)? [y/n]
```

### Combining Branches

When user selects combine:

1. Create new branch from main
2. Cherry-pick commits from each related branch (chronological order)
3. If conflicts, prompt to resolve or abort
4. Delete original branches (local + remote) after success

```
Combining 3 branches into feat/config-improvements...

✓ Cherry-picked: chore/config-project-cleanup (2 commits)
✓ Cherry-picked: chore/core-update-ai-provider (1 commit)
⚠ Conflict in: feat/config-commit-after-branch

  [r]esolve manually / [a]bort combine / [s]kip this branch
```

## Edge Cases

| Scenario | Behavior |
|----------|----------|
| Branch has no remote | Delete local only, skip remote |
| Remote already deleted | Delete local, note remote was gone |
| Conflicts during combine | Prompt to resolve, skip, or abort |
| Protected branch (main/master) | Never include in deletion list |
| Current branch in list | Switch to main first, then delete |

## Implementation

### New Files

- `src/clean.rs` - Branch analysis, grouping, deletion, combining logic

### Modified Files

- `src/cli.rs` - Add `Clean` subcommand with `--dry-run` flag
- `src/main.rs` - Route to clean handler

### Key Functions

```rust
// Gather branch metadata
async fn get_all_branches() -> Vec<BranchInfo>

// Check if branch is merged into target
async fn is_branch_merged(branch: &str, target: &str) -> bool

// AI analysis for grouping
async fn analyze_branches(branches: &[BranchInfo]) -> Vec<BranchGroup>

// Delete branch locally and remotely
async fn delete_branch(branch: &str, delete_remote: bool) -> Result<()>

// Combine multiple branches into one
async fn combine_branches(branches: &[&str], new_name: &str) -> Result<()>
```

### AI Prompt

Send to AI:
- Branch names
- Last commit message for each
- Age (days since last commit)
- Merge status

Ask AI to return JSON:
```json
{
  "groups": [
    {
      "label": "Branch detection feature",
      "reason": "All related to detecting and suggesting branches",
      "branches": ["feat/auto-branch", "feat/cli-auto-branching"]
    }
  ]
}
```

## Estimated Scope

- ~300-400 lines of new code
- Similar complexity to PR generation feature
- No new dependencies required

## Future Enhancements

- `--force` flag to skip confirmations
- Config option to auto-delete merged branches
- Integration with GitHub API to check PR status
