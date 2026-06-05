# Claude Code Session Resume — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect when Claude Code CLI is running in a terminal pane, capture its session ID, and auto-resume the exact conversation on session restore.

**Architecture:** The `ForegroundWorker` background thread (500ms poll) is extended to detect Claude processes and look up their session IDs from `~/.claude/sessions/{pid}.json`. Session IDs are stored eagerly on `SessionEntry`, persisted to `SavedSession`, and used at restore time to generate `claude --resume "<id>"` commands. The existing `pending_command` mechanism handles the actual command injection.

**Tech Stack:** Rust, serde_json (already a dependency), std::fs for tiny JSON reads

**Dependency note:** Tasks 6 and 7 (adding `tempfile` and `dirs` crate dependencies) must be completed before Task 1, since Task 1's tests use `tempfile::tempdir()` and the implementation uses `dirs::home_dir()`.

---

### Task 1: Claude Session Detection Module

**Files:**
- Create: `src/app/claude_session.rs`

- [ ] **Step 1: Write failing tests for `is_claude_process`**

In `src/app/claude_session.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_claude_exact_name() {
        assert!(is_claude_process("claude", &["claude".into()]));
    }

    #[test]
    fn test_is_claude_exe_name() {
        assert!(is_claude_process("claude.exe", &["claude.exe".into()]));
    }

    #[test]
    fn test_is_claude_case_insensitive() {
        assert!(is_claude_process("Claude.EXE", &["Claude.EXE".into()]));
    }

    #[test]
    fn test_is_claude_node_with_claude_arg() {
        assert!(is_claude_process(
            "node",
            &["node".into(), "/usr/lib/claude/bin/claude".into()]
        ));
    }

    #[test]
    fn test_is_claude_node_exe_with_claude_arg() {
        assert!(is_claude_process(
            "node.exe",
            &["node.exe".into(), "C:\\Users\\user\\AppData\\claude".into()]
        ));
    }

    #[test]
    fn test_is_not_claude_node_without_claude_arg() {
        assert!(!is_claude_process(
            "node",
            &["node".into(), "server.js".into()]
        ));
    }

    #[test]
    fn test_is_not_claude_other_process() {
        assert!(!is_claude_process("vim", &["vim".into()]));
    }

    #[test]
    fn test_is_not_claude_empty() {
        assert!(!is_claude_process("", &[]));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib claude_session -- --nocapture`
Expected: FAIL — module and function don't exist.

- [ ] **Step 3: Implement `is_claude_process` and register module**

In `src/app/claude_session.rs`, above the `#[cfg(test)]` block:

```rust
pub(super) fn is_claude_process(name: &str, cmdline: &[String]) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == "claude" || lower == "claude.exe" {
        return true;
    }
    if lower == "node" || lower == "node.exe" {
        return cmdline
            .iter()
            .any(|a| a.to_ascii_lowercase().contains("claude"));
    }
    false
}
```

In `src/app.rs`, add among the module declarations (after line 22, alphabetical):

```rust
mod claude_session;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib claude_session -- --nocapture`
Expected: All 8 tests PASS.

- [ ] **Step 5: Write failing tests for `lookup_claude_session_id`**

Add to the `tests` module in `src/app/claude_session.rs`:

```rust
    #[test]
    fn test_lookup_valid_session_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("12345.json");
        std::fs::write(
            &file,
            r#"{"pid":12345,"sessionId":"abc-def-123","cwd":"/tmp","status":"busy"}"#,
        )
        .unwrap();
        let result = lookup_claude_session_id_in(dir.path(), 12345);
        assert_eq!(result, Some("abc-def-123".to_string()));
    }

    #[test]
    fn test_lookup_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let result = lookup_claude_session_id_in(dir.path(), 99999);
        assert_eq!(result, None);
    }

    #[test]
    fn test_lookup_malformed_json() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("12345.json");
        std::fs::write(&file, "not valid json").unwrap();
        let result = lookup_claude_session_id_in(dir.path(), 12345);
        assert_eq!(result, None);
    }

    #[test]
    fn test_lookup_missing_session_id_field() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("12345.json");
        std::fs::write(&file, r#"{"pid":12345,"cwd":"/tmp"}"#).unwrap();
        let result = lookup_claude_session_id_in(dir.path(), 12345);
        assert_eq!(result, None);
    }
```

