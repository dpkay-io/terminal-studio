//! Where the git panel's file paths are rooted. `git status --porcelain`
//! names every file relative to the repository's top folder, whatever
//! folder it runs in, while a terminal may be anywhere inside the
//! repository. So a status path is joined to the repository root — never
//! to the terminal's folder — and every git command that takes a path runs
//! there, where git reads its paths from the same place.

use std::path::{Path, PathBuf};

/// The separator git uses in paths (`HEAD:<path>`), on every platform.
const GIT_PATH_SEPARATOR: &str = "/";

/// The folder a status path from `cwd` is relative to: the repository root
/// when `cwd` is inside one (`git_root`, as the watcher found it), else
/// `cwd` itself. Pure.
pub(super) fn status_base(cwd: &Path, git_root: Option<&Path>) -> PathBuf {
    git_root.unwrap_or(cwd).to_path_buf()
}

/// The repository root above `dir`, or `dir` when it is not in one. Reads
/// the file system: worker threads only.
pub(super) fn repo_root_or_self(dir: &Path) -> PathBuf {
    crate::util::find_git_root(dir).unwrap_or_else(|| dir.to_path_buf())
}

/// `file` as git names it from `root`: relative, `/`-separated. `None` when
/// `file` is not under `root`. Pure.
pub(super) fn git_relative(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join(GIT_PATH_SEPARATOR))
}

/// Where `file` (absolute) lives for git: its repository root (the folder
/// above it when it is in none) and its path from there. Reads the file
/// system: worker threads only.
pub(super) fn locate(file: &Path) -> Option<(PathBuf, String)> {
    let dir = file.parent()?;
    let root = repo_root_or_self(dir);
    let rel = git_relative(&root, file)?;
    Some((root, rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_paths_are_rooted_at_the_repository() {
        let cwd = Path::new("/repo/src/app");
        assert_eq!(
            status_base(cwd, Some(Path::new("/repo"))),
            PathBuf::from("/repo")
        );
        assert_eq!(status_base(cwd, None), cwd, "no repository: the folder");
    }

    #[test]
    fn a_file_is_named_from_its_root_with_forward_slashes() {
        let root = Path::new("/repo");
        assert_eq!(
            git_relative(root, &root.join("src").join("main.rs")).as_deref(),
            Some("src/main.rs")
        );
        assert_eq!(git_relative(root, Path::new("/elsewhere/a.rs")), None);
        assert_eq!(git_relative(root, root), None, "the root is not a file");
    }

    #[test]
    fn a_file_in_a_subfolder_is_found_from_the_repository_root() {
        let tmp = std::env::temp_dir().join(format!("git_paths_locate_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let sub = tmp.join("src").join("app");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        let file = sub.join("x.rs");
        assert_eq!(
            locate(&file),
            Some((tmp.clone(), "src/app/x.rs".to_string()))
        );
        assert_eq!(repo_root_or_self(&sub), tmp);
        let _ = std::fs::remove_dir_all(&tmp);

        let outside = std::env::temp_dir().join(format!("git_paths_none_{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        // Not in a repository (unless the temp folder itself is in one).
        if crate::util::find_git_root(&outside).is_none() {
            assert_eq!(
                locate(&outside.join("y.txt")),
                Some((outside.clone(), "y.txt".to_string()))
            );
        }
        let _ = std::fs::remove_dir_all(&outside);
    }
}
