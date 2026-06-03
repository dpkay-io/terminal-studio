# Merge Conflict Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add per-hunk inline merge conflict detection and resolution to the git diff panel, with a dedicated `ConflictResolver` pane type.

**Architecture:** Parse conflict markers (`<<<<<<<`/`=======`/`>>>>>>>`) directly from file content into structured `ConflictBlock` types. A new `PaneContent::ConflictResolver` pane renders blocks as color-coded inline sections with floating action bars. Resolutions write back to disk immediately via `atomic_write()`. A "Conflicts" section in the git status panel routes `UU`/`AA`/`DU`/`UD` files to this pane.

**Tech Stack:** Rust, egui 0.28, `crate::util::atomic_write`, `crate::theme` design tokens, `crate::git::parser` for status parsing.

---

## File Structure

| File | Responsibility | Action |
|------|---------------|--------|
| `src/app/conflict_parser.rs` | Conflict marker parser, types (`ConflictFile`, `ConflictBlock`, `Resolution`), `write_resolved_file()`, `resolve_block()` | **Create** |
| `src/git/parser.rs` | Add `FileChangeKind::Conflicted`; parse `UU`/`AA`/`DU`/`UD` status codes | **Modify** |
| `src/app/pane.rs` | Add `PaneContent::ConflictResolver(ConflictResolverState)` and `ConflictResolverState` struct | **Modify** |
| `src/app/ui/conflict_resolver.rs` | Conflict resolver pane rendering: toolbar, conflict blocks, action buttons, context lines | **Create** |
| `src/app/ui/mod.rs` | Register `conflict_resolver` module | **Modify** |
| `src/app/git_diff.rs` | Add "Conflicts" section at top of status panel; add `open_conflict_file` to `GitDiffResult` | **Modify** |
| `src/app/ui/pane_renderer.rs` | Dispatch `PaneContent::ConflictResolver` to renderer | **Modify** |
| `src/app.rs` | Handle conflict pane creation from `open_conflict_file`; skip `ConflictResolver` in persistence save | **Modify** |

---

### Task 1: Conflict Marker Parser — Types and Parsing

**Files:**
- Create: `src/app/conflict_parser.rs`

This task creates the core parser that extracts conflict blocks from file content. No UI, no git integration — pure parsing logic with full test coverage.

- [ ] **Step 1: Write failing tests for conflict parser**

Create `src/app/conflict_parser.rs` with types and test module:

```rust
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ours,
    Theirs,
    Both,
}

#[derive(Debug, Clone)]
pub enum ConflictBlock {
    Context {
        lines: Vec<String>,
    },
    Conflict {
        index: usize,
        ours_lines: Vec<String>,
        theirs_lines: Vec<String>,
        ours_label: String,
        theirs_label: String,
        resolved: Option<Resolution>,
    },
}

#[derive(Debug, Clone)]
pub struct ConflictFile {
    pub path: PathBuf,
    pub blocks: Vec<ConflictBlock>,
    pub total_conflicts: usize,
}

pub fn parse_conflict_file(path: &Path, content: &str) -> ConflictFile {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_conflicts_returns_single_context_block() {
        let content = "line 1\nline 2\nline 3\n";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        assert_eq!(result.total_conflicts, 0);
        assert_eq!(result.blocks.len(), 1);
        match &result.blocks[0] {
            ConflictBlock::Context { lines } => {
                assert_eq!(lines.len(), 3);
                assert_eq!(lines[0], "line 1");
            }
            _ => panic!("expected Context block"),
        }
    }

    #[test]
    fn single_conflict_parsed_correctly() {
        let content = "\
before
<<<<<<< HEAD
ours line 1
ours line 2
=======
theirs line 1
>>>>>>> feature-branch
after
";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        assert_eq!(result.total_conflicts, 1);
        assert_eq!(result.blocks.len(), 3); // context, conflict, context
        match &result.blocks[1] {
            ConflictBlock::Conflict {
                index,
                ours_lines,
                theirs_lines,
                ours_label,
                theirs_label,
                resolved,
            } => {
                assert_eq!(*index, 0);
                assert_eq!(ours_lines, &["ours line 1", "ours line 2"]);
                assert_eq!(theirs_lines, &["theirs line 1"]);
                assert_eq!(ours_label, "HEAD");
                assert_eq!(theirs_label, "feature-branch");
                assert!(resolved.is_none());
            }
            _ => panic!("expected Conflict block"),
        }
    }

    #[test]
    fn multiple_conflicts_parsed() {
        let content = "\
top
<<<<<<< HEAD
a
=======
b
>>>>>>> branch1
middle
<<<<<<< HEAD
c
=======
d
>>>>>>> branch2
bottom
";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        assert_eq!(result.total_conflicts, 2);
        // context, conflict, context, conflict, context
        assert_eq!(result.blocks.len(), 5);
        match &result.blocks[1] {
            ConflictBlock::Conflict { index, ours_lines, theirs_lines, .. } => {
                assert_eq!(*index, 0);
                assert_eq!(ours_lines, &["a"]);
                assert_eq!(theirs_lines, &["b"]);
            }
            _ => panic!("expected first Conflict"),
        }
        match &result.blocks[3] {
            ConflictBlock::Conflict { index, ours_lines, theirs_lines, .. } => {
                assert_eq!(*index, 1);
                assert_eq!(ours_lines, &["c"]);
                assert_eq!(theirs_lines, &["d"]);
            }
            _ => panic!("expected second Conflict"),
        }
    }

    #[test]
    fn empty_ours_section() {
        let content = "\
<<<<<<< HEAD
=======
theirs
>>>>>>> branch
";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        assert_eq!(result.total_conflicts, 1);
        match &result.blocks[0] {
            ConflictBlock::Conflict { ours_lines, theirs_lines, .. } => {
                assert!(ours_lines.is_empty());
                assert_eq!(theirs_lines, &["theirs"]);
            }
            _ => panic!("expected Conflict"),
        }
    }

    #[test]
    fn empty_theirs_section() {
        let content = "\
<<<<<<< HEAD
ours
=======
>>>>>>> branch
";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        assert_eq!(result.total_conflicts, 1);
        match &result.blocks[0] {
            ConflictBlock::Conflict { ours_lines, theirs_lines, .. } => {
                assert_eq!(ours_lines, &["ours"]);
                assert!(theirs_lines.is_empty());
            }
            _ => panic!("expected Conflict"),
        }
    }

    #[test]
    fn labels_extracted_from_markers() {
        let content = "\
<<<<<<< HEAD
x
=======
y
>>>>>>> refs/heads/my-feature
";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        match &result.blocks[0] {
            ConflictBlock::Conflict { ours_label, theirs_label, .. } => {
                assert_eq!(ours_label, "HEAD");
                assert_eq!(theirs_label, "refs/heads/my-feature");
            }
            _ => panic!("expected Conflict"),
        }
    }

    #[test]
    fn malformed_missing_separator_treated_as_context() {
        let content = "\
<<<<<<< HEAD
ours
>>>>>>> branch
";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        // Missing ======= → malformed, treat entire region as context
        assert_eq!(result.total_conflicts, 0);
        assert_eq!(result.blocks.len(), 1);
        match &result.blocks[0] {
            ConflictBlock::Context { lines } => {
                assert_eq!(lines.len(), 3);
            }
            _ => panic!("expected Context"),
        }
    }

    #[test]
    fn malformed_missing_end_marker_treated_as_context() {
        let content = "\
<<<<<<< HEAD
ours
=======
theirs
";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        // Missing >>>>>>> → malformed, treat as context
        assert_eq!(result.total_conflicts, 0);
        assert_eq!(result.blocks.len(), 1);
    }

    #[test]
    fn empty_file() {
        let result = parse_conflict_file(Path::new("test.rs"), "");
        assert_eq!(result.total_conflicts, 0);
        assert_eq!(result.blocks.len(), 0);
    }

    #[test]
    fn conflict_at_start_of_file() {
        let content = "\
<<<<<<< HEAD
ours
=======
theirs
>>>>>>> branch
after
";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        assert_eq!(result.total_conflicts, 1);
        assert_eq!(result.blocks.len(), 2); // conflict, context
        matches!(&result.blocks[0], ConflictBlock::Conflict { .. });
    }

    #[test]
    fn conflict_at_end_of_file() {
        let content = "\
before
<<<<<<< HEAD
ours
=======
theirs
>>>>>>> branch
";
        let result = parse_conflict_file(Path::new("test.rs"), content);
        assert_eq!(result.total_conflicts, 1);
        assert_eq!(result.blocks.len(), 2); // context, conflict
    }

    #[test]
    fn path_preserved() {
        let result = parse_conflict_file(Path::new("src/main.rs"), "hello\n");
        assert_eq!(result.path, PathBuf::from("src/main.rs"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib conflict_parser -- --nocapture 2>&1 | head -20`
