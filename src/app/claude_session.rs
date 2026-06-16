use std::path::{Path, PathBuf};

/// Returns true when running inside WSL (Windows Subsystem for Linux).
/// Result is cached after the first call.
#[cfg(target_os = "linux")]
fn is_wsl() -> bool {
    use std::sync::OnceLock;
    static IS_WSL: OnceLock<bool> = OnceLock::new();
    *IS_WSL.get_or_init(|| {
        std::fs::read_to_string("/proc/version")
            .map(|v| {
                let lower = v.to_lowercase();
                lower.contains("microsoft") || lower.contains("wsl")
            })
            .unwrap_or(false)
    })
}

/// On WSL, finds the Windows user home directory (e.g. `/mnt/c/Users/dpk`)
/// by scanning `/mnt/c/Users/` for a directory containing `.claude/`.
/// Prefers matching the current Linux username; falls back to the first other candidate.
#[cfg(target_os = "linux")]
fn wsl_windows_home() -> Option<PathBuf> {
    use std::sync::OnceLock;
    static HOME: OnceLock<Option<PathBuf>> = OnceLock::new();
    HOME.get_or_init(|| {
        let users_dir = PathBuf::from("/mnt/c/Users");
        let entries = std::fs::read_dir(&users_dir).ok()?;
        let skip = ["Public", "Default", "Default User", "All Users"];
        let linux_user = std::env::var("USER").unwrap_or_default();

        let mut candidates: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            if !entry.file_type().ok().is_some_and(|ft| ft.is_dir()) {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if skip.iter().any(|&s| s.eq_ignore_ascii_case(&name_str)) {
                continue;
            }
            if entry.path().join(".claude").is_dir() {
                if name_str.eq_ignore_ascii_case(&linux_user) {
                    return Some(entry.path());
                }
                candidates.push(entry.path());
            }
        }
        candidates.into_iter().next()
    })
    .clone()
}

/// Returns the platform-appropriate path to the Claude Code sessions directory:
/// - Windows: `%USERPROFILE%\.claude\sessions`
/// - Non-Windows: `$HOME/.claude/sessions`
/// - WSL: falls back to `/mnt/c/Users/<user>/.claude/sessions`
fn claude_sessions_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .ok()
            .map(|base| PathBuf::from(base).join(".claude").join("sessions"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let native = std::env::var("HOME")
            .ok()
            .map(|base| PathBuf::from(base).join(".claude").join("sessions"));
        if let Some(ref dir) = native {
            if dir.is_dir() {
                return native;
            }
        }
        #[cfg(target_os = "linux")]
        if is_wsl() {
            if let Some(home) = wsl_windows_home() {
                let wsl_dir = home.join(".claude").join("sessions");
                if wsl_dir.is_dir() {
                    return Some(wsl_dir);
                }
            }
        }
        native
    }
}

/// Returns true if the given process name/cmdline belongs to a Claude Code session.
///
/// Matches:
/// - Process named `claude` or `claude.exe` (case-insensitive)
/// - Process named `node` or `node.exe` (case-insensitive) with `"claude"` anywhere
///   in any cmdline argument (case-insensitive)
pub(crate) fn is_claude_process(name: &str, cmdline: &[String]) -> bool {
    if name.is_empty() {
        return false;
    }
    let name_lower = name.to_lowercase();

    // Direct match: the binary is claude / claude.exe
    if name_lower == "claude" || name_lower == "claude.exe" {
        return true;
    }

    // Node process running a Claude script
    if name_lower == "node" || name_lower == "node.exe" {
        return cmdline
            .iter()
            .any(|arg| arg.to_lowercase().contains("claude"));
    }

    false
}

/// Reads `{dir}/{pid}.json`, parses JSON, and extracts the `"sessionId"` field.
/// Returns `None` on any I/O or parse error.
fn lookup_claude_session_id_in(dir: &Path, pid: u32) -> Option<String> {
    let path = dir.join(format!("{pid}.json"));
    let text = std::fs::read_to_string(&path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_owned())
}

