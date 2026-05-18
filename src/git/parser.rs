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
    pub staged: bool,
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
        let path = line[3..].trim().to_string();

        // Staged (index) changes — first column
        match x {
            b'M' => entries.push(GitFileStatus {
                kind: FileChangeKind::Modified,
                path: path.clone(),
                staged: true,
            }),
            b'A' => entries.push(GitFileStatus {
                kind: FileChangeKind::Added,
                path: path.clone(),
                staged: true,
            }),
            b'D' => entries.push(GitFileStatus {
                kind: FileChangeKind::Deleted,
                path: path.clone(),
                staged: true,
            }),
            b'R' => entries.push(GitFileStatus {
                kind: FileChangeKind::Renamed,
                path: path.clone(),
                staged: true,
            }),
            _ => {}
        }

        // Unstaged (working-tree) changes — second column
        if x == b'?' && y == b'?' {
            entries.push(GitFileStatus {
                kind: FileChangeKind::Untracked,
                path,
                staged: false,
            });
        } else {
            match y {
                b'M' => entries.push(GitFileStatus {
                    kind: FileChangeKind::Modified,
                    path,
                    staged: false,
                }),
                b'D' => entries.push(GitFileStatus {
                    kind: FileChangeKind::Deleted,
                    path,
                    staged: false,
                }),
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
        assert_eq!(entries[0].path, "old.rs -> new.rs");
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
}