Expected: FAIL with "not yet implemented"

- [ ] **Step 3: Implement `parse_conflict_file`**

Replace the `todo!()` in `parse_conflict_file` with:

```rust
pub fn parse_conflict_file(path: &Path, content: &str) -> ConflictFile {
    if content.is_empty() {
        return ConflictFile {
            path: path.to_path_buf(),
            blocks: Vec::new(),
            total_conflicts: 0,
        };
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut blocks: Vec<ConflictBlock> = Vec::new();
    let mut conflict_count: usize = 0;
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("<<<<<<<") {
            // Attempt to parse a complete conflict block
            let ours_label = lines[i].get(8..).unwrap_or("").trim().to_string();
            let marker_start = i;
            i += 1;

            // Collect ours lines until =======
            let mut ours_lines: Vec<String> = Vec::new();
            let mut found_separator = false;
            while i < lines.len() {
                if lines[i].starts_with("=======") {
                    found_separator = true;
                    i += 1;
                    break;
                }
                if lines[i].starts_with(">>>>>>>") {
                    break; // malformed: no separator
                }
                ours_lines.push(lines[i].to_string());
                i += 1;
            }

            if !found_separator {
                // Malformed: missing =======. Treat all lines from marker as context.
                let malformed: Vec<String> = lines[marker_start..i.min(lines.len())]
                    .iter()
                    .map(|l| l.to_string())
                    .collect();
                if let Some(ConflictBlock::Context { lines: ctx }) = blocks.last_mut() {
                    ctx.extend(malformed);
                } else {
                    blocks.push(ConflictBlock::Context { lines: malformed });
                }
                continue;
            }

            // Collect theirs lines until >>>>>>>
            let mut theirs_lines: Vec<String> = Vec::new();
            let mut found_end = false;
            let mut theirs_label = String::new();
            while i < lines.len() {
                if lines[i].starts_with(">>>>>>>") {
                    theirs_label = lines[i].get(8..).unwrap_or("").trim().to_string();
                    found_end = true;
                    i += 1;
                    break;
                }
                theirs_lines.push(lines[i].to_string());
                i += 1;
            }

            if !found_end {
                // Malformed: missing >>>>>>>. Treat all lines from marker as context.
                let malformed: Vec<String> = lines[marker_start..i.min(lines.len())]
                    .iter()
                    .map(|l| l.to_string())
                    .collect();
                if let Some(ConflictBlock::Context { lines: ctx }) = blocks.last_mut() {
                    ctx.extend(malformed);
                } else {
                    blocks.push(ConflictBlock::Context { lines: malformed });
                }
                continue;
            }

            blocks.push(ConflictBlock::Conflict {
                index: conflict_count,
                ours_lines,
                theirs_lines,
                ours_label,
                theirs_label,
                resolved: None,
            });
            conflict_count += 1;
        } else {
            // Context line
            if let Some(ConflictBlock::Context { lines: ctx }) = blocks.last_mut() {
                ctx.push(lines[i].to_string());
            } else {
                blocks.push(ConflictBlock::Context {
                    lines: vec![lines[i].to_string()],
                });
            }
            i += 1;
        }
    }

    ConflictFile {
        path: path.to_path_buf(),
        blocks,
        total_conflicts: conflict_count,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib conflict_parser -- --nocapture`
Expected: All 12 tests PASS

- [ ] **Step 5: Register module in app mod**

Add `pub(super) mod conflict_parser;` to `src/app.rs` alongside the other `mod` declarations. Find the block of `mod` statements near the top (around line 30-45) and add it there.

