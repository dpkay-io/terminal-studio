use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::FontId;

use crate::pane_tree::{PaneNode, RemoveResult, SplitDir};
use crate::pty::foreground_worker::ForegroundWorker;
use crate::pty::{default_shell, SessionManager, ShellKind};
use crate::renderer::terminal_pass::TerminalGeometry;
use crate::shortcuts::{AppAction, ShortcutRegistry};
use crate::sys_monitor::SysMonitor;
use crate::theme;
use crate::updater::UpdateChecker;
use crate::workspace::{NoteStore, WindowId, WorkspaceStore};
use alacritty_terminal::{
    grid::{Dimensions, Scroll},
    term::TermMode,
};

// ── Submodules ───────────────────────────────────────────────────────────────

mod file_browser;
mod git_diff;
#[allow(dead_code)]
mod git_worker;
mod input;
mod markdown;
mod multi_window;
mod pane;
mod persistence;
pub(crate) mod settings;
mod state;
mod title;
mod ui;
mod watcher;
mod workspace_ui;

// ── Re-imports from submodules ───────────────────────────────────────────────

use file_browser::{
    render_dir_tree, render_flat_file_list, FileEntry, SubdirCache,
};
use git_diff::{render_git_diff, render_inline_diff, GitStageAction};
use git_worker::GitWorker;
use input::{key_to_pty_bytes, mouse_event_bytes};
use markdown::render_markdown;
use multi_window::{ExtraWindow, WindowView};
use pane::{
    FileDiffState, FileEditorState, PaneContent, PaneEntry, RightTab, SessionEntry, TermSelection,
};
use settings::{AppSettings, CursorStyle};
use title::effective_title;
use watcher::WatchState;
use workspace_ui::{WorkspaceDialog, WorkspaceEditDialog};

pub struct App {
    session_manager: SessionManager,
    sessions: Vec<SessionEntry>,
    active_id: Option<u32>,

    panes: Vec<PaneEntry>,
    active_pane_id: Option<u32>,
    next_pane_id: u32,
    right_tab: RightTab,
    shown_md_tabs: HashSet<PathBuf>,
    md_prefer_preview: bool,
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
    show_shortcut_help: bool,
    show_quick_switcher: bool,
    quick_switcher_query: String,
    shortcut_registry: ShortcutRegistry,
    settings: AppSettings,

    // Per-frame terminal geometry for mouse coordinate conversion
    active_term_geo: Option<TerminalGeometry>,
    // Last focused session, for sending focus-in/focus-out events
    last_focused_sid: Option<u32>,
    active_term_ui_id: Option<egui::Id>,
    // Debounced PTY resize targets: (cols, rows, stable_since). PTY is only
    // notified after the size has been stable for 150 ms, preventing ConPTY
    // from clearing the screen on every drag frame.
    resize_debounce: HashMap<u32, (u16, u16, Instant)>,
    // Fractional scroll accumulator — carries sub-line scrolls across frames.
    scroll_accum: HashMap<u32, f32>,
    // Background thread for foreground-process detection; UI reads from cache only.
    foreground_worker: ForegroundWorker,
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

    // Terminal text selection state (per-session)
    term_selection: Option<TermSelection>,
    term_selecting: bool,
    term_selection_sid: Option<u32>,

    // ── Multi-window support ──────────────────────────────────────────────
    /// Extra OS windows opened via "Open in new window" on a workspace.
    extra_windows: Vec<ExtraWindow>,
    /// Counter for generating unique WindowId values.
    next_window_id: u64,
    /// `None` when rendering the main window, `Some(id)` when rendering an extra
    /// viewport. Used by per-window-aware code (e.g. workspace switcher filter).
    current_window_id: Option<WindowId>,

    // ── Phase D: pane split trees ─────────────────────────────────────────
    /// Maps root_pane_id → layout tree for that tab.
    /// Each entry in `panes` that is a "root" (i.e. visible in the tab strip)
    /// has an entry here.  Panes created by splitting are *not* in the tab
    /// strip; they live only as leaves inside another pane's tree.
    pane_trees: HashMap<u32, PaneNode>,
    /// Monotonically-increasing counter for generating unique split node IDs.
    next_split_id: u32,

    // ── Per-frame UI caches ───────────────────────────────────────────────
    /// Cache of `fs::read_dir` results for expanded subdirectories in the
    /// file browser, keyed by absolute path. Without this the dir tree
    /// re-reads disk on every frame for every expanded node.
    /// Entries are refreshed once their TTL expires.
    subdir_cache: HashMap<PathBuf, (Arc<Vec<FileEntry>>, Instant)>,
    /// Last value sent to `ViewportCommand::Title` for this window. Sending
    /// the title every frame causes a syscall (SetWindowTextW on Windows).
    last_title_sent: Option<String>,

    // ── Search state ────────────────────────────────────────────────────
    session_search_query: String,
    session_search_active: bool,
    dir_search_query: String,
    dir_search_active: bool,
    dir_search_debounce_query: String,
    dir_search_debounce_at: Option<Instant>,

    // System resource monitor (CPU / RAM / Network), polled every 2 s.
    sys_monitor: SysMonitor,

    // Self-update: background checker for new releases.
    update_checker: UpdateChecker,

    // Background worker for git status/diff and directory listing.
    git_worker: GitWorker,

    // Terminal content search (Ctrl+F)
    term_search: crate::search::SearchState,

    // Global search across all sessions (Ctrl+Shift+N)
    show_global_search: bool,
    global_search_query: String,
    global_search_debounce_at: Option<Instant>,
    search_worker: crate::search_worker::SearchWorker,
    global_search_selected: usize,

    // Background file search worker for directory panel
    file_search_worker: crate::file_search_worker::FileSearchWorker,

    // Detected URLs in the currently visible terminal content
    detected_urls: Vec<crate::url_detector::DetectedUrl>,

    // Tab drag-to-reorder state
    tab_drag_source: Option<usize>,

    // Deferred actions (set by keyboard shortcuts in central panel, consumed by left_panel next frame)
    deferred_spawn: Option<ShellKind>,
    deferred_duplicate: bool,
    deferred_open_workspace: Option<u64>,

    show_close_all_confirm: bool,

    // Workspace filter for the session list: None = All, Some(None) = Other, Some(Some(id)) = specific workspace
    session_workspace_filter: Option<Option<u64>>,
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_session();
        self.save_windows();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Cursor blink is shared across all windows — toggle once per frame.
        {
            if self.settings.cursor_blink {
                if self.cursor_blink_last.elapsed() >= Duration::from_millis(500) {
                    self.cursor_blink_on = !self.cursor_blink_on;
                    self.cursor_blink_last = Instant::now();
                }
                ctx.request_repaint_after(Duration::from_millis(500));
            } else {
                self.cursor_blink_on = true;
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
                    let _ = entry.pty_tx.send(format!("{}\r", cmd).into_bytes());
                    #[cfg(not(target_os = "windows"))]
                    let _ = entry.pty_tx.send(format!("{}\n", cmd).into_bytes());
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
                for w in &mut self.extra_windows {
                    w.view.shown_md_tabs.remove(&path);
                }
            }
        }

        // ── Git worker: enqueue pending refreshes, drain completed results ─
        {
            let pending: Vec<PathBuf> = self
                .watch_state
                .as_mut()
                .map(|ws| ws.take_pending_git_refreshes())
                .unwrap_or_default();
            for dir in &pending {
                self.git_worker.enqueue_git(dir);
            }

            let watched_dirs: Vec<PathBuf> = self
                .watch_state
                .as_ref()
                .map(|ws| ws.dir_data.keys().cloned().collect())
                .unwrap_or_default();
            let completed: Vec<(PathBuf, (String, String))> = watched_dirs
                .iter()
                .filter_map(|d| self.git_worker.take_git(d).map(|r| (d.clone(), r)))
                .collect();
            if let Some(ws) = &mut self.watch_state {
                for (dir, (diff, status)) in completed {
                    ws.apply_git_result(&dir, diff, status);
                }
            }
        }

