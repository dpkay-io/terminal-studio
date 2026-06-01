use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::FontId;

use crate::pane_tree::{PaneNode, RemoveResult, SplitDir};
use crate::pty::{default_shell, SessionManager, ShellKind};
use crate::renderer::terminal_pass::TerminalGeometry;
use crate::shortcuts::{AppAction, ShortcutRegistry};
use crate::theme;
use crate::workspace::{NoteStore, WindowId, WorkspaceStore};
use alacritty_terminal::{
    grid::{Dimensions, Scroll},
    term::TermMode,
};

// ── Submodules ───────────────────────────────────────────────────────────────

mod feedback;
mod file_browser;
mod git_diff;
mod git_worker;
mod input;
mod markdown;
mod multi_window;
mod pane;
mod pane_state;
mod persistence;
mod session_state;
pub(crate) mod settings;
mod state;
mod title;
mod ui;
mod watcher;
mod worker_manager;
mod workspace_git_worker;
mod workspace_ui;

// ── Re-imports from submodules ───────────────────────────────────────────────

use file_browser::{render_dir_tree, render_flat_file_list, FileEntry, SubdirCache};
use git_diff::{render_git_diff, GitStageAction};
use input::{key_to_pty_bytes, mouse_event_bytes};
use markdown::render_markdown;
use multi_window::{ExtraWindow, PendingWindowFocus, WindowView};
use pane::{
    FileDiffState, FileEditorState, NoteEditorState, PaneContent, PaneEntry, RightTab,
    TermSelection,
};
use pane_state::PaneState;
use session_state::SessionState;
use settings::AppSettings;

use watcher::WatchState;
use worker_manager::WorkerManager;
use workspace_ui::{OpenFolderDialog, WorkspaceDialog, WorkspaceEditDialog};

#[derive(Clone, Copy, Default)]
enum CloseAllTarget {
    #[default]
    ActiveGroup,
    Group(Option<u64>),
    All,
}

pub struct App {
    session_manager: SessionManager,
    session_state: SessionState,

    pane_state: PaneState,
    right_tab: RightTab,
    shown_md_tabs: HashSet<PathBuf>,
    md_prefer_preview: bool,
    watch_state: Option<WatchState>,

    workspace_store: WorkspaceStore,
    active_group: Option<u64>,
    last_pane_per_group: HashMap<Option<u64>, u32>,
    workspace_dialog: Option<WorkspaceDialog>,
    workspace_edit_dialog: Option<WorkspaceEditDialog>,
    open_folder_dialog: Option<OpenFolderDialog>,

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
    quick_switcher_selected_ws: Option<usize>,
    quick_switcher_search_active: bool,
    show_command_palette: bool,
    command_palette_query: String,
    command_palette_selected: usize,
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
    // Background workers: foreground detection, git, search, sys monitor, update checker.
    workers: WorkerManager,
    // Used to detect when the window gains focus so we can flush stale GPU frames.
    was_focused: bool,
    // Shells available on this system, computed once at startup.
    available_shells: Vec<ShellKind>,
    // Cursor blink phase (toggles every 500 ms)
    cursor_blink_on: bool,
    cursor_blink_last: Instant,

    // Terminal text selection state (per-session)
    term_selection: Option<TermSelection>,
    term_selecting: bool,
    term_selection_sid: Option<u32>,

    // Key events stripped from raw_input by raw_input_hook (Tab, arrows, Escape).
    // Stored here so the terminal input routing can still send them to the PTY.
    raw_intercepted_keys: Vec<egui::Event>,

    // ── Multi-window support ──────────────────────────────────────────────
    /// Extra OS windows opened via "Open in new window" on a workspace.
    extra_windows: Vec<ExtraWindow>,
    /// Counter for generating unique WindowId values.
    next_window_id: u64,
    /// `None` when rendering the main window, `Some(id)` when rendering an extra
    /// viewport. Used by per-window-aware code (e.g. workspace switcher filter).
    current_window_id: Option<WindowId>,

    // ── Per-frame UI caches ───────────────────────────────────────────────
    /// Cached cell dimensions for the central panel font measurement.
    /// Invalidated when font_size changes.
    cached_cell_size: Option<(f32, f32, f32)>,
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
    dir_search_debouncer: crate::app::ui::debounce::Debouncer,

    // Terminal content search (Ctrl+F)
    term_search: crate::search::SearchState,

    // Global search across all sessions (Ctrl+Shift+N)
    show_global_search: bool,
    global_search_query: String,
    global_search_debouncer: crate::app::ui::debounce::Debouncer,
    global_search_selected: usize,

    // Detected URLs in the currently visible terminal content
    detected_urls: Vec<crate::url_detector::DetectedUrl>,

    // Detected markdown file paths in the currently visible terminal content
    detected_md_paths: Vec<crate::md_detector::DetectedMdPath>,
    // Hash of visible lines from last detection pass — skip re-detection when unchanged
    detection_lines_hash: u64,
    // Tracks which md paths were already auto-opened in the right panel to avoid re-triggering
    auto_opened_md: HashSet<PathBuf>,
    // Content cache for terminal-detected MD files, with associated workspace ID
    terminal_md_content: HashMap<PathBuf, (Arc<String>, Option<u64>)>,

    // Tab drag-to-reorder state
    tab_drag_source: Option<usize>,

    // Deferred actions (set by keyboard shortcuts in central panel, consumed by left_panel next frame)
    deferred_spawn: Option<ShellKind>,
    deferred_duplicate: bool,
    deferred_split: Option<crate::pane_tree::SplitDir>,
    deferred_close_pane: bool,
    tab_rename_pane_id: Option<u32>,
    tab_rename_text: String,
    deferred_open_workspace: Option<u64>,

    show_close_all_confirm: bool,
    /// Which group of sessions to close when the close-all dialog is confirmed.
    close_all_target: CloseAllTarget,
    show_quit_confirm: bool,
    quit_confirmed: bool,

    // Workspace filter for the session list: None = All, Some(None) = Other, Some(Some(id)) = specific workspace
    session_workspace_filter: Option<Option<u64>>,

    // Pending cross-window focus request set by sidebar click, processed after viewports render.
    pending_window_focus: Option<PendingWindowFocus>,

    // Maps full file path → pane_id for FileDiff panes awaiting async diff results.
    pending_diff_panes: HashMap<PathBuf, u32>,

    // Async file load results buffer: spawned threads push (pane_id, content) here,
    // drained at the start of each update() frame.
    file_load_results: Arc<parking_lot::Mutex<Vec<(u32, String)>>>,
    // Async md content load results from background threads.
    #[allow(clippy::type_complexity)]
    md_load_results: Arc<parking_lot::Mutex<Vec<(PathBuf, String, Option<u64>)>>>,

    // Flash feedback manager for subtle UI feedback
    flash: feedback::FlashManager,

    // Multi-click tracking for double/triple-click selection
    last_click_time: Instant,
    last_click_cell: (u16, u16),
    click_count: u8,

    // Process completion tracking: session_id → time command started (prompt went non-ready)
    command_start_times: HashMap<u32, Instant>,
    // Sessions with completed long-running commands (badge shown on tab)
    completed_badges: std::collections::HashSet<u32>,

    // Pane zoom: when set, the zoomed pane fills the entire content area
    zoomed_pane_id: Option<u32>,

    // Git commit dialog state
    show_commit_dialog: bool,
    commit_message: String,
    commit_amend: bool,
    commit_dialog_focus_requested: bool,

    // Git push dialog state
    show_push_dialog: bool,
    push_force: bool,
    push_in_progress: bool,
    push_error: Option<String>,

    // Git stage-all confirm dialog state
    show_stage_all_confirm: bool,

    // Position where the terminal context menu was opened (captured on right-click)
    context_menu_pos: Option<egui::Pos2>,
}

