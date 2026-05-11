# Terminal Studio — Codebase Reference

GPU-accelerated terminal multiplexer in Rust using egui/eframe (wgpu renderer). Tiling terminal interface with workspaces, file browsing, git integration, and markdown preview.

## Build & Run

```
cargo build              # dev (opt-level 1)
cargo run
cargo build --release    # opt-level 3, LTO=true, codegen-units=1
cargo test               # all unit tests
RUST_LOG=debug cargo run # enable debug logging
```

## Module Map

| File | Responsibility |
|------|---------------|
| `src/main.rs` | Entry point; eframe setup, 1280×800 viewport, wgpu renderer, decorations=false |
| `src/app.rs` | `App` struct (~3387 lines); ALL UI logic, state management, persistence |
| `src/theme.rs` | Catppuccin Mocha palette, semantic color aliases, layout constants, helper fns |
| `src/terminal/mod.rs` | `Session` struct, `MouseMode` enum |
| `src/terminal/grid.rs` | `Grid`, `Cell`, `Color`, `CellAttrs`; scrollback (10k lines max) |
| `src/terminal/performer.rs` | VTE `Performer` impl — CSI/ESC/OSC dispatch |
| `src/terminal/tests.rs` | ~971 lines of terminal emulator tests |
| `src/pty/mod.rs` | `SessionManager`: spawn/write/resize PTY sessions |
| `src/pty/reader.rs` | Dedicated reader thread per PTY (8KB buffer, 8ms batching) |
| `src/pty/foreground.rs` | Platform-specific foreground process detection (cache TTL 500ms) |
| `src/renderer/mod.rs` | Re-export of `terminal_pass` |
| `src/renderer/terminal_pass.rs` | `TerminalView`, `TerminalGeometry` — cell rendering |
| `src/workspace.rs` | `WorkspaceStore`, `NoteStore` — JSON persistence |

## Key Types & IDs

- Pane IDs: `u32`
- Session IDs: `u32`
- Workspace IDs: `u64`
- `PaneContent` is either `Terminal(session_id: u32)` or `FileEditor(FileEditorState)`
- `WorkspaceStore` is the source of truth for workspace data
- Grid coordinates: 0-based row/col; terminal rows/cols are `u16`

## Core Architecture

**Threading model:**
- UI runs on the main thread
- Each PTY has a dedicated reader thread feeding VTE → `Session` under `RwLock` write lock
- `Session` state is `Arc<RwLock<Session>>` — shared between PTY reader thread and UI thread
- `alive` flag is `Arc<AtomicBool>` — signals reader thread to stop
- Reader calls `ctx.request_repaint_after(8ms)` to coalesce repaint requests

**Persistence:**
- JSON files in `%APPDATA%\terminal-studio\` (Windows) or `~/.config/terminal-studio/` (Unix)
- `WorkspaceStore` / `NoteStore` serialize to JSON via serde

**File watching:**
- `notify::RecommendedWatcher` with a channel-based event loop
- Only tracks non-hidden directories (no dotfiles)
- Git diff via `std::process::Command("git")`; refresh debounce is 500ms

## Terminal Emulator Details

- VTE parser: `vte 0.13` (same as Alacritty)
- Scrollback max: hardcoded 10,000 lines in `grid.rs`
- OSC 7 → set `cwd`; OSC 0/2 → set title; `prompt_ready` is set on first OSC 7
- Mouse events: SGR format (`?1006`) when `mouse_sgr=true`; coordinates are 1-based
- PSReadLine (Windows) clears screen on resize — workaround: snapshot `cursor_y` before resize, restore if shell clears

## Dependencies

```toml
eframe = "0.28"         # wgpu, default_fonts, persistence
egui = "0.28"
vte = "0.13"            # ANSI parser
portable-pty = "0.8"    # ConPTY (Windows) / openpty (Unix)
serde + serde_json = "1"
parking_lot = "0.12"    # RwLock for session state
notify = "6"            # file system watching
anyhow = "1"
log = "0.4"
env_logger = "0.11"
windows-sys = "0.52"    # Windows-only: toolhelp, DWM, window messages
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

**Theme constants:** All colors live in `theme.rs`. Semantic aliases (e.g. `CURSOR_COLOR`, `SELECTION_BG`) wrap the raw Catppuccin palette values.

## Common Editing Tasks

**Add a new escape sequence:**
- CSI: edit `performer.rs` → `csi_dispatch()`
- OSC: edit `performer.rs` → `osc_dispatch()`
- ESC: edit `performer.rs` → `esc_dispatch()`

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

| File | Tests |
|------|-------|
| `src/terminal/tests.rs` | ~43 tests: cursor/text, alt screen, device responses, erase, SGR, scroll regions, mouse modes, resize |
| `src/terminal/grid.rs` | 20 grid unit tests |
| `src/workspace.rs` | 14 workspace/notes tests |
| `src/theme.rs` | 16 color/theme tests |
| `src/app.rs` | 8 title formatting tests |

## Known Quirks & Gotchas

- In tests, `Session::new` requires 4 arguments: `Session::new(id, cols, rows, None)` — the `cwd` arg is `Option<PathBuf>`, always `None` in tests
- Mouse SGR coordinates are 1-based (not 0-based like grid internals)
- Scrollback is hard-capped at 10,000 lines; changing this requires editing the constant in `grid.rs`
- The watcher skips dotfile directories; hidden files inside tracked dirs are still visible
- Foreground process detection result is cached with a 500ms TTL to avoid hammering the OS
- Git status refresh is debounced at 500ms
- Windows DWM and toolhelp APIs are accessed directly via `windows-sys`; keep all such calls behind `#[cfg(target_os = "windows")]`
