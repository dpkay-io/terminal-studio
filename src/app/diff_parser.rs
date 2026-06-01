#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct DiffLine {
    pub(super) kind: DiffLineKind,
    pub(super) old_lineno: Option<usize>,
    pub(super) new_lineno: Option<usize>,
    pub(super) content: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct DiffHunk {
    pub(super) old_start: usize,
    pub(super) old_count: usize,
    pub(super) new_start: usize,
    pub(super) new_count: usize,
    pub(super) lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum DiffViewMode {
    Inline,
    SideBySide,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SideBySideLine {
    pub(super) lineno: Option<usize>,
    pub(super) content: Option<String>,
    pub(super) kind: DiffLineKind,
}

pub(super) fn parse_diff(raw: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line: usize = 0;
    let mut new_line: usize = 0;

    for line in raw.lines() {
        if line.starts_with("@@") {
            if let Some(h) = current_hunk.take() {
                hunks.push(h);
            }
            if let Some((os, oc, ns, nc)) = parse_hunk_header(line) {
                old_line = os;
                new_line = ns;
                current_hunk = Some(DiffHunk {
                    old_start: os,
                    old_count: oc,
                    new_start: ns,
                    new_count: nc,
                    lines: Vec::new(),
                });
            }
        } else if line.starts_with("diff --git ")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("new file")
            || line.starts_with("deleted file")
            || line.starts_with("old mode")
            || line.starts_with("new mode")
            || line.starts_with("similarity index")
            || line.starts_with("rename from")
            || line.starts_with("rename to")
            || line.starts_with("\\ No newline")
        {
            // skip metadata
        } else if let Some(hunk) = current_hunk.as_mut() {
            if let Some(rest) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Added,
                    old_lineno: None,
                    new_lineno: Some(new_line),
                    content: rest.to_string(),
                });
                new_line += 1;
            } else if let Some(rest) = line.strip_prefix('-') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    old_lineno: Some(old_line),
                    new_lineno: None,
                    content: rest.to_string(),
                });
                old_line += 1;
            } else {
                let content = line.strip_prefix(' ').unwrap_or(line);
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Context,
                    old_lineno: Some(old_line),
                    new_lineno: Some(new_line),
                    content: content.to_string(),
                });
                old_line += 1;
                new_line += 1;
            }
        }
    }
    if let Some(h) = current_hunk {
        hunks.push(h);
    }
    hunks
}

fn parse_hunk_header(line: &str) -> Option<(usize, usize, usize, usize)> {
    let after_at = line.strip_prefix("@@ ")?;
    let rest = after_at.split(" @@").next()?;
    let mut parts = rest.split(' ');
    let old_part = parts.next()?.strip_prefix('-')?;
    let new_part = parts.next()?.strip_prefix('+')?;

    let (os, oc) = parse_range(old_part);
    let (ns, nc) = parse_range(new_part);
    Some((os, oc, ns, nc))
}

fn parse_range(s: &str) -> (usize, usize) {
    if let Some((start, count)) = s.split_once(',') {
        (
            start.parse().unwrap_or(0),
            count.parse().unwrap_or(0),
        )
    } else {
        let start = s.parse().unwrap_or(0);
        (start, if start == 0 { 0 } else { 1 })
    }
}

