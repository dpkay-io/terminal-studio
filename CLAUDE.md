# Terminal Studio — Codebase Reference

GPU-accelerated terminal multiplexer in Rust using egui/eframe (wgpu renderer). Tiling terminal interface with workspaces, file browsing, git integration, and markdown preview.

## Build & Run

```
cargo build              # dev (opt-level 1)
cargo run
cargo build --release    # opt-level 3, LTO=true, codegen-units=1
cargo test               # all unit tests (986 tests)
RUST_LOG=debug cargo run # enable debug logging
```

The Rust version is pinned in `rust-toolchain.toml` (1.99.1); local, CI and release builds all use it. Rust 1.100.0 can't compile `objc2 0.4.1` (macOS, via eframe 0.28 → winit 0.29). Move the pin in its own PR once build + clippy pass on the new version.

## Module Map

### Core

| File | Responsibility |
|------|---------------|
| `src/main.rs` | Entry point; eframe setup, 1280×800 viewport, decorations=false; calls `SingleInstanceGuard::try_acquire()`; tries renderers in order (wgpu, then glow) |
| `src/renderer_choice.rs` | Renderer order, `--renderer=` override, and `another_renderer_may_help()`: no glow retry when the window system or app creation failed (that would mask the real error with winit's "EventLoop can't be recreated") |
| `src/app.rs` | `App` struct; implements `eframe::App`; render dispatch to UI submodules |
| `src/theme.rs` | Design language tokens: spacing (SP_0–SP_6), radii (R_NONE–R_LG), typography (FONT_*), alpha/blend constants, 15-theme palette, semantic colors, WCAG contrast helpers |
| `src/workspace.rs` | `WorkspaceStore`, `NoteStore`, `Workspace`, `WindowId` — JSON persistence |
| `src/pane_tree.rs` | `PaneNode` tree (Leaf/Split), `SplitDir`, `split_rect()` — recursive pane splitting within tabs |
| `src/logging.rs` | `LogLevel` enum, `LogGuard`; non-blocking daily-rotated file logging via tracing-appender; 7-day retention cleanup; `load_configured_level()` partial settings parse |
| `src/single_instance.rs` | `SingleInstanceGuard`: Windows `CreateMutexW` / Unix `flock`; bypass with `--no-singleton` |
| `src/util.rs` | `safe_json_load()`, `atomic_write()`, directory expansion, panic handler |

### Terminal & PTY

| File | Responsibility |
|------|---------------|
| `src/terminal/mod.rs` | `Session` struct wrapping `Term<EventProxy>`; `EventProxy` (`EventListener` impl); `TermSize` (`Dimensions` impl) |
| `src/terminal/tests.rs` | 12 terminal emulator tests using `alacritty_terminal` APIs |
| `src/pty/mod.rs` | `SessionManager`: spawn/resize PTY sessions; `ShellKind` enum; each session gets dedicated reader/writer threads |
| `src/pty/reader.rs` | Dedicated reader thread per PTY: tee-parses OSC 7 with `vte 0.13`, feeds rest to alacritty `Processor` in 4KB chunks |
| `src/pty/shell_integration.rs` | Shell prompt integration helpers; OSC 7 injection for bash/zsh/fish |
| `src/pty/foreground.rs` | Platform-specific foreground process detection (Windows toolhelp / Linux /proc) |
| `src/pty/foreground_worker.rs` | `ForegroundWorker`: background thread polling foreground detection every 500ms; UI reads from cache |
| `src/renderer/mod.rs` | Re-export of `terminal_pass` |
| `src/renderer/terminal_pass.rs` | `TerminalView`, `TerminalGeometry`, `SelectionRange` — cell rendering using `alacritty_terminal` grid API |

### App State & Logic (`src/app/`)

| File | Responsibility |
|------|---------------|
| `src/app/state.rs` | Core app state initialization; session/pane/workspace restoration from persisted JSON; `pane_group()` / `pane_workspace()`: a pane's workspace by its folder first (single source for tab grouping, sidebar, titlebar, info bar) |
| `src/app/fonts.rs` | Font definitions: egui's fonts first, then memory-mapped system fallbacks per OS (symbols, dingbats ✓✕✗, CJK, Hangul) so those don't render as boxes |
| `src/app/pane_state.rs` | `PaneState`: pane list management, active pane tracking, next ID counters, tree management; `shown_pane_ids()` (the tab each split on screen shows) |
| `src/app/deferred_terminals.rs` | `start_shown_restored_sessions()`: a restored session waits as `DeferredTerminal` until a split shows it — any split, not only the focused one — then starts with its saved output, name and pending command |
| `src/app/pane.rs` | `PaneEntry`, `SessionEntry`, `PaneContent` enum, `TermSelection` struct; `FileEditorState::write_to_disk()` (the one save path), `has_unsaved_changes()`, and `apply_edit()` / `apply_file_text()` / `mark_saved()`, which compare the buffer to a hash of the file's text so undoing back to it clears the `*` |
| `src/app/close_guard.rs` | `request_close_tab()` / `TabClose`: every single-tab close (header ✖, ⇧⌘W, pane menu, session list ✕/middle click/menu) asks "Save changes?" first when the tab has unsaved edits; `has_unsaved_changes()` / `unsaved_count()` |
| `src/app/session_state.rs` | `SessionState`: session list management, find/add/remove operations |
| `src/app/settings.rs` | `AppSettings`, `CursorStyle` enum; user settings (theme, fonts, cursor, scrollback); JSON persistence |
| `src/app/input.rs` | `key_to_pty_bytes()`: keyboard→PTY byte translation (Ctrl+A–Z, arrows, function keys, modifiers) |
| `src/app/title.rs` | `effective_title()`: workspace+session title formatting; `session_title()`: a user-given (pinned) name word for word, else `effective_title` — behind `App::session_label()`, the one terminal name for rows, split headers, window title and saved sessions; shell escape helpers |
| `src/app/feedback.rs` | `FlashManager`, `FlashTarget`, `FlashKind`: subtle UI flash feedback (copy, paste, error) with auto-fade |
| `src/app/markdown.rs` | `render_markdown()`: headers, bold, code blocks, lists, blockquotes, links |
| `src/app/persistence.rs` | `AppSession`, `SavedPane`, `SavedPaneContent`, `SavedSession`; JSON serialization; session restore. A restored session that was never opened keeps its scrollback file across saves (`SavedPaneContent::DeferredTerminal.scrollback_file`); running sessions' files are written under a per-save stamp so they never overwrite it |
| `src/app/multi_window.rs` | `ExtraWindow`, `WindowState`, `WindowView`; egui multi-viewport for workspaces |
| `src/app/workspace_ui.rs` | Workspace management UI; preset color picker; new/edit/delete dialogs |
| `src/app/worker_manager.rs` | Thread lifecycle management; cleanup helpers |
| `src/app/layout_shortcuts.rs` | `App::apply_layout_shortcut()` / `AppAction::arranges_layout()`: split, close tab, show/hide side panels, previous/next and numbered tabs — the one handler for the terminal key loop and for an active file editor (which otherwise keeps its keys for typing) |
| `src/app/view_toggles.rs` | `VIEW_TOGGLES`, `App::toggle_view()` / `view_is_open()`: the view's show/hide actions (sidebar, explorer, switcher, recently closed, settings, shortcut sheet, and the tab bar, which has no titlebar button) — the single path for titlebar buttons, shortcuts, command palette, the macOS menu bar and the Settings checkbox |
| `src/app/macos_fullscreen.rs` | macOS only: in native full screen AppKit puts the title bar winit adds for the switch (title + traffic lights) in a child strip window (`NSToolbarFullScreenWindow`) that slides down under the menu bar on hover, over the app's title bar. On every entry (and at launch for a restored window) that strip is emptied and made click-through; window style and the menu bar are left alone |
| `src/app/macos_menu.rs` | macOS only: adds Settings… to the app menu, a View menu (Hide/Show Sidebar, Hide/Show Explorer, Hide/Show Tab Bar, Switcher, Recently Closed Sessions) and Help → Keyboard Shortcuts, each showing its current shortcut. The menu never claims those keys (an item disables itself while AppKit matches a key press against its own chord), so the app's own key handling stays the only one. Clicks are queued and run by the focused window's frame |

### File System & Git (`src/app/`)

| File | Responsibility |
|------|---------------|
| `src/app/watcher.rs` | `WatchState`, `WatchCommand`, `WatchResult`; background file system watcher (notify crate); debounced git refresh |
| `src/app/file_browser.rs` | `FileEntry`, `DirData`, `SubdirCache`; directory tree listing; file entry sorting/caching; `.gitignore` parsing |
| `src/app/git_diff.rs` | `GitDiffState`; git diff rendering, staged/unstaged sections, inline hunks, side-by-side toggle |
| `src/app/git_worker.rs` | `GitWorker`: background thread for git operations (info, stage/unstage, commit, push, diff) |
| `src/app/workspace_git_worker.rs` | Workspace-scoped git worker for per-workspace git state tracking |
| `src/app/diff_parser.rs` | `DiffHunk`, `DiffLine`, `DiffViewMode`, `SideBySideLine`; unified diff format parser |
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
| `src/app/ui/pane_labels.rs` | `tab_display_text()`, `tab_tooltip_text()`, `ws_color_for_pane()`, `pane_cwd()`: display text, tooltip, colour and path derivation for a pane — the one source for the session list row, the split header and dialogs, so an edited file reads `* name` (`! name` after a failed save) everywhere. No rendering |
| `src/app/ui/tab_bar.rs` | The optional row of tabs above each split (`settings.show_tab_bar`, on by default; Settings, View menu, command palette, ⌥⇧⌘T / Ctrl+Alt+Shift+T). One tab per tab the split holds, in its order: click shows it, double-click renames a terminal, ✕ / middle click close it like the split's ✖, right-click gives the session-list row's menu (`App::pane_menu`), drag reorders it, moves it into a gap in another split's tab bar (`DropTarget::GroupTabBar`) or splits it off at an edge. Actions run through the session list's `process_tab_actions`; the info bar below keeps the split buttons |
| `src/app/ui/info_bar.rs` | The pane header for each split: identity text (ellipsised, full text on hover), position toggle, hide, split ⊞ ⊟, ✖ closes the tab the split shows (right-click: close all tabs in the split, confirmed when more than one); `info_bar_layout()` places the buttons and gives the text the rest. Position/hidden is per split (`EditorGroup.info_bar`, saved with the group layout; unset follows `settings.info_bar_position`, which is the place the user last moved a bar to — hiding one doesn't change it, and a revealed bar goes there). A hidden bar's ℹ reveal button only appears while the pointer is in the pane's top-right corner |
| `src/app/ui/pane_renderer.rs` | `RenderCtx`, `PaneContextAction`; recursive pane tree renderer; terminal/editor/diff/notes dispatch |
| `src/app/ui/command_palette.rs` | Command palette: action search/filter/dispatch; keyboard-driven execution |
| `src/app/ui/dialogs.rs` | Modal dialogs: new session, new workspace, close confirmation, "Save changes?" for one unsaved tab, git commit/push, about. `request_close_all()` asks when a close covers more than one tab or any unsaved one; one saved tab closes at once. Close-all and quit say how many files have unsaved changes |
| `src/app/ui/settings_overlay.rs` | Settings panel: theme picker, cursor style, font size, scrollback, keybinding editor |
| `src/app/ui/search_bar.rs` | Reusable search input widget; persistent focus tracking |
| `src/app/ui/debounce.rs` | `Debouncer<T>`: time-based debouncing for input fields |
| `src/app/ui/left_panel/session_list.rs` | The single session navigator: split sections, three-state row highlighting, row chrome and interactions, "+New" menu, fuzzy filter, workspace filter dropdown. Row menu: Rename (pins the name on the session — `Session::pin_title` — so prompt hooks and programs can't replace it; an empty name goes back to the automatic one; a restored session not started yet is renamed too, and the pin applies when it starts; saved with the session and closed-session records), Copy path, Show in Finder/File Explorer (`util::REVEAL_IN_FILE_MANAGER_LABEL`) |
| `src/app/ui/left_panel/sections.rs` | `section_labels()`, `SortCache`, `RowState`: split-section grouping and naming, frozen recency ordering (rows hold their place while hovered; closed ones drop out, new ones append), row prominence. Pure — no `egui::Ui`, so it is unit-testable |
| `src/app/ui/left_panel/workspace_section.rs` | Workspace cards; collapse toggle; color swatch; git branch/diff badges |
| `src/app/ui/left_panel/context_menu.rs` | Right-click context menu: rename, edit color, delete, duplicate, open in new window |

### UI Kit (`src/ui_kit/`)

| File | Responsibility |
|------|---------------|
| `src/ui_kit/buttons.rs` | `IconButton`, `ActionButton`, `IconButtonStyle`, `ActionButtonStyle`; themed button wrappers |
| `src/ui_kit/containers.rs` | `dialog()`, `DialogConfig`, `DialogResponse`, `DialogAnchor`, `DialogWidth`; flex-box layouts |
| `src/ui_kit/layout.rs` | Grid/flex helpers; responsive layout utilities |
| `src/ui_kit/lists.rs` | Scrollable list rendering with selection tracking |
| `src/ui_kit/text.rs` | Rich text formatting, emphasis, code inline styling; `one_line_elided()` (one line ending in …) and `full_hover_text()` (a tooltip that never wraps, so it grows when its text does) |

### Utilities & Infrastructure

| File | Responsibility |
|------|---------------|
| `src/shortcuts.rs` | `AppAction` enum (50+ actions), `ShortcutRegistry`, `Shortcut`, `ShortcutGroup`; keybinding resolution |
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
- Each PTY has a dedicated `pty-reader-N` thread: reads bytes → tee CWD → alacritty `Processor::advance` under write lock → `ctx.request_repaint_after(8ms)`
- Each PTY has a dedicated `pty-writer-N` thread: drains `mpsc::Receiver<Vec<u8>>` → writes to PTY master
- UI sends input via `SessionEntry.pty_tx: mpsc::Sender<Vec<u8>>`; alacritty's `PtyWrite` events go through the same channel
- `Session` state is `Arc<RwLock<Session>>` — shared between reader thread and UI thread
- `alive` flag is `Arc<AtomicBool>` — signals reader thread to stop
- Background singleton threads (each uses `Arc<AtomicBool> alive` + `Arc<Mutex<>> results`):
  - `ForegroundWorker`: process detection polling (500ms)
  - `WatchState`: file system monitoring + git refresh debounce
  - `GitWorker`: git operations queue (stage/unstage/commit/push/diff)
  - `SearchWorker`: cross-session fuzzy search
  - `FileSearchWorker`: file path fuzzy search
  - `UpdateChecker`: GitHub releases polling (24h interval)
  - `SysMonitor`: CPU/RAM/network stats

**Pane tree (in-tab splits):**
- `App.pane_trees: HashMap<tab_id, PaneNode>` — one tree per tab
- `PaneNode` is a recursive enum: `Leaf { pane_id, last_size }` or `Split { split_id, dir, ratio, a, b }`
- `render_node()` in `app.rs` recurses the tree, calling `split_rect()` to divide screen space and rendering each leaf
- 4px interactive dividers support drag-to-resize (ratio clamped to [0.1, 0.9])
- Keyboard shortcuts: `Ctrl+Shift+\` (horizontal split), `Ctrl+Shift+-` (vertical split), `Ctrl+Shift+W` (close pane), `Alt+Arrow` (focus movement), `Ctrl+Shift+[` / `Ctrl+Shift+]` (⇧⌘[ / ⇧⌘] on macOS: previous/next pane **within the focused split only**). These work from a file editor too (`layout_shortcuts.rs`)
- Each split has an optional tab bar (`tab_bar.rs`, on by default) above its info bar. The left nav session list shows every tab either way — it groups rows into split sections and marks which panes are on screen. Rename / close / copy-path / reveal / labels are the same in both (`pane_menu`, `render_rename_field`, `process_tab_actions`)
- Drag-and-drop: dropping on the middle of a split joins that split; dropping in the outer third of any edge splits it (`edge_zone()` in `src/app/drag.rs`); dropping on a left-nav section header joins that split
- **Note:** `pane_trees` is not persisted to disk — splits reset on restart

**Multi-window (multi-viewport):**
- `App.extra_windows: Vec<WindowState>` — additional egui viewports
- Each viewport renders a specific workspace via `ctx.show_viewport_deferred(...)`
- Single-instance enforcement: `SingleInstanceGuard::try_acquire()` at startup (bypass with `--no-singleton`)

**Persistence:**
- JSON files in `%APPDATA%\terminal-studio\` (Windows) or `~/.config/terminal-studio/` (Unix)
- `session.json`: open panes, CWDs, active workspace, panel layout
- `workspaces.json`: workspace definitions (name, path, color)
- `notes.json`: per-workspace scratch-pad notes
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
eframe = "0.28"              # wgpu, default_fonts, persistence
egui = "0.28"
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
```

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
- Radii: `R_NONE` (0), `R_SM` (2), `R_MD` (4), `R_LG` (6)
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
- Render calls dispatch from `app.rs` → `update()`

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
| `src/theme.rs` | 29 | color roundtrip, tinted, short_path, header_bg, text contrast, all-theme validation, sRGB LUT, ensure_term_contrast |
| `src/app/input.rs` | 30 | keyboard→PTY byte translation for all keys and modifier combinations, Ctrl+Tab detection |
| `src/app/pane_state.rs` | 78 | each split's shown tab, pane CRUD, tree operations (split/nested-split), remove, ratio mutation, size updates, group detach/erase/collapse, orphaned-layout pruning, Ctrl+Tab cycling within a group, per-split info bar position, moving one sets the default for new splits (others keep theirs) |
| `src/app/drag.rs` | 76 | drag payload/target resolution, edge-zone geometry, split-edge drops, tab-bar drops (reorder, move to a gap in another split, open new things there), one-pane-per-session and per-file delegation, per-frame cross-window drop harvesting |
| `src/editor_group.rs` | 54 | group layout tree: split (either side), remove, collapse, group id ordering |
| `src/app/ui/left_panel/sections.rs` | 17 | split-section labels, frozen sort order (close/open while hovered keeps rows in place), three-state row prominence |
| `src/app/ui/info_bar.rs` | 8 | position serde, button layout order/overlap, text never under a button, narrow split, reveal hot zone |
| `src/app/title.rs` | 21 | title formatting with workspaces, effective title, a pinned name shown as typed (even "zsh"), shell escaping |
| `src/pane_tree.rs` | 19 | leaf IDs, split/nested-split, remove (all cases), ratio mutation, split_rect geometry |
| `src/app/diff_parser.rs` | 18 | unified diff parsing, hunk extraction, line kind classification |
| `src/git/parser.rs` | 17 | git status --porcelain parsing, C-style unquoting, file status kinds |
| `src/workspace.rs` | 17 | store CRUD, find_for_cwd, find_for_path, unique name, note store |
| `src/app/workspace_ui.rs` | 5 | open-folder dialog: Save as Workspace on outside workspaces / off inside one / existing kept, default name and colour always free, generated colours distinct |
| `src/app/watcher.rs` | 16 | file system watch, git refresh debounce, directory caching |
| `src/terminal/tests.rs` | 12 | session dims, resize, content preservation, OSC title, a user-given (pinned) title outlasting the shell's, cursor, bracketed paste, mouse SGR |
| `src/search.rs` | 11 | text search, match finding, index navigation |
| `src/renderer/terminal_pass.rs` | 10 | terminal geometry, cell rendering, scrollbar hit-testing, selection |
| `src/pty/reader.rs` | 10 | OSC 7 CWD tracking, OSC 52 clipboard, byte streaming |
| `src/syntax.rs` | 10 | syntax highlighting, language detection, theme building |
| `src/app/git_worker.rs` | 10 | git info, stage/unstage, commit, push operations |
| `src/app/ui/debounce.rs` | 9 | time-based debounce logic |
| `src/app/persistence.rs` | 27 | session serialization, restore logic, per-split info bar position, pinned session names, untitled buffers (old files load) |
| `src/app/pane.rs` | 15 | pane content enum, terminal selection, editor save (written, untitled never written, failure keeps edits), undoing back to the saved text leaves nothing unsaved |
| `src/app/ui/tab_bar.rs` | 3 | drop gap from the pointer, hidden tabs keep their place in the order, empty bar |
| `src/pty/mod.rs` | 8 | shell detection, session manager operations |
| `src/app/workspace_git_worker.rs` | 8 | workspace-scoped git state |
| `src/file_search_worker.rs` | 8 | file fuzzy search, result filtering |
| `src/app/feedback.rs` | 7 | trigger/tick, flash expiry, alpha decay, duplicate replacement, color generation |
| `src/app/markdown.rs` | 7 | markdown rendering (headers, code, lists, blockquotes) |
| `src/md_detector.rs` | 7 | markdown path detection |
| `src/util.rs` | 12 | atomic write, JSON loading, directory expansion, git root, parent dir of a bare name |
| `src/app/file_browser.rs` | 10 | directory listing, file entry sorting, gitignore parsing |
| `src/shortcuts.rs` | 24 | keybinding parsing, action resolution, layout chords never use a text-editing key |
| `src/keybindings.rs` | 6 | keybinding config loading, JSON handling |
| `src/app/settings.rs` | 6 | settings persistence, JSON config |
| `src/search_worker.rs` | 6 | cross-session fuzzy search |
| `src/pty/shell_integration.rs` | 5 | shell prompt integration helpers |
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
| `src/app/state.rs` | 12 | pane workspace follows the folder, falls back to the opening workspace, none when deleted; saving scrollback keeps the files restored-but-unopened sessions still need, new names never reuse old ones |
| `src/app/ui/pane_renderer.rs` | 3 | drop-preview geometry: join covers the leaf, each edge covers the matching half |
| `src/app/ui/pane_labels.rs` | 28 | per-content-kind display text, tooltips, pane directory, workspace colour, untitled buffer numbering |
| `src/app/ui/dialogs.rs` | 1 | unsaved-changes note wording |
| `src/app/macos_menu.rs` | 10 | every titlebar toggle in the menu once, Mac placement and wording (Show/Hide titles, checkmarks, Settings… stays open), shortcut → key equivalent, exact-chord match (Shift not applied), menu state |
| `src/app/view_toggles.rs` | 1 | toggle list has no duplicates |
| `src/app/macos_fullscreen.rs` | 2 | only full-screen windows are touched, names NUL-terminated |
| `src/macos_objc.rs` | 4 | names NUL-terminated, nil receivers ignored, NSString round trip, class definition reused |
| **Total** | **986 + 2 ignored (macOS)** | macOS-only updater tests are cfg-gated; Linux/Windows run fewer. DMG tests are serialized (`hdiutil_lock`) and cache images in `target/test-dmg-cache` |

## Release Workflow

- Releases are created by `.github/workflows/release.yml`, triggered on `v*` tag push
- Builds run on 3 platforms: Windows (x86_64-pc-windows-msvc), Linux (x86_64-unknown-linux-gnu), macOS (aarch64-apple-darwin)
- **Never create a GitHub release before all builds succeed** — the `release` job depends on `build`, and a `cleanup-on-failure` job deletes any pre-existing release if any build fails
- To release: push a tag (`git tag vX.Y.Z && git push origin vX.Y.Z`) — do NOT create the release manually; let the workflow handle it
- Homebrew cask is `Casks/terminal-studio.rb` in this repo (users tap it by URL). Bump its `version` together with `Cargo.toml` in the release PR; `cask_version_matches_cargo_version` and `scripts/release.sh` fail on a mismatch

## Known Quirks & Gotchas

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

