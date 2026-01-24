use colored::*;
use std::process::Command;

/// Represents a single file diff with its metadata
#[derive(Debug)]
struct FileDiff {
    filename: String,
    status: DiffStatus,
    additions: usize,
    deletions: usize,
    hunks: Vec<DiffHunk>,
}

/// The type of change for a file
#[derive(Debug, Clone, PartialEq)]
enum DiffStatus {
    Added,
    Modified,
    Deleted,
    Renamed(String), // old filename
    Copied,
    Unknown,
}

/// A hunk within a diff (a contiguous block of changes)
#[derive(Debug)]
struct DiffHunk {
    header: String,
    lines: Vec<DiffLine>,
}

/// A single line in a diff
#[derive(Debug)]
struct DiffLine {
    kind: LineKind,
    content: String,
}

#[derive(Debug, Clone, PartialEq)]
enum LineKind {
    Addition,
    Deletion,
    Context,
}

fn main() {
    println!();
    println!("{}", "╔══════════════════════════════════════════════════════════════╗".cyan().bold());
    println!("{}", "║                    🔍 COMMITTER                              ║".cyan().bold());
    println!("{}", "║              Git Diff Analyzer & Summarizer                  ║".cyan().bold());
    println!("{}", "╚══════════════════════════════════════════════════════════════╝".cyan().bold());
    println!();

    // Check if we're in a git repository
    if !is_git_repo() {
        eprintln!("{}", "Error: Not a git repository!".red().bold());
        std::process::exit(1);
    }

    // Get the diffs
    let diffs = get_git_diffs();

    if diffs.is_empty() {
        println!("{}", "No changes detected in the working directory.".yellow());
        println!("{}", "Tip: Make some changes to tracked files or stage new files.".dimmed());
        return;
    }

    // Display summary header
    print_summary_header(&diffs);

    // Display each diff with explanation
    for (i, diff) in diffs.iter().enumerate() {
        if should_skip_diff(diff) {
            println!();
            println!(
                "{} {} {}",
                format!("[{}/{}]", i + 1, diffs.len()).dimmed(),
                diff.filename.yellow(),
                "(skipped - arbitrary/generated file)".dimmed()
            );
            continue;
        }

        print_file_diff(diff, i + 1, diffs.len());
    }

    // Print overall summary
    print_overall_summary(&diffs);
}

/// Check if current directory is a git repository
fn is_git_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Get all git diffs (staged and unstaged)
fn get_git_diffs() -> Vec<FileDiff> {
    let mut diffs = Vec::new();

    // Get list of changed files with status
    let diff_stat = Command::new("git")
        .args(["diff", "--name-status", "HEAD"])
        .output();

    // Also check unstaged changes if HEAD doesn't exist (new repo)
    let diff_stat = diff_stat.or_else(|_| {
        Command::new("git")
            .args(["diff", "--name-status"])
            .output()
    });

    // Also get staged files
    let staged_stat = Command::new("git")
        .args(["diff", "--name-status", "--cached"])
        .output();

    // Collect filenames and their status
    let mut files: Vec<(String, DiffStatus)> = Vec::new();

    if let Ok(output) = diff_stat {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some((status, filename)) = parse_status_line(line) {
                files.push((filename, status));
            }
        }
    }

    if let Ok(output) = staged_stat {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if let Some((status, filename)) = parse_status_line(line) {
                if !files.iter().any(|(f, _)| f == &filename) {
                    files.push((filename, status));
                }
            }
        }
    }

    // Also check for untracked files
    if let Ok(output) = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let filename = line.trim().to_string();
            if !filename.is_empty() && !files.iter().any(|(f, _)| f == &filename) {
                files.push((filename, DiffStatus::Added));
            }
        }
    }

    // Get detailed diff for each file
    for (filename, status) in files {
        if let Some(diff) = get_file_diff(&filename, &status) {
            diffs.push(diff);
        }
    }

    diffs
}

/// Parse a git status line like "M\tfilename"
fn parse_status_line(line: &str) -> Option<(DiffStatus, String)> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 2 {
        return None;
    }

    let status = match parts[0].chars().next()? {
        'A' => DiffStatus::Added,
        'M' => DiffStatus::Modified,
        'D' => DiffStatus::Deleted,
        'R' => DiffStatus::Renamed(parts.get(1).unwrap_or(&"").to_string()),
        'C' => DiffStatus::Copied,
        _ => DiffStatus::Unknown,
    };

    let filename = if matches!(status, DiffStatus::Renamed(_)) {
        parts.get(2).unwrap_or(&parts[1]).to_string()
    } else {
        parts[1].to_string()
    };

    Some((status, filename))
}