pub(super) fn build_full_diff_lines(
    old_content: &str,
    new_content: &str,
    hunks: &[DiffHunk],
) -> Vec<DiffLine> {
    let old_lines: Vec<&str> = if old_content.is_empty() {
        Vec::new()
    } else {
        old_content.lines().collect()
    };
    let new_lines: Vec<&str> = if new_content.is_empty() {
        Vec::new()
    } else {
        new_content.lines().collect()
    };

    if hunks.is_empty() {
        let source = if !new_lines.is_empty() { &new_lines } else { &old_lines };
        return source
            .iter()
            .enumerate()
            .map(|(i, line)| DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(i + 1),
                new_lineno: Some(i + 1),
                content: line.to_string(),
            })
            .collect();
    }

    let mut result = Vec::new();
    let mut old_cursor: usize = 1;
    let mut new_cursor: usize = 1;

    for hunk in hunks {
        let hunk_old_start = hunk.old_start.max(1);
        while old_cursor < hunk_old_start && old_cursor <= old_lines.len() {
            let content = old_lines[old_cursor - 1];
            result.push(DiffLine {
                kind: DiffLineKind::Context,
                old_lineno: Some(old_cursor),
                new_lineno: Some(new_cursor),
                content: content.to_string(),
            });
            old_cursor += 1;
            new_cursor += 1;
        }

        for line in &hunk.lines {
            result.push(line.clone());
            match line.kind {
                DiffLineKind::Context => {
                    old_cursor += 1;
                    new_cursor += 1;
                }
                DiffLineKind::Removed => {
                    old_cursor += 1;
                }
                DiffLineKind::Added => {
                    new_cursor += 1;
                }
            }
        }
    }

    while new_cursor <= new_lines.len() {
        let content = new_lines[new_cursor - 1];
        result.push(DiffLine {
            kind: DiffLineKind::Context,
            old_lineno: Some(old_cursor),
            new_lineno: Some(new_cursor),
            content: content.to_string(),
        });
        old_cursor += 1;
        new_cursor += 1;
    }

    result
}

