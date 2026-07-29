//! PR-aware cleanup of merged local branches.

use console::style;
use serde::Deserialize;
use std::collections::HashSet;
use std::io::{self, Write};
use tokio::process::Command;

use crate::branch::PROTECTED_BRANCHES;
use crate::cli::CleanArgs;
use crate::git::{check_git_installed, get_current_branch};

#[derive(Clone, Debug, PartialEq, Eq)]
struct BranchInfo {
    name: String,
    head_oid: String,
    upstream: Option<String>,
    upstream_gone: bool,
    worktree_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DeleteReason {
    GitMerged,
    MergedPr(u64),
}

impl DeleteReason {
    fn description(&self, base: &str) -> String {
        match self {
            Self::GitMerged => format!("fully merged into {base}"),
            Self::MergedPr(number) => format!("PR #{number} merged into {base}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Candidate {
    branch: BranchInfo,
    reason: DeleteReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum KeepReason {
    Base,
    Protected,
    OtherWorktree,
    DirtyCurrent,
    PrHeadMismatch(u64),
    Unmerged {
        unique_commits: usize,
        upstream_gone: bool,
    },
}

impl KeepReason {
    fn description(&self, base: &str) -> String {
        match self {
            Self::Base => "base branch".to_string(),
            Self::Protected => "protected branch".to_string(),
            Self::OtherWorktree => "checked out in another worktree".to_string(),
            Self::DirtyCurrent => "current branch has uncommitted changes".to_string(),
            Self::PrHeadMismatch(number) => {
                format!("PR #{number} merged, but the local branch has newer or different commits")
            }
            Self::Unmerged {
                unique_commits,
                upstream_gone,
            } => {
                let commits = if *unique_commits == 1 {
                    "1 unique commit".to_string()
                } else {
                    format!("{unique_commits} unique commits")
                };
                if *upstream_gone {
                    format!("{commits} not in {base}; upstream gone")
                } else {
                    format!("{commits} not in {base}")
                }
            }
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct MergedPr {
    number: u64,
    head_ref_name: String,
    head_ref_oid: String,
    base_ref_name: String,
    is_cross_repository: bool,
}

fn parse_branches(output: &str) -> Vec<BranchInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(5, '\t');
            let name = fields.next()?.trim();
            let head_oid = fields.next()?.trim();
            let upstream = fields.next().unwrap_or("").trim();
            let upstream_track = fields.next().unwrap_or("");
            let worktree_path = fields.next().unwrap_or("");

            if name.is_empty() || head_oid.is_empty() {
                return None;
            }

            Some(BranchInfo {
                name: name.to_string(),
                head_oid: head_oid.to_string(),
                upstream: (!upstream.is_empty()).then(|| upstream.to_string()),
                upstream_gone: upstream_track.contains("gone"),
                worktree_path: (!worktree_path.is_empty()).then(|| worktree_path.to_string()),
            })
        })
        .collect()
}

fn parse_merged_prs(output: &[u8]) -> Result<Vec<MergedPr>, serde_json::Error> {
    serde_json::from_slice(output)
}

fn matching_pr<'a>(branch: &BranchInfo, base: &str, prs: &'a [MergedPr]) -> Option<&'a MergedPr> {
    prs.iter().find(|pr| {
        !pr.is_cross_repository
            && pr.base_ref_name == base
            && pr.head_ref_name == branch.name
            && pr.head_ref_oid == branch.head_oid
    })
}

fn mismatched_pr<'a>(branch: &BranchInfo, base: &str, prs: &'a [MergedPr]) -> Option<&'a MergedPr> {
    prs.iter().find(|pr| {
        !pr.is_cross_repository
            && pr.base_ref_name == base
            && pr.head_ref_name == branch.name
            && pr.head_ref_oid != branch.head_oid
    })
}

fn classify_branch(
    branch: &BranchInfo,
    base: &str,
    current: &str,
    current_clean: bool,
    git_merged: bool,
    unique_commits: usize,
    prs: &[MergedPr],
) -> Result<Candidate, KeepReason> {
    if branch.name == base {
        return Err(KeepReason::Base);
    }
    if PROTECTED_BRANCHES.contains(&branch.name.as_str()) {
        return Err(KeepReason::Protected);
    }
    if branch.name != current && branch.worktree_path.is_some() {
        return Err(KeepReason::OtherWorktree);
    }

    let reason = if git_merged {
        Some(DeleteReason::GitMerged)
    } else {
        matching_pr(branch, base, prs).map(|pr| DeleteReason::MergedPr(pr.number))
    };

    if let Some(reason) = reason {
        if branch.name == current && !current_clean {
            return Err(KeepReason::DirtyCurrent);
        }
        return Ok(Candidate {
            branch: branch.clone(),
            reason,
        });
    }

    if let Some(pr) = mismatched_pr(branch, base, prs) {
        return Err(KeepReason::PrHeadMismatch(pr.number));
    }

    Err(KeepReason::Unmerged {
        unique_commits,
        upstream_gone: branch.upstream_gone,
    })
}

async fn local_branch_exists(branch: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let ref_name = format!("refs/heads/{branch}");
    let status = Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &ref_name])
        .status()
        .await?;
    Ok(status.success())
}

