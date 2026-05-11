use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::FontId;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;

use serde::{Deserialize, Serialize};

use crate::pty::foreground::ForegroundProcess;
use crate::pty::{available_shells, default_shell, ShellKind, SessionManager};
use crate::renderer::terminal_pass::TerminalGeometry;
use crate::terminal::grid::Cell;
use crate::terminal::{MouseMode, Session};
use crate::theme;
use crate::workspace::{NoteStore, Workspace, WorkspaceStore};

// ── Preset workspace colours ──────────────────────────────────────────────────

const PRESET_COLORS: &[[u8; 3]] = &[
    [100, 140, 230], // blue
    [80, 200, 100],  // green
    [220, 120, 80],  // orange
    [200, 80, 160],  // pink
    [140, 100, 220], // purple
    [80, 200, 200],  // teal
    [220, 200, 60],  // yellow
    [200, 80, 80],   // red
];

// ── Right-panel tab ───────────────────────────────────────────────────────────

#[derive(PartialEq, Clone, Debug)]
enum RightTab {
    Directory,
    GitDiff,
    Markdown(PathBuf),
}

// ── Center panel (multi-pane) ─────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct FileEditorState {
    path: PathBuf,
    content: String,
    dirty: bool,
    save_error: bool,
    workspace_id: Option<u64>,
}

#[derive(Debug)]
enum PaneContent {
    Terminal(u32),
    /// PTY not yet spawned — deferred until the pane is first focused.
    /// Keeps cwd/command so the session can be materialized on demand.
    DeferredTerminal {
        cwd: Option<PathBuf>,
        pending_command: Option<String>,
    },
    FileEditor(FileEditorState),
}

struct PaneEntry {
    id: u32,
    content: PaneContent,
    manual_width: Option<f32>,
    last_size: (u16, u16),
}

// ── Session persistence ───────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct SavedSession {
    cwd: PathBuf,
    #[serde(default)]
    command: Option<String>,
}

#[derive(Serialize, Deserialize)]
enum SavedPaneContent {
    Terminal {
        session_index: usize,
    },
    DeferredTerminal {
        cwd: PathBuf,
        #[serde(default)]
        command: Option<String>,
    },
    FileEditor {
        path: PathBuf,
        content: String,
        dirty: bool,
        workspace_id: Option<u64>,
    },
}

#[derive(Serialize, Deserialize)]
struct SavedPane {
    content: SavedPaneContent,
    manual_width: Option<f32>,
}

#[derive(Serialize, Deserialize)]
enum SavedRightTab {
    Directory,
    GitDiff,
    Markdown(PathBuf),
}

#[derive(Serialize, Deserialize)]
struct AppSession {
    sessions: Vec<SavedSession>,
    panes: Vec<SavedPane>,
    active_pane_index: Option<usize>,
    active_session_index: Option<usize>,
    active_group: Option<u64>,
    last_pane_per_group: Vec<(Option<u64>, usize)>,
    workspace_panel_ratio: f32,
    workspace_panel_collapsed: bool,
    notes_panel_ratio: f32,
    notes_panel_collapsed: bool,
    right_tab: SavedRightTab,
    shown_md_tabs: Vec<PathBuf>,
}

fn session_data_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|base| {
            PathBuf::from(base)
                .join("terminal-studio")
                .join("session.json")
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(|base| {
            PathBuf::from(base)
                .join(".config")
                .join("terminal-studio")
                .join("session.json")
        })
    }
}

// ── App settings ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone)]
struct AppSettings {
    default_workspace_id: Option<u64>,
    restore_last_session: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            default_workspace_id: None,
            restore_last_session: true,
        }
    }
}

fn settings_data_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|base| {
            PathBuf::from(base)
                .join("terminal-studio")
                .join("settings.json")
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(|base| {
            PathBuf::from(base)
                .join(".config")
                .join("terminal-studio")
                .join("settings.json")
        })
    }
}

impl AppSettings {
    fn load() -> Self {
        let Some(path) = settings_data_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    fn save(&self) {
        let Some(path) = settings_data_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

// ── Directory file entry ──────────────────────────────────────────────────────

#[derive(Clone)]
struct FileEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

fn list_dir_entries(path: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(path) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            entries.push(FileEntry {
                is_dir: p.is_dir(),
                name,
                path: p,
            });
        }
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    entries
}

// ── Per-directory data ────────────────────────────────────────────────────────

struct DirData {
    is_git: bool,
    git_diff: String,
    git_status: String,
    git_refresh_at: Option<Instant>,
    md_files: HashMap<PathBuf, String>,
    dir_entries: Vec<FileEntry>,
}

impl DirData {
    fn new(path: &Path) -> Self {
        let is_git = path.join(".git").exists();
        let (git_diff, git_status) = if is_git {
            run_git_info(path)
        } else {
            (String::new(), String::new())
        };
        DirData {
            is_git,
            git_diff,
            git_status,
            git_refresh_at: None,
            md_files: HashMap::new(),
            dir_entries: list_dir_entries(path),
        }
    }
}

fn run_git_info(dir: &Path) -> (String, String) {
    use std::process::Command;
    let git = |args: &[&str]| -> String {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default()
    };
    let staged = git(&["diff", "--cached", "--no-color"]);
    let unstaged = git(&["diff", "--no-color"]);
    let status = git(&["status", "--porcelain"]);
    let diff = match (staged.is_empty(), unstaged.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("=== Staged ===\n{staged}"),
        (true, false) => format!("=== Unstaged ===\n{unstaged}"),
        (false, false) => format!("=== Staged ===\n{staged}\n=== Unstaged ===\n{unstaged}"),
    };
    (diff, status)
}

// ── File-watcher ──────────────────────────────────────────────────────────────

struct WatchState {
    watcher: RecommendedWatcher,
    watched: HashSet<PathBuf>,
    git_dirs: HashMap<PathBuf, PathBuf>,
    events: Arc<std::sync::Mutex<Vec<Event>>>,
    dir_data: HashMap<PathBuf, DirData>,
    last_sync: Instant,
    last_session_count: usize,
}

impl WatchState {
    fn new(ctx: egui::Context) -> Option<Self> {
        let events: Arc<std::sync::Mutex<Vec<Event>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let ev = Arc::clone(&events);
        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                if let Ok(mut g) = ev.lock() {
                    g.push(event);
                }
                ctx.request_repaint();
            }
        })
        .ok()?;
        Some(WatchState {
            watcher,
            watched: HashSet::new(),
            git_dirs: HashMap::new(),
            events,
            dir_data: HashMap::new(),
            last_sync: Instant::now(),
            last_session_count: 0,
        })
    }

    fn sync(&mut self, sessions: &[SessionEntry]) {
        let current: HashSet<PathBuf> = sessions
            .iter()
            .map(|e| e.session.read().cwd.clone())
            .filter(|p| !p.as_os_str().is_empty() && p.is_dir())
            .collect();

        let to_remove: Vec<PathBuf> = self
            .watched
            .iter()
            .filter(|d| !current.contains(*d))
            .cloned()
            .collect();
        for dir in to_remove {
            let _ = self.watcher.unwatch(&dir);
            if let Some(gd) = self
                .git_dirs
                .iter()
                .find(|(_, v)| **v == dir)
                .map(|(k, _)| k.clone())
            {
                let _ = self.watcher.unwatch(&gd);
                self.git_dirs.remove(&gd);
            }
            self.watched.remove(&dir);
            self.dir_data.remove(&dir);
        }

        let to_add: Vec<PathBuf> = current
            .into_iter()
            .filter(|d| !self.watched.contains(d))
            .collect();
        for dir in to_add {
            if self
                .watcher
                .watch(&dir, RecursiveMode::NonRecursive)
                .is_ok()
            {
                let gd = dir.join(".git");
                if gd.is_dir() && self.watcher.watch(&gd, RecursiveMode::NonRecursive).is_ok() {
                    self.git_dirs.insert(gd, dir.clone());
                }
                self.dir_data.insert(dir.clone(), DirData::new(&dir));
                self.watched.insert(dir);
            }
        }
    }

    fn process_events(&mut self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let events: Vec<Event> = {
            let Ok(mut g) = self.events.lock() else {
                return (vec![], vec![]);
            };
            std::mem::take(&mut *g)
        };

        let now = Instant::now();
        let debounce = Duration::from_millis(500);
        let mut created_md: Vec<PathBuf> = Vec::new();
        let mut removed_md: Vec<PathBuf> = Vec::new();

        for event in events {
            for path in &event.paths {
                let dir: PathBuf = match path.parent().map(PathBuf::from) {
                    Some(p) if self.watched.contains(&p) => p,
                    Some(p) if self.git_dirs.contains_key(&p) => self.git_dirs[&p].clone(),
                    _ => continue,
                };
                let Some(data) = self.dir_data.get_mut(&dir) else {
                    continue;
                };
                let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");

                match &event.kind {
                    EventKind::Create(_) => {
                        if is_md && path.is_file() {
                            let content = std::fs::read_to_string(path).unwrap_or_default();
                            data.md_files.insert(path.clone(), content);
                            created_md.push(path.clone());
                        }
                        data.dir_entries = list_dir_entries(&dir);
                        if data.is_git {
                            data.git_refresh_at.get_or_insert(now + debounce);
                        }
                    }
                    EventKind::Modify(_) => {
                        if is_md && path.is_file() && data.md_files.contains_key(path) {
                            let content = std::fs::read_to_string(path).unwrap_or_default();
                            data.md_files.insert(path.clone(), content);
                        }
                        if data.is_git {
                            data.git_refresh_at.get_or_insert(now + debounce);
                        }
                    }
                    EventKind::Remove(_) => {
                        if data.md_files.remove(path).is_some() {
                            removed_md.push(path.clone());
                        }
                        data.dir_entries = list_dir_entries(&dir);
                        if data.is_git {
                            data.git_refresh_at.get_or_insert(now + debounce);
                        }
                    }
                    _ => {}
                }
            }
        }

        for (dir, data) in &mut self.dir_data {
            if data.git_refresh_at.map(|t| now >= t).unwrap_or(false) {
                let (diff, status) = run_git_info(dir);
                data.git_diff = diff;
                data.git_status = status;
                data.git_refresh_at = None;
            }
        }

        (created_md, removed_md)
    }
}

// ── Workspace dialog state ────────────────────────────────────────────────────

struct WorkspaceDialog {
    name: String,
    path: PathBuf,
    selected_color: [u8; 3],
    custom_color: [f32; 3],
    show_custom_picker: bool,
    focus_requested: bool,
}

impl WorkspaceDialog {
    fn new(path: PathBuf) -> Self {
        WorkspaceDialog {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path,
            selected_color: PRESET_COLORS[0],
            custom_color: [0.4, 0.55, 0.9],
            show_custom_picker: false,
            focus_requested: false,
        }
    }
}

// ── Workspace edit dialog state ───────────────────────────────────────────────

struct WorkspaceEditDialog {
    workspace_id: u64,
    name: String,
    selected_color: [u8; 3],
    custom_color: [f32; 3],
    show_custom_picker: bool,
    confirm_delete: bool,
    focus_requested: bool,
}

impl WorkspaceEditDialog {
    fn new(id: u64, name: String, color: [u8; 3]) -> Self {
        let is_preset = PRESET_COLORS.contains(&color);
        WorkspaceEditDialog {
            workspace_id: id,
            name,
            selected_color: color,
            custom_color: [
                color[0] as f32 / 255.0,
                color[1] as f32 / 255.0,
                color[2] as f32 / 255.0,
            ],
            show_custom_picker: !is_preset,
            confirm_delete: false,
            focus_requested: false,
        }
    }
}

// ── Session entry ─────────────────────────────────────────────────────────────

struct SessionEntry {
    id: u32,
    session: Arc<RwLock<Session>>,
    writer: Box<dyn Write + Send>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    shell_pid: u32,
    alive: Arc<AtomicBool>,
    is_active: Arc<AtomicBool>,
    pending_command: Option<String>,
    shell: ShellKind,
}

fn display_title(title: &str) -> String {
    let t = title.trim();
    let looks_like_path = t.starts_with('/')
        || t.starts_with('~')
        || (t.len() >= 3
            && t.chars()
                .next()
                .map(|c| c.is_ascii_alphabetic())
                .unwrap_or(false)
            && &t[1..3] == ":\\");
    if looks_like_path {
        t.split(['/', '\\'])
            .rfind(|s| !s.is_empty())
            .unwrap_or(t)
            .to_string()
    } else {
        t.to_string()
    }
}

