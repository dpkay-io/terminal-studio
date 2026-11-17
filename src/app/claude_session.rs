use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read};
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

const JS_RUNTIMES: &[&str] = &[
    "node", "node.exe", "bun", "bun.exe", "deno", "deno.exe", "npx", "npx.exe",
];

/// Returns true if the given process name/cmdline belongs to a Claude Code session.
///
/// Matches:
/// - Process named `claude` or `claude.exe` (case-insensitive)
/// - Any JS runtime (`node`, `bun`, `deno`, `npx` + `.exe` variants) with
///   `"claude"` anywhere in any cmdline argument (case-insensitive)
pub(crate) fn is_claude_process(name: &str, cmdline: &[String]) -> bool {
    if name.is_empty() {
        return false;
    }
    let name_lower = name.to_lowercase();

    if name_lower == "claude" || name_lower == "claude.exe" {
        return true;
    }

    if JS_RUNTIMES.iter().any(|&rt| name_lower == rt) {
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

pub(crate) fn normalize_cwd_for_match(cwd: &str) -> String {
    cwd.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// Scans all session files in `dir` and returns the `sessionId` of the most
/// recently updated session whose `cwd` matches the given path, leaving out
/// the ids in `exclude` (conversations other open sessions already hold, so
/// one folder with several Claude sessions never hands one of them
/// another's id). When `active_only` is true, only "busy"/"idle" sessions
/// are considered; when false, any status is accepted (for close-time
/// capture where Claude may have already exited and updated its status to
/// "completed").
fn lookup_claude_session_id_by_cwd_in(
    dir: &Path,
    cwd: &str,
    active_only: bool,
    exclude: &HashSet<String>,
) -> Option<String> {
    let entries = std::fs::read_dir(dir).ok()?;
    let target = normalize_cwd_for_match(cwd);
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
        if active_only {
            let status = value.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if status != "busy" && status != "idle" {
                continue;
            }
        }
        let file_cwd = value.get("cwd").and_then(|v| v.as_str()).unwrap_or("");
        if normalize_cwd_for_match(file_cwd) != target {
            continue;
        }
        let session_id = value
            .get("sessionId")
            .and_then(|v| v.as_str())
            .filter(|sid| !exclude.contains(*sid));
        let updated_at = value.get("updatedAt").and_then(|v| v.as_u64()).unwrap_or(0);
        if let Some(sid) = session_id {
            if best.as_ref().map_or(true, |b| updated_at > b.1) {
                best = Some((sid.to_owned(), updated_at));
            }
        }
    }
    best.map(|(sid, _)| sid)
}

/// Scans `~/.claude/sessions/` for an active session matching the given CWD
/// that no other open session holds (`exclude`).
/// Only matches sessions with "busy"/"idle" status (currently running).
pub(crate) fn lookup_claude_session_id_by_cwd(
    cwd: &str,
    exclude: &HashSet<String>,
) -> Option<String> {
    let dir = claude_sessions_dir()?;
    lookup_claude_session_id_by_cwd_in(&dir, cwd, true, exclude)
}

/// Like `lookup_claude_session_id_by_cwd` but accepts any session status.
/// Used at session close time when Claude may have already exited and updated
/// its status to "completed".
pub(crate) fn lookup_claude_session_id_by_cwd_any(
    cwd: &str,
    exclude: &HashSet<String>,
) -> Option<String> {
    let dir = claude_sessions_dir()?;
    lookup_claude_session_id_by_cwd_in(&dir, cwd, false, exclude)
}

/// The Claude conversation a closing session had: the one already known
/// for it, else what `scan` finds for its folder among the conversations
/// no other open session holds (`taken`). `scan` reads files, so it runs
/// only when needed, off the UI thread.
pub(crate) fn pick_claude_session_id(
    known: Option<String>,
    scan: impl FnOnce(&HashSet<String>) -> Option<String>,
    taken: &HashSet<String>,
) -> Option<String> {
    known.or_else(|| scan(taken))
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

/// Claude keeps a project folder's name as is up to this many characters;
/// a longer one is cut to it and gets a hash of the full path, a hash this
/// app can't reproduce (such folders are found by searching instead).
const PROJECT_DIR_NAME_MAX_LEN: usize = 200;
/// What Claude puts in place of each character that isn't an ASCII letter or digit.
const PROJECT_DIR_NAME_FILL: char = '-';
/// Extension of a conversation's transcript, `<session id>.jsonl`.
const TRANSCRIPT_EXTENSION: &str = "jsonl";
/// How much of a transcript is read to find the folder it began in (its
/// first lines carry it; a transcript can be many megabytes).
const TRANSCRIPT_CWD_SCAN_BYTES: u64 = 1024 * 1024;

/// Claude Code's name for `cwd`'s folder under `~/.claude/projects`. Claude
/// builds it as `path.replace(/[^a-zA-Z0-9]/g, "-")` on the path's UTF-16
/// units (read from its binary), so `C:\ws\my_app.v2` is
/// `C--ws-my-app-v2` and a character outside the BMP gives two dashes.
/// `None` when the name is longer than Claude keeps as is.
fn claude_project_dir_name(cwd: &Path) -> Option<String> {
    let name: String = cwd
        .to_string_lossy()
        .encode_utf16()
        .map(|unit| match u8::try_from(unit) {
            Ok(byte) if byte.is_ascii_alphanumeric() => char::from(byte),
            _ => PROJECT_DIR_NAME_FILL,
        })
        .collect();
    (name.len() <= PROJECT_DIR_NAME_MAX_LEN).then_some(name)
}

/// A Claude session id is safe to use as a file name and inside a shell
/// command: letters, digits, `-` and `_` only (Claude's ids are UUIDs).
fn is_valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The transcript file `file_name` in any project folder under `projects`.
/// Session ids are unique, so the first match is the one.
fn find_transcript_in_any_project(projects: &Path, file_name: &str) -> Option<PathBuf> {
    std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_dir()))
        .map(|entry| entry.path().join(file_name))
        .find(|transcript| transcript.is_file())
}

/// The folder a conversation began in: the first `"cwd"` its transcript
/// records (Claude keeps one working directory for a whole session).
fn transcript_cwd(transcript: &Path) -> Option<PathBuf> {
    let file = std::fs::File::open(transcript).ok()?;
    BufReader::new(file.take(TRANSCRIPT_CWD_SCAN_BYTES))
        .lines()
        .map_while(Result::ok)
        .find_map(|line| {
            let value: serde_json::Value = serde_json::from_str(&line).ok()?;
            let cwd = value.get("cwd")?.as_str()?;
            (!cwd.is_empty()).then(|| PathBuf::from(cwd))
        })
}

/// Start of the command that resumes one Claude conversation by its id.
const RESUME_COMMAND_PREFIX: &str = "claude --resume ";
/// The command that resumes a folder's latest Claude conversation.
pub(crate) const CONTINUE_COMMAND: &str = "claude --continue";

fn resume_command_for(session_id: &str) -> String {
    format!("{RESUME_COMMAND_PREFIX}\"{session_id}\"")
}

/// The conversation id in a command [`resume_plan`] built, so a session
/// closed before it ever started can still resume that conversation.
pub(crate) fn resumed_session_id(command: &str) -> Option<String> {
    let id = command
        .strip_prefix(RESUME_COMMAND_PREFIX)?
        .trim()
        .trim_matches('"');
    (!id.is_empty()).then(|| id.to_string())
}

/// How to bring back a Claude conversation: the command, and the folder
/// the terminal must start in. Claude resumes a conversation only from the
/// folder it began in (from anywhere else it prints "This conversation is
/// from a different directory").
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeResume {
    pub(crate) command: String,
    pub(crate) cwd: PathBuf,
}

impl ClaudeResume {
    /// The folder to start the terminal in; `None` when it isn't known (a
    /// session saved without one), so the terminal's default applies.
    pub(crate) fn start_dir(&self) -> Option<PathBuf> {
        (!self.cwd.as_os_str().is_empty()).then(|| self.cwd.clone())
    }
}

/// `claude --resume "<id>"` for a conversation whose transcript is on disk
/// under `projects` (`~/.claude/projects`), else `None`. Looks in the folder
/// Claude names after `cwd` first (one check), then in every project folder
/// (`cwd` may not be where the conversation began, or its name may be too
/// long to derive); a transcript found elsewhere resumes in the folder it
/// records, when that folder exists here (on WSL it is a Windows path, so
/// the terminal's own folder is kept).
fn find_resume_in(projects: &Path, session_id: &str, cwd: &Path) -> Option<ClaudeResume> {
    if !is_valid_session_id(session_id) {
        return None;
    }
    let file_name = format!("{session_id}.{TRANSCRIPT_EXTENSION}");
    let resume_in = |cwd: PathBuf| ClaudeResume {
        command: resume_command_for(session_id),
        cwd,
    };
    let in_own_folder = claude_project_dir_name(cwd)
        .is_some_and(|name| projects.join(name).join(&file_name).is_file());
    if in_own_folder {
        return Some(resume_in(cwd.to_path_buf()));
    }
    let transcript = find_transcript_in_any_project(projects, &file_name)?;
    let began_in = transcript_cwd(&transcript).filter(|dir| dir.is_dir());
    Some(resume_in(began_in.unwrap_or_else(|| cwd.to_path_buf())))
}

/// What brings back the Claude conversation `session_id` that a terminal in
/// `cwd` had: `claude --resume` when its transcript is on disk (see
/// [`find_resume_in`]), else `claude --continue` in `cwd` when
/// `continue_allowed` (no other Claude session there could be the one
/// continued), else nothing — logged, since the conversation then has to be
/// resumed by hand.
pub(crate) fn resume_plan(
    session_id: &str,
    cwd: &Path,
    continue_allowed: bool,
) -> Option<ClaudeResume> {
    let projects = claude_home_dir().map(|home| home.join("projects"));
    plan_resume_in(projects.as_deref(), session_id, cwd, continue_allowed)
}

fn plan_resume_in(
    projects: Option<&Path>,
    session_id: &str,
    cwd: &Path,
    continue_allowed: bool,
) -> Option<ClaudeResume> {
    if let Some(found) = projects.and_then(|dir| find_resume_in(dir, session_id, cwd)) {
        return Some(found);
    }
    if continue_allowed {
        log::info!(
            "claude transcript {session_id} not found; continuing the latest in {}",
            cwd.display()
        );
        return Some(ClaudeResume {
            command: CONTINUE_COMMAND.to_string(),
            cwd: cwd.to_path_buf(),
        });
    }
    log::warn!(
        "claude transcript {session_id} not found under {:?} and {} has other Claude sessions; not resuming",
        projects,
        cwd.display()
    );
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn known_id_wins_without_scanning() {
        let picked = pick_claude_session_id(
            Some("known".into()),
            |_| panic!("no scan when the id is known"),
            &none(),
        );
        assert_eq!(picked.as_deref(), Some("known"));
    }

    #[test]
    fn scan_gets_the_ids_other_sessions_hold() {
        let taken: HashSet<String> = ["other".to_string()].into();
        let picked = pick_claude_session_id(
            None,
            |exclude| {
                assert_eq!(exclude, &taken);
                Some("scanned".into())
            },
            &taken,
        );
        assert_eq!(picked.as_deref(), Some("scanned"));
    }
    use tempfile::tempdir;

    #[test]
    fn a_resume_command_gives_back_its_conversation_id() {
        let cmd = resume_command_for("abc-123");
        assert_eq!(resumed_session_id(&cmd).as_deref(), Some("abc-123"));
        assert_eq!(resumed_session_id("claude --continue"), None);
        assert_eq!(resumed_session_id("claude --resume \"\""), None);
        assert_eq!(resumed_session_id("ls -la"), None);
    }

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
    fn test_is_claude_bun_with_claude_arg() {
        let args = vec!["claude".to_string()];
        assert!(is_claude_process("bun", &args));
    }

    #[test]
    fn test_is_claude_bun_exe_with_claude_arg() {
        let args = vec!["C:\\Users\\user\\AppData\\bun\\claude\\cli.js".to_string()];
        assert!(is_claude_process("bun.exe", &args));
    }

    #[test]
    fn test_is_not_claude_bun_without_claude_arg() {
        let args = vec!["server.ts".to_string()];
        assert!(!is_claude_process("bun", &args));
    }

    #[test]
    fn test_is_claude_npx_with_claude_arg() {
        let args = vec!["@anthropic-ai/claude-code".to_string()];
        assert!(is_claude_process("npx", &args));
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

    // ── claude_project_dir_name ────────────────────────────────────────────

    fn project_name(path: &str) -> Option<String> {
        claude_project_dir_name(Path::new(path))
    }

    #[test]
    fn project_name_windows_path() {
        assert_eq!(
            project_name(r"C:\Users\dpk\ws\my-project").as_deref(),
            Some("C--Users-dpk-ws-my-project")
        );
    }

    #[test]
    fn project_name_unix_path() {
        assert_eq!(
            project_name("/home/user/project").as_deref(),
            Some("-home-user-project")
        );
    }

    /// The case that broke resume: Claude turns `_` (and every other
    /// non-alphanumeric) into `-` too.
    #[test]
    fn project_name_underscore_becomes_dash() {
        assert_eq!(
            project_name(r"C:\workspace\ws-riya\Blusky\blub0x_web").as_deref(),
            Some("C--workspace-ws-riya-Blusky-blub0x-web")
        );
    }

    /// Names on this Mac's own `~/.claude/projects`, as Claude wrote them.
    #[test]
    fn project_name_matches_folders_claude_wrote() {
        assert_eq!(
            project_name("/Users/dpk/ws/dpk-jha.github.io").as_deref(),
            Some("-Users-dpk-ws-dpk-jha-github-io")
        );
        assert_eq!(
            project_name("/private/tmp/claude-501/-Users-dpk-ws-ghostpm").as_deref(),
            Some("-private-tmp-claude-501--Users-dpk-ws-ghostpm")
        );
    }

    #[test]
    fn project_name_spaces_and_symbols() {
        assert_eq!(
            project_name("/a b/c+d@e~f").as_deref(),
            Some("-a-b-c-d-e-f")
        );
    }

    /// JS replaces per UTF-16 unit: one dash for a BMP character, two for
    /// one outside it (an emoji is a surrogate pair).
    #[test]
    fn project_name_non_ascii_per_utf16_unit() {
        assert_eq!(project_name("/caf\u{e9}").as_deref(), Some("-caf-"));
        assert_eq!(project_name("/x\u{1F600}y").as_deref(), Some("-x--y"));
    }

    #[test]
    fn project_name_too_long_to_derive() {
        let at_limit = format!("/{}", "a".repeat(PROJECT_DIR_NAME_MAX_LEN - 1));
        assert_eq!(
            project_name(&at_limit).map(|n| n.len()),
            Some(PROJECT_DIR_NAME_MAX_LEN)
        );
        let over = format!("/{}", "a".repeat(PROJECT_DIR_NAME_MAX_LEN));
        assert_eq!(project_name(&over), None);
    }

    // ── session ids ──────────────────────────────────────────────────────────

    #[test]
    fn session_id_validation() {
        assert!(is_valid_session_id("4718ef8e-2253-4d9c-9ace-afc3d715b92c"));
        assert!(is_valid_session_id("abc_123"));
        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("../etc/passwd"));
        assert!(!is_valid_session_id("x\"; rm -rf ~; \""));
        assert!(!is_valid_session_id("a b"));
    }

    // ── find_resume_in / plan_resume_in ──────────────────────────────────────

    const SID: &str = "4718ef8e-2253-4d9c-9ace-afc3d715b92c";

    /// Writes `<projects>/<folder>/<SID>.jsonl` with `lines`.
    fn write_transcript(projects: &Path, folder: &str, lines: &[&str]) {
        let dir = projects.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{SID}.jsonl")), lines.join("\n")).unwrap();
    }

    fn cwd_line(cwd: &Path) -> String {
        serde_json::json!({ "type": "user", "cwd": cwd.to_string_lossy() }).to_string()
    }

    #[test]
    fn resume_found_in_the_folder_claude_names() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        let cwd = root.path().join("my_app.v2");
        std::fs::create_dir_all(&cwd).unwrap();
        write_transcript(&projects, &claude_project_dir_name(&cwd).unwrap(), &["{}"]);
        let found = find_resume_in(&projects, SID, &cwd).unwrap();
        assert_eq!(found.command, format!("claude --resume \"{SID}\""));
        assert_eq!(found.cwd, cwd);
    }

    /// The old naming (only `:`, `\`, `/` replaced) looked in a folder that
    /// doesn't exist; the transcript is still found.
    #[test]
    fn resume_found_for_a_folder_with_an_underscore() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        let cwd = root.path().join("blub0x_web");
        std::fs::create_dir_all(&cwd).unwrap();
        let name = claude_project_dir_name(&cwd).unwrap();
        assert!(name.ends_with("blub0x-web"));
        write_transcript(&projects, &name, &["{}"]);
        assert!(find_resume_in(&projects, SID, &cwd).is_some());
    }

    /// A transcript under another folder resumes in the folder it began in.
    #[test]
    fn resume_elsewhere_starts_in_the_folder_the_conversation_began_in() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        let began_in = root.path().join("began_here");
        let terminal_cwd = root.path().join("moved_on");
        std::fs::create_dir_all(&began_in).unwrap();
        std::fs::create_dir_all(&terminal_cwd).unwrap();
        write_transcript(
            &projects,
            &claude_project_dir_name(&began_in).unwrap(),
            &[
                r#"{"type":"mode"}"#,
                "not json",
                &cwd_line(&began_in),
                &cwd_line(&terminal_cwd),
            ],
        );
        let found = find_resume_in(&projects, SID, &terminal_cwd).unwrap();
        assert_eq!(found.cwd, began_in, "first recorded cwd wins");
    }

    /// A path too long to name (Claude hashes it) is found by searching.
    #[test]
    fn resume_found_for_a_path_too_long_to_name() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        let long = root.path().join("d".repeat(PROJECT_DIR_NAME_MAX_LEN));
        assert!(claude_project_dir_name(&long).is_none());
        write_transcript(&projects, "cut-name-abc123", &[&cwd_line(&long)]);
        let found = find_resume_in(&projects, SID, &long).unwrap();
        assert_eq!(
            found.cwd, long,
            "began-in folder missing here: keep the terminal's"
        );
    }

    #[test]
    fn resume_keeps_terminal_folder_when_transcript_names_none() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        let cwd = root.path().join("here");
        write_transcript(
            &projects,
            "elsewhere",
            &[r#"{"cwd":"C:\\not\\on\\this\\machine"}"#],
        );
        assert_eq!(find_resume_in(&projects, SID, &cwd).unwrap().cwd, cwd);
    }

    #[test]
    fn no_resume_without_a_transcript() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        std::fs::create_dir_all(projects.join("some-project")).unwrap();
        assert_eq!(find_resume_in(&projects, SID, root.path()), None);
        assert_eq!(
            find_resume_in(&root.path().join("missing"), SID, root.path()),
            None
        );
    }

    #[test]
    fn no_resume_for_an_unsafe_id() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        let bad = "../../x";
        std::fs::create_dir_all(projects.join("p")).unwrap();
        std::fs::write(projects.join("p").join(format!("{bad}.jsonl")), "{}").ok();
        assert_eq!(find_resume_in(&projects, bad, root.path()), None);
    }

    #[test]
    fn plan_continues_when_allowed_and_not_found() {
        let root = tempdir().unwrap();
        let plan =
            plan_resume_in(Some(&root.path().join("projects")), SID, root.path(), true).unwrap();
        assert_eq!(plan.command, CONTINUE_COMMAND);
        assert_eq!(plan.cwd, root.path());
        assert_eq!(
            plan_resume_in(None, SID, root.path(), true)
                .unwrap()
                .command,
            CONTINUE_COMMAND
        );
    }

    #[test]
    fn plan_nothing_when_not_found_and_continue_unsafe() {
        let root = tempdir().unwrap();
        assert_eq!(
            plan_resume_in(Some(root.path()), SID, root.path(), false),
            None
        );
    }

    #[test]
    fn plan_prefers_resume_over_continue() {
        let root = tempdir().unwrap();
        let projects = root.path().join("projects");
        write_transcript(
            &projects,
            &claude_project_dir_name(root.path()).unwrap(),
            &["{}"],
        );
        let plan = plan_resume_in(Some(&projects), SID, root.path(), true).unwrap();
        assert_eq!(resumed_session_id(&plan.command).as_deref(), Some(SID));
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

    // ── normalize_cwd_for_match ────────────────────────────────────────────

    #[test]
    fn test_normalize_cwd_backslashes() {
        assert_eq!(
            normalize_cwd_for_match(r"C:\Users\dpk\ws\project"),
            "c:/users/dpk/ws/project"
        );
    }

    #[test]
    fn test_normalize_cwd_trailing_slash() {
        assert_eq!(
            normalize_cwd_for_match("/home/user/project/"),
            "/home/user/project"
        );
    }

    #[test]
    fn test_normalize_cwd_case_insensitive() {
        assert_eq!(
            normalize_cwd_for_match("C:/Users/DPK/WS"),
            normalize_cwd_for_match("c:/users/dpk/ws")
        );
    }

    // ── lookup_claude_session_id_by_cwd_in ─────────────────────────────────

    #[test]
    fn test_cwd_lookup_finds_matching_session() {
        let dir = tempdir().unwrap();
        let json = r#"{"pid":100,"sessionId":"cwd-uuid","cwd":"C:\\Users\\dpk\\ws\\project","status":"busy","updatedAt":1000}"#;
        std::fs::write(dir.path().join("100.json"), json).unwrap();
        let result = lookup_claude_session_id_by_cwd_in(
            dir.path(),
            r"C:\Users\dpk\ws\project",
            true,
            &none(),
        );
        assert_eq!(result, Some("cwd-uuid".to_string()));
    }

    #[test]
    fn test_cwd_lookup_ignores_different_cwd() {
        let dir = tempdir().unwrap();
        let json = r#"{"pid":100,"sessionId":"other-uuid","cwd":"/home/other/project","status":"busy","updatedAt":1000}"#;
        std::fs::write(dir.path().join("100.json"), json).unwrap();
        let result =
            lookup_claude_session_id_by_cwd_in(dir.path(), "/home/user/project", true, &none());
        assert_eq!(result, None);
    }

    #[test]
    fn test_cwd_lookup_skips_inactive_sessions() {
        let dir = tempdir().unwrap();
        let json = r#"{"pid":100,"sessionId":"done-uuid","cwd":"/home/user/project","status":"completed","updatedAt":2000}"#;
        std::fs::write(dir.path().join("100.json"), json).unwrap();
        let result =
            lookup_claude_session_id_by_cwd_in(dir.path(), "/home/user/project", true, &none());
        assert_eq!(result, None);
    }

    #[test]
    fn test_cwd_lookup_any_status_finds_completed() {
        let dir = tempdir().unwrap();
        let json = r#"{"pid":100,"sessionId":"done-uuid","cwd":"/home/user/project","status":"completed","updatedAt":2000}"#;
        std::fs::write(dir.path().join("100.json"), json).unwrap();
        let result =
            lookup_claude_session_id_by_cwd_in(dir.path(), "/home/user/project", false, &none());
        assert_eq!(result, Some("done-uuid".to_string()));
    }

    #[test]
    fn test_cwd_lookup_picks_most_recent() {
        let dir = tempdir().unwrap();
        let old =
            r#"{"pid":100,"sessionId":"old-uuid","cwd":"C:\\ws","status":"busy","updatedAt":1000}"#;
        let new =
            r#"{"pid":200,"sessionId":"new-uuid","cwd":"C:\\ws","status":"idle","updatedAt":2000}"#;
        std::fs::write(dir.path().join("100.json"), old).unwrap();
        std::fs::write(dir.path().join("200.json"), new).unwrap();
        let result = lookup_claude_session_id_by_cwd_in(dir.path(), r"C:\ws", true, &none());
        assert_eq!(result, Some("new-uuid".to_string()));
    }

    #[test]
    fn test_cwd_lookup_normalizes_separators() {
        let dir = tempdir().unwrap();
        let json = r#"{"pid":100,"sessionId":"norm-uuid","cwd":"C:\\Users\\dpk","status":"busy","updatedAt":1000}"#;
        std::fs::write(dir.path().join("100.json"), json).unwrap();
        let result = lookup_claude_session_id_by_cwd_in(dir.path(), "C:/Users/dpk", true, &none());
        assert_eq!(result, Some("norm-uuid".to_string()));
    }

    #[test]
    fn test_cwd_lookup_empty_dir() {
        let dir = tempdir().unwrap();
        let result = lookup_claude_session_id_by_cwd_in(dir.path(), "/home/user", true, &none());
        assert_eq!(result, None);
    }

    #[test]
    fn test_cwd_lookup_skips_ids_held_elsewhere_and_takes_the_next() {
        let dir = tempdir().unwrap();
        let old = r#"{"pid":100,"sessionId":"free-uuid","cwd":"C:\\ws","status":"busy","updatedAt":1000}"#;
        let new = r#"{"pid":200,"sessionId":"held-uuid","cwd":"C:\\ws","status":"idle","updatedAt":2000}"#;
        std::fs::write(dir.path().join("100.json"), old).unwrap();
        std::fs::write(dir.path().join("200.json"), new).unwrap();
        let held: HashSet<String> = ["held-uuid".to_string()].into();
        let result = lookup_claude_session_id_by_cwd_in(dir.path(), r"C:\ws", true, &held);
        assert_eq!(result, Some("free-uuid".to_string()));
        let all: HashSet<String> = ["held-uuid".to_string(), "free-uuid".to_string()].into();
        assert_eq!(
            lookup_claude_session_id_by_cwd_in(dir.path(), r"C:\ws", true, &all),
            None
        );
    }

    #[test]
    fn start_dir_is_none_for_an_unknown_folder() {
        let resume = |cwd: &str| ClaudeResume {
            command: CONTINUE_COMMAND.into(),
            cwd: cwd.into(),
        };
        assert_eq!(resume("").start_dir(), None);
        assert_eq!(resume("/a").start_dir(), Some(PathBuf::from("/a")));
    }
}
