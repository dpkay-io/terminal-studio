use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Deserialize JSON from a file. On parse failure, creates a `.bak` backup
/// of the corrupt file and returns `None` so callers fall back to defaults
/// without silently destroying the original data on next save.
pub fn safe_json_load<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&text) {
        Ok(val) => Some(val),
        Err(e) => {
            let bak = path.with_extension("json.bak");
            log::warn!(
                "corrupt JSON in {}: {e} — backing up to {}",
                path.display(),
                bak.display()
            );
            let _ = std::fs::copy(path, &bak);
            None
        }
    }
}

/// Write `content` to `path` atomically: writes to a sibling temp file, then
/// renames over the target. On failure the target file is left untouched (or
/// absent). This prevents data corruption if the app crashes mid-write.
pub fn atomic_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    std::fs::create_dir_all(parent)?;

    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("data"),
        std::process::id(),
        seq,
    ));

    std::fs::write(&tmp, content)?;

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide = |p: &std::path::Path| -> Vec<u16> {
            p.as_os_str()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        };
        let src = wide(&tmp);
        let dst = wide(path);
        let ret = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(
                src.as_ptr(),
                dst.as_ptr(),
                windows_sys::Win32::Storage::FileSystem::MOVEFILE_REPLACE_EXISTING
                    | windows_sys::Win32::Storage::FileSystem::MOVEFILE_WRITE_THROUGH,
            )
        };
        if ret == 0 {
            let e = std::io::Error::last_os_error();
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Err(e) = std::fs::rename(&tmp, path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(())
    }
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

/// Walk up from `path` looking for a `.git` directory. Returns the directory
/// containing `.git` (the repository root), or `None` if not inside a git repo.
pub fn find_git_root(path: &Path) -> Option<PathBuf> {
    let mut current = path.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Open the system file manager showing the parent folder of `path`, with the
/// file/folder selected if the platform supports it.
pub fn reveal_in_file_manager(path: &Path) {
    let path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => path.to_path_buf(),
    };

    std::thread::spawn(move || {
        let _result = reveal_platform(&path);
    });
}

#[cfg(target_os = "windows")]
fn reveal_platform(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("explorer")
        .arg("/select,")
        .arg(path)
        .status()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn reveal_platform(path: &Path) -> std::io::Result<()> {
    std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .status()
        .map(|_| ())
}

#[cfg(target_os = "linux")]
fn reveal_platform(path: &Path) -> std::io::Result<()> {
    let uri = format!("file://{}", path.display());
    let dbus = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:{uri}"))
        .arg("string:")
        .status();
    match dbus {
        Ok(_) => Ok(()),
        Err(_) => {
            let dir = if path.is_dir() {
                path
            } else {
                path.parent().unwrap_or(path)
            };
            std::process::Command::new("xdg-open")
                .arg(dir)
                .status()
                .map(|_| ())
        }
    }
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

    #[test]
    fn test_find_git_root_at_root() {
        let dir = std::env::temp_dir().join("ts_test_git_root_at_root");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        assert_eq!(find_git_root(&dir), Some(dir.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_find_git_root_in_subdirectory() {
        let dir = std::env::temp_dir().join("ts_test_git_root_subdir");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let sub = dir.join("src").join("components");
        std::fs::create_dir_all(&sub).unwrap();
        assert_eq!(find_git_root(&sub), Some(dir.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_find_git_root_none() {
        let dir = std::env::temp_dir().join("ts_test_git_root_none");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(find_git_root(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_find_git_root_nested_repos() {
        let outer = std::env::temp_dir().join("ts_test_git_root_nested");
        let _ = std::fs::remove_dir_all(&outer);
        std::fs::create_dir_all(outer.join(".git")).unwrap();
        let inner = outer.join("vendor").join("dep");
        std::fs::create_dir_all(inner.join(".git")).unwrap();
        let sub = inner.join("src");
        std::fs::create_dir_all(&sub).unwrap();
        // Should find the innermost repo
        assert_eq!(find_git_root(&sub), Some(inner.clone()));
        // From inner root itself
        assert_eq!(find_git_root(&inner), Some(inner.clone()));
        let _ = std::fs::remove_dir_all(&outer);
    }
}