/// Wraps an argument in shell-appropriate quoting when it contains special characters.
fn shell_escape_arg(s: &str) -> String {
    let safe = !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '@' | '='));
    if safe {
        return s.to_string();
    }
    #[cfg(target_os = "windows")]
    {
        format!("\"{}\"", s.replace('"', "\"\""))
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

// When the shell hasn't set a meaningful title, show the cwd's last path fragment.
fn effective_title(title: &str, cwd: &std::path::Path) -> String {
    let t = title.trim();
    let tl = t.to_lowercase();
    let is_shell_default = t.is_empty()
        || tl.starts_with("session ")
        || tl == "powershell.exe"
        || tl == "windows powershell"
        || tl == "cmd.exe"
        || tl == "bash"
        || tl == "zsh"
        || tl == "sh"
        || tl == "fish";
    if is_shell_default {
        cwd.file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(t)
            .to_string()
    } else {
        display_title(title)
    }
}

struct ResizeSnapshot {
    cursor_y: u16,
    cells: Vec<Cell>,
    expires: Instant,
}

struct CachedForeground {
    session_id: u32,
    result: Option<ForegroundProcess>,
    checked_at: Instant,
}

pub struct App {
    session_manager: SessionManager,
    sessions: Vec<SessionEntry>,
    active_id: Option<u32>,

    panes: Vec<PaneEntry>,
    active_pane_id: Option<u32>,
    next_pane_id: u32,
    right_tab: RightTab,
    shown_md_tabs: HashSet<PathBuf>,
    watch_state: Option<WatchState>,

    workspace_store: WorkspaceStore,
    active_group: Option<u64>,
    last_pane_per_group: HashMap<Option<u64>, u32>,
    workspace_dialog: Option<WorkspaceDialog>,
    workspace_edit_dialog: Option<WorkspaceEditDialog>,

    workspace_panel_ratio: f32,
    workspace_panel_collapsed: bool,

    note_store: NoteStore,
    notes_panel_ratio: f32,
    notes_panel_collapsed: bool,

    show_left_panel: bool,
    show_right_panel: bool,
    show_settings: bool,
    settings: AppSettings,

    // Per-frame terminal geometry for mouse coordinate conversion
    active_term_geo: Option<TerminalGeometry>,
    // Last focused session, for sending focus-in/focus-out events
    last_focused_sid: Option<u32>,
    active_term_ui_id: Option<egui::Id>,
    // Cursor-row snapshot taken before each PTY resize; restored each frame
    // if the shell clears the prompt without immediately redrawing it.
    resize_snapshots: HashMap<u32, ResizeSnapshot>,
    // Debounced PTY resize targets: (cols, rows, stable_since). PTY is only
    // notified after the size has been stable for 150 ms, preventing ConPTY
    // from clearing the screen on every drag frame.
    resize_debounce: HashMap<u32, (u16, u16, Instant)>,
    // Per-session scrollback offset (lines above grid visible on screen).
    // Only active when mouse_mode == None; reset to 0 on any key input.
    term_scroll_offset: HashMap<u32, usize>,
    scroll_accum: HashMap<u32, f32>,
    // Cached foreground-process detection result (500 ms TTL)
    foreground_cache: Option<CachedForeground>,
    // Used to detect when the window gains focus so we can flush stale GPU frames.
    was_focused: bool,
    // Shells available on this system, computed once at startup.
    available_shells: Vec<ShellKind>,
    // IDs of sessions that have not yet received their first OSC 7 (CWD not set).
    // Avoids scanning all sessions every frame once all are initialized.
    uninit_sessions: HashSet<u32>,
    // Cursor blink phase (toggles every 500 ms)
    cursor_blink_on: bool,
    cursor_blink_last: Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let ctx = cc.egui_ctx.clone();

        // Apply global dark theme visuals
        {
            use egui::{Rounding, Shadow, Stroke, Visuals};
            let mut vis = Visuals::dark();
            vis.panel_fill = theme::BG_PANEL_FILL;
            vis.window_fill = theme::BG_TERM;
            vis.window_rounding = Rounding::same(6.0);
            vis.window_shadow = Shadow::NONE;
            vis.popup_shadow = Shadow::NONE;
            vis.widgets.noninteractive.bg_fill = theme::SURFACE0;
            vis.widgets.inactive.bg_fill = theme::SURFACE0;
            vis.widgets.hovered.bg_fill = theme::SURFACE1;
            vis.widgets.active.bg_fill = theme::SURFACE2;
            vis.widgets.inactive.fg_stroke = Stroke::new(1.0, theme::SUBTEXT0);
            vis.widgets.noninteractive.fg_stroke = Stroke::new(1.0, theme::OVERLAY0);
            vis.selection.bg_fill = egui::Color32::from_rgb(75, 85, 130);
            for state in [
                &mut vis.widgets.noninteractive,
                &mut vis.widgets.inactive,
                &mut vis.widgets.hovered,
                &mut vis.widgets.active,
                &mut vis.widgets.open,
            ] {
                state.rounding = Rounding::same(4.0);
            }
            vis.override_text_color = Some(theme::TEXT);
            cc.egui_ctx.set_visuals(vis);
        }

        // Add Segoe UI Symbol as a fallback font so that Geometric Shapes (▶ ▼ □),
        // Mathematical Operators (⊞), and Dingbats (✓) render correctly instead of
        // falling back to empty rectangles when egui's default Ubuntu font lacks them.
        #[cfg(target_os = "windows")]
        {
            use egui::{FontData, FontDefinitions, FontFamily};
            let mut fonts = FontDefinitions::default();
            let win_root =
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
            let font_path = format!("{}\\Fonts\\seguisym.ttf", win_root);
            if let Ok(data) = std::fs::read(&font_path) {
                fonts
                    .font_data
                    .insert("segoe_ui_symbol".to_owned(), FontData::from_owned(data));
                for family in [&FontFamily::Proportional, &FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(family.clone())
                        .or_default()
                        .push("segoe_ui_symbol".to_owned());
                }
                cc.egui_ctx.set_fonts(fonts);
            }
        }

        let mgr = SessionManager::new(ctx.clone());
        let mut app = App {
            session_manager: mgr,
            sessions: vec![],
            active_id: None,
            panes: vec![],
            active_pane_id: None,
            next_pane_id: 0,
            right_tab: RightTab::Directory,
            shown_md_tabs: HashSet::new(),
            watch_state: WatchState::new(ctx),
            workspace_store: WorkspaceStore::load(),
            active_group: None,
            last_pane_per_group: HashMap::new(),
            workspace_dialog: None,
            workspace_edit_dialog: None,
            workspace_panel_ratio: 0.35,
            workspace_panel_collapsed: false,
            note_store: NoteStore::load(),
            notes_panel_ratio: 0.30,
            notes_panel_collapsed: false,
            show_left_panel: true,
            show_right_panel: true,
            show_settings: false,
            settings: AppSettings::load(),
            active_term_geo: None,
            last_focused_sid: None,
            active_term_ui_id: None,
            resize_snapshots: HashMap::new(),
            resize_debounce: HashMap::new(),
            term_scroll_offset: HashMap::new(),
            scroll_accum: HashMap::new(),
            foreground_cache: None,
            was_focused: true,
            available_shells: available_shells(),
            uninit_sessions: HashSet::new(),
            cursor_blink_on: true,
            cursor_blink_last: Instant::now(),
        };
        // Estimate initial terminal size from window dimensions. Fonts aren't available
        // until after the first Context::run(), so use empirical cell dimensions for
        // Ubuntu Mono 14pt (the egui default) to avoid starting at 80x24.
        let (init_cols, init_rows) = {
            const CELL_W: f32 = 8.4;
            const CELL_H: f32 = 18.0;
            let est_w = (1280.0 - theme::LEFT_SIDEBAR_W - 300.0 - 4.0).max(100.0);
            let est_h = (800.0 - theme::TITLEBAR_H - theme::HEADER_H - 4.0).max(50.0);
            let cols = ((est_w / CELL_W) as u16).max(80);
            let rows = ((est_h / CELL_H) as u16).max(24);
            (cols, rows)
        };

        let did_restore = app.settings.restore_last_session && app.restore_session();
        if !did_restore {
            let cwd = app
                .settings
                .default_workspace_id
                .and_then(|id| app.workspace_store.workspaces.iter().find(|w| w.id == id))
                .map(|w| w.path.clone());
            app.spawn_session(&default_shell(), init_cols, init_rows, cwd);
            if let Some(ws_id) = app.settings.default_workspace_id {
                app.active_group = Some(ws_id);
            }
        }
        app
    }

    fn spawn_session(
        &mut self,
        shell: &ShellKind,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
    ) -> Option<u32> {
        match self.session_manager.spawn(cols, rows, cwd, shell) {
            Ok((id, session, master, writer, shell_pid, alive, is_active)) => {
                let entry = SessionEntry {
                    id,
                    session,
                    writer,
                    master,
                    shell_pid,
                    alive,
                    is_active,
                    pending_command: None,
                    shell: shell.clone(),
                };
                if self.active_id.is_none() {
                    self.active_id = Some(id);
                }
                self.uninit_sessions.insert(id);
                self.sessions.push(entry);
                if self.panes.is_empty() {
                    let pane_id = self.next_pane_id;
                    self.next_pane_id += 1;
                    self.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::Terminal(id),
                        manual_width: None,
                        last_size: (cols, rows),
                    });
                    self.active_pane_id = Some(pane_id);
                }
                Some(id)
            }
            Err(e) => {
                log::error!("Failed to spawn session: {e}");
                None
            }
        }
    }

    fn active_session_index(&self) -> Option<usize> {
        let id = self.active_id?;
        self.sessions.iter().position(|e| e.id == id)
    }

    fn active_cwd(&self) -> Option<PathBuf> {
        let idx = self.active_session_index()?;
        let p = self.sessions[idx].session.read().cwd.clone();
        if p.as_os_str().is_empty() {
            None
        } else {
            Some(p)
        }
    }

    /// Computes which workspace group a pane belongs to.
    /// Terminal panes: group = workspace whose path is a prefix of the session's CWD.
    /// File editor panes: group = the workspace_id stored at open time.
    fn pane_group(
        sessions: &[SessionEntry],
        ws_store: &WorkspaceStore,
        pane: &PaneEntry,
    ) -> Option<u64> {
        match &pane.content {
            PaneContent::Terminal(sid) => sessions.iter().find(|e| e.id == *sid).and_then(|e| {
                let cwd = e.session.read().cwd.clone();
                if cwd.as_os_str().is_empty() {
                    return None;
                }
                ws_store.find_for_cwd(&cwd).map(|w| w.id)
            }),
            PaneContent::DeferredTerminal { cwd, .. } => {
                cwd.as_ref().and_then(|c| ws_store.find_for_cwd(c).map(|w| w.id))
            }
            PaneContent::FileEditor(ed) => ed.workspace_id,
        }
    }

    /// Active workspace: whichever group is currently selected.
    fn active_workspace(&self) -> Option<&Workspace> {
        let ws_id = self.active_group?;
        self.workspace_store
            .workspaces
            .iter()
            .find(|w| w.id == ws_id)
    }

    /// Focus a pane and sync active_id for terminal panes.
    fn activate_pane(&mut self, pid: u32) {
        self.active_pane_id = Some(pid);
        if let Some(pane) = self.panes.iter().find(|p| p.id == pid) {
            if let PaneContent::Terminal(sid) = pane.content {
                self.active_id = Some(sid);
                self.update_is_active_flags();
            }
            // DeferredTerminal: active_id unchanged until the session materializes
        }
    }

    /// Switch to a workspace group (or "Other" when group = None).
    /// Restores the last active pane in the group, or spawns a new session if the group is empty.
    fn switch_group(&mut self, group: Option<u64>, cols: u16, rows: u16) {
        self.active_group = group;

        let panes_in_group: Vec<u32> = self
            .panes
            .iter()
            .filter(|p| Self::pane_group(&self.sessions, &self.workspace_store, p) == group)
            .map(|p| p.id)
            .collect();

        // Restore last-visited pane
        if let Some(&last_pid) = self.last_pane_per_group.get(&group) {
            if panes_in_group.contains(&last_pid) {
                self.activate_pane(last_pid);
                return;
            }
        }

        // Fall back to first pane in group
        if let Some(&first_pid) = panes_in_group.first() {
            self.activate_pane(first_pid);
            return;
        }

        // No panes in this group — spawn a new session
        let cwd = group.and_then(|ws_id| {
            self.workspace_store
                .workspaces
                .iter()
                .find(|w| w.id == ws_id)
                .map(|w| w.path.clone())
        });
        if let Some(sid) = self.spawn_session(&default_shell(), cols, rows, cwd) {
            self.active_id = Some(sid);
            // spawn_session only auto-creates a pane when panes is empty; add one explicitly here
            if !self
                .panes
                .iter()
                .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == sid))
            {
                let pane_id = self.next_pane_id;
                self.next_pane_id += 1;
                self.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::Terminal(sid),
                    manual_width: None,
                    last_size: (cols, rows),
                });
                self.active_pane_id = Some(pane_id);
            }
            self.update_is_active_flags();
        }
    }

    fn spawn_session_no_pane(
        &mut self,
        shell: &ShellKind,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
    ) -> Option<u32> {
        match self.session_manager.spawn(cols, rows, cwd, shell) {
            Ok((id, session, master, writer, shell_pid, alive, is_active)) => {
                self.uninit_sessions.insert(id);
                self.sessions.push(SessionEntry {
                    id,
                    session,
                    writer,
                    master,
                    shell_pid,
                    alive,
                    is_active,
                    pending_command: None,
                    shell: shell.clone(),
                });
                Some(id)
            }
            Err(e) => {
                log::error!("Failed to restore session: {e}");
                None
            }
        }
    }

    /// Sync the `is_active` flag on every session entry with `self.active_id`.
    /// Called whenever `active_id` changes so background reader threads throttle
    /// their repaint cadence.
    fn update_is_active_flags(&self) {
        let active = self.active_id;
        for entry in &self.sessions {
            entry.is_active.store(active == Some(entry.id), Ordering::Relaxed);
        }
    }

    fn save_session(&self) {
        let Some(path) = session_data_path() else {
            return;
        };

        let session_id_to_index: HashMap<u32, usize> = self
            .sessions
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        let pane_id_to_index: HashMap<u32, usize> = self
            .panes
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id, i))
            .collect();

        let sessions = self
            .sessions
            .iter()
            .map(|e| {
                let cwd = e.session.read().cwd.clone();
                let command = crate::pty::foreground::detect_child(e.shell_pid).map(|fp| {
                    let parts: Vec<String> =
                        fp.cmdline.iter().map(|a| shell_escape_arg(a)).collect();
                    let joined = parts.join(" ");
                    #[cfg(target_os = "windows")]
                    {
                        format!("& {}", joined)
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        joined
                    }
                });
                SavedSession { cwd, command }
            })
            .collect();

        let panes = self
            .panes
            .iter()
            .map(|p| SavedPane {
                content: match &p.content {
                    PaneContent::Terminal(sid) => SavedPaneContent::Terminal {
                        session_index: session_id_to_index.get(sid).copied().unwrap_or(0),
                    },
                    PaneContent::DeferredTerminal { cwd, pending_command } => {
                        SavedPaneContent::DeferredTerminal {
                            cwd: cwd.clone().unwrap_or_default(),
                            command: pending_command.clone(),
                        }
                    }
                    PaneContent::FileEditor(ed) => SavedPaneContent::FileEditor {
                        path: ed.path.clone(),
                        content: ed.content.clone(),
                        dirty: ed.dirty,
                        workspace_id: ed.workspace_id,
                    },
                },
                manual_width: p.manual_width,
            })
            .collect();

        let active_pane_index = self
            .active_pane_id
            .and_then(|pid| pane_id_to_index.get(&pid).copied());
        let active_session_index = self
            .active_id
            .and_then(|sid| session_id_to_index.get(&sid).copied());
        let last_pane_per_group = self
            .last_pane_per_group
            .iter()
            .filter_map(|(&g, &pid)| pane_id_to_index.get(&pid).map(|&i| (g, i)))
            .collect();

        let right_tab = match &self.right_tab {
            RightTab::Directory => SavedRightTab::Directory,
            RightTab::GitDiff => SavedRightTab::GitDiff,
            RightTab::Markdown(p) => SavedRightTab::Markdown(p.clone()),
        };

        let state = AppSession {
            sessions,
            panes,
            active_pane_index,
            active_session_index,
            active_group: self.active_group,
            last_pane_per_group,
            workspace_panel_ratio: self.workspace_panel_ratio,
            workspace_panel_collapsed: self.workspace_panel_collapsed,
            notes_panel_ratio: self.notes_panel_ratio,
            notes_panel_collapsed: self.notes_panel_collapsed,
            right_tab,
            shown_md_tabs: self.shown_md_tabs.iter().cloned().collect(),
        };

        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(&state) {
            let _ = std::fs::write(path, text);
        }
    }

    fn restore_session(&mut self) -> bool {
        let Some(path) = session_data_path() else {
            return false;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return false;
        };
        let Ok(state) = serde_json::from_str::<AppSession>(&text) else {
            return false;
        };
        if state.sessions.is_empty() && state.panes.is_empty() {
            return false;
        }

        // Determine which saved session index the active pane needs so we can
        // spawn it eagerly; all other terminal panes are deferred until focused.
        let active_session_idx: Option<usize> = state
            .active_pane_index
            .and_then(|pi| state.panes.get(pi))
            .and_then(|p| {
                if let SavedPaneContent::Terminal { session_index } = &p.content {
                    Some(*session_index)
                } else {
                    None
                }
            });

        // Spawn only the active pane's session immediately.
        let mut eagerly_spawned: HashMap<usize, u32> = HashMap::new();
        if let Some(active_idx) = active_session_idx {
            if let Some(s) = state.sessions.get(active_idx) {
                let cwd = if s.cwd.as_os_str().is_empty() {
                    None
                } else {
                    Some(s.cwd.clone())
                };
                if let Some(sid) = self.spawn_session_no_pane(&default_shell(), 80, 24, cwd) {
                    if let Some(cmd) = s.command.clone() {
                        if let Some(entry) = self.sessions.iter_mut().find(|e| e.id == sid) {
                            entry.pending_command = Some(cmd);
                        }
                    }
                    eagerly_spawned.insert(active_idx, sid);
                }
            }
        }

        let mut pane_ids: Vec<u32> = Vec::new();
        for saved in &state.panes {
            let content = match &saved.content {
                SavedPaneContent::Terminal { session_index } => {
                    if let Some(&sid) = eagerly_spawned.get(session_index) {
                        PaneContent::Terminal(sid)
                    } else {
                        // Not the active session — defer until first focus.
                        // Recover cwd/command from the saved sessions list so the
                        // terminal starts in the right directory when materialized.
                        let cwd = state.sessions.get(*session_index).and_then(|s| {
                            if s.cwd.as_os_str().is_empty() {
                                None
                            } else {
                                Some(s.cwd.clone())
                            }
                        });
                        let pending_command =
                            state.sessions.get(*session_index).and_then(|s| s.command.clone());
                        PaneContent::DeferredTerminal { cwd, pending_command }
                    }
                }
                SavedPaneContent::DeferredTerminal { cwd, command } => {
                    PaneContent::DeferredTerminal {
                        cwd: if cwd.as_os_str().is_empty() {
                            None
                        } else {
                            Some(cwd.clone())
                        },
                        pending_command: command.clone(),
                    }
                }
                SavedPaneContent::FileEditor {
                    path,
                    content,
                    dirty,
                    workspace_id,
                } => PaneContent::FileEditor(FileEditorState {
                    path: path.clone(),
                    content: content.clone(),
                    dirty: *dirty,
                    save_error: false,
                    workspace_id: *workspace_id,
                }),
            };
            let pane_id = self.next_pane_id;
            self.next_pane_id += 1;
            pane_ids.push(pane_id);
            // last_size (0, 0) forces a resize on the first rendered frame so the PTY
            // gets the actual terminal dimensions rather than the 80x24 spawn size.
            self.panes.push(PaneEntry {
                id: pane_id,
                content,
                manual_width: saved.manual_width,
                last_size: (0, 0),
            });
        }

        if let Some(idx) = state.active_pane_index {
            if let Some(&pid) = pane_ids.get(idx) {
                self.activate_pane(pid);
            }
        }
        if self.active_pane_id.is_none() {
            if let Some(&pid) = pane_ids.first() {
                self.activate_pane(pid);
            }
        }
        if self.active_id.is_none() {
            if let Some(&sid) = eagerly_spawned.values().next() {
                self.active_id = Some(sid);
                self.update_is_active_flags();
            }
        }

        self.active_group = state.active_group;
        for (group, pane_index) in &state.last_pane_per_group {
            if let Some(&pid) = pane_ids.get(*pane_index) {
                self.last_pane_per_group.insert(*group, pid);
            }
        }

        self.workspace_panel_ratio = state.workspace_panel_ratio;
        self.workspace_panel_collapsed = state.workspace_panel_collapsed;
        self.notes_panel_ratio = state.notes_panel_ratio;
        self.notes_panel_collapsed = state.notes_panel_collapsed;

        self.right_tab = match &state.right_tab {
            SavedRightTab::Directory => RightTab::Directory,
            SavedRightTab::GitDiff => RightTab::GitDiff,
            SavedRightTab::Markdown(p) => RightTab::Markdown(p.clone()),
        };
        self.shown_md_tabs = state.shown_md_tabs.into_iter().collect();

        true
    }

    /// Record last active pane for the current group.
    fn track_active_pane_group(&mut self) {
        if let Some(pid) = self.active_pane_id {
            if let Some(pane) = self.panes.iter().find(|p| p.id == pid) {
                let group = Self::pane_group(&self.sessions, &self.workspace_store, pane);
                if group == self.active_group {
                    self.last_pane_per_group.insert(self.active_group, pid);
                }
            }
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_session();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request an extra repaint the frame after the window gains focus so that
        // any stale wgpu surface frames (visible as a distorted first frame after
        // restoring from the taskbar) are immediately replaced with a correct one.
        {
            let focused = ctx.input(|i| i.focused);
            if focused && !self.was_focused {
                ctx.request_repaint();
                ctx.request_repaint_after(Duration::from_millis(16));
            }
            self.was_focused = focused;
        }

        // Cursor blink: toggle every 500 ms, schedule next repaint
        {
            if self.cursor_blink_last.elapsed() >= Duration::from_millis(500) {
                self.cursor_blink_on = !self.cursor_blink_on;
                self.cursor_blink_last = Instant::now();
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }

        // ── Track last active pane per group ───────────────────────────────
        self.track_active_pane_group();

        // ── Send pending terminal responses (DSR / DA1 / DA2) ──────────────
        // Read lock first — avoids taking a write lock on every session every frame
        // when most sessions have nothing to send (the common case).
        for i in 0..self.sessions.len() {
            if self.sessions[i].session.read().pending_dsr_response.is_empty() {
                continue;
            }
            let responses = {
                let mut session = self.sessions[i].session.write();
                std::mem::take(&mut session.pending_dsr_response)
            };
            for resp in responses {
                log::trace!("PTY[{}] OUT {:?}", self.sessions[i].id, resp);
                SessionManager::write(&mut self.sessions[i].writer, resp.as_bytes());
            }
        }

        // Send deferred duplicate commands once the shell signals it is at a prompt (OSC 7).
        for entry in &mut self.sessions {
            if entry.pending_command.is_some() {
                let ready = entry.session.read().prompt_ready;
                if ready {
                    let cmd = entry.pending_command.take().unwrap();
                    log::debug!("PTY[{}] replaying command: {:?}", entry.id, cmd);
                    // Windows ConPTY/PSReadLine executes on \r, not \n.
                    #[cfg(target_os = "windows")]
                    SessionManager::write(&mut entry.writer, format!("{}\r", cmd).as_bytes());
                    #[cfg(not(target_os = "windows"))]
                    SessionManager::write(&mut entry.writer, format!("{}\n", cmd).as_bytes());
                } else {
                    // Keep repainting until the prompt arrives.
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                }
            }
        }

        // Poll quickly while any session is still initializing (CWD not set yet).
        // Only check sessions we know are still uninitialized to avoid per-frame
        // read locks on all sessions in the steady state.
        if !self.uninit_sessions.is_empty() {
            self.uninit_sessions.retain(|&id| {
                self.sessions
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.session.read().cwd.as_os_str().is_empty())
                    .unwrap_or(false)
            });
            if !self.uninit_sessions.is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }

        // Restore cursor row from pre-resize snapshot if the shell erased it
        // without immediately redrawing (common PSReadLine/Windows behavior).
        if !self.resize_snapshots.is_empty() {
            let now = Instant::now();
            let snaps: Vec<(u32, u16, Vec<Cell>)> = self
                .resize_snapshots
                .iter()
                .filter(|(_, s)| now <= s.expires)
                .map(|(&sid, s)| (sid, s.cursor_y, s.cells.clone()))
                .collect();
            self.resize_snapshots.retain(|_, s| now <= s.expires);
            for (sid, snap_cy, snap_cells) in snaps {
                if let Some(entry) = self.sessions.iter().find(|e| e.id == sid) {
                    let mut sess = entry.session.write();
                    let row_blank = (0..sess.grid.cols).all(|c| sess.grid.get(snap_cy, c).c == ' ');
                    if row_blank {
                        for (c, &cell) in snap_cells.iter().enumerate() {
                            if (c as u16) < sess.grid.cols {
                                sess.grid.set(snap_cy, c as u16, cell);
                            }
                        }
                    }
                }
            }
            if !self.resize_snapshots.is_empty() {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }

        // ── Sync watchers + process FS events ──────────────────────────────
        if let Some(ws) = &mut self.watch_state {
            // Resync when sessions are added/removed or after 1s to catch CWD changes.
            let session_count = self.sessions.len();
            let now = Instant::now();
            if session_count != ws.last_session_count
                || now.duration_since(ws.last_sync) >= Duration::from_secs(1)
            {
                ws.sync(&self.sessions);
                ws.last_sync = now;
                ws.last_session_count = session_count;
            }
            let (created_md, removed_md) = ws.process_events();
            for path in created_md {
                self.shown_md_tabs.insert(path);
            }
            for path in removed_md {
                self.shown_md_tabs.remove(&path);
            }
        }

        // Validate current right_tab; fall back to Directory if stale
        {
            let keep = match &self.right_tab {
                RightTab::Directory => true,
                RightTab::GitDiff => self
                    .active_cwd()
                    .and_then(|cwd| self.watch_state.as_ref()?.dir_data.get(&cwd))
                    .map(|d| d.is_git)
                    .unwrap_or(false),
                RightTab::Markdown(p) => self.shown_md_tabs.contains(p),
            };
            if !keep {
                self.right_tab = RightTab::Directory;
            }
        }

        // ── Update window title with active workspace ───────────────────────
        let ws_title: String = self
            .active_workspace()
            .map(|w| format!("Terminal Studio — {}", w.name))
            .unwrap_or_else(|| "Terminal Studio".to_string());
        let active_ws_color: Option<[u8; 3]> = self.active_workspace().map(|w| w.color);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(ws_title.clone()));

        // ── Custom titlebar ─────────────────────────────────────────────────
        let tb_bg = match active_ws_color {
            Some(c) => theme::from_rgb(c),
            None => theme::BG_PANEL_FILL,
        };
        let tb_fg = active_ws_color
            .map(theme::text_on)
            .unwrap_or(theme::SUBTEXT1);

        egui::TopBottomPanel::top("titlebar")
            .exact_height(theme::TITLEBAR_H)
            .frame(egui::Frame::none().fill(tb_bg))
            .show(ctx, |ui| {
                let r = ui.max_rect();
                let painter = ui.painter().clone();

                // Drag the whole bar to move the window
                if ui
                    .interact(r, egui::Id::new("tb_drag"), egui::Sense::drag())
                    .dragged()
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                // ── macOS: traffic lights on the left ──────────────────────
                #[cfg(target_os = "macos")]
                {
                    let btn_y = r.center().y;
                    // hover_any: show colour only when any circle is hovered
                    let hover_pos = ctx.input(|i| i.pointer.hover_pos());
                    let hover_any = hover_pos
                        .map(|p| {
                            [18.0_f32, 38.0, 58.0].iter().any(|&ox| {
                                (p.x - (r.min.x + ox)).abs() < 8.0 && (p.y - btn_y).abs() < 8.0
                            })
                        })
                        .unwrap_or(false);

                    let circles: &[(f32, egui::Color32, usize)] = &[
                        (r.min.x + 18.0, egui::Color32::from_rgb(255, 96, 89), 0), // close
                        (r.min.x + 38.0, egui::Color32::from_rgb(255, 189, 68), 1), // minimize
                        (r.min.x + 58.0, egui::Color32::from_rgb(39, 201, 63), 2), // maximize
                    ];
                    for &(cx, color, idx) in circles {
                        let pos = egui::pos2(cx, btn_y);
                        let brect = egui::Rect::from_center_size(pos, egui::vec2(14.0, 14.0));
                        let resp = ui.interact(
                            brect,
                            egui::Id::new(("tb_mac", idx)),
                            egui::Sense::click(),
                        );
                        let fill = if hover_any {
                            color
                        } else {
                            egui::Color32::from_gray(80)
                        };
                        painter.circle_filled(pos, 6.0, fill);
                        if resp.clicked() {
                            match idx {
                                0 => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                                1 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                                _ => {
                                    let is_max =
                                        ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                                        !is_max,
                                    ));
                                }
                            }
                        }
                    }
                    // Left panel toggle (after traffic lights)
                    let mac_btn_w = 28.0_f32;
                    let left_tbr = egui::Rect::from_min_size(
                        egui::pos2(r.min.x + 72.0, r.min.y),
                        egui::vec2(mac_btn_w, r.height()),
                    );
                    let left_resp = ui.interact(
                        left_tbr,
                        egui::Id::new("tb_left_toggle"),
                        egui::Sense::click(),
                    );
                    if left_resp.hovered() {
                        painter.rect_filled(left_tbr, 0.0, theme::SURFACE1);
                    }
                    let left_icon = if self.show_left_panel { "◀" } else { "▶" };
                    painter.text(
                        left_tbr.center(),
                        egui::Align2::CENTER_CENTER,
                        left_icon,
                        egui::FontId::proportional(12.0),
                        tb_fg,
                    );
                    if left_resp.clicked() {
                        self.show_left_panel = !self.show_left_panel;
                    }

                    // Gear / Settings (rightmost on macOS)
                    let gear_mac_tbr = egui::Rect::from_min_size(
                        egui::pos2(r.max.x - mac_btn_w, r.min.y),
                        egui::vec2(mac_btn_w, r.height()),
                    );
                    let gear_mac_resp = ui.interact(
                        gear_mac_tbr,
                        egui::Id::new("tb_settings"),
                        egui::Sense::click(),
                    );
                    if gear_mac_resp.hovered() || self.show_settings {
                        painter.rect_filled(gear_mac_tbr, 0.0, theme::SURFACE1);
                    }
                    painter.text(
                        gear_mac_tbr.center(),
                        egui::Align2::CENTER_CENTER,
                        "⚙",
                        egui::FontId::proportional(14.0),
                        tb_fg,
                    );
                    if gear_mac_resp.clicked() {
                        self.show_settings = !self.show_settings;
                    }

                    // Right panel toggle (before gear on macOS)
                    let right_tbr = egui::Rect::from_min_size(
                        egui::pos2(r.max.x - mac_btn_w * 2.0, r.min.y),
                        egui::vec2(mac_btn_w, r.height()),
                    );
                    let right_resp = ui.interact(
                        right_tbr,
                        egui::Id::new("tb_right_toggle"),
                        egui::Sense::click(),
                    );
                    if right_resp.hovered() {
                        painter.rect_filled(right_tbr, 0.0, theme::SURFACE1);
                    }
                    let right_icon = if self.show_right_panel { "▶" } else { "◀" };
                    painter.text(
                        right_tbr.center(),
                        egui::Align2::CENTER_CENTER,
                        right_icon,
                        egui::FontId::proportional(12.0),
                        tb_fg,
                    );
                    if right_resp.clicked() {
                        self.show_right_panel = !self.show_right_panel;
                    }

                    // Title centered between the two toggles
                    painter.text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        &ws_title,
                        egui::FontId::proportional(13.0),
                        tb_fg,
                    );
                }

                // ── Windows / Linux: controls on the right ─────────────────
                #[cfg(not(target_os = "macos"))]
                {
                    let btn_w = theme::TITLEBAR_BTN_W;
                    // right-to-left: close(0), maximize(1), minimize(2)
                    let btns: &[(&str, usize, bool)] = &[
                        ("×", 0, true),  // close   — danger colour on hover
                        ("□", 1, false), // maximize
                        ("–", 2, false), // minimize
                    ];

                    // Left panel toggle — leftmost button
                    {
                        let br = egui::Rect::from_min_size(
                            egui::pos2(r.min.x, r.min.y),
                            egui::vec2(btn_w, r.height()),
                        );
                        let resp =
                            ui.interact(br, egui::Id::new("tb_left_toggle"), egui::Sense::click());
                        let bg = if resp.hovered() {
                            theme::SURFACE1
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        painter.rect_filled(br, 0.0, bg);
                        let icon = if self.show_left_panel { "◀" } else { "▶" };
                        painter.text(
                            br.center(),
                            egui::Align2::CENTER_CENTER,
                            icon,
                            egui::FontId::proportional(12.0),
                            tb_fg,
                        );
                        if resp.clicked() {
                            self.show_left_panel = !self.show_left_panel;
                        }
                    }

                    // Gear / Settings button — just before window controls
                    {
                        let gear_x = r.max.x - btn_w * (btns.len() as f32 + 1.0);
                        let br = egui::Rect::from_min_size(
                            egui::pos2(gear_x, r.min.y),
                            egui::vec2(btn_w, r.height()),
                        );
                        let resp =
                            ui.interact(br, egui::Id::new("tb_settings"), egui::Sense::click());
                        let bg = if resp.hovered() || self.show_settings {
                            theme::SURFACE1
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        painter.rect_filled(br, 0.0, bg);
                        painter.text(
                            br.center(),
                            egui::Align2::CENTER_CENTER,
                            "⚙",
                            egui::FontId::proportional(14.0),
                            tb_fg,
                        );
                        if resp.clicked() {
                            self.show_settings = !self.show_settings;
                        }
                    }

                    // Right panel toggle — just before gear button
                    let right_toggle_x = r.max.x - btn_w * (btns.len() as f32 + 2.0);
                    {
                        let br = egui::Rect::from_min_size(
                            egui::pos2(right_toggle_x, r.min.y),
                            egui::vec2(btn_w, r.height()),
                        );
                        let resp =
                            ui.interact(br, egui::Id::new("tb_right_toggle"), egui::Sense::click());
                        let bg = if resp.hovered() {
                            theme::SURFACE1
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        painter.rect_filled(br, 0.0, bg);
                        let icon = if self.show_right_panel { "▶" } else { "◀" };
                        painter.text(
                            br.center(),
                            egui::Align2::CENTER_CENTER,
                            icon,
                            egui::FontId::proportional(12.0),
                            tb_fg,
                        );
                        if resp.clicked() {
                            self.show_right_panel = !self.show_right_panel;
                        }
                    }

                    for &(symbol, idx, is_danger) in btns {
                        let x = r.max.x - btn_w * (idx as f32 + 1.0);
                        let br = egui::Rect::from_min_size(
                            egui::pos2(x, r.min.y),
                            egui::vec2(btn_w, r.height()),
                        );
                        let resp =
                            ui.interact(br, egui::Id::new(("tb_btn", idx)), egui::Sense::click());
                        let bg = if resp.hovered() {
                            if is_danger {
                                theme::DANGER_BG
                            } else {
                                theme::SURFACE1
                            }
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        painter.rect_filled(br, 0.0, bg);
                        let fg = if resp.hovered() && is_danger {
                            egui::Color32::WHITE
                        } else {
                            tb_fg
                        };
                        painter.text(
                            br.center(),
                            egui::Align2::CENTER_CENTER,
                            symbol,
                            egui::FontId::proportional(12.0),
                            fg,
                        );
                        if resp.clicked() {
                            match idx {
                                0 => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                                1 => {
                                    let is_max =
                                        ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                                        !is_max,
                                    ));
                                }
                                2 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                                _ => {}
                            }
                        }
                    }
                    // Title between left toggle and right toggle
                    let clip_min_x = r.min.x + btn_w + 4.0;
                    let clip_max_x = right_toggle_x - 4.0;
                    painter
                        .with_clip_rect(egui::Rect::from_min_max(
                            egui::pos2(clip_min_x, r.min.y),
                            egui::pos2(clip_max_x, r.max.y),
                        ))
                        .text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            &ws_title,
                            egui::FontId::proportional(13.0),
                            tb_fg,
                        );
                }
            });

        // ── Foreground process detection (500 ms cached) ──────────────────
        // Refresh only when the active session changes or the TTL expires.
        let active_fg: Option<ForegroundProcess> = {
            let need_refresh = self.foreground_cache.as_ref().map_or(true, |c| {
                c.session_id != self.active_id.unwrap_or(0)
                    || c.checked_at.elapsed() > Duration::from_millis(500)
            });
            if need_refresh {
                let result = self
                    .active_id
                    .and_then(|sid| self.sessions.iter().find(|e| e.id == sid))
                    .filter(|e| e.alive.load(Ordering::Relaxed))
                    .and_then(|e| crate::pty::foreground::detect_child(e.shell_pid));
                self.foreground_cache = Some(CachedForeground {
                    session_id: self.active_id.unwrap_or(0),
                    result,
                    checked_at: Instant::now(),
                });
            }
            self.foreground_cache
                .as_ref()
                .and_then(|c| c.result.clone())
        };

        // ── Left panel: sessions (top) + workspaces (bottom) ───────────────
        let mut spawn_new_session: Option<ShellKind> = None;
        let mut duplicate_session = false;
        let shells = self.available_shells.clone();
        let mut open_workspace_id: Option<u64> = None;
        let mut edit_workspace_id: Option<u64> = None;
        let mut quit_session_id: Option<u32> = None;
        let mut clicked_session_id: Option<u32> = None;
        let mut clicked_session_ws_group: Option<u64> = None;

        if self.show_left_panel {
            egui::SidePanel::left("sessions")
            .default_width(theme::LEFT_SIDEBAR_W)
            .width_range(80.0..=400.0)
            .resizable(true)
            .show(ctx, |ui| {
                let panel_rect = ui.max_rect();
                let panel_w    = panel_rect.width();
                let total_h    = panel_rect.height();

                const DIV_H:       f32 = 4.0;
                const COLLAPSED_H: f32 = theme::HEADER_H;

                // ── Height allocation ──────────────────────────────────────
                let (sess_h, ws_h) = if self.workspace_panel_collapsed {
                    (total_h - COLLAPSED_H - DIV_H, COLLAPSED_H)
                } else {
                    let wh = (total_h * self.workspace_panel_ratio).max(60.0);
                    let sh = (total_h - wh - DIV_H).max(60.0);
                    (sh, wh)
                };

                // Claim the full panel rect so egui's layout system doesn't
                // re-use this space for anything else.
                ui.allocate_rect(panel_rect, egui::Sense::hover());

                // ── Sessions section ───────────────────────────────────────
                let sess_rect = egui::Rect::from_min_size(
                    panel_rect.min,
                    egui::vec2(panel_w, sess_h),
                );
                ui.allocate_ui_at_rect(sess_rect, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), theme::HEADER_H),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(egui::RichText::new("Sessions").strong().size(theme::HEADER_FONT_SZ));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.menu_button(
                                    egui::RichText::new("+ New ▾").size(theme::HEADER_FONT_SZ),
                                    |ui| {
                                        for shell in &shells {
                                            if ui.button(shell.display_name()).clicked() {
                                                spawn_new_session = Some(shell.clone());
                                                ui.close_menu();
                                            }
                                        }
                                    },
                                );
                                if let Some(ref fp) = active_fg {
                                    if ui.button(egui::RichText::new("Duplicate").size(theme::HEADER_FONT_SZ))
                                        .on_hover_text(format!("Duplicate: {}", fp.name))
                                        .clicked()
                                    {
                                        duplicate_session = true;
                                    }
                                }
                            });
                        }
                    );
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_source("sessions_scroll")
                        .show(ui, |ui| {
                            for entry in &self.sessions {
                                let (title, cwd) = {
                                    let s = entry.session.read();
                                    (s.title.clone(), s.cwd.clone())
                                };
                                let is_active = self.active_id == Some(entry.id);

                                let ws_color: Option<[u8; 3]> = if cwd.as_os_str().is_empty() {
                                    None
                                } else {
                                    self.workspace_store.find_for_cwd(&cwd).map(|w| w.color)
                                };

                                let (resp, painter) = ui.allocate_painter(
                                    egui::vec2(ui.available_width(), theme::SESSION_ROW_H),
                                    egui::Sense::click(),
                                );
                                let row_rect = resp.rect;

                                // Quit button — always reserved at right edge
                                let quit_rect = egui::Rect::from_min_size(
                                    egui::pos2(row_rect.max.x - theme::QUIT_W, row_rect.min.y),
                                    egui::vec2(theme::QUIT_W, row_rect.height()),
                                );
                                let quit_resp = ui.interact(
                                    quit_rect,
                                    egui::Id::new(("session_quit", entry.id)),
                                    egui::Sense::click(),
                                );

                                let bg = if is_active {
                                    theme::BG_ROW_ACTIVE
                                } else if resp.hovered() || quit_resp.hovered() {
                                    theme::BG_ROW_HOVER
                                } else {
                                    egui::Color32::TRANSPARENT
                                };
                                painter.rect_filled(row_rect, 0.0, bg);

                                if let Some(c) = ws_color {
                                    let border = egui::Rect::from_min_size(
                                        row_rect.min,
                                        egui::vec2(theme::WS_BORDER_W, row_rect.height()),
                                    );
                                    painter.rect_filled(border, 0.0, theme::from_rgb(c));
                                }

                                // Draw quit button
                                if quit_resp.hovered() {
                                    painter.rect_filled(quit_rect, 0.0, theme::DANGER_BG);
                                }
                                painter.text(
                                    quit_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    "×",
                                    egui::FontId::proportional(14.0),
                                    theme::DANGER_FG,
                                );

                                // Pane indicator badge (P1, P2...) between title and quit btn
                                let pane_label: Option<String> = self.panes.iter().enumerate()
                                    .find(|(_, p)| matches!(&p.content, PaneContent::Terminal(sid) if *sid == entry.id))
                                    .map(|(idx, _)| format!("P{}", idx + 1));
                                let badge_w = if pane_label.is_some() { 22.0 } else { 0.0 };
                                if let Some(ref label) = pane_label {
                                    painter.text(
                                        egui::pos2(quit_rect.min.x - badge_w / 2.0 - 2.0, row_rect.center().y),
                                        egui::Align2::CENTER_CENTER,
                                        label,
                                        egui::FontId::proportional(10.0),
                                        theme::OVERLAY0,
                                    );
                                }

                                // Title text clipped to leave room for quit button + badge
                                let text_x = row_rect.min.x + if ws_color.is_some() { theme::WS_BORDER_W + theme::BAR_PAD_X } else { theme::BAR_PAD_X };
                                let clip_max = quit_rect.min.x - badge_w - 3.0;
                                painter.with_clip_rect(egui::Rect::from_min_max(
                                    egui::pos2(text_x, row_rect.min.y),
                                    egui::pos2(clip_max, row_rect.max.y),
                                )).text(
                                    egui::pos2(text_x, row_rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    effective_title(&title, &cwd),
                                    egui::FontId::proportional(theme::SESSION_FONT_SZ),
                                    if is_active { theme::TEXT } else { theme::SUBTEXT0 },
                                );

                                if quit_resp.clicked() {
                                    quit_session_id = Some(entry.id);
                                } else if resp.clicked() {
                                    let clicked_sid = entry.id;
                                    // Determine which workspace this session belongs to
                                    let ws_group = {
                                        let cwd = self.sessions.iter()
                                            .find(|e| e.id == clicked_sid)
                                            .map(|e| e.session.read().cwd.clone());
                                        cwd.and_then(|c| {
                                            if c.as_os_str().is_empty() { None }
                                            else { self.workspace_store.find_for_cwd(&c).map(|w| w.id) }
                                        })
                                    };
                                    clicked_session_id = Some(clicked_sid);
                                    clicked_session_ws_group = ws_group;
                                }
                            }
                        });
                });

                // ── Draggable divider ──────────────────────────────────────
                let div_top  = panel_rect.min.y + sess_h;
                let div_rect = egui::Rect::from_min_size(
                    egui::pos2(panel_rect.left(), div_top),
                    egui::vec2(panel_w, DIV_H),
                );
                let div_resp = ui.interact(
                    div_rect,
                    egui::Id::new("ws_panel_divider"),
                    egui::Sense::drag(),
                );
                if div_resp.hovered() || div_resp.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                let div_color = if div_resp.hovered() || div_resp.dragged() {
                    theme::WS_DIV_ACTIVE
                } else {
                    theme::WS_DIV_IDLE
                };
                ui.painter().rect_filled(div_rect, 0.0, div_color);
                if !self.workspace_panel_collapsed && div_resp.dragged() {
                    let delta     = div_resp.drag_delta().y;
                    // Drag down → workspace grows; drag up → workspace shrinks.
                    // ws_h and sess_h come from the ratio calculation above so we
                    // invert: moving the divider up means less workspace height.
                    let new_ws_h  = (ws_h - delta).clamp(60.0, total_h - 60.0 - DIV_H);
                    self.workspace_panel_ratio = new_ws_h / total_h;
                }

                // ── Workspaces section ─────────────────────────────────────
                let ws_top  = div_top + DIV_H;
                let ws_rect = egui::Rect::from_min_size(
                    egui::pos2(panel_rect.left(), ws_top),
                    egui::vec2(panel_w, ws_h),
                );
                ui.allocate_ui_at_rect(ws_rect, |ui| {
                    ui.painter().rect_filled(
                        ws_rect,
                        0.0,
                        theme::BG_WORKSPACE_FILL,
                    );

                    let ws_count = self.workspace_store.workspaces.len();
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), theme::HEADER_H),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            let arrow = if self.workspace_panel_collapsed { "▶" } else { "▼" };
                            if ui.add(
                                egui::Button::new(egui::RichText::new(arrow).size(11.0))
                                    .min_size(egui::vec2(theme::HEADER_H, theme::HEADER_H))
                                    .frame(true)
                            ).clicked() {
                                self.workspace_panel_collapsed = !self.workspace_panel_collapsed;
                            }
                            ui.label(
                                egui::RichText::new(format!("Workspaces ({})", ws_count))
                                    .strong()
                                    .size(theme::HEADER_FONT_SZ),
                            );
                        }
                    );

                    if !self.workspace_panel_collapsed {
                        let active_group_snap = self.active_group;
                        let workspaces: Vec<(u64, String, [u8; 3], bool)> = self
                            .workspace_store
                            .workspaces
                            .iter()
                            .map(|w| (w.id, w.name.clone(), w.color, !self.note_store.get(Some(w.id)).is_empty()))
                            .collect();

                        egui::ScrollArea::vertical()
                            .id_source("ws_panel_scroll")
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 3.0;
                                for (id, name, color, has_note) in &workspaces {
                                    let active = active_group_snap == Some(*id);
                                    let tint_factor = if active { 0.65 } else { 0.45 };
                                    let fill = theme::from_rgb(theme::tinted(*color, tint_factor));
                                    let fg   = theme::text_on(theme::tinted(*color, tint_factor));

                                    {
                                        const GEAR_W: f32 = 26.0;
                                        let full_w = ui.available_width();
                                        let stroke_val = if active {
                                            egui::Stroke::new(2.0, theme::TEXT)
                                        } else {
                                            egui::Stroke::new(1.0, theme::from_rgb(theme::tinted(*color, 0.30)))
                                        };
                                        let (full_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(full_w, theme::HEADER_H),
                                            egui::Sense::hover(),
                                        );
                                        let gear_rect = egui::Rect::from_min_size(
                                            egui::pos2(full_rect.max.x - GEAR_W, full_rect.min.y),
                                            egui::vec2(GEAR_W, full_rect.height()),
                                        );
                                        let name_rect = egui::Rect::from_min_max(
                                            full_rect.min,
                                            egui::pos2(gear_rect.min.x, full_rect.max.y),
                                        );
                                        let name_resp = ui.interact(name_rect, egui::Id::new(("ws_name", *id)), egui::Sense::click());
                                        let gear_resp = ui.interact(gear_rect, egui::Id::new(("ws_gear", *id)), egui::Sense::click());

                                        if ui.is_rect_visible(full_rect) {
                                            let rounding = egui::Rounding::same(4.0);
                                            ui.painter().rect_filled(full_rect, rounding, fill);
                                            ui.painter().rect_stroke(full_rect, rounding, stroke_val);

                                            let name_str = if active { format!("▶ {}", name) } else { name.clone() };
                                            let name_galley = ui.fonts(|f| {
                                                f.layout_no_wrap(name_str, egui::FontId::proportional(theme::SESSION_FONT_SZ), fg)
                                            });
                                            let text_y = full_rect.center().y - name_galley.size().y / 2.0;
                                            ui.painter().galley(
                                                egui::pos2(full_rect.left() + theme::BAR_PAD_X, text_y),
                                                name_galley,
                                                fg,
                                            );

                                            if *has_note {
                                                let note_galley = ui.fonts(|f| {
                                                    f.layout_no_wrap("📝".to_string(), egui::FontId::proportional(12.0), fg)
                                                });
                                                let note_x = gear_rect.left() - 4.0 - note_galley.size().x;
                                                ui.painter().galley(
                                                    egui::pos2(note_x, text_y),
                                                    note_galley,
                                                    fg,
                                                );
                                            }

                                            let gear_fg = if gear_resp.hovered() { theme::TEXT } else { theme::SUBTEXT0 };
                                            ui.painter().text(
                                                gear_rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "⚙",
                                                egui::FontId::proportional(12.0),
                                                gear_fg,
                                            );
                                        }

                                        if name_resp.clicked() { open_workspace_id = Some(*id); }
                                        if gear_resp.clicked() { edit_workspace_id = Some(*id); }
                                    }
                                }

                                // "Other" group — unaffiliated panes
                                let other_active   = active_group_snap.is_none();
                                let other_has_note = !self.note_store.get(None).is_empty();
                                let other_fill     = if other_active { theme::SURFACE2 } else { theme::SURFACE0 };
                                let other_fg       = if other_active { theme::TEXT } else { theme::SUBTEXT0 };
                                let other_stroke   = if other_active {
                                    egui::Stroke::new(2.0, theme::TEXT)
                                } else {
                                    egui::Stroke::new(1.0, theme::OVERLAY0)
                                };
                                let other_w = ui.available_width();
                                let (other_rect, other_resp) = ui.allocate_exact_size(
                                    egui::vec2(other_w, 28.0),
                                    egui::Sense::click(),
                                );
                                if ui.is_rect_visible(other_rect) {
                                    let rounding = egui::Rounding::same(4.0);
                                    ui.painter().rect_filled(other_rect, rounding, other_fill);
                                    ui.painter().rect_stroke(other_rect, rounding, other_stroke);

                                    let other_name = if other_active { "▶ Other".to_string() } else { "Other".to_string() };
                                    let other_galley = ui.fonts(|f| {
                                        f.layout_no_wrap(other_name, egui::FontId::proportional(13.0), other_fg)
                                    });
                                    let text_y = other_rect.center().y - other_galley.size().y / 2.0;
                                    ui.painter().galley(
                                        egui::pos2(other_rect.left() + 8.0, text_y),
                                        other_galley,
                                        other_fg,
                                    );

                                    if other_has_note {
                                        let note_galley = ui.fonts(|f| {
                                            f.layout_no_wrap("📝".to_string(), egui::FontId::proportional(12.0), other_fg)
                                        });
                                        let note_x = other_rect.right() - 8.0 - note_galley.size().x;
                                        ui.painter().galley(
                                            egui::pos2(note_x, text_y),
                                            note_galley,
                                            other_fg,
                                        );
                                    }
                                }
                                if other_resp.clicked() {
                                    open_workspace_id = Some(u64::MAX); // sentinel for "Other"
                                }
                            });
                    }
                });
            });
        } // end if self.show_left_panel

        if let Some(ws_id) = open_workspace_id {
            let (cols, rows) = self.panes.first().map(|p| p.last_size).unwrap_or((80, 24));
            // u64::MAX is the sentinel for the "Other" group
            let group = if ws_id == u64::MAX { None } else { Some(ws_id) };
            self.switch_group(group, cols, rows);
        }

        if let Some(ws_id) = edit_workspace_id {
            if let Some(ws) = self
                .workspace_store
                .workspaces
                .iter()
                .find(|w| w.id == ws_id)
            {
                self.workspace_edit_dialog =
                    Some(WorkspaceEditDialog::new(ws.id, ws.name.clone(), ws.color));
            }
        }

        if let Some(qid) = quit_session_id {
            // Remove all panes referencing this session
            let pane_ids: Vec<u32> = self
                .panes
                .iter()
                .filter(|p| matches!(&p.content, PaneContent::Terminal(sid) if *sid == qid))
                .map(|p| p.id)
                .collect();
            for pid in pane_ids {
                self.panes.retain(|p| p.id != pid);
                if self.active_pane_id == Some(pid) {
                    self.active_pane_id = self.panes.last().map(|p| p.id);
                }
            }
            self.uninit_sessions.remove(&qid);
            self.sessions.retain(|e| e.id != qid);
            if self.active_id == Some(qid) {
                self.active_id = self.sessions.first().map(|e| e.id);
                self.update_is_active_flags();
            }
            // Ensure active session is shown in a pane
            if self.panes.is_empty() {
                if let Some(new_sid) = self.active_id {
                    let pane_id = self.next_pane_id;
                    self.next_pane_id += 1;
                    self.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::Terminal(new_sid),
                        manual_width: None,
                        last_size: (0, 0),
                    });
                    self.active_pane_id = Some(pane_id);
                }
            }
            self.save_session();
        }

        if let Some(clicked_sid) = clicked_session_id {
            self.active_group = clicked_session_ws_group;
            let pid_opt = self
                .panes
                .iter()
                .find(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == clicked_sid))
                .map(|p| p.id);
            if let Some(pid) = pid_opt {
                self.activate_pane(pid);
                self.last_pane_per_group
                    .insert(clicked_session_ws_group, pid);
            } else {
                self.active_id = Some(clicked_sid);
                self.update_is_active_flags();
            }
        }

        if let Some(ref new_shell) = spawn_new_session {
            let new_shell = new_shell.clone();
            let cwd = self.active_cwd().or_else(|| {
                self.active_group.and_then(|gid| {
                    self.workspace_store
                        .workspaces
                        .iter()
                        .find(|w| w.id == gid)
                        .map(|w| w.path.clone())
                })
            });
            let (cols, rows) = self
                .panes
                .iter()
                .find(|p| Some(p.id) == self.active_pane_id)
                .map(|p| p.last_size)
                .unwrap_or_else(|| self.panes.first().map(|p| p.last_size).unwrap_or((80, 24)));
            if let Some(new_id) = self.spawn_session(&new_shell, cols, rows, cwd) {
                self.active_id = Some(new_id);
                if !self
                    .panes
                    .iter()
                    .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == new_id))
                {
                    let pane_id = self.next_pane_id;
                    self.next_pane_id += 1;
                    self.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::Terminal(new_id),
                        manual_width: None,
                        last_size: (cols, rows),
                    });
                    self.activate_pane(pane_id);
                }
            }
        }

        if duplicate_session {
            let dup_shell = self
                .sessions
                .iter()
                .find(|e| Some(e.id) == self.active_id)
                .map(|e| e.shell.clone())
                .unwrap_or_else(default_shell);
            let cwd = self.active_cwd().or_else(|| {
                self.active_group.and_then(|gid| {
                    self.workspace_store
                        .workspaces
                        .iter()
                        .find(|w| w.id == gid)
                        .map(|w| w.path.clone())
                })
            });
            let (cols, rows) = self
                .panes
                .iter()
                .find(|p| Some(p.id) == self.active_pane_id)
                .map(|p| p.last_size)
                .unwrap_or_else(|| self.panes.first().map(|p| p.last_size).unwrap_or((80, 24)));
            // Build the command string to replay in the new session
            let cmd_to_run: Option<String> = active_fg.as_ref().map(|fp| {
                let parts: Vec<String> = fp.cmdline.iter().map(|a| shell_escape_arg(a)).collect();
                let joined = parts.join(" ");
                // PowerShell does not invoke a quoted path string as a command without
                // the call operator; & works for both bare names and quoted full paths.
                #[cfg(target_os = "windows")]
                {
                    format!("& {}", joined)
                }
                #[cfg(not(target_os = "windows"))]
                {
                    joined
                }
            });
            if let Some(new_id) = self.spawn_session(&dup_shell, cols, rows, cwd) {
                self.active_id = Some(new_id);
                if !self
                    .panes
                    .iter()
                    .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == new_id))
                {
                    // Insert new pane immediately after the current active pane
                    let insert_at = self
                        .panes
                        .iter()
                        .position(|p| Some(p.id) == self.active_pane_id)
                        .map(|i| i + 1)
                        .unwrap_or(self.panes.len());
                    let pane_id = self.next_pane_id;
                    self.next_pane_id += 1;
                    self.panes.insert(
                        insert_at,
                        PaneEntry {
                            id: pane_id,
                            content: PaneContent::Terminal(new_id),
                            manual_width: None,
                            last_size: (cols, rows),
                        },
                    );
                    self.activate_pane(pane_id);
                }
                // Queue the command; it will be sent once the new shell emits OSC 7 (prompt ready).
                if let Some(cmd) = cmd_to_run {
                    if let Some(entry) = self.sessions.iter_mut().find(|e| e.id == new_id) {
                        entry.pending_command = Some(cmd);
                    }
                }
            }
        }

        // ── Snapshot right-panel data before closures capture self ──────────
        let active_cwd = self.active_cwd();
        let active_tab = self.right_tab.clone();

        let (is_git, git_diff, git_status, dir_entries, raw_md) =
            match (active_cwd.as_ref(), self.watch_state.as_ref()) {
                (Some(cwd), Some(ws)) => match ws.dir_data.get(cwd) {
                    Some(d) => (
                        d.is_git,
                        d.git_diff.clone(),
                        d.git_status.clone(),
                        d.dir_entries.clone(),
                        d.md_files
                            .iter()
                            .map(|(p, c)| (p.clone(), c.clone()))
                            .collect::<Vec<_>>(),
                    ),
                    None => (false, String::new(), String::new(), vec![], vec![]),
                },
                _ => (false, String::new(), String::new(), vec![], vec![]),
            };

        let mut md_tabs: Vec<(PathBuf, String)> = raw_md
            .into_iter()
            .filter(|(p, _)| self.shown_md_tabs.contains(p))
            .collect();
        md_tabs.sort_by(|(a, _), (b, _)| a.file_name().cmp(&b.file_name()));

        let mut new_tab: Option<RightTab> = None;
        let mut close_tab: Option<PathBuf> = None;
        let mut open_md: Option<PathBuf> = None;
        let mut open_editor: Option<PathBuf> = None;
        let mut open_ws_dialog: Option<PathBuf> = None;

        // Snapshot current note so TextEdit can mutate it inside the closure
        let mut note_text = self.note_store.get(self.active_group).to_string();

        // ── Right panel ──────────────────────────────────────────────────────
        if self.show_right_panel {
            egui::SidePanel::right("right_panel")
                .default_width(300.0)
                .width_range(100.0..=600.0)
                .resizable(true)
                .show(ctx, |ui| {
                    let panel_rect = ui.max_rect();
                    let panel_w = panel_rect.width();
                    let total_h = panel_rect.height();

                    const DIV_H: f32 = 4.0;
                    const COLLAPSED_H: f32 = theme::HEADER_H;

                    let (content_h, notes_h) = if self.notes_panel_collapsed {
                        (total_h - COLLAPSED_H - DIV_H, COLLAPSED_H)
                    } else {
                        let nh = (total_h * self.notes_panel_ratio).max(60.0);
                        let ch = (total_h - nh - DIV_H).max(60.0);
                        (ch, nh)
                    };

                    ui.allocate_rect(panel_rect, egui::Sense::hover());

                    // ── Top: directory / git / markdown tabs ─────────────────────
                    let content_rect =
                        egui::Rect::from_min_size(panel_rect.min, egui::vec2(panel_w, content_h));
                    ui.allocate_ui_at_rect(content_rect, |ui| {
                        egui::Frame::none()
                            .fill(theme::SURFACE0)
                            .inner_margin(egui::Margin::ZERO)
                            .show(ui, |ui| {
                                egui::ScrollArea::horizontal()
                                    .id_source("right_tab_bar")
                                    .max_height(theme::HEADER_H)
                                    .show(ui, |ui| {
                                        ui.set_min_height(theme::HEADER_H);
                                        ui.horizontal(|ui| {
                                            ui.set_min_height(theme::HEADER_H);
                                            ui.spacing_mut().item_spacing.x = 2.0;

                                            if ui
                                                .selectable_label(
                                                    active_tab == RightTab::Directory,
                                                    egui::RichText::new("Directory")
                                                        .size(theme::HEADER_FONT_SZ),
                                                )
                                                .clicked()
                                            {
                                                new_tab = Some(RightTab::Directory);
                                            }

                                            if is_git
                                                && ui
                                                    .selectable_label(
                                                        active_tab == RightTab::GitDiff,
                                                        egui::RichText::new("Git Diff")
                                                            .size(theme::HEADER_FONT_SZ),
                                                    )
                                                    .clicked()
                                            {
                                                new_tab = Some(RightTab::GitDiff);
                                            }

                                            for (path, _) in &md_tabs {
                                                let name = path
                                                    .file_name()
                                                    .map(|n| n.to_string_lossy().into_owned())
                                                    .unwrap_or_default();
                                                let is_active =
                                                    active_tab == RightTab::Markdown(path.clone());
                                                if ui
                                                    .selectable_label(
                                                        is_active,
                                                        egui::RichText::new(&name)
                                                            .size(theme::HEADER_FONT_SZ),
                                                    )
                                                    .clicked()
                                                {
                                                    new_tab =
                                                        Some(RightTab::Markdown(path.clone()));
                                                }
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new("×")
                                                                .size(theme::HEADER_FONT_SZ)
                                                                .color(theme::OVERLAY1),
                                                        )
                                                        .frame(false)
                                                        .min_size(egui::vec2(
                                                            theme::HEADER_H,
                                                            theme::HEADER_H,
                                                        )),
                                                    )
                                                    .on_hover_text("Close tab")
                                                    .clicked()
                                                {
                                                    close_tab = Some(path.clone());
                                                }
                                            }
                                        });
                                    });
                            });

                        ui.separator();

                        egui::ScrollArea::vertical()
                            .id_source("right_content")
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                match &active_tab {
                                    RightTab::Directory => {
                                        if let Some(cwd) = active_cwd.as_ref() {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(theme::short_path(cwd))
                                                        .monospace()
                                                        .size(theme::CWD_FONT_SZ)
                                                        .color(theme::FG_PATH),
                                                )
                                                .on_hover_text(cwd.display().to_string());
                                                let already_saved = self
                                                    .workspace_store
                                                    .find_for_path(cwd)
                                                    .is_some();
                                                let (btn_text, tip) = if already_saved {
                                                    ("✓ Saved", "Already saved as workspace")
                                                } else {
                                                    ("🔖 Save", "Save as workspace")
                                                };
                                                let save_btn = ui.add_enabled(
                                                    !already_saved,
                                                    egui::Button::new(
                                                        egui::RichText::new(btn_text).size(12.0),
                                                    )
                                                    .frame(false),
                                                );
                                                if save_btn.on_hover_text(tip).clicked() {
                                                    open_ws_dialog = Some(cwd.clone());
                                                }
                                            });
                                            ui.add_space(4.0);
                                            render_dir_tree(
                                                ui,
                                                &dir_entries,
                                                &mut open_md,
                                                &mut open_editor,
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new("(no active session)")
                                                    .italics()
                                                    .color(theme::OVERLAY0)
                                                    .size(12.0),
                                            );
                                        }
                                    }
                                    RightTab::GitDiff => {
                                        render_git_diff(ui, &git_diff, &git_status);
                                    }
                                    RightTab::Markdown(path) => {
                                        let content = md_tabs
                                            .iter()
                                            .find(|(p, _)| p == path)
                                            .map(|(_, c)| c.as_str())
                                            .unwrap_or("(file not found)");
                                        render_markdown(ui, content);
                                    }
                                }
                            });
                    });

                    // ── Draggable divider ────────────────────────────────────────
                    let div_top = panel_rect.min.y + content_h;
                    let div_rect = egui::Rect::from_min_size(
                        egui::pos2(panel_rect.left(), div_top),
                        egui::vec2(panel_w, DIV_H),
                    );
                    let div_resp = ui.interact(
                        div_rect,
                        egui::Id::new("notes_panel_divider"),
                        egui::Sense::drag(),
                    );
                    if div_resp.hovered() || div_resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    }
                    ui.painter().rect_filled(
                        div_rect,
                        0.0,
                        if div_resp.hovered() || div_resp.dragged() {
                            theme::WS_DIV_ACTIVE
                        } else {
                            theme::WS_DIV_IDLE
                        },
                    );
                    if !self.notes_panel_collapsed && div_resp.dragged() {
                        let delta = div_resp.drag_delta().y;
                        // drag down → notes shrinks; drag up → notes grows
                        let new_notes_h = notes_h - delta;
                        if new_notes_h < 40.0 {
                            self.notes_panel_collapsed = true;
                        } else {
                            self.notes_panel_ratio =
                                new_notes_h.min(total_h - 60.0 - DIV_H) / total_h;
                        }
                    } else if self.notes_panel_collapsed && div_resp.dragged() {
                        let delta = div_resp.drag_delta().y;
                        // drag up from collapsed → expand
                        let new_notes_h = COLLAPSED_H - delta;
                        if new_notes_h >= 40.0 {
                            self.notes_panel_collapsed = false;
                            self.notes_panel_ratio =
                                new_notes_h.min(total_h - 60.0 - DIV_H) / total_h;
                        }
                    }

                    // ── Notes section ────────────────────────────────────────────
                    let notes_top = div_top + DIV_H;
                    let notes_rect = egui::Rect::from_min_size(
                        egui::pos2(panel_rect.left(), notes_top),
                        egui::vec2(panel_w, notes_h),
                    );
                    ui.allocate_ui_at_rect(notes_rect, |ui| {
                        ui.painter()
                            .rect_filled(notes_rect, 0.0, theme::BG_WORKSPACE_FILL);

                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), theme::HEADER_H),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let arrow = if self.notes_panel_collapsed {
                                    "▶"
                                } else {
                                    "▼"
                                };
                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new(arrow).size(11.0))
                                            .min_size(egui::vec2(theme::HEADER_H, theme::HEADER_H))
                                            .frame(true),
                                    )
                                    .clicked()
                                {
                                    self.notes_panel_collapsed = !self.notes_panel_collapsed;
                                }
                                ui.label(
                                    egui::RichText::new("Notes")
                                        .strong()
                                        .size(theme::HEADER_FONT_SZ),
                                );
                            },
                        );

                        if !self.notes_panel_collapsed {
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(notes_rect.left(), notes_rect.min.y + COLLAPSED_H),
                                    notes_rect.max,
                                ),
                                0.0,
                                theme::BG_TERM,
                            );
                            egui::ScrollArea::both()
                                .id_source("notes_scroll")
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut note_text)
                                            .id(egui::Id::new("notes_textedit"))
                                            .font(egui::TextStyle::Monospace)
                                            .desired_width(f32::INFINITY)
                                            .hint_text("Notes for this workspace…")
                                            .frame(false),
                                    );
                                });
                        }
                    });
                });
        } // end if self.show_right_panel

        // Persist note if it changed
        {
            let group = self.active_group;
            if note_text != self.note_store.get(group) {
                self.note_store.set(group, note_text);
                self.note_store.save();
            }
        }

        if let Some(tab) = new_tab {
            self.right_tab = tab;
        }
        if let Some(path) = close_tab {
            self.shown_md_tabs.remove(&path);
            if self.right_tab == RightTab::Markdown(path) {
                self.right_tab = RightTab::Directory;
            }
        }
        if let Some(path) = open_md {
            if let (Some(cwd), Some(ws)) = (active_cwd.as_ref(), self.watch_state.as_mut()) {
                if let Some(data) = ws.dir_data.get_mut(cwd) {
                    if !data.md_files.contains_key(&path) {
                        let content = std::fs::read_to_string(&path).unwrap_or_default();
                        data.md_files.insert(path.clone(), content);
                    }
                }
            }
            self.shown_md_tabs.insert(path.clone());
            self.right_tab = RightTab::Markdown(path);
        }

        // Open workspace dialog
        if let Some(path) = open_ws_dialog {
            if self.workspace_dialog.is_none() {
                self.workspace_dialog = Some(WorkspaceDialog::new(path));
            }
        }

        // Load file content for double-clicked file
        let pending_open_editor: Option<(PathBuf, String)> = open_editor.map(|path| {
            let content = std::fs::read_to_string(&path).unwrap_or_default();
            (path, content)
        });

        // ── Snapshot editor contents for TextEdit (must be mutable locals) ─
        let mut editor_texts: Vec<(u32, Option<String>)> = self
            .panes
            .iter()
            .map(|p| {
                let text = match &p.content {
                    PaneContent::FileEditor(ed) => Some(ed.content.clone()),
                    _ => None,
                };
                (p.id, text)
            })
            .collect();

        // ── Workspace colours per pane (before closure to avoid borrow conflict) ─
        let ws_colors: Vec<Option<[u8; 3]>> = self
            .panes
            .iter()
            .map(|p| match &p.content {
                PaneContent::Terminal(sid) => {
                    let sid = *sid;
                    self.sessions.iter().find(|e| e.id == sid).and_then(|e| {
                        let cwd = e.session.read().cwd.clone();
                        if cwd.as_os_str().is_empty() {
                            return None;
                        }
                        self.workspace_store.find_for_cwd(&cwd).map(|w| w.color)
                    })
                }
                PaneContent::DeferredTerminal { cwd, .. } => {
                    cwd.as_ref().and_then(|c| self.workspace_store.find_for_cwd(c).map(|w| w.color))
                }
                PaneContent::FileEditor(ed) => ed.workspace_id.and_then(|id| {
                    self.workspace_store
                        .workspaces
                        .iter()
                        .find(|w| w.id == id)
                        .map(|w| w.color)
                }),
            })
            .collect();

        // ── Group membership + visible pane indices for active group ─────────
        let pane_groups: Vec<Option<u64>> = self
            .panes
            .iter()
            .map(|p| Self::pane_group(&self.sessions, &self.workspace_store, p))
            .collect();
        let active_group_snap = self.active_group;
        let visible_indices: Vec<usize> = pane_groups
            .iter()
            .enumerate()
            .filter(|(_, g)| **g == active_group_snap)
            .map(|(i, _)| i)
            .collect();
        // If active_pane_id is no longer in the visible group, redirect to first visible pane
        if let Some(pid) = self.active_pane_id {
            if !visible_indices.iter().any(|&i| self.panes[i].id == pid) {
                self.active_pane_id = visible_indices.first().map(|&i| self.panes[i].id);
                if let Some(new_pid) = self.active_pane_id {
                    if let Some(pane) = self.panes.iter().find(|p| p.id == new_pid) {
                        if let PaneContent::Terminal(sid) = pane.content {
                            self.active_id = Some(sid);
                        }
                    }
                }
            }
        }

        // ── Output variables collected inside the central panel closure ─────
        let divider_drags: Vec<(usize, usize, f32, f32, f32)> = vec![]; // retained for post-closure mutation compatibility
        let mut close_pane_id: Option<u32> = None;
        let mut clicked_pane_id: Option<u32> = None;
        let mut editor_saves: Vec<u32> = vec![];
        let mut pane_widths_snap: Vec<(u32, f32)> = vec![];
        let mut resize_total_h: f32 = 0.0;
        let mut resize_cell_w: f32 = 0.0;
        let mut resize_cell_h: f32 = 0.0;
        let equalize_widths: bool = false;
        let mut panel_w_snap: f32 = 0.0;

        // ── Central panel ──────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::none().inner_margin(egui::Margin::same(2.0)))
            .show(ctx, |ui| {
            let panel_rect = ui.max_rect();
            let font_id    = FontId::monospace(14.0);
            let cw         = ui.fonts(|fonts| {
                // Use a galley measurement (same as terminal_pass) so the PTY column count
                // matches the renderer exactly. glyph_width can differ from rendered advance
                // on HiDPI displays, causing the terminal to appear at half width.
                let galley = fonts.layout_no_wrap(
                    "MMMMMMMMMMMMMMMMMMMM".to_string(),
                    font_id.clone(),
                    theme::TEXT,
                );
                galley.rect.width() / 20.0
            });
            let ch         = ui.fonts(|f| f.row_height(&font_id));
            resize_cell_w  = cw;
            resize_cell_h  = ch;
            resize_total_h = panel_rect.height();
            panel_w_snap   = panel_rect.width();

            let nv = visible_indices.len();

            let active_pane_id_snap = self.active_pane_id;
            let active_is_editor = self.active_pane_id
                .and_then(|pid| self.panes.iter().find(|p| p.id == pid))
                .map(|p| matches!(p.content, PaneContent::FileEditor(_)))
                .unwrap_or(false);
            let active_session_id: Option<u32> = self.active_pane_id
                .and_then(|pid| self.panes.iter().find(|p| p.id == pid))
                .and_then(|p| match &p.content { PaneContent::Terminal(sid) => Some(*sid), _ => None });

            if nv == 0 {
                // Empty group — show a placeholder
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("No sessions in this group.\nUse '+ New' in the Sessions panel to add one.")
                            .color(theme::OVERLAY0).size(14.0)
                    );
                });
            } else {
                // ── Tab bar + single content area ────────────────────────────
                let tab_h   = theme::HEADER_H;
                let panel_h = panel_rect.height();

                let tab_bar_rect = egui::Rect::from_min_size(
                    panel_rect.min,
                    egui::vec2(panel_rect.width(), tab_h),
                );
                let content_rect = egui::Rect::from_min_size(
                    egui::pos2(panel_rect.min.x, panel_rect.min.y + tab_h),
                    egui::vec2(panel_rect.width(), (panel_h - tab_h).max(0.0)),
                );

                // ── Tab bar (horizontally scrollable) ────────────────────────
                ui.allocate_ui_at_rect(tab_bar_rect, |ui| {
                    ui.painter().rect_filled(tab_bar_rect, 0.0, theme::SURFACE0);
                    egui::ScrollArea::horizontal()
                        .id_source("tab_bar_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for &i in &visible_indices {
                                    let pane_id   = self.panes[i].id;
                                    let is_active = Some(pane_id) == active_pane_id_snap;
                                    let ws_color  = ws_colors[i];

                                    let (title, title_cwd): (String, std::path::PathBuf) = match &self.panes[i].content {
                                        PaneContent::Terminal(sid) => {
                                            let sid = *sid;
                                            self.sessions.iter()
                                                .find(|e| e.id == sid)
                                                .map(|e| {
                                                    let s = e.session.read();
                                                    (s.title.clone(), s.cwd.clone())
                                                })
                                                .unwrap_or_else(|| (format!("Terminal {sid}"), std::path::PathBuf::new()))
                                        }
                                        PaneContent::DeferredTerminal { cwd, .. } => {
                                            let name = cwd.as_ref()
                                                .and_then(|p| p.file_name())
                                                .map(|n| n.to_string_lossy().into_owned())
                                                .unwrap_or_else(|| "Terminal".to_string());
                                            (name, cwd.clone().unwrap_or_default())
                                        }
                                        PaneContent::FileEditor(ed) => {
                                            let fname = ed.path.file_name()
                                                .map(|n| n.to_string_lossy().into_owned())
                                                .unwrap_or_default();
                                            let t = if ed.save_error {
                                                format!("! {fname}")
                                            } else if ed.dirty {
                                                format!("* {fname}")
                                            } else {
                                                fname
                                            };
                                            (t, std::path::PathBuf::new())
                                        }
                                    };
                                    let display = effective_title(&title, &title_cwd);

                                    let tab_w: f32 = 160.0;
                                    let (_, tab_rect) = ui.allocate_space(egui::vec2(tab_w, tab_h));

                                    let hbg = theme::header_bg(ws_color, is_active);
                                    let title_color = match ws_color {
                                        Some(c) => theme::text_on(theme::tinted(c, if is_active { 0.75 } else { 0.35 })),
                                        None    => if is_active { theme::TEXT } else { theme::SUBTEXT1 },
                                    };

                                    let painter = ui.painter().clone();
                                    painter.rect_filled(tab_rect, 0.0, hbg);

                                    // Workspace colour strip on left edge
                                    if let Some(c) = ws_color {
                                        painter.rect_filled(
                                            egui::Rect::from_min_size(tab_rect.min, egui::vec2(3.0, tab_h)),
                                            0.0,
                                            theme::from_rgb(c),
                                        );
                                    }

                                    // Bottom highlight on active tab
                                    if is_active {
                                        painter.rect_filled(
                                            egui::Rect::from_min_size(
                                                egui::pos2(tab_rect.min.x, tab_rect.max.y - 2.0),
                                                egui::vec2(tab_w, 2.0),
                                            ),
                                            0.0, theme::TEXT,
                                        );
                                    }

                                    // Right-edge separator between tabs
                                    painter.rect_filled(
                                        egui::Rect::from_min_size(
                                            egui::pos2(tab_rect.max.x - 1.0, tab_rect.min.y),
                                            egui::vec2(1.0, tab_h),
                                        ),
                                        0.0, theme::SURFACE2,
                                    );

                                    // Register tab-wide click first (lower z-order); close button
                                    // is registered second so it has higher priority in egui's
                                    // last-registered-wins model for overlapping regions.
                                    let tab_resp = ui.interact(
                                        tab_rect,
                                        egui::Id::new(("tab_click", pane_id)),
                                        egui::Sense::click(),
                                    );

                                    // Close button (×)
                                    let close_rect = egui::Rect::from_min_size(
                                        egui::pos2(tab_rect.max.x - theme::BTN_W, tab_rect.min.y),
                                        egui::vec2(theme::BTN_W, tab_h),
                                    );
                                    let close_resp = ui.interact(
                                        close_rect,
                                        egui::Id::new(("tab_close", pane_id)),
                                        egui::Sense::click(),
                                    );
                                    if close_resp.hovered() {
                                        painter.rect_filled(close_rect, 0.0, theme::DANGER_BG);
                                    }
                                    painter.text(
                                        close_rect.center(), egui::Align2::CENTER_CENTER,
                                        "×", egui::FontId::proportional(14.0),
                                        theme::DANGER_FG,
                                    );

                                    // Title text (clipped before close button)
                                    let text_x = tab_rect.min.x + if ws_color.is_some() { 7.0 } else { 5.0 };
                                    painter.with_clip_rect(egui::Rect::from_min_max(
                                        egui::pos2(text_x, tab_rect.min.y),
                                        egui::pos2(close_rect.min.x - 3.0, tab_rect.max.y),
                                    )).text(
                                        egui::pos2(text_x, tab_rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        &display,
                                        egui::FontId::proportional(theme::HEADER_FONT_SZ),
                                        title_color,
                                    );

                                    if close_resp.on_hover_text("Close tab").clicked() {
                                        close_pane_id = Some(pane_id);
                                    } else if tab_resp.clicked() {
                                        clicked_pane_id = Some(pane_id);
                                    }
                                }
                            });
                        });
                });

                // ── Active tab content (full-size) ────────────────────────────
                if let Some(&active_i) = visible_indices.iter()
                    .find(|&&i| Some(self.panes[i].id) == active_pane_id_snap)
                {
                    let pane_id = self.panes[active_i].id;
                    pane_widths_snap.push((pane_id, content_rect.width()));

                    ui.allocate_ui_at_rect(content_rect, |ui| {
                        match &self.panes[active_i].content {
                            PaneContent::Terminal(sid) => {
                                let sid = *sid;
                                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                    let session = Arc::clone(&self.sessions[idx].session);
                                    let scroll_off = self.term_scroll_offset.get(&sid).copied().unwrap_or(0);
                                    let geo = crate::renderer::terminal_pass::TerminalView::new(session).show(ui, true, scroll_off, self.cursor_blink_on);
                                    self.active_term_geo = Some(geo);
                                }
                                self.active_term_ui_id = Some(ui.id());
                                let this_id = ui.id();
                                let other_widget_focused = ctx.memory(|m| {
                                    m.focused().map(|id| id != this_id).unwrap_or(false)
                                });
                                let dialog_open = self.workspace_dialog.is_some()
                                    || self.workspace_edit_dialog.is_some()
                                    || self.show_settings;
                                if !other_widget_focused && !dialog_open {
                                    ui.memory_mut(|m| m.request_focus(this_id));
                                }
                            }
                            PaneContent::DeferredTerminal { .. } => {
                                // Session will be spawned in the post-closure step;
                                // show a blank terminal background until that frame.
                                ui.painter().rect_filled(ui.max_rect(), 0.0, theme::BG_TERM);
                            }
                            PaneContent::FileEditor(_) => {
                                ui.painter().rect_filled(ui.max_rect(), 0.0, theme::BG_TERM);
                                if let Some(ref mut text) = editor_texts[active_i].1 {
                                    egui::ScrollArea::both()
                                        .id_source(("editor_scroll", pane_id))
                                        .auto_shrink([false; 2])
                                        .show(ui, |ui| {
                                            ui.add(
                                                egui::TextEdit::multiline(text)
                                                    .font(egui::TextStyle::Monospace)
                                                    .desired_width(f32::INFINITY)
                                                    .frame(false),
                                            );
                                        });
                                }
                                if ui.input(|inp| inp.modifiers.ctrl && inp.key_pressed(egui::Key::S)) {
                                    editor_saves.push(pane_id);
                                }
                            }
                        }
                    });

                    // Click content area to focus active tab
                    if ctx.input(|inp| inp.pointer.button_clicked(egui::PointerButton::Primary)) {
                        if let Some(pos) = ctx.input(|inp| inp.pointer.interact_pos()) {
                            if content_rect.contains(pos) {
                                clicked_pane_id = Some(pane_id);
                            }
                        }
                    }
                }

            // ── Terminal input routing ─────────────────────────────────────
            let any_other_widget_focused = {
                let term_id = self.active_term_ui_id;
                ctx.memory(|m| m.focused().map(|id| Some(id) != term_id).unwrap_or(false))
            };
            let modal_open = self.workspace_dialog.is_some()
                || self.workspace_edit_dialog.is_some()
                || self.show_settings;
            if !active_is_editor && !any_other_widget_focused && !modal_open {

                // Focus-in / focus-out events (?1004h)
                if active_session_id != self.last_focused_sid {
                    // Send focus-out to the session we just left
                    if let Some(old_sid) = self.last_focused_sid {
                        if let Some(idx) = self.sessions.iter().position(|e| e.id == old_sid) {
                            let tracking = self.sessions[idx].session.read().focus_tracking;
                            if tracking {
                                SessionManager::write(&mut self.sessions[idx].writer, b"\x1b[O");
                            }
                        }
                    }
                    // Send focus-in to the newly active session
                    if let Some(sid) = active_session_id {
                        if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                            let tracking = self.sessions[idx].session.read().focus_tracking;
                            if tracking {
                                SessionManager::write(&mut self.sessions[idx].writer, b"\x1b[I");
                            }
                        }
                    }
                    self.last_focused_sid = active_session_id;
                }

                let events = ctx.input(|inp| inp.events.clone());
                for event in &events {
                    match event {
                        egui::Event::Text(text) => {
                            if let Some(sid) = active_session_id {
                                self.term_scroll_offset.remove(&sid);
                                self.scroll_accum.remove(&sid);
                                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                    SessionManager::write(&mut self.sessions[idx].writer, text.as_bytes());
                                }
                            }
                        }
                        egui::Event::Key { key, pressed: true, modifiers, .. } => {
                            if let Some(bytes) = key_to_pty_bytes(key, modifiers) {
                                if let Some(sid) = active_session_id {
                                    self.term_scroll_offset.remove(&sid);
                                    self.scroll_accum.remove(&sid);
                                    if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                        SessionManager::write(&mut self.sessions[idx].writer, &bytes);
                                    }
                                }
                            }
                        }
                        // egui-winit converts Ctrl+C to Event::Copy before emitting Event::Key,
                        // so we must handle Copy here to send the SIGINT byte to the PTY.
                        egui::Event::Copy => {
                            if let Some(sid) = active_session_id {
                                self.term_scroll_offset.remove(&sid);
                                self.scroll_accum.remove(&sid);
                                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                    SessionManager::write(&mut self.sessions[idx].writer, &[3]);
                                }
                            }
                        }
                        // egui-winit converts Ctrl+V to Event::Paste before emitting Event::Key.
                        // Wrap in bracketed-paste sequences only if the app opted in (?2004h).
                        egui::Event::Paste(text) => {
                            if let Some(sid) = active_session_id {
                                self.term_scroll_offset.remove(&sid);
                                self.scroll_accum.remove(&sid);
                                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                    let bp = self.sessions[idx].session.read().bracketed_paste;
                                    let data = if bp {
                                        let mut v = b"\x1b[200~".to_vec();
                                        v.extend_from_slice(text.as_bytes());
                                        v.extend_from_slice(b"\x1b[201~");
                                        v
                                    } else {
                                        text.as_bytes().to_vec()
                                    };
                                    SessionManager::write(&mut self.sessions[idx].writer, &data);
                                }
                            }
                        }
                        // Mouse events forwarded when the application has enabled mouse reporting.
                        egui::Event::PointerButton { pos, button, pressed, .. } => {
                            if let Some(sid) = active_session_id {
                                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                    let (mode, sgr) = {
                                        let s = self.sessions[idx].session.read();
                                        (s.mouse_mode, s.mouse_sgr)
                                    };
                                    if mode != MouseMode::None {
                                        if let Some(geo) = &self.active_term_geo {
                                            if let Some((col, row)) = geo.to_cell(*pos) {
                                                let btn = match button {
                                                    egui::PointerButton::Primary   => 0u8,
                                                    egui::PointerButton::Middle    => 1,
                                                    egui::PointerButton::Secondary => 2,
                                                    _ => return,
                                                };
                                                let bytes = mouse_event_bytes(btn, col, row, *pressed, sgr);
                                                SessionManager::write(&mut self.sessions[idx].writer, &bytes);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        egui::Event::MouseWheel { unit, delta, .. } => {
                            let mouse_pos = ctx.input(|inp| inp.pointer.latest_pos());
                            let over_term = mouse_pos
                                .zip(self.active_term_geo.as_ref())
                                .map(|(pos, geo)| geo.rect.contains(pos))
                                .unwrap_or(false);
                            if over_term {
                                if let Some(sid) = active_session_id {
                                    if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                        let (mode, sgr) = {
                                            let s = self.sessions[idx].session.read();
                                            (s.mouse_mode, s.mouse_sgr)
                                        };
                                        if mode != MouseMode::None {
                                            // App has mouse mode — forward scroll to PTY
                                            if let Some(pos) = mouse_pos {
                                                if let Some(geo) = &self.active_term_geo {
                                                    if let Some((col, row)) = geo.to_cell(pos) {
                                                        // Button 64 = scroll up, 65 = scroll down
                                                        let btn = if delta.y > 0.0 { 64u8 } else { 65 };
                                                        let bytes = mouse_event_bytes(btn, col, row, true, sgr);
                                                        SessionManager::write(&mut self.sessions[idx].writer, &bytes);
                                                    }
                                                }
                                            }
                                        } else {
                                            // No mouse mode — scroll local scrollback instead.
                                            // Accumulate in fractional lines. Convert delta.y based on
                                            // its unit: Point=pixels (divide by cell height), Line=already
                                            // in lines, Page=multiply by visible rows.
                                            let scrollback_len = {
                                                self.sessions[idx].session.read().grid.scrollback.len()
                                            };
                                            let geo = self.active_term_geo.as_ref();
                                            let cell_h = geo.map(|g| g.cell_h).unwrap_or(18.0);
                                            let visible_rows = geo
                                                .map(|g| g.rect.height() / g.cell_h)
                                                .unwrap_or(24.0);
                                            let delta_lines = match unit {
                                                egui::MouseWheelUnit::Point => delta.y / cell_h,
                                                egui::MouseWheelUnit::Line  => delta.y,
                                                egui::MouseWheelUnit::Page  => delta.y * visible_rows,
                                            };
                                            let accum = self.scroll_accum.entry(sid).or_insert(0.0);
                                            *accum += delta_lines;
                                            let lines = accum.abs() as usize;
                                            if lines > 0 {
                                                let direction = if *accum > 0.0 { 1.0f32 } else { -1.0 };
                                                *accum -= direction * lines as f32;
                                                let offset = self.term_scroll_offset.entry(sid).or_insert(0);
                                                if direction > 0.0 {
                                                    *offset = (*offset + lines).min(scrollback_len);
                                                } else {
                                                    *offset = offset.saturating_sub(lines);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            } // end else (nv > 0)
        });

        // ── Post-closure mutations ─────────────────────────────────────────

        // 1. Divider drags → freeze manual widths on both adjacent panes
        for (left_idx, right_idx, delta_x, left_w, right_w) in divider_drags {
            self.panes[left_idx].manual_width = Some((left_w + delta_x).max(theme::MIN_PANE_W));
            self.panes[right_idx].manual_width = Some((right_w - delta_x).max(theme::MIN_PANE_W));
        }

        // 2. Close pane
        if let Some(pid) = close_pane_id {
            if let Some(pos) = self.panes.iter().position(|p| p.id == pid) {
                if let PaneContent::Terminal(sid) = self.panes[pos].content {
                    self.uninit_sessions.remove(&sid);
                    self.sessions.retain(|e| e.id != sid);
                    if self.active_id == Some(sid) {
                        self.active_id = self.sessions.first().map(|e| e.id);
                        self.update_is_active_flags();
                    }
                }
                // DeferredTerminal: no live session to clean up
                self.panes.remove(pos);
                editor_texts.retain(|(id, _)| *id != pid);
                if self.active_pane_id == Some(pid) {
                    self.active_pane_id = self.panes.last().map(|p| p.id);
                }
            }
            self.save_session();
        }

        // 3. Equalize pane widths for visible panes (split icon clicked)
        if equalize_widths {
            for &i in &visible_indices {
                self.panes[i].manual_width = None;
            }
            loop {
                let nv2 = visible_indices
                    .iter()
                    .filter(|&&i| i < self.panes.len())
                    .count();
                if nv2 <= 1 {
                    break;
                }
                let avail = panel_w_snap - (nv2 - 1) as f32 * theme::DIVIDER_W;
                if avail / nv2 as f32 >= theme::MIN_PANE_W {
                    break;
                }
                // Remove the first visible pane
                if let Some(&first_vi) = visible_indices.first() {
                    let removed = self.panes.remove(first_vi);
                    if let PaneContent::Terminal(sid) = removed.content {
                        self.uninit_sessions.remove(&sid);
                        self.sessions.retain(|e| e.id != sid);
                        if self.active_id == Some(sid) {
                            self.active_id = self.sessions.first().map(|e| e.id);
                            self.update_is_active_flags();
                        }
                    }
                    if self.active_pane_id == Some(removed.id) {
                        self.active_pane_id = self.panes.first().map(|p| p.id);
                    }
                    break; // recompute visible_indices next frame
                }
            }
        }

        // 5. Pane focus from click
        if let Some(pid) = clicked_pane_id {
            self.activate_pane(pid);
        }

        // 5b. Materialize any DeferredTerminal that is now the active pane.
        // This runs after activate_pane() so the deferred pane spawns in the same
        // update() call as the click, meaning the terminal is ready by the next frame.
        if let Some(pid) = self.active_pane_id {
            if let Some(pane_idx) = self.panes.iter().position(|p| p.id == pid) {
                if matches!(&self.panes[pane_idx].content, PaneContent::DeferredTerminal { .. }) {
                    let (cwd, pending_command) =
                        if let PaneContent::DeferredTerminal { cwd, pending_command } =
                            &self.panes[pane_idx].content
                        {
                            (cwd.clone(), pending_command.clone())
                        } else {
                            unreachable!()
                        };
                    if let Some(sid) = self.spawn_session_no_pane(&default_shell(), 80, 24, cwd) {
                        if let Some(cmd) = pending_command {
                            if let Some(entry) = self.sessions.iter_mut().find(|e| e.id == sid) {
                                entry.pending_command = Some(cmd);
                            }
                        }
                        self.panes[pane_idx].content = PaneContent::Terminal(sid);
                        self.panes[pane_idx].last_size = (0, 0); // force resize next frame
                        self.active_id = Some(sid);
                        self.update_is_active_flags();
                        ctx.request_repaint();
                    }
                }
            }
        }

        // 6. Editor text changes
        for (i, (pane_id, new_text)) in editor_texts.iter().enumerate() {
            if let Some(ref new_text) = new_text {
                if i < self.panes.len() && self.panes[i].id == *pane_id {
                    if let PaneContent::FileEditor(ref mut ed) = self.panes[i].content {
                        if *new_text != ed.content {
                            ed.content = new_text.clone();
                            ed.dirty = true;
                            ed.save_error = false;
                        }
                    }
                }
            }
        }

        // 7. Editor saves (Ctrl+S)
        for save_id in &editor_saves {
            if let Some(p) = self.panes.iter_mut().find(|p| p.id == *save_id) {
                if let PaneContent::FileEditor(ref mut ed) = p.content {
                    match std::fs::write(&ed.path, &ed.content) {
                        Ok(()) => {
                            ed.dirty = false;
                            ed.save_error = false;
                        }
                        Err(e) => {
                            log::error!("Failed to save {}: {e}", ed.path.display());
                            ed.save_error = true;
                        }
                    }
                }
            }
        }

        // 8. PTY resize detection (per-pane) — debounced.
        // We record the target size when it changes and only send the PTY resize
        // after the size has been stable for 150 ms. This prevents ConPTY from
        // issuing a clear-screen sequence on every drag frame, which would leave
        // the terminal blank for as long as the running app takes to redraw.
        for (pane_id, width) in pane_widths_snap {
            if let Some(pane_idx) = self.panes.iter().position(|p| p.id == pane_id) {
                if let PaneContent::Terminal(sid) = self.panes[pane_idx].content {
                    let cols = ((width / resize_cell_w) as u16).max(1);
                    let rows = (((resize_total_h - theme::HEADER_H) / resize_cell_h) as u16).max(1);
                    let target = (cols, rows);
                    if target != self.panes[pane_idx].last_size {
                        // Reset debounce timer only if the target itself changed
                        // (not just re-entering the same pending target each frame).
                        let need_reset = !matches!(self.resize_debounce.get(&sid), Some(&(dc, dr, _)) if (dc, dr) == target);
                        if need_reset {
                            self.resize_debounce
                                .insert(sid, (cols, rows, Instant::now()));
                        }
                    }
                }
            }
        }

        // Apply debounced resizes that have been stable for ≥150 ms.
        {
            const DEBOUNCE: Duration = Duration::from_millis(150);
            let now = Instant::now();
            let stable: Vec<(u32, u16, u16)> = self
                .resize_debounce
                .iter()
                .filter(|(_, &(_, _, t))| now.duration_since(t) >= DEBOUNCE)
                .map(|(&sid, &(c, r, _))| (sid, c, r))
                .collect();
            for (sid, cols, rows) in stable {
                self.resize_debounce.remove(&sid);
                self.term_scroll_offset.remove(&sid);
                self.scroll_accum.remove(&sid);
                // Update pane's recorded size
                if let Some(pane) = self
                    .panes
                    .iter_mut()
                    .find(|p| matches!(p.content, PaneContent::Terminal(s) if s == sid))
                {
                    pane.last_size = (cols, rows);
                }
                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                    // Snapshot the prompt row before PTY resize (PSReadLine workaround)
                    let snap = {
                        let sess = self.sessions[idx].session.read();
                        let cy = sess.cursor_y;
                        let ncols = sess.grid.cols;
                        if (0..ncols).any(|c| sess.grid.get(cy, c).c != ' ') {
                            let cells = (0..ncols).map(|c| *sess.grid.get(cy, c)).collect();
                            Some(ResizeSnapshot {
                                cursor_y: cy,
                                cells,
                                expires: Instant::now() + Duration::from_secs(2),
                            })
                        } else {
                            None
                        }
                    };
                    if let Some(snap) = snap {
                        self.resize_snapshots.insert(sid, snap);
                    }
                    let entry = &self.sessions[idx];
                    SessionManager::resize(&entry.master, cols, rows);
                    entry.session.write().resize(cols, rows);
                }
            }
            // Keep repainting while a resize is pending so the debounce fires promptly.
            if !self.resize_debounce.is_empty() {
                ctx.request_repaint_after(Duration::from_millis(160));
            }
        }

        // 9. File opened from right panel → add FileEditor pane (or focus existing)
        if let Some((path, content)) = pending_open_editor {
            let existing_id = self
                .panes
                .iter()
                .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path))
                .map(|p| p.id);
            if let Some(pid) = existing_id {
                self.activate_pane(pid);
            } else {
                let pane_id = self.next_pane_id;
                self.next_pane_id += 1;
                self.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::FileEditor(FileEditorState {
                        path,
                        content,
                        dirty: false,
                        save_error: false,
                        workspace_id: self.active_group,
                    }),
                    manual_width: None,
                    last_size: (0, 0),
                });
                self.activate_pane(pane_id);
            }
        }

        // ── Settings overlay ───────────────────────────────────────────────
        if self.show_settings {
            let mut settings_changed = false;
            let mut close_settings = false;
            let screen_rect = ctx.screen_rect();
            let dialog_w = (screen_rect.width() * 0.38).clamp(320.0, 520.0);
            let dialog_h = 220.0_f32;

            egui::Area::new(egui::Id::new("settings_dim"))
                .fixed_pos(screen_rect.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let resp = ui.interact(
                        screen_rect,
                        egui::Id::new("settings_dim_click"),
                        egui::Sense::click(),
                    );
                    ui.painter().rect_filled(
                        screen_rect,
                        0.0,
                        egui::Color32::from_black_alpha(160),
                    );
                    if resp.clicked() {
                        close_settings = true;
                    }
                });

            egui::Area::new(egui::Id::new("settings_dialog"))
                .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, dialog_h / 2.0))
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    egui::Frame::window(&ctx.style()).show(ui, |ui| {
                        ui.set_min_width(dialog_w);

                        // Header
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Settings").strong().size(15.0));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.add(
                                    egui::Button::new(egui::RichText::new("×").size(16.0))
                                        .min_size(egui::vec2(24.0, 24.0))
                                ).clicked() {
                                    close_settings = true;
                                }
                            });
                        });
                        ui.separator();
                        ui.add_space(8.0);

                        // Default workspace
                        ui.label(egui::RichText::new("Default workspace on launch").size(13.0));
                        ui.add_space(4.0);
                        let selected_name = self.settings.default_workspace_id
                            .and_then(|id| self.workspace_store.workspaces.iter().find(|w| w.id == id))
                            .map(|w| w.name.clone())
                            .unwrap_or_else(|| "(none)".to_string());
                        egui::ComboBox::from_id_source("settings_default_ws")
                            .selected_text(&selected_name)
                            .width(dialog_w - 32.0)
                            .show_ui(ui, |ui| {
                                if ui.selectable_label(
                                    self.settings.default_workspace_id.is_none(),
                                    "(none)",
                                ).clicked() {
                                    self.settings.default_workspace_id = None;
                                    settings_changed = true;
                                }
                                let ws_list: Vec<(u64, String)> = self.workspace_store.workspaces
                                    .iter().map(|w| (w.id, w.name.clone())).collect();
                                for (id, name) in ws_list {
                                    if ui.selectable_label(
                                        self.settings.default_workspace_id == Some(id),
                                        &name,
                                    ).clicked() {
                                        self.settings.default_workspace_id = Some(id);
                                        settings_changed = true;
                                    }
                                }
                            });
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Used when there is no session to restore.")
                                .size(11.0).color(theme::OVERLAY0)
                        );

                        ui.add_space(12.0);

                        // Restore last session
                        let mut restore = self.settings.restore_last_session;
                        if ui.checkbox(&mut restore, "Restore last session on launch").changed() {
                            self.settings.restore_last_session = restore;
                            settings_changed = true;
                        }
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("When disabled, always opens a fresh session in the default workspace.")
                                .size(11.0).color(theme::OVERLAY0)
                        );
                    });
                });

            if settings_changed {
                self.settings.save();
            }
            if close_settings {
                self.show_settings = false;
            }
        }

        // ── Workspace save dialog (modal overlay) ──────────────────────────
        if self.workspace_dialog.is_some() {
            let mut save_it = false;
            let mut cancel = false;
            let screen_rect = ctx.screen_rect();
            let dialog_w = (screen_rect.width() * 0.4).clamp(300.0, 480.0);

            egui::Area::new(egui::Id::new("ws_dialog_dim"))
                .fixed_pos(screen_rect.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.painter().rect_filled(
                        screen_rect,
                        0.0,
                        egui::Color32::from_black_alpha(160),
                    );
                });

            egui::Area::new(egui::Id::new("ws_dialog"))
                .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, 140.0))
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    egui::Frame::window(&ctx.style()).show(ui, |ui| {
                        ui.set_min_width(dialog_w);

                        ui.label(egui::RichText::new("Save Workspace").strong().size(15.0));
                        ui.add_space(8.0);

                        if let Some(dlg) = &mut self.workspace_dialog {
                            ui.label("Name");
                            let name_resp = ui.add(
                                egui::TextEdit::singleline(&mut dlg.name)
                                    .hint_text("e.g. My Project")
                                    .desired_width(f32::INFINITY),
                            );
                            if !dlg.focus_requested {
                                name_resp.request_focus();
                                dlg.focus_requested = true;
                            }
                            ui.add_space(6.0);

                            ui.label(
                                egui::RichText::new(theme::short_path(&dlg.path))
                                    .monospace()
                                    .size(11.0)
                                    .color(theme::FG_PATH),
                            )
                            .on_hover_text(dlg.path.display().to_string());
                            ui.add_space(8.0);

                            ui.label("Color");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                for &preset in PRESET_COLORS {
                                    let selected =
                                        dlg.selected_color == preset && !dlg.show_custom_picker;
                                    let swatch = egui::Button::new("")
                                        .fill(theme::from_rgb(preset))
                                        .stroke(if selected {
                                            egui::Stroke::new(2.5, egui::Color32::WHITE)
                                        } else {
                                            egui::Stroke::new(1.0, egui::Color32::from_gray(60))
                                        })
                                        .min_size(egui::vec2(24.0, 24.0))
                                        .rounding(4.0);
                                    if ui.add(swatch).clicked() {
                                        dlg.selected_color = preset;
                                        dlg.show_custom_picker = false;
                                        dlg.custom_color = [
                                            preset[0] as f32 / 255.0,
                                            preset[1] as f32 / 255.0,
                                            preset[2] as f32 / 255.0,
                                        ];
                                    }
                                }

                                // Custom — inline picker, one click opens directly
                                let picker_resp = egui::color_picker::color_edit_button_rgb(
                                    ui,
                                    &mut dlg.custom_color,
                                );
                                if picker_resp.changed() {
                                    dlg.show_custom_picker = true;
                                    dlg.selected_color = [
                                        (dlg.custom_color[0] * 255.0) as u8,
                                        (dlg.custom_color[1] * 255.0) as u8,
                                        (dlg.custom_color[2] * 255.0) as u8,
                                    ];
                                }
                            });

                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                let can_save = !dlg.name.trim().is_empty();
                                if ui
                                    .add_enabled(can_save, egui::Button::new("Save"))
                                    .clicked()
                                {
                                    save_it = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    cancel = true;
                                }
                            });
                        }
                    });
                });

            if save_it {
                if let Some(dlg) = self.workspace_dialog.take() {
                    let id = self.workspace_store.next_id();
                    self.workspace_store.workspaces.push(Workspace {
                        id,
                        name: dlg.name.trim().to_string(),
                        path: dlg.path,
                        color: dlg.selected_color,
                    });
                    self.workspace_store.save();
                    let (cols, rows) = self.panes.first().map(|p| p.last_size).unwrap_or((80, 24));
                    self.switch_group(Some(id), cols, rows);
                }
            } else if cancel {
                self.workspace_dialog = None;
            }
        }

        // ── Workspace edit dialog (modal overlay) ──────────────────────────
        if self.workspace_edit_dialog.is_some() {
            let mut save_it = false;
            let mut delete_it = false;
            let mut cancel = false;
            let screen_rect = ctx.screen_rect();
            let dialog_w = (screen_rect.width() * 0.4).clamp(300.0, 480.0);

            egui::Area::new(egui::Id::new("ws_edit_dim"))
                .fixed_pos(screen_rect.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.painter().rect_filled(
                        screen_rect,
                        0.0,
                        egui::Color32::from_black_alpha(160),
                    );
                });

            egui::Area::new(egui::Id::new("ws_edit_dialog"))
                .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, 140.0))
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    egui::Frame::window(&ctx.style()).show(ui, |ui| {
                        ui.set_min_width(dialog_w);

                        ui.label(
                            egui::RichText::new("⚙ Workspace Settings")
                                .strong()
                                .size(15.0),
                        );
                        ui.add_space(8.0);

                        if let Some(dlg) = &mut self.workspace_edit_dialog {
                            ui.label("Name");
                            let name_resp = ui.add(
                                egui::TextEdit::singleline(&mut dlg.name)
                                    .hint_text("Workspace name")
                                    .desired_width(f32::INFINITY),
                            );
                            if !dlg.focus_requested {
                                name_resp.request_focus();
                                dlg.focus_requested = true;
                            }
                            ui.add_space(8.0);

                            ui.label("Color");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                for &preset in PRESET_COLORS {
                                    let selected =
                                        dlg.selected_color == preset && !dlg.show_custom_picker;
                                    let swatch = egui::Button::new("")
                                        .fill(theme::from_rgb(preset))
                                        .stroke(if selected {
                                            egui::Stroke::new(2.5, egui::Color32::WHITE)
                                        } else {
                                            egui::Stroke::new(1.0, egui::Color32::from_gray(60))
                                        })
                                        .min_size(egui::vec2(24.0, 24.0))
                                        .rounding(4.0);
                                    if ui.add(swatch).clicked() {
                                        dlg.selected_color = preset;
                                        dlg.show_custom_picker = false;
                                        dlg.custom_color = [
                                            preset[0] as f32 / 255.0,
                                            preset[1] as f32 / 255.0,
                                            preset[2] as f32 / 255.0,
                                        ];
                                    }
                                }

                                // Custom — inline picker, one click opens directly
                                let picker_resp = egui::color_picker::color_edit_button_rgb(
                                    ui,
                                    &mut dlg.custom_color,
                                );
                                if picker_resp.changed() {
                                    dlg.show_custom_picker = true;
                                    dlg.selected_color = [
                                        (dlg.custom_color[0] * 255.0) as u8,
                                        (dlg.custom_color[1] * 255.0) as u8,
                                        (dlg.custom_color[2] * 255.0) as u8,
                                    ];
                                }
                            });

                            ui.add_space(12.0);
                            ui.separator();
                            ui.add_space(8.0);

                            if dlg.confirm_delete {
                                ui.colored_label(
                                    egui::Color32::from_rgb(220, 70, 70),
                                    "Are you sure? This cannot be undone.",
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("Delete Workspace")
                                                    .color(egui::Color32::WHITE),
                                            )
                                            .fill(egui::Color32::from_rgb(180, 40, 40)),
                                        )
                                        .clicked()
                                    {
                                        delete_it = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        dlg.confirm_delete = false;
                                    }
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    let can_save = !dlg.name.trim().is_empty();
                                    if ui
                                        .add_enabled(can_save, egui::Button::new("Save"))
                                        .clicked()
                                    {
                                        save_it = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        cancel = true;
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Delete").color(
                                                            egui::Color32::from_rgb(220, 80, 80),
                                                        ),
                                                    )
                                                    .stroke(egui::Stroke::new(
                                                        1.0,
                                                        egui::Color32::from_rgb(220, 80, 80),
                                                    )),
                                                )
                                                .clicked()
                                            {
                                                dlg.confirm_delete = true;
                                            }
                                        },
                                    );
                                });
                            }
                        }
                    });
                });

            if save_it {
                if let Some(dlg) = self.workspace_edit_dialog.take() {
                    if let Some(ws) = self
                        .workspace_store
                        .workspaces
                        .iter_mut()
                        .find(|w| w.id == dlg.workspace_id)
                    {
                        ws.name = dlg.name.trim().to_string();
                        ws.color = dlg.selected_color;
                    }
                    self.workspace_store.save();
                }
            } else if delete_it {
                if let Some(dlg) = self.workspace_edit_dialog.take() {
                    self.workspace_store
                        .workspaces
                        .retain(|w| w.id != dlg.workspace_id);
                    self.workspace_store.save();
                    if self.active_group == Some(dlg.workspace_id) {
                        self.active_group = None;
                    }
                }
            } else if cancel {
                self.workspace_edit_dialog = None;
            }
        }
    }
}