/// Scans all session files in `dir` and returns the `sessionId` of the most
/// recently updated active session (status == "busy" or "idle").
/// Used on WSL where the Linux PID doesn't match the Windows PID in filenames.
#[cfg(any(target_os = "linux", test))]
fn lookup_claude_session_id_by_scan(dir: &Path) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(String, u64)> = None;
    for entry in entries.flatten() {
        if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let text = match std::fs::read_to_string(entry.path()) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let value: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status != "busy" && status != "idle" {
            continue;
        }
        let session_id = value.get("sessionId").and_then(|v| v.as_str());
        let updated_at = value.get("updatedAt").and_then(|v| v.as_u64()).unwrap_or(0);
        if let Some(sid) = session_id {
            if best.as_ref().map_or(true, |b| updated_at > b.1) {
                best = Some((sid.to_owned(), updated_at));
            }
        }
    }
    best.map(|(sid, _)| sid)
}

/// Looks up the Claude Code session ID for the given PID.
///
/// Claude Code stores active session info at `~/.claude/sessions/{pid}.json`.
/// Returns `None` if the sessions directory cannot be determined, the file is
/// absent, or the JSON is malformed / missing the `"sessionId"` field.
///
/// On WSL, falls back to scanning all session files (Linux PID ≠ Windows PID).
pub(crate) fn lookup_claude_session_id(pid: u32) -> Option<String> {
    let dir = claude_sessions_dir()?;
    if let Some(sid) = lookup_claude_session_id_in(&dir, pid) {
        return Some(sid);
    }
    #[cfg(target_os = "linux")]
    if is_wsl() {
        return lookup_claude_session_id_by_scan(&dir);
    }
    None
}

fn claude_home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .ok()
            .map(|base| PathBuf::from(base).join(".claude"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        let native = std::env::var("HOME")
            .ok()
            .map(|base| PathBuf::from(base).join(".claude"));
        if let Some(ref dir) = native {
            if dir.is_dir() {
                return native;
            }
        }
        #[cfg(target_os = "linux")]
        if is_wsl() {
            if let Some(home) = wsl_windows_home() {
                let wsl_dir = home.join(".claude");
                if wsl_dir.is_dir() {
                    return Some(wsl_dir);
                }
            }
        }
        native
    }
}

fn cwd_to_project_dir_name(cwd: &Path) -> String {
    cwd.to_string_lossy().replace([':', '\\', '/'], "-")
}

/// Scans all project subdirectories under `projects_dir` for a `{session_id}.jsonl`
/// file. Used on WSL where the project dir name derived from the Linux CWD won't
/// match the Windows-style name Claude stored.
#[cfg(any(target_os = "linux", test))]
fn find_session_jsonl_in_any_project(projects_dir: &Path, session_id: &str) -> bool {
    let filename = format!("{session_id}.jsonl");
    let entries = match std::fs::read_dir(projects_dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        if entry.file_type().ok().is_some_and(|ft| ft.is_dir())
            && entry.path().join(&filename).exists()
        {
            return true;
        }
    }
    false
}

