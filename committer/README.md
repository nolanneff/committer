# 🔍 Committer

A beautiful CLI tool that analyzes your git diffs, displays them in a nice format, and provides intelligent summaries and explanations for each change.

## Features

- **📊 Change Summary** - Quick overview of files changed, additions, deletions
- **💡 Smart Explanations** - Automatically generates brief explanations for each diff based on content analysis
- **📋 Formatted Diffs** - Clean, colorful display of changes with syntax highlighting
- **🚫 Skips Noise** - Automatically skips lock files, build artifacts, and generated files
- **📝 Commit Suggestions** - Generates conventional commit message suggestions

## Installation

### From Source

```bash
# Clone or navigate to the committer directory
cd committer

# Build in release mode
cargo build --release

# The binary will be at ./target/release/committer
```

### Add to PATH (Optional)

```bash
# Copy to a directory in your PATH
sudo cp target/release/committer /usr/local/bin/

# Or add to your local bin
cp target/release/committer ~/.local/bin/
```

## Usage

Simply run `committer` in any git repository:

```bash
cd your-git-repo
committer
```

### Example Output

```
╔══════════════════════════════════════════════════════════════╗
║                    🔍 COMMITTER                              ║
║              Git Diff Analyzer & Summarizer                  ║
╚══════════════════════════════════════════════════════════════╝

┌─────────────────────────────────────────────────────────────┐
│ 📊 CHANGE SUMMARY
├─────────────────────────────────────────────────────────────┤
│ Files changed: 3 (1 added, 2 modified, 0 deleted)
│ Lines: +150 insertions(+), -23 deletions(-)
└─────────────────────────────────────────────────────────────┘

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
[1/3] ✨ src/main.rs [NEW]
      +100 -0 (2 hunks)

  💡 Explanation:
     Created new Rust source executable/program.

  📋 Changes:
     @@ -0,0 +1,50 @@
       + fn main() {
       +     println!("Hello, world!");
       + }
       ... and 47 more lines

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

┌─────────────────────────────────────────────────────────────┐
│ 📝 COMMIT SUGGESTION
├─────────────────────────────────────────────────────────────┤
│ feat: add src/main.rs
└─────────────────────────────────────────────────────────────┘
```

## Skipped Files

The tool automatically skips common generated/arbitrary files:

- Lock files: `package-lock.json`, `yarn.lock`, `Cargo.lock`, `poetry.lock`, etc.
- Minified files: `*.min.js`, `*.min.css`
- Source maps: `*.map`
- Build outputs: `node_modules/`, `target/`, `dist/`, `build/`
- Cache files: `__pycache__/`, `*.pyc`, `*.class`
- System files: `.DS_Store`, `Thumbs.db`

## Smart Explanations

The tool analyzes diff content to provide meaningful explanations:

- Detects new functions, structs, classes, and types
- Identifies test additions
- Recognizes import/dependency changes
- Notes documentation and comment updates
- Flags TODO/FIXME markers
- Highlights error handling modifications
- Detects async/await patterns

## Requirements

- Git installed and available in PATH
- A git repository (obviously!)
- Rust 1.70+ (for building from source)

## License

MIT