- [ ] **Step 6: Run tests to verify they fail**

Run: `cargo test --lib claude_session -- --nocapture`
Expected: FAIL — `lookup_claude_session_id_in` not defined.

- [ ] **Step 7: Implement `lookup_claude_session_id_in` and `lookup_claude_session_id`**

Add to `src/app/claude_session.rs`, above the tests:

```rust
use std::path::Path;

fn claude_sessions_dir() -> Option<std::path::PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("sessions"))
}

pub(super) fn lookup_claude_session_id(pid: u32) -> Option<String> {
    lookup_claude_session_id_in(&claude_sessions_dir()?, pid)
}

fn lookup_claude_session_id_in(dir: &Path, pid: u32) -> Option<String> {
    let path = dir.join(format!("{pid}.json"));
    let content = std::fs::read_to_string(&path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&content).ok()?;
    val.get("sessionId")?.as_str().map(|s| s.to_string())
}
```

- [ ] **Step 8: Run all tests to verify they pass**

Run: `cargo test --lib claude_session -- --nocapture`
Expected: All 12 tests PASS.

- [ ] **Step 9: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: Clean.

- [ ] **Step 10: Commit**

```bash
git add src/app/claude_session.rs src/app.rs
git commit -m "Add claude_session module with process detection and session ID lookup"
```

---

### Task 2: Add PID to ForegroundProcess

**Files:**
- Modify: `src/pty/foreground.rs`

- [ ] **Step 1: Write failing test for PID field**

Add to `src/pty/foreground.rs` at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_foreground_process_with_pid() {
        let proc = ForegroundProcess {
            name: "claude".into(),
            cmdline: vec!["claude".into()],
            pid: Some(12345),
        };
        assert_eq!(proc.pid, Some(12345));
        assert_eq!(proc.name, "claude");
    }

    #[test]
    fn test_foreground_process_without_pid() {
        let proc = ForegroundProcess {
            name: "vim".into(),
            cmdline: vec!["vim".into()],
            pid: None,
        };
        assert_eq!(proc.pid, None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib foreground::tests -- --nocapture`
Expected: FAIL — `pid` field doesn't exist.

- [ ] **Step 3: Add `pid` field to `ForegroundProcess` struct**

In `src/pty/foreground.rs`, change the struct (line 2-9):

```rust
#[derive(Clone, Debug)]
pub struct ForegroundProcess {
    pub name: String,
    pub cmdline: Vec<String>,
    pub pid: Option<u32>,
}
```

- [ ] **Step 4: Update Windows `detect_child` to populate `pid`**

In `src/pty/foreground.rs`, update the Windows `detect_child` function (line 38-44):

```rust
    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        let (child_pid, name) = find_child(shell_pid)?;
        Some(ForegroundProcess {
            cmdline: vec![name.clone()],
            name,
            pid: Some(child_pid),
        })
    }
```

- [ ] **Step 5: Update Linux `detect_child` to populate `pid`**

In `src/pty/foreground.rs`, update the Linux `detect_child` function (lines 89-108). Change the return statement to include `pid`:

```rust
    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        let fg_pid = find_foreground_pid(shell_pid)?;
        let cmdline_bytes = std::fs::read(format!("/proc/{}/cmdline", fg_pid)).ok()?;
        if cmdline_bytes.is_empty() {
            return None;
        }
        let args: Vec<String> = cmdline_bytes
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        if args.is_empty() {
            return None;
        }
        let name = args[0].rsplit('/').next().unwrap_or(&args[0]).to_string();
        Some(ForegroundProcess {
            name,
            cmdline: args,
            pid: Some(fg_pid),
        })
    }