// ── Directory tree rendering ──────────────────────────────────────────────────

fn render_dir_tree(
    ui: &mut egui::Ui,
    entries: &[FileEntry],
    open_md: &mut Option<PathBuf>,
    open_editor: &mut Option<PathBuf>,
) {
    use crate::theme;
    const ROW_SZ: f32 = 13.0;

    if entries.is_empty() {
        ui.label(
            egui::RichText::new("(empty directory)")
                .italics()
                .color(theme::OVERLAY0)
                .size(12.0),
        );
        return;
    }

    ui.spacing_mut().item_spacing.y = 2.0;

    for entry in entries {
        if entry.is_dir {
            let id = ui.make_persistent_id(&entry.path);
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                false,
            );
            let chevron = if state.is_open() { "▼" } else { "▶" };
            let resp = ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("{} {}", chevron, &entry.name))
                        .color(theme::FG_DIR_ENTRY)
                        .size(ROW_SZ),
                )
                .sense(egui::Sense::click()),
            );
            if resp.clicked() {
                state.toggle(ui);
            }
            state.show_body_indented(&resp, ui, |ui| {
                let children = list_dir_entries(&entry.path);
                render_dir_tree(ui, &children, open_md, open_editor);
            });
        } else {
            let ext = entry
                .path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            let is_md = ext == "md";
            let color = if is_md {
                theme::FG_MD_FILE
            } else {
                theme::FG_OTHER_FILE
            };
            let resp = ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("{} {}", file_icon(ext), &entry.name))
                        .color(color)
                        .size(ROW_SZ),
                )
                .sense(egui::Sense::click()),
            );
            if resp.double_clicked() {
                *open_editor = Some(entry.path.clone());
            } else if resp.clicked() && is_md {
                *open_md = Some(entry.path.clone());
            }
        }
    }
}

