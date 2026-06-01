# Terminal Studio — Full Codebase Bug Audit

**Date:** 2026-06-01
**Scope:** 18,000 lines across 65 source files
**Result:** ~71 unique actionable bugs

---

## CRITICAL — Data Loss / Crash Risk (7 bugs — all fixed)

### ~~C1: Silent data wipe on corrupt JSON~~ FIXED
- **Files:** `src/workspace.rs`, `src/app/settings.rs`, `src/app/state.rs`
- **Bug:** `unwrap_or_default()` on parse failure replaces corrupt file with empty data. The next `save()` call overwrites the file, permanently destroying all workspace/settings/session data.
- **Resolution:** Added `util::safe_json_load()` helper that logs a warning and creates a `.json.bak` backup on parse failure. All 4 JSON load sites (workspaces, notes, settings, windows, session) now use it.

### ~~C2: Atomic write gap on Windows~~ FIXED
- **File:** `src/util.rs`
- **Bug:** On Windows, `atomic_write` does `remove_file(path)` then `rename(tmp, path)`. A crash between these two calls leaves the target deleted with only the temp file remaining.
- **Resolution:** Windows path now uses `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH` — a single atomic OS call. Unix path unchanged (rename is already atomic on POSIX).

### ~~C3: 256-color cube completely wrong~~ FIXED
- **File:** `src/renderer/terminal_pass.rs`
- **Bug:** Used `level * 51` producing wrong color values for palette indices 16-231.
- **Resolution:** Replaced with xterm standard lookup table `CUBE_STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255]`. Updated tests to assert correct values.

### ~~C4: Double-click on non-ASCII text panics~~ FIXED
- **File:** `src/app.rs`
- **Bug:** Bounds check used `line_text.len()` (byte length) but indexed `Vec<char>` by column position.
- **Resolution:** Moved `Vec<char>` conversion before the bounds check, now uses `chars.len()` for the comparison.

### ~~C5: Scrollback copy returns empty text~~ FIXED
- **File:** `src/app/state.rs`
- **Bug:** `extract_selected_text` filtered out `grid_line < 0`, but negative grid lines are valid scrollback rows.
- **Resolution:** Changed bounds check to `grid_line < -(grid.history_size() as i32) || grid_line >= term_rows as i32`.

### ~~C6: Zombie processes on Unix~~ FIXED
- **File:** `src/pty/mod.rs`
- **Bug:** `Child` handle was immediately dropped after `spawn_command` without calling `wait()`.
- **Resolution:** Spawns a dedicated `pty-reaper-N` thread that calls `child.wait()`, preventing zombie accumulation.

### ~~C7: Self-update always fails on Windows~~ FIXED
- **File:** `src/updater.rs`
- **Bug:** `fs::rename(current_exe, backup_path)` fails on Windows because the OS locks the running executable.
- **Resolution:** Windows path now uses the `self_replace` crate which handles locked-exe replacement. Unix path kept the existing rename-with-backup approach. `backup_path_for` is now Unix-only.

---

## HIGH — Feature Broken / Incorrect Behavior (19 bugs)

### H1: Command palette keyboard navigation broken
- **File:** `src/app.rs:274-284`
- **Bug:** `show_command_palette` is missing from `any_overlay` in `raw_input_hook`. Arrow keys and Escape are stripped from raw input before egui processes them. Up/Down navigation and Escape-to-close don't work.
- **Fix:** Add `|| self.show_command_palette` to the `any_overlay` expression.

### H2: Ctrl+F never reaches terminal apps
- **Files:** `src/app.rs:2000-2001`, `src/app/input.rs:14`
- **Bug:** `Ctrl+F` is consumed globally for `SearchTerminal` before PTY input routing. Terminal apps like `less`, `vim`, `nano`, `man` that use Ctrl+F for forward-search/page-down cannot receive it.
- **Fix:** Only intercept when no terminal application is actively requesting input, or use `Ctrl+Shift+F` for the app's search.

