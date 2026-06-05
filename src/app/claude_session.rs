use std::path::{Path, PathBuf};

/// Returns the platform-appropriate path to the Claude Code sessions directory:
/// - Windows: `%USERPROFILE%\.claude\sessions`
/// - Non-Windows: `$HOME/.claude/sessions`
fn claude_sessions_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE")
            .ok()
            .map(|base| PathBuf::from(base).join(".claude").join("sessions"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .ok()
            .map(|base| PathBuf::from(base).join(".claude").join("sessions"))
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

/// Looks up the Claude Code session ID for the given PID.
///
/// Claude Code stores active session info at `~/.claude/sessions/{pid}.json`.
/// Returns `None` if the sessions directory cannot be determined, the file is
/// absent, or the JSON is malformed / missing the `"sessionId"` field.
pub(crate) fn lookup_claude_session_id(pid: u32) -> Option<String> {
    let dir = claude_sessions_dir()?;
    lookup_claude_session_id_in(&dir, pid)
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

    #[test]
    fn test_lookup_missing_session_id_field() {
        let dir = tempdir().unwrap();
        let json = r#"{"pid":777,"cwd":"/home/user","status":"idle"}"#;
        std::fs::write(dir.path().join("777.json"), json).unwrap();
        let result = lookup_claude_session_id_in(dir.path(), 777);
        assert_eq!(result, None);
    }
}
