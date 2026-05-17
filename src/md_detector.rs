use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

static MD_PATH_REGEX: OnceLock<Regex> = OnceLock::new();

fn md_path_regex() -> &'static Regex {
    MD_PATH_REGEX.get_or_init(|| {
        Regex::new(r"(?:(?:[A-Za-z]:|\.\.?)[\\/])?(?:[\w.\-]+[\\/])*[\w.\-]+\.md\b").unwrap()
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
            } else {
                cwd.join(candidate)
            };
            if resolved.is_file() {
                paths.push(DetectedMdPath {
                    line: *line_idx,
                    start_col: m.start(),
                    end_col: m.end(),
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
