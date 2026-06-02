# Terminal Studio — Codebase Reference

GPU-accelerated terminal multiplexer in Rust using egui/eframe (wgpu renderer). Tiling terminal interface with workspaces, file browsing, git integration, and markdown preview.

## Build & Run

```
cargo build              # dev (opt-level 1)
cargo run
cargo build --release    # opt-level 3, LTO=true, codegen-units=1
cargo test               # all unit tests (333 tests)
RUST_LOG=debug cargo run # enable debug logging
```

## Module Map

### Core

| File | Responsibility |
|------|---------------|
| `src/main.rs` | Entry point; eframe setup, 1280×800 viewport, wgpu renderer, decorations=false; calls `SingleInstanceGuard::try_acquire()` |
| `src/app.rs` | `App` struct; implements `eframe::App`; render dispatch to UI submodules |
| `src/theme.rs` | Design language tokens: spacing (SP_0–SP_6), radii (R_NONE–R_LG), typography (FONT_*), alpha/blend constants, 15-theme palette, semantic colors, WCAG contrast helpers |
| `src/workspace.rs` | `WorkspaceStore`, `NoteStore`, `Workspace`, `WindowId` — JSON persistence |
| `src/pane_tree.rs` | `PaneNode` tree (Leaf/Split), `SplitDir`, `split_rect()` — recursive pane splitting within tabs |
| `src/single_instance.rs` | `SingleInstanceGuard`: Windows `CreateMutexW` / Unix `flock`; bypass with `--no-singleton` |
| `src/util.rs` | `safe_json_load()`, `atomic_write()`, directory expansion, panic handler |

### Terminal & PTY

| File | Responsibility |
|------|---------------|
| `src/terminal/mod.rs` | `Session` struct wrapping `Term<EventProxy>`; `EventProxy` (`EventListener` impl); `TermSize` (`Dimensions` impl) |
| `src/terminal/tests.rs` | 11 terminal emulator tests using `alacritty_terminal` APIs |
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
| `src/app/state.rs` | Core app state initialization; session/pane/workspace restoration from persisted JSON |
| `src/app/pane_state.rs` | `PaneState`: pane list management, active pane tracking, next ID counters, tree management |
| `src/app/pane.rs` | `PaneEntry`, `SessionEntry`, `PaneContent` enum, `TermSelection` struct |
| `src/app/session_state.rs` | `SessionState`: session list management, find/add/remove operations |
| `src/app/settings.rs` | `AppSettings`, `CursorStyle` enum; user settings (theme, fonts, cursor, scrollback); JSON persistence |
| `src/app/input.rs` | `key_to_pty_bytes()`: keyboard→PTY byte translation (Ctrl+A–Z, arrows, function keys, modifiers) |
| `src/app/title.rs` | `effective_title()`: workspace+session title formatting; shell escape helpers |
| `src/app/feedback.rs` | `FlashManager`, `FlashTarget`, `FlashKind`: subtle UI flash feedback (copy, paste, error) with auto-fade |
| `src/app/markdown.rs` | `render_markdown()`: headers, bold, code blocks, lists, blockquotes, links |
| `src/app/persistence.rs` | `AppSession`, `SavedPane`, `SavedPaneContent`, `SavedSession`; JSON serialization; session restore |
| `src/app/multi_window.rs` | `ExtraWindow`, `WindowState`, `WindowView`; egui multi-viewport for workspaces |
| `src/app/workspace_ui.rs` | Workspace management UI; preset color picker; new/edit/delete dialogs |
| `src/app/worker_manager.rs` | Thread lifecycle management; cleanup helpers |

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
| `src/app/ui/titlebar.rs` | Window title bar with workspace color tint, update button, toolbar, focus tracking |
| `src/app/ui/tab_bar.rs` | `TabBarResult`, `render_tab_bar()`: horizontally scrollable tab strip, split group indicators |
| `src/app/ui/pane_renderer.rs` | `RenderCtx`, `PaneContextAction`; recursive pane tree renderer; terminal/editor/diff/notes dispatch |
| `src/app/ui/command_palette.rs` | Command palette: action search/filter/dispatch; keyboard-driven execution |
| `src/app/ui/dialogs.rs` | Modal dialogs: new session, new workspace, close confirmation, git commit/push, about |
| `src/app/ui/settings_overlay.rs` | Settings panel: theme picker, cursor style, font size, scrollback, keybinding editor |
| `src/app/ui/search_bar.rs` | Reusable search input widget; persistent focus tracking |
| `src/app/ui/debounce.rs` | `Debouncer<T>`: time-based debouncing for input fields |
| `src/app/ui/left_panel/session_list.rs` | Session list; "+New" menu; fuzzy filter; workspace filter dropdown |
| `src/app/ui/left_panel/workspace_section.rs` | Workspace cards; collapse toggle; color swatch; git branch/diff badges |
| `src/app/ui/left_panel/context_menu.rs` | Right-click context menu: rename, edit color, delete, duplicate, open in new window |

### UI Kit (`src/ui_kit/`)