fn file_icon(ext: &str) -> &'static str {
    match ext {
        "rs" => "⚙",
        "md" => "📝",
        "toml" | "yaml" | "yml" => "≡",
        "json" => "≡",
        "txt" => "≡",
        "sh" | "bat" | "ps1" | "cmd" => "▸",
        "py" => "▸",
        "js" | "ts" | "jsx" | "tsx" => "▸",
        _ => "·",
    }
}

// ── Git diff rendering ────────────────────────────────────────────────────────

fn render_git_diff(ui: &mut egui::Ui, diff: &str, status: &str) {
    use crate::theme;

    if !status.is_empty() {
        ui.label(
            egui::RichText::new("Status")
                .strong()
                .size(theme::STATUS_FONT_SZ),
        );
        ui.add_space(4.0);
        for line in status.lines() {
            if line.len() < 3 {
                continue;
            }
            let code = line[..2].trim();
            let path = line[3..].trim();
            let (tag, color) = match code {
                "M" | "MM" | " M" => ("M", theme::GIT_MODIFIED),
                "A" | " A" => ("A", theme::GIT_ADDED),
                "D" | " D" => ("D", theme::GIT_REMOVED),
                "R" | " R" => ("R", theme::GIT_RENAMED),
                "??" => ("?", theme::GIT_UNTRACKED),
                _ => ("?", theme::GIT_UNTRACKED),
            };
            ui.horizontal(|ui| {
                // Coloured badge with subtle background tint
                let (badge_rect, _) =
                    ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(badge_rect, 3.0, color.gamma_multiply(0.25));
                ui.painter().text(
                    badge_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    tag,
                    egui::FontId::monospace(10.0),
                    color,
                );
                ui.label(
                    egui::RichText::new(path)
                        .monospace()
                        .size(theme::STATUS_FONT_SZ),
                );
            });
        }
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);
    }

    if diff.is_empty() {
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No changes")
                    .italics()
                    .color(theme::OVERLAY0)
                    .size(13.0),
            );
            ui.label(
                egui::RichText::new("Working tree is clean")
                    .size(11.0)
                    .color(theme::OVERLAY0),
            );
        });
        return;
    }

    let mut skip_meta = false;
    for line in diff.lines() {
        if line.starts_with("=== ") {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(line).strong().color(theme::GIT_HEADER));
            skip_meta = false;
        } else if line.starts_with("diff --git ") {
            ui.add_space(6.0);
            let fname = line
                .strip_prefix("diff --git ")
                .and_then(|s| s.split(" b/").last())
                .unwrap_or(line);
            ui.label(
                egui::RichText::new(fname)
                    .strong()
                    .color(theme::GIT_FILENAME)
                    .size(13.0),
            );
            skip_meta = true;
        } else if skip_meta
            && (line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ "))
        {
            // skip file header meta lines
        } else if line.starts_with("@@") {
            skip_meta = false;
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::DIFF_FONT_SZ)
                    .color(theme::GIT_HUNK),
            );
        } else if line.starts_with('+') {
            skip_meta = false;
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::DIFF_FONT_SZ)
                    .color(theme::GIT_ADDED),
            );
        } else if line.starts_with('-') {
            skip_meta = false;
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::DIFF_FONT_SZ)
                    .color(theme::GIT_REMOVED),
            );
        } else {
            skip_meta = false;
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::DIFF_FONT_SZ)
                    .color(theme::SUBTEXT0),
            );
        }
    }
}