/// Returns the `claude --resume "<id>"` command if the session file exists on
/// disk, otherwise falls back to `claude --continue`.
///
/// On WSL, scans all project directories since the Linux CWD format differs
/// from the Windows CWD format Claude used when creating the project dir.
pub(crate) fn claude_resume_command(session_id: &str, cwd: &Path) -> String {
    if let Some(claude_home) = claude_home_dir() {
        let project_name = cwd_to_project_dir_name(cwd);
        let jsonl = claude_home
            .join("projects")
            .join(&project_name)
            .join(format!("{session_id}.jsonl"));
        if jsonl.exists() {
            return format!("claude --resume \"{}\"", session_id);
        }
        #[cfg(target_os = "linux")]
        if is_wsl() && find_session_jsonl_in_any_project(&claude_home.join("projects"), session_id)
        {
            return format!("claude --resume \"{}\"", session_id);
        }
    }
    "claude --continue".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── is_claude_process ────────────────────────────────────────────────────

    #[test]
    fn test_is_claude_exact_name() {
        assert!(is_claude_process("claude", &[]));
    }

    #[test]
    fn test_is_claude_exe_name() {
        assert!(is_claude_process("claude.exe", &[]));
    }

    #[test]
    fn test_is_claude_case_insensitive() {
        assert!(is_claude_process("Claude.EXE", &[]));
    }

    #[test]
    fn test_is_claude_node_with_claude_arg() {
        let args = vec![
            "/usr/local/bin/claude".to_string(),
            "--some-flag".to_string(),
        ];
        assert!(is_claude_process("node", &args));
    }

    #[test]
    fn test_is_claude_node_exe_with_claude_arg() {
        let args = vec!["C:\\Users\\dpk\\AppData\\Roaming\\npm\\node_modules\\@anthropic-ai\\claude-code\\cli.js".to_string()];
        assert!(is_claude_process("node.exe", &args));
    }

    #[test]
    fn test_is_not_claude_node_without_claude_arg() {
        let args = vec!["server.js".to_string()];
        assert!(!is_claude_process("node", &args));
    }

    #[test]
    fn test_is_not_claude_other_process() {
        assert!(!is_claude_process("vim", &[]));
    }

    #[test]
    fn test_is_not_claude_empty() {
        assert!(!is_claude_process("", &[]));
    }

    // ── lookup_claude_session_id_in ──────────────────────────────────────────

    #[test]
    fn test_lookup_valid_session_file() {
        let dir = tempdir().unwrap();
        let json = r#"{"pid":12345,"sessionId":"1bd1c774-cef1-41a3-8a06-fec29d11ef29","cwd":"/home/user","status":"busy"}"#;
        std::fs::write(dir.path().join("12345.json"), json).unwrap();
        let result = lookup_claude_session_id_in(dir.path(), 12345);
        assert_eq!(
            result,
            Some("1bd1c774-cef1-41a3-8a06-fec29d11ef29".to_string())
        );
    }

    #[test]
    fn test_lookup_missing_file() {
        let dir = tempdir().unwrap();
        let result = lookup_claude_session_id_in(dir.path(), 99999);
        assert_eq!(result, None);
    }

    #[test]
    fn test_lookup_malformed_json() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("42.json"), "not valid json {{{{").unwrap();
        let result = lookup_claude_session_id_in(dir.path(), 42);
        assert_eq!(result, None);
    }

    // ── cwd_to_project_dir_name ────────────────────────────────────────────

    #[test]
    fn test_cwd_to_project_dir_name_windows() {
        let name = cwd_to_project_dir_name(Path::new("C:\\Users\\dpk\\ws\\my-project"));
        assert_eq!(name, "C--Users-dpk-ws-my-project");
    }

    #[test]
    fn test_cwd_to_project_dir_name_unix() {
        let name = cwd_to_project_dir_name(Path::new("/home/user/project"));
        assert_eq!(name, "-home-user-project");
    }

    // ── claude_resume_command ───────────────────────────────────────────────

    #[test]
    fn test_resume_command_falls_back_when_no_jsonl() {
        let dir = tempdir().unwrap();
        let cwd = dir.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let cmd = claude_resume_command("nonexistent-uuid", &cwd);
        assert_eq!(cmd, "claude --continue");
    }

    #[test]
    fn test_lookup_missing_session_id_field() {
        let dir = tempdir().unwrap();
        let json = r#"{"pid":777,"cwd":"/home/user","status":"idle"}"#;
        std::fs::write(dir.path().join("777.json"), json).unwrap();
        let result = lookup_claude_session_id_in(dir.path(), 777);
        assert_eq!(result, None);
    }

    // ── lookup_claude_session_id_by_scan ────────────────────────────────────

    #[test]
    fn test_scan_finds_most_recent_active_session() {
        let dir = tempdir().unwrap();
        let old = r#"{"pid":100,"sessionId":"old-uuid","status":"busy","updatedAt":1000}"#;
        let new = r#"{"pid":200,"sessionId":"new-uuid","status":"idle","updatedAt":2000}"#;
        std::fs::write(dir.path().join("100.json"), old).unwrap();
        std::fs::write(dir.path().join("200.json"), new).unwrap();
        let result = lookup_claude_session_id_by_scan(dir.path());
        assert_eq!(result, Some("new-uuid".to_string()));
    }

    #[test]
    fn test_scan_skips_non_active_sessions() {
        let dir = tempdir().unwrap();
        let done = r#"{"pid":100,"sessionId":"done-uuid","status":"completed","updatedAt":3000}"#;
        let active = r#"{"pid":200,"sessionId":"active-uuid","status":"busy","updatedAt":1000}"#;
        std::fs::write(dir.path().join("100.json"), done).unwrap();
        std::fs::write(dir.path().join("200.json"), active).unwrap();
        let result = lookup_claude_session_id_by_scan(dir.path());
        assert_eq!(result, Some("active-uuid".to_string()));
    }

    #[test]
    fn test_scan_returns_none_for_empty_dir() {
        let dir = tempdir().unwrap();
        let result = lookup_claude_session_id_by_scan(dir.path());
        assert_eq!(result, None);
    }

    #[test]
    fn test_scan_skips_non_json_files() {
        let dir = tempdir().unwrap();
        let json = r#"{"pid":100,"sessionId":"test-uuid","status":"busy","updatedAt":1000}"#;
        std::fs::write(dir.path().join("100.txt"), json).unwrap();
        let result = lookup_claude_session_id_by_scan(dir.path());
        assert_eq!(result, None);
    }

    #[test]
    fn test_scan_skips_malformed_json() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("100.json"), "not json {{").unwrap();
        let valid = r#"{"pid":200,"sessionId":"valid-uuid","status":"idle","updatedAt":1000}"#;
        std::fs::write(dir.path().join("200.json"), valid).unwrap();
        let result = lookup_claude_session_id_by_scan(dir.path());
        assert_eq!(result, Some("valid-uuid".to_string()));
    }

    // ── find_session_jsonl_in_any_project ───────────────────────────────────

    #[test]
    fn test_find_jsonl_in_matching_project_dir() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("C--Users-dpk-ws-project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("abc-123.jsonl"), "data").unwrap();
        assert!(find_session_jsonl_in_any_project(dir.path(), "abc-123"));
    }

    #[test]
    fn test_find_jsonl_returns_false_when_missing() {
        let dir = tempdir().unwrap();
        let project = dir.path().join("some-project");
        std::fs::create_dir_all(&project).unwrap();
        assert!(!find_session_jsonl_in_any_project(
            dir.path(),
            "nonexistent"
        ));
    }

    #[test]
    fn test_find_jsonl_returns_false_for_empty_projects_dir() {
        let dir = tempdir().unwrap();
        assert!(!find_session_jsonl_in_any_project(dir.path(), "any-id"));
    }

    #[test]
    fn test_find_jsonl_scans_multiple_project_dirs() {
        let dir = tempdir().unwrap();
        let proj_a = dir.path().join("project-a");
        let proj_b = dir.path().join("project-b");
        std::fs::create_dir_all(&proj_a).unwrap();
        std::fs::create_dir_all(&proj_b).unwrap();
        std::fs::write(proj_b.join("target-uuid.jsonl"), "data").unwrap();
        assert!(find_session_jsonl_in_any_project(dir.path(), "target-uuid"));
    }
}