```

- [ ] **Step 6: Update macOS `detect_child` to populate `pid`**

In `src/pty/foreground.rs`, the macOS `detect_child` (lines 141-175). The `ps` command currently uses `-eo ppid=,command=`. Change to `-eo ppid=,pid=,command=` and parse the PID:

```rust
    pub fn detect_child(shell_pid: u32) -> Option<ForegroundProcess> {
        let out = std::process::Command::new("ps")
            .args(["-eo", "ppid=,pid=,command="])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let shell_pid_str = shell_pid.to_string();
        for line in text.lines() {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix(&shell_pid_str) {
                if rest.starts_with(' ') {
                    let rest = rest.trim_start();
                    let (pid_str, cmd) = rest.split_once(' ')?;
                    let child_pid: u32 = pid_str.trim().parse().ok()?;
                    let cmd = cmd.trim();
                    if cmd.is_empty() {
                        continue;
                    }
                    let args: Vec<String> = cmd.split_whitespace().map(str::to_string).collect();
                    let name = args
                        .first()?
                        .rsplit('/')
                        .next()
                        .unwrap_or(&args[0])
                        .to_string();
                    return Some(ForegroundProcess {
                        name,
                        cmdline: args,
                        pid: Some(child_pid),
                    });
                }
            }
        }
        None
    }
