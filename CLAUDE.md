# Terminal Studio — Codebase Reference

GPU-accelerated terminal multiplexer in Rust using egui/eframe (wgpu renderer). Tiling terminal interface with workspaces, file browsing, git integration, and markdown preview.

## Build & Run

```
cargo build              # dev (opt-level 1)
cargo run
cargo build --release    # opt-level 3, LTO=true, codegen-units=1
cargo test               # all unit tests (261 tests)
RUST_LOG=debug cargo run # enable debug logging
```

## Module Map

| File | Responsibility |
|------|---------------|
| `src/main.rs` | Entry point; eframe setup, 1280×800 viewport, wgpu renderer, decorations=false; calls `SingleInstanceGuard::try_acquire()` |
| `src/app.rs` | `App` struct; ALL UI logic, state management, persistence |
| `src/theme.rs` | Design language tokens: spacing (SP_0–SP_6), radii (R_NONE–R_LG), typography (FONT_*), alpha/blend constants, 15-theme palette, semantic colors, WCAG contrast helpers |
| `src/app/feedback.rs` | `FlashManager`: subtle UI flash feedback system (copy, paste, errors) with auto-fade |
| `src/terminal/mod.rs` | `Session` struct wrapping `Term<EventProxy>`; `EventProxy` (`EventListener` impl); `TermSize` (`Dimensions` impl) |
| `src/terminal/tests.rs` | 11 terminal emulator tests using `alacritty_terminal` APIs |
| `src/pty/mod.rs` | `SessionManager`: spawn/resize PTY sessions; each session gets a dedicated pty-writer-N thread |
| `src/pty/reader.rs` | Dedicated reader thread per PTY: tee-parses OSC 7 with `vte 0.13`, feeds rest to alacritty `Processor` in 4KB chunks |
| `src/pty/foreground.rs` | Platform-specific foreground process detection (Windows toolhelp / Linux /proc) |
| `src/pty/foreground_worker.rs` | `ForegroundWorker`: background thread polling foreground detection every 500ms; UI reads from cache |
| `src/renderer/mod.rs` | Re-export of `terminal_pass` |
| `src/renderer/terminal_pass.rs` | `TerminalView`, `TerminalGeometry` — cell rendering using `alacritty_terminal` grid API |
| `src/workspace.rs` | `WorkspaceStore`, `NoteStore` — JSON persistence |
| `src/pane_tree.rs` | `PaneNode` tree (Leaf/Split), `SplitDir`, `split_rect()` — recursive pane splitting within tabs |
| `src/single_instance.rs` | `SingleInstanceGuard`: Windows `CreateMutexW` / Unix `flock`; bypass with `--no-singleton` |

## Key Types & IDs

- Pane IDs: `u32`
- Session IDs: `u32`
- Workspace IDs: `u64`
- Split IDs: `u32` (unique within `App.next_split_id`)
- `PaneContent` is either `Terminal(session_id: u32)` or `FileEditor(FileEditorState)`
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
- Foreground process detection runs in a single `foreground-detector` background thread (`ForegroundWorker`); UI reads cached results

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
- `WorkspaceStore` / `NoteStore` serialize to JSON via serde

**File watching:**
- `notify::RecommendedWatcher` with a channel-based event loop
- Only tracks non-hidden directories (no dotfiles)
- Git diff via `std::process::Command("git")`; refresh debounce is 500ms

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
alacritty_terminal = "0.26"  # full terminal emulator (grid, parser, modes)
eframe = "0.28"              # wgpu, default_fonts, persistence
egui = "0.28"
vte = "0.13"                 # tee parser for OSC 7 CWD extraction
portable-pty = "0.8"         # ConPTY (Windows) / openpty (Unix)
serde + serde_json = "1"
parking_lot = "0.12"         # RwLock for session state
notify = "6"                 # file system watching
anyhow = "1"
log = "0.4"
env_logger = "0.11"
windows-sys = "0.52"         # Windows-only: toolhelp, DWM, CreateMutexW, window messages
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
- Edit `app.rs` → `update()` method

**Add a new theme color:**
1. Add constant to `theme.rs`
2. Reference via `crate::theme::NEW_COLOR` in `app.rs` or renderer

**Add a new workspace field:**
1. Update `Workspace` struct in `workspace.rs`
2. Update `SavedSession` / `AppSession` in `app.rs`

**Add platform-specific behavior:**
- Gate with `#[cfg(target_os = "windows")]` / `#[cfg(not(target_os = "windows"))]`

## Test Coverage

| File | Tests | Coverage |
|------|-------|---------|
| `src/terminal/tests.rs` | 11 | session dims, resize, content preservation, OSC 0/2 title, cursor movement, bracketed paste, mouse click/SGR, cursor visibility, bold SGR |
| `src/pane_tree.rs` | 13 | leaf IDs, split/nested-split, remove (all cases), ratio mutation, update_size, split_rect geometry |
| `src/workspace.rs` | 11 | store CRUD, find_for_cwd, find_for_path, note store |
| `src/theme.rs` | 22 | color roundtrip, tinted, short_path, header_bg, text contrast, all-theme validation, sRGB LUT, ensure_term_contrast, ensure_readable |
| `src/app/feedback.rs` | 6 | trigger/tick, flash expiry, alpha decay, duplicate replacement, color generation, multi-target independence |
| `src/app.rs` | 8 | title formatting |
| **Total** | **261** | |

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