/// Get detailed diff for a specific file
fn get_file_diff(filename: &str, status: &DiffStatus) -> Option<FileDiff> {
    // Try to get diff from HEAD, then cached, then for new files
    let diff_output = Command::new("git")
        .args(["diff", "HEAD", "--", filename])
        .output()
        .ok()?;

    let mut diff_text = String::from_utf8_lossy(&diff_output.stdout).to_string();

    // If empty, try cached
    if diff_text.trim().is_empty() {
        let cached_output = Command::new("git")
            .args(["diff", "--cached", "--", filename])
            .output()
            .ok()?;
        diff_text = String::from_utf8_lossy(&cached_output.stdout).to_string();
    }

    // If still empty and file is new, show file contents as addition
    if diff_text.trim().is_empty() && *status == DiffStatus::Added {
        if let Ok(contents) = std::fs::read_to_string(filename) {
            let lines: Vec<DiffLine> = contents
                .lines()
                .map(|l| DiffLine {
                    kind: LineKind::Addition,
                    content: l.to_string(),
                })
                .collect();

            return Some(FileDiff {
                filename: filename.to_string(),
                status: status.clone(),
                additions: lines.len(),
                deletions: 0,
                hunks: vec![DiffHunk {
                    header: "@@ -0,0 +1,{} @@ (new file)".to_string(),
                    lines,
                }],
            });
        }
    }

    // Parse the diff output
    parse_diff(&diff_text, filename, status)
}

/// Parse diff output into structured FileDiff
fn parse_diff(diff_text: &str, filename: &str, status: &DiffStatus) -> Option<FileDiff> {
    let lines: Vec<&str> = diff_text.lines().collect();

    if lines.is_empty() {
        return None;
    }

    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut additions = 0;
    let mut deletions = 0;

    for line in lines {
        if line.starts_with("@@") {
            // Save previous hunk
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            // Start new hunk
            current_hunk = Some(DiffHunk {
                header: line.to_string(),
                lines: Vec::new(),
            });
        } else if let Some(ref mut hunk) = current_hunk {
            if let Some(stripped) = line.strip_prefix('+') {
                if !line.starts_with("+++") {
                    additions += 1;
                    hunk.lines.push(DiffLine {
                        kind: LineKind::Addition,
                        content: stripped.to_string(),
                    });
                }
            } else if let Some(stripped) = line.strip_prefix('-') {
                if !line.starts_with("---") {
                    deletions += 1;
                    hunk.lines.push(DiffLine {
                        kind: LineKind::Deletion,
                        content: stripped.to_string(),
                    });
                }
            } else if let Some(stripped) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine {
                    kind: LineKind::Context,
                    content: stripped.to_string(),
                });
            }
        }
    }

    // Don't forget the last hunk
    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }

    if hunks.is_empty() && additions == 0 && deletions == 0 {
        return None;
    }

    Some(FileDiff {
        filename: filename.to_string(),
        status: status.clone(),
        additions,
        deletions,
        hunks,
    })
}

/// Determine if a diff should be skipped (lock files, generated files, etc.)
fn should_skip_diff(diff: &FileDiff) -> bool {
    let skip_patterns = [
        "package-lock.json",
        "yarn.lock",
        "Cargo.lock",
        "Gemfile.lock",
        "poetry.lock",
        "composer.lock",
        "pnpm-lock.yaml",
        ".min.js",
        ".min.css",
        ".map",
        "node_modules/",
        "target/debug/",
        "target/release/",
        ".pyc",
        "__pycache__",
        ".class",
        "dist/",
        "build/",
        ".DS_Store",
        "Thumbs.db",
    ];

    let filename = &diff.filename;
    skip_patterns.iter().any(|p| filename.contains(p))
}