impl eframe::App for App {
    /// Strip all navigation keys from raw input before egui's begin_frame
    /// can use them for focus traversal. egui's Focus::begin_frame reads
    /// Tab, Shift+Tab, arrows, and Escape from RawInput to move focus
    /// between widgets — we must remove them at the source so the terminal
    /// gets everything.
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.raw_intercepted_keys.clear();
        let active_is_editor = self
            .pane_state
            .active_pane_id
            .and_then(|pid| self.pane_state.panes.iter().find(|p| p.id == pid))
            .map(|p| {
                matches!(
                    p.content,
                    PaneContent::FileEditor(_) | PaneContent::NoteEditor(_)
                )
            })
            .unwrap_or(false);
        let any_overlay = self.workspace_dialog.is_some()
            || self.workspace_edit_dialog.is_some()
            || self.open_folder_dialog.is_some()
            || self.show_settings
            || self.show_shortcut_help
            || self.show_quick_switcher
            || self.show_command_palette
            || self.term_search.active
            || self.show_global_search
            || self.session_search_active
            || self.dir_search_active
            || self.show_quit_confirm;
        let notes_has_focus = _ctx.memory(|m| {
            m.focused()
                .map(|id| id == self.vp_id("notes_textedit"))
                .unwrap_or(false)
        });
        if active_is_editor || any_overlay || notes_has_focus {
            return;
        }
        let intercepted = &mut self.raw_intercepted_keys;
        raw_input.events.retain(|event| {
            if let egui::Event::Key { key, .. } = event {
                if matches!(
                    key,
                    egui::Key::Tab
                        | egui::Key::ArrowUp
                        | egui::Key::ArrowDown
                        | egui::Key::ArrowLeft
                        | egui::Key::ArrowRight
                        | egui::Key::Escape
                ) {
                    intercepted.push(event.clone());
                    return false;
                }
            }
            true
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_session();
        self.save_windows();
        for entry in &self.session_state.sessions {
            entry
                .alive
                .store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        theme::clear_contrast_cache();

        if ctx.input(|i| i.viewport().close_requested()) && !self.quit_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.show_quit_confirm = true;
        }

        self.flash.tick();
        if self.flash.has_active() {
            ctx.request_repaint();
        }

        // Cursor blink — only schedule repaint when a terminal pane is focused.
        {
            let has_active_terminal = self
                .pane_state
                .active_pane_id
                .and_then(|pid| self.pane_state.panes.iter().find(|p| p.id == pid))
                .map(|p| matches!(p.content, PaneContent::Terminal(_)))
                .unwrap_or(false);
            if self.settings.cursor_blink && has_active_terminal {
                if self.cursor_blink_last.elapsed() >= Duration::from_millis(500) {
                    self.cursor_blink_on = !self.cursor_blink_on;
                    self.cursor_blink_last = Instant::now();
                }
                ctx.request_repaint_after(Duration::from_millis(500));
            } else {
                self.cursor_blink_on = true;
            }
        }

        // Drain async file load results from background threads.
        {
            let mut results = self.file_load_results.lock();
            for (pane_id, content) in results.drain(..) {
                if let Some(pane) = self.pane_state.panes.iter_mut().find(|p| p.id == pane_id) {
                    if let PaneContent::FileEditor(ref mut ed) = pane.content {
                        ed.content = content;
                    }
                }
            }
        }

        // Drain async markdown content loads from background threads.
        {
            let mut results = self.md_load_results.lock();
            for (path, content, ws_id) in results.drain(..) {
                self.terminal_md_content
                    .insert(path.clone(), (Arc::new(content), ws_id));
                self.shown_md_tabs.insert(path.clone());
                self.show_right_panel = true;
                self.right_tab = RightTab::Markdown(path);
            }
        }

        // Send deferred duplicate commands once the shell signals it is at a prompt (OSC 7).
        for entry in &mut self.session_state.sessions {
            if entry.pending_command.is_some() {
                let ready = entry.session.read().prompt_ready;
                if ready {
                    let cmd = entry.pending_command.take().unwrap();
                    log::debug!("PTY[{}] replaying command: {:?}", entry.id, cmd);
                    // Windows ConPTY/PSReadLine executes on \r, not \n.
                    #[cfg(target_os = "windows")]
                    let _ = entry.pty_tx.try_send(format!("{}\r", cmd).into_bytes());
                    #[cfg(not(target_os = "windows"))]
                    let _ = entry.pty_tx.try_send(format!("{}\n", cmd).into_bytes());
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }

        // Track command start/completion for long-running process notifications.
        for entry in &self.session_state.sessions {
            let ready = entry.session.read().prompt_ready;
            let sid = entry.id;
            if ready {
                if let Some(start) = self.command_start_times.remove(&sid) {
                    if start.elapsed() > Duration::from_secs(5) {
                        let is_active = self.session_state.active_id == Some(sid);
                        if !is_active {
                            self.completed_badges.insert(sid);
                        }
                    }
                }
            } else {
                self.command_start_times
                    .entry(sid)
                    .or_insert_with(Instant::now);
            }
        }

        // Check for bell events and trigger visual flash.
        for entry in &self.session_state.sessions {
            if entry
                .session
                .read()
                .bell
                .swap(false, std::sync::atomic::Ordering::Relaxed)
            {
                if let Some(pane) =
                    self.pane_state.panes.iter().find(
                        |p| matches!(&p.content, PaneContent::Terminal(sid) if *sid == entry.id),
                    )
                {
                    self.flash.trigger(
                        feedback::FlashTarget::Pane(pane.id),
                        feedback::FlashKind::Neutral,
                    );
                }
            }
        }

        // Poll quickly while any session is still initializing (CWD not set yet).
        // Only check sessions we know are still uninitialized to avoid per-frame
        // read locks on all sessions in the steady state.
        if !self.session_state.uninit_sessions.is_empty() {
            self.session_state.uninit_sessions.retain(|&id| {
                self.session_state
                    .sessions
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.session.read().cwd.as_os_str().is_empty())
                    .unwrap_or(false)
            });
            if !self.session_state.uninit_sessions.is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_millis(150));
            }
        }

        // ── Sync watchers + process FS events ──────────────────────────────
        if let Some(ws) = &mut self.watch_state {
            // Resync when sessions are added/removed or every 3s to catch CWD changes.
            let session_count = self.session_state.sessions.len();
            let now = Instant::now();
            if session_count != ws.last_session_count
                || now.duration_since(ws.last_sync) >= Duration::from_secs(3)
            {
                ws.sync(&self.session_state.sessions);
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
                self.workers.git_worker.enqueue_git(dir);
                self.workers.git_worker.enqueue_unpushed(dir);
            }

            let watched_dirs: Vec<PathBuf> = self
                .watch_state
                .as_ref()
                .map(|ws| ws.dir_data.keys().cloned().collect())
                .unwrap_or_default();
            let completed: Vec<(PathBuf, (String, String))> = watched_dirs
                .iter()
                .filter_map(|d| self.workers.git_worker.take_git(d).map(|r| (d.clone(), r)))
                .collect();
            let completed_unpushed: Vec<(PathBuf, Vec<(String, String)>)> = watched_dirs
                .iter()
                .filter_map(|d| {
                    self.workers
                        .git_worker
                        .take_unpushed(d)
                        .map(|r| (d.clone(), r))
                })
                .collect();
            if let Some(ws) = &mut self.watch_state {
                for (dir, (diff, status)) in completed {
                    ws.apply_git_result(&dir, diff, status);
                }
                for (dir, commits) in completed_unpushed {
                    ws.apply_unpushed_result(&dir, commits);
                }
            }

            // Drain completed diff results and update pending FileDiff panes
            let diff_results = self.workers.git_worker.take_diff_results();
            for (full_path, diff_output) in diff_results {
                if let Some(pane_id) = self.pending_diff_panes.remove(&full_path) {
                    if let Some(pane) = self.pane_state.panes.iter_mut().find(|p| p.id == pane_id) {
                        if let PaneContent::FileDiff(ref mut d) = pane.content {
                            d.diff_content = diff_output;
                        }
                    }
                }
            }

            // Drain commit/push results and show flash feedback
            for result in self.workers.git_worker.take_commit_results() {
                match result {
                    Ok(cwd) => {
                        self.flash
                            .trigger(feedback::FlashTarget::Global, feedback::FlashKind::Success);
                        self.workers.git_worker.enqueue_git(&cwd);
                        self.workers.git_worker.enqueue_unpushed(&cwd);
                    }
                    Err(msg) => {
                        log::error!("git commit failed: {msg}");
                        self.flash
                            .trigger(feedback::FlashTarget::Global, feedback::FlashKind::Error);
                    }
                }
            }
            for result in self.workers.git_worker.take_push_results() {
                self.push_in_progress = false;
                match result {
                    Ok(cwd) => {
                        self.push_error = None;
                        self.flash
                            .trigger(feedback::FlashTarget::Global, feedback::FlashKind::Success);
                        self.workers.git_worker.enqueue_unpushed(&cwd);
                    }
                    Err(msg) => {
                        log::error!("git push failed: {msg}");
                        self.push_error = Some(msg);
                        self.flash
                            .trigger(feedback::FlashTarget::Global, feedback::FlashKind::Error);
                    }
                }
            }
            for result in self.workers.git_worker.take_gitignore_results() {
                match result {
                    Ok(_cwd) => {
                        self.flash
                            .trigger(feedback::FlashTarget::Global, feedback::FlashKind::Success);
                    }
                    Err(msg) => {
                        log::error!("gitignore update failed: {msg}");
                        self.flash
                            .trigger(feedback::FlashTarget::Global, feedback::FlashKind::Error);
                    }
                }
            }

            // Drain last commit message result (for amend pre-fill)
            if let Some(cwd) = self.active_cwd() {
                if let Some(msg) = self.workers.git_worker.take_last_commit_msg(&cwd) {
                    if self.show_commit_dialog && self.commit_amend {
                        self.commit_message = msg;
                    }
                }
            }
        }

        // ── Global search debounce: fire after 200ms of stable input ─────
        if self.global_search_debouncer.ready() {
            let query = self.global_search_query.clone();
            if !query.is_empty() {
                let sessions: Vec<_> = self
                    .session_state
                    .sessions
                    .iter()
                    .map(|e| {
                        let title = e.session.read().title();
                        (e.id, title, e.session.clone())
                    })
                    .collect();
                self.workers.search_worker.search(query, sessions);
            } else {
                self.workers.search_worker.cancel();
            }
        } else if self.global_search_debouncer.pending() {
            ctx.request_repaint_after(Duration::from_millis(100));
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

        // Process pending cross-window focus actions (set by sidebar click routing).
        if let Some(action) = self.pending_window_focus.take() {
            match action.target_window_idx {
                Some(idx) if idx < self.extra_windows.len() => {
                    // Target is an extra window.
                    let ew = &mut self.extra_windows[idx];
                    ew.view.active_group = action.group;
                    ew.view.active_pane_id = Some(action.pane_id);
                    if let Some(pane) = self
                        .pane_state
                        .panes
                        .iter()
                        .find(|p| p.id == action.pane_id)
                    {
                        if let PaneContent::Terminal(sid) = pane.content {
                            ew.view.active_id = Some(sid);
                        }
                    }
                    ew.view
                        .last_pane_per_group
                        .insert(action.group, action.pane_id);
                    ctx.send_viewport_cmd_to(
                        action.target_viewport_id,
                        egui::ViewportCommand::Focus,
                    );
                }
                None => {
                    // Target is the main window — set its state directly
                    // (at this point the main view is restored in self).
                    let pane_exists = self.pane_state.panes.iter().any(|p| p.id == action.pane_id);
                    if pane_exists {
                        self.active_group = action.group;
                        self.activate_pane(action.pane_id);
                        self.last_pane_per_group
                            .insert(action.group, action.pane_id);
                    } else {
                        let (cols, rows) = self
                            .pane_state
                            .panes
                            .first()
                            .map(|p| p.last_size)
                            .unwrap_or((80, 24));
                        self.switch_group(action.group, cols, rows);
                    }
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Focus);
                }
                _ => {}
            }
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
    pub(crate) fn vp_id(&self, base: &str) -> egui::Id {
        let id = egui::Id::new(base);
        match &self.current_window_id {
            Some(w) => id.with(w.0),
            None => id,
        }
    }

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
            git_status: String,
            git_unpushed: Vec<(String, String)>,
            dir_entries: Arc<Vec<FileEntry>>,
            md_paths: Vec<PathBuf>,
            md_active_content: Option<Arc<String>>,
        }
        let snap: PanelSnap = match (active_cwd.as_ref(), self.watch_state.as_ref()) {
            (Some(cwd), Some(ws)) => match ws.dir_data.get(cwd) {
                Some(d) => {
                    let mut md_paths: Vec<PathBuf> = d.md_files.keys().cloned().collect();
                    for (p, (_, ws_id)) in &self.terminal_md_content {
                        if *ws_id == self.active_group && !md_paths.contains(p) {
                            md_paths.push(p.clone());
                        }
                    }
                    let md_active_content = if let RightTab::Markdown(p) = &self.right_tab {
                        d.md_files
                            .get(p)
                            .cloned()
                            .or_else(|| self.terminal_md_content.get(p).map(|(c, _)| Arc::clone(c)))
                    } else {
                        None
                    };
                    PanelSnap {
                        is_git: d.is_git,
                        git_status: d.git_status.clone(),
                        git_unpushed: d.git_unpushed.clone(),
                        dir_entries: Arc::clone(&d.dir_entries),
                        md_paths,
                        md_active_content,
                    }
                }
                None => {
                    let md_paths: Vec<PathBuf> = self
                        .terminal_md_content
                        .iter()
                        .filter(|(_, (_, ws_id))| *ws_id == self.active_group)
                        .map(|(p, _)| p.clone())
                        .collect();
                    let md_active_content = if let RightTab::Markdown(p) = &self.right_tab {
                        self.terminal_md_content.get(p).map(|(c, _)| Arc::clone(c))
                    } else {
                        None
                    };
                    PanelSnap {
                        is_git: false,
                        git_status: String::new(),
                        git_unpushed: Vec::new(),
                        dir_entries: Arc::new(Vec::new()),
                        md_paths,
                        md_active_content,
                    }
                }
            },
            _ => {
                let md_paths: Vec<PathBuf> = self
                    .terminal_md_content
                    .iter()
                    .filter(|(_, (_, ws_id))| *ws_id == self.active_group)
                    .map(|(p, _)| p.clone())
                    .collect();
                let md_active_content = if let RightTab::Markdown(p) = &self.right_tab {
                    self.terminal_md_content.get(p).map(|(c, _)| Arc::clone(c))
                } else {
                    None
                };
                PanelSnap {
                    is_git: false,
                    git_status: String::new(),
                    git_unpushed: Vec::new(),
                    dir_entries: Arc::new(Vec::new()),
                    md_paths,
                    md_active_content,
                }
            }
        };
        let PanelSnap {
            is_git,
            git_status,
            git_unpushed,
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
        let mut git_show_commit_dialog = false;
        let mut git_show_push_dialog = false;
        let mut git_show_stage_all_confirm = false;
        let mut git_gitignore_pattern: Option<String> = None;
        let mut open_md_in_editor: Option<PathBuf> = None;

        // Snapshot current note so TextEdit can mutate it inside the closure
        let mut note_text = self.note_store.get(self.active_group).to_string();
        let mut pending_open_note: Option<Option<u64>> = None;

        // ── Right panel ──────────────────────────────────────────────────────
        if self.show_right_panel {
            egui::SidePanel::right(self.vp_id("right_panel"))
                .default_width(theme::RIGHT_SIDEBAR_W)
                .width_range(80.0..=400.0)
                .resizable(true)
                .frame(egui::Frame::none().inner_margin(egui::Margin::ZERO))
                .show(ctx, |ui| {
                    let panel_rect = ui.max_rect();
                    let panel_w = panel_rect.width();
                    let total_h = panel_rect.height();

                    const DIV_H: f32 = 8.0;
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
                                    .id_source(self.vp_id("right_tab_bar"))
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
                                                        .size(theme::FONT_UI_MD),
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
                                                        .size(theme::FONT_UI_MD),
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
                                                            .size(theme::FONT_UI_MD),
                                                    )
                                                    .clicked()
                                                {
                                                    new_tab =
                                                        Some(RightTab::Markdown(path.clone()));
                                                }
                                                ui.add_space(theme::SP_1);
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new("×")
                                                                .size(theme::FONT_UI_MD)
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
                            .id_source(self.vp_id("right_content"))
                            .show(ui, |ui| {
                                let w = ui.available_width();
                                ui.set_min_width(w);
                                ui.set_max_width(w);
                                match &active_tab {
                                    RightTab::Directory => {
                                        if let Some(cwd) = active_cwd.as_ref() {
                                            // ── Workspace name + path ────────────────
                                            ui.horizontal(|ui| {
                                                if let Some(ws) =
                                                    self.workspace_store.find_for_cwd(cwd)
                                                {
                                                    let c = ws.color;
                                                    let panel_bg = theme::active().mantle_rgb;
                                                    ui.label(
                                                        egui::RichText::new(&ws.name)
                                                            .strong()
                                                            .size(theme::FONT_UI_SM)
                                                            .color(theme::ensure_readable(
                                                                c, panel_bg,
                                                            )),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new("›")
                                                            .size(theme::FONT_UI_SM)
                                                            .color(theme::active().overlay0),
                                                    );
                                                }
                                                ui.label(
                                                    egui::RichText::new(theme::short_path(cwd))
                                                        .monospace()
                                                        .size(theme::FONT_UI_SM)
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
                                                        egui::RichText::new(btn_text).size(theme::FONT_UI_MD),
                                                    )
                                                    .frame(false),
                                                );
                                                if save_btn.on_hover_text(tip).clicked() {
                                                    open_ws_dialog = Some(cwd.clone());
                                                }
                                            });
                                            // ── Directory search bar (always visible) ──
                                            {
                                                let focus = self.dir_search_active;
                                                let dir_search_id = self.vp_id("dir_search_input");
                                                let sb = crate::app::ui::search_bar::search_bar_persistent(
                                                    ui,
                                                    &mut self.dir_search_query,
                                                    "\u{1f50d}",
                                                    "Search files\u{2026}",
                                                    dir_search_id,
                                                    focus,
                                                );
                                                if focus {
                                                    self.dir_search_active = false;
                                                }
                                                if sb.escaped {
                                                    self.dir_search_query.clear();
                                                    self.dir_search_debouncer.reset();
                                                    self.workers.file_search_worker.cancel();
                                                }
                                                self.dir_search_debouncer.update(&self.dir_search_query);
                                            }

                                            ui.add_space(theme::SP_2);

                                            // ── Dispatch to file search worker ─────────
                                            let show_search_results = if !self.dir_search_query.is_empty()
                                            {
                                                if self.dir_search_debouncer.ready() || !self.dir_search_debouncer.pending() {
                                                    let results = self.workers.file_search_worker.results();
                                                    if results.query != self.dir_search_query
                                                        || results.root != *cwd
                                                    {
                                                        drop(results);
                                                        self.workers.file_search_worker.search(
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
                                                let results = self.workers.file_search_worker.results();
                                                if !results.completed {
                                                    ui.label(
                                                        egui::RichText::new("Searching…")
                                                            .italics()
                                                            .color(theme::active().overlay0)
                                                            .size(theme::FONT_UI_MD),
                                                    );
                                                } else {
                                                    let entries: Vec<FileEntry> = results
                                                        .matches
                                                        .iter()
                                                        .map(|m| FileEntry {
                                                            name: m.name.clone(),
                                                            path: m.path.clone(),
                                                            is_dir: m.is_dir,
                                                        })
                                                        .collect();
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
                                                    .size(theme::FONT_UI_MD),
                                            );
                                        }
                                    }
                                    RightTab::GitDiff => {
                                        let result = render_git_diff(
                                            ui,
                                            &git_status,
                                            &git_unpushed,
                                            self.push_in_progress,
                                            self.push_error.as_deref(),
                                        );
                                        git_stage_action = result.stage_action;
                                        if result.open_diff_file.is_some() {
                                            git_open_diff_file = result.open_diff_file;
                                        }
                                        if result.open_file.is_some() {
                                            git_open_file = result.open_file;
                                        }
                                        if result.show_commit_dialog {
                                            git_show_commit_dialog = true;
                                        }
                                        if result.show_push_dialog {
                                            git_show_push_dialog = true;
                                        }
                                        if result.show_stage_all_confirm {
                                            git_show_stage_all_confirm = true;
                                        }
                                        if result.gitignore_pattern.is_some() {
                                            git_gitignore_pattern = result.gitignore_pattern;
                                        }
                                    }
                                    RightTab::Markdown(md_path) => {
                                        ui.horizontal(|ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Open in Editor")
                                                            .size(theme::FONT_UI_SM),
                                                    )
                                                    .rounding(theme::R_SM),
                                                )
                                                .clicked()
                                            {
                                                open_md_in_editor = Some(md_path.clone());
                                            }
                                        });
                                        ui.add_space(theme::SP_2);
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
                        self.vp_id("notes_panel_divider"),
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
                                        .size(theme::FONT_UI_MD),
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
                                                        .size(theme::FONT_UI_MD),
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
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new("\u{2197}")
                                                        .size(theme::FONT_UI_MD),
                                                )
                                                .min_size(egui::vec2(
                                                    theme::HEADER_H,
                                                    theme::HEADER_H,
                                                ))
                                                .frame(false),
                                            )
                                            .on_hover_text("Open in pane")
                                            .clicked()
                                        {
                                            pending_open_note = Some(self.active_group);
                                        }
                                    },
                                );
                            },
                        );

                        if !self.notes_panel_collapsed {
                            egui::ScrollArea::both()
                                .id_source(self.vp_id("notes_scroll"))
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::TextEdit::multiline(&mut note_text)
                                            .id(self.vp_id("notes_textedit"))
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
            self.terminal_md_content.remove(&path);
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

        // Execute git stage/unstage action (non-blocking via worker thread)
        if let Some(action) = git_stage_action {
            if let Some(cwd) = active_cwd.as_ref() {
                match action {
                    GitStageAction::Stage(path) => {
                        self.workers.git_worker.enqueue_stage(cwd, path);
                    }
                    GitStageAction::Unstage(path) => {
                        self.workers.git_worker.enqueue_unstage(cwd, path);
                    }
                    GitStageAction::UnstageAll => {
                        self.workers.git_worker.enqueue_unstage_all(cwd);
                    }
                }
            }
        }

        // Trigger git dialogs from render_git_diff result
        if git_show_commit_dialog && !self.show_commit_dialog {
            self.show_commit_dialog = true;
            self.commit_message.clear();
            self.commit_amend = false;
            self.commit_dialog_focus_requested = false;
        }
        if git_show_push_dialog && !self.show_push_dialog {
            self.show_push_dialog = true;
            self.push_force = false;
            self.push_error = None;
        }
        if git_show_stage_all_confirm && !self.show_stage_all_confirm {
            self.show_stage_all_confirm = true;
        }
        if let Some(pattern) = git_gitignore_pattern {
            if let Some(cwd) = active_cwd.as_ref() {
                self.workers.git_worker.enqueue_gitignore(cwd, pattern);
            }
        }

        // File to open in editor (content loaded async after pane creation)
        let pending_open_editor: Option<PathBuf> = open_editor;

        // ── Snapshot editor contents for TextEdit (must be mutable locals) ─
        let mut editor_texts: Vec<(u32, Option<String>)> = self
            .pane_state
            .panes
            .iter()
            .map(|p| {
                let text = match &p.content {
                    PaneContent::FileEditor(ed) => Some(ed.content.clone()),
                    PaneContent::NoteEditor(ne) => {
                        Some(self.note_store.get(ne.workspace_id).to_string())
                    }
                    _ => None,
                };
                (p.id, text)
            })
            .collect();

        // ── Workspace colours per pane (before closure to avoid borrow conflict) ─
        let ws_colors: Vec<Option<[u8; 3]>> = self
            .pane_state
            .panes
            .iter()
            .map(|p| match &p.content {
                PaneContent::Terminal(sid) => {
                    let sid = *sid;
                    self.session_state.find(sid).and_then(|e| {
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
                PaneContent::NoteEditor(ne) => ne.workspace_id.and_then(|id| {
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
            .pane_state
            .panes
            .iter()
            .map(|p| Self::pane_group(&self.session_state.sessions, &self.workspace_store, p))
            .collect();
        let active_group_snap = self.active_group;
        let visible_indices: Vec<usize> = pane_groups
            .iter()
            .enumerate()
            .filter(|(i, g)| {
                **g == active_group_snap
                    && self
                        .pane_state
                        .pane_trees
                        .contains_key(&self.pane_state.panes[*i].id)
            })
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
        if let Some(pid) = self.pane_state.active_pane_id {
            let root_id = self.pane_state.root_of(pid);
            let root_visible = root_id.is_some_and(|rid| {
                visible_indices
                    .iter()
                    .any(|&i| self.pane_state.panes[i].id == rid)
            });
            if !root_visible {
                let pane_idx = self.pane_state.panes.iter().position(|p| p.id == pid);
                if let Some(idx) = pane_idx {
                    // Pane still exists — its group changed. Follow it.
                    self.active_group = pane_groups[idx];
                } else {
                    // Pane was removed — fall back to first pane in the current group.
                    self.pane_state.active_pane_id = visible_indices
                        .first()
                        .map(|&i| self.pane_state.panes[i].id);
                    if let Some(new_pid) = self.pane_state.active_pane_id {
                        if let Some(pane) = self.pane_state.panes.iter().find(|p| p.id == new_pid) {
                            if let PaneContent::Terminal(sid) = pane.content {
                                self.session_state.active_id = Some(sid);
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
        let mut split_request: Option<SplitDir> = self.deferred_split.take();
        let mut close_split_pane: bool = std::mem::take(&mut self.deferred_close_pane);
        // Phase D: split divider ratio changes (split_id, new_ratio)
        let mut split_ratio_changes: Vec<(u32, f32)> = vec![];
        // Phase D: pane context menu actions (3-dot menu on split panes)
        let mut pane_context_actions: Vec<ui::PaneContextAction> = vec![];
        // Phase D: move existing tab into split alongside active pane
        let mut move_to_split: Option<(u32, SplitDir)> = None;

        // ── Central panel ──────────────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(egui::Frame::none().inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
            let panel_rect = ui.max_rect();
            let font_id    = FontId::monospace(self.settings.font_size);
            let (cw, ch) = match self.cached_cell_size {
                Some((fs, w, h)) if fs == self.settings.font_size => (w, h),
                _ => {
                    let w = ui.fonts(|f| f.glyph_width(&font_id, 'M'));
                    let h = ui.fonts(|f| f.row_height(&font_id));
                    self.cached_cell_size = Some((self.settings.font_size, w, h));
                    (w, h)
                }
            };
            resize_cell_w  = cw;
            resize_cell_h  = ch;
            resize_total_h = panel_rect.height();
            panel_w_snap   = panel_rect.width();

            let nv = visible_indices.len();

            let active_pane_id_snap = self.pane_state.active_pane_id;
            let active_is_editor = self.pane_state.active_pane_id
                .and_then(|pid| self.pane_state.panes.iter().find(|p| p.id == pid))
                .map(|p| matches!(p.content, PaneContent::FileEditor(_) | PaneContent::NoteEditor(_)))
                .unwrap_or(false);
            let active_session_id: Option<u32> = self.pane_state.active_pane_id
                .and_then(|pid| self.pane_state.panes.iter().find(|p| p.id == pid))
                .and_then(|p| match &p.content { PaneContent::Terminal(sid) => Some(*sid), _ => None });

            if nv == 0 {
                // Empty group — show a placeholder
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("No sessions in this group.\nUse '+ New' in the Sessions panel to add one.")
                            .color(theme::active().overlay0).size(theme::FONT_TERM)
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
                let status_h = theme::STATUS_BAR_H;
                let content_rect = egui::Rect::from_min_size(
                    egui::pos2(panel_rect.min.x, panel_rect.min.y + tab_h),
                    egui::vec2(panel_rect.width(), (panel_h - tab_h - status_h).max(0.0)),
                );
                let status_rect = egui::Rect::from_min_size(
                    egui::pos2(panel_rect.min.x, content_rect.max.y),
                    egui::vec2(panel_rect.width(), status_h),
                );

                // ── Tab bar (horizontally scrollable) + action buttons ──────
                let tab_result = self.render_tab_bar(
                    ui,
                    &visible_indices,
                    active_pane_id_snap,
                    &ws_colors,
                    tab_h,
                    tab_bar_rect,
                    tab_actions_rect,
                );
                if tab_result.close_pane_id.is_some() {
                    close_pane_id = tab_result.close_pane_id;
                }
                if tab_result.clicked_pane_id.is_some() {
                    clicked_pane_id = tab_result.clicked_pane_id;
                }
                if tab_result.split_request.is_some() {
                    split_request = tab_result.split_request;
                }
                if tab_result.move_to_split.is_some() {
                    move_to_split = tab_result.move_to_split;
                }

                // ── Active tab content (full-size, split-aware) ──────────────
                self.render_pane_content(
                    ui,
                    content_rect,
                    active_pane_id_snap,
                    &mut editor_texts,
                    &mut clicked_pane_id,
                    &mut editor_saves,
                    &mut editor_preview_toggles,
                    &mut pane_widths_snap,
                    &mut split_ratio_changes,
                    &mut pane_context_actions,
                );

            // ── URL detection + search overlay ────────────────────────────
            if let (Some(ref geo), Some(sid)) = (&self.active_term_geo, self.session_state.active_id) {
                if let Some(entry) = self.session_state.find(sid) {
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
                        let cwd = session.cwd.clone();
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
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        lines.hash(&mut hasher);
                        cwd.hash(&mut hasher);
                        let lines_hash = hasher.finish();
                        if lines_hash != self.detection_lines_hash {
                            self.detection_lines_hash = lines_hash;
                            self.detected_urls = crate::url_detector::detect_urls(&lines);
                            self.detected_md_paths = crate::md_detector::detect_md_paths(&lines, &cwd);
                        }
                        for md in &self.detected_md_paths {
                            if self.auto_opened_md.contains(&md.path) {
                                continue;
                            }
                            let is_recent = std::fs::metadata(&md.path)
                                .and_then(|m| m.modified())
                                .ok()
                                .and_then(|t| t.elapsed().ok())
                                .is_some_and(|age| age.as_secs() < 10);
                            if !is_recent {
                                continue;
                            }
                            self.auto_opened_md.insert(md.path.clone());
                            let path = md.path.clone();
                            let ws_id = self.active_group;
                            let results = Arc::clone(&self.md_load_results);
                            let ctx_clone = ctx.clone();
                            std::thread::spawn(move || {
                                let content = std::fs::read_to_string(&path).unwrap_or_default();
                                results.lock().push((path, content, ws_id));
                                ctx_clone.request_repaint();
                            });
                            break;
                        }
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

                    // ── MD path underlines + click-to-open ───────────────────
                    if !self.detected_md_paths.is_empty() {
                        let t = theme::active();
                        let painter = ui.painter();
                        let session = entry.session.read();
                        let display_offset = session.term.grid().display_offset();
                        let visible_rows = (geo.rect.height() / geo.cell_h) as usize;
                        drop(session);
                        for detected in &self.detected_md_paths {
                            let screen_row = detected.line + display_offset as i32;
                            if screen_row < 0 || screen_row >= visible_rows as i32 {
                                continue;
                            }
                            let y = geo.rect.min.y + screen_row as f32 * geo.cell_h + geo.cell_h - 1.5;
                            let x0 = geo.rect.min.x + detected.start_col as f32 * geo.cell_w;
                            let x1 = geo.rect.min.x + detected.end_col as f32 * geo.cell_w;
                            painter.line_segment(
                                [egui::pos2(x0, y), egui::pos2(x1, y)],
                                egui::Stroke::new(0.5, t.green),
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
                                    if let Some(det) = self.detected_md_paths.iter().find(|m| m.line == grid_line && (col as usize) >= m.start_col && (col as usize) < m.end_col) {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                        let sy = geo.rect.min.y + row as f32 * geo.cell_h + geo.cell_h - 1.5;
                                        let sx0 = geo.rect.min.x + det.start_col as f32 * geo.cell_w;
                                        let sx1 = geo.rect.min.x + det.end_col as f32 * geo.cell_w;
                                        ui.painter().line_segment(
                                            [egui::pos2(sx0, sy), egui::pos2(sx1, sy)],
                                            egui::Stroke::new(1.5, t.green),
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
                                    if let Some(path) = crate::md_detector::md_at_position(&self.detected_md_paths, grid_line, col as usize) {
                                        open_md_in_editor = Some(path.to_path_buf());
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
                        ui.painter().rect_filled(bar_rect, theme::R_MD, t.surface0);
                        ui.painter().rect_stroke(bar_rect, theme::R_MD, egui::Stroke::new(1.0, t.overlay0));

                        let input_rect = egui::Rect::from_min_max(
                            egui::pos2(bar_rect.min.x + 6.0, bar_rect.min.y + 4.0),
                            egui::pos2(bar_rect.max.x - 90.0, bar_rect.max.y - 4.0),
                        );
                        let resp = ui.put(
                            input_rect,
                            egui::TextEdit::singleline(&mut self.term_search.query)
                                .desired_width(input_rect.width())
                                .font(egui::FontId::monospace(theme::FONT_UI_MD))
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
                            egui::FontId::monospace(theme::FONT_UI_SM),
                            t.subtext0,
                        );
                    }
                }
            }

            // ── Status bar ─────────────────────────────────────────────────
            {
                let sb_cwd_path = self.active_cwd().unwrap_or_default();
                let sb_cwd = sb_cwd_path.to_string_lossy().to_string();
                let unsaved_folder = self.active_group.is_none()
                    && !sb_cwd.is_empty();
                let (sb_branch, sb_diff) = self.active_group
                    .and_then(|ws_id| self.workers.workspace_git_worker.get(ws_id))
                    .map(|gi| (gi.branch.clone(), gi.diff_count))
                    .unwrap_or_default();
                let sb_shell = active_session_id
                    .and_then(|sid| self.session_state.find(sid))
                    .map(|e| e.shell.display_name().to_string())
                    .unwrap_or_default();
                let (sb_cols, sb_rows) = self.pane_state.active_pane_id
                    .and_then(|pid| self.pane_state.panes.iter().find(|p| p.id == pid))
                    .map(|p| p.last_size)
                    .unwrap_or((0, 0));
                let sb_result = ui::status_bar::render_status_bar(ui, status_rect, &ui::status_bar::StatusBarData {
                    cwd: sb_cwd,
                    git_branch: sb_branch,
                    git_diff_count: sb_diff,
                    shell_name: sb_shell,
                    cols: sb_cols,
                    rows: sb_rows,
                    zoomed: self.zoomed_pane_id.is_some(),
                    unsaved_folder,
                });
                if sb_result.save_workspace_clicked && self.workspace_dialog.is_none() {
                    self.workspace_dialog = Some(WorkspaceDialog::new(sb_cwd_path));
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
                || self.show_quick_switcher
                || self.show_command_palette
                || self.open_folder_dialog.is_some()
                || self.show_close_all_confirm
                || self.show_commit_dialog
                || self.show_push_dialog
                || self.show_stage_all_confirm
                || self.show_quit_confirm;

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
                        Some(AppAction::CommandPalette)
                    } else if i.consume_key(cs, egui::Key::D) {
                        Some(AppAction::RightTabDirectory)
                    } else if i.consume_key(cs, egui::Key::Space) {
                        Some(AppAction::OpenQuickSwitcher)
                    } else if i.consume_key(cs, egui::Key::N) {
                        Some(AppAction::SearchAllSessions)
                    } else if i.consume_key(cs, egui::Key::Z) {
                        Some(AppAction::ZoomPane)
                    } else if i.consume_key(egui::Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: false }, egui::Key::F) {
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
                            self.quick_switcher_selected_ws = None;
                            self.quick_switcher_search_active = false;
                        }
                    }
                    Some(AppAction::OpenSettings) => {
                        self.show_shortcut_help = false;
                        self.show_settings = !self.show_settings;
                    }
                    Some(AppAction::CommandPalette) => {
                        self.show_command_palette = !self.show_command_palette;
                        if !self.show_command_palette {
                            self.command_palette_query.clear();
                            self.command_palette_selected = 0;
                        }
                    }
                    Some(AppAction::FocusSessionSearch) => {
                        self.show_left_panel = true;
                        self.session_search_active = true;
                    }
                    Some(AppAction::FocusFileSearch) | Some(AppAction::RightTabDirectory) => {
                        self.show_right_panel = true;
                        self.right_tab = RightTab::Directory;
                        self.dir_search_active = true;
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
                            self.global_search_debouncer.reset();
                            self.global_search_selected = 0;
                            self.workers.search_worker.cancel();
                        }
                    }
                    Some(AppAction::ZoomPane) => {
                        if self.zoomed_pane_id.is_some() {
                            self.zoomed_pane_id = None;
                        } else {
                            self.zoomed_pane_id = self.pane_state.active_pane_id;
                        }
                    }
                    _ => {}
                }
            }

            if !active_is_editor && !any_other_widget_focused && !modal_open {

                // Focus-in / focus-out events (?1004h)
                if active_session_id != self.last_focused_sid {
                    // Send focus-out to the session we just left
                    if let Some(old_sid) = self.last_focused_sid {
                        if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == old_sid) {
                            let tracking = self.session_state.sessions[idx].session.read().term.mode().contains(TermMode::FOCUS_IN_OUT);
                            if tracking {
                                let _ = self.session_state.sessions[idx].pty_tx.try_send(b"\x1b[O".to_vec());
                            }
                        }
                    }
                    // Send focus-in to the newly active session
                    if let Some(sid) = active_session_id {
                        if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                            let tracking = self.session_state.sessions[idx].session.read().term.mode().contains(TermMode::FOCUS_IN_OUT);
                            if tracking {
                                let _ = self.session_state.sessions[idx].pty_tx.try_send(b"\x1b[I".to_vec());
                            }
                        }
                    }
                    self.last_focused_sid = active_session_id;
                }

                let mut events = ctx.input(|inp| inp.events.clone());
                events.append(&mut self.raw_intercepted_keys);
                for event in &events {
                    match event {
                        egui::Event::Text(text) => {
                            if let Some(sid) = active_session_id {
                                self.term_selection = None;
                                self.term_selection_sid = None;
                                self.scroll_accum.remove(&sid);
                                if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                    self.session_state.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                    let _ = self.session_state.sessions[idx].pty_tx.try_send(text.as_bytes().to_vec());
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
                                                if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
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
                                        self.session_search_active = true;
                                    }
                                    AppAction::FocusFileSearch => {
                                        self.show_right_panel = true;
                                        self.right_tab = RightTab::Directory;
                                        self.dir_search_active = true;
                                    }
                                    AppAction::PreviousTab => {
                                        if nv > 1 {
                                            let root_id = self.pane_state.active_pane_id.and_then(|pid| self.pane_state.root_of(pid));
                                            let cur = root_id.and_then(|rid| visible_indices.iter().position(|&i| self.pane_state.panes[i].id == rid)).unwrap_or(0);
                                            let prev = if cur == 0 { nv - 1 } else { cur - 1 };
                                            clicked_pane_id = Some(self.pane_state.panes[visible_indices[prev]].id);
                                        }
                                    }
                                    AppAction::NextTab => {
                                        if nv > 1 {
                                            let root_id = self.pane_state.active_pane_id.and_then(|pid| self.pane_state.root_of(pid));
                                            let cur = root_id.and_then(|rid| visible_indices.iter().position(|&i| self.pane_state.panes[i].id == rid)).unwrap_or(0);
                                            let next = (cur + 1) % nv;
                                            clicked_pane_id = Some(self.pane_state.panes[visible_indices[next]].id);
                                        }
                                    }
                                    AppAction::NextWorkspace => {
                                        if self.current_window_id.is_none() {
                                            let ws_ids: Vec<u64> = self.workspace_store.workspaces.iter()
                                                .filter(|w| w.host_window_id.is_none())
                                                .map(|w| w.id).collect();
                                            if !ws_ids.is_empty() {
                                                let cur = self.active_group.and_then(|g| ws_ids.iter().position(|&id| id == g)).unwrap_or(0);
                                                let next = (cur + 1) % ws_ids.len();
                                                self.deferred_open_workspace = Some(ws_ids[next]);
                                            }
                                        }
                                    }
                                    AppAction::PrevWorkspace => {
                                        if self.current_window_id.is_none() {
                                            let ws_ids: Vec<u64> = self.workspace_store.workspaces.iter()
                                                .filter(|w| w.host_window_id.is_none())
                                                .map(|w| w.id).collect();
                                            if !ws_ids.is_empty() {
                                                let cur = self.active_group.and_then(|g| ws_ids.iter().position(|&id| id == g)).unwrap_or(0);
                                                let prev = if cur == 0 { ws_ids.len() - 1 } else { cur - 1 };
                                                self.deferred_open_workspace = Some(ws_ids[prev]);
                                            }
                                        }
                                    }
                                    AppAction::RightTabDirectory => {
                                        self.show_right_panel = true;
                                        self.right_tab = RightTab::Directory;
                                        self.dir_search_active = true;
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
                                                clicked_pane_id = Some(self.pane_state.panes[visible_indices[tab_idx]].id);
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
                                let mut handled = false;
                                if let Some(_dir) = dir_opt {
                                    if let Some(active_pid) = active_pane_id_snap {
                                        let root_pid_opt = self.pane_state.pane_trees.iter()
                                            .find(|(_, tree)| tree.leaf_ids().contains(&active_pid))
                                            .map(|(&rpid, _)| rpid);
                                        if let Some(root_pid) = root_pid_opt {
                                            if let Some(tree) = self.pane_state.pane_trees.get(&root_pid) {
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
                                                        handled = true;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                if !handled {
                                    if let Some(bytes) = key_to_pty_bytes(key, modifiers) {
                                        if let Some(sid) = active_session_id {
                                            self.scroll_accum.remove(&sid);
                                            if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                                self.session_state.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                                let _ = self.session_state.sessions[idx].pty_tx.try_send(bytes.to_vec());
                                            }
                                        }
                                    }
                                }
                            } else if let Some(bytes) = key_to_pty_bytes(key, modifiers) {
                                if let Some(sid) = active_session_id {
                                    self.scroll_accum.remove(&sid);
                                    if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                        self.session_state.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                        let _ = self.session_state.sessions[idx].pty_tx.try_send(bytes.to_vec());
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
                                    if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                        let text = self.extract_selected_text(idx);
                                        if !text.is_empty() {
                                            ctx.output_mut(|o| o.copied_text = text);
                                            if let Some(pid) = self.pane_state.active_pane_id {
                                                self.flash.trigger(feedback::FlashTarget::Pane(pid), feedback::FlashKind::Neutral);
                                            }
                                        }
                                    }
                                    self.term_selection = None;
                                    self.term_selection_sid = None;
                                } else {
                                    self.scroll_accum.remove(&sid);
                                    if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                        self.session_state.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                        let _ = self.session_state.sessions[idx].pty_tx.try_send(vec![3u8]);
                                    }
                                }
                            }
                        }
                        // egui-winit converts Ctrl+V to Event::Paste before emitting Event::Key.
                        // Wrap in bracketed-paste sequences only if the app opted in (?2004h).
                        egui::Event::Paste(text) => {
                            if let Some(sid) = active_session_id {
                                self.scroll_accum.remove(&sid);
                                if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                    self.session_state.sessions[idx].session.write().term.scroll_display(Scroll::Bottom);
                                    let bp = self.session_state.sessions[idx].session.read().term.mode().contains(TermMode::BRACKETED_PASTE);
                                    let data = if bp {
                                        let mut v = b"\x1b[200~".to_vec();
                                        v.extend_from_slice(text.as_bytes());
                                        v.extend_from_slice(b"\x1b[201~");
                                        v
                                    } else {
                                        text.as_bytes().to_vec()
                                    };
                                    let _ = self.session_state.sessions[idx].pty_tx.try_send(data);
                                    if let Some(pid) = self.pane_state.active_pane_id {
                                        self.flash.trigger(feedback::FlashTarget::Pane(pid), feedback::FlashKind::Neutral);
                                    }
                                }
                            }
                        }
                        // Mouse events forwarded when the application has enabled mouse reporting.
                        egui::Event::PointerButton { pos, button, pressed, .. } => {
                            let sb_active = self.active_term_geo.as_ref()
                                .map(|g| g.scrollbar_hovered || g.scrollbar_drag_offset.is_some())
                                .unwrap_or(false);
                            if !sb_active {
                            if let Some(sid) = active_session_id {
                                if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                    let (has_mouse, sgr) = {
                                        let s = self.session_state.sessions[idx].session.read();
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
                                                    _ => continue,
                                                };
                                                let bytes = mouse_event_bytes(btn, col, row, *pressed, sgr);
                                                let _ = self.session_state.sessions[idx].pty_tx.try_send(bytes.to_vec());
                                            }
                                        }
                                    } else if *button == egui::PointerButton::Secondary && *pressed && !has_mouse {
                                        if let Some(geo) = &self.active_term_geo {
                                            if geo.rect.contains(*pos) {
                                                self.context_menu_pos = Some(*pos);
                                                let popup_id = egui::Id::new("term_context_menu");
                                                ui.memory_mut(|m| m.open_popup(popup_id));
                                            }
                                        }
                                    } else if *button == egui::PointerButton::Primary {
                                        if let Some(geo) = &self.active_term_geo {
                                            if geo.rect.contains(*pos) {
                                            let clamped = egui::pos2(
                                                pos.x.clamp(geo.rect.min.x, geo.rect.max.x - 1.0),
                                                pos.y.clamp(geo.rect.min.y, geo.rect.max.y - 1.0),
                                            );
                                            if let Some((col, row)) = geo.to_cell(clamped) {
                                                if *pressed {
                                                    let now = Instant::now();
                                                    let same_cell = self.last_click_cell == (col, row);
                                                    let quick = now.duration_since(self.last_click_time) < Duration::from_millis(400);
                                                    if same_cell && quick {
                                                        self.click_count = (self.click_count + 1).min(3);
                                                    } else {
                                                        self.click_count = 1;
                                                    }
                                                    self.last_click_time = now;
                                                    self.last_click_cell = (col, row);

                                                    match self.click_count {
                                                        2 => {
                                                            // Double-click: select word
                                                            let session = self.session_state.sessions[idx].session.read();
                                                            let grid = session.term.grid();
                                                            let display_offset = grid.display_offset();
                                                            let grid_line = row as i32 - display_offset as i32;
                                                            let term_cols = session.term.columns();
                                                            let mut line_text = String::with_capacity(term_cols);
                                                            if grid_line >= -(grid.history_size() as i32) && grid_line < session.term.screen_lines() as i32 {
                                                                for c in 0..term_cols {
                                                                    let cell = &grid[alacritty_terminal::index::Line(grid_line)][alacritty_terminal::index::Column(c)];
                                                                    line_text.push(cell.c);
                                                                }
                                                            }
                                                            drop(session);
                                                            let chars: Vec<char> = line_text.chars().collect();
                                                            let col_usize = col as usize;
                                                            if col_usize < chars.len() {
                                                                let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.';
                                                                let mut start = col_usize;
                                                                let mut end = col_usize;
                                                                while start > 0 && is_word(chars[start - 1]) { start -= 1; }
                                                                while end + 1 < chars.len() && is_word(chars[end + 1]) { end += 1; }
                                                                self.term_selection = Some(TermSelection {
                                                                    start_col: start as u16,
                                                                    start_row: row,
                                                                    end_col: end as u16,
                                                                    end_row: row,
                                                                });
                                                                self.term_selecting = false;
                                                                self.term_selection_sid = Some(sid);
                                                            }
                                                        }
                                                        3 => {
                                                            // Triple-click: select line
                                                            let cols = {
                                                                let session = self.session_state.sessions[idx].session.read();
                                                                session.term.columns() as u16
                                                            };
                                                            self.term_selection = Some(TermSelection {
                                                                start_col: 0,
                                                                start_row: row,
                                                                end_col: cols.saturating_sub(1),
                                                                end_row: row,
                                                            });
                                                            self.term_selecting = false;
                                                            self.term_selection_sid = Some(sid);
                                                        }
                                                        _ => {
                                                            // Single-click: start selection
                                                            self.term_selection = Some(TermSelection {
                                                                start_col: col,
                                                                start_row: row,
                                                                end_col: col,
                                                                end_row: row,
                                                            });
                                                            self.term_selecting = true;
                                                            self.term_selection_sid = Some(sid);
                                                        }
                                                    }
                                                } else {
                                                    self.term_selecting = false;
                                                    if self.click_count <= 1 {
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
                            }
                            } // !sb_active
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
                        egui::Event::PointerMoved(pos) if !self.term_selecting => {
                            if let Some(sid) = active_session_id {
                                if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                    let (drag, motion, sgr) = {
                                        let s = self.session_state.sessions[idx].session.read();
                                        let mode = s.term.mode();
                                        (
                                            mode.contains(TermMode::MOUSE_DRAG),
                                            mode.contains(TermMode::MOUSE_MOTION),
                                            mode.contains(TermMode::SGR_MOUSE),
                                        )
                                    };
                                    if drag || motion {
                                        if let Some(geo) = &self.active_term_geo {
                                            if geo.rect.contains(*pos) {
                                                if let Some((col, row)) = geo.to_cell(*pos) {
                                                    let held_btn = ctx.input(|i| {
                                                        if i.pointer.button_down(egui::PointerButton::Primary) { Some(0u8) }
                                                        else if i.pointer.button_down(egui::PointerButton::Middle) { Some(1u8) }
                                                        else if i.pointer.button_down(egui::PointerButton::Secondary) { Some(2u8) }
                                                        else { None }
                                                    });
                                                    let btn_code = match held_btn {
                                                        Some(b) => Some(b + 32),
                                                        None if motion => Some(35),
                                                        None => None,
                                                    };
                                                    if let Some(code) = btn_code {
                                                        let bytes = mouse_event_bytes(code, col, row, true, sgr);
                                                        let _ = self.session_state.sessions[idx].pty_tx.try_send(bytes.to_vec());
                                                    }
                                                }
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
                                    if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                        let (has_mouse, sgr) = {
                                            let s = self.session_state.sessions[idx].session.read();
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
                                                        let _ = self.session_state.sessions[idx].pty_tx.try_send(bytes.to_vec());
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
                                            let multiplier = self.settings.scroll_lines.max(1) as f32;
                                            let delta_lines = match unit {
                                                egui::MouseWheelUnit::Point => delta.y / cell_h * multiplier,
                                                egui::MouseWheelUnit::Line  => delta.y * multiplier,
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
                                                self.session_state.sessions[idx].session.write().term.scroll_display(Scroll::Delta(scroll_delta));
                                                self.term_selection = None;
                                                self.term_selection_sid = None;
                                                self.term_selecting = false;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // ── Terminal right-click context menu ─────────────────────
                {
                    let popup_id = egui::Id::new("term_context_menu");
                    let popup_open = ui.memory(|m| m.is_popup_open(popup_id));
                    if !popup_open {
                        self.context_menu_pos = None;
                    }
                    if popup_open {
                        if let Some(pos) = self.context_menu_pos {
                            let dummy_resp = ui.interact(
                                egui::Rect::from_center_size(pos, egui::vec2(1.0, 1.0)),
                                popup_id.with("anchor"),
                                egui::Sense::hover(),
                            );
                            egui::containers::popup::popup_below_widget(
                                ui,
                                popup_id,
                                &dummy_resp,
                                egui::containers::popup::PopupCloseBehavior::CloseOnClickOutside,
                                |ui| {
                                    ui.set_min_width(140.0);
                                    let has_selection = self.term_selection.is_some();

                                    if ui.add_enabled(has_selection, egui::Button::new("Copy").min_size(egui::vec2(0.0, 22.0))).clicked() {
                                        if let Some(sid) = active_session_id {
                                            if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                                let text = self.extract_selected_text(idx);
                                                if !text.is_empty() {
                                                    ui.output_mut(|o| o.copied_text = text);
                                                }
                                            }
                                        }
                                        ui.memory_mut(|m| m.close_popup());
                                    }
                                    if ui.button("Paste").clicked() {
                                        if let Some(sid) = active_session_id {
                                            if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                                                if let Some(clip) = ui.input(|i| i.events.iter().find_map(|e| {
                                                    if let egui::Event::Paste(t) = e { Some(t.clone()) } else { None }
                                                })) {
                                                    let _ = self.session_state.sessions[idx].pty_tx.try_send(clip.into_bytes());
                                                } else {
                                                    let mut ctx2 = arboard::Clipboard::new();
                                                    if let Ok(ref mut cb) = ctx2 {
                                                        if let Ok(text) = cb.get_text() {
                                                            let bp = self.session_state.sessions[idx].session.read().term.mode().contains(TermMode::BRACKETED_PASTE);
                                                            let data = if bp {
                                                                let mut v = b"\x1b[200~".to_vec();
                                                                v.extend_from_slice(text.as_bytes());
                                                                v.extend_from_slice(b"\x1b[201~");
                                                                v
                                                            } else {
                                                                text.into_bytes()
                                                            };
                                                            let _ = self.session_state.sessions[idx].pty_tx.try_send(data);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        ui.memory_mut(|m| m.close_popup());
                                    }
                                    ui.separator();
                                    if ui.button("Search (Ctrl+F)").clicked() {
                                        self.term_search.active = true;
                                        ui.memory_mut(|m| m.close_popup());
                                    }
                                    if ui.button("Select All").clicked() {
                                        if let Some(geo) = &self.active_term_geo {
                                            if let Some(sid) = active_session_id {
                                                if let Some(entry) = self.session_state.find(sid) {
                                                    let session = entry.session.read();
                                                    let visible_rows = (geo.rect.height() / geo.cell_h) as u16;
                                                    let cols = session.term.columns() as u16;
                                                    drop(session);
                                                    self.term_selection = Some(TermSelection {
                                                        start_col: 0,
                                                        start_row: 0,
                                                        end_col: cols.saturating_sub(1),
                                                        end_row: visible_rows.saturating_sub(1),
                                                    });
                                                    self.term_selection_sid = Some(sid);
                                                }
                                            }
                                        }
                                        ui.memory_mut(|m| m.close_popup());
                                    }
                                },
                            );
                        }
                    }
                }

                // ── File drag-and-drop → paste shell-quoted paths ──────────
                let dropped = ctx.input(|i| i.raw.dropped_files.clone());
                if !dropped.is_empty() {
                    if let Some(sid) = active_session_id {
                        if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                            let paths: Vec<String> = dropped
                                .iter()
                                .filter_map(|f| f.path.as_ref())
                                .map(|p| input::shell_quote_path(p))
                                .collect();
                            if !paths.is_empty() {
                                let text = paths.join(" ");
                                self.session_state.sessions[idx]
                                    .session
                                    .write()
                                    .term
                                    .scroll_display(Scroll::Bottom);
                                let bp = self.session_state.sessions[idx]
                                    .session
                                    .read()
                                    .term
                                    .mode()
                                    .contains(TermMode::BRACKETED_PASTE);
                                let data = if bp {
                                    let mut v = b"\x1b[200~".to_vec();
                                    v.extend_from_slice(text.as_bytes());
                                    v.extend_from_slice(b"\x1b[201~");
                                    v
                                } else {
                                    text.as_bytes().to_vec()
                                };
                                let _ = self.session_state.sessions[idx]
                                    .pty_tx
                                    .try_send(data);
                            }
                        }
                    }
                }
            }
            } // end else (nv > 0)
        });

        // ── Post-closure mutations ─────────────────────────────────────────

        // Phase D-0: Apply split divider ratio changes from drag
        for (split_id_changed, new_ratio) in split_ratio_changes {
            for tree in self.pane_state.pane_trees.values_mut() {
                if let Some(ratio) = tree.find_split_ratio_mut(split_id_changed) {
                    *ratio = new_ratio;
                    break;
                }
            }
        }

        // Phase D-1: Handle split request (Ctrl+Shift+\ or Ctrl+Shift+-)
        if let Some(dir) = split_request {
            if let Some(active_pid) = self.pane_state.active_pane_id {
                // Find the root pane that contains the active pane
                let root_pid_opt = self
                    .pane_state
                    .pane_trees
                    .iter()
                    .find(|(_, tree)| tree.leaf_ids().contains(&active_pid))
                    .map(|(&rpid, _)| rpid);
                if let Some(root_pid) = root_pid_opt {
                    // Get current size for the new pane
                    let (cols, rows) = self
                        .pane_state
                        .panes
                        .iter()
                        .find(|p| p.id == active_pid)
                        .map(|p| p.last_size)
                        .unwrap_or((80, 24));
                    // Get cwd and shell from active session
                    let (cwd, shell) = {
                        let active_session_entry = self
                            .pane_state
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
                            .and_then(|sid| self.session_state.find(sid));
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
                        let new_pane_id = self.pane_state.next_pane_id;
                        self.pane_state.next_pane_id += 1;
                        let split_id = self.pane_state.next_split_id;
                        self.pane_state.next_split_id += 1;
                        // Add pane entry (NOT a root pane, so no pane_trees entry)
                        self.pane_state.panes.push(PaneEntry {
                            id: new_pane_id,
                            content: PaneContent::Terminal(new_sid),
                            manual_width: None,
                            last_size: (cols, rows),
                        });
                        // Modify the tree to split the active leaf
                        if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                            tree.split_pane(active_pid, new_pane_id, split_id, dir);
                        }
                        // Focus the new pane
                        self.pane_state.active_pane_id = Some(new_pane_id);
                        self.session_state.active_id = Some(new_sid);
                        self.update_is_active_flags();
                        self.flash.trigger(
                            feedback::FlashTarget::Pane(new_pane_id),
                            feedback::FlashKind::Success,
                        );
                        ctx.request_repaint();
                    }
                }
            }
        }

        // Phase D-1b: Handle "Move to split" — move an existing tab into split alongside active pane
        if let Some((clicked_pane, dir)) = move_to_split {
            if let Some(active_pid) = self.pane_state.active_pane_id {
                // When the user clicks split on the active tab, pick the next
                // visible tab as the source to merge alongside it.
                let source_pane_id = if clicked_pane == active_pid {
                    let active_vis_pos = visible_indices
                        .iter()
                        .position(|&i| self.pane_state.panes[i].id == active_pid);
                    active_vis_pos.and_then(|pos| {
                        let next = if pos + 1 < visible_indices.len() {
                            pos + 1
                        } else {
                            pos.checked_sub(1)?
                        };
                        Some(self.pane_state.panes[visible_indices[next]].id)
                    })
                } else {
                    Some(clicked_pane)
                };

                if let Some(source_pane_id) = source_pane_id {
                    if source_pane_id != active_pid {
                        let source_root = self.pane_state.root_of(source_pane_id);
                        let active_root = self.pane_state.root_of(active_pid);
                        if let (Some(source_root_pid), Some(active_root_pid)) =
                            (source_root, active_root)
                        {
                            // Remove the source from its tree (same or different root)
                            let source_node = if source_root_pid == active_root_pid {
                                if let Some(tree) =
                                    self.pane_state.pane_trees.get_mut(&active_root_pid)
                                {
                                    match tree.remove_pane(source_pane_id) {
                                        RemoveResult::IsTarget => None,
                                        RemoveResult::CollapseToSibling(replacement) => {
                                            let node = PaneNode::Leaf {
                                                pane_id: source_pane_id,
                                                last_size: (80, 24),
                                            };
                                            if let Some(tree) = self
                                                .pane_state
                                                .pane_trees
                                                .get_mut(&active_root_pid)
                                            {
                                                *tree = replacement;
                                            }
                                            Some(node)
                                        }
                                        RemoveResult::Done => Some(PaneNode::Leaf {
                                            pane_id: source_pane_id,
                                            last_size: (80, 24),
                                        }),
                                        RemoveResult::NotFound => None,
                                    }
                                } else {
                                    None
                                }
                            } else {
                                self.pane_state.pane_trees.remove(&source_root_pid)
                            };

                            // Insert source alongside active pane
                            if let Some(subtree) = source_node {
                                let split_id = self.pane_state.next_split_id;
                                self.pane_state.next_split_id += 1;
                                if let Some(target_tree) =
                                    self.pane_state.pane_trees.get_mut(&active_root_pid)
                                {
                                    if !target_tree.split_pane_with_node(
                                        active_pid,
                                        subtree.clone(),
                                        split_id,
                                        dir,
                                    ) {
                                        self.pane_state.pane_trees.insert(source_pane_id, subtree);
                                    } else {
                                        let moved_leaves = subtree.leaf_ids();
                                        if let Some(&first) = moved_leaves.first() {
                                            self.pane_state.active_pane_id = Some(first);
                                            if let Some(pane) =
                                                self.pane_state.panes.iter().find(|p| p.id == first)
                                            {
                                                if let PaneContent::Terminal(sid) = pane.content {
                                                    self.session_state.active_id = Some(sid);
                                                    self.update_is_active_flags();
                                                }
                                            }
                                        }
                                        ctx.request_repaint();
                                    }
                                } else {
                                    self.pane_state.pane_trees.insert(source_pane_id, subtree);
                                }
                            }
                        }
                    }
                }
            }
        }

        // Phase D-2: Handle Ctrl+Shift+W — close the focused split pane
        if close_split_pane {
            if let Some(active_pid) = self.pane_state.active_pane_id {
                // Find the root that contains the active pane
                let root_pid_opt = self
                    .pane_state
                    .pane_trees
                    .iter()
                    .find(|(_, tree)| tree.leaf_ids().contains(&active_pid))
                    .map(|(&rpid, _)| rpid);
                if let Some(root_pid) = root_pid_opt {
                    let is_root_itself = root_pid == active_pid;
                    // Check if tree has only one leaf (the root itself)
                    let leaf_count = self
                        .pane_state
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
                            if let Some(pos) = self
                                .pane_state
                                .panes
                                .iter()
                                .position(|p| p.id == active_pid)
                            {
                                if let PaneContent::Terminal(sid) =
                                    self.pane_state.panes[pos].content
                                {
                                    self.session_state.remove(sid);
                                    if self.session_state.active_id == Some(sid) {
                                        self.session_state.active_id =
                                            self.session_state.sessions.first().map(|e| e.id);
                                        self.update_is_active_flags();
                                    }
                                }
                                self.pane_state.panes.remove(pos);
                            }
                            self.pane_state.pane_trees.remove(&root_pid);
                            self.pane_state.active_pane_id =
                                self.pane_state.panes.last().map(|p| p.id);
                            if self.zoomed_pane_id == Some(active_pid) {
                                self.zoomed_pane_id = None;
                            }
                            self.save_session();
                        }
                    } else {
                        // Remove the leaf from the tree, collapsing the parent split
                        let remove_result =
                            if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                                tree.remove_pane(active_pid)
                            } else {
                                RemoveResult::NotFound
                            };
                        if let RemoveResult::CollapseToSibling(replacement) = remove_result {
                            if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                                *tree = replacement;
                            }
                        }
                        // Kill the session of the removed pane
                        if let Some(pos) = self
                            .pane_state
                            .panes
                            .iter()
                            .position(|p| p.id == active_pid)
                        {
                            if let PaneContent::Terminal(sid) = self.pane_state.panes[pos].content {
                                self.session_state.remove(sid);
                                if self.session_state.active_id == Some(sid) {
                                    self.session_state.active_id =
                                        self.session_state.sessions.first().map(|e| e.id);
                                    self.update_is_active_flags();
                                }
                            }
                            self.pane_state.panes.remove(pos);
                        }
                        if self.zoomed_pane_id == Some(active_pid) {
                            self.zoomed_pane_id = None;
                        }
                        // Focus sibling — pick the first leaf of the root tree
                        if let Some(tree) = self.pane_state.pane_trees.get(&root_pid) {
                            let leaves = tree.leaf_ids();
                            self.pane_state.active_pane_id = leaves.first().copied();
                            if let Some(new_pid) = self.pane_state.active_pane_id {
                                if let Some(pane) =
                                    self.pane_state.panes.iter().find(|p| p.id == new_pid)
                                {
                                    if let PaneContent::Terminal(sid) = pane.content {
                                        self.session_state.active_id = Some(sid);
                                        self.update_is_active_flags();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Phase D-3: Handle pane context menu actions (3-dot menu on split panes)
        for action in pane_context_actions {
            use ui::PaneContextAction;
            match action {
                PaneContextAction::MoveToTab(pid) => {
                    if let Some(root_pid) = self.pane_state.root_of(pid) {
                        let leaf_count = self
                            .pane_state
                            .pane_trees
                            .get(&root_pid)
                            .map(|t| t.leaf_ids().len())
                            .unwrap_or(1);
                        if leaf_count > 1 {
                            let remove_result =
                                if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                                    tree.remove_pane(pid)
                                } else {
                                    RemoveResult::NotFound
                                };
                            if let RemoveResult::CollapseToSibling(replacement) = remove_result {
                                if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                                    *tree = replacement;
                                }
                            }
                            let last_size = self
                                .pane_state
                                .panes
                                .iter()
                                .find(|p| p.id == pid)
                                .map(|p| p.last_size)
                                .unwrap_or((80, 24));
                            self.pane_state.pane_trees.insert(
                                pid,
                                PaneNode::Leaf {
                                    pane_id: pid,
                                    last_size,
                                },
                            );
                            self.pane_state.active_pane_id = Some(pid);
                            if let Some(pane) = self.pane_state.panes.iter().find(|p| p.id == pid) {
                                if let PaneContent::Terminal(sid) = pane.content {
                                    self.session_state.active_id = Some(sid);
                                    self.update_is_active_flags();
                                }
                            }
                            ctx.request_repaint();
                        }
                    }
                }
                PaneContextAction::Close(pid) => {
                    if let Some(root_pid) = self.pane_state.root_of(pid) {
                        let leaf_count = self
                            .pane_state
                            .pane_trees
                            .get(&root_pid)
                            .map(|t| t.leaf_ids().len())
                            .unwrap_or(1);
                        if leaf_count <= 1 {
                            if let Some(pos) =
                                self.pane_state.panes.iter().position(|p| p.id == pid)
                            {
                                if let PaneContent::Terminal(sid) =
                                    self.pane_state.panes[pos].content
                                {
                                    self.session_state.remove(sid);
                                    if self.session_state.active_id == Some(sid) {
                                        self.session_state.active_id =
                                            self.session_state.sessions.first().map(|e| e.id);
                                        self.update_is_active_flags();
                                    }
                                }
                                self.pane_state.panes.remove(pos);
                            }
                            self.pane_state.pane_trees.remove(&root_pid);
                            self.pane_state.active_pane_id =
                                self.pane_state.panes.last().map(|p| p.id);
                            self.save_session();
                        } else {
                            let remove_result =
                                if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                                    tree.remove_pane(pid)
                                } else {
                                    RemoveResult::NotFound
                                };
                            if let RemoveResult::CollapseToSibling(replacement) = remove_result {
                                if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                                    *tree = replacement;
                                }
                            }
                            if let Some(pos) =
                                self.pane_state.panes.iter().position(|p| p.id == pid)
                            {
                                if let PaneContent::Terminal(sid) =
                                    self.pane_state.panes[pos].content
                                {
                                    self.session_state.remove(sid);
                                    if self.session_state.active_id == Some(sid) {
                                        self.session_state.active_id =
                                            self.session_state.sessions.first().map(|e| e.id);
                                        self.update_is_active_flags();
                                    }
                                }
                                self.pane_state.panes.remove(pos);
                            }
                            if let Some(tree) = self.pane_state.pane_trees.get(&root_pid) {
                                let leaves = tree.leaf_ids();
                                self.pane_state.active_pane_id = leaves.first().copied();
                                if let Some(new_pid) = self.pane_state.active_pane_id {
                                    if let Some(pane) =
                                        self.pane_state.panes.iter().find(|p| p.id == new_pid)
                                    {
                                        if let PaneContent::Terminal(sid) = pane.content {
                                            self.session_state.active_id = Some(sid);
                                            self.update_is_active_flags();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                PaneContextAction::SplitHorizontal(pid) | PaneContextAction::SplitVertical(pid) => {
                    let dir = if matches!(action, PaneContextAction::SplitHorizontal(_)) {
                        SplitDir::Horizontal
                    } else {
                        SplitDir::Vertical
                    };
                    if let Some(root_pid) = self.pane_state.root_of(pid) {
                        let (cols, rows) = self
                            .pane_state
                            .panes
                            .iter()
                            .find(|p| p.id == pid)
                            .map(|p| p.last_size)
                            .unwrap_or((80, 24));
                        let (cwd, shell) = {
                            let entry = self
                                .pane_state
                                .panes
                                .iter()
                                .find(|p| p.id == pid)
                                .and_then(|p| {
                                    if let PaneContent::Terminal(sid) = &p.content {
                                        Some(*sid)
                                    } else {
                                        None
                                    }
                                })
                                .and_then(|sid| self.session_state.find(sid));
                            let cwd = entry
                                .map(|e| e.session.read().cwd.clone())
                                .filter(|p| !p.as_os_str().is_empty());
                            let shell =
                                entry.map(|e| e.shell.clone()).unwrap_or_else(default_shell);
                            (cwd, shell)
                        };
                        if let Some(new_sid) = self.spawn_session_no_pane(&shell, cols, rows, cwd) {
                            let new_pane_id = self.pane_state.next_pane_id;
                            self.pane_state.next_pane_id += 1;
                            let split_id = self.pane_state.next_split_id;
                            self.pane_state.next_split_id += 1;
                            self.pane_state.panes.push(PaneEntry {
                                id: new_pane_id,
                                content: PaneContent::Terminal(new_sid),
                                manual_width: None,
                                last_size: (cols, rows),
                            });
                            if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                                tree.split_pane(pid, new_pane_id, split_id, dir);
                            }
                            self.pane_state.active_pane_id = Some(new_pane_id);
                            self.session_state.active_id = Some(new_sid);
                            self.update_is_active_flags();
                            ctx.request_repaint();
                        }
                    }
                }
            }
        }

        // 1. Divider drags → freeze manual widths on both adjacent panes
        for (left_idx, right_idx, delta_x, left_w, right_w) in divider_drags {
            self.pane_state.panes[left_idx].manual_width =
                Some((left_w + delta_x).max(theme::MIN_PANE_W));
            self.pane_state.panes[right_idx].manual_width =
                Some((right_w - delta_x).max(theme::MIN_PANE_W));
        }

        // 2. Close pane (tab-strip close — kills the entire split tree for that root)
        if let Some(pid) = close_pane_id {
            // Collect all pane IDs in this root's split tree so we can kill them all.
            let tree_ids: Vec<u32> = self
                .pane_state
                .pane_trees
                .get(&pid)
                .map(|t| t.leaf_ids())
                .unwrap_or_else(|| vec![pid]);
            // Kill every session belonging to any leaf of this tree.
            for leaf_pid in &tree_ids {
                if let Some(pos) = self.pane_state.panes.iter().position(|p| p.id == *leaf_pid) {
                    if let PaneContent::Terminal(sid) = self.pane_state.panes[pos].content {
                        self.session_state.remove(sid);
                        if self.session_state.active_id == Some(sid) {
                            self.session_state.active_id =
                                self.session_state.sessions.first().map(|e| e.id);
                            self.update_is_active_flags();
                        }
                    }
                }
            }
            // Remove all leaf panes from the panes vec.
            self.pane_state.panes.retain(|p| !tree_ids.contains(&p.id));
            editor_texts.retain(|(id, _)| !tree_ids.contains(id));
            // Remove the root's tree entry.
            self.pane_state.pane_trees.remove(&pid);
            if self
                .pane_state
                .active_pane_id
                .map(|ap| tree_ids.contains(&ap))
                .unwrap_or(false)
            {
                self.pane_state.active_pane_id = self.pane_state.panes.last().map(|p| p.id);
                self.active_group = self
                    .pane_state
                    .active_pane_id
                    .and_then(|pid| self.pane_state.panes.iter().find(|p| p.id == pid))
                    .and_then(|p| {
                        Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
                    });
            }
            if self
                .zoomed_pane_id
                .map(|zp| tree_ids.contains(&zp))
                .unwrap_or(false)
            {
                self.zoomed_pane_id = None;
            }
            self.save_session();
        }

        // 3. Equalize pane widths for visible panes (split icon clicked)
        if equalize_widths {
            for &i in &visible_indices {
                self.pane_state.panes[i].manual_width = None;
            }
            loop {
                let nv2 = visible_indices
                    .iter()
                    .filter(|&&i| i < self.pane_state.panes.len())
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
                    let removed = self.pane_state.panes.remove(first_vi);
                    if let PaneContent::Terminal(sid) = removed.content {
                        self.session_state.remove(sid);
                        if self.session_state.active_id == Some(sid) {
                            self.session_state.active_id =
                                self.session_state.sessions.first().map(|e| e.id);
                            self.update_is_active_flags();
                        }
                    }
                    if self.pane_state.active_pane_id == Some(removed.id) {
                        self.pane_state.active_pane_id =
                            self.pane_state.panes.first().map(|p| p.id);
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
        if let Some(pid) = self.pane_state.active_pane_id {
            if let Some(pane_idx) = self.pane_state.panes.iter().position(|p| p.id == pid) {
                if matches!(
                    &self.pane_state.panes[pane_idx].content,
                    PaneContent::DeferredTerminal { .. }
                ) {
                    let (cwd, pending_command, saved_title) =
                        if let PaneContent::DeferredTerminal {
                            cwd,
                            pending_command,
                            saved_title,
                        } = &self.pane_state.panes[pane_idx].content
                        {
                            (cwd.clone(), pending_command.clone(), saved_title.clone())
                        } else {
                            unreachable!()
                        };
                    let shell = self.configured_shell();
                    if let Some(sid) = self.spawn_session_no_pane(&shell, 80, 24, cwd) {
                        if let Some(entry) = self.session_state.find_mut(sid) {
                            if let Some(cmd) = pending_command {
                                entry.pending_command = Some(cmd);
                            }
                            if let Some(t) = saved_title.filter(|t| !t.is_empty()) {
                                entry.session.read().set_title(t);
                            }
                        }
                        self.pane_state.panes[pane_idx].content = PaneContent::Terminal(sid);
                        self.pane_state.panes[pane_idx].last_size = (0, 0); // force resize next frame
                        self.session_state.active_id = Some(sid);
                        self.update_is_active_flags();
                        ctx.request_repaint();
                    }
                }
            }
        }

        // 6. Editor text changes
        for (pane_id, new_text) in &editor_texts {
            if let Some(ref new_text) = new_text {
                if let Some(pane) = self.pane_state.panes.iter_mut().find(|p| p.id == *pane_id) {
                    match pane.content {
                        PaneContent::FileEditor(ref mut ed) if *new_text != ed.content => {
                            ed.content = new_text.clone();
                            ed.dirty = true;
                            ed.save_error = false;
                        }
                        PaneContent::NoteEditor(ref ne)
                            if *new_text != self.note_store.get(ne.workspace_id) =>
                        {
                            self.note_store.set(ne.workspace_id, new_text.clone());
                            self.note_store.save();
                        }
                        _ => {}
                    }
                }
            }
        }

        // 7. Editor saves (Ctrl+S)
        for save_id in &editor_saves {
            if let Some(p) = self.pane_state.panes.iter_mut().find(|p| p.id == *save_id) {
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
            if let Some(p) = self
                .pane_state
                .panes
                .iter_mut()
                .find(|p| p.id == *toggle_id)
            {
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
            if let Some(pane_idx) = self.pane_state.panes.iter().position(|p| p.id == pane_id) {
                if let PaneContent::Terminal(sid) = self.pane_state.panes[pane_idx].content {
                    let cols = ((width / resize_cell_w) as u16).max(1);
                    let rows = (((resize_total_h - theme::HEADER_H) / resize_cell_h) as u16).max(1);
                    let target = (cols, rows);
                    if target != self.pane_state.panes[pane_idx].last_size {
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
                    .pane_state
                    .panes
                    .iter_mut()
                    .find(|p| matches!(p.content, PaneContent::Terminal(s) if s == sid))
                {
                    pane.last_size = (cols, rows);
                }
                if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
                    let entry = &self.session_state.sessions[idx];
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
        if let Some(path) = pending_open_editor {
            let existing_id = self
                .pane_state
                .panes
                .iter()
                .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path))
                .map(|p| p.id);
            if let Some(pid) = existing_id {
                self.activate_pane(pid);
            } else {
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");
                self.pane_state.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::FileEditor(FileEditorState {
                        path: path.clone(),
                        content: String::new(),
                        dirty: false,
                        save_error: false,
                        workspace_id: self.active_group,
                        show_preview: is_md && self.md_prefer_preview,
                    }),
                    manual_width: None,
                    last_size: (0, 0),
                });
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (0, 0),
                    },
                );
                self.activate_pane(pane_id);
                {
                    let results = Arc::clone(&self.file_load_results);
                    let ctx_clone = ctx.clone();
                    std::thread::spawn(move || {
                        let content = match std::fs::read(&path) {
                            Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|e| {
                                String::from_utf8_lossy(e.as_bytes()).into_owned()
                            }),
                            Err(_) => String::new(),
                        };
                        results.lock().push((pane_id, content));
                        ctx_clone.request_repaint();
                    });
                }
            }
        }

        // 9a. Markdown "Open in Editor" or Ctrl+Click from terminal
        if let Some(path) = open_md_in_editor {
            let existing_id = self
                .pane_state
                .panes
                .iter()
                .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path))
                .map(|p| p.id);
            if let Some(pid) = existing_id {
                self.activate_pane(pid);
            } else {
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                self.pane_state.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::FileEditor(FileEditorState {
                        path: path.clone(),
                        content: String::new(),
                        dirty: false,
                        save_error: false,
                        workspace_id: self.active_group,
                        show_preview: true,
                    }),
                    manual_width: None,
                    last_size: (0, 0),
                });
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (0, 0),
                    },
                );
                self.activate_pane(pane_id);
                {
                    let results = Arc::clone(&self.file_load_results);
                    let ctx_clone = ctx.clone();
                    std::thread::spawn(move || {
                        let content = match std::fs::read(&path) {
                            Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|e| {
                                String::from_utf8_lossy(e.as_bytes()).into_owned()
                            }),
                            Err(_) => String::new(),
                        };
                        results.lock().push((pane_id, content));
                        ctx_clone.request_repaint();
                    });
                }
            }
        }

        // 9a2. Open notes in a pane
        if let Some(ws_id) = pending_open_note {
            let existing_id = self
                .pane_state
                .panes
                .iter()
                .find(|p| matches!(&p.content, PaneContent::NoteEditor(ne) if ne.workspace_id == ws_id))
                .map(|p| p.id);
            if let Some(pid) = existing_id {
                self.activate_pane(pid);
            } else {
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                self.pane_state.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::NoteEditor(NoteEditorState {
                        workspace_id: ws_id,
                    }),
                    manual_width: None,
                    last_size: (0, 0),
                });
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (0, 0),
                    },
                );
                self.activate_pane(pane_id);
            }
        }

        // 9b. Git diff file double-clicked → open FileDiff pane (non-blocking)
        if let Some(rel_path) = git_open_diff_file {
            if let Some(cwd) = self.active_cwd() {
                let full_path = cwd.join(&rel_path);
                let existing_id = self
                    .pane_state
                    .panes
                    .iter()
                    .find(|p| matches!(&p.content, PaneContent::FileDiff(d) if d.path == full_path))
                    .map(|p| p.id);
                if let Some(pid) = existing_id {
                    self.activate_pane(pid);
                } else {
                    let pane_id = self.pane_state.next_pane_id;
                    self.pane_state.next_pane_id += 1;
                    self.pane_state.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::FileDiff(FileDiffState {
                            path: full_path.clone(),
                            diff_content: String::new(),
                        }),
                        manual_width: None,
                        last_size: (0, 0),
                    });
                    self.pane_state.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (0, 0),
                        },
                    );
                    self.pending_diff_panes.insert(full_path, pane_id);
                    self.workers.git_worker.enqueue_diff(&cwd, rel_path);
                    self.activate_pane(pane_id);
                }
            }
        }

        // 9b2. Git file double-clicked → open file in editor
        if let Some(rel_path) = git_open_file {
            if let Some(cwd) = self.active_cwd() {
                let full_path = cwd.join(&rel_path);
                let existing_id = self
                    .pane_state.panes
                    .iter()
                    .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == full_path))
                    .map(|p| p.id);
                if let Some(pid) = existing_id {
                    self.activate_pane(pid);
                } else {
                    let pane_id = self.pane_state.next_pane_id;
                    self.pane_state.next_pane_id += 1;
                    let is_md = full_path.extension().and_then(|e| e.to_str()) == Some("md");
                    self.pane_state.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::FileEditor(FileEditorState {
                            path: full_path.clone(),
                            content: String::new(),
                            dirty: false,
                            save_error: false,
                            workspace_id: self.active_group,
                            show_preview: is_md && self.md_prefer_preview,
                        }),
                        manual_width: None,
                        last_size: (0, 0),
                    });
                    self.pane_state.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (0, 0),
                        },
                    );
                    self.activate_pane(pane_id);
                    {
                        let results = Arc::clone(&self.file_load_results);
                        let ctx_clone = ctx.clone();
                        std::thread::spawn(move || {
                            let content = match std::fs::read(&full_path) {
                                Ok(bytes) => String::from_utf8(bytes).unwrap_or_else(|e| {
                                    String::from_utf8_lossy(e.as_bytes()).into_owned()
                                }),
                                Err(_) => String::new(),
                            };
                            results.lock().push((pane_id, content));
                            ctx_clone.request_repaint();
                        });
                    }
                }
            }
        }

        // 9c. Folder double-clicked in directory → open new terminal pane at that path
        if let Some(dir_path) = open_terminal_at {
            let pane_id = self.pane_state.next_pane_id;
            self.pane_state.next_pane_id += 1;
            self.pane_state.panes.push(PaneEntry {
                id: pane_id,
                content: PaneContent::DeferredTerminal {
                    cwd: Some(dir_path),
                    pending_command: None,
                    saved_title: None,
                },
                manual_width: None,
                last_size: (0, 0),
            });
            self.pane_state.pane_trees.insert(
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
        self.render_command_palette(ctx);

        self.render_workspace_save_dialog(ctx);

        self.render_workspace_edit_dialog(ctx);

        self.render_open_folder_dialog(ctx);

        self.render_close_all_confirm(ctx);
        self.render_quit_confirm(ctx);
        self.render_commit_dialog(ctx);
        self.render_push_dialog(ctx);
        self.render_stage_all_confirm(ctx);
    }
}