### H3: Mouse drag/motion events never forwarded to PTY
- **File:** `src/app.rs:2456-2468`
- **Bug:** `MOUSE_DRAG` (?1002h) and `MOUSE_MOTION` (?1003h) modes are detected but no `PointerMoved` handler sends motion reports. Mouse-aware apps (vim mouse mode, tmux pane resize, htop) are broken.
- **Fix:** Add a `PointerMoved` handler that sends mouse motion escape sequences (SGR/X11 format) to the PTY when in drag/motion mode.

### H4: Alt+Arrow swallowed in single-pane layout
- **File:** `src/app.rs:2219-2259`
- **Bug:** When only one pane exists, Alt+Arrow is consumed for focus navigation (which does nothing with one pane) instead of being sent to the PTY as a word-jump sequence (`\x1b[1;3D` etc.).
- **Fix:** When `leaves.len() <= 1`, fall through to `key_to_pty_bytes` to send Alt+Arrow to the PTY.

### H5: Multiple dialogs missing from `modal_open`
- **File:** `src/app.rs:1973-1978`
- **Bug:** `show_close_all_confirm`, `show_commit_dialog`, `show_push_dialog`, `show_stage_all_confirm`, `show_quit_confirm`, `open_folder_dialog` are not checked in `modal_open`. Keystrokes leak to the PTY while these dialogs are open. Pressing 'y' to confirm close-all also sends 'y' to the terminal.
- **Fix:** Add all dialog/overlay booleans to the `modal_open` check.

### H6: `close_all` orphans split trees
- **File:** `src/app/ui/dialogs.rs:1069-1074`
- **Bug:** `pane_trees.remove(pid)` uses leaf IDs but trees are keyed by root ID. For panes inside a split tree, the remove is a no-op, leaving orphaned trees with dangling pane references.
- **Fix:** Collect all root tree keys that contain any of the panes being closed and remove/prune those trees.

### H7: `process_quit_pane` on root orphans siblings
- **File:** `src/app/ui/left_panel/context_menu.rs:94-158`
- **Bug:** Closing a root pane from the sidebar removes the tree entry but leaves sibling panes alive with running PTY sessions. Sessions leak indefinitely.
- **Fix:** When removing a root tree with multiple leaves, either remove the single leaf (like `close_split_pane`) or remove all leaves and kill all their sessions.

### H8: `zoomed_pane_id` not cleared on sidebar close
- **File:** `src/app/ui/left_panel/context_menu.rs`
- **Bug:** Closing a zoomed pane from the sidebar leaves `zoomed_pane_id` pointing to a deleted pane. The content area renders a synthetic leaf for the nonexistent pane → blank screen.
- **Fix:** Add `if self.zoomed_pane_id == Some(qpid) { self.zoomed_pane_id = None; }` in `process_quit_pane`.

### H9: `active_term_geo` stale after tab switch
- **File:** `src/app/ui/pane_renderer.rs:292-300`
- **Bug:** Switching from a terminal tab to a file editor tab leaves old geometry set. Mouse events in the editor area may still match the old terminal geometry and send data to the previous session's PTY.
- **Fix:** Clear `self.active_term_geo = None` at the start of `render_pane_content()`.

### H10: Renamed files can't be staged/diffed
- **File:** `src/git/parser.rs:56-59`
- **Bug:** For renamed files, git outputs `R old_name -> new_name`. The parser stores the entire string as the path. `git add -- "old.rs -> new.rs"` fails because no such file exists.
- **Fix:** Split on ` -> ` and use the new name (destination) as the path. Store both old and new for display.

### H11: Non-ASCII filenames break git operations
- **File:** `src/git/parser.rs:28-89`
- **Bug:** Git's `--porcelain` output quotes filenames with C-style escaping (`"\303\251file"`). The parser doesn't unquote these, so operations fail for international filenames.
- **Fix:** Detect quoted paths (start/end with `"`) and unescape them, converting octal escapes to UTF-8.

### H12: Subdirectory changes not detected by watcher
- **File:** `src/app/watcher.rs:87`
- **Bug:** `RecursiveMode::NonRecursive` means file edits in `src/`, `tests/`, etc. never trigger git status refresh or file browser updates. Only direct CWD changes trigger events.
- **Fix:** Use `RecursiveMode::Recursive` for the main directory, or add explicit watches for known subdirectories.