/// Print the summary header
fn print_summary_header(diffs: &[FileDiff]) {
    let total_files = diffs.len();
    let total_additions: usize = diffs.iter().map(|d| d.additions).sum();
    let total_deletions: usize = diffs.iter().map(|d| d.deletions).sum();

    let added = diffs.iter().filter(|d| matches!(d.status, DiffStatus::Added)).count();
    let modified = diffs.iter().filter(|d| matches!(d.status, DiffStatus::Modified)).count();
    let deleted = diffs.iter().filter(|d| matches!(d.status, DiffStatus::Deleted)).count();

    println!("{}", "┌─────────────────────────────────────────────────────────────┐".blue());
    println!("{} {}", "│".blue(), "📊 CHANGE SUMMARY".white().bold());
    println!("{}", "├─────────────────────────────────────────────────────────────┤".blue());
    println!(
        "{} Files changed: {} ({} added, {} modified, {} deleted)",
        "│".blue(),
        total_files.to_string().white().bold(),
        added.to_string().green(),
        modified.to_string().yellow(),
        deleted.to_string().red()
    );
    println!(
        "{} Lines: {} insertions(+), {} deletions(-)",
        "│".blue(),
        format!("+{}", total_additions).green().bold(),
        format!("-{}", total_deletions).red().bold()
    );
    println!("{}", "└─────────────────────────────────────────────────────────────┘".blue());
}

/// Print a single file diff with explanation
fn print_file_diff(diff: &FileDiff, index: usize, total: usize) {
    println!();
    println!("{}", "━".repeat(65).dimmed());

    // File header
    let status_icon = match &diff.status {
        DiffStatus::Added => "✨",
        DiffStatus::Modified => "📝",
        DiffStatus::Deleted => "🗑️ ",
        DiffStatus::Renamed(_) => "📋",
        DiffStatus::Copied => "📄",
        DiffStatus::Unknown => "❓",
    };

    let status_text = match &diff.status {
        DiffStatus::Added => "NEW".green().bold(),
        DiffStatus::Modified => "MODIFIED".yellow().bold(),
        DiffStatus::Deleted => "DELETED".red().bold(),
        DiffStatus::Renamed(old) => format!("RENAMED from {}", old).magenta().bold(),
        DiffStatus::Copied => "COPIED".cyan().bold(),
        DiffStatus::Unknown => "UNKNOWN".dimmed().bold(),
    };

    println!(
        "{} {} {} [{}]",
        format!("[{}/{}]", index, total).dimmed(),
        status_icon,
        diff.filename.white().bold(),
        status_text
    );

    // Stats
    println!(
        "      {} {} {}",
        format!("+{}", diff.additions).green(),
        format!("-{}", diff.deletions).red(),
        format!("({} hunks)", diff.hunks.len()).dimmed()
    );

    // Generate and print explanation
    let explanation = generate_explanation(diff);
    println!();
    println!("  {} {}", "💡".yellow(), "Explanation:".yellow().bold());
    println!("     {}", explanation.white());

    // Show diff hunks (limited)
    if !diff.hunks.is_empty() {
        println!();
        println!("  {} {}", "📋".blue(), "Changes:".blue().bold());

        for (hunk_idx, hunk) in diff.hunks.iter().take(3).enumerate() {
            println!("     {}", hunk.header.cyan().dimmed());

            // Show limited lines per hunk
            let max_lines = 8;
            let lines_to_show: Vec<_> = hunk.lines.iter().take(max_lines).collect();

            for line in &lines_to_show {
                let formatted = match line.kind {
                    LineKind::Addition => format!("  + {}", line.content).green(),
                    LineKind::Deletion => format!("  - {}", line.content).red(),
                    LineKind::Context => format!("    {}", line.content).dimmed(),
                };
                println!("     {}", formatted);
            }

            if hunk.lines.len() > max_lines {
                println!(
                    "     {}",
                    format!("  ... and {} more lines", hunk.lines.len() - max_lines).dimmed()
                );
            }

            if hunk_idx < diff.hunks.len().min(3) - 1 {
                println!();
            }
        }

        if diff.hunks.len() > 3 {
            println!(
                "     {}",
                format!("... and {} more hunks", diff.hunks.len() - 3).dimmed()
            );
        }
    }
}