- [ ] **Step 6: Run `cargo build` to confirm compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles successfully (warnings about dead code are OK for now)

- [ ] **Step 7: Commit**

```bash
git add src/app/conflict_parser.rs src/app.rs
git commit -m "Add conflict marker parser with types and tests"
```

---

### Task 2: File Writing — `write_resolved_file` and `resolve_block`

**Files:**
- Modify: `src/app/conflict_parser.rs`

Add the file-writing logic that reconstructs file content from blocks and writes it atomically.

- [ ] **Step 1: Write failing tests for file writing**

Add these tests to the existing `mod tests` block in `src/app/conflict_parser.rs`:

```rust
    #[test]
    fn reconstruct_no_conflicts() {
        let blocks = vec![ConflictBlock::Context {
            lines: vec!["line 1".into(), "line 2".into()],
        }];
        let output = reconstruct_content(&blocks);
        assert_eq!(output, "line 1\nline 2\n");
    }

    #[test]
    fn reconstruct_resolved_ours() {
        let blocks = vec![
            ConflictBlock::Context { lines: vec!["before".into()] },
            ConflictBlock::Conflict {
                index: 0,
                ours_lines: vec!["kept".into()],
                theirs_lines: vec!["dropped".into()],
                ours_label: "HEAD".into(),
                theirs_label: "branch".into(),
                resolved: Some(Resolution::Ours),
            },
            ConflictBlock::Context { lines: vec!["after".into()] },
        ];
        let output = reconstruct_content(&blocks);
        assert_eq!(output, "before\nkept\nafter\n");
    }

    #[test]
    fn reconstruct_resolved_theirs() {
        let blocks = vec![ConflictBlock::Conflict {
            index: 0,
            ours_lines: vec!["dropped".into()],
            theirs_lines: vec!["kept".into()],
            ours_label: "HEAD".into(),
            theirs_label: "branch".into(),
            resolved: Some(Resolution::Theirs),
        }];
        let output = reconstruct_content(&blocks);
        assert_eq!(output, "kept\n");
    }

    #[test]
    fn reconstruct_resolved_both() {
        let blocks = vec![ConflictBlock::Conflict {
            index: 0,
            ours_lines: vec!["ours".into()],
            theirs_lines: vec!["theirs".into()],
            ours_label: "HEAD".into(),
            theirs_label: "branch".into(),
            resolved: Some(Resolution::Both),
        }];
        let output = reconstruct_content(&blocks);
        assert_eq!(output, "ours\ntheirs\n");
    }

    #[test]
    fn reconstruct_unresolved_preserves_markers() {
        let blocks = vec![ConflictBlock::Conflict {
            index: 0,
            ours_lines: vec!["ours".into()],
            theirs_lines: vec!["theirs".into()],
            ours_label: "HEAD".into(),
            theirs_label: "branch".into(),
            resolved: None,
        }];
        let output = reconstruct_content(&blocks);
        assert_eq!(
            output,
            "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n"
        );
    }

    #[test]
    fn reconstruct_partial_resolution() {
        let blocks = vec![
            ConflictBlock::Conflict {
                index: 0,
                ours_lines: vec!["a".into()],
                theirs_lines: vec!["b".into()],
                ours_label: "HEAD".into(),
                theirs_label: "br".into(),
                resolved: Some(Resolution::Ours),
            },
            ConflictBlock::Context { lines: vec!["mid".into()] },
            ConflictBlock::Conflict {
                index: 1,
                ours_lines: vec!["c".into()],
                theirs_lines: vec!["d".into()],
                ours_label: "HEAD".into(),
                theirs_label: "br".into(),
                resolved: None,
            },
        ];
        let output = reconstruct_content(&blocks);
        assert_eq!(
            output,
            "a\nmid\n<<<<<<< HEAD\nc\n=======\nd\n>>>>>>> br\n"
        );
    }

    #[test]
    fn resolve_block_updates_state() {
        let mut file = ConflictFile {
            path: PathBuf::from("test.rs"),
            blocks: vec![
                ConflictBlock::Context { lines: vec!["x".into()] },
                ConflictBlock::Conflict {
                    index: 0,
                    ours_lines: vec!["a".into()],
                    theirs_lines: vec!["b".into()],
                    ours_label: "HEAD".into(),
                    theirs_label: "br".into(),
                    resolved: None,
                },
            ],
            total_conflicts: 1,
        };
        let resolved = resolve_block(&mut file, 0, Resolution::Theirs);
        assert_eq!(resolved, 1);
        match &file.blocks[1] {
            ConflictBlock::Conflict { resolved, .. } => {
                assert_eq!(*resolved, Some(Resolution::Theirs));
            }
            _ => panic!("expected Conflict"),
        }
    }

    #[test]
    fn resolve_all_blocks() {
        let mut file = ConflictFile {
            path: PathBuf::from("test.rs"),
            blocks: vec![
                ConflictBlock::Conflict {
                    index: 0,
                    ours_lines: vec!["a".into()],
                    theirs_lines: vec!["b".into()],
                    ours_label: "HEAD".into(),
                    theirs_label: "br".into(),
                    resolved: None,
                },
                ConflictBlock::Context { lines: vec!["mid".into()] },
                ConflictBlock::Conflict {
                    index: 1,
                    ours_lines: vec!["c".into()],
                    theirs_lines: vec!["d".into()],
                    ours_label: "HEAD".into(),
                    theirs_label: "br".into(),
                    resolved: None,
                },
            ],
            total_conflicts: 2,
        };
        let resolved = resolve_all(&mut file, Resolution::Ours);
        assert_eq!(resolved, 2);
    }

    #[test]
    fn reconstruct_empty_ours_resolved() {
        let blocks = vec![ConflictBlock::Conflict {
            index: 0,
            ours_lines: vec![],
            theirs_lines: vec!["theirs".into()],
            ours_label: "HEAD".into(),
            theirs_label: "branch".into(),
            resolved: Some(Resolution::Ours),
        }];
        let output = reconstruct_content(&blocks);
        // Ours is empty, so only trailing newline
        assert_eq!(output, "");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib conflict_parser -- --nocapture 2>&1 | head -20`
Expected: FAIL — functions not defined