// ── Markdown preview rendering ────────────────────────────────────────────────

fn render_markdown(ui: &mut egui::Ui, content: &str) {
    use crate::theme;
    let mut in_code = false;
    let mut code_buf: Vec<&str> = Vec::new();

    for line in content.lines() {
        if line.starts_with("```") {
            if in_code {
                // Flush code block into a framed widget
                in_code = false;
                egui::Frame::none()
                    .fill(theme::MD_CODE_BG)
                    .stroke(egui::Stroke::new(1.0, theme::MD_CODE_BORDER))
                    .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                    .rounding(egui::Rounding::same(4.0))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        for code_line in &code_buf {
                            ui.label(
                                egui::RichText::new(*code_line)
                                    .monospace()
                                    .size(12.0)
                                    .color(theme::MD_CODE),
                            );
                        }
                    });
                code_buf.clear();
                ui.add_space(4.0);
            } else {
                in_code = true;
                ui.add_space(4.0);
            }
            continue;
        }
        if in_code {
            code_buf.push(line);
            continue;
        }

        if let Some(t) = line.strip_prefix("# ") {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(t).size(22.0).strong());
            ui.add_space(2.0);
        } else if let Some(t) = line.strip_prefix("## ") {
            ui.add_space(3.0);
            ui.label(egui::RichText::new(t).size(18.0).strong());
        } else if let Some(t) = line.strip_prefix("### ") {
            ui.label(egui::RichText::new(t).size(15.0).strong());
        } else if let Some(t) = line.strip_prefix("#### ") {
            ui.label(egui::RichText::new(t).size(13.0).strong());
        } else if let Some(t) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("•").color(theme::MD_BULLET));
                theme::render_inline(ui, t);
            });
        } else if let Some(t) = line.strip_prefix("> ") {
            ui.horizontal(|ui| {
                // Left border strip
                let bar_h = ui.text_style_height(&egui::TextStyle::Body);
                let (bar_rect, _) =
                    ui.allocate_exact_size(egui::vec2(3.0, bar_h), egui::Sense::hover());
                ui.painter().rect_filled(bar_rect, 0.0, theme::OVERLAY0);
                ui.add_space(6.0);
                ui.label(egui::RichText::new(t).italics().color(theme::MD_BLOCKQUOTE));
            });
        } else if line.starts_with("---") && line.chars().all(|c| c == '-') {
            ui.separator();
        } else if line.is_empty() {
            ui.add_space(4.0);
        } else {
            theme::render_inline(ui, line);
        }
    }
}

