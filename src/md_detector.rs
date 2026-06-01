use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

static MD_PATH_REGEX: OnceLock<Regex> = OnceLock::new();

fn md_path_regex() -> &'static Regex {
    MD_PATH_REGEX.get_or_init(|| {
        Regex::new(r"(?:(?:[A-Za-z]:|\.\.?)?[\\/])?(?:[\w.~\-]+[\\/])*[\w.~\-]+\.md\b").unwrap()
    })
}

pub struct DetectedMdPath {
    pub line: i32,
    pub start_col: usize,
    pub end_col: usize,
    pub path: PathBuf,
}

pub fn detect_md_paths(lines: &[(i32, String)], cwd: &Path) -> Vec<DetectedMdPath> {
    let re = md_path_regex();
    let mut paths = Vec::new();
    for (line_idx, text) in lines {
        for m in re.find_iter(text) {
            if m.start() >= 3 && text[..m.start()].ends_with("://") {
                continue;
            }
            let raw = m.as_str();
            let candidate = Path::new(raw);
            let resolved = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else if !cwd.as_os_str().is_empty() {
                cwd.join(candidate)
            } else {
                continue;
            };
            if resolved.is_file() {
                paths.push(DetectedMdPath {
                    line: *line_idx,
                    start_col: text[..m.start()].chars().count(),
                    end_col: text[..m.end()].chars().count(),
                    path: resolved,
                });
            }
        }
    }
    paths
}

pub fn md_at_position(paths: &[DetectedMdPath], line: i32, col: usize) -> Option<&Path> {
    paths
        .iter()
        .find(|p| p.line == line && col >= p.start_col && col < p.end_col)
        .map(|p| p.path.as_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn test_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("md_detect_test_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn detect_absolute_path_with_empty_cwd() {
        let dir = test_dir("abs_empty_cwd");
        let md_file = dir.join("test.md");
        std::fs::File::create(&md_file)
            .unwrap()
            .write_all(b"# hello")
            .unwrap();

        let abs = md_file.to_string_lossy().to_string();
        let lines = vec![(0, format!("Created {abs}"))];
        let results = detect_md_paths(&lines, Path::new(""));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, md_file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn relative_path_skipped_with_empty_cwd() {
        let lines = vec![(0, "memory/plan.md".to_string())];
        let results = detect_md_paths(&lines, Path::new(""));
        assert!(results.is_empty());
    }

    #[test]
    fn relative_path_resolved_with_cwd() {
        let dir = test_dir("rel_with_cwd");
        let sub = dir.join("docs");
        std::fs::create_dir(&sub).unwrap();
        let md_file = sub.join("plan.md");
        std::fs::File::create(&md_file)
            .unwrap()
            .write_all(b"# plan")
            .unwrap();

        let lines = vec![(0, "docs/plan.md".to_string())];
        let results = detect_md_paths(&lines, &dir);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, md_file);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn url_with_md_extension_is_skipped() {
        let lines = vec![(0, "https://example.com/readme.md".to_string())];
        let results = detect_md_paths(&lines, Path::new(""));
        assert!(results.is_empty());
    }

    #[test]
    fn md_at_position_finds_correct_path() {
        let p = PathBuf::from("C:\\tmp\\test.md");
        let paths = vec![DetectedMdPath {
            line: 0,
            start_col: 5,
            end_col: 15,
            path: p.clone(),
        }];
        assert_eq!(md_at_position(&paths, 0, 5), Some(p.as_path()));
        assert_eq!(md_at_position(&paths, 0, 14), Some(p.as_path()));
        assert_eq!(md_at_position(&paths, 0, 4), None);
        assert_eq!(md_at_position(&paths, 0, 15), None);
        assert_eq!(md_at_position(&paths, 1, 5), None);
    }

    #[test]
    fn multiple_md_paths_on_same_line() {
        let dir = test_dir("multi_same_line");
        let a = dir.join("a.md");
        let b = dir.join("b.md");
        std::fs::File::create(&a).unwrap();
        std::fs::File::create(&b).unwrap();

        let lines = vec![(0, "a.md and b.md".to_string())];
        let results = detect_md_paths(&lines, &dir);
        assert_eq!(results.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nonexistent_file_not_detected() {
        let dir = test_dir("nonexistent");
        let lines = vec![(0, "ghost.md".to_string())];
        let results = detect_md_paths(&lines, &dir);
        assert!(results.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