- [ ] **Step 3: Implement `reconstruct_content`, `resolve_block`, `resolve_all`, and `write_resolved_file`**

Add these functions to `src/app/conflict_parser.rs` above the `#[cfg(test)]` block:

```rust
pub fn reconstruct_content(blocks: &[ConflictBlock]) -> String {
    let mut output = String::new();
    for block in blocks {
        match block {
            ConflictBlock::Context { lines } => {
                for line in lines {
                    output.push_str(line);
                    output.push('\n');
                }
            }
            ConflictBlock::Conflict {
                ours_lines,
                theirs_lines,
                ours_label,
                theirs_label,
                resolved,
                ..
            } => match resolved {
                Some(Resolution::Ours) => {
                    for line in ours_lines {
                        output.push_str(line);
                        output.push('\n');
                    }
                }
                Some(Resolution::Theirs) => {
                    for line in theirs_lines {
                        output.push_str(line);
                        output.push('\n');
                    }
                }
                Some(Resolution::Both) => {
                    for line in ours_lines {
                        output.push_str(line);
                        output.push('\n');
                    }
                    for line in theirs_lines {
                        output.push_str(line);
                        output.push('\n');
                    }
                }
                None => {
                    output.push_str(&format!("<<<<<<< {}\n", ours_label));
                    for line in ours_lines {
                        output.push_str(line);
                        output.push('\n');
                    }
                    output.push_str("=======\n");
                    for line in theirs_lines {
                        output.push_str(line);
                        output.push('\n');
                    }
                    output.push_str(&format!(">>>>>>> {}\n", theirs_label));
                }
            },
        }
    }
    output
}

/// Mark a single conflict block as resolved. Returns total resolved count.
pub fn resolve_block(file: &mut ConflictFile, conflict_index: usize, resolution: Resolution) -> usize {
    for block in &mut file.blocks {
        if let ConflictBlock::Conflict { index, resolved, .. } = block {
            if *index == conflict_index {
                *resolved = Some(resolution);
                break;
            }
        }
    }
    resolved_count(file)
}

/// Mark all unresolved conflicts with the given resolution. Returns total resolved count.
pub fn resolve_all(file: &mut ConflictFile, resolution: Resolution) -> usize {
    for block in &mut file.blocks {
        if let ConflictBlock::Conflict { resolved, .. } = block {
            if resolved.is_none() {
                *resolved = Some(resolution.clone());
            }
        }
    }
    resolved_count(file)
}

/// Count how many conflicts are currently resolved.
pub fn resolved_count(file: &ConflictFile) -> usize {
    file.blocks
        .iter()
        .filter(|b| matches!(b, ConflictBlock::Conflict { resolved: Some(_), .. }))
        .count()
}

/// Write the conflict file back to disk with resolved blocks substituted.
pub fn write_resolved_file(path: &Path, blocks: &[ConflictBlock]) -> anyhow::Result<()> {
    let content = reconstruct_content(blocks);
    crate::util::atomic_write(path, &content)?;
    Ok(())
}
```

Also add `use anyhow;` to the top of the file (or just use `anyhow::Result` inline — check if `anyhow` is already in scope from the crate).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib conflict_parser -- --nocapture`
Expected: All 21 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/app/conflict_parser.rs
git commit -m "Add conflict file reconstruction and resolution helpers"
```

---

### Task 3: Git Status Parser — `Conflicted` Variant

**Files:**
- Modify: `src/git/parser.rs`

Extend the git status parser to recognize unmerged file status codes.

- [ ] **Step 1: Write failing tests for conflict status parsing**

Add these tests to the `mod tests` block in `src/git/parser.rs`:

```rust
    #[test]
    fn unmerged_both_modified() {
        let entries = parse_git_status("UU src/conflict.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Conflicted);
        assert_eq!(entries[0].path, "src/conflict.rs");
        assert!(!entries[0].staged);
    }

    #[test]
    fn unmerged_both_added() {
        let entries = parse_git_status("AA src/both_added.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Conflicted);
        assert_eq!(entries[0].path, "src/both_added.rs");
        assert!(!entries[0].staged);
    }

    #[test]
    fn unmerged_delete_modify() {
        let entries = parse_git_status("DU src/del_mod.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Conflicted);
    }

    #[test]
    fn unmerged_modify_delete() {
        let entries = parse_git_status("UD src/mod_del.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Conflicted);
    }

    #[test]
    fn conflicted_mixed_with_normal() {
        let input = "M  src/ok.rs\nUU src/conflict.rs\n?? new.txt\n";
        let entries = parse_git_status(input);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, FileChangeKind::Modified);
        assert_eq!(entries[1].kind, FileChangeKind::Conflicted);
        assert_eq!(entries[2].kind, FileChangeKind::Untracked);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib git::parser -- --nocapture 2>&1 | head -20`
Expected: FAIL — no `Conflicted` variant

- [ ] **Step 3: Add `Conflicted` variant and parsing logic**

In `src/git/parser.rs`:

1. Add `Conflicted` to the `FileChangeKind` enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}
```

2. In `parse_git_status`, add conflict detection **before** the existing staged/unstaged match blocks. Insert this right after the `let x` and `let y` lines, before the `// Staged (index) changes` comment:

```rust
        // Unmerged (conflicted) entries — both columns are significant
        let is_conflict = matches!(
            (x, y),
            (b'U', b'U') | (b'A', b'A') | (b'D', b'U') | (b'U', b'D')
        );
        if is_conflict {
            let path = unquote_git_path(raw_path);
            entries.push(GitFileStatus {
                kind: FileChangeKind::Conflicted,
                path,
                original_path: None,
                staged: false,
            });
            continue;
        }
```

- [ ] **Step 4: Update `kind_to_tag` and `kind_to_color` in `git_diff.rs`**

In `src/app/git_diff.rs`, update both helper functions to handle the new variant:

In `kind_to_tag`:
```rust
FileChangeKind::Conflicted => "!",
```

In `kind_to_color`:
```rust
FileChangeKind::Conflicted => theme::active().warning,
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib git::parser -- --nocapture`
Expected: All 22 tests PASS (17 existing + 5 new)

- [ ] **Step 6: Run `cargo build` to verify no breakage**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles. Check for any non-exhaustive match warnings on `FileChangeKind` — add `Conflicted` arms to any match statements the compiler flags.

