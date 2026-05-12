# Terminal Studio

[![CI](https://github.com/dpkay-io/terminal-studio/actions/workflows/ci.yml/badge.svg)](https://github.com/dpkay-io/terminal-studio/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/dpkay-io/terminal-studio?include_prereleases)](https://github.com/dpkay-io/terminal-studio/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

> **Alpha software** — core features work, but expect bugs and breaking changes between releases. Bug reports are very welcome.

![Terminal Studio](assets/screenshot.png)

**Terminal Studio** is a GPU-accelerated terminal multiplexer built with Rust, [egui](https://github.com/emilk/egui), and the wgpu renderer. It combines multi-pane terminals with an integrated file browser, git diff viewer, markdown renderer, and a workspace system — all in a single frameless window with hardware-accelerated text rendering.

---

## Features

- **Multi-pane terminal** — split into as many side-by-side panes as you need, each independently resizable
- **GPU rendering via wgpu** — smooth, pixel-perfect text rendered on the GPU through egui's wgpu backend
- **Full VTE/ANSI emulation** — 256-color, SGR attributes, mouse reporting, bracketed paste, alternate screen (same VTE parser as Alacritty)
- **Workspace system** — named workspaces bound to directories; custom accent color auto-activates when your CWD matches
- **File browser** — recursive directory tree; click any file to open it, `.md` files open in the markdown previewer
- **Git integration** — live diff viewer with colored hunks and inline status badges
- **Integrated markdown renderer** — headers, bold, code blocks, lists, blockquotes
- **Integrated file editor** — edit any file in a center pane with `Ctrl+S` to save
- **Per-workspace notes** — scratch-pad scoped to each workspace, auto-saved
- **Session persistence** — open terminals and their CWDs are restored on next launch
- **Catppuccin Mocha theme** — consistent dark theme throughout

See [FEATURES.md](FEATURES.md) for the full feature list.

---

## Installation

### Pre-built binaries (recommended)

**Linux / macOS**

```sh
curl -fsSL https://raw.githubusercontent.com/dpkay-io/terminal-studio/master/scripts/install.sh | sh
```

Downloads the latest release binary to `~/.local/bin/terminal-studio`. Override the destination with `INSTALL_DIR=/your/path`.

**Windows (PowerShell)**

```powershell
iwr https://raw.githubusercontent.com/dpkay-io/terminal-studio/master/scripts/install.ps1 | iex
```

Installs to `%LOCALAPPDATA%\terminal-studio\terminal-studio.exe`.

**Manual download**

Download the binary for your platform from the [latest release](https://github.com/dpkay-io/terminal-studio/releases/latest), make it executable, and place it on your `PATH`.

| Platform | File |
|---|---|
| Windows x86-64 | `terminal-studio-windows.exe` |
| Linux x86-64 | `terminal-studio-linux` |
| macOS (Apple Silicon + Intel via Rosetta 2) | `terminal-studio-macos-arm` |

---

### Building from Source

#### Prerequisites

**All platforms** — Rust stable toolchain 1.75 or newer:

```sh
curl https://sh.rustup.rs -sSf | sh        # Linux / macOS
winget install Rustlang.Rustup              # Windows
```

**Windows** — [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **Desktop development with C++** workload (required for MSVC linking).

**Linux**

```sh
sudo apt install pkg-config libxkbcommon-dev libwayland-dev
# X11 (optional):
sudo apt install libx11-dev libxcb1-dev
```

**macOS**

```sh
xcode-select --install
```

#### Build

```sh
git clone https://github.com/dpkay-io/terminal-studio
cd terminal-studio
cargo build --release
# Binary: target/release/terminal-studio  (terminal-studio.exe on Windows)
```

---

## Usage

### Keyboard shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+S` | Save the active file-editor pane |
| `Ctrl+C` | Send SIGINT to the foreground process |
| `Ctrl+Shift+C` | Copy selection |

Terminal key sequences (`Ctrl+L`, `Ctrl+D`, etc.) pass through directly to the running shell.

### Managing panes

- Click **+** in the pane header to open a new terminal pane.
- Drag the vertical divider between panes to resize.
- Close a pane with its **×** button.

### Workspaces

1. Click **+ Workspace** in the left sidebar.
2. Enter a name and select the root directory.
3. An accent color is assigned automatically (pick from the preset palette to override).
4. Whenever a terminal's CWD falls under a workspace path, that workspace activates and its color tints the UI.

### File browser and editor

- The right panel shows the directory tree for the active terminal's CWD.
- Click any file to open it in an editor pane; `.md` files open in the markdown previewer.
- `Ctrl+S` saves changes.

### Git diff viewer

Switch to the **Git** tab in the right panel for a live diff of the working tree relative to HEAD. Click any file to expand its hunk view.

---

## Development

```sh
cargo run                           # debug build (faster compile)
cargo test                          # all unit tests
cargo clippy                        # lints
RUST_LOG=debug cargo run            # with debug logging (Linux / macOS)
$env:RUST_LOG="debug"; cargo run    # Windows PowerShell
```

---

## Data and Configuration

All state is stored in a platform-specific directory — no config files next to the binary.

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\terminal-studio\` |
| Linux / macOS | `~/.config/terminal-studio/` |

| File | Contents |
|---|---|
| `session.json` | Open panes, CWDs, active workspace, panel layout |
| `workspaces.json` | Workspace definitions (name, path, color) |
| `notes.json` | Per-workspace scratch-pad notes |

Delete the directory to reset all state.

---

## Platform Support

| Feature | Windows | Linux | macOS |
|---|---|---|---|
| PTY backend | ConPTY | openpty | openpty |
| Default shell | PowerShell | Bash | Bash |
| File watcher | ReadDirectoryChangesW | inotify | FSEvents |
| State directory | `%APPDATA%` | `~/.config` | `~/.config` |

---

## Alpha Status

Known gaps:

- Linux and macOS are less tested than Windows
- The terminal emulator handles most TUI apps well but may have gaps with advanced escape sequences
- No user-configurable theme or font yet

If you hit a bug, please [open an issue](https://github.com/dpkay-io/terminal-studio/issues). Include your OS, shell, and the app or escape sequence that triggered the problem.

---

## Contributing

Bug reports are the most helpful contribution right now. For non-trivial code changes, open an issue to discuss first.

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

---

## License

Apache License, Version 2.0 — see [LICENSE](LICENSE).