pub(super) fn build_side_by_side_lines(
    full_lines: &[DiffLine],
) -> (Vec<SideBySideLine>, Vec<SideBySideLine>) {
    let mut left = Vec::new();
    let mut right = Vec::new();

    let mut i = 0;
    while i < full_lines.len() {
        let line = &full_lines[i];
        match line.kind {
            DiffLineKind::Context => {
                left.push(SideBySideLine {
                    lineno: line.old_lineno,
                    content: Some(line.content.clone()),
                    kind: DiffLineKind::Context,
                });
                right.push(SideBySideLine {
                    lineno: line.new_lineno,
                    content: Some(line.content.clone()),
                    kind: DiffLineKind::Context,
                });
                i += 1;
            }
            DiffLineKind::Removed => {
                let rm_start = i;
                while i < full_lines.len() && full_lines[i].kind == DiffLineKind::Removed {
                    i += 1;
                }
                let add_start = i;
                while i < full_lines.len() && full_lines[i].kind == DiffLineKind::Added {
                    i += 1;
                }
                let removed = &full_lines[rm_start..add_start];
                let added = &full_lines[add_start..i];
                let max_len = removed.len().max(added.len());
                for j in 0..max_len {
                    if let Some(rm) = removed.get(j) {
                        left.push(SideBySideLine {
                            lineno: rm.old_lineno,
                            content: Some(rm.content.clone()),
                            kind: DiffLineKind::Removed,
                        });
                    } else {
                        left.push(SideBySideLine {
                            lineno: None,
                            content: None,
                            kind: DiffLineKind::Context,
                        });
                    }
                    if let Some(ad) = added.get(j) {
                        right.push(SideBySideLine {
                            lineno: ad.new_lineno,
                            content: Some(ad.content.clone()),
                            kind: DiffLineKind::Added,
                        });
                    } else {
                        right.push(SideBySideLine {
                            lineno: None,
                            content: None,
                            kind: DiffLineKind::Context,
                        });
                    }
                }
            }
            DiffLineKind::Added => {
                left.push(SideBySideLine {
                    lineno: None,
                    content: None,
                    kind: DiffLineKind::Context,
                });
                right.push(SideBySideLine {
                    lineno: line.new_lineno,
                    content: Some(line.content.clone()),
                    kind: DiffLineKind::Added,
                });
                i += 1;
            }
        }
    }

    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_diff() {
        let hunks = parse_diff("");
        assert!(hunks.is_empty());
    }

    #[test]
    fn test_parse_single_hunk_add_only() {
        let raw = "\
diff --git a/new.txt b/new.txt
new file mode 100644
index 0000000..f00c965
--- /dev/null
+++ b/new.txt
@@ -0,0 +1,3 @@
+line one
+line two
+line three
";
        let hunks = parse_diff(raw);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 0);
        assert_eq!(hunks[0].old_count, 0);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].new_count, 3);
        assert_eq!(hunks[0].lines.len(), 3);
        for (i, line) in hunks[0].lines.iter().enumerate() {
            assert_eq!(line.kind, DiffLineKind::Added);
            assert_eq!(line.old_lineno, None);
            assert_eq!(line.new_lineno, Some(i + 1));
        }
        assert_eq!(hunks[0].lines[0].content, "line one");
        assert_eq!(hunks[0].lines[2].content, "line three");
    }

    #[test]
    fn test_parse_single_hunk_remove_only() {
        let raw = "\
diff --git a/old.txt b/old.txt
deleted file mode 100644
index f00c965..0000000
--- a/old.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-gone one
-gone two
";
        let hunks = parse_diff(raw);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].old_count, 2);
        assert_eq!(hunks[0].new_start, 0);
        assert_eq!(hunks[0].new_count, 0);
        assert_eq!(hunks[0].lines.len(), 2);
        for line in &hunks[0].lines {
            assert_eq!(line.kind, DiffLineKind::Removed);
            assert!(line.new_lineno.is_none());
        }
        assert_eq!(hunks[0].lines[0].old_lineno, Some(1));
        assert_eq!(hunks[0].lines[1].old_lineno, Some(2));
    }

    #[test]
    fn test_parse_single_hunk_mixed() {
        let raw = "\
diff --git a/file.rs b/file.rs
index abc1234..def5678 100644
--- a/file.rs
+++ b/file.rs
@@ -2,4 +2,4 @@
 context before
-old line
+new line
 context after
";
        let hunks = parse_diff(raw);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 2);
        assert_eq!(hunks[0].new_start, 2);
        assert_eq!(hunks[0].lines.len(), 4);

        assert_eq!(hunks[0].lines[0].kind, DiffLineKind::Context);
        assert_eq!(hunks[0].lines[0].old_lineno, Some(2));
        assert_eq!(hunks[0].lines[0].new_lineno, Some(2));
        assert_eq!(hunks[0].lines[0].content, "context before");

        assert_eq!(hunks[0].lines[1].kind, DiffLineKind::Removed);
        assert_eq!(hunks[0].lines[1].old_lineno, Some(3));
        assert_eq!(hunks[0].lines[1].new_lineno, None);

        assert_eq!(hunks[0].lines[2].kind, DiffLineKind::Added);
        assert_eq!(hunks[0].lines[2].old_lineno, None);
        assert_eq!(hunks[0].lines[2].new_lineno, Some(3));

        assert_eq!(hunks[0].lines[3].kind, DiffLineKind::Context);
        assert_eq!(hunks[0].lines[3].old_lineno, Some(4));
        assert_eq!(hunks[0].lines[3].new_lineno, Some(4));
    }

    #[test]
    fn test_parse_multiple_hunks() {
        let raw = "\
diff --git a/file.rs b/file.rs
index abc..def 100644
--- a/file.rs
+++ b/file.rs
@@ -1,3 +1,3 @@
 line1
-old2
+new2
 line3
@@ -10,3 +10,4 @@
 line10
+inserted
 line11
 line12
";
        let hunks = parse_diff(raw);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[1].old_start, 10);
        assert_eq!(hunks[1].new_count, 4);
    }

    #[test]
    fn test_parse_no_newline_at_eof() {
        let raw = "\
diff --git a/f.txt b/f.txt
index abc..def 100644
--- a/f.txt
+++ b/f.txt
@@ -1,2 +1,2 @@
 line1
-old
\\ No newline at end of file
+new
\\ No newline at end of file
";
        let hunks = parse_diff(raw);
        assert_eq!(hunks.len(), 1);
        // "\ No newline" markers should be skipped
        assert_eq!(hunks[0].lines.len(), 3);
    }

    #[test]
    fn test_build_full_no_hunks() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline2\nline3";
        let lines = build_full_diff_lines(old, new, &[]);
        assert_eq!(lines.len(), 3);
        for line in &lines {
            assert_eq!(line.kind, DiffLineKind::Context);
        }
        assert_eq!(lines[0].old_lineno, Some(1));
        assert_eq!(lines[0].new_lineno, Some(1));
        assert_eq!(lines[2].old_lineno, Some(3));
        assert_eq!(lines[2].new_lineno, Some(3));
    }

    #[test]
    fn test_build_full_hunk_at_start() {
        let old = "old1\nline2\nline3";
        let new = "new1\nline2\nline3";
        let hunks = vec![DiffHunk {
            old_start: 1, old_count: 1, new_start: 1, new_count: 1,
            lines: vec![
                DiffLine { kind: DiffLineKind::Removed, old_lineno: Some(1), new_lineno: None, content: "old1".into() },
                DiffLine { kind: DiffLineKind::Added, old_lineno: None, new_lineno: Some(1), content: "new1".into() },
            ],
        }];
        let lines = build_full_diff_lines(old, new, &hunks);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].kind, DiffLineKind::Removed);
        assert_eq!(lines[1].kind, DiffLineKind::Added);
        assert_eq!(lines[2].kind, DiffLineKind::Context);
        assert_eq!(lines[2].content, "line2");
        assert_eq!(lines[3].kind, DiffLineKind::Context);
        assert_eq!(lines[3].content, "line3");
    }

    #[test]
    fn test_build_full_hunk_at_end() {
        let old = "line1\nline2\nold3";
        let new = "line1\nline2\nnew3";
        let hunks = vec![DiffHunk {
            old_start: 3, old_count: 1, new_start: 3, new_count: 1,
            lines: vec![
                DiffLine { kind: DiffLineKind::Removed, old_lineno: Some(3), new_lineno: None, content: "old3".into() },
                DiffLine { kind: DiffLineKind::Added, old_lineno: None, new_lineno: Some(3), content: "new3".into() },
            ],
        }];
        let lines = build_full_diff_lines(old, new, &hunks);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].kind, DiffLineKind::Context);
        assert_eq!(lines[0].content, "line1");
        assert_eq!(lines[1].kind, DiffLineKind::Context);
        assert_eq!(lines[1].content, "line2");
        assert_eq!(lines[2].kind, DiffLineKind::Removed);
        assert_eq!(lines[3].kind, DiffLineKind::Added);
    }

    #[test]
    fn test_build_full_gap_between_hunks() {
        let old = "a\nb\nc\nd\ne";
        let new = "A\nb\nc\nd\nE";
        let hunks = vec![
            DiffHunk {
                old_start: 1, old_count: 1, new_start: 1, new_count: 1,
                lines: vec![
                    DiffLine { kind: DiffLineKind::Removed, old_lineno: Some(1), new_lineno: None, content: "a".into() },
                    DiffLine { kind: DiffLineKind::Added, old_lineno: None, new_lineno: Some(1), content: "A".into() },
                ],
            },
            DiffHunk {
                old_start: 5, old_count: 1, new_start: 5, new_count: 1,
                lines: vec![
                    DiffLine { kind: DiffLineKind::Removed, old_lineno: Some(5), new_lineno: None, content: "e".into() },
                    DiffLine { kind: DiffLineKind::Added, old_lineno: None, new_lineno: Some(5), content: "E".into() },
                ],
            },
        ];
        let lines = build_full_diff_lines(old, new, &hunks);
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[2].kind, DiffLineKind::Context);
        assert_eq!(lines[2].content, "b");
        assert_eq!(lines[2].old_lineno, Some(2));
        assert_eq!(lines[2].new_lineno, Some(2));
    }

    #[test]
    fn test_build_full_new_file() {
        let hunks = vec![DiffHunk {
            old_start: 0, old_count: 0, new_start: 1, new_count: 2,
            lines: vec![
                DiffLine { kind: DiffLineKind::Added, old_lineno: None, new_lineno: Some(1), content: "hello".into() },
                DiffLine { kind: DiffLineKind::Added, old_lineno: None, new_lineno: Some(2), content: "world".into() },
            ],
        }];
        let lines = build_full_diff_lines("", "hello\nworld", &hunks);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.kind == DiffLineKind::Added));
    }

    #[test]
    fn test_build_full_deleted_file() {
        let hunks = vec![DiffHunk {
            old_start: 1, old_count: 2, new_start: 0, new_count: 0,
            lines: vec![
                DiffLine { kind: DiffLineKind::Removed, old_lineno: Some(1), new_lineno: None, content: "bye".into() },
                DiffLine { kind: DiffLineKind::Removed, old_lineno: Some(2), new_lineno: None, content: "gone".into() },
            ],
        }];
        let lines = build_full_diff_lines("bye\ngone", "", &hunks);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.kind == DiffLineKind::Removed));
    }

    #[test]
    fn test_side_by_side_no_changes() {
        let full = vec![
            DiffLine { kind: DiffLineKind::Context, old_lineno: Some(1), new_lineno: Some(1), content: "a".into() },
            DiffLine { kind: DiffLineKind::Context, old_lineno: Some(2), new_lineno: Some(2), content: "b".into() },
        ];
        let (left, right) = build_side_by_side_lines(&full);
        assert_eq!(left.len(), 2);
        assert_eq!(right.len(), 2);
        assert_eq!(left[0].content.as_deref(), Some("a"));
        assert_eq!(right[0].content.as_deref(), Some("a"));
    }

    #[test]
    fn test_side_by_side_remove_then_add() {
        let full = vec![
            DiffLine { kind: DiffLineKind::Removed, old_lineno: Some(1), new_lineno: None, content: "old".into() },
            DiffLine { kind: DiffLineKind::Added, old_lineno: None, new_lineno: Some(1), content: "new".into() },
        ];
        let (left, right) = build_side_by_side_lines(&full);
        assert_eq!(left.len(), right.len());
        assert_eq!(left[0].content.as_deref(), Some("old"));
        assert_eq!(left[0].kind, DiffLineKind::Removed);
        assert_eq!(right[0].content.as_deref(), Some("new"));
        assert_eq!(right[0].kind, DiffLineKind::Added);
    }

    #[test]
    fn test_side_by_side_pure_add() {
        let full = vec![
            DiffLine { kind: DiffLineKind::Context, old_lineno: Some(1), new_lineno: Some(1), content: "a".into() },
            DiffLine { kind: DiffLineKind::Added, old_lineno: None, new_lineno: Some(2), content: "inserted".into() },
            DiffLine { kind: DiffLineKind::Context, old_lineno: Some(2), new_lineno: Some(3), content: "b".into() },
        ];
        let (left, right) = build_side_by_side_lines(&full);
        assert_eq!(left.len(), right.len());
        assert_eq!(left.len(), 3);
        assert_eq!(left[0].content.as_deref(), Some("a"));
        assert_eq!(right[0].content.as_deref(), Some("a"));
        assert_eq!(left[1].content, None);
        assert_eq!(right[1].content.as_deref(), Some("inserted"));
        assert_eq!(left[2].content.as_deref(), Some("b"));
        assert_eq!(right[2].content.as_deref(), Some("b"));
    }

    #[test]
    fn test_side_by_side_pure_remove() {
        let full = vec![
            DiffLine { kind: DiffLineKind::Removed, old_lineno: Some(1), new_lineno: None, content: "deleted".into() },
            DiffLine { kind: DiffLineKind::Context, old_lineno: Some(2), new_lineno: Some(1), content: "kept".into() },
        ];
        let (left, right) = build_side_by_side_lines(&full);
        assert_eq!(left.len(), right.len());
        assert_eq!(left[0].content.as_deref(), Some("deleted"));
        assert_eq!(right[0].content, None);
        assert_eq!(left[1].content.as_deref(), Some("kept"));
        assert_eq!(right[1].content.as_deref(), Some("kept"));
    }

    #[test]
    fn test_diff_view_mode_serde_roundtrip() {
        let inline = DiffViewMode::Inline;
        let json = serde_json::to_string(&inline).unwrap();
        let back: DiffViewMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, DiffViewMode::Inline);

        let sbs = DiffViewMode::SideBySide;
        let json = serde_json::to_string(&sbs).unwrap();
        let back: DiffViewMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, DiffViewMode::SideBySide);
    }

    #[test]
    fn test_diff_view_mode_default_inline() {
        assert_eq!(DiffViewMode::Inline, DiffViewMode::Inline);
        assert_ne!(DiffViewMode::Inline, DiffViewMode::SideBySide);
    }
}