- [ ] **Step 7: Commit**

```bash
git add src/git/parser.rs src/app/git_diff.rs
git commit -m "Add Conflicted variant to git status parser"
```

---

### Task 4: Pane Content Variant — `ConflictResolver`

**Files:**
- Modify: `src/app/pane.rs`
- Modify: `src/app/persistence.rs`

Add the new `PaneContent::ConflictResolver` variant and its state struct.

- [ ] **Step 1: Add `ConflictResolverState` and pane variant**

In `src/app/pane.rs`:

1. Add the import at the top:
```rust
use super::conflict_parser::ConflictFile;
```

2. Add the state struct after `NoteEditorState`:
```rust
#[derive(Clone, Debug)]
pub(super) struct ConflictResolverState {
    pub(super) path: std::path::PathBuf,
    pub(super) content: ConflictFile,
    pub(super) resolved_count: usize,
    pub(super) scroll_offset: f32,
}
```

Note: `ConflictFile` must derive `Clone` and `Debug` — verify this was done in Task 1 (it was).

3. Add the variant to `PaneContent`:
```rust
pub(super) enum PaneContent {
    Terminal(u32),
    DeferredTerminal { ... },
    FileEditor(FileEditorState),
    FileDiff(FileDiffState),
    NoteEditor(NoteEditorState),
    ConflictResolver(ConflictResolverState),
}
```

- [ ] **Step 2: Skip `ConflictResolver` in persistence save**

In `src/app/persistence.rs`, find the function that converts `PaneContent` to `SavedPaneContent` (or wherever panes are serialized). The `ConflictResolver` pane should be skipped since conflicts are transient. Search for where `PaneContent` variants are matched during save and add:

```rust
PaneContent::ConflictResolver(_) => continue, // conflicts are transient, don't persist
```

This is likely in `src/app.rs` or `src/app/state.rs` where the save logic iterates panes. Search for `SavedPaneContent` or `SavedPane` construction.

- [ ] **Step 3: Handle `ConflictResolver` in all existing `match` arms on `PaneContent`**

Run `cargo build` and fix every non-exhaustive match error. The new variant should typically be a no-op or skip in existing match arms:

- In pane rendering (`pane_renderer.rs`): add a placeholder arm `PaneContent::ConflictResolver(_) => {}` (will be wired in Task 6)
- In title computation: show the file path
- In any save/persistence code: skip

- [ ] **Step 4: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1 | tail -10`
Expected: Compiles cleanly

- [ ] **Step 5: Update the pane content variant test**

In `src/app/pane.rs`, update `test_pane_content_variants` to include the new variant:

```rust
    let conflict = PaneContent::ConflictResolver(ConflictResolverState {
        path: PathBuf::from("conflict.rs"),
        content: crate::app::conflict_parser::ConflictFile {
            path: PathBuf::from("conflict.rs"),
            blocks: Vec::new(),
            total_conflicts: 0,
        },
        resolved_count: 0,
        scroll_offset: 0.0,
    });
    assert!(!format!("{:?}", conflict).is_empty());
```

- [ ] **Step 6: Run all tests**

Run: `cargo test 2>&1 | tail -10`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
git add src/app/pane.rs src/app/persistence.rs src/app.rs src/app/ui/pane_renderer.rs
git commit -m "Add ConflictResolver pane content variant"
```

---

### Task 5: Conflicts Section in Git Status Panel

**Files:**
- Modify: `src/app/git_diff.rs`

Add a "Conflicts" section at the top of the git status panel that lists files with `FileChangeKind::Conflicted` and emits `open_conflict_file` when clicked.

- [ ] **Step 1: Add `open_conflict_file` to `GitDiffResult`**

In `src/app/git_diff.rs`, add a new field to `GitDiffResult`:

```rust
pub(super) struct GitDiffResult {
    // ... existing fields ...
    pub(super) open_conflict_file: Option<String>,
}
```

Initialize it as `None` in the return statement at the bottom of `render_git_diff`. Add a `let mut open_conflict_file: Option<String> = None;` at the top of the function alongside the other mutable variables.

- [ ] **Step 2: Separate conflicted files from staged/unstaged**

In `render_git_diff`, after the `parse_git_status(status)` call and before building `staged`/`unstaged` vectors, add a third vector:

```rust
let mut conflicted: Vec<StatusEntry> = Vec::new();
```

Then in the loop where entries are sorted into `staged`/`unstaged`, add a check at the top:

```rust
for fs in &parsed {
    if fs.kind == FileChangeKind::Conflicted {
        conflicted.push(StatusEntry {
            tag: kind_to_tag(fs.kind),
            path: fs.path.clone(),
            color: kind_to_color(fs.kind),
            kind: fs.kind,
        });
        continue;
    }
    // ... existing staged/unstaged logic ...
}
```

- [ ] **Step 3: Render the Conflicts section**

Insert this section **before** the "Committed (unpushed)" section (i.e., right after building the `staged`/`unstaged`/`conflicted` vectors). Follow the same pattern as the staged section but with warning colors:

```rust
        // ── Conflicts section ──────────────────────────────────────
        if !conflicted.is_empty() {
            let t = theme::active();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("Conflicts ({})", conflicted.len()))
                        .strong()
                        .size(theme::FONT_UI_MD)
                        .color(t.warning),
                );
            });
            ui.add_space(theme::SP_2);
            for entry in &conflicted {
                ui.horizontal(|ui| {
                    ui.set_max_width(panel_width);
                    let (badge_rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
                    let badge_bg = entry.color.gamma_multiply(0.25);
                    ui.painter().rect_filled(badge_rect, theme::R_MD, badge_bg);
                    let badge_bg_rgb = [badge_bg.r(), badge_bg.g(), badge_bg.b()];
                    let badge_fg_rgb = [entry.color.r(), entry.color.g(), entry.color.b()];
                    let badge_fg = theme::ensure_readable(badge_fg_rgb, badge_bg_rgb);
                    ui.painter().text(
                        badge_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        entry.tag,
                        egui::FontId::monospace(theme::GIT_FONT_SZ),
                        badge_fg,
                    );
                    let label_max = (ui.available_width()).max(20.0);
                    let label_resp = ui
                        .allocate_ui(egui::vec2(label_max, 14.0), |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&entry.path)
                                        .monospace()
                                        .size(theme::FONT_UI_MD),
                                )
                                .truncate()
                                .sense(egui::Sense::click()),
                            )
                        })
                        .inner;
                    if label_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if label_resp.clicked() {
                        open_conflict_file = Some(entry.path.clone());
                    }
                });
            }
            ui.add_space(theme::SP_3);
            ui.separator();
            ui.add_space(theme::SP_2);
        }
```

