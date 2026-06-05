# Claude Code Session Resume — Design Spec

## Problem

When Terminal Studio restores a session that was running Claude Code CLI, the shell restarts but the Claude conversation is lost. The user must manually run `claude` again and loses their conversation context.

## Solution

Detect when Claude Code is the foreground process of a terminal pane, capture the exact session ID via Claude's `~/.claude/sessions/{pid}.json` file, persist it eagerly, and on restore auto-run `claude --resume "<id>"` to resume the exact conversation.

## Architecture

### Detection & Capture

**New module: `src/app/claude_session.rs`**

Two pure functions:

- `is_claude_process(name: &str, cmdline: &[String]) -> bool`
  - Matches: `claude`, `claude.exe`
  - Also matches: `node` / `node.exe` with `"claude"` in any arg (npm-installed Claude Code)
  - Case-insensitive on Windows

- `lookup_claude_session_id(pid: u32) -> Option<String>`
  - Reads `~/.claude/sessions/{pid}.json`
  - Parses JSON, extracts `"sessionId"` string field
  - Returns `None` on any error (file missing, parse failure, field missing)

**Extend `ForegroundProcess`** (in `src/pty/foreground.rs`):

Add `pid: Option<u32>` field. Populated from:
- **Windows:** `pe32.th32ProcessID` from the `CreateToolhelp32Snapshot` walk
- **Linux:** foreground process group ID from `/proc/{pid}/stat`
- **macOS:** PID parsed from `ps` output

**Extend `ForegroundWorker`** (in `src/pty/foreground_worker.rs`):

New field:
```rust
claude_sessions: Arc<Mutex<HashMap<u32, String>>>  // terminal session_id -> claude session UUID
```

Poll loop changes (runs every 500ms, already existing):
1. After detecting a foreground process for a terminal session
2. If `is_claude_process(&proc.name, &proc.cmdline)` and `proc.pid` is `Some(pid)`
3. Call `lookup_claude_session_id(pid)` (reads a ~200 byte JSON file — negligible I/O)
4. If found, insert into `claude_sessions` map
5. **Never evict** from this map — once captured, the session ID persists until the terminal session is removed

New methods:
- `get_claude_session_id(&self, session_id: u32) -> Option<String>` — reads from `claude_sessions` cache
- Cleanup in `set_sessions()`: prune `claude_sessions` entries for terminal sessions no longer in the active list

### Persistence

**Extend `SavedSession`** (in `src/app/persistence.rs`):
```rust
#[serde(default)]
pub(super) claude_session_id: Option<String>,
```

**Extend `SessionEntry`** (in `src/app/pane.rs`):
```rust
pub(super) claude_session_id: Option<String>,
```

**In `save_session()`** (in `src/app/state.rs`):
- For each session, check `foreground_worker.get_claude_session_id(entry.id)`
- If the worker has a value, use it; otherwise fall back to `entry.claude_session_id`
- Write to `SavedSession.claude_session_id`

**Eager save on capture:** In `App::update()` (in `src/app.rs`), after the existing foreground process reads, iterate terminal sessions. For each, call `foreground_worker.get_claude_session_id(entry.id)`. If the worker returns `Some(id)` and `entry.claude_session_id` is `None` or differs, update the entry and call `save_session()`. This ensures the ID is on disk even if the OS kills the app before the next organic save.

### Restore

**In `restore_session()`** (in `src/app/state.rs`):
- When a `SavedSession` has `claude_session_id: Some(id)`:
  - Set `pending_command = Some(format!("claude --resume \"{}\"", id))`
  - Reuses the existing command injection mechanism — no new restore codepath

**Restore sequence:**
1. Shell starts in saved CWD (existing)
2. Scrollback injected showing previous conversation (existing)
3. `pending_command` fires `claude --resume "<uuid>"` after shell init
4. Claude Code resumes the exact conversation with full context

## Edge Cases

| Case | Behavior |
|------|----------|
| Claude exits before 500ms detection | Near-impossible for interactive sessions; session ID not captured, shell restores normally |
| Multiple terminals, same CWD, both running Claude | PID-based lookup ensures each pane captures the correct session ID |
| Stale session ID (30-day cleanup) | `claude --resume` fails gracefully — shows error, drops to normal interactive mode |
| Claude not installed | `is_claude_process` never matches; zero impact |
| Session file not yet written | `lookup_claude_session_id` returns None; next poll retries in 500ms |
| OS kills app | Eager save on capture ensures the ID is on disk |
| Scrollback + resume | Old conversation visible as scrollback above; new resume output appears below |
| `DeferredTerminal` panes | `claude_session_id` carried through to deferred pane data; fires on lazy spawn |

## Files Changed

| File | Change |
|------|--------|
| `src/app/claude_session.rs` | **New** — `is_claude_process()`, `lookup_claude_session_id()` |
| `src/pty/foreground.rs` | Add `pid: Option<u32>` to `ForegroundProcess` |
| `src/pty/foreground_worker.rs` | Add `claude_sessions` cache, `get_claude_session_id()`, cleanup in `set_sessions()` |
| `src/app/pane.rs` | Add `claude_session_id: Option<String>` to `SessionEntry` |
| `src/app/persistence.rs` | Add `claude_session_id: Option<String>` to `SavedSession` and `SavedPaneContent::DeferredTerminal` |
| `src/app/state.rs` | Update `save_session()` to persist claude ID; update `restore_session()` to set `pending_command`; add eager save trigger in update loop |

## Testing

- `claude_session.rs`: Unit tests for `is_claude_process` (various name/cmdline combos) and `lookup_claude_session_id` (valid JSON, missing file, malformed JSON, missing field)
- `foreground_worker.rs`: Test that `claude_sessions` cache populates and never evicts, and that `set_sessions()` prunes stale entries
- `persistence.rs`: Test round-trip serialization of `SavedSession` with and without `claude_session_id`
- `state.rs`: Test that `restore_session()` sets `pending_command` when `claude_session_id` is present