### H13: Wide characters render at single-cell width
- **File:** `src/renderer/terminal_pass.rs:193-201,273-290`
- **Bug:** CJK/emoji characters are rendered at normal cell width. The background only covers one cell. The spacer cell next to it shows empty space. Visual corruption for any wide character content.
- **Fix:** When `c.wide` is true, render the character with `2 * cell_width` and paint the background spanning two cells.

### H14: Cursor colors use wrong alpha API
- **File:** `src/theme.rs:256-264`
- **Bug:** `from_rgba_premultiplied(255, 255, 255, 200)` — RGB channels (255) exceed alpha (200), which is impossible for premultiplied data. egui clamps to `(200, 200, 200, 200)`, producing wrong cursor appearance. Compare with `scrollbar_color` on line 268 which correctly uses `from_rgba_unmultiplied`.
- **Fix:** Change all cursor color lines to use `Color32::from_rgba_unmultiplied(...)`.

### H15: Tab drag-reorder broken when tab bar is scrolled
- **File:** `src/app/ui/tab_bar.rs:440-465`
- **Bug:** Target index calculation uses screen coordinates without accounting for the `ScrollArea` scroll offset. When the tab bar is scrolled (many tabs), drops land in the wrong position.
- **Fix:** Track scroll offset from `ScrollArea` output and subtract it before computing the target index.

### H16: URL click detection wrong with non-ASCII content
- **File:** `src/url_detector.rs:22-29`
- **Bug:** `m.start()` and `m.end()` are byte offsets but `start_col`/`end_col` are used as column indices. For lines with CJK/emoji before the URL, byte offset > character offset → click misses the URL.
- **Fix:** Convert to character offsets: `start_col: text[..m.start()].chars().count()`.

### H17: Markdown path click wrong with non-ASCII content
- **File:** `src/md_detector.rs:25-26`
- **Bug:** Same byte-vs-char offset bug as H16.
- **Fix:** Same conversion to character offsets.

### H18: Single-threaded git worker stalls during push
- **File:** `src/app/git_worker.rs:83-264`
- **Bug:** All git operations share one thread. A slow `git push` blocks `git status` refreshes. The git panel appears frozen during push operations.
- **Fix:** Use separate threads for long-running ops (push, commit) vs. fast queries (status, diff), or use a thread pool.

### H19: Reader thread never checks `alive` flag
- **File:** `src/pty/reader.rs:148-196`
- **Bug:** The reader loop does `loop { reader.read(...) }` and never checks `alive.load()`. When a session is removed and `alive` is set to false, the reader continues processing, consuming CPU, holding the session Arc alive, and requesting repaints for a dead session.
- **Fix:** Check `alive.load(Ordering::Relaxed)` at the top of each loop iteration, or use a non-blocking read with a timeout.

---

## MEDIUM — Correctness / UX Issues (25 bugs)

### M1: Editor text writeback uses stale indices after pane removal
- **File:** `src/app.rs:3289-3308`
- **Bug:** `editor_texts` built from original panes list; after `panes.retain()` in `close_pane_id` handler, indices shift. Pane ID guard mitigates out-of-bounds but edits to shifted panes are silently discarded.
- **Fix:** Use `pane_id` to find the pane via `.iter_mut().find()` instead of index alignment.

### M2: `navigate_to_workspace` uses `pane_id: 0` sentinel
- **File:** `src/app/state.rs:810-820`
- **Bug:** `unwrap_or(0)` for missing pane. If pane 0 exists but belongs to a different workspace, the user navigates to the wrong pane.
- **Fix:** Use `Option<u32>` for the pane_id field.

### M3: Shortcut help shows hardcoded defaults, ignores user customizations
- **File:** `src/shortcuts.rs:451-531`
- **Bug:** `groups()` constructs entries with hardcoded `Shortcut::cs(...)` values rather than reading from `self.bindings`.
- **Fix:** Build groups dynamically from `self.bindings`.

### M4: File I/O on every frame while command palette is open
- **File:** `src/app/ui/command_palette.rs:356`
- **Bug:** `all_palette_actions()` calls `ShortcutRegistry::new()` which does `fs::read_to_string()` from disk on every frame.
- **Fix:** Cache the palette entries or pass the existing `ShortcutRegistry` from `App`.