- [ ] **Step 4: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1 | tail -5`
Expected: Compiles cleanly

- [ ] **Step 5: Commit**

```bash
git add src/app/git_diff.rs
git commit -m "Add Conflicts section to git status panel"
```

---

### Task 6: Conflict Resolver Pane — UI Rendering

**Files:**
- Create: `src/app/ui/conflict_resolver.rs`
- Modify: `src/app/ui/mod.rs`

Build the conflict resolver pane renderer with toolbar, conflict blocks, and action buttons.

- [ ] **Step 1: Register the module**

In `src/app/ui/mod.rs`, add:
```rust
pub(super) mod conflict_resolver;
```

- [ ] **Step 2: Create the renderer**

Create `src/app/ui/conflict_resolver.rs`:

```rust
use egui;

use crate::app::conflict_parser::{ConflictBlock, ConflictFile, Resolution};
use crate::app::pane::ConflictResolverState;
use crate::theme;

pub(in crate::app) enum ConflictAction {
    ResolveBlock { conflict_index: usize, resolution: Resolution },
    ResolveAllOurs,
    ResolveAllTheirs,
}

pub(in crate::app) fn render_conflict_resolver(
    ui: &mut egui::Ui,
    state: &ConflictResolverState,
) -> Option<ConflictAction> {
    let pane_rect = ui.max_rect();
    let t = theme::active();
    ui.painter().rect_filled(pane_rect, 0.0, t.bg_term);

    let mut action: Option<ConflictAction> = None;

    // ── Toolbar ──────────────────────────────────────────────────
    ui.horizontal(|ui| {
        // Left: file path
        ui.label(
            egui::RichText::new(format!("\u{26A0} {}", state.path.display()))
                .strong()
                .size(theme::FONT_UI_LG)
                .color(t.warning),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::SP_4);

            // Bulk actions (only show if there are unresolved conflicts)
            if state.resolved_count < state.content.total_conflicts {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("All Theirs")
                                .size(theme::FONT_UI_SM)
                                .color(t.error),
                        )
                        .rounding(theme::R_SM),
                    )
                    .clicked()
                {
                    action = Some(ConflictAction::ResolveAllTheirs);
                }
                ui.add_space(theme::SP_2);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("All Ours")
                                .size(theme::FONT_UI_SM)
                                .color(t.success),
                        )
                        .rounding(theme::R_SM),
                    )
                    .clicked()
                {
                    action = Some(ConflictAction::ResolveAllOurs);
                }
                ui.add_space(theme::SP_4);
            }

            // Progress
            ui.label(
                egui::RichText::new(format!(
                    "{}/{} resolved",
                    state.resolved_count, state.content.total_conflicts
                ))
                .size(theme::FONT_UI_SM)
                .color(if state.resolved_count == state.content.total_conflicts {
                    t.success
                } else {
                    t.subtext0
                }),
            );
        });
    });
    ui.separator();

    // ── Scrollable content ───────────────────────────────────────
    egui::ScrollArea::both()
        .id_source(("conflict_scroll", state.path.display().to_string()))
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let mut line_number: usize = 1;

            for block in &state.content.blocks {
                match block {
                    ConflictBlock::Context { lines } => {
                        for line in lines {
                            render_context_line(ui, line_number, line, t);
                            line_number += 1;
                        }
                    }
                    ConflictBlock::Conflict {
                        index,
                        ours_lines,
                        theirs_lines,
                        ours_label,
                        theirs_label,
                        resolved,
                    } => {
                        if let Some(resolution) = resolved {
                            // ── Resolved block ───────────────────
                            render_resolved_block(
                                ui,
                                &mut line_number,
                                ours_lines,
                                theirs_lines,
                                resolution,
                                t,
                            );
                        } else {
                            // ── Unresolved conflict block ────────
                            let block_action = render_conflict_block(
                                ui,
                                *index,
                                ours_lines,
                                theirs_lines,
                                ours_label,
                                theirs_label,
                                state.content.total_conflicts,
                                t,
                            );
                            if action.is_none() {
                                action = block_action;
                            }
                            // Don't increment line_number for conflict markers
                        }
                    }
                }
            }
        });

    action
}

fn render_context_line(ui: &mut egui::Ui, line_number: usize, content: &str, t: &theme::Theme) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{:>4} ", line_number))
                .monospace()
                .size(theme::FONT_TERM)
                .color(t.overlay0),
        );
        ui.label(
            egui::RichText::new(content)
                .monospace()
                .size(theme::FONT_TERM)
                .color(t.text),
        );
    });
}

fn render_resolved_block(
    ui: &mut egui::Ui,
    line_number: &mut usize,
    ours_lines: &[String],
    theirs_lines: &[String],
    resolution: &Resolution,
    t: &theme::Theme,
) {
    let label = match resolution {
        Resolution::Ours => "Resolved: Ours",
        Resolution::Theirs => "Resolved: Theirs",
        Resolution::Both => "Resolved: Both",
    };
    let resolved_bg = theme::blend_colors(t.surface0, t.overlay0, 0.08);

    egui::Frame::none()
        .fill(resolved_bg)
        .inner_margin(egui::Margin::symmetric(0.0, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(theme::SP_2);
                ui.label(
                    egui::RichText::new(label)
                        .size(theme::FONT_UI_XS)
                        .color(t.overlay0)
                        .italics(),
                );
            });

            let lines = match resolution {
                Resolution::Ours => ours_lines,
                Resolution::Theirs => theirs_lines,
                Resolution::Both => ours_lines, // render ours first for Both
            };

            for line in lines {
                render_context_line(ui, *line_number, line, t);
                *line_number += 1;
            }

            if matches!(resolution, Resolution::Both) {
                for line in theirs_lines {
                    render_context_line(ui, *line_number, line, t);
                    *line_number += 1;
                }
            }
        });
}