async fn resolve_base_branch() -> Result<&'static str, Box<dyn std::error::Error>> {
    for branch in ["main", "master"] {
        if local_branch_exists(branch).await? {
            return Ok(branch);
        }
    }

    Err("No local main or master branch found".into())
}

async fn all_branches() -> Result<Vec<BranchInfo>, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)%09%(objectname)%09%(upstream:short)%09%(upstream:track)%09%(worktreepath)",
            "refs/heads",
        ])
        .output()
        .await?;

    if !output.status.success() {
        return Err(format!(
            "Failed to list local branches: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    Ok(parse_branches(&String::from_utf8_lossy(&output.stdout)))
}

async fn git_merged_branches(base: &str) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    let merged = format!("--merged={base}");
    let output = Command::new("git")
        .args([
            "for-each-ref",
            &merged,
            "--format=%(refname:short)",
            "refs/heads",
        ])
        .output()
        .await?;

    if !output.status.success() {
        return Err(format!(
            "Failed to inspect Git ancestry: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

async fn merged_prs() -> Result<Vec<MergedPr>, String> {
    let output = Command::new("gh")
        .args([
            "pr",
            "list",
            "--state",
            "merged",
            "--limit",
            "1000",
            "--json",
            "number,headRefName,headRefOid,baseRefName,isCrossRepository",
        ])
        .output()
        .await
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    parse_merged_prs(&output.stdout).map_err(|error| error.to_string())
}

async fn unique_commit_count(base: &str, branch: &str) -> usize {
    let range = format!("{base}..{branch}");
    let output = Command::new("git")
        .args(["rev-list", "--count", &range])
        .output()
        .await;

    match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .unwrap_or(0),
        _ => 0,
    }
}

async fn worktree_is_clean() -> Result<bool, Box<dyn std::error::Error>> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .await?;
    if !output.status.success() {
        return Err("Failed to inspect working tree".into());
    }
    Ok(output.stdout.is_empty())
}

fn confirm(prompt: &str) -> Result<bool, Box<dyn std::error::Error>> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(
        input.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

async fn switch_to_base(base: &str) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new("git").args(["switch", base]).output().await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Failed to switch to {base}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into())
    }
}

async fn current_branch_oid(branch: &str) -> Result<String, String> {
    let ref_name = format!("refs/heads/{branch}");
    let output = Command::new("git")
        .args(["rev-parse", "--verify", &ref_name])
        .output()
        .await
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

async fn delete_branch(candidate: &Candidate) -> Result<(), String> {
    let actual_oid = current_branch_oid(&candidate.branch.name).await?;
    if actual_oid != candidate.branch.head_oid {
        return Err("branch changed after analysis; run clean again".to_string());
    }

    let flag = match candidate.reason {
        DeleteReason::GitMerged => "-d",
        DeleteReason::MergedPr(_) => "-D",
    };
    let output = Command::new("git")
        .args(["branch", flag, "--", &candidate.branch.name])
        .output()
        .await
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub async fn handle_clean_command(args: CleanArgs) -> Result<(), Box<dyn std::error::Error>> {
    check_git_installed().await?;

    let base = resolve_base_branch().await?;
    let current = get_current_branch().await?;
    let current_clean = worktree_is_clean().await?;
    let branches = all_branches().await?;
    let git_merged = git_merged_branches(base).await?;
    let prs = match merged_prs().await {
        Ok(prs) => prs,
        Err(error) => {
            eprintln!(
                "{} GitHub PR status unavailable; using Git ancestry only",
                style("⚠").yellow()
            );
            if args.verbose && !error.is_empty() {
                eprintln!("— gh: {error}");
            }
            Vec::new()
        }
    };

    let mut candidates = Vec::new();
    let mut kept = Vec::new();
    for branch in branches {
        let unique_commits = unique_commit_count(base, &branch.name).await;
        match classify_branch(
            &branch,
            base,
            &current,
            current_clean,
            git_merged.contains(&branch.name),
            unique_commits,
            &prs,
        ) {
            Ok(candidate) => candidates.push(candidate),
            Err(reason) => kept.push((branch, reason)),
        }
    }

    if args.verbose {
        eprintln!("— Base branch: {base}");
        eprintln!("— {} merged PRs inspected", prs.len());
    }

    if !candidates.is_empty() {
        println!("{} Safe to remove:", style("🧹").cyan());
        for candidate in &candidates {
            let current_marker = if candidate.branch.name == current {
                " (current)"
            } else {
                ""
            };
            println!(
                "  • {}{} — {}",
                candidate.branch.name,
                current_marker,
                candidate.reason.description(base)
            );
        }
        println!();
    }

    if !kept.is_empty() {
        println!("{} Kept:", style("→").dim());
        for (branch, reason) in &kept {
            let upstream = branch
                .upstream
                .as_deref()
                .map(|name| format!("; tracks {name}"))
                .unwrap_or_default();
            println!(
                "  • {} — {}{}",
                branch.name,
                reason.description(base),
                upstream
            );
        }
        println!();
    }

    if candidates.is_empty() {
        println!(
            "{} No local branches are safe to remove",
            style("✓").green()
        );
        return Ok(());
    }

    if args.dry_run {
        println!(
            "{} Dry run; no branches switched or deleted",
            style("—").dim()
        );
        return Ok(());
    }

    if candidates
        .iter()
        .any(|candidate| candidate.branch.name == current)
    {
        if !confirm(&format!(
            "Current branch '{current}' is safe. Switch to '{base}' and include it?"
        ))? {
            candidates.retain(|candidate| candidate.branch.name != current);
            println!("{} Keeping current branch '{current}'", style("—").dim());
        } else {
            if !worktree_is_clean().await? {
                return Err("Working tree changed during analysis; cleanup aborted".into());
            }
            switch_to_base(base).await?;
        }
    }

    if candidates.is_empty() {
        return Ok(());
    }

    if !confirm(&format!(
        "Delete these {} local branches?",
        candidates.len()
    ))? {
        println!("{} Cancelled", style("—").dim());
        return Ok(());
    }

    let mut failures = Vec::new();
    for candidate in candidates {
        match delete_branch(&candidate).await {
            Ok(()) => println!("{} Deleted {}", style("✓").green(), candidate.branch.name),
            Err(error) => {
                eprintln!(
                    "{} Could not delete {}: {error}",
                    style("✗").red(),
                    candidate.branch.name
                );
                failures.push(candidate.branch.name);
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!("Failed to delete {} branch(es)", failures.len()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn branch(name: &str, oid: &str) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            head_oid: oid.to_string(),
            upstream: None,
            upstream_gone: false,
            worktree_path: None,
        }
    }

    fn merged_pr(name: &str, oid: &str) -> MergedPr {
        MergedPr {
            number: 42,
            head_ref_name: name.to_string(),
            head_ref_oid: oid.to_string(),
            base_ref_name: "main".to_string(),
            is_cross_repository: false,
        }
    }

    #[test]
    fn parses_branch_metadata() {
        let branches = parse_branches(
            "feature/merged\tabc123\torigin/feature/merged\t[gone]\t\nfeature/worktree\tdef456\t\t\tC:/other\n",
        );

        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].head_oid, "abc123");
        assert!(branches[0].upstream_gone);
        assert_eq!(branches[1].worktree_path.as_deref(), Some("C:/other"));
    }

    #[test]
    fn parses_merged_pr_json() {
        let prs = parse_merged_prs(
            br#"[{"number":42,"headRefName":"feature/x","headRefOid":"abc","baseRefName":"main","isCrossRepository":false}]"#,
        )
        .unwrap();

        assert_eq!(prs, vec![merged_pr("feature/x", "abc")]);
    }

    #[test]
    fn accepts_git_merge_without_pr() {
        let result = classify_branch(
            &branch("feature/x", "abc"),
            "main",
            "main",
            true,
            true,
            0,
            &[],
        );

        assert_eq!(result.unwrap().reason, DeleteReason::GitMerged);
    }

    #[test]
    fn accepts_only_exact_same_repo_pr_head() {
        let local = branch("feature/x", "abc");
        let exact = classify_branch(
            &local,
            "main",
            "main",
            true,
            false,
            1,
            &[merged_pr("feature/x", "abc")],
        );
        assert_eq!(exact.unwrap().reason, DeleteReason::MergedPr(42));

        let mismatch = classify_branch(
            &local,
            "main",
            "main",
            true,
            false,
            1,
            &[merged_pr("feature/x", "different")],
        );
        assert_eq!(mismatch, Err(KeepReason::PrHeadMismatch(42)));
    }

    #[test]
    fn ignores_cross_repository_and_wrong_base_prs() {
        let local = branch("feature/x", "abc");
        let mut cross_repo = merged_pr("feature/x", "abc");
        cross_repo.is_cross_repository = true;
        let mut wrong_base = merged_pr("feature/x", "abc");
        wrong_base.base_ref_name = "release".to_string();

        assert!(matches!(
            classify_branch(&local, "main", "main", true, false, 1, &[cross_repo]),
            Err(KeepReason::Unmerged { .. })
        ));
        assert!(matches!(
            classify_branch(&local, "main", "main", true, false, 1, &[wrong_base]),
            Err(KeepReason::Unmerged { .. })
        ));
    }

    #[test]
    fn keeps_protected_worktree_and_dirty_current_branches() {
        let protected = classify_branch(
            &branch("develop", "abc"),
            "main",
            "main",
            true,
            true,
            0,
            &[],
        );
        assert_eq!(protected, Err(KeepReason::Protected));

        let mut worktree = branch("feature/worktree", "abc");
        worktree.worktree_path = Some("C:/other".to_string());
        assert_eq!(
            classify_branch(&worktree, "main", "main", true, true, 0, &[]),
            Err(KeepReason::OtherWorktree)
        );

        assert_eq!(
            classify_branch(
                &branch("feature/current", "abc"),
                "main",
                "feature/current",
                false,
                true,
                0,
                &[]
            ),
            Err(KeepReason::DirtyCurrent)
        );
    }

    #[test]
    fn reports_unmerged_branch_with_gone_upstream() {
        let mut local = branch("feature/x", "abc");
        local.upstream_gone = true;

        assert_eq!(
            classify_branch(&local, "main", "main", true, false, 3, &[]),
            Err(KeepReason::Unmerged {
                unique_commits: 3,
                upstream_gone: true,
            })
        );
    }
}