        // ── Global search debounce: fire after 200ms of stable input ─────
        if let Some(t) = self.global_search_debounce_at {
            if t.elapsed() >= Duration::from_millis(200) {
                self.global_search_debounce_at = None;
                let query = self.global_search_query.clone();
                if !query.is_empty() {
                    let sessions: Vec<_> = self
                        .sessions
                        .iter()
                        .map(|e| {
                            let title = e.session.read().title();
                            (e.id, title, e.session.clone())
                        })
                        .collect();
                    self.search_worker.search(query, sessions);
                } else {
                    self.search_worker.cancel();
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }

        // ── Render the main window ────────────────────────────────────────
        // The body below operates on the current per-window state held in
        // App's per-window fields. For the main window that's already correct;
        // for each extra window we swap its `WindowView` in/out around the
        // render call.
        self.render_window_body(ctx);

        // ── Render each extra window via an immediate viewport ────────────
        // `show_viewport_immediate` runs synchronously and accepts FnOnce, so
        // the closure can borrow `&mut self`. We snapshot the metadata first
        // (id, viewport_id, title, size) so we don't hold a borrow on
        // `extra_windows` across the call.
        let extras_info: Vec<(usize, egui::ViewportId, String, [f32; 2], WindowId)> = self
            .extra_windows
            .iter()
            .enumerate()
            .map(|(i, w)| {
                (
                    i,
                    w.viewport_id,
                    w.title.clone(),
                    w.inner_size,
                    w.id.clone(),
                )
            })
            .collect();
        for (idx, viewport_id, title, inner_size, win_id) in extras_info {
            let builder = egui::ViewportBuilder::default()
                .with_title(&title)
                .with_inner_size(inner_size)
                .with_decorations(false);
            ctx.show_viewport_immediate(viewport_id, builder, |vp_ctx, _class| {
                // Swap this extra window's view into App's per-window fields,
                // render, then swap back. The placeholder used during swap is
                // discarded once the real view is restored.
                let mut view = std::mem::replace(
                    &mut self.extra_windows[idx].view,
                    WindowView::new_for_workspace(0),
                );
                self.swap_view(&mut view);
                let prev_window = self.current_window_id.take();
                self.current_window_id = Some(win_id.clone());

                self.render_window_body(vp_ctx);

                // Capture the current inner_size for persistence.
                let new_size = vp_ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
                if let Some(sz) = new_size {
                    self.extra_windows[idx].inner_size = [sz.x, sz.y];
                }

                self.current_window_id = prev_window;
                self.swap_view(&mut view);
                self.extra_windows[idx].view = view;

                if vp_ctx.input(|i| i.viewport().close_requested()) {
                    self.extra_windows[idx].close_requested = true;
                    vp_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }

        // Process pending close requests collected from the viewport callbacks.
        let closing_indices: Vec<usize> = self
            .extra_windows
            .iter()
            .enumerate()
            .filter(|(_, w)| w.close_requested)
            .map(|(i, _)| i)
            .collect();
        if !closing_indices.is_empty() {
            for &idx in closing_indices.iter().rev() {
                let ws_id = self.extra_windows[idx].workspace_id;
                self.extra_windows.remove(idx);
                if let Some(ws) = self
                    .workspace_store
                    .workspaces
                    .iter_mut()
                    .find(|w| w.id == ws_id)
                {
                    ws.host_window_id = None;
                }
            }
            self.workspace_store.save();
            self.save_windows();
        }
    }
}
impl App {
    fn render_window_body(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().minimized.unwrap_or(false)) {
            return;
        }

        self.render_titlebar(ctx);

        self.render_left_panel(ctx);

        // ── Snapshot right-panel data before closures capture self ──────────
        let active_cwd = self.active_cwd();
        let active_tab = self.right_tab.clone();

        // Snapshot data from the watch state. Heavy fields (dir_entries,
        // markdown contents) are held behind Arc so the per-frame snapshot is
        // O(1). Markdown contents are *only* fetched when the Markdown tab is
        // active — otherwise we just collect the paths needed to populate the
        // tab strip.
        struct PanelSnap {
            is_git: bool,
            git_diff: String,
            git_status: String,
            dir_entries: Arc<Vec<FileEntry>>,
            md_paths: Vec<PathBuf>,
            md_active_content: Option<Arc<String>>,
        }
        let snap: PanelSnap = match (active_cwd.as_ref(), self.watch_state.as_ref()) {
            (Some(cwd), Some(ws)) => match ws.dir_data.get(cwd) {
                Some(d) => {
                    let md_paths: Vec<PathBuf> = d.md_files.keys().cloned().collect();
                    let md_active_content = if let RightTab::Markdown(p) = &self.right_tab {
                        d.md_files.get(p).cloned()
                    } else {
                        None
                    };
                    PanelSnap {
                        is_git: d.is_git,
                        git_diff: d.git_diff.clone(),
                        git_status: d.git_status.clone(),
                        dir_entries: Arc::clone(&d.dir_entries),
                        md_paths,
                        md_active_content,
                    }
                }
                None => PanelSnap {
                    is_git: false,
                    git_diff: String::new(),
                    git_status: String::new(),
                    dir_entries: Arc::new(Vec::new()),
                    md_paths: Vec::new(),
                    md_active_content: None,
                },
            },
            _ => PanelSnap {
                is_git: false,
                git_diff: String::new(),
                git_status: String::new(),
                dir_entries: Arc::new(Vec::new()),
                md_paths: Vec::new(),
                md_active_content: None,
            },
        };
        let PanelSnap {
            is_git,
            git_diff,
            git_status,
            dir_entries,
            md_paths,
            md_active_content,
        } = snap;

        let mut md_tabs: Vec<PathBuf> = md_paths
            .into_iter()
            .filter(|p| self.shown_md_tabs.contains(p))
            .collect();
        md_tabs.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

        let mut new_tab: Option<RightTab> = None;
        let mut close_tab: Option<PathBuf> = None;
        let mut open_editor: Option<PathBuf> = None;
        let mut open_ws_dialog: Option<PathBuf> = None;
        let mut open_terminal_at: Option<PathBuf> = None;
        let mut git_stage_action: Option<GitStageAction> = None;
        let mut git_open_diff_file: Option<String> = None;
        let mut git_open_file: Option<String> = None;

        // Snapshot current note so TextEdit can mutate it inside the closure
        let mut note_text = self.note_store.get(self.active_group).to_string();

        // ── Right panel ──────────────────────────────────────────────────────
        if self.show_right_panel {
            egui::SidePanel::right("right_panel")
                .default_width(300.0)
                .width_range(100.0..=600.0)
                .resizable(true)
                .frame(egui::Frame::none().inner_margin(egui::Margin::ZERO))
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
                            .fill(theme::active().surface0)
                            .inner_margin(egui::Margin::ZERO)
                            .show(ui, |ui| {
                                egui::ScrollArea::horizontal()
                                    .id_source("right_tab_bar")
                                    .max_height(theme::HEADER_H)
                                    .show(ui, |ui| {
                                        ui.set_min_height(theme::HEADER_H);
                                        ui.horizontal(|ui| {
                                            ui.set_min_height(theme::HEADER_H);
                                            ui.spacing_mut().item_spacing.x = 0.0;

                                            {
                                                let resp = ui.selectable_label(
                                                    active_tab == RightTab::Directory,
                                                    egui::RichText::new("Directory")
                                                        .size(theme::HEADER_FONT_SZ),
                                                );
                                                if resp.clicked() {
                                                    new_tab = Some(RightTab::Directory);
                                                }
                                                resp.on_hover_text("Ctrl+Shift+D");
                                            }

                                            if is_git {
                                                let resp = ui.selectable_label(
                                                    active_tab == RightTab::GitDiff,
                                                    egui::RichText::new("Git Diff")
                                                        .size(theme::HEADER_FONT_SZ),
                                                );
                                                if resp.clicked() {
                                                    new_tab = Some(RightTab::GitDiff);
                                                }
                                                resp.on_hover_text("Ctrl+Shift+G");
                                            }

                                            for path in &md_tabs {
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
                                                                .color(theme::active().overlay1),
                                                        )
                                                        .frame(false)
                                                        .min_size(egui::vec2(
                                                            theme::HEADER_H,
                                                            theme::HEADER_H,
                                                        )),
                                                    )
                                                    .on_hover_text("Close tab (Ctrl+Shift+W)")
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
                                            // ── Workspace name + path ────────────────
                                            ui.horizontal(|ui| {
                                                if let Some(ws) = self.workspace_store.find_for_cwd(cwd) {
                                                    let c = ws.color;
                                                    ui.label(
                                                        egui::RichText::new(&ws.name)
                                                            .strong()
                                                            .size(theme::CWD_FONT_SZ)
                                                            .color(egui::Color32::from_rgb(c[0], c[1], c[2])),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new("›")
                                                            .size(theme::CWD_FONT_SZ)
                                                            .color(theme::active().overlay0),
                                                    );
                                                }
                                                ui.label(
                                                    egui::RichText::new(theme::short_path(cwd))
                                                        .monospace()
                                                        .size(theme::CWD_FONT_SZ)
                                                        .color(theme::active().fg_path),
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
                                            // ── Directory search bar ───────────────────
                                            if self.dir_search_active {
                                                let dir_search_id =
                                                    egui::Id::new("dir_search_input");
                                                ui.horizontal(|ui| {
                                                    ui.label(egui::RichText::new("🔍").size(12.0));
                                                    let te = egui::TextEdit::singleline(
                                                        &mut self.dir_search_query,
                                                    )
                                                    .desired_width(
                                                        ui.available_width() - theme::BTN_W,
                                                    )
                                                    .hint_text("Search files…")
                                                    .font(egui::FontId::proportional(
                                                        theme::SESSION_FONT_SZ,
                                                    ))
                                                    .id(dir_search_id);
                                                    let r = ui.add(te);
                                                    if r.lost_focus()
                                                        && ui.input(|i| {
                                                            i.key_pressed(egui::Key::Escape)
                                                        })
                                                    {
                                                        self.dir_search_active = false;
                                                        self.dir_search_query.clear();
                                                        self.dir_search_debounce_query.clear();
                                                        self.dir_search_debounce_at = None;
                                                        self.file_search_worker.cancel();
                                                    }
                                                    r.request_focus();
                                                });

                                                if self.dir_search_query
                                                    != self.dir_search_debounce_query
                                                {
                                                    self.dir_search_debounce_at =
                                                        Some(Instant::now());
                                                    self.dir_search_debounce_query =
                                                        self.dir_search_query.clone();
                                                }
                                            }

                                            ui.add_space(theme::SP_SM);

                                            // ── Dispatch to file search worker ─────────
                                            let show_search_results = if self.dir_search_active
                                                && !self.dir_search_query.is_empty()
                                            {
                                                let debounce_ready = self.dir_search_debounce_at.map_or(true, |t| {
                                                    t.elapsed() >= Duration::from_millis(150)
                                                });
                                                if debounce_ready {
                                                    let results = self.file_search_worker.results();
                                                    if results.query != self.dir_search_query || results.root != *cwd {
                                                        drop(results);
                                                        self.file_search_worker.search(
                                                            self.dir_search_query.clone(),
                                                            cwd.clone(),
                                                        );
                                                    }
                                                } else {
                                                    ctx.request_repaint_after(
                                                        std::time::Duration::from_millis(160),
                                                    );
                                                }
                                                true
                                            } else {
                                                false
                                            };

                                            if show_search_results {
                                                let results = self.file_search_worker.results();
                                                if !results.completed {
                                                    ui.label(
                                                        egui::RichText::new("Searching…")
                                                            .italics()
                                                            .color(theme::active().overlay0)
                                                            .size(12.0),
                                                    );
                                                } else {
                                                    let entries: Vec<FileEntry> = results.matches.iter().map(|m| FileEntry {
                                                        name: m.name.clone(),
                                                        path: m.path.clone(),
                                                        is_dir: m.is_dir,
                                                    }).collect();
                                                    drop(results);
                                                    render_flat_file_list(
                                                        ui,
                                                        &entries,
                                                        cwd,
                                                        &mut open_editor,
                                                        &mut open_terminal_at,
                                                    );
                                                }
                                            } else {
                                                let mut cache = SubdirCache {
                                                    map: &mut self.subdir_cache,
                                                    ttl: Duration::from_secs(2),
                                                };
                                                render_dir_tree(
                                                    ui,
                                                    &dir_entries,
                                                    &mut open_editor,
                                                    &mut open_terminal_at,
                                                    &mut cache,
                                                );
                                            }
                                        } else {
                                            ui.label(
                                                egui::RichText::new("(no active session)")
                                                    .italics()
                                                    .color(theme::active().overlay0)
                                                    .size(12.0),
                                            );
                                        }
                                    }
                                    RightTab::GitDiff => {
                                        let result = render_git_diff(ui, &git_diff, &git_status);
                                        git_stage_action = result.stage_action;
                                        if result.open_diff_file.is_some() {
                                            git_open_diff_file = result.open_diff_file;
                                        }
                                        if result.open_file.is_some() {
                                            git_open_file = result.open_file;
                                        }
                                    }
                                    RightTab::Markdown(_path) => {
                                        let content = md_active_content
                                            .as_deref()
                                            .map(|s| s.as_str())
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
                            theme::active().ws_div_active
                        } else {
                            theme::active().ws_div_idle
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
                        let header_rect = egui::Rect::from_min_size(
                            notes_rect.min,
                            egui::vec2(notes_rect.width(), theme::HEADER_H),
                        );
                        ui.painter().rect_filled(
                            header_rect,
                            0.0,
                            theme::active().bg_workspace_fill,
                        );

                        if !self.notes_panel_collapsed {
                            let content_rect = egui::Rect::from_min_max(
                                egui::pos2(notes_rect.left(), notes_rect.min.y + theme::HEADER_H),
                                notes_rect.max,
                            );
                            ui.painter()
                                .rect_filled(content_rect, 0.0, theme::active().bg_term);
                        }

                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), theme::HEADER_H),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new("Notes")
                                        .strong()
                                        .size(theme::HEADER_FONT_SZ),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let arrow = if self.notes_panel_collapsed {
                                            "▶"
                                        } else {
                                            "▼"
                                        };
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new(arrow)
                                                        .size(theme::HEADER_FONT_SZ),
                                                )
                                                .min_size(egui::vec2(
                                                    theme::HEADER_H,
                                                    theme::HEADER_H,
                                                ))
                                                .frame(false),
                                            )
                                            .on_hover_text("Toggle notes (Ctrl+Shift+J)")
                                            .clicked()
                                        {
                                            self.notes_panel_collapsed =
                                                !self.notes_panel_collapsed;
                                        }
                                    },
                                );
                            },
                        );

                        if !self.notes_panel_collapsed {
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

        // Open workspace dialog
        if let Some(path) = open_ws_dialog {
            if self.workspace_dialog.is_none() {
                self.workspace_dialog = Some(WorkspaceDialog::new(path));
            }
        }

        // Execute git stage/unstage action
        if let Some(action) = git_stage_action {
            if let Some(cwd) = active_cwd.as_ref() {
                use std::process::Command;
                let ok = match &action {
                    GitStageAction::Stage(path) => Command::new("git")
                        .args(["add", "--", path])
                        .current_dir(cwd)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false),
                    GitStageAction::Unstage(path) => Command::new("git")
                        .args(["reset", "HEAD", "--", path])
                        .current_dir(cwd)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false),
                    GitStageAction::StageAll => Command::new("git")
                        .args(["add", "-A"])
                        .current_dir(cwd)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false),
                    GitStageAction::UnstageAll => Command::new("git")
                        .args(["reset", "HEAD"])
                        .current_dir(cwd)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false),
                };
                if ok {
                    self.git_worker.enqueue_git(cwd);
                }
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
                PaneContent::DeferredTerminal { cwd, .. } => cwd
                    .as_ref()
                    .and_then(|c| self.workspace_store.find_for_cwd(c).map(|w| w.color)),
                PaneContent::FileEditor(ed) => ed.workspace_id.and_then(|id| {
                    self.workspace_store
                        .workspaces
                        .iter()
                        .find(|w| w.id == id)
                        .map(|w| w.color)
                }),
                PaneContent::FileDiff(_) => None,
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
        // If the focused pane's computed group no longer matches `active_group`, follow it.
        // This happens when the user runs `cd` in a terminal and the new CWD belongs to a
        // different workspace — without this, the user keeps typing into a pane they can't see.
        //
        // Only fall back to "first visible pane in current group" when the focused pane was
        // actually removed (e.g., closed). Callers that deliberately change `active_group`
        // without changing `active_pane_id` (e.g., `open_workspace_in_new_window`) must clear
        // `active_pane_id` themselves so this fallback path runs instead of the follow path.
        if let Some(pid) = self.active_pane_id {
            if !visible_indices.iter().any(|&i| self.panes[i].id == pid) {
                let pane_idx = self.panes.iter().position(|p| p.id == pid);
                if let Some(idx) = pane_idx {
                    // Pane still exists — its group changed. Follow it.
                    self.active_group = pane_groups[idx];
                } else {
                    // Pane was removed — fall back to first pane in the current group.
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
        }

        // ── Output variables collected inside the central panel closure ─────
        let divider_drags: Vec<(usize, usize, f32, f32, f32)> = vec![]; // retained for post-closure mutation compatibility
        let mut close_pane_id: Option<u32> = None;
        let mut clicked_pane_id: Option<u32> = None;
        let mut editor_saves: Vec<u32> = vec![];
        let mut editor_preview_toggles: Vec<u32> = vec![];
        let mut pane_widths_snap: Vec<(u32, f32)> = vec![];
        let mut resize_total_h: f32 = 0.0;
        let mut resize_cell_w: f32 = 0.0;
        let mut resize_cell_h: f32 = 0.0;
        let equalize_widths: bool = false;
        let mut panel_w_snap: f32 = 0.0;
        // Phase D: split / close-split requests collected from key handler
        let mut split_request: Option<SplitDir> = None;
        let mut close_split_pane: bool = false;
        // Phase D: split divider ratio changes (split_id, new_ratio)
        let mut split_ratio_changes: Vec<(u32, f32)> = vec![];

        // ── Central panel ──────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::none().inner_margin(egui::Margin::ZERO))
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
                    theme::active().text,
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
                            .color(theme::active().overlay0).size(14.0)
                    );
                });
            } else {
                // ── Tab bar + single content area ────────────────────────────
                let tab_h   = theme::HEADER_H;
                let panel_h = panel_rect.height();

                let tab_actions_w = theme::TAB_ACTIONS_W;
                let tab_scroll_w = (panel_rect.width() - tab_actions_w).max(0.0);
                let tab_bar_rect = egui::Rect::from_min_size(
                    panel_rect.min,
                    egui::vec2(tab_scroll_w, tab_h),
                );
                let tab_actions_rect = egui::Rect::from_min_size(
                    egui::pos2(panel_rect.min.x + tab_scroll_w, panel_rect.min.y),
                    egui::vec2(tab_actions_w, tab_h),
                );
                let content_rect = egui::Rect::from_min_size(
                    egui::pos2(panel_rect.min.x, panel_rect.min.y + tab_h),
                    egui::vec2(panel_rect.width(), (panel_h - tab_h).max(0.0)),
                );

                // ── Tab bar (horizontally scrollable) ────────────────────────
                ui.allocate_ui_at_rect(tab_bar_rect, |ui| {
                    ui.painter().rect_filled(tab_bar_rect, 0.0, theme::active().surface0);
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

                                    let display = match &self.panes[i].content {
                                        PaneContent::Terminal(sid) => {
                                            let sid = *sid;
                                            self.sessions.iter()
                                                .find(|e| e.id == sid)
                                                .map(|e| {
                                                    let s = e.session.read();
                                                    let title = s.title();
                                                    let cwd = s.cwd.clone();
                                                    drop(s);
                                                    let fg = self.foreground_worker.get(e.id);
                                                    effective_title(&title, &cwd, fg.as_ref(), Some(&e.shell))
                                                })
                                                .unwrap_or_else(|| format!("Terminal {sid}"))
                                        }
                                        PaneContent::DeferredTerminal { cwd, .. } => {
                                            let cwd_path = cwd.clone().unwrap_or_default();
                                            effective_title("", &cwd_path, None, None)
                                        }
                                        PaneContent::FileEditor(ed) => {
                                            let fname = ed.path.file_name()
                                                .map(|n| n.to_string_lossy().into_owned())
                                                .unwrap_or_default();
                                            if ed.save_error {
                                                format!("! {fname}")
                                            } else if ed.dirty {
                                                format!("* {fname}")
                                            } else {
                                                fname
                                            }
                                        }
                                        PaneContent::FileDiff(d) => {
                                            let fname = d.path.file_name()
                                                .map(|n| n.to_string_lossy().into_owned())
                                                .unwrap_or_default();
                                            format!("\u{21c4} {fname}")
                                        }
                                    };

                                    let (_, tab_rect) = ui.allocate_space(egui::vec2(theme::TAB_W, tab_h));

                                    let hbg = theme::header_bg(ws_color, is_active);
                                    let title_color = match ws_color {
                                        Some(c) => theme::text_on(theme::tinted(c, if is_active { 0.75 } else { 0.35 })),
                                        None    => if is_active { theme::active().text } else { theme::active().subtext1 },
                                    };

                                    let painter = ui.painter().clone();
                                    painter.rect_filled(tab_rect, 0.0, hbg);

                                    // Workspace colour strip on left edge
                                    if let Some(c) = ws_color {
                                        painter.rect_filled(
                                            egui::Rect::from_min_size(tab_rect.min, egui::vec2(theme::TAB_COLOR_STRIP_W, tab_h)),
                                            0.0,
                                            theme::from_rgb(c),
                                        );
                                    }

                                    // Bottom highlight on active tab
                                    if is_active {
                                        painter.rect_filled(
                                            egui::Rect::from_min_size(
                                                egui::pos2(tab_rect.min.x, tab_rect.max.y - theme::TAB_ACTIVE_HIGHLIGHT_H),
                                                egui::vec2(theme::TAB_W, theme::TAB_ACTIVE_HIGHLIGHT_H),
                                            ),
                                            0.0, theme::active().text,
                                        );
                                    }

                                    // Right-edge separator between tabs
                                    painter.rect_filled(
                                        egui::Rect::from_min_size(
                                            egui::pos2(tab_rect.max.x - theme::STROKE_THIN, tab_rect.min.y),
                                            egui::vec2(theme::STROKE_THIN, tab_h),
                                        ),
                                        0.0, theme::active().surface2,
                                    );

                                    // Register tab-wide click first (lower z-order); close button
                                    // is registered second so it has higher priority in egui's
                                    // last-registered-wins model for overlapping regions.
                                    let tab_resp = ui.interact(
                                        tab_rect,
                                        egui::Id::new(("tab_click", pane_id)),
                                        egui::Sense::click_and_drag(),
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
                                        painter.rect_filled(close_rect, 0.0, theme::active().danger_bg);
                                    }
                                    painter.text(
                                        close_rect.center(), egui::Align2::CENTER_CENTER,
                                        "×", egui::FontId::proportional(14.0),
                                        theme::active().danger_fg,
                                    );

                                    // Title text (clipped before close button)
                                    let text_x = tab_rect.min.x + theme::TAB_PAD_X + if ws_color.is_some() { theme::TAB_COLOR_STRIP_W } else { 0.0 };
                                    painter.with_clip_rect(egui::Rect::from_min_max(
                                        egui::pos2(text_x, tab_rect.min.y),
                                        egui::pos2(close_rect.min.x - theme::SP_XS, tab_rect.max.y),
                                    )).text(
                                        egui::pos2(text_x, tab_rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        &display,
                                        egui::FontId::proportional(theme::HEADER_FONT_SZ),
                                        title_color,
                                    );

                                    if close_resp.on_hover_text("Close tab (Ctrl+Shift+W)").clicked() {
                                        close_pane_id = Some(pane_id);
                                    } else if tab_resp.clicked() {
                                        clicked_pane_id = Some(pane_id);
                                    }

                                    // Tab drag-to-reorder
                                    if tab_resp.drag_started() {
                                        self.tab_drag_source = Some(i);
                                    }
                                    if let Some(drag_idx) = self.tab_drag_source {
                                        if tab_resp.hovered() && drag_idx != i {
                                            let indicator_x = if drag_idx < i { tab_rect.max.x } else { tab_rect.min.x };
                                            ui.painter().rect_filled(
                                                egui::Rect::from_min_size(
                                                    egui::pos2(indicator_x - 1.5, tab_rect.min.y),
                                                    egui::vec2(3.0, tab_h),
                                                ),
                                                0.0,
                                                theme::active().blue,
                                            );
                                        }
                                    }

                                    // Right-click context menu for tab operations.
                                    let extra_window_names: Vec<(u64, String)> = self
                                        .extra_windows
                                        .iter()
                                        .map(|w| (w.workspace_id, w.title.clone()))
                                        .collect();
                                    tab_resp.context_menu(|ui| {
                                        ui.label(
                                            egui::RichText::new("Move tab to window…")
                                                .size(12.0)
                                                .color(egui::Color32::from_gray(180)),
                                        );
                                        ui.separator();
                                        if extra_window_names.is_empty() {
                                            ui.label(
                                                egui::RichText::new("No other windows")
                                                    .italics()
                                                    .color(egui::Color32::from_gray(140)),
                                            );
                                        } else {
                                            for (_, win_title) in &extra_window_names {
                                                ui.add_enabled_ui(false, |ui| {
                                                    let _ = ui.button(win_title);
                                                });
                                            }
                                            ui.label(
                                                egui::RichText::new("(tab move coming in Phase D)")
                                                    .italics()
                                                    .size(11.0)
                                                    .color(egui::Color32::from_gray(130)),
                                            );
                                        }
                                    });
                                }
                            });
                        });
                });

                // ── Tab-bar action buttons (split / close-all) ──────────
                ui.allocate_ui_at_rect(tab_actions_rect, |ui| {
                    ui.painter().rect_filled(tab_actions_rect, 0.0, theme::active().surface0);
                    // Left separator
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            tab_actions_rect.left_top(),
                            egui::vec2(theme::STROKE_THIN, tab_h),
                        ),
                        0.0,
                        theme::active().surface2,
                    );
                    let icon_sz = egui::vec2(theme::BTN_W, tab_h);
                    let t = theme::active();
                    let mut x = tab_actions_rect.min.x + 2.0;

                    let icon_stroke = egui::Stroke::new(1.2, t.subtext1);
                    let icon_hover_stroke = egui::Stroke::new(1.2, t.text);
                    let icon_inset = 6.0_f32;

                    // Split horizontal (side-by-side)
                    let split_h_rect = egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
                    let split_h_resp = ui.interact(split_h_rect, egui::Id::new("tab_split_h"), egui::Sense::click());
                    let sh_stroke = if split_h_resp.hovered() {
                        ui.painter().rect_filled(split_h_rect, 2.0, t.surface2);
                        icon_hover_stroke
                    } else { icon_stroke };
                    {
                        let r = split_h_rect.shrink(icon_inset);
                        let p = ui.painter();
                        p.rect_stroke(r, 1.0, sh_stroke);
                        p.line_segment([r.center_top(), r.center_bottom()], sh_stroke);
                    }
                    if split_h_resp.on_hover_text("Split horizontal (Ctrl+Shift+\\)").clicked() {
                        split_request = Some(SplitDir::Horizontal);
                    }
                    x += icon_sz.x;

                    // Split vertical (top-bottom)
                    let split_v_rect = egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
                    let split_v_resp = ui.interact(split_v_rect, egui::Id::new("tab_split_v"), egui::Sense::click());
                    let sv_stroke = if split_v_resp.hovered() {
                        ui.painter().rect_filled(split_v_rect, 2.0, t.surface2);
                        icon_hover_stroke
                    } else { icon_stroke };
                    {
                        let r = split_v_rect.shrink(icon_inset);
                        let p = ui.painter();
                        p.rect_stroke(r, 1.0, sv_stroke);
                        p.line_segment([r.left_center(), r.right_center()], sv_stroke);
                    }
                    if split_v_resp.on_hover_text("Split vertical (Ctrl+Shift+-)").clicked() {
                        split_request = Some(SplitDir::Vertical);
                    }
                    x += icon_sz.x;

                    // Close all tabs in workspace
                    let close_all_rect = egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
                    let close_all_resp = ui.interact(close_all_rect, egui::Id::new("tab_close_all"), egui::Sense::click());
                    if close_all_resp.hovered() {
                        ui.painter().rect_filled(close_all_rect, 2.0, t.danger_bg);
                    }
                    ui.painter().text(
                        close_all_rect.center(), egui::Align2::CENTER_CENTER,
                        "\u{2716}", egui::FontId::proportional(12.0),
                        t.danger_fg,
                    );
                    if close_all_resp.on_hover_text("Close all sessions").clicked() {
                        self.show_close_all_confirm = true;
                    }
                });

                // Tab drag-to-reorder: finalize on pointer release
                if self.tab_drag_source.is_some() && ui.input(|i| i.pointer.any_released()) {
                    if let Some(drag_idx) = self.tab_drag_source.take() {
                        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                        if let Some(pos) = pointer_pos {
                            // Find which tab the pointer is over by index
                            let tab_count = visible_indices.len();
                            if tab_count > 1 {
                                let tab_bar_x = ui.min_rect().min.x;
                                let rel_x = pos.x - tab_bar_x;
                                let target_i = ((rel_x / theme::TAB_W) as usize).min(tab_count - 1);
                                let target_vis = visible_indices.get(target_i).copied();
                                let drag_vis = visible_indices.get(drag_idx).copied();
                                if let (Some(from), Some(to)) = (drag_vis, target_vis) {
                                    if from != to {
                                        let pane = self.panes.remove(from);
                                        let insert_at = if to > from { to - 1 } else { to };
                                        self.panes.insert(insert_at, pane);
                                    }
                                }
                            }
                        }
                    }
                }

                // ── Active tab content (full-size, split-aware) ──────────────
                if let Some(&active_i) = visible_indices.iter()
                    .find(|&&i| Some(self.panes[i].id) == active_pane_id_snap)
                {
                    let root_pane_id = self.panes[active_i].id;

                    // Render the pane tree rooted at root_pane_id recursively.
                    // We clone the tree to avoid borrowing self during the render pass.
                    let tree = self.pane_trees.get(&root_pane_id).cloned()
                        .unwrap_or_else(|| PaneNode::Leaf {
                            pane_id: root_pane_id,
                            last_size: self.panes[active_i].last_size,
                        });

                    // Recursive renderer (closure-based to avoid an out-of-closure fn
                    // that would need to take all these args).
                    struct RenderCtx<'a> {
                        sessions: &'a [SessionEntry],
                        panes: &'a [PaneEntry],
                        editor_texts: &'a mut Vec<(u32, Option<String>)>,
                        cursor_blink_on: bool,
                        focused_pane_id: Option<u32>,
                        active_term_geo: &'a mut Option<TerminalGeometry>,
                        active_term_ui_id: &'a mut Option<egui::Id>,
                        clicked_pane_id: &'a mut Option<u32>,
                        editor_saves: &'a mut Vec<u32>,
                        editor_preview_toggles: &'a mut Vec<u32>,
                        pane_widths_snap: &'a mut Vec<(u32, f32)>,
                        split_ratio_changes: &'a mut Vec<(u32, f32)>,
                        term_selection: &'a Option<TermSelection>,
                        term_selection_sid: Option<u32>,
                        workspace_dialog_open: bool,
                        workspace_edit_dialog_open: bool,
                        show_settings: bool,
                        font_size: f32,
                        cursor_style: CursorStyle,
                    }

                    fn render_node(
                        ui: &mut egui::Ui,
                        node: &PaneNode,
                        rect: egui::Rect,
                        rctx: &mut RenderCtx<'_>,
                    ) {
                        use crate::pane_tree::{split_rect, SplitDir};
                        match node {
                            PaneNode::Leaf { pane_id, .. } => {
                                let pane_id = *pane_id;
                                let is_focused = rctx.focused_pane_id == Some(pane_id);
                                let pane = rctx.panes.iter().find(|p| p.id == pane_id);
                                let Some(pane) = pane else { return };

                                // Track width for resize
                                rctx.pane_widths_snap.push((pane_id, rect.width()));

                                // Focused pane gets a highlighted border
                                if is_focused && rctx.focused_pane_id.is_some() {
                                    // Check if there's actually a split (more than one leaf in visible tree)
                                    // We draw a subtle focus border only when there's a sibling
                                    // (a simple single-pane view has no border)
                                }

                                ui.allocate_ui_at_rect(rect, |ui| {
                                    match &pane.content {
                                        PaneContent::Terminal(sid) => {
                                            let sid = *sid;
                                            if let Some(idx) = rctx.sessions.iter().position(|e| e.id == sid) {
                                                let session = Arc::clone(&rctx.sessions[idx].session);
                                                let sel_range = if rctx.term_selection_sid == Some(sid) {
                                                    rctx.term_selection.as_ref().map(|s| {
                                                        crate::renderer::terminal_pass::SelectionRange {
                                                            start_col: s.start_col,
                                                            start_row: s.start_row,
                                                            end_col: s.end_col,
                                                            end_row: s.end_row,
                                                        }
                                                    })
                                                } else {
                                                    None
                                                };
                                                let geo = crate::renderer::terminal_pass::TerminalView::new(session)
                                                    .show(ui, is_focused, rctx.cursor_blink_on, sel_range.as_ref(), rctx.font_size, rctx.cursor_style);
                                                if is_focused {
                                                    *rctx.active_term_geo = Some(geo);
                                                }
                                            }
                                            if is_focused {
                                                let this_id = ui.id();
                                                *rctx.active_term_ui_id = Some(this_id);
                                                let dialog_open = rctx.workspace_dialog_open
                                                    || rctx.workspace_edit_dialog_open
                                                    || rctx.show_settings;
                                                // Re-assert focus only when no other widget owns
                                                // it. This recovers from transient focus steals
                                                // (scroll areas, autocomplete) without trampling
                                                // intentional focus on widgets like the notes
                                                // TextEdit or the workspace search box.
                                                if !dialog_open {
                                                    let other_focused = ui.memory(|m| {
                                                        m.focused()
                                                            .map(|id| id != this_id)
                                                            .unwrap_or(false)
                                                    });
                                                    if !other_focused {
                                                        ui.memory_mut(|m| m.request_focus(this_id));
                                                    }
                                                }
                                            }
                                        }
                                        PaneContent::DeferredTerminal { .. } => {
                                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme::active().bg_term);
                                        }
                                        PaneContent::FileEditor(ed) => {
                                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme::active().bg_term);
                                            if !file_browser::is_supported_text_file(&ed.path, &ed.content) {
                                                ui.centered_and_justified(|ui| {
                                                    ui.label(
                                                        egui::RichText::new("File type not supported for preview")
                                                            .size(16.0)
                                                            .color(theme::active().overlay0),
                                                    );
                                                });
                                            } else {
                                            let is_md = ed.path.extension().and_then(|e| e.to_str()) == Some("md");
                                            let previewing = is_md && ed.show_preview;
                                            if is_md {
                                                ui.horizontal(|ui| {
                                                    let t = theme::active();
                                                    let raw_color = if !previewing { t.text } else { t.overlay0 };
                                                    let preview_color = if previewing { t.text } else { t.overlay0 };
                                                    let raw_bg = if !previewing { t.surface2 } else { egui::Color32::TRANSPARENT };
                                                    let preview_bg = if previewing { t.surface2 } else { egui::Color32::TRANSPARENT };
                                                    if ui.add(
                                                        egui::Button::new(
                                                            egui::RichText::new("Raw")
                                                                .size(11.0)
                                                                .color(raw_color),
                                                        )
                                                        .fill(raw_bg)
                                                        .rounding(egui::Rounding::same(theme::ROUNDING))
                                                        .min_size(egui::vec2(56.0, 20.0)),
                                                    ).clicked() && previewing {
                                                        rctx.editor_preview_toggles.push(pane_id);
                                                    }
                                                    if ui.add(
                                                        egui::Button::new(
                                                            egui::RichText::new("Preview")
                                                                .size(11.0)
                                                                .color(preview_color),
                                                        )
                                                        .fill(preview_bg)
                                                        .rounding(egui::Rounding::same(theme::ROUNDING))
                                                        .min_size(egui::vec2(56.0, 20.0)),
                                                    ).clicked() && !previewing {
                                                        rctx.editor_preview_toggles.push(pane_id);
                                                    }
                                                });
                                                ui.separator();
                                            }
                                            if previewing {
                                                if let Some(et) = rctx.editor_texts.iter().find(|(id, _)| *id == pane_id) {
                                                    if let Some(ref text) = et.1 {
                                                        egui::ScrollArea::both()
                                                            .id_source(("editor_preview_scroll", pane_id))
                                                            .auto_shrink([false; 2])
                                                            .show(ui, |ui| {
                                                                render_markdown(ui, text);
                                                            });
                                                    }
                                                }
                                            } else if let Some(et) = rctx.editor_texts.iter_mut().find(|(id, _)| *id == pane_id) {
                                                if let Some(ref mut text) = et.1 {
                                                    egui::ScrollArea::both()
                                                        .id_source(("editor_scroll", pane_id))
                                                        .auto_shrink([false; 2])
                                                        .show(ui, |ui| {
                                                            let line_count = text.lines().count().max(1);
                                                            let digits = ((line_count as f64).log10().floor() as usize) + 1;
                                                            let char_w = 7.5_f32; // approx monospace char width at default size
                                                            let gutter_w = (digits as f32 + 1.5) * char_w;
                                                            let line_h = ui.text_style_height(&egui::TextStyle::Monospace);

                                                            ui.horizontal_top(|ui| {
                                                                ui.spacing_mut().item_spacing.x = 0.0;
                                                                // Line number gutter
                                                                ui.vertical(|ui| {
                                                                    ui.set_min_width(gutter_w);
                                                                    // Pad top to match TextEdit internal padding
                                                                    ui.add_space(2.0);
                                                                    for n in 1..=line_count {
                                                                        let num_str = format!("{:>width$}", n, width = digits);
                                                                        ui.add_sized(
                                                                            egui::vec2(gutter_w, line_h),
                                                                            egui::Label::new(
                                                                                egui::RichText::new(num_str)
                                                                                    .monospace()
                                                                                    .color(theme::active().overlay0),
                                                                            ),
                                                                        );
                                                                    }
                                                                });
                                                                // Separator line
                                                                let sep_rect = ui.allocate_exact_size(
                                                                    egui::vec2(1.0, line_h * line_count as f32 + 4.0),
                                                                    egui::Sense::hover(),
                                                                ).0;
                                                                ui.painter().rect_filled(sep_rect, 0.0, theme::active().surface1);
                                                                ui.add_space(theme::SP_SM);
                                                                // Editor
                                                                ui.add(
                                                                    egui::TextEdit::multiline(text)
                                                                        .font(egui::TextStyle::Monospace)
                                                                        .desired_width(f32::INFINITY)
                                                                        .frame(false),
                                                                );
                                                            });
                                                        });
                                                }
                                            }
                                            if ui.input(|inp| inp.modifiers.ctrl && inp.key_pressed(egui::Key::S)) {
                                                rctx.editor_saves.push(pane_id);
                                            }
                                            } // end else (supported file)
                                        }
                                        PaneContent::FileDiff(d) => {
                                            ui.painter().rect_filled(ui.max_rect(), 0.0, theme::active().bg_term);
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    egui::RichText::new(format!("⇄ {}", d.path.display()))
                                                        .strong()
                                                        .size(13.0)
                                                        .color(theme::active().git_filename),
                                                );
                                            });
                                            ui.separator();
                                            egui::ScrollArea::both()
                                                .id_source(("diff_scroll", pane_id))
                                                .auto_shrink([false; 2])
                                                .show(ui, |ui| {
                                                    render_inline_diff(ui, &d.diff_content);
                                                });
                                        }
                                    }
                                });

                                // Click to focus pane
                                if ui.ctx().input(|inp| inp.pointer.button_clicked(egui::PointerButton::Primary)) {
                                    if let Some(pos) = ui.ctx().input(|inp| inp.pointer.interact_pos()) {
                                        if rect.contains(pos) {
                                            *rctx.clicked_pane_id = Some(pane_id);
                                            // Release focus from any other widget (e.g. the
                                            // notes TextEdit) so the terminal can take keyboard
                                            // focus on the next frame.
                                            if let Some(fid) = ui.ctx().memory(|m| m.focused()) {
                                                ui.ctx().memory_mut(|m| m.surrender_focus(fid));
                                            }
                                        }
                                    }
                                }
                            }
                            PaneNode::Split { split_id, dir, ratio, a, b } => {
                                let (rect_a, div_rect, rect_b) = split_rect(rect, *dir, *ratio);
                                render_node(ui, a, rect_a, rctx);
                                render_node(ui, b, rect_b, rctx);

                                // Draw divider
                                let div_id = egui::Id::new(("split_div", *split_id));
                                let div_resp = ui.interact(div_rect, div_id, egui::Sense::drag());
                                let div_color = if div_resp.dragged() || div_resp.hovered() {
                                    theme::active().divider_active
                                } else {
                                    theme::active().divider_idle
                                };
                                ui.painter().rect_filled(div_rect, theme::STROKE_THIN, div_color);

                                // Handle drag to resize
                                if div_resp.dragged() {
                                    let delta = div_resp.drag_delta();
                                    let movement = match dir {
                                        SplitDir::Horizontal => delta.x / rect.width(),
                                        SplitDir::Vertical   => delta.y / rect.height(),
                                    };
                                    let new_ratio = (*ratio + movement).clamp(0.1, 0.9);
                                    rctx.split_ratio_changes.push((*split_id, new_ratio));
                                }

                                // Cursor feedback
                                let cursor = match dir {
                                    SplitDir::Horizontal => egui::CursorIcon::ResizeHorizontal,
                                    SplitDir::Vertical   => egui::CursorIcon::ResizeVertical,
                                };
                                if div_resp.hovered() || div_resp.dragged() {
                                    ui.ctx().set_cursor_icon(cursor);
                                }
                            }
                        }
                    }

                    let mut rctx = RenderCtx {
                        sessions: &self.sessions,
                        panes: &self.panes,
                        editor_texts: &mut editor_texts,
                        cursor_blink_on: self.cursor_blink_on,
                        focused_pane_id: active_pane_id_snap,
                        active_term_geo: &mut self.active_term_geo,
                        active_term_ui_id: &mut self.active_term_ui_id,
                        clicked_pane_id: &mut clicked_pane_id,
                        editor_saves: &mut editor_saves,
                        editor_preview_toggles: &mut editor_preview_toggles,
                        pane_widths_snap: &mut pane_widths_snap,
                        split_ratio_changes: &mut split_ratio_changes,
                        term_selection: &self.term_selection,
                        term_selection_sid: self.term_selection_sid,
                        workspace_dialog_open: self.workspace_dialog.is_some(),
                        workspace_edit_dialog_open: self.workspace_edit_dialog.is_some(),
                        show_settings: self.show_settings,
                        font_size: self.settings.font_size,
                        cursor_style: self.settings.cursor_style,
                    };
                    render_node(ui, &tree, content_rect, &mut rctx);
                }

            // ── URL detection + search overlay ────────────────────────────
            if let (Some(ref geo), Some(sid)) = (&self.active_term_geo, self.active_id) {
                if let Some(entry) = self.sessions.iter().find(|e| e.id == sid) {
                    {
                        use alacritty_terminal::grid::Dimensions;
                        use alacritty_terminal::index::{Column, Line};
                        use alacritty_terminal::term::cell::Flags;
                        let session = entry.session.read();
                        let term = &session.term;
                        let grid = term.grid();
                        let cols = term.columns();
                        let display_offset = grid.display_offset();
                        let visible_rows = (geo.rect.height() / geo.cell_h) as usize;
                        let history = grid.history_size() as i32;
                        let term_rows = term.screen_lines() as i32;
                        let mut lines = Vec::with_capacity(visible_rows);
                        for screen_row in 0..visible_rows {
                            let grid_line = screen_row as i32 - display_offset as i32;
                            let mut text = String::with_capacity(cols);
                            if grid_line >= -history && grid_line < term_rows {
                                for col in 0..cols {
                                    let cell = &grid[Line(grid_line)][Column(col)];
                                    if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                                        continue;
                                    }
                                    text.push(cell.c);
                                }
                            }
                            lines.push((grid_line, text));
                        }
                        drop(session);
                        self.detected_urls = crate::url_detector::detect_urls(&lines);
                    }

                    if !self.detected_urls.is_empty() {
                        let t = theme::active();
                        let painter = ui.painter();
                        let session = entry.session.read();
                        let display_offset = session.term.grid().display_offset();
                        let visible_rows = (geo.rect.height() / geo.cell_h) as usize;
                        drop(session);
                        for detected in &self.detected_urls {
                            let screen_row = detected.line + display_offset as i32;
                            if screen_row < 0 || screen_row >= visible_rows as i32 {
                                continue;
                            }
                            let y = geo.rect.min.y + screen_row as f32 * geo.cell_h + geo.cell_h - 1.5;
                            let x0 = geo.rect.min.x + detected.start_col as f32 * geo.cell_w;
                            let x1 = geo.rect.min.x + detected.end_col as f32 * geo.cell_w;
                            painter.line_segment(
                                [egui::pos2(x0, y), egui::pos2(x1, y)],
                                egui::Stroke::new(0.5, t.overlay0),
                            );
                        }

                        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                        let ctrl_held = ui.input(|i| i.modifiers.ctrl && !i.modifiers.shift);
                        if ctrl_held {
                            if let Some(pos) = pointer_pos {
                                if let Some((col, row)) = geo.to_cell(pos) {
                                    let session = entry.session.read();
                                    let d_off = session.term.grid().display_offset();
                                    drop(session);
                                    let grid_line = row as i32 - d_off as i32;
                                    if let Some(det) = self.detected_urls.iter().find(|u| u.line == grid_line && (col as usize) >= u.start_col && (col as usize) < u.end_col) {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                        let sy = geo.rect.min.y + row as f32 * geo.cell_h + geo.cell_h - 1.5;
                                        let sx0 = geo.rect.min.x + det.start_col as f32 * geo.cell_w;
                                        let sx1 = geo.rect.min.x + det.end_col as f32 * geo.cell_w;
                                        ui.painter().line_segment(
                                            [egui::pos2(sx0, sy), egui::pos2(sx1, sy)],
                                            egui::Stroke::new(1.5, t.blue),
                                        );
                                    }
                                }
                            }
                        }

                        let clicked = ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Primary));
                        if ctrl_held && clicked {
                            if let Some(pos) = pointer_pos {
                                if let Some((col, row)) = geo.to_cell(pos) {
                                    let session = entry.session.read();
                                    let d_off = session.term.grid().display_offset();
                                    drop(session);
                                    let grid_line = row as i32 - d_off as i32;
                                    if let Some(url) = crate::url_detector::url_at_position(&self.detected_urls, grid_line, col as usize) {
                                        let _ = open::that(url);
                                    }
                                }
                            }
                        }
                    }

                    if self.term_search.active && !self.term_search.matches.is_empty() {
                        let session = entry.session.read();
                        let display_offset = session.term.grid().display_offset();
                        let visible_rows = (geo.rect.height() / geo.cell_h) as usize;
                        drop(session);
                        let painter = ui.painter();
                        for (i, m) in self.term_search.matches.iter().enumerate() {
                            let screen_row = m.line + display_offset as i32;
                            if screen_row < 0 || screen_row >= visible_rows as i32 {
                                continue;
                            }
                            let y = geo.rect.min.y + screen_row as f32 * geo.cell_h;
                            let x0 = geo.rect.min.x + m.start_col as f32 * geo.cell_w;
                            let x1 = geo.rect.min.x + m.end_col as f32 * geo.cell_w;
                            let color = if Some(i) == self.term_search.current_index {
                                egui::Color32::from_rgba_unmultiplied(255, 165, 0, 100)
                            } else {
                                egui::Color32::from_rgba_unmultiplied(255, 255, 0, 50)
                            };
                            painter.rect_filled(
                                egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + geo.cell_h)),
                                0.0,
                                color,
                            );
                        }
                    }

                    if self.term_search.active {
                        let t = theme::active();
                        let bar_w = 320.0_f32;
                        let bar_h = 30.0_f32;
                        let bar_rect = egui::Rect::from_min_size(
                            egui::pos2(geo.rect.max.x - bar_w - 8.0, geo.rect.min.y + 8.0),
                            egui::vec2(bar_w, bar_h),
                        );
                        ui.painter().rect_filled(bar_rect, 4.0, t.surface0);
                        ui.painter().rect_stroke(bar_rect, 4.0, egui::Stroke::new(1.0, t.overlay0));

                        let input_rect = egui::Rect::from_min_max(
                            egui::pos2(bar_rect.min.x + 6.0, bar_rect.min.y + 4.0),
                            egui::pos2(bar_rect.max.x - 90.0, bar_rect.max.y - 4.0),
                        );
                        let resp = ui.put(
                            input_rect,
                            egui::TextEdit::singleline(&mut self.term_search.query)
                                .desired_width(input_rect.width())
                                .font(egui::FontId::monospace(12.0))
                                .hint_text("Search\u{2026}"),
                        );
                        if resp.changed() {
                            self.term_search.search(&entry.session);
                        }
                        resp.request_focus();

                        let navigated = if ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.shift) {
                            self.term_search.prev_match();
                            true
                        } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            self.term_search.next_match();
                            true
                        } else {
                            false
                        };
                        if navigated || resp.changed() {
                            if let Some(m) = self.term_search.current_match() {
                                let match_line = m.line;
                                let mut session = entry.session.write();
                                let screen_lines = session.term.screen_lines() as i32;
                                let display_offset = session.term.grid().display_offset() as i32;
                                let top_line = -(display_offset);
                                let bottom_line = top_line + screen_lines - 1;
                                if match_line < top_line || match_line > bottom_line {
                                    let target_offset = -(match_line - screen_lines / 2);
                                    let delta = target_offset - display_offset;
                                    session.term.scroll_display(Scroll::Delta(delta));
                                }
                            }
                        }

                        let count_text = if self.term_search.matches.is_empty() {
                            if self.term_search.query.is_empty() { String::new() } else { "0/0".to_string() }
                        } else {
                            format!("{}/{}", self.term_search.current_index.unwrap_or(0) + 1, self.term_search.matches.len())
                        };
                        ui.painter().text(
                            egui::pos2(bar_rect.max.x - 48.0, bar_rect.center().y),
                            egui::Align2::CENTER_CENTER,
                            &count_text,
                            egui::FontId::monospace(11.0),
                            t.subtext0,
                        );
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
                || self.show_settings
                || self.show_shortcut_help
                || self.show_quick_switcher;

            // Global shortcuts that work even when a modal/dialog is open
            {
                let consumed = ctx.input_mut(|i| {
                    let cs = egui::Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: false };
                    if i.consume_key(cs, egui::Key::Slash) || i.consume_key(cs, egui::Key::Questionmark) {
                        Some(AppAction::ToggleShortcutHelp)
                    } else if i.consume_key(cs, egui::Key::Comma) {
                        Some(AppAction::OpenSettings)
                    } else if i.consume_key(cs, egui::Key::F) {
                        Some(AppAction::FocusSessionSearch)
                    } else if i.consume_key(cs, egui::Key::P) {
                        Some(AppAction::FocusFileSearch)
                    } else if i.consume_key(cs, egui::Key::D) {
                        Some(AppAction::RightTabDirectory)
                    } else if i.consume_key(cs, egui::Key::Space) {
                        Some(AppAction::OpenQuickSwitcher)
                    } else if i.consume_key(cs, egui::Key::N) {
                        Some(AppAction::SearchAllSessions)
                    } else if i.consume_key(egui::Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: false }, egui::Key::F) {
                        Some(AppAction::SearchTerminal)
                    } else if (self.show_shortcut_help || self.term_search.active || self.show_global_search) && i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                        if self.show_global_search {
                            Some(AppAction::SearchAllSessions)
                        } else if self.term_search.active {
                            Some(AppAction::SearchTerminal)
                        } else {
                            Some(AppAction::ToggleShortcutHelp)
                        }
                    } else {
                        None
                    }
                });
                match consumed {
                    Some(AppAction::ToggleShortcutHelp) => self.show_shortcut_help = !self.show_shortcut_help,
                    Some(AppAction::OpenQuickSwitcher) => {
                        self.show_quick_switcher = !self.show_quick_switcher;
                        if !self.show_quick_switcher {
                            self.quick_switcher_query.clear();
                        }
                    }
                    Some(AppAction::OpenSettings) => {
                        self.show_shortcut_help = false;
                        self.show_settings = !self.show_settings;
                    }
                    Some(AppAction::FocusSessionSearch) => {
                        self.show_left_panel = true;
                        self.session_search_active = !self.session_search_active;
                        if !self.session_search_active {
                            self.session_search_query.clear();
                        }
                    }
                    Some(AppAction::FocusFileSearch) | Some(AppAction::RightTabDirectory) => {
                        self.show_right_panel = true;
                        self.right_tab = RightTab::Directory;
                        self.dir_search_active = !self.dir_search_active;
                        if !self.dir_search_active {
                            self.dir_search_query.clear();
                            self.dir_search_debounce_query.clear();
                            self.dir_search_debounce_at = None;
                            self.file_search_worker.cancel();
                        }
                    }
                    Some(AppAction::SearchTerminal) => {
                        self.term_search.active = !self.term_search.active;
                        if !self.term_search.active {
                            self.term_search.query.clear();
                            self.term_search.matches.clear();
                            self.term_search.current_index = None;
                        }
                    }
                    Some(AppAction::SearchAllSessions) => {
                        self.show_global_search = !self.show_global_search;
                        if self.show_global_search {
                            self.show_left_panel = true;
                            self.session_search_active = false;
                            self.session_search_query.clear();
                        } else {
                            self.global_search_query.clear();
                            self.global_search_debounce_at = None;
                            self.global_search_selected = 0;
                            self.search_worker.cancel();
                        }
                    }
                    _ => {}
                }
            }

            // Tab must be consumed from egui *outside* the any_other_widget_focused guard.
            // If Tab cycles egui focus away, the guard becomes true and the consume never
            // runs — a permanent deadlock. Consuming unconditionally here prevents egui from
            // ever using Tab for focus traversal while a terminal pane is active.
            if !active_is_editor && !modal_open {
                let (tab_fwd, tab_rev) = ctx.input_mut(|i| (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                    i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab),
                ));
                if tab_fwd || tab_rev {
                    if let Some(sid) = active_session_id {
                        self.scroll_accum.remove(&sid);
                        if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                            self.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                            let bytes = if tab_fwd { b"\t".to_vec() } else { b"\x1b[Z".to_vec() };
                            let _ = self.sessions[idx].pty_tx.send(bytes);
                        }
                    }
                }
            }

            if !active_is_editor && !any_other_widget_focused && !modal_open {

                // Focus-in / focus-out events (?1004h)
                if active_session_id != self.last_focused_sid {
                    // Send focus-out to the session we just left
                    if let Some(old_sid) = self.last_focused_sid {
                        if let Some(idx) = self.sessions.iter().position(|e| e.id == old_sid) {
                            let tracking = self.sessions[idx].session.read().term.mode().contains(TermMode::FOCUS_IN_OUT);
                            if tracking {
                                let _ = self.sessions[idx].pty_tx.send(b"\x1b[O".to_vec());
                            }
                        }
                    }
                    // Send focus-in to the newly active session
                    if let Some(sid) = active_session_id {
                        if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                            let tracking = self.sessions[idx].session.read().term.mode().contains(TermMode::FOCUS_IN_OUT);
                            if tracking {
                                let _ = self.sessions[idx].pty_tx.send(b"\x1b[I".to_vec());
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
                                self.term_selection = None;
                                self.term_selection_sid = None;
                                self.scroll_accum.remove(&sid);
                                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                    self.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                    let _ = self.sessions[idx].pty_tx.send(text.as_bytes().to_vec());
                                }
                            }
                        }
                        egui::Event::Key { key, pressed: true, modifiers, .. } => {
                            if let Some(action) = self.shortcut_registry.match_event(key, modifiers) {
                                match action {
                                    AppAction::SplitHorizontal => split_request = Some(SplitDir::Horizontal),
                                    AppAction::SplitVertical => split_request = Some(SplitDir::Vertical),
                                    AppAction::CloseCurrentPane => close_split_pane = true,
                                    AppAction::ToggleLeftSidebar => self.show_left_panel = !self.show_left_panel,
                                    AppAction::ToggleRightSidebar => self.show_right_panel = !self.show_right_panel,
                                    AppAction::FocusTerminal => {
                                        ctx.memory_mut(|m| m.surrender_focus(egui::Id::NULL));
                                    }
                                    AppAction::NewTerminalTab => {
                                        self.deferred_spawn = Some(self.configured_shell());
                                    }
                                    AppAction::OpenSettings => { /* handled in global shortcuts block above */ }
                                                    AppAction::ToggleShortcutHelp => { /* handled in global shortcuts block above */ }
                                                    AppAction::OpenQuickSwitcher => { /* handled in global shortcuts block above */ }
                                    AppAction::CopySelection => {
                                        if let Some(sid) = active_session_id {
                                            if self.term_selection.is_some() && self.term_selection_sid == Some(sid) {
                                                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                                    let text = self.extract_selected_text(idx);
                                                    if !text.is_empty() {
                                                        ctx.output_mut(|o| o.copied_text = text);
                                                    }
                                                }
                                                self.term_selection = None;
                                                self.term_selection_sid = None;
                                            }
                                        }
                                    }
                                    AppAction::DuplicateSession => self.deferred_duplicate = true,
                                    AppAction::FocusSessionSearch => {
                                        self.show_left_panel = true;
                                        self.session_search_active = !self.session_search_active;
                                        if !self.session_search_active {
                                            self.session_search_query.clear();
                                        }
                                    }
                                    AppAction::FocusFileSearch => {
                                        self.show_right_panel = true;
                                        self.right_tab = RightTab::Directory;
                                        self.dir_search_active = !self.dir_search_active;
                                        if !self.dir_search_active {
                                            self.dir_search_query.clear();
                                            self.dir_search_debounce_query.clear();
                                            self.dir_search_debounce_at = None;
                                            self.file_search_worker.cancel();
                                        }
                                    }
                                    AppAction::PreviousTab => {
                                        if nv > 1 {
                                            let cur = self.active_pane_id.and_then(|pid| visible_indices.iter().position(|&i| self.panes[i].id == pid)).unwrap_or(0);
                                            let prev = if cur == 0 { nv - 1 } else { cur - 1 };
                                            clicked_pane_id = Some(self.panes[visible_indices[prev]].id);
                                        }
                                    }
                                    AppAction::NextTab => {
                                        if nv > 1 {
                                            let cur = self.active_pane_id.and_then(|pid| visible_indices.iter().position(|&i| self.panes[i].id == pid)).unwrap_or(0);
                                            let next = (cur + 1) % nv;
                                            clicked_pane_id = Some(self.panes[visible_indices[next]].id);
                                        }
                                    }
                                    AppAction::NextWorkspace => {
                                        let ws_ids: Vec<u64> = self.workspace_store.workspaces.iter().map(|w| w.id).collect();
                                        if !ws_ids.is_empty() {
                                            let cur = self.active_group.and_then(|g| ws_ids.iter().position(|&id| id == g)).unwrap_or(0);
                                            let next = (cur + 1) % ws_ids.len();
                                            self.deferred_open_workspace = Some(ws_ids[next]);
                                        }
                                    }
                                    AppAction::PrevWorkspace => {
                                        let ws_ids: Vec<u64> = self.workspace_store.workspaces.iter().map(|w| w.id).collect();
                                        if !ws_ids.is_empty() {
                                            let cur = self.active_group.and_then(|g| ws_ids.iter().position(|&id| id == g)).unwrap_or(0);
                                            let prev = if cur == 0 { ws_ids.len() - 1 } else { cur - 1 };
                                            self.deferred_open_workspace = Some(ws_ids[prev]);
                                        }
                                    }
                                    AppAction::RightTabDirectory => {
                                        self.show_right_panel = true;
                                        self.right_tab = RightTab::Directory;
                                        self.dir_search_active = !self.dir_search_active;
                                        if !self.dir_search_active {
                                            self.dir_search_query.clear();
                                            self.dir_search_debounce_query.clear();
                                            self.dir_search_debounce_at = None;
                                            self.file_search_worker.cancel();
                                        }
                                    }
                                    AppAction::RightTabGitDiff => {
                                        self.show_right_panel = true;
                                        self.right_tab = RightTab::GitDiff;
                                    }
                                    AppAction::ToggleNotes => {
                                        self.notes_panel_collapsed = !self.notes_panel_collapsed;
                                    }
                                    _ => {
                                        if let Some(tab_idx) = action.tab_index() {
                                            if tab_idx < nv {
                                                clicked_pane_id = Some(self.panes[visible_indices[tab_idx]].id);
                                            }
                                        }
                                    }
                                }
                            }
                            // Alt+Arrow → move focus between split panes
                            else if modifiers.alt && !modifiers.ctrl && !modifiers.shift {
                                let dir_opt = match key {
                                    egui::Key::ArrowLeft  => Some(SplitDir::Horizontal),
                                    egui::Key::ArrowRight => Some(SplitDir::Horizontal),
                                    egui::Key::ArrowUp    => Some(SplitDir::Vertical),
                                    egui::Key::ArrowDown  => Some(SplitDir::Vertical),
                                    _ => None,
                                };
                                if let Some(_dir) = dir_opt {
                                    if let Some(active_pid) = active_pane_id_snap {
                                        let root_pid_opt = self.pane_trees.iter()
                                            .find(|(_, tree)| tree.leaf_ids().contains(&active_pid))
                                            .map(|(&rpid, _)| rpid);
                                        if let Some(root_pid) = root_pid_opt {
                                            if let Some(tree) = self.pane_trees.get(&root_pid) {
                                                let leaves = tree.leaf_ids();
                                                if leaves.len() > 1 {
                                                    if let Some(pos) = leaves.iter().position(|&id| id == active_pid) {
                                                        let next = match key {
                                                            egui::Key::ArrowRight | egui::Key::ArrowDown => {
                                                                leaves[(pos + 1) % leaves.len()]
                                                            }
                                                            _ => {
                                                                leaves[(pos + leaves.len() - 1) % leaves.len()]
                                                            }
                                                        };
                                                        clicked_pane_id = Some(next);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else if let Some(bytes) = key_to_pty_bytes(key, modifiers) {
                                    if let Some(sid) = active_session_id {
                                        self.scroll_accum.remove(&sid);
                                        if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                            self.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                            let _ = self.sessions[idx].pty_tx.send(bytes.to_vec());
                                        }
                                    }
                                }
                            } else if let Some(bytes) = key_to_pty_bytes(key, modifiers) {
                                if let Some(sid) = active_session_id {
                                    self.scroll_accum.remove(&sid);
                                    if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                        self.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                        let _ = self.sessions[idx].pty_tx.send(bytes.to_vec());
                                    }
                                }
                            }
                        }
                        // egui-winit converts Ctrl+C to Event::Copy before emitting Event::Key,
                        // so we must handle Copy here. If there's a text selection, copy it
                        // to clipboard; otherwise send SIGINT (^C) to the PTY.
                        egui::Event::Copy => {
                            if let Some(sid) = active_session_id {
                                if self.term_selection.is_some() && self.term_selection_sid == Some(sid) {
                                    if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                        let text = self.extract_selected_text(idx);
                                        if !text.is_empty() {
                                            ctx.output_mut(|o| o.copied_text = text);
                                        }
                                    }
                                    self.term_selection = None;
                                    self.term_selection_sid = None;
                                } else {
                                    self.scroll_accum.remove(&sid);
                                    if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                        self.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                        let _ = self.sessions[idx].pty_tx.send(vec![3u8]);
                                    }
                                }
                            }
                        }
                        // egui-winit converts Ctrl+V to Event::Paste before emitting Event::Key.
                        // Wrap in bracketed-paste sequences only if the app opted in (?2004h).
                        egui::Event::Paste(text) => {
                            if let Some(sid) = active_session_id {
                                self.scroll_accum.remove(&sid);
                                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                    self.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                    let bp = self.sessions[idx].session.read().term.mode().contains(TermMode::BRACKETED_PASTE);
                                    let data = if bp {
                                        let mut v = b"\x1b[200~".to_vec();
                                        v.extend_from_slice(text.as_bytes());
                                        v.extend_from_slice(b"\x1b[201~");
                                        v
                                    } else {
                                        text.as_bytes().to_vec()
                                    };
                                    let _ = self.sessions[idx].pty_tx.send(data);
                                }
                            }
                        }
                        // Mouse events forwarded when the application has enabled mouse reporting.
                        egui::Event::PointerButton { pos, button, pressed, .. } => {
                            if let Some(sid) = active_session_id {
                                if let Some(idx) = self.sessions.iter().position(|e| e.id == sid) {
                                    let (has_mouse, sgr) = {
                                        let s = self.sessions[idx].session.read();
                                        let mode = s.term.mode();
                                        let has = mode.contains(TermMode::MOUSE_REPORT_CLICK)
                                            || mode.contains(TermMode::MOUSE_DRAG)
                                            || mode.contains(TermMode::MOUSE_MOTION);
                                        (has, mode.contains(TermMode::SGR_MOUSE))
                                    };
                                    if has_mouse {
                                        if let Some(geo) = &self.active_term_geo {
                                            if let Some((col, row)) = geo.to_cell(*pos) {
                                                let btn = match button {
                                                    egui::PointerButton::Primary   => 0u8,
                                                    egui::PointerButton::Middle    => 1,
                                                    egui::PointerButton::Secondary => 2,
                                                    _ => return,
                                                };
                                                let bytes = mouse_event_bytes(btn, col, row, *pressed, sgr);
                                                let _ = self.sessions[idx].pty_tx.send(bytes.to_vec());
                                            }
                                        }
                                    } else if *button == egui::PointerButton::Primary {
                                        if let Some(geo) = &self.active_term_geo {
                                            if let Some((col, row)) = geo.to_cell(*pos) {
                                                if *pressed {
                                                    self.term_selection = Some(TermSelection {
                                                        start_col: col,
                                                        start_row: row,
                                                        end_col: col,
                                                        end_row: row,
                                                    });
                                                    self.term_selecting = true;
                                                    self.term_selection_sid = Some(sid);
                                                } else {
                                                    self.term_selecting = false;
                                                    if let Some(sel) = &self.term_selection {
                                                        if sel.start_col == sel.end_col && sel.start_row == sel.end_row {
                                                            self.term_selection = None;
                                                            self.term_selection_sid = None;
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        egui::Event::PointerMoved(pos) if self.term_selecting => {
                            if let Some(geo) = &self.active_term_geo {
                                let clamped = egui::pos2(
                                    pos.x.clamp(geo.rect.min.x, geo.rect.max.x - 1.0),
                                    pos.y.clamp(geo.rect.min.y, geo.rect.max.y - 1.0),
                                );
                                if let Some((col, row)) = geo.to_cell(clamped) {
                                    if let Some(sel) = &mut self.term_selection {
                                        sel.end_col = col;
                                        sel.end_row = row;
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
                                        let (has_mouse, sgr) = {
                                            let s = self.sessions[idx].session.read();
                                            let mode = s.term.mode();
                                            let has = mode.contains(TermMode::MOUSE_REPORT_CLICK)
                                                || mode.contains(TermMode::MOUSE_DRAG)
                                                || mode.contains(TermMode::MOUSE_MOTION);
                                            (has, mode.contains(TermMode::SGR_MOUSE))
                                        };
                                        if has_mouse {
                                            // App has mouse mode — forward scroll to PTY
                                            if let Some(pos) = mouse_pos {
                                                if let Some(geo) = &self.active_term_geo {
                                                    if let Some((col, row)) = geo.to_cell(pos) {
                                                        // Button 64 = scroll up, 65 = scroll down
                                                        let btn = if delta.y > 0.0 { 64u8 } else { 65 };
                                                        let bytes = mouse_event_bytes(btn, col, row, true, sgr);
                                                        let _ = self.sessions[idx].pty_tx.send(bytes.to_vec());
                                                    }
                                                }
                                            }
                                        } else {
                                            // No mouse mode — scroll alacritty's internal scrollback.
                                            // Accumulate in fractional lines. Convert delta.y based on
                                            // its unit: Point=pixels (divide by cell height), Line=already
                                            // in lines, Page=multiply by visible rows.
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
                                                // Positive direction = scroll up (positive Delta)
                                                let scroll_delta = if direction > 0.0 {
                                                    lines as i32
                                                } else {
                                                    -(lines as i32)
                                                };
                                                self.sessions[idx].session.write().term.scroll_display(Scroll::Delta(scroll_delta));
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

        // Phase D-0: Apply split divider ratio changes from drag
        for (split_id_changed, new_ratio) in split_ratio_changes {
            for tree in self.pane_trees.values_mut() {
                if let Some(ratio) = tree.find_split_ratio_mut(split_id_changed) {
                    *ratio = new_ratio;
                    break;
                }
            }
        }

        // Phase D-1: Handle split request (Ctrl+Shift+\ or Ctrl+Shift+-)
        if let Some(dir) = split_request {
            if let Some(active_pid) = self.active_pane_id {
                // Find the root pane that contains the active pane
                let root_pid_opt = self
                    .pane_trees
                    .iter()
                    .find(|(_, tree)| tree.leaf_ids().contains(&active_pid))
                    .map(|(&rpid, _)| rpid);
                if let Some(root_pid) = root_pid_opt {
                    // Get current size for the new pane
                    let (cols, rows) = self
                        .panes
                        .iter()
                        .find(|p| p.id == active_pid)
                        .map(|p| p.last_size)
                        .unwrap_or((80, 24));
                    // Get cwd and shell from active session
                    let (cwd, shell) = {
                        let active_session_entry = self
                            .panes
                            .iter()
                            .find(|p| p.id == active_pid)
                            .and_then(|p| {
                                if let PaneContent::Terminal(sid) = &p.content {
                                    Some(*sid)
                                } else {
                                    None
                                }
                            })
                            .and_then(|sid| self.sessions.iter().find(|e| e.id == sid));
                        let cwd = active_session_entry
                            .map(|e| e.session.read().cwd.clone())
                            .filter(|p| !p.as_os_str().is_empty());
                        let shell = active_session_entry
                            .map(|e| e.shell.clone())
                            .unwrap_or_else(default_shell);
                        (cwd, shell)
                    };
                    // Spawn a new session (no pane entry yet — leaf only in tree)
                    if let Some(new_sid) = self.spawn_session_no_pane(&shell, cols, rows, cwd) {
                        let new_pane_id = self.next_pane_id;
                        self.next_pane_id += 1;
                        let split_id = self.next_split_id;
                        self.next_split_id += 1;
                        // Add pane entry (NOT a root pane, so no pane_trees entry)
                        self.panes.push(PaneEntry {
                            id: new_pane_id,
                            content: PaneContent::Terminal(new_sid),
                            manual_width: None,
                            last_size: (cols, rows),
                        });
                        // Modify the tree to split the active leaf
                        if let Some(tree) = self.pane_trees.get_mut(&root_pid) {
                            tree.split_pane(active_pid, new_pane_id, split_id, dir);
                        }
                        // Focus the new pane
                        self.active_pane_id = Some(new_pane_id);
                        self.active_id = Some(new_sid);
                        self.update_is_active_flags();
                        ctx.request_repaint();
                    }
                }
            }
        }

        // Phase D-2: Handle Ctrl+Shift+W — close the focused split pane
        if close_split_pane {
            if let Some(active_pid) = self.active_pane_id {
                // Find the root that contains the active pane
                let root_pid_opt = self
                    .pane_trees
                    .iter()
                    .find(|(_, tree)| tree.leaf_ids().contains(&active_pid))
                    .map(|(&rpid, _)| rpid);
                if let Some(root_pid) = root_pid_opt {
                    let is_root_itself = root_pid == active_pid;
                    // Check if tree has only one leaf (the root itself)
                    let leaf_count = self
                        .pane_trees
                        .get(&root_pid)
                        .map(|t| t.leaf_ids().len())
                        .unwrap_or(1);
                    if is_root_itself || leaf_count <= 1 {
                        // Closing the only pane in a tab — close the whole tab via close_pane_id
                        // (handled above already; if close_pane_id was not set, set it now)
                        // We set close_pane_id to None earlier so we'll handle directly:
                        if leaf_count <= 1 {
                            // Kill session if terminal
                            if let Some(pos) = self.panes.iter().position(|p| p.id == active_pid) {
                                if let PaneContent::Terminal(sid) = self.panes[pos].content {
                                    self.uninit_sessions.remove(&sid);
                                    self.sessions.retain(|e| e.id != sid);
                                    if self.active_id == Some(sid) {
                                        self.active_id = self.sessions.first().map(|e| e.id);
                                        self.update_is_active_flags();
                                    }
                                }
                                self.panes.remove(pos);
                            }
                            self.pane_trees.remove(&root_pid);
                            self.active_pane_id = self.panes.last().map(|p| p.id);
                            self.save_session();
                        }
                    } else {
                        // Remove the leaf from the tree, collapsing the parent split
                        let remove_result = if let Some(tree) = self.pane_trees.get_mut(&root_pid) {
                            tree.remove_pane(active_pid)
                        } else {
                            RemoveResult::NotFound
                        };
                        if let RemoveResult::CollapseToSibling(replacement) = remove_result {
                            if let Some(tree) = self.pane_trees.get_mut(&root_pid) {
                                *tree = replacement;
                            }
                        }
                        // Kill the session of the removed pane
                        if let Some(pos) = self.panes.iter().position(|p| p.id == active_pid) {
                            if let PaneContent::Terminal(sid) = self.panes[pos].content {
                                self.uninit_sessions.remove(&sid);
                                self.sessions.retain(|e| e.id != sid);
                                if self.active_id == Some(sid) {
                                    self.active_id = self.sessions.first().map(|e| e.id);
                                    self.update_is_active_flags();
                                }
                            }
                            self.panes.remove(pos);
                        }
                        // Focus sibling — pick the first leaf of the root tree
                        if let Some(tree) = self.pane_trees.get(&root_pid) {
                            let leaves = tree.leaf_ids();
                            self.active_pane_id = leaves.first().copied();
                            if let Some(new_pid) = self.active_pane_id {
                                if let Some(pane) = self.panes.iter().find(|p| p.id == new_pid) {
                                    if let PaneContent::Terminal(sid) = pane.content {
                                        self.active_id = Some(sid);
                                        self.update_is_active_flags();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 1. Divider drags → freeze manual widths on both adjacent panes
        for (left_idx, right_idx, delta_x, left_w, right_w) in divider_drags {
            self.panes[left_idx].manual_width = Some((left_w + delta_x).max(theme::MIN_PANE_W));
            self.panes[right_idx].manual_width = Some((right_w - delta_x).max(theme::MIN_PANE_W));
        }

        // 2. Close pane (tab-strip close — kills the entire split tree for that root)
        if let Some(pid) = close_pane_id {
            // Collect all pane IDs in this root's split tree so we can kill them all.
            let tree_ids: Vec<u32> = self
                .pane_trees
                .get(&pid)
                .map(|t| t.leaf_ids())
                .unwrap_or_else(|| vec![pid]);
            // Kill every session belonging to any leaf of this tree.
            for leaf_pid in &tree_ids {
                if let Some(pos) = self.panes.iter().position(|p| p.id == *leaf_pid) {
                    if let PaneContent::Terminal(sid) = self.panes[pos].content {
                        self.uninit_sessions.remove(&sid);
                        self.sessions.retain(|e| e.id != sid);
                        if self.active_id == Some(sid) {
                            self.active_id = self.sessions.first().map(|e| e.id);
                            self.update_is_active_flags();
                        }
                    }
                }
            }
            // Remove all leaf panes from the panes vec.
            self.panes.retain(|p| !tree_ids.contains(&p.id));
            editor_texts.retain(|(id, _)| !tree_ids.contains(id));
            // Remove the root's tree entry.
            self.pane_trees.remove(&pid);
            if self
                .active_pane_id
                .map(|ap| tree_ids.contains(&ap))
                .unwrap_or(false)
            {
                self.active_pane_id = self.panes.last().map(|p| p.id);
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
                if matches!(
                    &self.panes[pane_idx].content,
                    PaneContent::DeferredTerminal { .. }
                ) {
                    let (cwd, pending_command) = if let PaneContent::DeferredTerminal {
                        cwd,
                        pending_command,
                    } = &self.panes[pane_idx].content
                    {
                        (cwd.clone(), pending_command.clone())
                    } else {
                        unreachable!()
                    };
                    let shell = self.configured_shell();
                    if let Some(sid) = self.spawn_session_no_pane(&shell, 80, 24, cwd) {
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

        for toggle_id in &editor_preview_toggles {
            if let Some(p) = self.panes.iter_mut().find(|p| p.id == *toggle_id) {
                if let PaneContent::FileEditor(ref mut ed) = p.content {
                    ed.show_preview = !ed.show_preview;
                    self.md_prefer_preview = ed.show_preview;
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
                    let entry = &self.sessions[idx];
                    let mut sess = entry.session.write();
                    SessionManager::resize(&entry.master, cols, rows);
                    sess.resize(cols, rows);
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
                let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");
                self.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::FileEditor(FileEditorState {
                        path,
                        content,
                        dirty: false,
                        save_error: false,
                        workspace_id: self.active_group,
                        show_preview: is_md && self.md_prefer_preview,
                    }),
                    manual_width: None,
                    last_size: (0, 0),
                });
                self.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (0, 0),
                    },
                );
                self.activate_pane(pane_id);
            }
        }

        // 9b. Git diff file double-clicked → open FileDiff pane
        if let Some(rel_path) = git_open_diff_file {
            if let Some(cwd) = self.active_cwd() {
                let full_path = cwd.join(&rel_path);
                let existing_id = self
                    .panes
                    .iter()
                    .find(|p| matches!(&p.content, PaneContent::FileDiff(d) if d.path == full_path))
                    .map(|p| p.id);
                if let Some(pid) = existing_id {
                    self.activate_pane(pid);
                } else {
                    use std::process::Command;
                    let diff_output = Command::new("git")
                        .args(["diff", "HEAD", "--", &rel_path])
                        .current_dir(&cwd)
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                        .unwrap_or_default();
                    let pane_id = self.next_pane_id;
                    self.next_pane_id += 1;
                    self.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::FileDiff(FileDiffState {
                            path: full_path,
                            diff_content: diff_output,
                        }),
                        manual_width: None,
                        last_size: (0, 0),
                    });
                    self.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (0, 0),
                        },
                    );
                    self.activate_pane(pane_id);
                }
            }
        }

        // 9b2. Git file double-clicked → open file in editor
        if let Some(rel_path) = git_open_file {
            if let Some(cwd) = self.active_cwd() {
                let full_path = cwd.join(&rel_path);
                let existing_id = self
                    .panes
                    .iter()
                    .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == full_path))
                    .map(|p| p.id);
                if let Some(pid) = existing_id {
                    self.activate_pane(pid);
                } else {
                    let content = std::fs::read_to_string(&full_path).unwrap_or_default();
                    let pane_id = self.next_pane_id;
                    self.next_pane_id += 1;
                    let is_md = full_path.extension().and_then(|e| e.to_str()) == Some("md");
                    self.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::FileEditor(FileEditorState {
                            path: full_path,
                            content,
                            dirty: false,
                            save_error: false,
                            workspace_id: self.active_group,
                            show_preview: is_md && self.md_prefer_preview,
                        }),
                        manual_width: None,
                        last_size: (0, 0),
                    });
                    self.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (0, 0),
                        },
                    );
                    self.activate_pane(pane_id);
                }
            }
        }

        // 9c. Folder double-clicked in directory → open new terminal pane at that path
        if let Some(dir_path) = open_terminal_at {
            let pane_id = self.next_pane_id;
            self.next_pane_id += 1;
            self.panes.push(PaneEntry {
                id: pane_id,
                content: PaneContent::DeferredTerminal {
                    cwd: Some(dir_path),
                    pending_command: None,
                },
                manual_width: None,
                last_size: (0, 0),
            });
            self.pane_trees.insert(
                pane_id,
                PaneNode::Leaf {
                    pane_id,
                    last_size: (0, 0),
                },
            );
            self.activate_pane(pane_id);
        }

        self.render_settings_overlay(ctx);

        self.render_quick_switcher(ctx);

        self.render_workspace_save_dialog(ctx);

        self.render_workspace_edit_dialog(ctx);

        self.render_close_all_confirm(ctx);
    }
}