// ── Key → PTY bytes ───────────────────────────────────────────────────────────

fn key_to_pty_bytes(key: &egui::Key, modifiers: &egui::Modifiers) -> Option<Vec<u8>> {
    use egui::Key::*;
    let ctrl = modifiers.ctrl;
    let shift = modifiers.shift;
    let alt = modifiers.alt;

    // Ctrl+letter → bytes 1–26 (no other modifiers active)
    if ctrl && !shift && !alt {
        let byte: Option<u8> = match key {
            A => Some(1),
            B => Some(2),
            C => Some(3),
            D => Some(4),
            E => Some(5),
            F => Some(6),
            G => Some(7),
            H => Some(8),
            I => Some(9),
            J => Some(10),
            K => Some(11),
            L => Some(12),
            M => Some(13),
            N => Some(14),
            O => Some(15),
            P => Some(16),
            Q => Some(17),
            R => Some(18),
            S => Some(19),
            T => Some(20),
            U => Some(21),
            V => Some(22),
            W => Some(23),
            X => Some(24),
            Y => Some(25),
            Z => Some(26),
            _ => None,
        };
        if let Some(b) = byte {
            return Some(vec![b]);
        }
    }

    // Alt+letter → ESC + lowercase letter
    if alt && !ctrl && !shift {
        let ch: Option<u8> = match key {
            A => Some(b'a'),
            B => Some(b'b'),
            C => Some(b'c'),
            D => Some(b'd'),
            E => Some(b'e'),
            F => Some(b'f'),
            G => Some(b'g'),
            H => Some(b'h'),
            I => Some(b'i'),
            J => Some(b'j'),
            K => Some(b'k'),
            L => Some(b'l'),
            M => Some(b'm'),
            N => Some(b'n'),
            O => Some(b'o'),
            P => Some(b'p'),
            Q => Some(b'q'),
            R => Some(b'r'),
            S => Some(b's'),
            T => Some(b't'),
            U => Some(b'u'),
            V => Some(b'v'),
            W => Some(b'w'),
            X => Some(b'x'),
            Y => Some(b'y'),
            Z => Some(b'z'),
            _ => None,
        };
        if let Some(c) = ch {
            return Some(vec![0x1b, c]);
        }
    }

    // Alt+Backspace → word delete
    if alt && !ctrl && *key == Backspace {
        return Some(vec![0x1b, 0x7f]);
    }

    // Arrow keys with modifier: \x1b[1;<mod><dir>
    // Modifier codes: 2=shift, 3=alt, 4=shift+alt, 5=ctrl, 6=shift+ctrl, 7=alt+ctrl, 8=all
    let arrow_mod: Option<u8> = match (shift, alt, ctrl) {
        (true, false, false) => Some(b'2'),
        (false, true, false) => Some(b'3'),
        (true, true, false) => Some(b'4'),
        (false, false, true) => Some(b'5'),
        (true, false, true) => Some(b'6'),
        (false, true, true) => Some(b'7'),
        (true, true, true) => Some(b'8'),
        _ => None,
    };
    if let Some(m) = arrow_mod {
        let dir: Option<u8> = match key {
            ArrowUp => Some(b'A'),
            ArrowDown => Some(b'B'),
            ArrowRight => Some(b'C'),
            ArrowLeft => Some(b'D'),
            _ => None,
        };
        if let Some(d) = dir {
            return Some(vec![0x1b, b'[', b'1', b';', m, d]);
        }
    }

    Some(match key {
        Enter => b"\r".to_vec(),
        Backspace => b"\x7f".to_vec(),
        Tab if !shift => b"\t".to_vec(),
        Tab => b"\x1b[Z".to_vec(),
        Escape => b"\x1b".to_vec(),
        ArrowUp => b"\x1b[A".to_vec(),
        ArrowDown => b"\x1b[B".to_vec(),
        ArrowRight => b"\x1b[C".to_vec(),
        ArrowLeft => b"\x1b[D".to_vec(),
        Home => b"\x1b[H".to_vec(),
        End => b"\x1b[F".to_vec(),
        PageUp => b"\x1b[5~".to_vec(),
        PageDown => b"\x1b[6~".to_vec(),
        Delete => b"\x1b[3~".to_vec(),
        Insert => b"\x1b[2~".to_vec(),
        F1 => b"\x1bOP".to_vec(),
        F2 => b"\x1bOQ".to_vec(),
        F3 => b"\x1bOR".to_vec(),
        F4 => b"\x1bOS".to_vec(),
        F5 => b"\x1b[15~".to_vec(),
        F6 => b"\x1b[17~".to_vec(),
        F7 => b"\x1b[18~".to_vec(),
        F8 => b"\x1b[19~".to_vec(),
        F9 => b"\x1b[20~".to_vec(),
        F10 => b"\x1b[21~".to_vec(),
        F11 => b"\x1b[23~".to_vec(),
        F12 => b"\x1b[24~".to_vec(),
        _ => return None,
    })
}