/// Generate an explanation for a diff based on its content
fn generate_explanation(diff: &FileDiff) -> String {
    let filename = &diff.filename;
    let ext = filename.rsplit('.').next().unwrap_or("");

    // Collect all changed content for analysis
    let all_additions: Vec<&str> = diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.kind == LineKind::Addition)
        .map(|l| l.content.as_str())
        .collect();

    let all_deletions: Vec<&str> = diff
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.kind == LineKind::Deletion)
        .map(|l| l.content.as_str())
        .collect();

    // Status-based explanations
    match &diff.status {
        DiffStatus::Added => {
            return generate_new_file_explanation(filename, ext, &all_additions);
        }
        DiffStatus::Deleted => {
            return format!("Removed {} file from the project.", get_file_type_description(ext));
        }
        DiffStatus::Renamed(old) => {
            return format!("Renamed file from '{}' to '{}', possibly reorganizing project structure.", old, filename);
        }
        _ => {}
    }

    // Content-based analysis for modifications
    let mut insights: Vec<String> = Vec::new();

    // Check for specific patterns
    if contains_pattern(&all_additions, &["fn ", "func ", "def ", "function "]) {
        insights.push("Added new function(s)".to_string());
    }

    if contains_pattern(&all_deletions, &["fn ", "func ", "def ", "function "]) {
        insights.push("Removed function(s)".to_string());
    }

    if contains_pattern(&all_additions, &["struct ", "class ", "interface ", "type "]) {
        insights.push("Added new type/struct definitions".to_string());
    }

    if contains_pattern(&all_additions, &["impl ", "impl<"]) {
        insights.push("Added implementation block(s)".to_string());
    }

    if contains_pattern(&all_additions, &["#[test]", "@test", "test_", "#[cfg(test)]"]) {
        insights.push("Added test(s)".to_string());
    }

    if contains_pattern(&all_additions, &["use ", "import ", "require(", "from "]) {
        insights.push("Modified imports/dependencies".to_string());
    }

    if contains_pattern(&all_additions, &["// ", "/* ", "/// ", "//!", "# ", "\"\"\"", "'''"]) {
        insights.push("Added/updated comments or documentation".to_string());
    }

    if contains_pattern(&all_additions, &["TODO", "FIXME", "HACK", "XXX"]) {
        insights.push("Added TODO/FIXME markers".to_string());
    }

    if contains_pattern(&all_additions, &["error", "Error", "panic!", "unwrap()", "expect("]) {
        insights.push("Modified error handling".to_string());
    }

    if contains_pattern(&all_additions, &["pub ", "public ", "private ", "protected "]) {
        insights.push("Changed visibility/access modifiers".to_string());
    }

    if contains_pattern(&all_additions, &["async ", "await ", ".await"]) {
        insights.push("Added async/await patterns".to_string());
    }

    // Config file specific
    if filename.contains("Cargo.toml") || filename.contains("package.json") {
        if contains_pattern(&all_additions, &["dependencies", "devDependencies", "[dependencies]"]) {
            insights.push("Modified project dependencies".to_string());
        }
        if contains_pattern(&all_additions, &["version"]) {
            insights.push("Updated version information".to_string());
        }
    }

    // Build summary
    if insights.is_empty() {
        // Generic explanation based on line changes
        if diff.additions > diff.deletions * 2 {
            return format!("Significant additions to {} - expanded functionality or content.", get_file_type_description(ext));
        } else if diff.deletions > diff.additions * 2 {
            return format!("Cleanup/simplification of {} - removed unused code or content.", get_file_type_description(ext));
        } else {
            return format!("Refactored/modified {} with balanced changes.", get_file_type_description(ext));
        }
    }

    insights.join("; ")
}

/// Generate explanation for new files
fn generate_new_file_explanation(filename: &str, ext: &str, additions: &[&str]) -> String {
    let file_type = get_file_type_description(ext);

    // Check for specific new file types
    if filename.contains("test") || filename.contains("spec") {
        return format!("Added new {} test file for testing functionality.", file_type);
    }

    if filename == "main.rs" || filename == "main.py" || filename == "index.js" || filename == "index.ts" {
        return format!("Created {} entry point for the application.", file_type);
    }

    if filename.contains("README") || filename.contains("readme") {
        return "Added project documentation/README file.".to_string();
    }

    if filename == "Cargo.toml" || filename == "package.json" || filename == "pyproject.toml" {
        return "Created project manifest/configuration file.".to_string();
    }

    if filename.contains("config") || filename.contains("settings") {
        return format!("Added {} configuration file.", file_type);
    }

    // Check content for hints
    if contains_pattern(additions, &["fn main", "def main", "int main", "func main"]) {
        return format!("Created new {} executable/program.", file_type);
    }

    if contains_pattern(additions, &["mod ", "module ", "export "]) {
        return format!("Added new {} module.", file_type);
    }

    format!("Added new {} file to the project.", file_type)
}

