# Terminal Studio — Codebase Reference

GPU-accelerated terminal multiplexer in Rust using egui/eframe (wgpu renderer). Tiling terminal interface with workspaces, file browsing, git integration, and markdown preview.

## Build & Run

```
cargo build              # dev (opt-level 1)
cargo run
cargo build --release    # opt-level 3, LTO=true, codegen-units=1
cargo test               # all unit tests (1335 tests)
RUST_LOG=debug cargo run # enable debug logging
```

The Rust version is pinned in `rust-toolchain.toml` (1.99.1); local, CI and release builds all use it. Rust 1.100.0 couldn't compile `objc2 0.4.1`, which eframe 0.28 pulled in on macOS; eframe 0.37 no longer does. Move the pin in its own PR once build + clippy pass on the new version.

## Module Map

### Core

| File | Responsibility |
|------|---------------|
| `src/main.rs` | Entry point; eframe setup, 1280×800 viewport, decorations=false; calls `SingleInstanceGuard::try_acquire()`; tries renderers in order (wgpu, then glow) |
| `src/renderer_choice.rs` | Renderer order, `--renderer=` override, and `another_renderer_may_help()`: no glow retry when the window system or app creation failed (that would mask the real error with winit's "EventLoop can't be recreated") |
| `src/app.rs` | `App` struct; implements `eframe::App`: `logic()` (quit request, close confirmation, restart into an update, the saver's session saves — eframe also runs it while every window is minimized or hidden, when `ui()` is skipped) and `ui()` (everything else; draws the main window into its root `Ui`, then each extra window's viewport via `render_window_body(ui)`); render dispatch to UI submodules |
| `src/theme.rs` | Design language tokens: spacing (SP_0–SP_6, `f32`) with `margin_same/xy/sides()` (egui margins are `i8`), radii (R_NONE–R_LG, `u8` like egui's `CornerRadius`), `frameless_text_edit()` (no frame, padding kept), `scroll_style()` (egui's edge fade, coasting and rubber band off), `apply_widget_metrics()` (button padding, widget height, default text sizes, animation time, panel grab: egui 0.28's values, pinned), `rgba_unmultiplied()` / `with_alpha()` (every translucent colour; egui 0.28's linear-space premultiply), typography (FONT_*), alpha/blend constants, 15-theme palette, semantic colors, WCAG contrast helpers |
| `src/workspace.rs` | `WorkspaceStore` (`find(id)`, `find_for_cwd()` — most specific folder), `Workspace`, `WindowId` — JSON persistence |
| `src/notes/store.rs` | `NoteStore`: the UI side of the notes — each note's text, saves sent `SAVE_DEBOUNCE` (400 ms) after typing pauses (at most `SAVE_MAX_DELAY`, 2 s), outside edits taken in (merged with edits not saved yet), `set_link()` (`None` = the app's own file again), `status()`, `flush()` on quit/restart and on drop. Never touches files on the UI thread; `sync()` runs every frame and, when nothing changed, only hashes the workspaces' ids and names (a link change asks for new watch targets itself) |
| `src/notes/worker.rs` | `NoteWorker` / `Worker`: the one `note-sync` thread that reads and writes note files. Per note: last text, stamp, version; a save built on an older version or racing an outside edit is merged, never overwritten; polls every `POLL_INTERVAL` (1 s); relink merges new file + old file + unsaved edits and sets the own file aside as `<name>.md.bak`; never writes over a file it could not read; only a relink moves a note to or from a linked file |
| `src/notes/merge.rs` | `merge_note_texts(theirs, ours)`: theirs as is, plus every paragraph of ours it doesn't contain (trimmed text match), so shared text appears once; CRLF-aware. Pure |
| `src/notes/target.rs` | `NoteKey` (workspace id / `None` = Other), `NoteTarget` (own vs linked), `own_note_path()`, `backup_path()`, `sanitize_name()` |
| `src/notes/links.rs` | `NoteLinks`: `notes/links.json` (`{"general": path, "<id>": path}`) |
| `src/notes/disk.rs` | `read_note()` / `write_note()` / `stamp()` (mtime + length + file or not: a folder put in a note's place has a stamp, so its removal is noticed); ≤ `util::MAX_TEXT_FILE_BYTES`, UTF-8 only; an emptied own note removes its file, a linked file is never removed |
| `src/notes/legacy.rs` | One-off move of the old `notes.json` into note files |
| `src/pane_tree.rs` | `PaneNode` tree (Leaf/Split), `SplitDir`, `split_rect()` — recursive pane splitting within tabs |
| `src/logging.rs` | `LogLevel` enum, `LogGuard`; non-blocking daily-rotated file logging via tracing-appender; 7-day retention cleanup; `load_configured_level()` partial settings parse |
| `src/single_instance.rs` | `SingleInstanceGuard`: Windows `CreateMutexW` / Unix `flock`; bypass with `--no-singleton` |
| `src/util.rs` | `safe_json_load()`, `atomic_write()`, directory expansion, panic handler, `is_markdown_path()` / `MARKDOWN_EXTENSIONS` / `PLAIN_TEXT_EXTENSIONS`, `reveal_in_file_manager()` (a file not written yet is shown by its folder) |
| `src/egui_test_frames.rs` | Tests only: `run_ui()` / `run_pass()` run one headless egui frame and clear its texture uploads (epaint asserts they are never dropped unapplied) |
| `src/os_cursor.rs` | The pointer's screen position asked of the OS (CoreGraphics / `GetCursorPos` / X11 `XQueryPointer` via runtime-loaded libX11), and `to_window_points()`. Needed while files are dragged in from another app: winit 0.30 gives hovered/dropped files no position and the window gets no mouse moves then |

### Terminal & PTY

| File | Responsibility |
|------|---------------|
| `src/terminal/mod.rs` | `Session` struct wrapping `Term<EventProxy>`; `EventProxy` (`EventListener` impl); `TermSize` (`Dimensions` impl) |
| `src/terminal/tests.rs` | 12 terminal emulator tests using `alacritty_terminal` APIs |
| `src/pty/mod.rs` | `SessionManager`: spawn/resize PTY sessions; `ShellKind` enum; each session gets dedicated reader/writer threads |
| `src/pty/reader.rs` | Dedicated reader thread per PTY: tee-parses OSC 7 with `vte 0.13`, feeds rest to alacritty `Processor` in 4KB chunks (`advance_chunk`: feed, keep a scrolled-up view in place, bump the content generation) under the session's output gate; each chunk's write lock is released with `unlock_fair` so a waiting frame gets it |
| `src/pty/shell_integration.rs` | Shell prompt integration helpers; OSC 7 injection for bash/zsh/fish |
| `src/pty/foreground.rs` | Platform-specific foreground process detection (Windows toolhelp / Linux /proc) |
| `src/pty/foreground_worker.rs` | `ForegroundWorker`: background thread polling foreground detection every 500ms; UI reads from cache. Each terminal's Claude conversation: by the Claude process's `~/.claude/sessions/<pid>.json` (certain; takes the id from a terminal that guessed it), else the newest one in its folder no other terminal holds (a guess never takes another terminal's id) |
| `src/renderer/mod.rs` | Re-export of `terminal_pass` |
| `src/renderer/terminal_pass.rs` | `TerminalView`, `TerminalGeometry`, `SelectionRange`, `TerminalSnapshot` — cell rendering using `alacritty_terminal` grid API. Each session keeps the snapshot it was last drawn from (`SessionEntry.render_snapshot`); a frame waits at most `LOCK_WAIT_BUDGET` (2 ms) for the lock, else paints that snapshot — never a blank terminal |

### App State & Logic (`src/app/`)

| File | Responsibility |
|------|---------------|
| `src/app/state.rs` | Core app state initialization; session/pane/workspace restoration from persisted JSON; `pane_group()` / `pane_workspace()` (`pane_group_by()`, pure): a pane's workspace — its folder first, then the workspace recorded on it, then the one it was opened in, never a deleted one; the single source for tab grouping, the workspace filter and every workspace colour (row, tab, split header, title bar). `shown_group()` / `shown_workspace()`: the focused pane's workspace, which the title bar and the highlighted workspace card show |
| `src/app/fonts.rs` | Font definitions: egui's fonts first, then memory-mapped system fallbacks per OS (symbols, dingbats ✓✕✗, CJK, Hangul) so those don't render as boxes |
| `src/app/pane_state.rs` | `PaneState`: pane list management, active pane tracking, next ID counters, tree management; `shown_pane_ids()` (the tab each split on screen shows); `pane_sections_into()`: each pane's left nav section, the one map the nav order and the session list's sections both read |
| `src/app/deferred_terminals.rs` | `start_shown_restored_sessions()`: a restored session waits as `DeferredTerminal` until a split shows it — any split, not only the focused one — then starts with its saved output, name and pending command |
| `src/app/editor_texts.rs` | `EditorTexts`: the text each file / note editor pane's text box edits this frame, copied out (`get_or_load`) only when the box is drawn — hidden tabs and previews copy nothing — and written back only when the box reported an edit (`into_edited`); `pane_text()` reads a pane's text without copying |
| `src/app/pane.rs` | `PaneEntry`, `SessionEntry`, `PaneContent` enum, `TermSelection` struct; `FileEditorState::write_to_disk()` (the one save path), `has_unsaved_changes()`, and `apply_edit()` / `apply_file_text()` / `mark_saved()`, which compare the buffer to a hash of the file's text so undoing back to it clears the `*` |
| `src/app/close_guard.rs` | `request_close_tab()` / `TabClose`: every single-tab close (header ✖, ⇧⌘W, pane menu, session list ✕/middle click/menu) asks "Save changes?" first when the tab has unsaved edits; `has_unsaved_changes()` / `unsaved_count()` |
| `src/app/session_state.rs` | `SessionState`: session list management, find/add/remove operations |
| `src/app/settings.rs` | `AppSettings`, `CursorStyle` enum; user settings (theme, fonts, cursor, scrollback, `nav_stable_rows` — left nav rows that stay in place, default 7, 0–100; `workspace_stable_rows` — workspace list rows that stay in place, default 5, 0–100; `workspace_color_on_titlebar`, default on; `show_tab_bar`, default off; `info_bar_position`, default Bottom); JSON persistence; `sanitized()` clamps hand-edited values on load |
| `src/app/input.rs` | `key_to_pty_bytes()`: keyboard→PTY byte translation (Ctrl+A–Z, arrows, function keys, modifiers); `paste_bytes()` (bracketed paste when the program asked), `shell_quote_path()` |
| `src/app/title.rs` | `effective_title()`: workspace+session title formatting; `session_title()`: a user-given (pinned) name word for word, else `effective_title` — behind `App::session_label()`, the one terminal name for rows, split headers, window title and saved sessions; shell escape helpers |
| `src/app/feedback.rs` | `FlashManager`, `FlashTarget`, `FlashKind`: subtle UI flash feedback (copy, paste, error) with auto-fade |
| `src/app/markdown.rs` | `render_markdown()` / `render_markdown_zoomed()` (every size × the pane's zoom, `RenderCtx::px`): headers, bold, code blocks, lists, blockquotes, links |
| `src/app/persistence.rs` | `AppSession`, `SavedPane`, `SavedPaneContent`, `SavedSession`; JSON serialization; session restore. A restored session that was never opened keeps its scrollback file across saves (`SavedPaneContent::DeferredTerminal.scrollback_file`); running sessions' files are written under a per-save stamp so they never overwrite it |
| `src/app/multi_window.rs` | `ExtraWindow`, `WindowState`, `WindowView`; egui multi-viewport for workspaces. `DeferredSaves`: a save asked for during an extra window's frame (its view is swapped into `App` by `App::with_window_view`) waits until the view is swapped back, so it never writes one window's state as another's |
| `src/app/window_state.rs` | OS window state: minimized/fullscreen reads that are right per platform, maximize/fullscreen toggles, `minimize()`. On macOS, minimize and zoom go through AppKit (`macos_objc`): `ViewportCommand::Minimized(true)` sets egui's cached flag, nothing clears it there, and eframe 0.37 then never draws the restored window again |
| `src/app/quit.rs` | `App::quit_app()` / `close_main_window()`: every quit (the quit dialog, the main window's close button, macOS ⌘Q) closes the main window, which ends the app, from whichever window it is asked in. The quit dialog is drawn in every open window |
| `src/app/restart.rs` | Restarting into an installed update: the Restart pill / Settings button only ask for it (`request_restart`), and the main window's frame saves and restarts (`restart_if_due`), since a restart exits at once and a save from an extra window's frame would wait |
| `src/app/workspace_ui.rs` | What the workspace dialogs hold and the checks their buttons follow (pure): `ColorChoice` (preset swatch or custom), `WorkspaceDialog`, `WorkspaceEditDialog`, `OpenFolderDialog` / `FolderEntry` / `EntryCheck` — one or more folders, `add_folders()` (a second pick or drop joins the open dialog), names and colours free of the store and of each other |
| `src/app/workspace_colors.rs` | `PRESET_COLORS`, `pick_distinct_colors()`: each new workspace's colour is the candidate (presets first, then 24 hues × 3 tones) farthest in OKLab from every colour in use and every one picked before it; presets win close calls. Pure |
| `src/app/open_paths.rs` | Opening what was picked or dropped: `pick_folders_async()` (multi-select system picker on its own thread), `process_open_requests()`, `show_open_folders()`, `open_folders()` (saves/updates each workspace, one terminal per folder via `spawn_terminal_pane`, first folder focused), `open_file_in_editor()` — the one path that opens a file in an editor (file browser, Markdown, palette, git, drops) |
| `src/app/notes_link.rs` | `run_note_link_actions()`, the note file pickers (Markdown or text files; on a `note-file-picker` thread; the path is canonicalised there so a symlink stays a link), `NoteFilePicks`, `link_picked_note_files()`, `notes_header_info()` |
| `src/app/preview_shortcut.rs` | Toggle Markdown preview (⇧⌘M / Ctrl+Shift+M, command palette): the right panel's notes while the keyboard is in them (text box or preview, which takes the keyboard when clicked), else the active Markdown file editor or notes pane, else the shown notes (`preview_target()`, pure); works while typing; `toggle_pane_preview()` (shared with the panes' buttons), `preview_tips()` (the toggles' hover text with the chord) |
| `src/app/pending_open.rs` | `OpenQueue` / `OpenRequest`: folders and files to open from a background thread (picker, sorted drops), tagged with the window they were asked in; each window's frame takes only its own |
| `src/app/os_drop.rs` | Files and folders dragged in from the OS. `zone_at()`: over a terminal = paste its shell-quoted paths there, anywhere else = open (files in the split dropped on); pointer unknown = paste into the focused terminal or open. `sort_dropped()` (folders / text files ≤ 16 MB / skipped) runs on a thread; the hovered zone is lit up and labelled |
| `src/app/worker_manager.rs` | Thread lifecycle management; cleanup helpers |
| `src/app/scrollback_saver.rs` | `ScrollbackSaver`: saves running sessions' scrollback in the background and does every write of `session.json` (the UI thread only builds the JSON). A changed session is captured once its output is quiet for `SETTLE` (10 s) or changing for `MAX_STALE` (60 s), read in slices of at most `SLICE_BUDGET` (1 ms) per read lock while output waits on the session's output gate; a capture a resize tore is dropped. After each session write it prunes the scrollback folder to the files the JSON names plus each session's latest. `adopt()` keeps a restored session's file as its latest; `capture_all_now()` + `flush()` back `App::save_session_final()` (quit, restart). Sessions are tracked by a `SaverTracking` guard on `SessionEntry` |
| `src/app/scrollback_capture.rs` | `extract_grid_as_ansi()`, `AnsiCapture` (the same capture a few lines at a time; lines are independent, so any step size gives the same bytes) |
| `src/app/claude_session.rs` | Claude Code conversations: id lookup (`sessions/<pid>.json`, or by folder skipping ids other sessions hold), `claude_project_dir_name()` (Claude's own rule: every non-alphanumeric UTF-16 unit → `-`, names over 200 chars can't be derived), `resume_plan()` / `ClaudeResume` — the one place restore, reopen and deferred start decide `claude --resume "<id>"` (transcript found in its folder, else in any project folder, run in the folder the transcript says it began in) or `claude --continue` (only when no other Claude session shares the folder), else nothing (logged) |
| `src/app/term_context_menu.rs` | The terminal's right-click menu (Copy, Paste, Search, Select All), drawn every frame it is open — outside the terminal's input handling, which stands down while any floating layer (the menu itself) is open; `copy_terminal_selection()` / `paste_into_terminal()`, shared with ⌘C / ⌘V |
| `src/app/layout_shortcuts.rs` | `App::apply_layout_shortcut()` / `AppAction::arranges_layout()`: split, close tab, show/hide side panels, previous/next and numbered tabs, switch to last session (Ctrl+Tab) — the one handler for the terminal key loop and for an active file editor (which otherwise keeps its keys for typing); `run_held_layout_keys()` runs the Ctrl+Tab that `raw_input_hook` keeps away from a focused file editor or notes box; extra windows hold it in `hold_extra_window_keys()` and park the keyboard for that frame (egui has already read the Tab as a focus move), then `restore_parked_focus()` gives it back |
| `src/app/unsplit.rs` | `Unsplit` (`Split(group)` / `All`): leaving split mode without closing anything — a split's tabs join its layout sibling (which takes its space), or every tab joins the focused split; the tab in front stays in front. Asked for by the info bar's ⊡ (right-click: Unsplit All), a left nav split header's right-click menu, ⌥⇧⌘\ / Ctrl+Alt+Shift+\ (Unsplit; Unsplit All has no default chord) and the command palette; `App::apply_unsplit()` runs it after the splits are drawn and saves the layout |
| `src/app/nav_order_sync.rs` | `refresh_nav_order()`: feeds the window's `NavOrder` once per window frame (also while the left panel is hidden); `numbered_tab_targets()`: what ⌘1–9 open — the focused split's rows in left nav order (the whole list without splits), filtered like the list |
| `src/app/workspace_order_sync.rs` | `refresh_workspace_order()`: feeds the window's workspace list order (`NavOrder<u64>`) once per window frame — the session list's rule on workspaces: the top `workspace_stable_rows` cards never move on their own, one switched to from below them (`switch_group` stamps `last_activated`) moves to row 1; held while the pointer is over the list. Saved as `AppSession.workspace_order` (extra windows: `SavedWindowNavOrder.workspace_order`); with none saved it starts from the old sort (open tabs first by recency, then by name) |
| `src/app/pty_resize.rs` | `PendingResizes`: a terminal's grid follows its pane once the pane keeps a new size for `SETTLE` (150 ms; ConPTY clears the screen on every resize). A pane back at its grid's size cancels the wait, so a size it only passed through is never applied (that left the grid wider than the pane, cutting text off on the right). `grid_dim()`. Pure |
| `src/app/view_toggles.rs` | `VIEW_TOGGLES`, `App::toggle_view()` / `view_is_open()`: the view's show/hide actions (sidebar, explorer, switcher, recently closed, settings, shortcut sheet, and the tab bar, which has no titlebar button) — the single path for titlebar buttons, shortcuts, command palette, the macOS menu bar and the Settings checkbox |
| `src/app/macos_fullscreen.rs` | macOS only: in native full screen AppKit puts the title bar winit adds for the switch (title + traffic lights) in a child strip window (`NSToolbarFullScreenWindow`) that slides down under the menu bar on hover, over the app's title bar. On every entry (and at launch for a restored window) that strip is emptied and made click-through; window style and the menu bar are left alone |
| `src/app/macos_menu.rs` | macOS only: adds Settings… to the app menu, a View menu (Hide/Show Sidebar, Hide/Show Explorer, Hide/Show Tab Bar, Switcher, Recently Closed Sessions) and Help → Keyboard Shortcuts, each showing its current shortcut. The menu never claims those keys (an item disables itself while AppKit matches a key press against its own chord), so the app's own key handling stays the only one. Clicks are queued and run by the focused window's frame |

### File System & Git (`src/app/`)

| File | Responsibility |
|------|---------------|
| `src/app/watcher.rs` | `WatchState`, `WatchCommand`, `WatchResult`; background file system watcher (notify crate); debounced git refresh |
| `src/app/file_browser.rs` | `FileEntry`, `DirData`, `SubdirCache`; directory tree listing; file entry sorting/caching; `.gitignore` parsing |
| `src/app/git_diff.rs` | `GitDiffState`; git diff rendering, staged/unstaged sections, inline hunks, side-by-side toggle; the diff pane's toolbar (↑ ↓ change arrows always shown, greyed out with "No changes" when there is nothing to step to); `DiffContent` + zoom for the inline / side-by-side views |
| `src/app/git_worker.rs` | `GitWorker`: background thread for git operations (info, stage/unstage, commit, push, diff). Commands that take a status path (stage, unstage, revert, delete, .gitignore) run at the repository root. `file_diff()`: a file's (absolute path) text at HEAD, now, and its hunks, parsed on the worker; an untracked file (nothing at HEAD, no `git diff`) is one hunk of added lines |
| `src/app/git_paths.rs` | `git status --porcelain` paths are relative to the repository root, wherever the terminal is: `status_base()` (UI: the root the watcher found, `DirData.git_root`; `App::active_git_base()`), `repo_root_or_self()` / `locate()` (workers), `git_relative()` (`/`-separated) |
| `src/app/workspace_git_worker.rs` | Workspace-scoped git worker for per-workspace git state tracking |
| `src/app/diff_parser.rs` | `DiffHunk`, `DiffLine`, `DiffViewMode`, `SideBySideLine`; unified diff format parser; `new_file_hunks()` (a new file as all-added lines) |
| `src/git/parser.rs` | `FileChangeKind` enum, `GitFileStatus` struct; `git status --porcelain` parser (C-style unquoting) |

### Search

| File | Responsibility |
|------|---------------|
| `src/search.rs` | `SearchState`, `SearchMatch`, `TextSearchState`; in-session terminal text search with match navigation |
| `src/search_worker.rs` | `SearchWorker`, `GlobalSearchResults`; background thread for cross-session fuzzy search |
| `src/file_search_worker.rs` | `FileSearchWorker`, `FileSearchResults`; background thread for file path fuzzy search |

### UI Components (`src/app/ui/`)

| File | Responsibility |
|------|---------------|
| `src/app/ui/titlebar.rs` | Window title bar with workspace color tint, update pill, toolbar, focus tracking |
| `src/app/ui/update_pill.rs` | Titlebar update pill: label and click action per `UpdateStatus` (available, downloading, installing, restart) |
| `src/app/ui/notes_header.rs` | `NotesHeader`: the one notes header (side panel and notes pane): the shared title row (`preview_header::TitleRow`: title ellipsised, whole on hover, a click on it does nothing — only ▼ collapses the side panel's notes; Markdown preview toggle 👁 beside it, Markdown notes only), ⚠ only while saving/linking has a problem (shown only when the whole title still fits; the ⚙ takes the problem's colour either way), notes file button ⚙ before the caller's buttons. ⚙ menu (`menu_entries()`, pure): Notes file — Default / the linked file (radio), Choose a File…, Create a New File…, Copy Path, Show in Finder |
| `src/app/ui/preview_header.rs` | The title row every Markdown view shares: `TitleRow` (title + 👁 `PreviewToggle`, an optional `TitleBadge` only while the whole title fits), `PreviewHeader` (a Markdown file pane's header: its name + 👁, as a note's), `PreviewTips`, `HeaderResponse`, `open_in_pane_button()` (the ⬜ on the side panel's notes and Markdown tab headers) |
| `src/app/ui/zoom_badge.rs` | `zoom_input()` (⌘ / Ctrl + scroll or a pinch with the pointer over a pane, nothing under a popup), `zoom_badge()` (bottom-right "125%" pill while not 100 %; click = back to 100 %) |
| `src/app/ui/note_preview.rs` | `render_note_preview(ui, text, zoom)` (a note as Markdown, via `markdown::render_markdown_zoomed`; a hint when empty), `NOTES_PLACEHOLDER`. Preview state: side panel `notes_panel_preview` (per window, saved), notes pane `NoteEditorState.show_preview` (saved; a new pane starts as the side panel is) |
| `src/app/ui/pane_labels.rs` | `tab_display_text()`, `tab_tooltip_text()`, `ws_color_for_pane()` (the colour of `pane_workspace`), `pane_cwd()`: display text, tooltip, colour and path derivation for a pane — the one source for the session list row, the split header and dialogs, so an edited file reads `* name` (`! name` after a failed save) everywhere; `note_title()` ("Notes · <workspace>", "Notes · Other") for every place a note is named. No rendering |
| `src/app/ui/tab_bar.rs` | The optional row of tabs above each split (`settings.show_tab_bar`, off by default; Settings, View menu, command palette, ⌥⇧⌘T / Ctrl+Alt+Shift+T). One tab per tab the split holds, in its order: click shows it, double-click renames a terminal, ✕ / middle click close it like the split's ✖, right-click gives the session-list row's menu (`App::pane_menu`), drag reorders it, moves it into a gap in another split's tab bar (`DropTarget::GroupTabBar`) or splits it off at an edge. Actions run through the session list's `process_tab_actions`; the info bar below keeps the split buttons |
| `src/app/ui/info_bar.rs` | The pane header for each split: identity text (ellipsised, full text on hover), position toggle, hide, split ⊞ ⊟, unsplit ⊡ (only while there are splits; right-click: Unsplit All), ✖ closes the tab the split shows (right-click: close all tabs in the split, confirmed when more than one); `info_bar_layout()` places the buttons and gives the text the rest. Position/hidden is per split (`EditorGroup.info_bar`, saved with the group layout; unset follows `settings.info_bar_position` (default Bottom), which is the place the user last moved a bar to — hiding one doesn't change it, and a revealed bar goes there). A hidden bar's ℹ reveal button only appears while the pointer is in the pane's top-right corner |
| `src/app/ui/pane_renderer.rs` | `RenderCtx`, `PaneContextAction`; recursive pane tree renderer; terminal/editor/diff/notes dispatch. Document panes (file editor, its Markdown preview, notes pane, diff) read `document_zoom()` and draw `document_zoom_badge()`; text styles are scaled inside their scroll areas (headers stay as they are); zoom requests are applied after the group is drawn, with a repaint |
| `src/app/content_zoom.rs` | `PaneZoom` (each pane's zoom, kept in `PaneState.zoom`, closed panes forgotten each frame; saved with the pane as `SavedPane.zoom` once it has settled for `SAVE_SETTLE` (1 s), `restored_zoom()` clamps a saved value), `zoomed()` (0.5–3×, stops at 100 % on the way through), `scale_text_styles()`, `ZoomRequest`, `percent_label()`. Terminals never zoom; the conflict resolver's lines do (its toolbar doesn't). ⌘= / ⌘+ / ⌘- / ⌘0 zoom the whole window (`window_zoom`), separately. Pure |
| `src/app/window_zoom.rs` | Whole-window zoom (egui's `zoom_factor`): ⌘= / ⌘+ in, ⌘- out, ⌘0 back to 100 % (Ctrl off macOS), as the app's own exact-match shortcuts (`AppAction::ZoomWindow*`). `disable_builtin_shortcuts()`: egui's built-in zoom keys ignore Shift/Alt and fire after the frame (⇧⌘- split *and* zoomed out), so they are off; egui's quit key is off where it is Ctrl+Q (the terminal's) |
| `src/app/ui/command_palette.rs` | Command palette: action search/filter/dispatch; keyboard-driven execution |
| `src/app/ui/open_folders_dialog.rs` | The Open Folder dialog: one folder gets the full form (path, "inside workspace" note with Open-instead, shell, Save as Workspace, name, colour); several get a compact row each (tick, colour swatch that expands the colour choices inline, name, path) |
| `src/app/ui/color_choice.rs` | `color_choices()`: the preset swatches plus a custom swatch whose picker opens inline — every workspace dialog uses it. Dialogs float on egui's `Tooltip` layer and egui popups open on `Foreground`, behind them, so nothing here is a popup |
| `src/app/ui/dialogs.rs` | Modal dialogs: new session, new workspace, close confirmation, "Save changes?" for one unsaved tab, git commit/push, about. `request_close_all()` asks when a close covers more than one tab or any unsaved one; one saved tab closes at once. Close-all and quit say how many files have unsaved changes |
| `src/app/ui/settings_overlay.rs` | Settings panel: theme picker, cursor style, font size, scrollback, keybinding editor |
| `src/app/ui/search_bar.rs` | Reusable search input widget; persistent focus tracking |
| `src/app/ui/debounce.rs` | `Debouncer<T>`: time-based debouncing for input fields |
| `src/app/ui/left_panel/session_list.rs` | The single session navigator: split sections, three-state row highlighting, row chrome and interactions, "+New" menu (Open Folder… picks several), fuzzy filter, workspace filter dropdown; dragging a row onto another row of its section places it above/below that row in the left nav only (`DropTarget::NavRow`, a line shows where). Row menu: Rename (pins the name on the session — `Session::pin_title` — so prompt hooks and programs can't replace it; an empty name goes back to the automatic one; a restored session not started yet is renamed too, and the pin applies when it starts; saved with the session and closed-session records), Copy path, Show in Finder/File Explorer (`util::REVEAL_IN_FILE_MANAGER_LABEL`) |
| `src/app/ui/left_panel/sections.rs` | `section_labels()`, `RowState`: split-section grouping and naming, row prominence. Pure — no `egui::Ui`, so it is unit-testable |
| `src/app/ui/left_panel/nav_order.rs` | `NavOrder<Id>`, `NavItem<Id>` / `NavPane`, `SectionKey`: the left nav row order (also the workspace list's, by workspace id). Per split section, the top `nav_stable_rows` rows never move on their own; a session activated from below them (or new once they are full) moves to the section's row 1. Read from each pane's `last_active_at`; activations are held while the pointer is over the list, then applied oldest first; a pane changing section goes to the end of its new one at once (panes moving together keep their order), then follows the rule there if the move activated it. `place()` / `would_move()`: a row dragged next to another of its section. Saved as `AppSession.nav_order` (extra windows: `window_nav_orders`). Pure |
| `src/app/ui/left_panel/workspace_section.rs` | Workspace cards in the window's `workspace_order`; the focused tab's workspace (`shown_group`) highlighted; collapse toggle; color swatch; git branch/diff badges |
| `src/app/ui/right_panel_size.rs` | `right_panel_width_range()`: the right panel may take up to `RIGHT_PANEL_MAX_SHARE` (80 %) of the space beside the left panel, at least `RIGHT_PANEL_MIN_W`; `restored_right_panel_width()`. Pure |
| `src/app/ui/left_panel/context_menu.rs` | Right-click context menu: rename, edit color, delete, duplicate, open in new window |

### UI Kit (`src/ui_kit/`)

| File | Responsibility |
|------|---------------|
| `src/ui_kit/buttons.rs` | `IconButton`, `ActionButton`, `IconButtonStyle`, `ActionButtonStyle`; themed button wrappers |
| `src/ui_kit/containers.rs` | `dialog()`, `DialogConfig`, `DialogResponse`, `DialogAnchor`, `DialogWidth`; flex-box layouts; `popup_below()` (a popup list under a widget, items filling its width — every dropdown and the terminal menu) |
| `src/ui_kit/layout.rs` | Grid/flex helpers; responsive layout utilities |
| `src/ui_kit/lists.rs` | Scrollable list rendering with selection tracking |
| `src/ui_kit/text.rs` | Rich text formatting, emphasis, code inline styling; `one_line_elided()` (one line ending in …) and `full_hover_text()` (a tooltip that never wraps, so it grows when its text does); `TextEditFontHint::font_with_hint()` (a text box's font and its placeholder in that same font) |

### Utilities & Infrastructure

| File | Responsibility |
|------|---------------|
| `src/shortcuts.rs` | `AppAction` enum (50+ actions), `ShortcutRegistry`, `Shortcut`, `ShortcutGroup`; keybinding resolution (exact: extra Shift/Alt never match). `consume_global()`: a global chord a focused text box edits with (`TEXT_EDITING_PRIMARY_KEYS`: ⌘A/C/V/X/Y/Z with any Shift/Alt, e.g. ⇧⌘Z redo vs Zoom pane) stays the text box's. A keybindings file gets the default chords of actions it doesn't bind, unless it uses that chord |
| `src/keybindings.rs` | `KeyBinding`, `KeybindingsConfig`; JSON-based custom keybindings; default binding set |
| `src/syntax.rs` | Syntax highlighting via syntect; language detection; theme building from app theme |
| `src/url_detector.rs` | `DetectedUrl`; regex-based URL detection in terminal output; clickable links |
| `src/md_detector.rs` | `DetectedMdPath`; markdown file detection and path tracking |
| `src/updater/mod.rs` | `UpdateStatus`, `UpdateChecker`; background GitHub releases API polling; dispatches the update by install kind |
| `src/updater/install.rs` | `InstallKind` (`Standalone` / macOS `Homebrew` / macOS `AppBundle`): detection, required release asset, leftover cleanup |
| `src/updater/standalone.rs` | Raw-binary self-replace (+ elevation, `--apply-update`); Windows, Linux, bare macOS binaries |
| `src/updater/download.rs` | Streaming release-asset download with progress |
| `src/updater/http.rs` | The updater's one HTTP client setup: user agent, per-request total timeout, connect timeout split across a host's addresses (an unresponsive server is skipped in seconds) |
| `src/updater/macos/mod.rs` | Install classification (`classify` over an `FsProbe`): Homebrew only when brew's `Caskroom/terminal-studio/<ver>/` symlink points at this bundle; cask/DMG constants, temp-path naming |
| `src/updater/macos/homebrew.rs` | Updates cask installs via `brew update` + `brew upgrade --cask --no-quit terminal-studio`, then verifies `Info.plist` |
| `src/updater/macos/app_bundle.rs` | DMG installs: mount release DMG, stage, verify, atomic `renamex_np(RENAME_SWAP)` of the whole `.app` (two-rename fallback); startup sweep of scratch dirs/mounts left by an interrupted update |
| `src/updater/macos/disk_image.rs` | Attach (retried while busy) / detach by device with `diskutil image`/`diskutil eject` on macOS 27+ (where `hdiutil` is deprecated) and `hdiutil` before; `hdiutil info -plist` lookup of attached images, mount-point check |
| `src/updater/macos/os_version.rs` | Running macOS major version via `sysctlbyname("kern.osproductversion")` (no subprocess, read once) |
| `src/updater/macos/process.rs` | Helper-process runner: output to unlinked temp files (no pipes, so quitting the app never breaks brew), timeout that kills the process group or detaches (brew) |
| `src/updater/macos/bundle_version.rs` | Reads `CFBundleShortVersionString` via `plutil`; semver `is_at_least` |
| `src/sys_monitor.rs` | `SysMonitor`, `SystemStats`; CPU/RAM/network monitoring via sysinfo; background thread |

## Key Types & IDs

- Pane IDs: `u32`
- Session IDs: `u32`
- Workspace IDs: `u64`
- Split IDs: `u32` (unique within `App.next_split_id`)
- `PaneContent` enum: `Terminal(session_id)` | `DeferredTerminal(workspace_id)` | `FileEditor(FileEditorState)` | `FileDiff(path)` | `NoteEditor(workspace_id)`
- `WorkspaceStore` is the source of truth for workspace data
- Terminal grid coordinates: 0-based; columns/rows are `usize` inside alacritty, `u16` at the PTY layer

## Core Architecture

**Terminal emulator:**
- Backed by `alacritty_terminal 0.26` — `Term<EventProxy>` holds the grid, parser state, and mode flags
- `EventProxy` implements `alacritty_terminal::event::EventListener`; handles `Title`, `PtyWrite`, `CursorBlinkingChange`
- `TermSize` implements `alacritty_terminal::grid::Dimensions` — wraps `(cols: usize, lines: usize)`
- OSC 7 (CWD) extracted by a tee `vte 0.13` parser (`CwdPerformer` in `reader.rs`) run on the raw byte stream before alacritty processes it
- Scrollback: alacritty's built-in scrollback, default **100 000 lines** (configurable up to 1M in settings; set via `Config::scrolling_history` in `Session::new`)

**Threading model:**
- UI runs on the main thread
- Each PTY has a dedicated `pty-reader-N` thread: reads bytes → tee CWD → alacritty `Processor::advance` under write lock (4 KB per lock, released with `unlock_fair`) → `ctx.request_repaint_after(8ms)`
- Each PTY has a dedicated `pty-writer-N` thread: drains `mpsc::Receiver<Vec<u8>>` → writes to PTY master
- UI sends input via `SessionEntry.pty_tx: mpsc::Sender<Vec<u8>>`; alacritty's `PtyWrite` events go through the same channel
- `Session` state is `Arc<RwLock<Session>>` — shared between reader thread and UI thread. `parking_lot`'s lock is fair: a parked writer makes new readers wait, so nothing may hold a read lock long (the scrollback saver reads in slices). `Session::content_generation()` is bumped under the write lock on every grid change; `Session::output_gate()` is held by the reader while feeding and by the saver while capturing (lock order: gate, then session; the UI never takes the gate)
- `alive` flag is `Arc<AtomicBool>` — signals reader thread to stop
- Background singleton threads (each uses `Arc<AtomicBool> alive` + `Arc<Mutex<>> results`):
  - `ForegroundWorker`: process detection polling (500ms)
  - `WatchState`: file system monitoring + git refresh debounce
  - `GitWorker`: git operations queue (stage/unstage/commit/push/diff)
  - `SearchWorker`: cross-session fuzzy search
  - `FileSearchWorker`: file path fuzzy search
  - `UpdateChecker`: GitHub releases polling (24h interval)
  - `ScrollbackSaver`: scrollback captures and `session.json` writes (1 s tick)
- Closing a tab: `closed-session-save` thread looks up its Claude conversation first (`claude_session::pick_claude_session_id` — the closing Claude deletes its session file as it exits), then captures the session's scrollback
  - `SysMonitor`: CPU/RAM/network stats

**Pane tree (in-tab splits):**
- `App.pane_trees: HashMap<tab_id, PaneNode>` — one tree per tab
- `PaneNode` is a recursive enum: `Leaf { pane_id, last_size }` or `Split { split_id, dir, ratio, a, b }`
- `render_node()` in `app.rs` recurses the tree, calling `split_rect()` to divide screen space and rendering each leaf
- 4px interactive dividers support drag-to-resize (ratio clamped to [0.1, 0.9])
- Keyboard shortcuts: `Ctrl+Shift+\` (horizontal split), `Ctrl+Shift+-` (vertical split), `Ctrl+Shift+W` (close pane), `Alt+Arrow` (focus movement), `Ctrl+Shift+[` / `Ctrl+Shift+]` (⇧⌘[ / ⇧⌘] on macOS: previous/next pane **within the focused split only**), ⌘1–9 (Ctrl+Shift+1–9: the focused split's rows in left nav order), `Ctrl+Tab` (every platform: switch to the last session, in any split), `Ctrl+Alt+Shift+\` (⌥⇧⌘\: unsplit — the focused split's tabs join the split beside it, nothing closes). These work from a file editor too (`layout_shortcuts.rs`)
- Each split has an optional tab bar (`tab_bar.rs`, off by default) above its info bar, which sits under the pane by default. The left nav session list shows every tab either way — it groups rows into split sections and marks which panes are on screen. Rename / close / copy-path / reveal / labels are the same in both (`pane_menu`, `render_rename_field`, `process_tab_actions`)
- Drag-and-drop: dropping on the middle of a split joins that split; dropping in the outer third of any edge splits it (`edge_zone()` in `src/app/drag.rs`); dropping on a left-nav section header joins that split; dropping between left-nav rows of the same section reorders the list only
- OS drag-and-drop (`os_drop.rs`): files from Finder/Explorer pasted as paths over a terminal, opened anywhere else; the pointer comes from `os_cursor` (winit 0.30 Wayland reports no file drops at all)
- **Note:** `pane_trees` is not persisted to disk — splits reset on restart

**Multi-window (multi-viewport):**
- `App.extra_windows: Vec<WindowState>` — additional egui viewports
- Each viewport renders a specific workspace via `ctx.show_viewport_deferred(...)`
- Single-instance enforcement: `SingleInstanceGuard::try_acquire()` at startup (bypass with `--no-singleton`)

**Persistence:**
- JSON files in `%APPDATA%\terminal-studio\` (Windows) or `~/.config/terminal-studio/` (Unix)
- `session.json`: open panes, CWDs, active workspace, panel layout
- `workspaces.json`: workspace definitions (name, path, color)
- `notes/`: one Markdown file per workspace (`<name>.md`, `general.md` for Other) and `links.json` — the notes linked to a file of the user's choosing (an Obsidian page, any text file), edited both here and in other apps. The old `notes.json` is migrated once
- `settings.json`: user preferences (theme, font size, cursor style, scrollback)
- `keybindings.json`: custom keyboard shortcuts

**File watching:**
- `notify::RecommendedWatcher` with a channel-based event loop
- Only tracks non-hidden directories (no dotfiles)
- Git operations via `GitWorker` background thread; refresh debounce is 500ms

## Terminal Emulator Details

- Parser: `alacritty_terminal 0.26` internal parser (vte 0.15); OSC 7 tee-parsed by `vte 0.13` in reader thread
- Scrollback: 100 000 lines default (configurable up to 1M in settings UI; uses `Config::scrolling_history`)
- OSC 7 → set `cwd` + `prompt_ready`; OSC 0/2 → set `title` (via `EventProxy::send_event(Event::Title(...))`)
- Mouse events: SGR format (`?1006`) when `TermMode::SGR_MOUSE`; coordinates are 1-based
- Scrolling: `term.scroll_display(Scroll::Delta(n))` / `Scroll::Bottom`; `display_offset()` drives the renderer
- Mode flags read via `term.mode().contains(TermMode::XYZ)` — no separate bool fields on Session
- `cursor.point`: `grid.cursor.point.column.0` (usize) and `grid.cursor.point.line.0` (i32)

## Dependencies

```toml
# UI framework
eframe = "0.37"              # wgpu + glow, default_fonts, clipboard, links, persistence (no accesskit)
egui = "0.37"                # + monochrome_emoji_fonts (the emoji fonts 0.28 bundled)
serde + serde_json = "1"

# Terminal emulation
alacritty_terminal = "0.26"  # full terminal emulator (grid, parser, modes)
vte = "0.13"                 # tee parser for OSC 7 CWD extraction
portable-pty = "0.9"         # ConPTY (Windows) / openpty (Unix)

# Concurrency & error handling
parking_lot = "0.12"         # RwLock for session state
anyhow = "1"

# Logging
log = "0.4"
tracing-subscriber = "0.3"   # non-blocking file logging with env-filter + tracing-log bridge
tracing-appender = "0.2"     # daily rolling file appender

# File system & dialogs
notify = "6"                 # file system watching
rfd = "0.15"                 # native file/folder picker dialogs

# Search & text processing
fuzzy-matcher = "0.3"        # Sublime-style fuzzy scoring
regex = "1"                  # URL detection, search
base64 = "0.22"              # OSC 52 clipboard decoding
syntect = "5"                # syntax highlighting (Sublime Text grammars)

# Self-update
reqwest = "0.12"             # HTTP client (rustls, no OpenSSL)
semver = "1"                 # version comparison
self-replace = "1"           # replace running binary (Windows-safe)

# System utilities
sysinfo = "0.33"             # CPU, RAM, network monitoring
open = "5"                   # open URLs in browser
arboard = "3"                # clipboard access

# Windows-only
windows-sys = "0.52"         # DWM, toolhelp, CreateMutexW, window messages

# [patch.crates-io]
winit = { git = "https://github.com/rust-windowing/winit", rev = "39c4009c…" }
```

winit is patched to one commit on top of v0.30.13 (the open 0.30 backport rust-windowing/winit#4694, which brings in its own `dpi` 0.1.1). Without it, on Windows, dragging the window across monitors with different scaling flips it between the two scales and grows it on every crossing (winit#4041): flicker and a drag that fights the cursor; upstream reports a crash once the window passes the GPU's texture limit. `deny.toml` allows that one git source. Remove the patch and the `allow-git` entry once winit 0.30.14 (or an eframe on winit 0.31) has the fix.

## Conventions

**Colors:** Always use constants from `theme.rs`. Never hardcode RGB in `app.rs`.

```rust
// correct
use crate::theme::SURFACE_0;
// wrong
Color32::from_rgb(30, 30, 46)
```

**Platform-specific code:**
```rust
#[cfg(target_os = "windows")]
// ...
#[cfg(not(target_os = "windows"))]
// ...
```

**Design language tokens (all in `theme.rs`):**
- Spacing: `SP_0` (0) through `SP_6` (16) — use these, never hardcode pixel values
- Radii: `R_NONE` (0), `R_SM` (3), `R_MD` (6), `R_LG` (8)
- Typography: `FONT_HEADING_1` (22), `FONT_HEADING_2` (18), `FONT_STATUS` (16), `FONT_TERM` (14), `FONT_UI_LG` (13), `FONT_UI_MD` (12), `FONT_UI_SM` (11), `FONT_UI_XS` (10)
- Alpha: `ALPHA_CURSOR`, `ALPHA_SELECTION`, `ALPHA_OVERLAY_DIM`, `ALPHA_SCROLLBAR_*`, `ALPHA_FLASH`
- Blend factors: `BLEND_SUBTLE` (0.15), `BLEND_LIGHT` (0.30), `BLEND_MEDIUM` (0.50), `BLEND_STRONG` (0.75)
- Icons: `ICON_SM` (10), `ICON_MD` (14), `ICON_LG` (18), `ICON_STROKE`, `ICON_PAD`
- Semantic colors on `Theme`: `accent`, `accent_muted`, `accent_strong`, `success`, `warning`, `error`, `flash_bg`, `flash_success_bg`, `flash_error_bg`
- Flash feedback: use `FlashManager` in `app/feedback.rs` — call `flash.trigger(target, kind)`, render via `flash.render_on_rect()`

## Common Editing Tasks

**Add a new escape sequence:**
- CSI/SGR/ESC: alacritty_terminal handles these internally. Most standard sequences work out of the box.
- OSC: If you need to intercept a new OSC sequence before alacritty sees it, extend `CwdPerformer::osc_dispatch()` in `src/pty/reader.rs`.
- To add a custom response (e.g. device attribute): send bytes back via `EventProxy::send_event(Event::PtyWrite(...))` which alacritty calls automatically, or route through `pty_tx` directly.

**Add a new UI panel/section:**
- Create a new file in `src/app/ui/` and wire it from `src/app/ui/mod.rs`
- Render calls dispatch from `app.rs` → `App::ui()` → `render_window_body(ui)`; panels are added to the window's root `Ui` (`egui::Panel::left/right/top(..).show(ui, ..)`), never to a `Context`

**Add a new theme color:**
1. Add constant to `theme.rs`
2. Reference via `crate::theme::NEW_COLOR` in UI modules

**Add a new workspace field:**
1. Update `Workspace` struct in `workspace.rs`
2. Update `SavedSession` / `AppSession` in `src/app/persistence.rs`

**Add a new keyboard shortcut:**
1. Add variant to `AppAction` enum in `src/shortcuts.rs`
2. Add default binding in `src/keybindings.rs`
3. Handle the action in the appropriate UI module

**Add a new background worker:**
1. Create new file in `src/app/` following `git_worker.rs` pattern (job queue + background thread)
2. Register in `worker_manager.rs` for lifecycle management
3. Start/stop from `App` init/drop

**Add a reusable UI component:**
- Add to `src/ui_kit/` — buttons, containers, lists, layout, text are the existing categories

**Add platform-specific behavior:**
- Gate with `#[cfg(target_os = "windows")]` / `#[cfg(not(target_os = "windows"))]`

## Test Coverage

| File | Tests | Coverage |
|------|-------|---------|
| `src/theme.rs` | 31 | color roundtrip, tinted, short_path, header_bg, text contrast, all-theme validation, sRGB LUT, ensure_term_contrast, translucent colours equal egui 0.28's (every alpha), opaque/clear pass through |
| `src/app/input.rs` | 33 | keyboard→PTY byte translation for all keys and modifier combinations, Ctrl+Tab detection and holding it back from text boxes, bracketed paste bytes |
| `src/app/layout_shortcuts.rs` | 8 | layout keys from an editor apply only from its own text box, the notes box or its preview, or no focus — never a rename or filter field; parked focus is seen where it was and given back unless something took it |
| `src/app/pane_state.rs` | 86 | each split's shown tab, previous active pane (last session), pane CRUD, tree operations (split/nested-split), remove, ratio mutation, size updates, group detach/erase/collapse, orphaned-layout pruning, Ctrl+Tab cycling within a group, per-split info bar position, moving one sets the default for new splits (others keep theirs), each pane's left nav section (a split missing from the layout gives none; one split holds every pane) |
| `src/app/drag.rs` | 78 | drag payload/target resolution, left nav row placement (tabs only), edge-zone geometry, split-edge drops, tab-bar drops (reorder, move to a gap in another split, open new things there), one-pane-per-session and per-file delegation, per-frame cross-window drop harvesting |
| `src/editor_group.rs` | 54 | group layout tree: split (either side), remove, collapse, group id ordering |
| `src/app/ui/left_panel/mod.rs` | 2 | the left panel keeps its outer width, so clearing egui's memory (every 30 minutes, and on a version change) never narrows it by its margins |
| `src/app/ui/left_panel/sections.rs` | 9 | split-section labels, three-state row prominence |
| `src/app/ui/left_panel/nav_order.rs` | 32 | dragged rows placed above/below (no-op spots, same section only, rule applies after, while held), stable top rows, pull to row 1 from below, new rows (room / full), close, hold then apply oldest first, 0 = recency sort, per-section counting, section moves (with and without activation, keeping their order, landing at once while held), restore (saved order, duplicates, unknown ids, unlisted by recency), ⌘1–9 targets |
| `src/app/nav_order_sync.rs` | 2 | ⌘1–9 count the whole list without splits, the focused split with them |
| `src/app/workspace_order_sync.rs` | 7 | first order matches the old sort (ties never depend on store order), nothing saved seeds on the first refresh, saved order kept, top rows stay / a switch from below moves to the top, held while hovered, new and deleted workspaces |
| `src/app/pty_resize.rs` | 8 | applied once settled, same size keeps its start, a changing size restarts, back to the grid size cancels (right-edge clipping), grid size starts nothing, sessions independent / closed forgotten, dims never wrap, clock going back |
| `src/app/ui/right_panel_size.rs` | 5 | up to 80 % beside the left panel, the middle keeps its share, tiny/unknown space stays valid, wide saved width kept, minimum / not-a-number restore |
| `src/app/ui/info_bar.rs` | 9 | position serde, button layout order/overlap (with and without the unsplit button), text never under a button, narrow split, reveal hot zone |
| `src/app/unsplit.rs` | 15 | tabs join the sibling in order and nothing closes, the front tab stays in front and its split is focused, the space goes to the split it was shared with, down to one split, no splits / unknown split / sibling missing do nothing, unsplit all in screen order into the focused split (its info bar kept, first on screen without a focus, off-layout splits kept, duplicates once, stale front tab), fresh ids after, shortcut mapping |
| `src/app/title.rs` | 21 | title formatting with workspaces, effective title, a pinned name shown as typed (even "zsh"), shell escaping |
| `src/pane_tree.rs` | 19 | leaf IDs, split/nested-split, remove (all cases), ratio mutation, split_rect geometry |
| `src/app/diff_parser.rs` | 20 | unified diff parsing, hunk extraction, line kind classification, a new file as one added hunk (each line once) |
| `src/app/git_diff.rs` | 4 | change position label, the ↑ ↓ arrows always shown ("No changes" without hunks), ↓ steps and stops at the last, a zoomed diff is taller and as wide as its longest line |
| `src/app/content_zoom.rs` | 9 | range, stops at 100 % both ways, broken factors ignored, percent label, text styles scale, per-pane zoom, closed panes forgotten, saved zoom restored within range, a change saved once it settles |
| `src/app/ui/conflict_resolver.rs` | 2 | a zoomed conflict is bigger (inline and side by side), the whole pane draws zoomed |
| `src/app/ui/zoom_badge.rs` | 4 | a pinch over the pane asks for zoom, elsewhere nothing, badge only while zoomed and a click resets, hover text says how |
| `src/app/ui/preview_header.rs` | 3 | title keeps its room before a badge, toggle hover text with the shortcut, a file header's 👁 beside its name (a click on the name does nothing) |
| `src/git/parser.rs` | 17 | git status --porcelain parsing, C-style unquoting, file status kinds |
| `src/workspace.rs` | 14 | store CRUD, find_for_cwd, find_for_path, unique name, find by id |
| `src/notes/worker.rs` | 27 | load on watch, verbatim save on the latest version, merge when racing an outside edit or built on an old version, poll sends changes once (touch without change sends nothing), missing linked file kept + rewritten, unreadable files never written over, failed write reported and cleared, link merges file + note + unsaved edits and sets the own file aside, new linked file created, identical file untouched, unlink, failed link keeps the old file, a watch never undoes a relink, status recovery after re-tracking, rename moves / merges the own file, unwatched notes, emptied own note, links file + flush, real thread, deleted own file / deleted notes folder written again, a folder in a file's place fails until it is gone, a deleted linked file can still be unlinked |
| `src/notes/store.rs` | 15 | existing files load, saved after a pause (debounce/max delay), outside edits show up, unsaved edits merged with an incoming file, failed save retried, link merges and is remembered across restarts, unlink, same/own file no-op, failed link undone and reported, workspace rename, edits written on drop, no notes folder, note files (own, linked, links file) deleted while running keep the notes and are written again |
| `src/notes/merge.rs` | 11 | empty sides, identical, superset wins, paragraphs added once, idempotent, CRLF, indentation, non-ASCII |
| `src/notes/disk.rs` | 8 | missing file, stamps (a folder has one), emptied own vs linked note, folders / binary / huge files refused, error wording |
| `src/notes/links.rs` | 5 | round trip, unlink, missing/broken file, unknown keys, stable output |
| `src/notes/target.rs` | 4 | own paths, backup path, target kinds, file-name sanitising |
| `src/notes/legacy.rs` | 2 | old notes move into files (`.bak` kept), a broken old file is left alone |
| `src/app/ui/notes_header.rs` | 11 | no problem while saving fine, problem wording per state, ⚙ tooltip (file in use, leads with a problem), only Markdown notes preview, menu entries (default chosen until linked, Default unlinks, choose/create, copy/reveal the file in use, none without a notes folder), a menu row click, toggle beside the title / a title click does nothing / ⚙ opens the menu, a long title never pushes the icons out |
| `src/app/ui/note_preview.rs` | 2 | Markdown and empty notes both render, a zoomed preview is bigger |
| `src/app/notes_link.rs` | 1 | picked files are taken once, in order |
| `src/app/preview_shortcut.rs` | 3 | the notes being typed in come first, notes that can't preview never toggle a pane elsewhere, then the active pane, then the shown notes |
| `src/app/workspace_ui.rs` | 13 | open-folder dialog: Save as Workspace on outside workspaces / off inside one / existing kept, default names and colours free, several folders distinct from each other, duplicates listed once, saved-by-default folders get the first picks, clashes inside the dialog, blank name, adding folders keeps edits; save dialog defaults; colour choice swatch/picker |
| `src/app/workspace_colors.rs` | 10 | first preset when empty, a batch of 8 clearly different, never a used colour or a repeat, avoids a custom colour near a preset, farthest free candidate, prefers presets, deterministic, past every candidate, OKLab reference values |
| `src/app/pending_open.rs` | 3 | each window takes its own requests in order, closed windows dropped, shared across threads |
| `src/app/os_drop.rs` | 8 | zones (terminal pastes, other pane opens in its split, outside panes, unknown pointer), sorting folders/text/binary/missing/empty, size limit without reading, every zone paints, wording |
| `src/os_cursor.rs` | 4 (+1 Linux) | pixels and points to window points (zoom), no scale no position, OS pointer finite; Linux: no X display makes no connection, so nothing calls `XCloseDisplay(NULL)` (headless SIGSEGV) |
| `src/app/ui/open_folders_dialog.rs` | 4 | the first name field takes the keyboard (one and several folders), rows draw for saved/unsaved/nested, title, problem wording |
| `src/app/ui/color_choice.rs` | 2 | a swatch click inside a dialog-layer area picks it, the custom picker opens inline (no popup) |
| `src/app/ui/titlebar.rs` | 1 | workspace tint only when enabled |
| `src/app/watcher.rs` | 16 | file system watch, git refresh debounce, directory caching |
| `src/terminal/tests.rs` | 14 | session dims, resize (bumps the content generation only when the size changes), content preservation, OSC title, a user-given (pinned) title outlasting the shell's, cursor, bracketed paste, mouse SGR |
| `src/search.rs` | 11 | text search, match finding, index navigation |
| `src/renderer/terminal_pass.rs` | 17 | terminal geometry, cell rendering, scrollbar hit-testing, selection, a busy lock paints the last snapshot (background only before the first draw), reverse-video text on default colours stays visible |
| `src/app/claude_session.rs` | 54 | Claude process detection, session id by pid / scan / folder (ids held elsewhere skipped), project folder names (`_` `.` spaces, non-ASCII per UTF-16 unit, Mac folders Claude wrote, too long), unsafe ids, resume found in its folder / any folder / for a long path, resumes in the folder the conversation began in, `--continue` only when allowed, resume command round trip |
| `src/pty/foreground_worker.rs` | 8 | Claude id cache, pruning, a pid match takes a guessed id, a folder guess never takes another terminal's |
| `src/app/term_context_menu.rs` | 6 | drawn every frame the layer stays visible (every other frame it doesn't — the see-through-menu bug), Copy click with a selection, Copy disabled without one, closed menu draws nothing |
| `src/pty/reader.rs` | 11 | OSC 7 CWD tracking, OSC 52 clipboard, byte streaming, a fed chunk bumps the generation and keeps a scrolled-up view in place |
| `src/app/scrollback_capture.rs` | 17 | ANSI capture: colours, attributes, wide chars, trailing blanks, max lines, round trips; a capture in steps of any size equals one read |
| `src/app/scrollback_saver.rs` | 18 | save timing (settle, stale under constant output, saved), sliced capture (equals one read, the time budget decides how much one lock covers, dropped when resized, completes under constant output), passes write only changed sessions and ask for a session save, empty and dropped sessions, final capture writes only changed ones, saving off, adopt (and off), prune keeps referenced + latest, names never reused, flush, tracking guard |
| `src/syntax.rs` | 10 | syntax highlighting, language detection, theme building |
| `src/app/git_worker.rs` | 20 | git info, stage/unstage, commit, push operations, merge state, revert; diffs: an edited file's hunk, an untracked file is one added hunk, the diff job sends parsed hunks, a file in a subfolder; from a terminal in a subfolder, stage/unstage/revert/.gitignore/delete act on the right file |
| `src/app/git_paths.rs` | 3 | status paths rooted at the repository, `/`-separated relative paths, a subfolder's file located from the root |
| `src/app/ui/debounce.rs` | 9 | time-based debounce logic |
| `src/app/persistence.rs` | 38 | session serialization, the scrollback files a saved state names, restore logic, a pane's zoom (100 % and old files: none), per-split info bar position, pinned session names, untitled buffers (old files load), left nav order and each extra window's (old files load empty), saved indices stay aligned after a pane that can't be restored, workspace list orders (old files load empty), a notes pane's preview (old files open for editing) |
| `src/app/editor_texts.rs` | 4 | copied once per frame on first draw, never for panes not drawn, read without copying, only edited texts written back |
| `src/app/pane.rs` | 15 | pane content enum, terminal selection, editor save (written, untitled never written, failure keeps edits), undoing back to the saved text leaves nothing unsaved |
| `src/app/ui/tab_bar.rs` | 3 | drop gap from the pointer, hidden tabs keep their place in the order, empty bar |
| `src/pty/mod.rs` | 8 | shell detection, session manager operations |
| `src/app/workspace_git_worker.rs` | 8 | workspace-scoped git state |
| `src/file_search_worker.rs` | 8 | file fuzzy search, result filtering |
| `src/app/feedback.rs` | 7 | trigger/tick, flash expiry, alpha decay, duplicate replacement, color generation |
| `src/app/markdown.rs` | 7 | markdown rendering (headers, code, lists, blockquotes) |
| `src/md_detector.rs` | 7 | markdown path detection |
| `src/util.rs` | 14 | atomic write, JSON loading, directory expansion, git root, parent dir of a bare name, Markdown by extension, reveal falls back to the folder |
| `src/app/file_browser.rs` | 10 | directory listing, file entry sorting, gitignore parsing |
| `src/shortcuts.rs` | 32 | keybinding parsing, action resolution, layout chords never use a text-editing key, Ctrl+Tab = last session on every platform and not OS-reserved on macOS, each numbered tab action's index, a text box keeps the chords it edits with (every global chord checked), the first chord is the one shown, a keybindings file gets the defaults it doesn't bind |
| `src/app/multi_window.rs` | 3 | saves outside a window's frame run at once; inside one they wait, per kind, and are taken once |
| `src/app/window_state.rs` | 10 | minimized read per platform (macOS: no window rect, never egui's cached flag), edge-resize handles only on floating windows, minimize goes through AppKit on macOS (never `ViewportCommand::Minimized`), not in full screen |
| `src/app/restart.rs` | 3 | an installed update restarts once, a Restart click always (retry after a failure), nothing otherwise |
| `src/app/quit.rs` | 2 | a quit from an extra window closes the main window (not itself); from the main window, the main window |
| `src/keybindings.rs` | 6 | keybinding config loading, JSON handling |
| `src/app/settings.rs` | 13 | settings persistence, JSON config, rows that stay in place (default, old files, clamp), title bar workspace colour (default on, old files), workspace rows that stay in place (default 5, old files, clamp), tab bar off / info bar at the bottom by default (saved choices kept) |
| `src/search_worker.rs` | 6 | cross-session fuzzy search |
| `src/pty/shell_integration.rs` | 5 | shell prompt integration helpers |
| `src/app/window_zoom.rs` | 5 | egui left alone zooms on ⇧⌘- (the bug), once off egui answers no default chord (no zoom, no close), Ctrl+Q reaches the terminal off macOS, the zoom actions zoom, the zoom keys match exactly (⌘+ by Shift+= or a keypad +; ⇧⌘- only splits) |
| `src/app/ui/command_palette.rs` | 7 | command filtering, action dispatch, unique labels, file finder and New file listed |
| `src/single_instance.rs` | 3 | singleton guard (Windows/Unix) |
| `src/logging.rs` | 10 | log level enum, serde roundtrip, cleanup edge cases, configured level loading |
| `src/updater/mod.rs` | 5 | cask version = Cargo.toml version guard, release parsing per install kind |
| `src/updater/install.rs` | 3 | required release asset per install kind |
| `src/updater/http.rs` | 1 | an unresponsive address (TEST-NET-1) is skipped for the next one within the connect timeout |
| `src/updater/macos/mod.rs` | 12 | bundle-root detection, classification matrix (custom appdir, copies the cask did not install, brew prefix choice), real symlink probe, temp-path owner |
| `src/updater/macos/process.rs` | 11 | stdout/stderr capture to files (never pipes), env, exit errors, kill vs detach on timeout, process-group kill, reaping, process liveness |
| `src/updater/macos/homebrew.rs` | 7 (+1 ignored E2E) | brew steps/args/env, never killed on timeout, post-upgrade version check (newer is fine), recovery hint when the app is gone |
| `src/updater/macos/app_bundle.rs` | 17 (+1 ignored E2E) | atomic swap, undeletable stale backup, ENOTSUP fallback, honest failed-restore message, leftover cleanup (kept when the app is missing), sweep after a cut-off update, real hdiutil-built DMG install, wrong/newer version, garbage image, unwritable folder |
| `src/updater/macos/disk_image.rs` | 12 | tool choice per macOS version (unknown keeps hdiutil), attach/detach arguments for both tools, real attach/detach, timed-out attach and detach-by-path-match with hdiutil and, where the app picks it, diskutil, busy retry (fake tool), mount-point check |
| `src/updater/macos/os_version.rs` | 3 | major-version parsing, unreadable versions, this Mac's version |
| `src/updater/macos/bundle_version.rs` | 4 | Info.plist version read via plutil, semver at-least |
| `src/app/ui/update_pill.rs` | 5 | titlebar pill label/action for every update status |
| `src/app/fonts.rs` | 7 (2 opt-in off macOS) | egui fonts stay first, fallbacks registered once and mapped once, missing/empty files skipped, common symbols + CJK render, every non-ASCII char in source string literals renders, literal scanner |
| `src/renderer_choice.rs` | 7 | default/forced renderer order, `--renderer=` parsing, fallback only for renderer failures |
| `src/app/state.rs` | 15 | pane workspace follows the folder, falls back to the opening workspace, none when deleted; a discard request runs the frame's UI once (egui would run it twice); egui's style follows the app theme, not the OS; the style keeps the app's widget metrics and placeholder colour, a pane of a deleted workspace is under Other |
| `src/ui_kit/containers.rs` | 1 | popup list items fill a list at least as wide as its anchor |
| `src/ui_kit/text.rs` | 2 | a placeholder is drawn in its text box's font (font id and text style) |
| `src/app/ui/pane_renderer.rs` | 5 | drop-preview geometry: join covers the leaf, each edge covers the matching half; the active text box id is read and written without taking egui's context lock twice (no deadlock, also while another thread asks for repaints) |
| `src/app/ui/pane_labels.rs` | 32 | per-content-kind display text (a note: "Notes · <workspace>"), tooltips (a note: where it is kept), pane directory, workspace colour, untitled buffer numbering, the workspace rule (subfolder, outside every workspace, no folder yet, folder over opened-in, files, notes, deleted) |
| `src/app/ui/dialogs.rs` | 1 | unsaved-changes note wording |
| `src/app/macos_menu.rs` | 10 | every titlebar toggle in the menu once, Mac placement and wording (Show/Hide titles, checkmarks, Settings… stays open), shortcut → key equivalent, exact-chord match (Shift not applied), menu state |
| `src/app/view_toggles.rs` | 1 | toggle list has no duplicates |
| `src/app/macos_fullscreen.rs` | 2 | only full-screen windows are touched, names NUL-terminated |
| `src/macos_objc.rs` | 4 | names NUL-terminated, nil receivers ignored, NSString round trip, class definition reused |
| **Total** | **1335 + 2 ignored (macOS)** | macOS-only updater tests are cfg-gated; Linux/Windows run fewer. DMG tests are serialized (`hdiutil_lock`) and cache images in `target/test-dmg-cache` |

## Release Workflow

- Releases are created by `.github/workflows/release.yml`, triggered on `v*` tag push
- Builds run on 3 platforms: Windows (x86_64-pc-windows-msvc), Linux (x86_64-unknown-linux-gnu), macOS (aarch64-apple-darwin)
- **Never create a GitHub release before all builds succeed** — the `release` job depends on `build`, and a `cleanup-on-failure` job deletes any pre-existing release if any build fails
- To release: push a tag (`git tag vX.Y.Z && git push origin vX.Y.Z`) — do NOT create the release manually; let the workflow handle it
- Homebrew cask is `Casks/terminal-studio.rb` in this repo (users tap it by URL). Bump its `version` together with `Cargo.toml` in the release PR; `cask_version_matches_cargo_version` and `scripts/release.sh` fail on a mismatch

## Known Quirks & Gotchas

- egui runs **one UI pass per frame** here (`state::limit_ui_passes`, `max_passes = 1`): UI code writes key presses to PTYs and spawns/closes sessions as it draws, so egui's re-run after `request_discard` would do it twice. A widget that asks for a discard shows with a one-frame-late layout instead, as in egui 0.28
- egui's dark/light style choice is pinned to the app theme (`state::apply_theme_visuals` → `ctx.set_theme`), so the OS switching theme can't swap in egui's unstyled default
- Menus close on any click inside them (egui 0.32+). A menu item that must leave it open (Labels: tick several) uses `egui::menu::SubMenuButton` with `PopupCloseBehavior::CloseOnClickOutside`. Popups go through `egui::Popup` (`Popup::is_id_open/open_id/close_id`); egui's memory popup methods are private
- Translucent colours: always `theme::rgba_unmultiplied()` / `theme::with_alpha()`, never `Color32::from_rgba_unmultiplied` (clippy.toml disallows it): egui 0.32+ premultiplies in gamma space, which made every low-alpha fill 3–4× fainter than the design
- egui's spacing, default text sizes and animation time are pinned in `theme::apply_widget_metrics()` (egui 0.29+ changed the defaults: wider button padding, 20 px widgets, 13 pt text); a new egui default the UI depends on belongs there
- A popup list under a widget goes through `ui_kit::popup_below()` (items fill its width, as 0.28's `popup_below_widget` did); a text box with both a font and a placeholder uses `.font_with_hint()` (egui 0.32+ draws `hint_text` in the body font). Placeholders are drawn in the text colour (`weak_text_color` = `override_text_color`, as in 0.28)
- egui 0.37 caps a checkbox's corner radius (at 30 % of its size), so checkboxes are rounded squares, not the near-circles of 0.28; only egui's experimental theme API could change that
- `TextEdit::frame` takes a `Frame`: use `theme::frameless_text_edit()` for a borderless box (`Frame::NONE` would also drop its padding)
- In unit tests, drive egui frames with `crate::egui_test_frames::run_ui/run_pass`, not `ctx.run_ui` directly (a dropped frame output with unapplied textures panics in debug)
- In tests, use `Session::new_for_test(id, cols, rows)` (3 args, no Context/pty_tx) — available only under `#[cfg(test)]`
- In production, `Session::new` takes 6 args: `(id, cols, rows, cwd, ctx, pty_tx)`
- Mouse SGR coordinates are 1-based (not 0-based like grid internals)
- The `vte 0.13` tee parser in `reader.rs` runs on the raw byte stream — it sees the same bytes as alacritty but independently; keep `CwdPerformer` stateless across calls (it resets `new_cwd`/`new_prompt_ready` each read loop iteration)
- The watcher skips dotfile directories; hidden files inside tracked dirs are still visible
- Foreground process detection is cached with a 500ms TTL in `ForegroundWorker`; UI thread never calls OS APIs directly
- Git status refresh is debounced at 500ms
- Windows DWM, toolhelp, and `CreateMutexW` APIs are accessed via `windows-sys`; keep all such calls behind `#[cfg(target_os = "windows")]`
- `pane_trees` is not serialized — split layout resets on restart
- `grid.cursor.point.line.0` is `i32`; negative values indicate scrollback rows
- `display_offset` is the number of scrollback lines currently shown above the viewport (0 = live view)

# More Instructions for development
- Everything should be gracefully handled 
- Everything should be delegated to correct worker thread to keep main thread as available and snappy as possible. 
- Every UI interaction should be optimized and be snappy and responsive always. 
- App should never crash and everything should be caught and handled gracefully. 
- There should not be any memory leak or missing cleanup pending. 
- Code quality should be top notch. 
- Everything should have its own single responsibilty code. 
- No file should be monolith and everything should be split correctly. 
- Code should be reused as much as possible. 
- Code should be scalable and maintainable. 
- Code should be optimized for performance. 
- We should have 100% test coverage for features/code/path. 
- Our whole end to end development stack should be 100% correct.
- Text should be always visible even with dark background

