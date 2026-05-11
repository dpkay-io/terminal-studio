# Terminal Studio — Features

Terminal Studio is a GPU-accelerated terminal multiplexer with an integrated file browser, Git diff viewer, Markdown previewer, and text editor — all in a single, cohesive desktop application built with Rust and egui.

---

## Table of Contents

- [Terminal Emulation](#terminal-emulation)
- [Multi-Pane Layout](#multi-pane-layout)
- [Workspace System](#workspace-system)
- [Session Management](#session-management)
- [File Browser](#file-browser)
- [Git Integration](#git-integration)
- [Markdown Preview](#markdown-preview)
- [File Editor](#file-editor)
- [Shell Integration](#shell-integration)
- [Input Handling](#input-handling)
- [Persistence and Session Restore](#persistence-and-session-restore)
- [User Interface](#user-interface)
- [Platform Support](#platform-support)

---

## Terminal Emulation

Terminal Studio ships a full-featured terminal emulator with broad compatibility for modern CLI applications.

**ANSI / VTE Escape Sequences**
- Full CSI, ESC, and OSC sequence support (using the same VTE parser as Alacritty)
- 256-color palette: standard 16 colors, 6×6×6 color cube, and 24-level grayscale ramp
- SGR text attributes: bold, dim, italic, underline, blink, reverse video, invisible, and strikethrough

**Cursor Control**
- Absolute and relative cursor positioning: CUP, CUU, CUD, CUF, CUB, CNL, CPL, CHA, VPA
- Cursor save and restore (DECSC / DECRC)
- Reverse index (RI)

**Screen Editing**
- Erase in display (ED), erase in line (EL), erase characters (ECH), delete characters (DCH)
- Scroll up / down (SU / SD), insert / delete lines (IL / DL)
- Configurable scroll regions via DECSTBM

**Buffers**
- Primary and alternate screen buffers (`?47` and `?1049`)
- Scrollback buffer retaining up to 10,000 lines

**Mouse Support**
- Basic mouse reporting (`?1000`), button-motion tracking (`?1002`), all-motion tracking (`?1003`)
- SGR extended mouse coordinate encoding (`?1006`) for large terminals
- Mouse click, drag, and scroll wheel events forwarded to applications

**Additional Protocol Support**
- Bracketed paste mode (`?2004`)
- Focus in / focus out tracking (`?1004`)
- Device attributes (DA1, DA2) and device status reports (DSR, CPR)
- Terminal reset (RIS)
- CWD tracking via OSC 7 (`file://` URI)
- Window title updates via OSC 0 and OSC 2

---

## Multi-Pane Layout

Work with multiple terminals and editors side by side in a single window.

- Horizontal tab bar across the center panel for switching between open panes
- Open any combination of terminal and file editor panes simultaneously
- Manually adjust the width of each pane to suit your workflow
- Each pane independently tracks PTY size and reflows on resize
- Color strip in the tab bar reflects the active pane's workspace
- Active pane highlighted with an underline indicator
- Close any pane individually with its close button

---

## Workspace System

Workspaces let you color-code and organize sessions by project or context.

- Create named workspaces with a chosen accent color
- Eight preset colors plus a full custom color picker
- Workspaces are automatically activated when a terminal navigates to a directory that belongs to that workspace (via CWD prefix matching)
- The titlebar dynamically adopts the active workspace's color
- Color-coded session indicators in the left panel show which workspace each session belongs to
- Set a default workspace for new sessions
- Per-workspace notes panel with auto-saving plain-text notes
- Note indicator shown in the sidebar when a workspace has notes

---

## Session Management

- Open as many terminal sessions as you need
- Duplicate any session; the new session opens in the same working directory, with the foreground process identified in the name
- Sessions are automatically named from the shell title or current directory
- Session list displayed in a collapsible left panel with badge labels (P1, P2, …)
- Each session independently tracks whether its underlying process is alive or has exited

---

## File Browser

A built-in file tree lives in the right panel, keeping your project structure visible without leaving the application.

- Recursive directory tree for browsing any folder
- Directories listed before files, both groups sorted alphabetically
- Hidden files and dotfiles are excluded by default for a clean view
- File-type icons for quick visual identification
- Single-click a Markdown file to open it in the preview panel
- Double-click any file to open it in the integrated editor
- Collapsible directory headers; expand/collapse state is persisted across restarts

---

## Git Integration

Stay aware of your repository status without switching to another tool.

- Git diff viewer in the right panel showing the current working-tree changes
- Status badges next to each changed file:
  - **M** — modified
  - **A** — added
  - **D** — deleted
  - **R** — renamed
  - **?** — untracked
- Colored diff hunks for clear at-a-glance review
- File-system watcher automatically refreshes Git status when files change, with a 500 ms debounce to avoid spurious updates
- Git status is cached per directory to keep the UI responsive
- The watcher stays synchronized with the active terminal's current working directory

---

## Markdown Preview

Render Markdown files without leaving your development environment.

- Single-click any `.md` file in the file browser to preview it instantly
- Multiple Markdown files open as tabs in the right panel
- Rendered elements:
  - Headings at four levels (`#`, `##`, `###`, `####`)
  - **Bold** text (`**…**`)
  - Fenced code blocks (` ``` `)
  - Inline code
  - Blockquotes (`>`)
  - Unordered lists (`-`, `*`)
  - Horizontal rules (`---`)
- Smart spacing between elements for a readable layout

---

## File Editor

Edit files directly inside Terminal Studio without switching to an external editor.

- Full-featured text editor pane in the center panel, alongside terminals
- **Ctrl+S** to save
- Dirty-state indicator so you always know if there are unsaved changes
- Editor state is scoped to the active workspace

---

## Shell Integration

Terminal Studio sets up your shell automatically so features like CWD tracking work out of the box.

- **Windows**: PowerShell with a custom prompt function that emits the current directory via OSC 7
- **Unix / macOS**: Bash with `PROMPT_COMMAND` configured to emit OSC 7
- `TERM=xterm-256color` set in every session for broad application compatibility
- Prompt-ready detection enables pending commands to be replayed after the shell prompt appears
- Working directory is restored when a saved session is reopened

---

## Input Handling

Full keyboard and mouse input is forwarded faithfully to running applications.

**Keyboard**
- All printable characters routed directly to the PTY
- **Ctrl+letter** → control bytes 1–26
- **Alt+letter** → ESC-prefixed sequences
- Arrow keys with all modifier combinations: Shift, Ctrl, Alt
- Function keys F1–F12
- Special keys: Tab, Enter, Escape, Delete, Backspace, Page Up, Page Down, Home, End, Insert
- **Ctrl+C** sends SIGINT (byte 3) when the terminal has focus; copies selection when text is selected

**Mouse**
- Click to focus and position the cursor
- Click-and-drag text selection
- Scroll wheel with SGR encoding forwarded to mouse-aware applications

**Paste**
- Paste text from the clipboard with bracketed-paste wrapping when the running application has enabled it

---

## Persistence and Session Restore

Close the app and pick up exactly where you left off.

- Complete session state is saved automatically on exit
- On next launch, all sessions are restored (this behavior can be disabled in Settings)
- Restored state includes:
  - Working directories for every session
  - Pane layout and individual pane widths
  - Workspace configuration and active workspace
  - Left and right panel collapse state
  - Notes panel content and collapse state
  - Open Markdown tabs
  - Active right-panel tab selection
- Pending commands are replayed automatically once the shell prompt is ready after restore

---

## User Interface

A polished, distraction-free environment built for daily use.

**Window Chrome**
- Frameless window with fully custom titlebar
- macOS: native-style traffic-light controls (close / minimize / maximize) with a centered title
- Windows / Linux: right-aligned window controls with panel toggle buttons in the titlebar
- Drag anywhere on the titlebar to move the window
- Titlebar accent color reflects the active workspace

**Panels**
- Collapsible left panel containing the session list and workspace list, with a draggable divider
- Collapsible right panel containing the file browser, Git diff, and Markdown preview, with a draggable divider
- Notes section in the right panel, also collapsible

**Dialogs**
- Workspace create / edit dialog with name field and color picker
- Workspace delete confirmation dialog
- Settings dialog for default workspace and session restore preference

**Rendering**
- GPU-accelerated rendering via wgpu for smooth, high-framerate repaints
- Catppuccin Mocha color theme throughout the UI
- Per-PTY reader threads with 8 ms repaint batching keep the terminal responsive without burning CPU

---

## Platform Support

| Feature | Windows | Linux | macOS |
|---|---|---|---|
| PTY backend | ConPTY | openpty | openpty |
| Default shell | PowerShell | Bash | Bash |
| File watcher | ReadDirectoryChangesW | inotify | FSEvents |
| Foreground process detection | CreateToolhelp32Snapshot + wmic | /proc traversal | pgrep + ps |
| State directory | `%APPDATA%` | `~/.config` | `~/.config` |
| Window controls | Right-aligned (Win style) | Right-aligned | Traffic lights (macOS style) |

All three platforms receive the same full feature set; platform differences are limited to native OS integrations listed above.

---

## Contributing

Terminal Studio is open source and welcomes contributions. Whether you are fixing a bug, adding a feature, or improving documentation, feel free to open an issue or pull request.