/// Get human-readable file type description
fn get_file_type_description(ext: &str) -> &'static str {
    match ext {
        "rs" => "Rust source",
        "py" => "Python",
        "js" => "JavaScript",
        "ts" => "TypeScript",
        "jsx" | "tsx" => "React component",
        "go" => "Go",
        "c" | "h" => "C",
        "cpp" | "hpp" | "cc" => "C++",
        "java" => "Java",
        "rb" => "Ruby",
        "php" => "PHP",
        "swift" => "Swift",
        "kt" => "Kotlin",
        "scala" => "Scala",
        "html" => "HTML",
        "css" => "CSS",
        "scss" | "sass" => "SASS/SCSS",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "xml" => "XML",
        "md" => "Markdown",
        "sql" => "SQL",
        "sh" | "bash" => "Shell script",
        "dockerfile" | "Dockerfile" => "Docker",
        _ => "source",
    }
}

/// Check if any line contains any of the patterns
fn contains_pattern(lines: &[&str], patterns: &[&str]) -> bool {
    lines.iter().any(|line| {
        patterns.iter().any(|p| line.contains(p))
    })
}

/// Print overall summary and suggestions
fn print_overall_summary(diffs: &[FileDiff]) {
    println!();
    println!("{}", "━".repeat(65).dimmed());
    println!();
    println!("{}", "┌─────────────────────────────────────────────────────────────┐".green());
    println!("{} {}", "│".green(), "📝 COMMIT SUGGESTION".white().bold());
    println!("{}", "├─────────────────────────────────────────────────────────────┤".green());

    // Generate commit message suggestion
    let commit_msg = generate_commit_suggestion(diffs);
    println!("{} {}", "│".green(), commit_msg.white());

    println!("{}", "└─────────────────────────────────────────────────────────────┘".green());
    println!();
}

/// Generate a commit message suggestion based on the diffs
fn generate_commit_suggestion(diffs: &[FileDiff]) -> String {
    let relevant_diffs: Vec<_> = diffs.iter().filter(|d| !should_skip_diff(d)).collect();

    if relevant_diffs.is_empty() {
        return "chore: update generated files".to_string();
    }

    let added: Vec<_> = relevant_diffs.iter().filter(|d| matches!(d.status, DiffStatus::Added)).collect();
    let modified: Vec<_> = relevant_diffs.iter().filter(|d| matches!(d.status, DiffStatus::Modified)).collect();
    let deleted: Vec<_> = relevant_diffs.iter().filter(|d| matches!(d.status, DiffStatus::Deleted)).collect();

    // Single file change
    if relevant_diffs.len() == 1 {
        let diff = &relevant_diffs[0];
        let short_name = diff.filename.rsplit('/').next().unwrap_or(&diff.filename);

        return match &diff.status {
            DiffStatus::Added => format!("feat: add {}", short_name),
            DiffStatus::Modified => format!("refactor: update {}", short_name),
            DiffStatus::Deleted => format!("chore: remove {}", short_name),
            DiffStatus::Renamed(old) => format!("refactor: rename {} to {}", old, short_name),
            _ => format!("chore: update {}", short_name),
        };
    }

    // Multiple files
    if !added.is_empty() && modified.is_empty() && deleted.is_empty() {
        if added.len() == 1 {
            let name = added[0].filename.rsplit('/').next().unwrap_or(&added[0].filename);
            return format!("feat: add {}", name);
        }
        return format!("feat: add {} new files", added.len());
    }

    if !deleted.is_empty() && added.is_empty() && modified.is_empty() {
        return format!("chore: remove {} files", deleted.len());
    }

    if !modified.is_empty() && added.is_empty() && deleted.is_empty() {
        if modified.len() <= 3 {
            let names: Vec<_> = modified.iter()
                .map(|d| d.filename.rsplit('/').next().unwrap_or(&d.filename))
                .collect();
            return format!("refactor: update {}", names.join(", "));
        }
        return format!("refactor: update {} files", modified.len());
    }

    // Mixed changes
    format!(
        "chore: {} files changed ({} added, {} modified, {} deleted)",
        relevant_diffs.len(),
        added.len(),
        modified.len(),
        deleted.len()
    )
}
