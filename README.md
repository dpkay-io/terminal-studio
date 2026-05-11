# Terminal Studio

> **Alpha software** — core features work, but expect bugs, rough edges, and breaking changes between releases. Bug reports and feedback are very welcome.

![Terminal Studio](assets/screenshot.png)

**Terminal Studio** is a GPU-accelerated terminal multiplexer built with Rust, [egui](https://github.com/emilk/egui), and the wgpu renderer. It combines a full-featured multi-pane terminal with an integrated file browser, git diff viewer, markdown renderer, and a workspace system — all in a single, frameless desktop window with crisp, hardware-accelerated text rendering.

---

## Features

- **Multi-pane terminal** — split the center panel into as many side-by-side terminal panes as you need, each independently resizable
- **GPU rendering via wgpu** — text and UI are rendered on the GPU through egui's wgpu backend, delivering smooth, pixel-perfect output
- **Full VTE/ANSI emulation** — 256-color support, SGR attributes, mouse reporting, bracketed paste, alternate screen, and more, using the same VTE parser as Alacritty
- **Workspace system** — create named workspaces bound to directory paths; each workspace gets a custom accent color and switches automatically as you navigate your filesystem
- **File browser** — recursive directory tree in the right-hand panel; click any file to open it, `.md` files open in the markdown previewer
- **Git integration** — live diff viewer shows working-tree changes with colored hunks and inline status badges
- **Integrated markdown renderer** — renders headers, bold, inline code, fenced code blocks, lists, and blockquotes
- **Integrated file editor** — edit any file directly in a center pane with `Ctrl+S` to save
- **Per-workspace notes** — a scratch-pad panel scoped to each workspace, auto-saved across sessions
- **Session persistence** — all open terminals and their working directories are saved on exit and restored on the next launch
- **Catppuccin Mocha theme** — consistent, low-eye-strain dark theme throughout

See [FEATURES.md](FEATURES.md) for the full feature breakdown.

---

## Building from Source

### Prerequisites

**All platforms**

- Rust stable toolchain, version 1.75 or newer
  ```
  curl https://sh.rustup.rs -sSf | sh   # Linux / macOS
  winget install Rustlang.Rustup         # Windows
  ```

**Windows**

- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **Desktop development with C++** workload — required by wgpu/winit for MSVC linking

**Linux**

```
# Debian / Ubuntu
sudo apt install pkg-config libxkbcommon-dev libwayland-dev
# For X11 instead of Wayland:
sudo apt install libx11-dev libxcb1-dev
```

**macOS**

```
xcode-select --install
```

### Build steps

```
# Clone the repository
git clone https://github.com/dpkay-io/terminal-studio
cd terminal-studio

# Release build (recommended)
cargo build --release

# The binary is placed at:
#   Windows  → target\release\terminal-studio.exe
#   Linux    → target/release/terminal-studio
#   macOS    → target/release/terminal-studio
```

### Run

```
cargo run --release
```

---

## Usage

### Starting the app

Launch the binary directly, or run `cargo run --release` from the repository root. On first launch the workspace list will be empty and a single terminal pane will open in your current working directory.

### Keyboard shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+S` | Save the file in the active file-editor pane |
| `Ctrl+C` | Send SIGINT to the foreground process in the active terminal |
| `Ctrl+Shift+C` | Copy selection (when mouse selection is active) |

> Terminal key sequences (e.g. `Ctrl+L` to clear, `Ctrl+D` to exit a shell) are passed directly to the running process, the same as any terminal emulator.

### Managing panes

- Click the **+** button in the pane header area to open a new terminal pane.
- Drag the vertical divider between panes to resize them.
- Close a pane with its **×** button in the header.

### Workspaces

1. Click **+ Workspace** in the left sidebar.
2. Enter a name and choose the root directory for the workspace.
3. An accent color is assigned automatically (or pick one from the preset palette).
4. Whenever a terminal's CWD is under a workspace path, that workspace becomes active and its color tints the titlebar and pane headers.

### File browser and editor

- The right panel shows the directory tree for the currently active terminal's CWD.
- Click any file to open it in a new editor pane in the center.
- `.md` files open in the markdown preview tab instead.
- Press `Ctrl+S` in an editor pane to write changes to disk.

### Git diff viewer

Switch to the **Git** tab in the right panel to see a live diff of the working tree relative to HEAD. Changed files are listed with status badges; click a file to expand its hunk view.

---

## Development

```
# Debug build (faster compile, opt-level 1)
cargo run

# Run tests
cargo test

# Enable debug logging
RUST_LOG=debug cargo run          # Linux / macOS
$env:RUST_LOG="debug"; cargo run  # Windows PowerShell
```

---

## Data and Configuration

Terminal Studio stores all persistent data in a platform-specific directory — no configuration files are written next to the binary.

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\terminal-studio\` |
| Linux / macOS | `~/.config/terminal-studio/` |

Files written under that directory:

| File | Contents |
|---|---|
| `session.json` | Open panes, terminal CWDs, active workspace, panel layout |
| `workspaces.json` | Named workspace definitions (name, path, color) |
| `notes.json` | Per-workspace scratch-pad notes |

To reset all state, delete the directory. The app will start fresh on the next launch.

---

## Platform Support

| Feature | Windows | Linux | macOS |
|---|---|---|---|
| PTY backend | ConPTY | openpty | openpty |
| Default shell | PowerShell | Bash | Bash |
| File watcher | ReadDirectoryChangesW | inotify | FSEvents |
| State directory | `%APPDATA%` | `~/.config` | `~/.config` |
| Window controls | Right-aligned | Right-aligned | Traffic lights |

---

## Alpha Status

Terminal Studio is **alpha** software. Things that are known to be incomplete or rough:

- No binary releases yet — must build from source
- Linux and macOS are less tested than Windows
- The terminal emulator handles most applications well but may have gaps with advanced TUI apps
- No configuration file yet — theme and fonts are not user-customizable

If you hit a bug, please [open an issue](https://github.com/dpkay-io/terminal-studio/issues). Include your OS, shell, and the app or escape sequence that triggered the problem.

---

## Contributing

Bug reports are the most helpful contribution right now. The architecture is still evolving, so for non-trivial code changes, please open an issue to discuss before sending a PR.

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

---

## License

Terminal Studio is released under the [Apache License, Version 2.0](LICENSE).