```

- [ ] **Step 7: Update fallback platform to populate `pid: None`**

The fallback already returns `None`, so no change needed — but verify this compiles.

- [ ] **Step 8: Run all tests**

Run: `cargo test --lib -- --nocapture`
Expected: All tests PASS (including the two new ones).

- [ ] **Step 9: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: Clean.

- [ ] **Step 10: Commit**

```bash
git add src/pty/foreground.rs
git commit -m "Add pid field to ForegroundProcess for Claude session lookup"
```

---

### Task 3: Extend ForegroundWorker with Claude Session Cache

**Files:**
- Modify: `src/pty/foreground_worker.rs`

- [ ] **Step 1: Write failing test for `get_claude_session_id`**

Add at the bottom of `src/pty/foreground_worker.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_sessions_empty_by_default() {
        let worker = ForegroundWorker::spawn();
        assert!(worker.get_claude_session_id(1).is_none());
    }

    #[test]
    fn test_claude_sessions_set_and_get() {
        let worker = ForegroundWorker::spawn();
        worker.claude_sessions.lock().insert(1, "abc-123".to_string());
        assert_eq!(
            worker.get_claude_session_id(1),
            Some("abc-123".to_string())
        );
    }

    #[test]
    fn test_set_sessions_prunes_claude_cache() {
        let worker = ForegroundWorker::spawn();
        worker.claude_sessions.lock().insert(1, "abc-123".to_string());
        worker.claude_sessions.lock().insert(2, "def-456".to_string());
        // Only keep session 1
        worker.set_sessions(vec![(1, 100)]);
        assert_eq!(
            worker.get_claude_session_id(1),
            Some("abc-123".to_string())
        );
        assert!(worker.get_claude_session_id(2).is_none());
    }

    #[test]
    fn test_claude_sessions_never_evicts_on_poll() {
        let worker = ForegroundWorker::spawn();
        // Simulate: claude detected, session ID captured
        worker.claude_sessions.lock().insert(5, "xyz-789".to_string());
        // Even though foreground might change, the cache entry stays
        assert_eq!(
            worker.get_claude_session_id(5),
            Some("xyz-789".to_string())
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib foreground_worker::tests -- --nocapture`
Expected: FAIL — `claude_sessions` field and `get_claude_session_id` don't exist.

- [ ] **Step 3: Add `claude_sessions` field and `get_claude_session_id` method**

In `src/pty/foreground_worker.rs`:

Add import at the top:

```rust
use crate::app::claude_session::{is_claude_process, lookup_claude_session_id};
```

Add field to `ForegroundWorker` struct (after `pids` field, line 19):

```rust
    claude_sessions: Arc<Mutex<HashMap<u32, String>>>,
```

In `spawn()`, after `let pids` (line 28), add:

```rust
        let claude_sessions: Arc<Mutex<HashMap<u32, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
```

Add a clone for the background thread (after `let alive_bg`, line 33):

```rust
        let claude_bg = claude_sessions.clone();
```

In the poll loop, after `cache_bg.lock().insert(sid, result);` (line 45), add the claude detection:

```rust
                    if let Some(ref proc) = *cache_bg.lock().get(&sid).unwrap_or(&None) {
                        if is_claude_process(&proc.name, &proc.cmdline) {
                            if let Some(pid) = proc.pid {
                                if let Some(session_id) = lookup_claude_session_id(pid) {
                                    claude_bg.lock().insert(sid, session_id);
                                }
                            }
                        }
                    }
```

Wait — this re-locks the cache. Better to capture the result before inserting:

Replace the poll loop body (lines 40-46) with:

```rust
                    for (sid, shell_pid) in snapshot {
                        if shell_pid == u32::MAX {
                            continue;
                        }
                        let result = detect_child(shell_pid);
                        if let Some(ref proc) = result {
                            if is_claude_process(&proc.name, &proc.cmdline) {
                                if let Some(pid) = proc.pid {
                                    if let Some(session_id) = lookup_claude_session_id(pid) {
                                        claude_bg.lock().insert(sid, session_id);
                                    }
                                }
                            }
                        }
                        cache_bg.lock().insert(sid, result);
                    }
```

Add the field in the constructor return (after `pids`, line 59):

```rust
            claude_sessions,
```

In `set_sessions()` (line 67-72), add claude_sessions pruning after the cache pruning:

```rust
        self.claude_sessions
            .lock()
            .retain(|sid, _| active_ids.contains(sid));
```

Add the new getter method (after `get`, line 75-77):

```rust
    pub fn get_claude_session_id(&self, session_id: u32) -> Option<String> {
        self.claude_sessions.lock().get(&session_id).cloned()
    }
```

- [ ] **Step 4: Make `claude_session` functions accessible from `pty` module**

The `foreground_worker.rs` is in `src/pty/` but `claude_session.rs` is in `src/app/`. The worker needs to access `is_claude_process` and `lookup_claude_session_id`. Since these are pure utility functions, change their visibility:

In `src/app/claude_session.rs`, change both functions from `pub(super)` to `pub(crate)`:

```rust
pub(crate) fn is_claude_process(name: &str, cmdline: &[String]) -> bool {
```

```rust
pub(crate) fn lookup_claude_session_id(pid: u32) -> Option<String> {
```

In `src/app.rs`, change the module declaration:

```rust
pub(crate) mod claude_session;
```

In `src/pty/foreground_worker.rs`, the import becomes:

```rust
use crate::app::claude_session::{is_claude_process, lookup_claude_session_id};
```

- [ ] **Step 5: Run all tests**

Run: `cargo test --lib -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 6: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: Clean.

- [ ] **Step 7: Commit**

```bash
git add src/pty/foreground_worker.rs src/app/claude_session.rs src/app.rs
git commit -m "Add Claude session ID cache to ForegroundWorker"
```

---

### Task 4: Add Persistence Fields

**Files:**
- Modify: `src/app/pane.rs`
- Modify: `src/app/persistence.rs`

- [ ] **Step 1: Write failing test for `SavedSession` with `claude_session_id`**

Add to the existing `tests` module in `src/app/persistence.rs`:

```rust
    #[test]
    fn test_saved_session_with_claude_session_id_roundtrip() {
        let original = SavedSession {
            cwd: PathBuf::from("/home/user/project"),
            command: Some("claude".into()),
            title: Some("Claude session".into()),
            scrollback_file: None,
            claude_session_id: Some("abc-def-123-456".into()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: SavedSession = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.claude_session_id,
            Some("abc-def-123-456".to_string())
        );
    }

    #[test]
    fn test_saved_session_without_claude_session_id_defaults_none() {
        let json = r#"{"cwd": "/home/user"}"#;
        let s: SavedSession = serde_json::from_str(json).unwrap();
        assert_eq!(s.claude_session_id, None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib persistence::tests -- --nocapture`
Expected: FAIL — `claude_session_id` field doesn't exist.

- [ ] **Step 3: Add `claude_session_id` to `SavedSession`**

In `src/app/persistence.rs`, add to the `SavedSession` struct (after `scrollback_file` field, line 15):

```rust
    #[serde(default)]
    pub(super) claude_session_id: Option<String>,
```

- [ ] **Step 4: Update existing test that constructs `SavedSession`**

In the existing `test_saved_session_roundtrip` test and `test_app_session_roundtrip` test, add the new field to all `SavedSession` constructors:

```rust
    claude_session_id: None,
```

- [ ] **Step 5: Add `claude_session_id` to `SessionEntry`**

In `src/app/pane.rs`, add to the `SessionEntry` struct (after `restore_title` field, line 106):

```rust
    pub(super) claude_session_id: Option<String>,
```

- [ ] **Step 6: Update all `SessionEntry` construction sites**

There are two places in `src/app/state.rs` where `SessionEntry` is constructed (around lines 354-372 and lines 1005-1024). Add to both:

```rust
    claude_session_id: None,
```

- [ ] **Step 7: Run all tests**

Run: `cargo test --lib -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 8: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: Clean.

- [ ] **Step 9: Commit**

```bash
git add src/app/pane.rs src/app/persistence.rs src/app/state.rs
git commit -m "Add claude_session_id field to SessionEntry and SavedSession"
```

---

### Task 5: Wire Save, Restore, and Eager Sync

**Files:**
- Modify: `src/app/state.rs` (save_session, restore_session)
- Modify: `src/app.rs` (eager sync in update loop)

- [ ] **Step 1: Update `save_session()` to persist claude session ID**

In `src/app/state.rs`, in the `save_session()` method, in the `.map(|e| { ... })` closure that builds `SavedSession` (around line 1170-1214). After `let scrollback_file = ...` (line 1207), add:

```rust
                let claude_session_id = self
                    .workers
                    .foreground_worker
                    .get_claude_session_id(e.id)
                    .or_else(|| e.claude_session_id.clone());
```

Then add the field to the `SavedSession` constructor (after `scrollback_file`, line 1213):

```rust
                    claude_session_id,
```

- [ ] **Step 2: Update `restore_session()` — active session (eager spawn)**

In `src/app/state.rs`, in `restore_session()`, after the existing `entry.restore_title = ...` line (line 1352), add:

```rust
                        if let Some(ref claude_id) = s.claude_session_id {
                            entry.pending_command =
                                Some(format!("claude --resume \"{}\"", claude_id));
                            entry.claude_session_id = Some(claude_id.clone());
                        }
```

This overrides any previously set `pending_command` from the normal command replay. The Claude resume takes priority.

- [ ] **Step 3: Update `restore_session()` — deferred terminals (non-active sessions)**

In `src/app/state.rs`, in `restore_session()`, the block that builds `PaneContent::DeferredTerminal` for non-active `SavedPaneContent::Terminal` variants (around lines 1383-1394). Replace the `pending_command` computation:

Current code:
```rust
let pending_command = if scrollback_file.is_some() {
    None
} else {
    saved.and_then(|s| s.command.clone())
};
```

New code:
```rust
let claude_session_id = saved.and_then(|s| s.claude_session_id.clone());
let pending_command = if claude_session_id.is_some() {
    claude_session_id
        .as_ref()
        .map(|id| format!("claude --resume \"{}\"", id))
} else if scrollback_file.is_some() {
    None
} else {
    saved.and_then(|s| s.command.clone())
};
```

- [ ] **Step 4: Add eager sync in `App::update()`**

In `src/app.rs`, after the deferred terminal materialization block (around line 3641, after the closing `}`), add:

```rust
        // 5c. Sync Claude session IDs from foreground worker to session entries.
        // Triggers save_session() on first capture so the ID is on disk even if
        // the OS kills the app before the next organic save.
        {
            let mut new_claude_capture = false;
            for entry in &mut self.session_state.sessions {
                if let Some(id) = self.workers.foreground_worker.get_claude_session_id(entry.id) {
                    if entry.claude_session_id.as_ref() != Some(&id) {
                        entry.claude_session_id = Some(id);
                        new_claude_capture = true;
                    }
                }
            }
            if new_claude_capture {
                self.save_session();
            }
        }
```

- [ ] **Step 5: Run full test suite**

Run: `cargo test -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 6: Run cargo fmt and clippy**

Run: `cargo fmt && cargo clippy --all-targets -- -D warnings`
Expected: Clean.

- [ ] **Step 7: Commit**

```bash
git add src/app/state.rs src/app.rs
git commit -m "Wire Claude session ID save, restore, and eager sync"
```

---

### Task 6: Add `tempfile` Dev Dependency (if not present)

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Check if `tempfile` is already a dependency**

Run: `cargo tree -i tempfile 2>&1 || echo "not found"`

- [ ] **Step 2: Add `tempfile` as dev-dependency if missing**

In `Cargo.toml`, under `[dev-dependencies]` (create section if needed):

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo test --lib claude_session -- --nocapture`
Expected: All tests PASS (the `lookup_claude_session_id_in` tests use `tempfile::tempdir()`).

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "Add tempfile dev-dependency for Claude session tests"
```

---

### Task 7: Add `dirs` Dependency (if not present)

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Check if `dirs` is already a dependency**

Run: `cargo tree -i dirs 2>&1 || echo "not found"`

The `claude_sessions_dir()` function uses `dirs::home_dir()`. If `dirs` is not a dependency, add it. If the project already uses another method for getting the home directory (e.g., `std::env::var("HOME")` or `std::env::var("USERPROFILE")`), use that approach instead and skip this task.

- [ ] **Step 2: Add `dirs` if needed, or adapt `claude_sessions_dir` to use existing home-dir pattern**

Check how the existing codebase finds the home directory (look for `home_dir`, `HOME`, `USERPROFILE` usage in `src/`). Follow the same pattern in `claude_sessions_dir()`.

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test --lib claude_session -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 4: Commit if any changes were made**

```bash
git add Cargo.toml Cargo.lock src/app/claude_session.rs
git commit -m "Wire claude_sessions_dir to project home-dir pattern"
```

---

### Task 8: End-to-End Manual Verification

- [ ] **Step 1: Build the app**

Run: `cargo build`
Expected: Clean build, no warnings.

- [ ] **Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests PASS (333 existing + ~18 new).

- [ ] **Step 3: Run the app and verify Claude session detection**

1. Run `cargo run`
2. Open a terminal pane
3. Run `claude` in the terminal
4. Wait ~1 second (two poll cycles)
5. Close the app gracefully
6. Check `session.json` in the data directory — verify `claude_session_id` field is populated with a UUID
7. Reopen the app — verify it runs `claude --resume "<uuid>"` in the restored pane

- [ ] **Step 4: Verify edge case — no Claude running**

1. Run `cargo run`
2. Open a terminal pane, run any non-Claude command (e.g., `ls`)
3. Close and reopen — verify normal restore (no `claude --resume`)
4. Check `session.json` — `claude_session_id` should be `null`

- [ ] **Step 5: Final commit if any fixups needed**

```bash
git add -A
git commit -m "Fix any issues found during manual verification"
```