### M5: Git parser strips leading/trailing whitespace from filenames
- **File:** `src/git/parser.rs:37`
- **Bug:** `line[3..].trim()` mangles files with leading/trailing spaces (legal on Linux).
- **Fix:** Don't trim. Git porcelain format has the path starting at byte 3 with no padding.

### M6: Git parser may panic on multi-byte UTF-8 at positions 0-2
- **File:** `src/git/parser.rs:32-37`
- **Bug:** `line[3..]` is a byte-offset slice. Multi-byte UTF-8 in the first 3 bytes → "byte index is not a char boundary" panic.
- **Fix:** Use `line.get(3..)` for safety.

### M7: Concurrent git operation results overwritten
- **File:** `src/app/git_worker.rs:49-52`
- **Bug:** `commit_result` and `push_result` are single `Option` slots. Rapid operations overwrite the first result before the UI reads it.
- **Fix:** Use a `Vec` or channel.

### M8: No unpushed commits indicator for branches without upstream
- **File:** `src/app/git_worker.rs:160-177`
- **Bug:** `git log @{upstream}..HEAD` fails silently for new branches. Users see no unpushed indicator and may think work is pushed.
- **Fix:** Check for upstream first; if none, show "No upstream" or compare against `origin/main`.

### M9: `.gitignore` write is not atomic
- **File:** `src/app/git_worker.rs:240-261`
- **Bug:** Uses `std::fs::write` directly instead of `atomic_write`. Crash during write → corrupted `.gitignore`.
- **Fix:** Use `util::atomic_write`.

### M10: `save_session` maps missing session ID to index 0
- **File:** `src/app/state.rs:1065`
- **Bug:** `session_id_to_index.get(sid).copied().unwrap_or(0)` — if session was closed between iteration passes, pane reconnects to wrong session on restore.
- **Fix:** Use `Option` and skip the pane if session not found.

### M11: Concurrent saves race on same temp filename
- **File:** `src/workspace.rs:42-51`
- **Bug:** Temp file names are deterministic (`.workspaces.json.tmp`). Two rapid saves to the same store race on the same temp file.
- **Fix:** Use unique temp file names (PID + timestamp) or `tempfile::NamedTempFile`.

### M12: Vertical split dividers not pixel-snapped
- **File:** `src/pane_tree.rs:226-236`
- **Bug:** Horizontal splits round the split position (`.round()`), but vertical splits don't. Causes blurry horizontal dividers.
- **Fix:** Add `.round()` to the vertical case.

### M13: Negative-dimension panes possible with very small windows
- **File:** `src/pane_tree.rs:209-237`
- **Bug:** `split_rect` subtracts `half=3.0` from split position. If rect is <6px wide, resulting rects have negative dimensions.
- **Fix:** Clamp resulting rects or skip rendering below minimum viable size.

### M14: Selection end on wide char extends 1 cell too far
- **File:** `src/renderer/terminal_pass.rs:397-414`
- **Bug:** `snap_end` adds 1 to `ec` for wide chars, then the end_col calculation adds another 1 for the selection end row.
- **Fix:** Clamp `end_col` to `cols as u16` after snap adjustment.

### M15: Scrollbar thumb escapes bounds in tiny panes
- **File:** `src/renderer/terminal_pass.rs:471-477`
- **Bug:** `thumb_h` clamped to min 20px, but if `rect.height() < 20`, `track_h` goes negative → thumb above rect.
- **Fix:** Clamp `thumb_h` to at most `rect.height()` and ensure `track_h >= 0`.

### M16: Bold text blurry (0.5px double-draw)
- **File:** `src/renderer/terminal_pass.rs:281-289`
- **Bug:** Bold simulated by drawing the character twice at 0.5px offset. Creates blur/ghosting rather than genuine bold weight.
- **Fix:** Use a bold font variant if available, or increase offset to 1.0.

### M17: Selection highlight shifts during scroll-while-selecting
- **File:** `src/renderer/terminal_pass.rs:392-432`
- **Bug:** Selection coordinates are in screen space. If `display_offset` changes during selection (user scrolls while selecting), highlight no longer matches actual selected text.