fn render_conflict_block(
    ui: &mut egui::Ui,
    conflict_index: usize,
    ours_lines: &[String],
    theirs_lines: &[String],
    ours_label: &str,
    theirs_label: &str,
    total_conflicts: usize,
    t: &theme::Theme,
) -> Option<ConflictAction> {
    let mut action: Option<ConflictAction> = None;

    let ours_bg = theme::blend_colors(t.surface0, t.success, theme::BLEND_SUBTLE);
    let theirs_bg = theme::blend_colors(t.surface0, t.error, theme::BLEND_SUBTLE);

    // ── Floating action bar ──────────────────────────────────────
    egui::Frame::none()
        .fill(t.surface1)
        .rounding(egui::Rounding {
            nw: theme::R_SM,
            ne: theme::R_SM,
            sw: 0.0,
            se: 0.0,
        })
        .inner_margin(egui::Margin::symmetric(theme::SP_3, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "CONFLICT {}/{}",
                        conflict_index + 1,
                        total_conflicts
                    ))
                    .strong()
                    .size(theme::FONT_UI_XS)
                    .color(t.text),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Both")
                                    .size(theme::FONT_UI_XS)
                                    .color(t.base),
                            )
                            .fill(t.accent)
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        action = Some(ConflictAction::ResolveBlock {
                            conflict_index,
                            resolution: Resolution::Both,
                        });
                    }
                    ui.add_space(theme::SP_1);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Theirs")
                                    .size(theme::FONT_UI_XS)
                                    .color(t.base),
                            )
                            .fill(t.error)
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        action = Some(ConflictAction::ResolveBlock {
                            conflict_index,
                            resolution: Resolution::Theirs,
                        });
                    }
                    ui.add_space(theme::SP_1);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Ours")
                                    .size(theme::FONT_UI_XS)
                                    .color(t.base),
                            )
                            .fill(t.success)
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        action = Some(ConflictAction::ResolveBlock {
                            conflict_index,
                            resolution: Resolution::Ours,
                        });
                    }
                });
            });
        });

    // ── Ours section ─────────────────────────────────────────────
    egui::Frame::none()
        .fill(ours_bg)
        .inner_margin(egui::Margin::symmetric(0.0, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(theme::SP_2);
                ui.label(
                    egui::RichText::new(format!("\u{25C0} OURS ({})", ours_label))
                        .size(theme::FONT_UI_XS)
                        .strong()
                        .color(t.success),
                );
            });
            for line in ours_lines {
                ui.horizontal(|ui| {
                    // 3px green left border
                    let (border_rect, _) =
                        ui.allocate_exact_size(egui::vec2(3.0, ui.spacing().interact_size.y), egui::Sense::hover());
                    ui.painter().rect_filled(border_rect, 0.0, t.success);
                    ui.add_space(theme::SP_1);
                    ui.label(
                        egui::RichText::new(line)
                            .monospace()
                            .size(theme::FONT_TERM)
                            .color(t.success),
                    );
                });
            }
        });

    // ── Separator ────────────────────────────────────────────────
    ui.horizontal(|ui| {
        ui.add_space(theme::SP_2);
        ui.label(
            egui::RichText::new("\u{2550}".repeat(30))
                .monospace()
                .size(theme::FONT_UI_XS)
                .color(t.overlay0),
        );
    });

    // ── Theirs section ───────────────────────────────────────────
    egui::Frame::none()
        .fill(theirs_bg)
        .inner_margin(egui::Margin::symmetric(0.0, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(theme::SP_2);
                ui.label(
                    egui::RichText::new(format!("\u{25B6} THEIRS ({})", theirs_label))
                        .size(theme::FONT_UI_XS)
                        .strong()
                        .color(t.error),
                );
            });
            for line in theirs_lines {
                ui.horizontal(|ui| {
                    let (border_rect, _) =
                        ui.allocate_exact_size(egui::vec2(3.0, ui.spacing().interact_size.y), egui::Sense::hover());
                    ui.painter().rect_filled(border_rect, 0.0, t.error);
                    ui.add_space(theme::SP_1);
                    ui.label(
                        egui::RichText::new(line)
                            .monospace()
                            .size(theme::FONT_TERM)
                            .color(t.error),
                    );
                });
            }
        });

    action
}
```

- [ ] **Step 3: Check if `blend_colors` exists in theme.rs**

The renderer uses `theme::blend_colors(a, b, factor)`. Search for this function in `src/theme.rs`. If it doesn't exist, add it:

```rust
pub fn blend_colors(base: Color32, overlay: Color32, factor: f32) -> Color32 {
    let r = (base.r() as f32 * (1.0 - factor) + overlay.r() as f32 * factor) as u8;
    let g = (base.g() as f32 * (1.0 - factor) + overlay.g() as f32 * factor) as u8;
    let b = (base.b() as f32 * (1.0 - factor) + overlay.b() as f32 * factor) as u8;
    Color32::from_rgb(r, g, b)
}
```

Check existing code for a `blend` function that already does this — if one exists with a different name, use it instead.

- [ ] **Step 4: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1 | tail -10`
Expected: Compiles. Fix any type/import issues.

- [ ] **Step 5: Commit**

```bash
git add src/app/ui/conflict_resolver.rs src/app/ui/mod.rs src/theme.rs
git commit -m "Add conflict resolver pane UI renderer"
```

---

### Task 7: Wire Pane Renderer and App Integration

**Files:**
- Modify: `src/app/ui/pane_renderer.rs`
- Modify: `src/app.rs`

Connect the conflict resolver to the pane renderer and handle conflict pane creation from git status clicks.

- [ ] **Step 1: Dispatch ConflictResolver in pane renderer**

In `src/app/ui/pane_renderer.rs`, find the `render_file_diff_leaf` call site (around line 658-723) and the match on `PaneContent` where different pane types are rendered. Add a branch for `ConflictResolver`:

Find the match arm pattern where `PaneContent::FileDiff` is handled and add nearby:

