use std::path::PathBuf;

/// Write `content` to `path` atomically: writes to a sibling temp file, then
/// renames over the target. On failure the target file is left untouched (or
/// absent). This prevents data corruption if the app crashes mid-write.
pub fn atomic_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;

    let tmp = parent.join(format!(
        ".{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("data")
    ));

    std::fs::write(&tmp, content)?;

    // On Windows, rename fails if the target exists — remove it first.
    #[cfg(target_os = "windows")]
    {
        let _ = std::fs::remove_file(path);
    }

    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Returns the platform-appropriate config directory for Terminal Studio.
///
/// - Windows: `%APPDATA%\terminal-studio\`
/// - Linux/macOS: `$XDG_CONFIG_HOME/terminal-studio/` (falls back to `$HOME/.config/terminal-studio/`)
pub fn data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .ok()
            .map(|base| PathBuf::from(base).join("terminal-studio"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg).join("terminal-studio"));
            }
        }
        std::env::var("HOME")
            .ok()
            .map(|base| PathBuf::from(base).join(".config").join("terminal-studio"))
    }
}

/// Convenience: returns `data_dir().join(filename)`.
pub fn data_file(filename: &str) -> Option<PathBuf> {
    data_dir().map(|d| d.join(filename))
}

/// Case-aware path prefix check. On Windows, uses case-insensitive comparison.
pub fn path_starts_with(child: &std::path::Path, parent: &std::path::Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        let child_lower = child.to_string_lossy().to_lowercase();
        let parent_lower = parent.to_string_lossy().to_lowercase();
        let child_path = std::path::Path::new(&child_lower);
        let parent_path = std::path::Path::new(&parent_lower);
        child_path.starts_with(parent_path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        child.starts_with(parent)
    }
}

/// Case-aware path equality. On Windows, uses case-insensitive comparison.
pub fn paths_equal(a: &std::path::Path, b: &std::path::Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
    #[cfg(not(target_os = "windows"))]
    {
        a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_data_dir_returns_some() {
        // Should return Some on any system with HOME or APPDATA set
        if cfg!(target_os = "windows") {
            if std::env::var("APPDATA").is_ok() {
                assert!(data_dir().is_some());
            }
        } else if std::env::var("HOME").is_ok() {
            assert!(data_dir().is_some());
        }
    }

    #[test]
    fn test_data_file() {
        if let Some(dir) = data_dir() {
            let f = data_file("test.json").unwrap();
            assert_eq!(f, dir.join("test.json"));
        }
    }

    #[test]
    fn test_paths_equal_same() {
        let p = Path::new("/home/user/project");
        assert!(paths_equal(p, p));
    }

    #[test]
    fn test_path_starts_with_basic() {
        let child = Path::new("/home/user/project/src");
        let parent = Path::new("/home/user/project");
        assert!(path_starts_with(child, parent));
        assert!(!path_starts_with(parent, child));
    }

    #[test]
    fn test_atomic_write_creates_file() {
        let dir = std::env::temp_dir().join("ts_test_atomic");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("test_atomic.json");
        let _ = std::fs::remove_file(&path);

        atomic_write(&path, "hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");

        // Overwrite
        atomic_write(&path, "updated").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "updated");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_paths_equal_case_insensitive() {
        let a = Path::new("C:\\Users\\Dpk\\Project");
        let b = Path::new("c:\\users\\dpk\\project");
        assert!(paths_equal(a, b));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_path_starts_with_case_insensitive() {
        let child = Path::new("C:\\Users\\Dpk\\Project\\src");
        let parent = Path::new("c:\\users\\dpk\\project");
        assert!(path_starts_with(child, parent));
    }
}