### M18: Markdown table cells get full frame border instead of column separators
- **File:** `src/app/markdown.rs:279-284`
- **Bug:** `Frame::stroke` applies to all four sides, creating doubled borders between cells.
- **Fix:** Use `painter.line_segment()` to draw vertical separators between columns instead.

### M19: `restart_app()` bypasses all Drop/save logic
- **File:** `src/updater.rs:363-369`
- **Bug:** Calls `std::process::exit(0)` without saving session state or cleaning up resources.
- **Fix:** Trigger graceful quit flow, or call `save_session()` before exit.

### M20: macOS title text not clipped
- **File:** `src/app/ui/titlebar.rs:367-406`
- **Bug:** Centered title text has no clip rect on macOS. Long workspace names overlap traffic lights and right-side buttons.
- **Fix:** Add a clip rect similar to the Windows/Linux path.

### M21: Search highlights wrong position for wide characters
- **File:** `src/search.rs:61-76`
- **Bug:** Character count in `line_text` doesn't match column count in terminal grid because wide chars occupy 2 columns but count as 1 character. Highlights shift left by one column per wide char before the match.
- **Fix:** Track mapping from character index to grid column index, accounting for wide characters.

### M22: Open Folder permanently changes default shell preference
- **File:** `src/app/ui/dialogs.rs:1614-1617`
- **Bug:** Opening a folder with a non-default shell unconditionally persists it as the new default.
- **Fix:** Only persist if the user explicitly chose a different shell, or remove this side effect.

### M23: Resize holds write lock during potentially-blocking ConPTY resize
- **File:** `src/app.rs:3389-3392`
- **Bug:** `session.write()` lock held during `SessionManager::resize()`, which can block on Windows ConPTY. Reader thread stalls during resize.
- **Fix:** Call `SessionManager::resize(&entry.master, cols, rows)` before acquiring the write lock.

### M24: Foreground detection returns arbitrary child, not actual foreground process
- **Files:** `src/pty/foreground.rs:46-76` (Windows), `src/pty/foreground.rs:111-131` (Linux)
- **Bug:** Returns the first child found in snapshot/proc enumeration. With background jobs, may return the wrong process name.
- **Fix:** On Linux, read `/proc/<shell_pid>/stat` field 8 (`tpgid`) to identify the foreground process group. On Windows, check console attachment or creation time.

### M25: Clipboard access from reader thread may fail silently on Linux/Wayland
- **File:** `src/pty/reader.rs:170-172`
- **Bug:** `arboard::Clipboard::new()` called from `pty-reader-N` thread. On Wayland, clipboard access from non-main threads fails silently.
- **Fix:** Send clipboard text to the UI thread via channel and perform the clipboard operation there.

---

## LOW — Performance / Minor Issues (20+ bugs)

### L1: Session-keyed maps not cleaned on session removal
- **Files:** `src/app.rs` (various locations)
- **Bug:** `command_start_times`, `completed_badges`, `resize_debounce`, `scroll_accum` not cleaned when session removed. Slow memory leak.

### L2: `SubdirCache` has unbounded growth
- **File:** `src/app/file_browser.rs:13-30`
- **Bug:** HashMap only grows, TTL controls re-reads not eviction. Memory leak over long sessions.

### L3: File browser hides all dotfiles
- **File:** `src/app/file_browser.rs:66`
- **Bug:** `name.starts_with('.')` unconditionally skips all dot-prefixed entries including `.github/`, `.vscode/`, `.gitignore`, `.editorconfig`.

### L4: Symlinked directories shown as files
- **File:** `src/app/file_browser.rs:69`
- **Bug:** `DirEntry::file_type().is_dir()` returns false for symlinks to directories. Users cannot expand symlinked directories.
- **Fix:** Use `std::fs::metadata(e.path())` which follows symlinks.

### L5: Foreground worker thread not joined on drop
- **File:** `src/pty/foreground_worker.rs:71-75`
- **Bug:** Sets `alive = false` but no join. Thread continues for up to 1 second after drop.

### L6: Shell integration files rewritten every Zsh spawn
- **File:** `src/pty/shell_integration.rs:31-53`
- **Bug:** `fs::create_dir_all()` and `fs::write()` for 4 files every time a Zsh shell is spawned.

