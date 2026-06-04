#![allow(dead_code)] // types and functions consumed by later integration tasks

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // variants used in future resolution UI
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

/// Parse a file's content for git conflict markers, returning a structured
/// `ConflictFile` with alternating `Context` and `Conflict` blocks.
///
/// Marker format:
/// ```text
/// <<<<<<< ours_label
/// ... ours lines ...
/// =======
/// ... theirs lines ...
/// >>>>>>> theirs_label
/// ```
///
/// Malformed conflicts (missing `=======` or `>>>>>>>` before the next
/// `<<<<<<<` or end-of-file) are treated as plain context lines.
pub fn parse_conflict_file(path: &Path, content: &str) -> ConflictFile {
    let lines: Vec<&str> = content.lines().collect();

    let mut blocks: Vec<ConflictBlock> = Vec::new();
    let mut conflict_index: usize = 0;
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("<<<<<<<") {
            // Look ahead for ======= and >>>>>>>
            let open_i = i;
            let ours_label = lines[i].get(8..).unwrap_or("").trim().to_string();

            // Find the separator
            let sep_i = find_separator(&lines, open_i + 1);

            if let Some(sep_i) = sep_i {
                // Find the closing marker, starting after the separator
                let close_i = find_close_marker(&lines, sep_i + 1);

                if let Some(close_i) = close_i {
                    let theirs_label = lines[close_i].get(8..).unwrap_or("").trim().to_string();

                    let ours_lines: Vec<String> = lines[open_i + 1..sep_i]
                        .iter()
                        .map(|l| l.to_string())
                        .collect();
                    let theirs_lines: Vec<String> = lines[sep_i + 1..close_i]
                        .iter()
                        .map(|l| l.to_string())
                        .collect();

                    push_conflict(
                        &mut blocks,
                        conflict_index,
                        ours_lines,
                        theirs_lines,
                        ours_label,
                        theirs_label,
                    );
                    conflict_index += 1;
                    i = close_i + 1;
                    continue;
                }
            }

            // Malformed: treat the <<<<<<< line as context and advance by one
            push_context_line(&mut blocks, lines[i]);
            i += 1;
        } else {
            push_context_line(&mut blocks, lines[i]);
            i += 1;
        }
    }

    let total_conflicts = conflict_index;
    ConflictFile {
        path: path.to_path_buf(),
        blocks,
        total_conflicts,
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Search forward from `start` for the first `=======` line that is NOT itself
/// a conflict-open marker.  Stops (returns `None`) if it hits another
/// `<<<<<<<` first, because that would mean the current conflict is malformed.
fn find_separator(lines: &[&str], start: usize) -> Option<usize> {
    for (i, line) in lines.iter().enumerate().skip(start) {
        if line.starts_with("<<<<<<<") {
            return None;
        }
        if *line == "=======" || line.starts_with("=======") && line.trim() == "=======" {
            return Some(i);
        }
    }
    None
}

/// Search forward from `start` for the first `>>>>>>>` line.  Stops (returns
/// `None`) if it hits another `<<<<<<<` first.
fn find_close_marker(lines: &[&str], start: usize) -> Option<usize> {
    for (i, line) in lines.iter().enumerate().skip(start) {
        if line.starts_with("<<<<<<<") {
            return None;
        }
        if line.starts_with(">>>>>>>") {
            return Some(i);
        }
    }
    None
}

/// Append `line` to the last `Context` block, or create a new one.
fn push_context_line(blocks: &mut Vec<ConflictBlock>, line: &str) {
    match blocks.last_mut() {
        Some(ConflictBlock::Context { lines }) => {
            lines.push(line.to_string());
        }
        _ => {
            blocks.push(ConflictBlock::Context {
                lines: vec![line.to_string()],
            });
        }
    }
}

fn push_conflict(
    blocks: &mut Vec<ConflictBlock>,
    index: usize,
    ours_lines: Vec<String>,
    theirs_lines: Vec<String>,
    ours_label: String,
    theirs_label: String,
) {
    blocks.push(ConflictBlock::Conflict {
        index,
        ours_lines,
        theirs_lines,
        ours_label,
        theirs_label,
        resolved: None,
    });
}

// ── reconstruction & resolution ───────────────────────────────────────────────

/// Reconstruct file content from a list of `ConflictBlock`s.
///
/// Resolved conflicts emit only the chosen side(s); unresolved conflicts emit
/// the full marker/separator/close syntax so the file remains valid.
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

/// Mark a single conflict block (by its `index`) as resolved with `resolution`.
/// Returns the total number of resolved conflicts in the file.
pub fn resolve_block(
    file: &mut ConflictFile,
    conflict_index: usize,
    resolution: Resolution,
) -> usize {
    for block in &mut file.blocks {
        if let ConflictBlock::Conflict {
            index, resolved, ..
        } = block
        {
            if *index == conflict_index {
                *resolved = Some(resolution);
                break;
            }
        }
    }
    resolved_count(file)
}

/// Mark every unresolved conflict with `resolution`.
/// Returns the total number of resolved conflicts in the file.
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

/// Count how many conflicts currently have a resolution set.
pub fn resolved_count(file: &ConflictFile) -> usize {
    file.blocks
        .iter()
        .filter(|b| {
            matches!(
                b,
                ConflictBlock::Conflict {
                    resolved: Some(_),
                    ..
                }
            )
        })
        .count()
}

/// Write the conflict file back to disk, substituting resolved blocks.
pub fn write_resolved_file(path: &Path, blocks: &[ConflictBlock]) -> anyhow::Result<()> {
    let content = reconstruct_content(blocks);
    crate::util::atomic_write(path, &content)?;
    Ok(())
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn context_lines(block: &ConflictBlock) -> &Vec<String> {
        match block {
            ConflictBlock::Context { lines } => lines,
            _ => panic!("expected Context block"),
        }
    }

    fn conflict_fields(
        block: &ConflictBlock,
    ) -> (
        usize,
        &Vec<String>,
        &Vec<String>,
        &str,
        &str,
        &Option<Resolution>,
    ) {
        match block {
            ConflictBlock::Conflict {
                index,
                ours_lines,
                theirs_lines,
                ours_label,
                theirs_label,
                resolved,
            } => (
                *index,
                ours_lines,
                theirs_lines,
                ours_label,
                theirs_label,
                resolved,
            ),
            _ => panic!("expected Conflict block"),
        }
    }

    // ── tests ─────────────────────────────────────────────────────────────────

    #[test]
    fn no_conflicts_returns_single_context_block() {
        let content = "line one\nline two\nline three";
        let cf = parse_conflict_file(Path::new("file.txt"), content);

        assert_eq!(cf.total_conflicts, 0);
        assert_eq!(cf.blocks.len(), 1);
        assert_eq!(
            context_lines(&cf.blocks[0]),
            &["line one", "line two", "line three"]
        );
    }

    #[test]
    fn single_conflict_parsed_correctly() {
        let content = "\
before
<<<<<<< HEAD
ours line
=======
theirs line
>>>>>>> branch
after";
        let cf = parse_conflict_file(Path::new("file.txt"), content);

        assert_eq!(cf.total_conflicts, 1);
        // blocks: context, conflict, context
        assert_eq!(cf.blocks.len(), 3);

        assert_eq!(context_lines(&cf.blocks[0]), &["before"]);

        let (idx, ours, theirs, ol, tl, resolved) = conflict_fields(&cf.blocks[1]);
        assert_eq!(idx, 0);
        assert_eq!(ours, &["ours line"]);
        assert_eq!(theirs, &["theirs line"]);
        assert_eq!(ol, "HEAD");
        assert_eq!(tl, "branch");
        assert_eq!(resolved, &None);

        assert_eq!(context_lines(&cf.blocks[2]), &["after"]);
    }

    #[test]
    fn multiple_conflicts_parsed() {
        let content = "\
ctx1
<<<<<<< HEAD
a1
=======
b1
>>>>>>> br1
ctx2
<<<<<<< HEAD
a2
=======
b2
>>>>>>> br2
ctx3";
        let cf = parse_conflict_file(Path::new("file.txt"), content);

        assert_eq!(cf.total_conflicts, 2);
        // context, conflict0, context, conflict1, context
        assert_eq!(cf.blocks.len(), 5);

        let (idx0, _, _, _, _, _) = conflict_fields(&cf.blocks[1]);
        assert_eq!(idx0, 0);

        let (idx1, _, _, _, _, _) = conflict_fields(&cf.blocks[3]);
        assert_eq!(idx1, 1);

        assert_eq!(context_lines(&cf.blocks[0]), &["ctx1"]);
        assert_eq!(context_lines(&cf.blocks[2]), &["ctx2"]);
        assert_eq!(context_lines(&cf.blocks[4]), &["ctx3"]);
    }

    #[test]
    fn empty_ours_section() {
        let content = "\
<<<<<<< HEAD
=======
theirs
>>>>>>> br";
        let cf = parse_conflict_file(Path::new("f"), content);

        assert_eq!(cf.total_conflicts, 1);
        let (_, ours, theirs, _, _, _) = conflict_fields(&cf.blocks[0]);
        assert!(ours.is_empty());
        assert_eq!(theirs, &["theirs"]);
    }

    #[test]
    fn empty_theirs_section() {
        let content = "\
<<<<<<< HEAD
ours
=======
>>>>>>> br";
        let cf = parse_conflict_file(Path::new("f"), content);

        assert_eq!(cf.total_conflicts, 1);
        let (_, ours, theirs, _, _, _) = conflict_fields(&cf.blocks[0]);
        assert_eq!(ours, &["ours"]);
        assert!(theirs.is_empty());
    }

    #[test]
    fn labels_extracted_from_markers() {
        let content = "<<<<<<< my-branch\nours\n=======\ntheirs\n>>>>>>> their-branch";
        let cf = parse_conflict_file(Path::new("f"), content);

        let (_, _, _, ol, tl, _) = conflict_fields(&cf.blocks[0]);
        assert_eq!(ol, "my-branch");
        assert_eq!(tl, "their-branch");
    }

    #[test]
    fn malformed_missing_separator_treated_as_context() {
        // <<<<<<< with no ======= at all → treat the <<<<<<< line as context
        let content = "<<<<<<< HEAD\nsome line\nno separator here";
        let cf = parse_conflict_file(Path::new("f"), content);

        assert_eq!(cf.total_conflicts, 0);
        // All lines end up in context
        let lines = context_lines(&cf.blocks[0]);
        assert!(lines.contains(&"<<<<<<< HEAD".to_string()));
        assert!(lines.contains(&"some line".to_string()));
    }

    #[test]
    fn malformed_missing_end_marker_treated_as_context() {
        // <<<<<<< and ======= but no >>>>>>>
        let content = "<<<<<<< HEAD\nours\n=======\ntheirs";
        let cf = parse_conflict_file(Path::new("f"), content);

        assert_eq!(cf.total_conflicts, 0);
        let lines = context_lines(&cf.blocks[0]);
        assert!(lines.contains(&"<<<<<<< HEAD".to_string()));
    }

    #[test]
    fn empty_file() {
        let cf = parse_conflict_file(Path::new("f"), "");
        assert_eq!(cf.total_conflicts, 0);
        assert!(cf.blocks.is_empty());
    }

    #[test]
    fn conflict_at_start_of_file() {
        let content = "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> br\nafter";
        let cf = parse_conflict_file(Path::new("f"), content);

        assert_eq!(cf.total_conflicts, 1);
        assert_eq!(cf.blocks.len(), 2);
        let (_, ours, theirs, _, _, _) = conflict_fields(&cf.blocks[0]);
        assert_eq!(ours, &["ours"]);
        assert_eq!(theirs, &["theirs"]);
        assert_eq!(context_lines(&cf.blocks[1]), &["after"]);
    }

    #[test]
    fn conflict_at_end_of_file() {
        let content = "before\n<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> br";
        let cf = parse_conflict_file(Path::new("f"), content);

        assert_eq!(cf.total_conflicts, 1);
        assert_eq!(cf.blocks.len(), 2);
        assert_eq!(context_lines(&cf.blocks[0]), &["before"]);
        let (_, ours, theirs, _, _, _) = conflict_fields(&cf.blocks[1]);
        assert_eq!(ours, &["ours"]);
        assert_eq!(theirs, &["theirs"]);
    }

    #[test]
    fn path_preserved() {
        let path = Path::new("/some/repo/file.rs");
        let cf = parse_conflict_file(path, "hello");
        assert_eq!(cf.path, path);
    }

    // ── reconstruction & resolution tests ────────────────────────────────────

    #[test]
    fn reconstruct_no_conflicts() {
        let blocks = vec![ConflictBlock::Context {
            lines: vec!["line 1".to_string(), "line 2".to_string()],
        }];
        assert_eq!(reconstruct_content(&blocks), "line 1\nline 2\n");
    }

    #[test]
    fn reconstruct_resolved_ours() {
        let blocks = vec![
            ConflictBlock::Context {
                lines: vec!["before".to_string()],
            },
            ConflictBlock::Conflict {
                index: 0,
                ours_lines: vec!["kept".to_string()],
                theirs_lines: vec!["dropped".to_string()],
                ours_label: "HEAD".to_string(),
                theirs_label: "br".to_string(),
                resolved: Some(Resolution::Ours),
            },
            ConflictBlock::Context {
                lines: vec!["after".to_string()],
            },
        ];
        assert_eq!(reconstruct_content(&blocks), "before\nkept\nafter\n");
    }

    #[test]
    fn reconstruct_resolved_theirs() {
        let blocks = vec![ConflictBlock::Conflict {
            index: 0,
            ours_lines: vec!["dropped".to_string()],
            theirs_lines: vec!["kept".to_string()],
            ours_label: "HEAD".to_string(),
            theirs_label: "br".to_string(),
            resolved: Some(Resolution::Theirs),
        }];
        assert_eq!(reconstruct_content(&blocks), "kept\n");
    }

    #[test]
    fn reconstruct_resolved_both() {
        let blocks = vec![ConflictBlock::Conflict {
            index: 0,
            ours_lines: vec!["ours".to_string()],
            theirs_lines: vec!["theirs".to_string()],
            ours_label: "HEAD".to_string(),
            theirs_label: "br".to_string(),
            resolved: Some(Resolution::Both),
        }];
        assert_eq!(reconstruct_content(&blocks), "ours\ntheirs\n");
    }

    #[test]
    fn reconstruct_unresolved_preserves_markers() {
        let blocks = vec![ConflictBlock::Conflict {
            index: 0,
            ours_lines: vec!["ours line".to_string()],
            theirs_lines: vec!["theirs line".to_string()],
            ours_label: "HEAD".to_string(),
            theirs_label: "branch".to_string(),
            resolved: None,
        }];
        let expected = "<<<<<<< HEAD\nours line\n=======\ntheirs line\n>>>>>>> branch\n";
        assert_eq!(reconstruct_content(&blocks), expected);
    }

    #[test]
    fn reconstruct_partial_resolution() {
        // First conflict resolved (Ours), second still unresolved
        let blocks = vec![
            ConflictBlock::Conflict {
                index: 0,
                ours_lines: vec!["kept".to_string()],
                theirs_lines: vec!["not used".to_string()],
                ours_label: "HEAD".to_string(),
                theirs_label: "br".to_string(),
                resolved: Some(Resolution::Ours),
            },
            ConflictBlock::Conflict {
                index: 1,
                ours_lines: vec!["a".to_string()],
                theirs_lines: vec!["b".to_string()],
                ours_label: "HEAD".to_string(),
                theirs_label: "br".to_string(),
                resolved: None,
            },
        ];
        let result = reconstruct_content(&blocks);
        assert!(result.starts_with("kept\n"));
        assert!(result.contains("<<<<<<< HEAD"));
        assert!(result.contains(">>>>>>> br"));
    }

    #[test]
    fn resolve_block_updates_state() {
        let content = "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> br";
        let mut file = parse_conflict_file(Path::new("f"), content);
        let count = resolve_block(&mut file, 0, Resolution::Theirs);
        assert_eq!(count, 1);
        if let ConflictBlock::Conflict { resolved, .. } = &file.blocks[0] {
            assert_eq!(resolved, &Some(Resolution::Theirs));
        } else {
            panic!("expected Conflict block at index 0");
        }
    }

    #[test]
    fn resolve_all_blocks() {
        let content = "\
<<<<<<< HEAD
a1
=======
b1
>>>>>>> br1
<<<<<<< HEAD
a2
=======
b2
>>>>>>> br2";
        let mut file = parse_conflict_file(Path::new("f"), content);
        let count = resolve_all(&mut file, Resolution::Ours);
        assert_eq!(count, 2);
    }

    #[test]
    fn reconstruct_empty_ours_resolved() {
        let blocks = vec![ConflictBlock::Conflict {
            index: 0,
            ours_lines: vec![],
            theirs_lines: vec!["theirs".to_string()],
            ours_label: "HEAD".to_string(),
            theirs_label: "br".to_string(),
            resolved: Some(Resolution::Ours),
        }];
        assert_eq!(reconstruct_content(&blocks), "");
    }
}