```rust
PaneContent::ConflictResolver(ref state) => {
    let conflict_action = super::ui::conflict_resolver::render_conflict_resolver(ui, state);
    if let Some(action) = conflict_action {
        pane_context_actions.push(PaneContextAction::ConflictResolve {
            pane_id,
            action,
        });
    }
}
```

Add `ConflictResolve` to the `PaneContextAction` enum:

```rust
pub(crate) enum PaneContextAction {
    // ... existing variants ...
    ConflictResolve {
        pane_id: u32,
        action: super::ui::conflict_resolver::ConflictAction,
    },
}
```

- [ ] **Step 2: Handle `open_conflict_file` in `app.rs`**

In `src/app.rs`, find where `git_open_diff_file` is handled (around line 3721). Add similar handling for `open_conflict_file` right after:

1. Add `let mut git_open_conflict_file: Option<String> = None;` alongside the other `git_open_*` variables.

2. In the block where `render_git_diff` result is processed (around line 1319), add:
```rust
if result.open_conflict_file.is_some() {
    git_open_conflict_file = result.open_conflict_file;
}
```

3. After the `git_open_diff_file` handling block (around line 3759), add:

```rust
        // 9b-conflict. Conflicted file clicked → open ConflictResolver pane
        if let Some(rel_path) = git_open_conflict_file {
            if let Some(cwd) = self.active_cwd() {
                let full_path = cwd.join(&rel_path);
                let existing_id = self
                    .pane_state
                    .panes
                    .iter()
                    .find(|p| matches!(&p.content, PaneContent::ConflictResolver(s) if s.path == full_path))
                    .map(|p| p.id);
                if let Some(pid) = existing_id {
                    self.activate_pane(pid);
                } else {
                    match std::fs::read_to_string(&full_path) {
                        Ok(file_content) => {
                            let conflict_file = conflict_parser::parse_conflict_file(&full_path, &file_content);
                            if conflict_file.total_conflicts == 0 {
                                self.flash.trigger(
                                    crate::app::feedback::FlashTarget::Global,
                                    crate::app::feedback::FlashKind::Neutral,
                                );
                            } else {
                                let pane_id = self.pane_state.next_pane_id;
                                self.pane_state.next_pane_id += 1;
                                self.pane_state.panes.push(PaneEntry {
                                    id: pane_id,
                                    content: PaneContent::ConflictResolver(ConflictResolverState {
                                        path: full_path,
                                        content: conflict_file,
                                        resolved_count: 0,
                                        scroll_offset: 0.0,
                                    }),
                                    manual_width: None,
                                    last_size: (0, 0),
                                });
                                self.pane_state.pane_trees.insert(
                                    pane_id,
                                    crate::pane_tree::PaneNode::Leaf {
                                        pane_id,
                                        last_size: (0, 0),
                                    },
                                );
                                self.activate_pane(pane_id);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to read conflict file {}: {}", full_path.display(), e);
                            self.flash.trigger(
                                crate::app::feedback::FlashTarget::Global,
                                crate::app::feedback::FlashKind::Error,
                            );
                        }
                    }
                }
            }
        }
```

- [ ] **Step 3: Handle `ConflictResolve` pane context actions**

In `src/app.rs`, find where `PaneContextAction` variants are processed (search for `pane_context_actions` processing). Add handling for the new variant:

```rust
PaneContextAction::ConflictResolve { pane_id, action } => {
    if let Some(pane) = self.pane_state.panes.iter_mut().find(|p| p.id == pane_id) {
        if let PaneContent::ConflictResolver(ref mut state) = pane.content {
            use crate::app::ui::conflict_resolver::ConflictAction;
            match action {
                ConflictAction::ResolveBlock { conflict_index, resolution } => {
                    state.resolved_count = conflict_parser::resolve_block(
                        &mut state.content,
                        conflict_index,
                        resolution,
                    );
                }
                ConflictAction::ResolveAllOurs => {
                    state.resolved_count = conflict_parser::resolve_all(
                        &mut state.content,
                        conflict_parser::Resolution::Ours,
                    );
                }
                ConflictAction::ResolveAllTheirs => {
                    state.resolved_count = conflict_parser::resolve_all(
                        &mut state.content,
                        conflict_parser::Resolution::Theirs,
                    );
                }
            }
                    
            // Write resolved file to disk
            if let Err(e) = conflict_parser::write_resolved_file(&state.path, &state.content.blocks) {
                log::error!("Failed to write resolved file: {}", e);
                self.flash.trigger(
                    feedback::FlashTarget::Pane(pane_id),
                    feedback::FlashKind::Error,
                );
            } else if state.resolved_count == state.content.total_conflicts {
                // All conflicts resolved
                self.flash.trigger(
                    feedback::FlashTarget::Pane(pane_id),
                    feedback::FlashKind::Success,
                );
            }
        }
    }
}
```

- [ ] **Step 4: Add necessary imports to `app.rs`**

Ensure these are imported at the top of `src/app.rs`:

```rust
use pane::ConflictResolverState;
```

(It should already have the `use pane::{...}` block — add `ConflictResolverState` to it.)

- [ ] **Step 5: Run `cargo build` to verify compilation**

Run: `cargo build 2>&1 | tail -10`
Expected: Compiles cleanly

- [ ] **Step 6: Run all tests**

Run: `cargo test 2>&1 | tail -10`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
git add src/app/ui/pane_renderer.rs src/app.rs
git commit -m "Wire conflict resolver to pane renderer and app integration"
```

---

### Task 8: Final Verification and Cleanup

**Files:**
- All modified files

Run fmt, clippy, full test suite, and verify the build.

- [ ] **Step 1: Run `cargo fmt`**

Run: `cargo fmt`

- [ ] **Step 2: Run `cargo clippy`**

Run: `cargo clippy 2>&1 | tail -20`
Expected: No errors. Fix any warnings.

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1 | tail -10`
Expected: All tests pass (333 existing + ~26 new ≈ 359 tests)

- [ ] **Step 4: Run the app to verify it builds and launches**

Run: `cargo run` — verify the app opens. Navigate to the git diff panel and confirm no crash. If you have a repo with merge conflicts available, test the full flow.

- [ ] **Step 5: Commit any cleanup**

```bash
git add -A
git commit -m "Format and clippy cleanup for conflict resolver"
```