### L7: Watcher `is_dir()` syscall on every frame for unwatched CWDs
- **File:** `src/app/watcher.rs:49-54`
- **Bug:** For CWDs not yet in `self.watched`, `p.is_dir()` is called on every sync. Blocks UI on slow/network filesystems.

### L8: Download progress jumps 0% → 100%
- **File:** `src/updater.rs:224-246`
- **Bug:** `resp.bytes()?.to_vec()` downloads entire file at once. No intermediate progress.

### L9: Binary file detection incomplete
- **File:** `src/app/file_browser.rs:239-244`
- **Bug:** Only checks for null bytes. UTF-16, PDFs, and other binary formats without nulls show as garbled text.

### L10: Flash feedback too subtle
- **Files:** `src/app/feedback.rs:56`, `src/theme.rs:497,508`
- **Bug:** `ALPHA_FLASH=60` (23% opacity) decaying over 150ms = ~9 frames. Functionally invisible on most displays.

### L11: Update-checker thread blocks shutdown
- **File:** `src/updater.rs:79-111`
- **Bug:** Blocking `recv()` with no alive flag. Mid-download, thread keeps process alive for up to 5 minutes.

### L12: Quick switcher dialog overflows on small windows
- **File:** `src/app/ui/dialogs.rs:85-90`
- **Bug:** `.max(400.0)` forces 400px width even if screen is narrower.

### L13: Force-push button label lags one frame
- **File:** `src/app/ui/dialogs.rs:1277`
- **Bug:** `push_force` captured before checkbox renders. Button shows previous frame's state.

### L14: `key_name` round-trip incomplete for custom bindings
- **File:** `src/shortcuts.rs:264-338`
- **Bug:** Keys not in the lookup table serialize as `"?"`. On reload, `key_from_name("?")` returns `None`, dropping the binding.

### L15: X11 mouse encoding silently truncates coordinates beyond 222
- **File:** `src/app/input.rs:211-215`
- **Bug:** Inherent protocol limitation, but no warning logged.

### L16: Foreground polling interval docs say 500ms, code does 1000ms
- **File:** `src/pty/foreground_worker.rs:46`

### L17: `request_if_stale` TOCTOU race between check and insert
- **File:** `src/app/workspace_git_worker.rs:74-87`

### L18: Git worker sends dummy `GitInfo(PathBuf::new())` on Drop
- **File:** `src/app/git_worker.rs:377-383`
- **Bug:** Shutdown sends a dummy job that runs git commands against empty path.

### L19: `pending_diff_panes` entries leak when panes closed before diff completes
- **File:** `src/app.rs:519-528`

### L20: `SysMonitor::drop()` does not join the thread
- **File:** `src/sys_monitor.rs:47-51`

### L21: `CopySelection` missing from command palette and help dialog
- **Files:** `src/app/ui/command_palette.rs:332-354`, `src/shortcuts.rs:451-531`

### L22: Watcher does not handle watched directory deletion gracefully
- **File:** `src/app/watcher.rs:45-97`

### L23: `process_events()` reads markdown files that may still be written
- **File:** `src/app/watcher.rs:153-160`
- **Bug:** No debounce on file reads after modify event. May read partial content.

### L24: `syntect` color conversion uses wrong premultiplied alpha API
- **File:** `src/syntax.rs:28-30`
- **Bug:** Benign at alpha=255 (common case) but incorrect for transparent syntax styles.

### L25: Markdown underscore-based emphasis not supported
- **File:** `src/theme.rs:870-908`
- **Bug:** Only `*italic*` / `**bold**` handled, not `_italic_` / `__bold__`.

### L26: macOS foreground detection spawns 2 external processes per session per poll
- **File:** `src/pty/foreground.rs:152-178`
- **Bug:** `pgrep` + `ps` every 1000ms per session. With 10 tabs = 20 process spawns/second.

### L27: Worker struct constructed even when thread spawn fails
- **Files:** `src/file_search_worker.rs:55-114`, `src/search_worker.rs:57-133`
- **Bug:** Worker appears alive but is non-functional. Search silently does nothing.