fn mouse_event_bytes(btn: u8, col: u16, row: u16, pressed: bool, sgr: bool) -> Vec<u8> {
    if sgr {
        // SGR extended: \x1b[<btn;col;rowM (press) or \x1b[<btn;col;rowm (release)
        let final_char = if pressed { b'M' } else { b'm' };
        format!(
            "\x1b[<{};{};{}{}",
            btn,
            col + 1,
            row + 1,
            final_char as char
        )
        .into_bytes()
    } else {
        // X10/normal: \x1b[M + (btn+32) + (col+33) + (row+33), clamped to 255
        let b = btn + 32;
        let x = ((col + 1) + 32).min(255) as u8;
        let y = ((row + 1) + 32).min(255) as u8;
        vec![0x1b, b'[', b'M', b, x, y]
    }
}

// ── App ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod title_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn display_title_plain_text_unchanged() {
        assert_eq!(display_title("  vim  "), "vim");
    }

    #[test]
    fn display_title_unix_path_returns_last_segment() {
        assert_eq!(display_title("/home/user/projects/myapp"), "myapp");
    }

    #[test]
    fn display_title_tilde_path_returns_last_segment() {
        assert_eq!(display_title("~/projects/myapp"), "myapp");
    }

    #[test]
    fn display_title_windows_path_returns_last_segment() {
        assert_eq!(display_title("C:\\Users\\testuser\\proj"), "proj");
    }

    #[test]
    fn effective_title_shell_default_uses_cwd() {
        let cwd = Path::new("/home/user/myproject");
        assert_eq!(effective_title("bash", cwd), "myproject");
        assert_eq!(effective_title("Session 1", cwd), "myproject");
        assert_eq!(effective_title("PowerShell.exe", cwd), "myproject");
        assert_eq!(effective_title("", cwd), "myproject");
    }

    #[test]
    fn effective_title_real_title_uses_display_title() {
        let cwd = Path::new("/home/user/myproject");
        assert_eq!(effective_title("vim README.md", cwd), "vim README.md");
    }

    #[test]
    fn effective_title_real_title_strips_path() {
        let cwd = Path::new("/home/user");
        assert_eq!(effective_title("/home/user/projects/src", cwd), "src");
    }

    #[test]
    fn effective_title_empty_cwd_falls_back_to_title() {
        let cwd = Path::new("");
        // cwd has no file_name → falls back to the title text itself
        let result = effective_title("Session 1", cwd);
        assert_eq!(result, "Session 1");
    }
}
