//! Pure parsing of `git status --porcelain` output, with no UI dependencies.

/// The kind of change a file has undergone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

/// A single entry from `git status --porcelain` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileStatus {
    pub kind: FileChangeKind,
    pub path: String,
    /// For renamed files, the original (source) path before the rename.
    pub original_path: Option<String>,
    pub staged: bool,
}

/// Unquote a git path that may be C-style quoted.
///
/// Git's `--porcelain` output quotes filenames containing non-ASCII or special
/// characters using C-style escaping: `"\303\251file"` for `éfile`.
/// This function strips the outer quotes, processes escape sequences
/// (`\\`, `\"`, `\n`, `\t`, `\NNN` octal), and interprets the result as UTF-8.
fn unquote_git_path(s: &str) -> String {
    if !(s.starts_with('"') && s.ends_with('"') && s.len() >= 2) {
        return s.to_string();
    }
    let inner = &s[1..s.len() - 1];
    let mut bytes: Vec<u8> = Vec::with_capacity(inner.len());
    let mut chars = inner.bytes().peekable();
    while let Some(b) = chars.next() {
        if b == b'\\' {
            match chars.next() {
                Some(b'\\') => bytes.push(b'\\'),
                Some(b'"') => bytes.push(b'"'),
                Some(b'n') => bytes.push(b'\n'),
                Some(b't') => bytes.push(b'\t'),
                Some(d @ b'0'..=b'7') => {
                    // Octal escape: up to 3 digits
                    let mut val: u8 = d - b'0';
                    for _ in 0..2 {
                        if let Some(&next) = chars.peek() {
                            if (b'0'..=b'7').contains(&next) {
                                val = val * 8 + (next - b'0');
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    }
                    bytes.push(val);
                }
                Some(other) => {
                    bytes.push(b'\\');
                    bytes.push(other);
                }
                None => bytes.push(b'\\'),
            }
        } else {
            bytes.push(b);
        }
    }
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// For renamed files, extract `(new_path, Some(old_path))` from `"old -> new"`.
/// For non-renames, returns `(path, None)`.
fn extract_rename_paths(raw: &str) -> (String, Option<String>) {
    if let Some(idx) = raw.find(" -> ") {
        let old = unquote_git_path(raw[..idx].trim());
        let new = unquote_git_path(raw[idx + 4..].trim());
        (new, Some(old))
    } else {
        (unquote_git_path(raw), None)
    }
}

/// Parse the full output of `git status --porcelain` into a list of
/// [`GitFileStatus`] entries.
///
/// Each porcelain line has the format `XY path`, where X is the index
/// (staged) status and Y is the working-tree (unstaged) status.  A single
/// file can appear as both staged *and* unstaged if it has changes in both
/// areas.
pub fn parse_git_status(output: &str) -> Vec<GitFileStatus> {
    let mut entries = Vec::new();

    for line in output.lines() {
        if line.len() < 3 {
            continue;
        }
        let x = line.as_bytes()[0];
        let y = line.as_bytes()[1];
        let Some(raw_path) = line.get(3..) else {
            continue;
        };

        // Staged (index) changes -- first column
        match x {
            b'M' => {
                let path = unquote_git_path(raw_path);
                entries.push(GitFileStatus {
                    kind: FileChangeKind::Modified,
                    path,
                    original_path: None,
                    staged: true,
                });
            }
            b'A' => {
                let path = unquote_git_path(raw_path);
                entries.push(GitFileStatus {
                    kind: FileChangeKind::Added,
                    path,
                    original_path: None,
                    staged: true,
                });
            }
            b'D' => {
                let path = unquote_git_path(raw_path);
                entries.push(GitFileStatus {
                    kind: FileChangeKind::Deleted,
                    path,
                    original_path: None,
                    staged: true,
                });
            }
            b'R' => {
                let (new_path, old_path) = extract_rename_paths(raw_path);
                entries.push(GitFileStatus {
                    kind: FileChangeKind::Renamed,
                    path: new_path,
                    original_path: old_path,
                    staged: true,
                });
            }
            _ => {}
        }

        // Unstaged (working-tree) changes -- second column
        if x == b'?' && y == b'?' {
            let path = unquote_git_path(raw_path);
            entries.push(GitFileStatus {
                kind: FileChangeKind::Untracked,
                path,
                original_path: None,
                staged: false,
            });
        } else {
            match y {
                b'M' => {
                    let path = unquote_git_path(raw_path);
                    entries.push(GitFileStatus {
                        kind: FileChangeKind::Modified,
                        path,
                        original_path: None,
                        staged: false,
                    });
                }
                b'D' => {
                    let path = unquote_git_path(raw_path);
                    entries.push(GitFileStatus {
                        kind: FileChangeKind::Deleted,
                        path,
                        original_path: None,
                        staged: false,
                    });
                }
                _ => {}
            }
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_output() {
        let entries = parse_git_status("");
        assert!(entries.is_empty());
    }

    #[test]
    fn staged_modified_file() {
        let entries = parse_git_status("M  src/app.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Modified);
        assert_eq!(entries[0].path, "src/app.rs");
        assert!(entries[0].staged);
    }

    #[test]
    fn staged_added_file() {
        let entries = parse_git_status("A  src/new.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Added);
        assert_eq!(entries[0].path, "src/new.rs");
        assert!(entries[0].staged);
    }

    #[test]
    fn staged_deleted_file() {
        let entries = parse_git_status("D  src/old.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Deleted);
        assert_eq!(entries[0].path, "src/old.rs");
        assert!(entries[0].staged);
    }

    #[test]
    fn staged_renamed_file() {
        let entries = parse_git_status("R  old.rs -> new.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Renamed);
        assert_eq!(entries[0].path, "new.rs");
        assert_eq!(entries[0].original_path, Some("old.rs".to_string()));
        assert!(entries[0].staged);
    }

    #[test]
    fn unstaged_modified_file() {
        let entries = parse_git_status(" M src/app.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Modified);
        assert_eq!(entries[0].path, "src/app.rs");
        assert!(!entries[0].staged);
    }

    #[test]
    fn unstaged_deleted_file() {
        let entries = parse_git_status(" D src/gone.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Deleted);
        assert_eq!(entries[0].path, "src/gone.rs");
        assert!(!entries[0].staged);
    }

    #[test]
    fn untracked_file() {
        let entries = parse_git_status("?? newfile.txt\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, FileChangeKind::Untracked);
        assert_eq!(entries[0].path, "newfile.txt");
        assert!(!entries[0].staged);
    }

    #[test]
    fn both_staged_and_unstaged() {
        // File modified in index AND working tree
        let entries = parse_git_status("MM src/app.rs\n");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].kind, FileChangeKind::Modified);
        assert!(entries[0].staged);
        assert_eq!(entries[1].kind, FileChangeKind::Modified);
        assert!(!entries[1].staged);
    }

    #[test]
    fn multiple_files() {
        let input = "M  src/app.rs\n?? newfile.txt\n D src/gone.rs\nA  src/added.rs\n";
        let entries = parse_git_status(input);
        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0].kind, FileChangeKind::Modified);
        assert!(entries[0].staged);
        assert_eq!(entries[0].path, "src/app.rs");

        assert_eq!(entries[1].kind, FileChangeKind::Untracked);
        assert!(!entries[1].staged);
        assert_eq!(entries[1].path, "newfile.txt");

        assert_eq!(entries[2].kind, FileChangeKind::Deleted);
        assert!(!entries[2].staged);
        assert_eq!(entries[2].path, "src/gone.rs");

        assert_eq!(entries[3].kind, FileChangeKind::Added);
        assert!(entries[3].staged);
        assert_eq!(entries[3].path, "src/added.rs");
    }

    #[test]
    fn short_lines_are_skipped() {
        // Lines shorter than 3 chars should be ignored
        let entries = parse_git_status("M\n??\nM  ok.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "ok.rs");
    }

    #[test]
    fn unquote_plain_path() {
        assert_eq!(unquote_git_path("src/app.rs"), "src/app.rs");
    }

    #[test]
    fn unquote_c_style_octal_utf8() {
        // "\303\251file" is the C-style quoted form of "éfile" (UTF-8: 0xC3 0xA9)
        assert_eq!(unquote_git_path(r#""\303\251file""#), "\u{00e9}file");
    }

    #[test]
    fn unquote_backslash_escapes() {
        assert_eq!(unquote_git_path(r#""a\\b""#), "a\\b");
        assert_eq!(unquote_git_path(r#""a\"b""#), "a\"b");
        assert_eq!(unquote_git_path(r#""a\nb""#), "a\nb");
        assert_eq!(unquote_git_path(r#""a\tb""#), "a\tb");
    }

    #[test]
    fn quoted_path_in_status() {
        // Git outputs: ?? "\303\251file"  (C-style quoted octal)
        let input = "?? \"\\303\\251file\"\n";
        let entries = parse_git_status(input);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "\u{00e9}file");
    }

    #[test]
    fn renamed_file_path_is_destination() {
        let entries = parse_git_status("R  src/old.rs -> src/new.rs\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "src/new.rs");
        assert_eq!(entries[0].original_path, Some("src/old.rs".to_string()));
    }

    #[test]
    fn renamed_file_with_quoted_paths() {
        // Both old and new names contain non-ASCII (C-style quoted)
        let input = "R  \"\\303\\251old\" -> \"\\303\\251new\"\n";
        let entries = parse_git_status(input);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "\u{00e9}new");
        assert_eq!(
            entries[0].original_path,
            Some("\u{00e9}old".to_string())
        );
    }
}