| File | Responsibility |
|------|---------------|
| `src/ui_kit/buttons.rs` | `IconButton`, `ActionButton`, `IconButtonStyle`, `ActionButtonStyle`; themed button wrappers |
| `src/ui_kit/containers.rs` | `dialog()`, `DialogConfig`, `DialogResponse`, `DialogAnchor`, `DialogWidth`; flex-box layouts |
| `src/ui_kit/layout.rs` | Grid/flex helpers; responsive layout utilities |
| `src/ui_kit/lists.rs` | Scrollable list rendering with selection tracking |
| `src/ui_kit/text.rs` | Rich text formatting, emphasis, code inline styling |

### Utilities & Infrastructure

| File | Responsibility |
|------|---------------|
| `src/shortcuts.rs` | `AppAction` enum (50+ actions), `ShortcutRegistry`, `Shortcut`, `ShortcutGroup`; keybinding resolution |
| `src/keybindings.rs` | `KeyBinding`, `KeybindingsConfig`; JSON-based custom keybindings; default binding set |
| `src/syntax.rs` | Syntax highlighting via syntect; language detection; theme building from app theme |
| `src/url_detector.rs` | `DetectedUrl`; regex-based URL detection in terminal output; clickable links |
| `src/md_detector.rs` | `DetectedMdPath`; markdown file detection and path tracking |
| `src/updater.rs` | `UpdateStatus`, `UpdateChecker`; background GitHub releases API polling; self-update |
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
- Keyboard shortcuts: `Ctrl+Shift+\` (horizontal split), `Ctrl+Shift+-` (vertical split), `Ctrl+Shift+W` (close pane), `Alt+Arrow` (focus movement)
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
portable-pty = "0.8"         # ConPTY (Windows) / openpty (Unix)

# Concurrency & error handling
parking_lot = "0.12"         # RwLock for session state
anyhow = "1"

# Logging
log = "0.4"
env_logger = "0.11"

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
| `src/app/input.rs` | 26 | keyboard→PTY byte translation for all keys and modifier combinations |
| `src/app/pane_state.rs` | 20 | pane CRUD, tree operations (split/nested-split), remove, ratio mutation, size updates |
| `src/app/title.rs` | 20 | title formatting with workspaces, effective title, shell escaping |
| `src/pane_tree.rs` | 19 | leaf IDs, split/nested-split, remove (all cases), ratio mutation, split_rect geometry |
| `src/app/diff_parser.rs` | 18 | unified diff parsing, hunk extraction, line kind classification |
| `src/git/parser.rs` | 17 | git status --porcelain parsing, C-style unquoting, file status kinds |
| `src/workspace.rs` | 16 | store CRUD, find_for_cwd, find_for_path, note store |
| `src/app/watcher.rs` | 16 | file system watch, git refresh debounce, directory caching |
| `src/terminal/tests.rs` | 11 | session dims, resize, content preservation, OSC title, cursor, bracketed paste, mouse SGR |
| `src/search.rs` | 11 | text search, match finding, index navigation |
| `src/renderer/terminal_pass.rs` | 10 | terminal geometry, cell rendering, scrollbar hit-testing, selection |
| `src/pty/reader.rs` | 10 | OSC 7 CWD tracking, OSC 52 clipboard, byte streaming |
| `src/syntax.rs` | 10 | syntax highlighting, language detection, theme building |
| `src/app/git_worker.rs` | 10 | git info, stage/unstage, commit, push operations |
| `src/app/ui/debounce.rs` | 9 | time-based debounce logic |
| `src/app/persistence.rs` | 9 | session serialization, restore logic |
| `src/app/pane.rs` | 8 | pane content enum, terminal selection |
| `src/pty/mod.rs` | 8 | shell detection, session manager operations |
| `src/app/workspace_git_worker.rs` | 8 | workspace-scoped git state |
| `src/file_search_worker.rs` | 8 | file fuzzy search, result filtering |
| `src/app/feedback.rs` | 7 | trigger/tick, flash expiry, alpha decay, duplicate replacement, color generation |
| `src/app/markdown.rs` | 7 | markdown rendering (headers, code, lists, blockquotes) |
| `src/md_detector.rs` | 7 | markdown path detection |
| `src/util.rs` | 7 | atomic write, JSON loading, directory expansion |
| `src/app/file_browser.rs` | 10 | directory listing, file entry sorting, gitignore parsing |
| `src/shortcuts.rs` | 6 | keybinding parsing, action resolution |
| `src/keybindings.rs` | 6 | keybinding config loading, JSON handling |
| `src/app/settings.rs` | 6 | settings persistence, JSON config |
| `src/search_worker.rs` | 6 | cross-session fuzzy search |
| `src/pty/shell_integration.rs` | 5 | shell prompt integration helpers |
| `src/app/ui/command_palette.rs` | 4 | command filtering, action dispatch |
| `src/single_instance.rs` | 3 | singleton guard (Windows/Unix) |
| **Total** | **333** | |

## Release Workflow

- Releases are created by `.github/workflows/release.yml`, triggered on `v*` tag push
- Builds run on 3 platforms: Windows (x86_64-pc-windows-msvc), Linux (x86_64-unknown-linux-gnu), macOS (aarch64-apple-darwin)
- **Never create a GitHub release before all builds succeed** — the `release` job depends on `build`, and a `cleanup-on-failure` job deletes any pre-existing release if any build fails
- To release: push a tag (`git tag vX.Y.Z && git push origin vX.Y.Z`) — do NOT create the release manually; let the workflow handle it

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

