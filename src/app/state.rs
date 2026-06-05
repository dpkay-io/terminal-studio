use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::pane_tree::{PaneNode, RemoveResult};
use crate::pty::foreground_worker::ForegroundWorker;
use crate::pty::{available_shells, default_shell, SessionManager, ShellKind};
use crate::shortcuts::ShortcutRegistry;
use crate::sys_monitor::SysMonitor;
use crate::theme;
use crate::updater::UpdateChecker;
use crate::workspace::{NoteStore, WindowId, Workspace, WorkspaceStore};

use alacritty_terminal::grid::Dimensions;

use super::drag;
use super::multi_window::{ExtraWindow, PendingWindowFocus, SavedExtraWindow, WindowView};
use super::pane::{
    FileDiffState, FileEditorState, NoteEditorState, PaneContent, PaneEntry, RightTab, SessionEntry,
};
use super::pane_state::PaneState;
use super::persistence::{
    session_data_path, AppSession, SavedPane, SavedPaneContent, SavedRightTab, SavedSession,
};
use super::session_state::SessionState;
use super::settings::{windows_data_path, AppSettings};
use super::title::{effective_title, shell_escape_arg};
use super::watcher::WatchState;
use super::{App, CloseAllTarget};

impl App {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let ctx = cc.egui_ctx.clone();

        {
            use egui::{Rounding, Shadow, Stroke, Visuals};
            let t = theme::active();
            let mut vis = if t.is_light {
                Visuals::light()
            } else {
                Visuals::dark()
            };
            vis.panel_fill = t.bg_panel_fill;
            vis.window_fill = t.bg_term;
            vis.window_rounding = Rounding::same(theme::R_LG);
            vis.window_shadow = Shadow::NONE;
            vis.popup_shadow = Shadow::NONE;
            vis.widgets.noninteractive.bg_fill = t.surface0;
            vis.widgets.inactive.bg_fill = t.surface0;
            vis.widgets.hovered.bg_fill = t.surface1;
            vis.widgets.active.bg_fill = t.surface2;
            vis.widgets.inactive.fg_stroke = Stroke::new(1.0, t.subtext0);
            vis.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.overlay0);
            vis.selection.bg_fill = t.selection_bg;
            for state in [
                &mut vis.widgets.noninteractive,
                &mut vis.widgets.inactive,
                &mut vis.widgets.hovered,
                &mut vis.widgets.active,
                &mut vis.widgets.open,
            ] {
                state.rounding = Rounding::same(theme::R_MD);
            }
            vis.override_text_color = Some(t.text);
            cc.egui_ctx.set_visuals(vis);

            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.scroll = egui::style::ScrollStyle {
                floating: true,
                bar_width: 12.0,
                handle_min_length: 20.0,
                bar_inner_margin: theme::SP_1,
                bar_outer_margin: 0.0,
                floating_width: 4.0,
                floating_allocated_width: 0.0,
                foreground_color: true,
                dormant_background_opacity: 0.0,
                active_background_opacity: 0.4,
                interact_background_opacity: 0.7,
                dormant_handle_opacity: 0.0,
                active_handle_opacity: 0.7,
                interact_handle_opacity: 1.0,
            };
            style.always_scroll_the_only_direction = true;
            cc.egui_ctx.set_style(style);
        }

        {
            use egui::{FontData, FontDefinitions, FontFamily};
            let mut fonts = FontDefinitions::default();
            let mut loaded = false;

            #[cfg(target_os = "windows")]
            {
                let win_root =
                    std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
                let font_path = std::path::PathBuf::from(&win_root)
                    .join("Fonts")
                    .join("seguisym.ttf");
                if let Ok(data) = std::fs::read(&font_path) {
                    fonts
                        .font_data
                        .insert("symbol_fallback".to_owned(), FontData::from_owned(data));
                    loaded = true;
                }
            }

            #[cfg(target_os = "linux")]
            {
                let candidates = [
                    "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
                    "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
                    "/usr/share/fonts/google-noto/NotoSansSymbols2-Regular.ttf",
                    "/usr/share/fonts/TTF/NotoSansSymbols2-Regular.ttf",
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                    "/usr/share/fonts/dejavu/DejaVuSans.ttf",
                ];
                for path in candidates {
                    if let Ok(data) = std::fs::read(path) {
                        fonts
                            .font_data
                            .insert("symbol_fallback".to_owned(), FontData::from_owned(data));
                        loaded = true;
                        break;
                    }
                }
            }

            #[cfg(target_os = "macos")]
            {
                let path = "/System/Library/Fonts/Apple Symbols.ttf";
                if let Ok(data) = std::fs::read(path) {
                    fonts
                        .font_data
                        .insert("symbol_fallback".to_owned(), FontData::from_owned(data));
                    loaded = true;
                }
            }

            if loaded {
                for family in [&FontFamily::Proportional, &FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(family.clone())
                        .or_default()
                        .push("symbol_fallback".to_owned());
                }
                cc.egui_ctx.set_fonts(fonts);
            }
        }

        let mgr = SessionManager::new(ctx.clone());
        let loaded_settings = AppSettings::load();
        let last_update_check = loaded_settings.last_update_check;
        let show_sys_monitor = loaded_settings.show_sys_monitor;
        let (subdir_load_tx, subdir_load_rx) = std::sync::mpsc::channel();
        let mut app = App {
            session_manager: mgr,
            session_state: SessionState::new(),
            pane_state: PaneState::new(),
            right_tab: RightTab::Directory,
            shown_md_tabs: HashSet::new(),
            watch_state: WatchState::new(ctx.clone()),
            workspace_store: WorkspaceStore::load(),
            active_group: None,
            last_pane_per_group: HashMap::new(),
            workspace_dialog: None,
            workspace_edit_dialog: None,
            open_folder_dialog: None,
            workspace_panel_ratio: 0.35,
            workspace_panel_collapsed: false,
            workspace_search_query: String::new(),
            note_store: NoteStore::load(),
            notes_panel_ratio: 0.35,
            notes_panel_collapsed: false,
            show_left_panel: true,
            show_right_panel: true,
            show_settings: false,
            settings_saved_at: None,
            show_shortcut_help: false,
            show_quick_switcher: false,
            quick_switcher_query: String::new(),
            quick_switcher_selected_ws: None,
            quick_switcher_search_active: false,
            show_command_palette: false,
            command_palette_query: String::new(),
            command_palette_selected: 0,
            show_closed_sessions: false,
            closed_sessions_query: String::new(),
            closed_sessions_selected: 0,
            closed_sessions_cache: None,
            shortcut_registry: ShortcutRegistry::new(),
            settings: loaded_settings,
            active_term_geo: None,
            last_focused_sid: None,
            active_term_ui_id: None,
            resize_debounce: HashMap::new(),
            scroll_accum: HashMap::new(),
            workers: super::worker_manager::WorkerManager {
                foreground_worker: ForegroundWorker::spawn(),
                git_worker: super::git_worker::GitWorker::spawn(ctx.clone()),
                workspace_git_worker: super::workspace_git_worker::WorkspaceGitWorker::spawn(
                    ctx.clone(),
                ),
                search_worker: crate::search_worker::SearchWorker::spawn(ctx.clone()),
                file_search_worker: crate::file_search_worker::FileSearchWorker::spawn(ctx.clone()),
                sys_monitor: if show_sys_monitor {
                    SysMonitor::spawn(ctx.clone(), Duration::from_secs(2))
                } else {
                    None
                },
                update_checker: UpdateChecker::spawn(ctx.clone(), last_update_check),
            },
            was_focused: true,
            available_shells: available_shells(),
            cursor_alpha: 1.0,
            cursor_blink_start: Instant::now(),
            term_selection: None,
            term_selecting: false,
            term_selection_sid: None,
            raw_intercepted_keys: Vec::new(),
            extra_windows: Vec::new(),
            next_window_id: 1,
            current_window_id: None,
            cached_cell_size: None,
            subdir_cache: HashMap::new(),
            subdir_load_tx,
            subdir_load_rx,
            subdir_loading: HashSet::new(),
            last_title_sent: None,
            session_search_query: String::new(),
            session_search_active: false,
            dir_search_query: String::new(),
            dir_search_active: false,
            dir_search_debouncer: crate::app::ui::debounce::Debouncer::new(Duration::from_millis(
                150,
            )),
            dir_search_changed_at: Instant::now(),
            md_prefer_preview: false,
            term_search: crate::search::SearchState::new(),
            text_search: crate::search::TextSearchState::new(),
            show_global_search: false,
            global_search_query: String::new(),
            global_search_debouncer: crate::app::ui::debounce::Debouncer::new(
                Duration::from_millis(200),
            ),
            global_search_selected: 0,
            detected_urls: Vec::new(),
            detected_md_paths: Vec::new(),
            detection_lines_hash: 0,
            auto_opened_md: HashSet::new(),
            terminal_md_content: HashMap::new(),
            drag_state: drag::DragState::new(),
            deferred_spawn: None,
            deferred_duplicate: false,
            deferred_split: None,
            deferred_close_pane: false,
            tab_rename_pane_id: None,
            tab_rename_text: String::new(),
            deferred_open_workspace: None,
            show_close_all_confirm: false,
            close_all_target: CloseAllTarget::default(),
            show_quit_confirm: false,
            quit_confirmed: false,
            session_workspace_filter: None,
            pending_window_focus: None,
            pending_diff_panes: HashMap::new(),
            file_load_results: std::sync::Arc::new(parking_lot::Mutex::new(Vec::new())),
            md_load_results: std::sync::Arc::new(parking_lot::Mutex::new(Vec::new())),
            flash: super::feedback::FlashManager::new(),
            last_click_time: Instant::now(),
            last_click_cell: (0, 0),
            click_count: 0,
            command_start_times: HashMap::new(),
            completed_badges: std::collections::HashSet::new(),
            zoomed_pane_id: None,
            show_commit_dialog: false,
            commit_message: String::new(),
            commit_amend: false,
            commit_dialog_focus_requested: false,
            show_push_dialog: false,
            push_force: false,
            push_in_progress: false,
            push_error: None,
            show_stage_all_confirm: false,
            revert_confirm_file: None,
            context_menu_pos: None,
        };

        let (init_cols, init_rows) = {
            const CELL_W: f32 = 8.4;
            const CELL_H: f32 = 18.0;
            let est_w = (1280.0 - theme::LEFT_SIDEBAR_W - 300.0 - 4.0).max(100.0);
            let est_h = (800.0 - theme::TITLEBAR_H - theme::HEADER_H - 4.0).max(50.0);
            let cols = ((est_w / CELL_W) as u16).max(80);
            let rows = ((est_h / CELL_H) as u16).max(24);
            (cols, rows)
        };

        app.load_windows();

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

    pub(super) fn spawn_session(
        &mut self,
        shell: &ShellKind,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
    ) -> Option<u32> {
        self.spawn_session_inner(shell, cols, rows, cwd, None)
    }

    pub(super) fn spawn_session_with_scrollback(
        &mut self,
        shell: &ShellKind,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
        scrollback: Option<&[u8]>,
    ) -> Option<u32> {
        self.spawn_session_inner(shell, cols, rows, cwd, scrollback)
    }

    fn spawn_session_inner(
        &mut self,
        shell: &ShellKind,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
        pre_inject: Option<&[u8]>,
    ) -> Option<u32> {
        match self.session_manager.spawn(
            cols,
            rows,
            cwd,
            shell,
            self.settings.scrollback_lines,
            pre_inject,
        ) {
            Ok((id, session, master, pty_tx, shell_pid, alive, is_active, injected_lines)) => {
                let entry = SessionEntry {
                    id,
                    session,
                    pty_tx,
                    master,
                    shell_pid,
                    alive,
                    is_active,
                    pending_command: None,
                    shell: shell.clone(),
                    restore_scroll_ready: false,
                    restore_scroll_lines: if injected_lines > 0 {
                        Some(injected_lines)
                    } else {
                        None
                    },
                    restore_title: None,
                    claude_session_id: None,
                };
                if self.session_state.active_id.is_none() {
                    self.session_state.active_id = Some(id);
                }
                self.session_state.uninit_sessions.insert(id);
                self.session_state.sessions.push(entry);
                if self.pane_state.panes.is_empty() {
                    let pane_id = self.pane_state.next_pane_id;
                    self.pane_state.next_pane_id += 1;
                    self.pane_state.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::Terminal(id),
                        manual_width: None,
                        last_size: (cols, rows),
                    });
                    self.pane_state.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (cols, rows),
                        },
                    );
                    self.pane_state.active_pane_id = Some(pane_id);
                }
                Some(id)
            }
            Err(e) => {
                log::error!("Failed to spawn session: {e}");
                self.flash.trigger(
                    super::feedback::FlashTarget::Global,
                    super::feedback::FlashKind::Error,
                );
                None
            }
        }
    }

    pub(super) fn configured_shell(&self) -> ShellKind {
        if let Some(name) = &self.settings.default_shell {
            self.available_shells
                .iter()
                .find(|s| s.display_name() == name)
                .cloned()
                .unwrap_or_else(default_shell)
        } else {
            default_shell()
        }
    }

    pub(super) fn active_session_index(&self) -> Option<usize> {
        let id = self.session_state.active_id?;
        self.session_state.sessions.iter().position(|e| e.id == id)
    }

    pub(super) fn active_cwd(&self) -> Option<PathBuf> {
        let idx = self.active_session_index()?;
        let p = self.session_state.sessions[idx].session.read().cwd.clone();
        if p.as_os_str().is_empty() {
            None
        } else {
            Some(p)
        }
    }

    /// Derive CWD from the active pane, regardless of its content type.
    pub(super) fn active_pane_cwd(&self) -> Option<PathBuf> {
        let pane = self
            .pane_state
            .active_pane_id
            .and_then(|id| self.pane_state.find(id))?;
        match &pane.content {
            PaneContent::Terminal(sid) => self
                .session_state
                .sessions
                .iter()
                .find(|e| e.id == *sid)
                .and_then(|e| {
                    let cwd = e.session.read().cwd.clone();
                    if cwd.as_os_str().is_empty() {
                        None
                    } else {
                        Some(cwd)
                    }
                }),
            PaneContent::DeferredTerminal { cwd, .. } => cwd.clone(),
            PaneContent::FileEditor(ed) => self
                .workspace_path(ed.workspace_id)
                .or_else(|| ed.path.parent().map(|p| p.to_path_buf())),
            PaneContent::FileDiff(d) => {
                let ws = self.workspace_store.find_for_cwd(&d.path);
                ws.map(|w| w.path.clone())
                    .or_else(|| d.path.parent().map(|p| p.to_path_buf()))
            }
            PaneContent::NoteEditor(ne) => self.workspace_path(ne.workspace_id),
            PaneContent::ConflictResolver(cr) => cr.path.parent().map(|p| p.to_path_buf()),
        }
    }

    fn workspace_path(&self, ws_id: Option<u64>) -> Option<PathBuf> {
        ws_id.and_then(|wid| {
            self.workspace_store
                .workspaces
                .iter()
                .find(|w| w.id == wid)
                .map(|w| w.path.clone())
        })
    }

    pub(super) fn pane_group(
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
            PaneContent::DeferredTerminal { cwd, .. } => cwd
                .as_ref()
                .and_then(|c| ws_store.find_for_cwd(c).map(|w| w.id)),
            PaneContent::FileEditor(ed) => ed.workspace_id,
            PaneContent::FileDiff(d) => ws_store.find_for_cwd(&d.path).map(|w| w.id),
            PaneContent::NoteEditor(ne) => ne.workspace_id,
            PaneContent::ConflictResolver(cr) => ws_store.find_for_cwd(&cr.path).map(|w| w.id),
        }
    }

    pub(super) fn active_workspace(&self) -> Option<&Workspace> {
        let ws_id = self.active_group?;
        self.workspace_store
            .workspaces
            .iter()
            .find(|w| w.id == ws_id)
    }

    pub(super) fn open_workspace_in_new_window(&mut self, _ctx: &egui::Context, ws_id: u64) {
        if self.extra_windows.iter().any(|w| w.workspace_id == ws_id) {
            return;
        }

        let ws_name = match self
            .workspace_store
            .workspaces
            .iter()
            .find(|w| w.id == ws_id)
        {
            Some(ws) => ws.name.clone(),
            None => return,
        };

        let win_id = WindowId(self.next_window_id);
        self.next_window_id += 1;

        let viewport_id = egui::ViewportId::from_hash_of(("extra_window", win_id.0));
        let title = format!("{} — Terminal Studio", ws_name);

        if let Some(ws) = self
            .workspace_store
            .workspaces
            .iter_mut()
            .find(|w| w.id == ws_id)
        {
            ws.host_window_id = Some(win_id.clone());
        }
        self.workspace_store.save();

        let mut view = WindowView::new_for_workspace(ws_id);
        view.session_workspace_filter = Some(Some(ws_id));
        let initial_pane = self
            .pane_state
            .panes
            .iter()
            .find(|p| {
                Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
                    == Some(ws_id)
            })
            .map(|p| p.id);
        view.active_pane_id = initial_pane;
        if let Some(pid) = initial_pane {
            view.last_pane_per_group.insert(Some(ws_id), pid);
            if let Some(pane) = self.pane_state.panes.iter().find(|p| p.id == pid) {
                if let PaneContent::Terminal(sid) = pane.content {
                    view.active_id = Some(sid);
                }
            }
        }

        self.extra_windows.push(ExtraWindow {
            id: win_id,
            workspace_id: ws_id,
            viewport_id,
            title,
            inner_size: [1280.0, 800.0],
            view,
            close_requested: false,
        });
        self.save_windows();

        if self.current_window_id.is_none() && self.active_group == Some(ws_id) {
            self.active_group = None;
            self.pane_state.active_pane_id = None;
            self.session_state.active_id = None;
        }
    }

    pub(super) fn swap_view(&mut self, view: &mut WindowView) {
        use std::mem::swap;
        swap(&mut self.active_group, &mut view.active_group);
        swap(
            &mut self.pane_state.active_pane_id,
            &mut view.active_pane_id,
        );
        swap(&mut self.session_state.active_id, &mut view.active_id);
        swap(&mut self.last_pane_per_group, &mut view.last_pane_per_group);
        swap(&mut self.last_focused_sid, &mut view.last_focused_sid);
        swap(&mut self.right_tab, &mut view.right_tab);
        swap(&mut self.shown_md_tabs, &mut view.shown_md_tabs);
        swap(
            &mut self.workspace_panel_ratio,
            &mut view.workspace_panel_ratio,
        );
        swap(
            &mut self.workspace_panel_collapsed,
            &mut view.workspace_panel_collapsed,
        );
        swap(&mut self.notes_panel_ratio, &mut view.notes_panel_ratio);
        swap(
            &mut self.notes_panel_collapsed,
            &mut view.notes_panel_collapsed,
        );
        swap(&mut self.show_left_panel, &mut view.show_left_panel);
        swap(&mut self.show_right_panel, &mut view.show_right_panel);
        swap(&mut self.show_settings, &mut view.show_settings);
        swap(&mut self.show_shortcut_help, &mut view.show_shortcut_help);
        swap(&mut self.show_quick_switcher, &mut view.show_quick_switcher);
        swap(
            &mut self.quick_switcher_query,
            &mut view.quick_switcher_query,
        );
        swap(
            &mut self.quick_switcher_selected_ws,
            &mut view.quick_switcher_selected_ws,
        );
        swap(
            &mut self.quick_switcher_search_active,
            &mut view.quick_switcher_search_active,
        );
        swap(&mut self.workspace_dialog, &mut view.workspace_dialog);
        swap(
            &mut self.workspace_edit_dialog,
            &mut view.workspace_edit_dialog,
        );
        swap(&mut self.active_term_geo, &mut view.active_term_geo);
        swap(&mut self.active_term_ui_id, &mut view.active_term_ui_id);
        swap(&mut self.was_focused, &mut view.was_focused);
        swap(
            &mut self.session_workspace_filter,
            &mut view.session_workspace_filter,
        );
    }

    pub(super) fn save_windows(&self) {
        let Some(path) = windows_data_path() else {
            return;
        };
        let saved: Vec<SavedExtraWindow> = self
            .extra_windows
            .iter()
            .map(|w| SavedExtraWindow {
                id: w.id.clone(),
                workspace_id: w.workspace_id,
                inner_size: w.inner_size,
                workspace_panel_ratio: Some(w.view.workspace_panel_ratio),
                workspace_panel_collapsed: Some(w.view.workspace_panel_collapsed),
                notes_panel_ratio: Some(w.view.notes_panel_ratio),
                notes_panel_collapsed: Some(w.view.notes_panel_collapsed),
            })
            .collect();
        if let Ok(text) = serde_json::to_string_pretty(&saved) {
            if let Err(e) = crate::util::atomic_write(&path, &text) {
                log::error!("failed to save windows data: {e}");
            }
        }
    }

    pub(super) fn load_windows(&mut self) {
        let Some(path) = windows_data_path() else {
            return;
        };
        let Some(saved) = crate::util::safe_json_load::<Vec<SavedExtraWindow>>(&path) else {
            return;
        };
        let mut max_id: u64 = self.next_window_id.saturating_sub(1);
        for s in saved {
            let ws_exists = self
                .workspace_store
                .workspaces
                .iter()
                .any(|w| w.id == s.workspace_id);
            if !ws_exists {
                continue;
            }
            max_id = max_id.max(s.id.0);
            if let Some(ws) = self
                .workspace_store
                .workspaces
                .iter_mut()
                .find(|w| w.id == s.workspace_id)
            {
                ws.host_window_id = Some(s.id.clone());
            }
            let ws_name = self
                .workspace_store
                .workspaces
                .iter()
                .find(|w| w.id == s.workspace_id)
                .map(|w| w.name.clone())
                .unwrap_or_default();
            let title = format!("{} — Terminal Studio", ws_name);
            let mut view = WindowView::new_for_workspace(s.workspace_id);
            view.session_workspace_filter = Some(Some(s.workspace_id));
            if let Some(v) = s.workspace_panel_ratio {
                view.workspace_panel_ratio = v;
            }
            if let Some(v) = s.workspace_panel_collapsed {
                view.workspace_panel_collapsed = v;
            }
            if let Some(v) = s.notes_panel_ratio {
                view.notes_panel_ratio = v;
            }
            if let Some(v) = s.notes_panel_collapsed {
                view.notes_panel_collapsed = v;
            }
            let viewport_id = egui::ViewportId::from_hash_of(("extra_window", s.id.0));
            self.extra_windows.push(ExtraWindow {
                id: s.id,
                workspace_id: s.workspace_id,
                viewport_id,
                title,
                inner_size: s.inner_size,
                view,
                close_requested: false,
            });
        }
        self.next_window_id = max_id + 1;
        self.workspace_store.save();
    }

    pub(super) fn activate_pane(&mut self, pid: u32) {
        self.pane_state.active_pane_id = Some(pid);
        if let Some(pane) = self.pane_state.panes.iter().find(|p| p.id == pid) {
            if let PaneContent::Terminal(sid) = pane.content {
                self.session_state.active_id = Some(sid);
                self.update_is_active_flags();
            }
        }
    }

    pub(super) fn switch_group(&mut self, group: Option<u64>, cols: u16, rows: u16) {
        self.active_group = group;

        if let Some(ws_id) = group {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            if let Some(ws) = self
                .workspace_store
                .workspaces
                .iter_mut()
                .find(|w| w.id == ws_id)
            {
                ws.last_activated = now;
            }
            self.workspace_store.save();
        }

        let panes_in_group: Vec<u32> = self
            .pane_state
            .panes
            .iter()
            .filter(|p| {
                Self::pane_group(&self.session_state.sessions, &self.workspace_store, p) == group
            })
            .map(|p| p.id)
            .collect();

        if let Some(&last_pid) = self.last_pane_per_group.get(&group) {
            if panes_in_group.contains(&last_pid) {
                self.activate_pane(last_pid);
                return;
            }
        }

        if let Some(&first_pid) = panes_in_group.first() {
            self.activate_pane(first_pid);
            return;
        }

        let cwd = group.and_then(|ws_id| {
            self.workspace_store
                .workspaces
                .iter()
                .find(|w| w.id == ws_id)
                .map(|w| w.path.clone())
        });
        if let Some(sid) = self.spawn_session(&default_shell(), cols, rows, cwd) {
            self.session_state.active_id = Some(sid);
            if !self
                .pane_state
                .panes
                .iter()
                .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == sid))
            {
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                self.pane_state.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::Terminal(sid),
                    manual_width: None,
                    last_size: (cols, rows),
                });
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (cols, rows),
                    },
                );
                self.pane_state.active_pane_id = Some(pane_id);
            }
            self.update_is_active_flags();
        }
    }

    /// Navigate to a workspace, respecting multi-window ownership.
    /// If the workspace is hosted in a different extra window, focuses that window.
    /// If in an extra window and the target isn't this window's workspace, routes
    /// to the correct window (main or another extra).
    pub(super) fn navigate_to_workspace(&mut self, ws_id_raw: u64) {
        let group = if ws_id_raw == u64::MAX {
            None
        } else {
            Some(ws_id_raw)
        };

        if let Some(ws_id) = group {
            if let Some((idx, viewport_id, ew_id)) = self
                .extra_windows
                .iter()
                .enumerate()
                .find(|(_, ew)| ew.workspace_id == ws_id)
                .map(|(idx, ew)| (idx, ew.viewport_id, ew.id.clone()))
            {
                if self.current_window_id.as_ref() != Some(&ew_id) {
                    let pane_id = self.extra_windows[idx].view.active_pane_id.or_else(|| {
                        self.pane_state
                            .panes
                            .iter()
                            .find(|p| {
                                Self::pane_group(
                                    &self.session_state.sessions,
                                    &self.workspace_store,
                                    p,
                                ) == group
                            })
                            .map(|p| p.id)
                    });
                    if let Some(pid) = pane_id {
                        self.pending_window_focus = Some(PendingWindowFocus {
                            target_viewport_id: viewport_id,
                            target_window_idx: Some(idx),
                            pane_id: pid,
                            group: Some(ws_id),
                        });
                    }
                    return;
                }
            } else if self.current_window_id.is_some() {
                if let Some(pane_id) = self
                    .pane_state
                    .panes
                    .iter()
                    .find(|p| {
                        Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
                            == group
                    })
                    .map(|p| p.id)
                {
                    self.pending_window_focus = Some(PendingWindowFocus {
                        target_viewport_id: egui::ViewportId::ROOT,
                        target_window_idx: None,
                        pane_id,
                        group: Some(ws_id),
                    });
                }
                return;
            }
        } else if self.current_window_id.is_some() {
            if let Some(pane_id) = self
                .pane_state
                .panes
                .iter()
                .find(|p| {
                    Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
                        .is_none()
                })
                .map(|p| p.id)
            {
                self.pending_window_focus = Some(PendingWindowFocus {
                    target_viewport_id: egui::ViewportId::ROOT,
                    target_window_idx: None,
                    pane_id,
                    group: None,
                });
            }
            return;
        }

        let (cols, rows) = self
            .pane_state
            .panes
            .first()
            .map(|p| p.last_size)
            .unwrap_or((80, 24));
        self.switch_group(group, cols, rows);
    }

    /// Navigate to a specific pane, respecting multi-window ownership.
    pub(super) fn navigate_to_pane(&mut self, pane_id: u32) {
        let group = self
            .pane_state
            .panes
            .iter()
            .find(|p| p.id == pane_id)
            .map(|p| Self::pane_group(&self.session_state.sessions, &self.workspace_store, p));
        let Some(group) = group else { return };

        if let Some(ws_id) = group {
            if let Some((idx, viewport_id, ew_id)) = self
                .extra_windows
                .iter()
                .enumerate()
                .find(|(_, ew)| ew.workspace_id == ws_id)
                .map(|(idx, ew)| (idx, ew.viewport_id, ew.id.clone()))
            {
                if self.current_window_id.as_ref() != Some(&ew_id) {
                    self.pending_window_focus = Some(PendingWindowFocus {
                        target_viewport_id: viewport_id,
                        target_window_idx: Some(idx),
                        pane_id,
                        group: Some(ws_id),
                    });
                    return;
                }
            } else if self.current_window_id.is_some() {
                self.pending_window_focus = Some(PendingWindowFocus {
                    target_viewport_id: egui::ViewportId::ROOT,
                    target_window_idx: None,
                    pane_id,
                    group: Some(ws_id),
                });
                return;
            }
        } else if self.current_window_id.is_some() {
            self.pending_window_focus = Some(PendingWindowFocus {
                target_viewport_id: egui::ViewportId::ROOT,
                target_window_idx: None,
                pane_id,
                group: None,
            });
            return;
        }

        if group != self.active_group {
            let (cols, rows) = self
                .pane_state
                .panes
                .first()
                .map(|p| p.last_size)
                .unwrap_or((80, 24));
            self.switch_group(group, cols, rows);
        }
        self.activate_pane(pane_id);
    }

    /// Close the extra window hosting a workspace and release ownership.
    pub(super) fn close_extra_window_for_workspace(&mut self, ws_id: u64) {
        if let Some(idx) = self
            .extra_windows
            .iter()
            .position(|ew| ew.workspace_id == ws_id)
        {
            self.extra_windows.remove(idx);
        }
        if let Some(ws) = self
            .workspace_store
            .workspaces
            .iter_mut()
            .find(|w| w.id == ws_id)
        {
            ws.host_window_id = None;
        }
        self.workspace_store.save();
        self.save_windows();
    }

    /// Check whether a pane is active in any window (main or extra).
    pub(super) fn is_pane_active_in_any_window(&self, pane_id: u32) -> bool {
        if self.pane_state.active_pane_id == Some(pane_id) {
            return true;
        }
        self.extra_windows
            .iter()
            .any(|ew| ew.view.active_pane_id == Some(pane_id))
    }

    pub(super) fn spawn_session_no_pane(
        &mut self,
        shell: &ShellKind,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
    ) -> Option<u32> {
        match self.session_manager.spawn(
            cols,
            rows,
            cwd,
            shell,
            self.settings.scrollback_lines,
            None,
        ) {
            Ok((id, session, master, pty_tx, shell_pid, alive, is_active, _)) => {
                self.session_state.uninit_sessions.insert(id);
                self.session_state.sessions.push(SessionEntry {
                    id,
                    session,
                    pty_tx,
                    master,
                    shell_pid,
                    alive,
                    is_active,
                    pending_command: None,
                    shell: shell.clone(),
                    restore_scroll_lines: None,
                    restore_scroll_ready: false,
                    restore_title: None,
                    claude_session_id: None,
                });
                Some(id)
            }
            Err(e) => {
                log::error!("Failed to restore session: {e}");
                None
            }
        }
    }

    pub(super) fn restore_closed_session(&mut self, record_id: u64) {
        let manifest = super::closed_sessions::ClosedSessionManifest::load();
        let Some(record) = manifest.records.iter().find(|r| r.id == record_id) else {
            return;
        };

        let scrollback = record
            .scrollback_file
            .as_ref()
            .and_then(|f| super::closed_sessions::load_scrollback(f));

        let shell = crate::pty::ShellKind::from_name(&record.shell);
        let cwd = if record.cwd.as_os_str().is_empty() {
            None
        } else {
            Some(record.cwd.clone())
        };

        let cols = record.cols;
        let rows = record.rows;

        if let Some(sid) =
            self.spawn_session_with_scrollback(&shell, cols, rows, cwd, scrollback.as_deref())
        {
            if let Some(entry) = self.session_state.find_mut(sid) {
                entry.restore_title = Some(record.title.clone());
            }
            self.session_state.active_id = Some(sid);

            if !self
                .pane_state
                .panes
                .iter()
                .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == sid))
            {
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                self.pane_state.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::Terminal(sid),
                    manual_width: None,
                    last_size: (cols, rows),
                });
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (cols, rows),
                    },
                );
                self.activate_pane(pane_id);
            }
            self.update_is_active_flags();
            self.save_session();
        }
    }

    pub(super) fn update_is_active_flags(&self) {
        let active = self.session_state.active_id;
        for entry in &self.session_state.sessions {
            entry
                .is_active
                .store(active == Some(entry.id), Ordering::Relaxed);
        }
    }

    fn save_active_scrollbacks(&self) -> HashMap<u32, String> {
        let mut result = HashMap::new();
        let Some(dir) = crate::util::data_dir().map(|d| d.join("active_scrollback")) else {
            return result;
        };
        std::fs::create_dir_all(&dir).ok();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                std::fs::remove_file(entry.path()).ok();
            }
        }
        for (idx, entry) in self.session_state.sessions.iter().enumerate() {
            let session = entry.session.read();
            let ansi_bytes = super::scrollback_capture::extract_grid_as_ansi(&session.term, None);
            drop(session);
            if ansi_bytes.is_empty() {
                continue;
            }
            let filename = format!("{}.zst", idx);
            let filepath = dir.join(&filename);
            match zstd::encode_all(ansi_bytes.as_slice(), 3) {
                Ok(compressed) => {
                    if std::fs::write(&filepath, &compressed).is_ok() {
                        result.insert(entry.id, filename);
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Failed to compress scrollback for session {}: {}",
                        entry.id,
                        e
                    );
                }
            }
        }
        result
    }

    pub(super) fn save_session(&self) {
        let Some(path) = session_data_path() else {
            return;
        };

        // Save active scrollback if enabled
        let scrollback_files: HashMap<u32, String> = if self.settings.save_scrollback_on_exit {
            self.save_active_scrollbacks()
        } else {
            HashMap::new()
        };

        let session_id_to_index: HashMap<u32, usize> = self
            .session_state
            .sessions
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect();
        let saved_pane_ids: Vec<u32> = self
            .pane_state
            .panes
            .iter()
            .filter(|p| !matches!(p.content, PaneContent::FileDiff(_)))
            .map(|p| p.id)
            .collect();
        let pane_id_to_index: HashMap<u32, usize> = saved_pane_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();

        let sessions = self
            .session_state
            .sessions
            .iter()
            .map(|e| {
                let s = e.session.read();
                let cwd = s.cwd.clone();
                let title_raw = s.title();
                drop(s);
                let command = self.workers.foreground_worker.get(e.id).map(|fp| {
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
                let fg = self.workers.foreground_worker.get(e.id);
                let ws_name = if cwd.as_os_str().is_empty() {
                    None
                } else {
                    self.workspace_store
                        .find_for_cwd(&cwd)
                        .map(|w| w.name.clone())
                };
                let title = Some(effective_title(
                    &title_raw,
                    &cwd,
                    fg.as_ref(),
                    Some(&e.shell),
                    ws_name.as_deref(),
                ));
                let scrollback_file = scrollback_files.get(&e.id).cloned();
                let claude_session_id = self
                    .workers
                    .foreground_worker
                    .get_claude_session_id(e.id)
                    .or_else(|| e.claude_session_id.clone());
                SavedSession {
                    cwd,
                    command,
                    title,
                    scrollback_file,
                    claude_session_id,
                }
            })
            .collect();

        let panes = self
            .pane_state
            .panes
            .iter()
            .filter_map(|p| {
                let content = match &p.content {
                    PaneContent::Terminal(sid) => {
                        let &idx = session_id_to_index.get(sid)?;
                        SavedPaneContent::Terminal { session_index: idx }
                    }
                    PaneContent::DeferredTerminal {
                        cwd,
                        pending_command,
                        saved_title,
                        ..
                    } => SavedPaneContent::DeferredTerminal {
                        cwd: cwd.clone().unwrap_or_default(),
                        command: pending_command.clone(),
                        title: saved_title.clone(),
                    },
                    PaneContent::FileEditor(ed) => SavedPaneContent::FileEditor {
                        path: ed.path.clone(),
                        content: ed.content.clone(),
                        dirty: ed.dirty,
                        workspace_id: ed.workspace_id,
                    },
                    PaneContent::FileDiff(_) => return None,
                    PaneContent::NoteEditor(ne) => SavedPaneContent::NoteEditor {
                        workspace_id: ne.workspace_id,
                    },
                    PaneContent::ConflictResolver(_) => return None,
                };
                Some(SavedPane {
                    content,
                    manual_width: p.manual_width,
                })
            })
            .collect();

        let active_pane_index = self
            .pane_state
            .active_pane_id
            .and_then(|pid| pane_id_to_index.get(&pid).copied());
        let active_session_index = self
            .session_state
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

        if let Ok(text) = serde_json::to_string_pretty(&state) {
            if let Err(e) = crate::util::atomic_write(&path, &text) {
                log::error!("failed to save session state: {e}");
            }
        }
    }

    pub(super) fn restore_session(&mut self) -> bool {
        let Some(path) = session_data_path() else {
            return false;
        };
        let Some(state) = crate::util::safe_json_load::<AppSession>(&path) else {
            return false;
        };
        if state.sessions.is_empty() && state.panes.is_empty() {
            return false;
        }

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
        let scrollback_dir = crate::util::data_dir().map(|d| d.join("active_scrollback"));

        let mut eagerly_spawned: HashMap<usize, u32> = HashMap::new();
        if let Some(active_idx) = active_session_idx {
            if let Some(s) = state.sessions.get(active_idx) {
                let cwd = if s.cwd.as_os_str().is_empty() {
                    None
                } else {
                    Some(s.cwd.clone())
                };
                let scrollback = s.scrollback_file.as_ref().and_then(|f| {
                    let dir = scrollback_dir.as_ref()?;
                    let compressed = std::fs::read(dir.join(f)).ok()?;
                    zstd::decode_all(compressed.as_slice()).ok()
                });
                // Only replay the command if there's no scrollback to restore.
                // When scrollback exists, the user sees the old session output
                // above the new prompt — replaying the command would wipe it
                // (full-screen TUIs like `claude` clear the screen on start).
                let replay_command = scrollback.is_none() || s.scrollback_file.is_none();
                if let Some(sid) = self.spawn_session_with_scrollback(
                    &default_shell(),
                    80,
                    24,
                    cwd,
                    scrollback.as_deref(),
                ) {
                    if let Some(entry) = self.session_state.find_mut(sid) {
                        if replay_command {
                            if let Some(cmd) = s.command.clone() {
                                entry.pending_command = Some(cmd);
                            }
                        }
                        entry.restore_title = s.title.as_ref().filter(|t| !t.is_empty()).cloned();
                        if let Some(ref claude_id) = s.claude_session_id {
                            entry.pending_command = Some(
                                super::claude_session::claude_resume_command(claude_id, &s.cwd),
                            );
                            entry.claude_session_id = Some(claude_id.clone());
                        }
                    }
                    eagerly_spawned.insert(active_idx, sid);
                }
            }
        }

        // Clear any auto-created panes from spawn_session_inner — the
        // pane list is rebuilt from the persisted state below.
        self.pane_state.panes.clear();
        self.pane_state.pane_trees.clear();
        self.pane_state.active_pane_id = None;
        self.pane_state.next_pane_id = 0;

        let mut pane_ids: Vec<u32> = Vec::new();
        for saved in &state.panes {
            let content = match &saved.content {
                SavedPaneContent::Terminal { session_index } => {
                    if let Some(&sid) = eagerly_spawned.get(session_index) {
                        PaneContent::Terminal(sid)
                    } else {
                        let saved = state.sessions.get(*session_index);
                        let cwd = saved.and_then(|s| {
                            if s.cwd.as_os_str().is_empty() {
                                None
                            } else {
                                Some(s.cwd.clone())
                            }
                        });
                        let saved_title = saved.and_then(|s| s.title.clone());
                        let scrollback_file = saved.and_then(|s| s.scrollback_file.clone());
                        let claude_session_id = saved.and_then(|s| s.claude_session_id.clone());
                        let pending_command = if let Some(ref cid) = claude_session_id {
                            let session_cwd = cwd.as_deref().unwrap_or(Path::new(""));
                            Some(super::claude_session::claude_resume_command(
                                cid,
                                session_cwd,
                            ))
                        } else if scrollback_file.is_some() {
                            None
                        } else {
                            saved.and_then(|s| s.command.clone())
                        };
                        PaneContent::DeferredTerminal {
                            cwd,
                            pending_command,
                            saved_title,
                            scrollback_file,
                        }
                    }
                }
                SavedPaneContent::DeferredTerminal {
                    cwd,
                    command,
                    title,
                } => PaneContent::DeferredTerminal {
                    cwd: if cwd.as_os_str().is_empty() {
                        None
                    } else {
                        Some(cwd.clone())
                    },
                    pending_command: command.clone(),
                    saved_title: title.clone(),
                    scrollback_file: None,
                },
                SavedPaneContent::FileEditor {
                    path,
                    content,
                    dirty,
                    workspace_id,
                } => {
                    let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");
                    PaneContent::FileEditor(FileEditorState {
                        path: path.clone(),
                        content: content.clone(),
                        dirty: *dirty,
                        save_error: false,
                        workspace_id: *workspace_id,
                        show_preview: is_md,
                    })
                }
                SavedPaneContent::NoteEditor { workspace_id } => {
                    PaneContent::NoteEditor(NoteEditorState {
                        workspace_id: *workspace_id,
                    })
                }
            };
            let pane_id = self.pane_state.next_pane_id;
            self.pane_state.next_pane_id += 1;
            pane_ids.push(pane_id);
            self.pane_state.panes.push(PaneEntry {
                id: pane_id,
                content,
                manual_width: saved.manual_width,
                last_size: (0, 0),
            });
            self.pane_state.pane_trees.insert(
                pane_id,
                PaneNode::Leaf {
                    pane_id,
                    last_size: (0, 0),
                },
            );
        }

        if let Some(idx) = state.active_pane_index {
            if let Some(&pid) = pane_ids.get(idx) {
                self.activate_pane(pid);
            }
        }
        if self.pane_state.active_pane_id.is_none() {
            if let Some(&pid) = pane_ids.first() {
                self.activate_pane(pid);
            }
        }
        if self.session_state.active_id.is_none() {
            if let Some(&sid) = eagerly_spawned.values().next() {
                self.session_state.active_id = Some(sid);
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

    pub(super) fn track_active_pane_group(&mut self) {
        if let Some(pid) = self.pane_state.active_pane_id {
            if let Some(pane) = self.pane_state.panes.iter().find(|p| p.id == pid) {
                let group =
                    Self::pane_group(&self.session_state.sessions, &self.workspace_store, pane);
                if group == self.active_group {
                    self.last_pane_per_group.insert(self.active_group, pid);
                }
            }
        }
    }

    pub(super) fn apply_theme_visuals(&self, ctx: &egui::Context) {
        use egui::{Rounding, Shadow, Stroke, Visuals};
        let t = theme::active();
        let mut vis = if t.is_light {
            Visuals::light()
        } else {
            Visuals::dark()
        };
        vis.panel_fill = t.bg_panel_fill;
        vis.window_fill = t.bg_term;
        vis.window_rounding = Rounding::same(theme::R_LG);
        vis.window_shadow = Shadow::NONE;
        vis.popup_shadow = Shadow::NONE;
        vis.widgets.noninteractive.bg_fill = t.surface0;
        vis.widgets.inactive.bg_fill = t.surface0;
        vis.widgets.hovered.bg_fill = t.surface1;
        vis.widgets.active.bg_fill = t.surface2;
        vis.widgets.inactive.fg_stroke = Stroke::new(1.0, t.subtext0);
        vis.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.overlay0);
        vis.selection.bg_fill = t.selection_bg;
        for state in [
            &mut vis.widgets.noninteractive,
            &mut vis.widgets.inactive,
            &mut vis.widgets.hovered,
            &mut vis.widgets.active,
            &mut vis.widgets.open,
        ] {
            state.rounding = Rounding::same(theme::R_MD);
        }
        vis.override_text_color = Some(t.text);
        ctx.set_visuals(vis);

        let mut style = (*ctx.style()).clone();
        style.spacing.scroll = egui::style::ScrollStyle {
            floating: true,
            bar_width: 12.0,
            handle_min_length: 20.0,
            bar_inner_margin: theme::SP_1,
            bar_outer_margin: 0.0,
            floating_width: 4.0,
            floating_allocated_width: 0.0,
            foreground_color: true,
            dormant_background_opacity: 0.0,
            active_background_opacity: 0.4,
            interact_background_opacity: 0.7,
            dormant_handle_opacity: 0.0,
            active_handle_opacity: 0.7,
            interact_handle_opacity: 1.0,
        };
        style.always_scroll_the_only_direction = true;
        ctx.set_style(style);
    }

    pub(super) fn extract_selected_text(&self, session_idx: usize) -> String {
        let sel = match &self.term_selection {
            Some(s) => s,
            None => return String::new(),
        };
        let Some(entry) = self.session_state.sessions.get(session_idx) else {
            return String::new();
        };
        let (sc, sr, ec, er) = sel.ordered();
        let session = entry.session.read();
        let term = &session.term;
        let grid = term.grid();
        let term_cols = term.columns();
        let term_rows = term.screen_lines();
        let display_offset = grid.display_offset();

        let mut result = String::new();
        for screen_row in sr..=er {
            let grid_line = screen_row as i32 - display_offset as i32;
            if grid_line < -(grid.history_size() as i32) || grid_line >= term_rows as i32 {
                continue;
            }
            let row_start = if screen_row == sr { sc as usize } else { 0 };
            let row_end = if screen_row == er {
                (ec as usize + 1).min(term_cols)
            } else {
                term_cols
            };
            let mut line = String::new();
            for col in row_start..row_end {
                let cell = &grid[alacritty_terminal::index::Line(grid_line)]
                    [alacritty_terminal::index::Column(col)];
                if cell
                    .flags
                    .contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER)
                {
                    continue;
                }
                line.push(cell.c);
            }
            let trimmed = line.trim_end();
            result.push_str(trimmed);
            if screen_row < er {
                result.push('\n');
            }
        }
        result
    }

    pub(super) fn paint_sys_monitor(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        fg: egui::Color32,
    ) {
        let stats = self
            .workers
            .sys_monitor
            .as_ref()
            .map(|m| m.stats())
            .unwrap_or_default();
        let bar_h = 5.0_f32;
        let bar_w = rect.width() - 8.0;
        let x0 = rect.min.x + 4.0;

        let font = egui::FontId::monospace(theme::FONT_SYS_SM);
        let t = theme::active();

        struct Row<'a> {
            label: &'a str,
            pct: f32,
            color: egui::Color32,
        }
        let rows = [
            Row {
                label: "C",
                pct: stats.cpu_percent,
                color: t.blue,
            },
            Row {
                label: "M",
                pct: stats.ram_percent,
                color: t.green,
            },
        ];

        let row_h = 11.0_f32;
        let y_start = rect.min.y + (rect.height() - row_h * rows.len() as f32 - 10.0) / 2.0;

        for (i, row) in rows.iter().enumerate() {
            let y = y_start + i as f32 * row_h;

            let pct_label = format!("{} {:2.0}%", row.label, row.pct);
            painter.text(
                egui::pos2(x0, y + row_h * 0.5),
                egui::Align2::LEFT_CENTER,
                &pct_label,
                font.clone(),
                fg,
            );

            let track_x = x0 + 32.0;
            let track = egui::Rect::from_min_size(
                egui::pos2(track_x, y + (row_h - bar_h) / 2.0),
                egui::vec2(bar_w - 32.0, bar_h),
            );
            painter.rect_filled(track, theme::R_SM, t.surface2);

            let fill_w = (track.width() * (row.pct / 100.0).clamp(0.0, 1.0)).max(0.0);
            if fill_w > 0.5 {
                let fill = egui::Rect::from_min_size(track.min, egui::vec2(fill_w, bar_h));
                painter.rect_filled(fill, theme::R_SM, row.color);
            }
        }

        let net_text = format!(
            "↓{} ↑{}",
            crate::sys_monitor::format_bytes_rate(stats.net_rx_per_sec),
            crate::sys_monitor::format_bytes_rate(stats.net_tx_per_sec),
        );
        painter.text(
            egui::pos2(rect.center().x, rect.max.y - 2.0),
            egui::Align2::CENTER_BOTTOM,
            &net_text,
            egui::FontId::monospace(theme::FONT_SYS_XS),
            fg.linear_multiply(0.7),
        );
    }

    // ── Drag-and-drop action execution ──────────────────────────────────

    pub(super) fn execute_drag_action(&mut self, action: drag::DragAction, ctx: &egui::Context) {
        use drag::DragAction;
        match action {
            DragAction::Noop => {}
            DragAction::ReorderTab {
                from_pane_id,
                to_index,
            } => {
                if let Some(from) = self
                    .pane_state
                    .panes
                    .iter()
                    .position(|p| p.id == from_pane_id)
                {
                    let pane = self.pane_state.panes.remove(from);
                    let insert_at = if to_index > from {
                        to_index - 1
                    } else {
                        to_index
                    };
                    let insert_at = insert_at.min(self.pane_state.panes.len());
                    self.pane_state.panes.insert(insert_at, pane);
                }
            }
            DragAction::ExtractFromSplitAndInsert { pane_id, to_index } => {
                if let Some(root) = self.pane_state.root_of(pane_id) {
                    if let Some(tree) = self.pane_state.pane_trees.get_mut(&root) {
                        let result = tree.remove_pane(pane_id);
                        if let RemoveResult::CollapseToSibling(replacement) = result {
                            *tree = replacement;
                            let new_root = tree.first_leaf_id();
                            if new_root != root {
                                if let Some(old_tree) = self.pane_state.pane_trees.remove(&root) {
                                    self.pane_state.pane_trees.insert(new_root, old_tree);
                                }
                            }
                        }
                    }
                }
                if let Some(from) = self.pane_state.panes.iter().position(|p| p.id == pane_id) {
                    let pane = self.pane_state.panes.remove(from);
                    let insert_at = to_index.min(self.pane_state.panes.len());
                    self.pane_state.panes.insert(insert_at, pane);
                }
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (0, 0),
                    },
                );
                self.activate_pane(pane_id);
            }
            DragAction::InsertTerminalPane {
                session_id,
                at_index,
            } => {
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                let entry = PaneEntry {
                    id: pane_id,
                    content: PaneContent::Terminal(session_id),
                    manual_width: None,
                    last_size: (0, 0),
                };
                self.insert_pane_entry(entry, at_index);
                self.activate_pane(pane_id);
            }
            DragAction::InsertFileEditorPane { path, at_index } => {
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");
                let entry = PaneEntry {
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
                };
                self.insert_pane_entry(entry, at_index);
                self.activate_pane(pane_id);
                let results = std::sync::Arc::clone(&self.file_load_results);
                let ctx_clone = ctx.clone();
                std::thread::spawn(move || {
                    let content = std::fs::read_to_string(&path).unwrap_or_default();
                    results.lock().push((pane_id, content));
                    ctx_clone.request_repaint();
                });
            }
            DragAction::InsertDiffPane { rel_path, at_index } => {
                if let Some(cwd) = self.active_cwd() {
                    let full_path = cwd.join(&rel_path);
                    let pane_id = self.pane_state.next_pane_id;
                    self.pane_state.next_pane_id += 1;
                    let entry = PaneEntry {
                        id: pane_id,
                        content: PaneContent::FileDiff(FileDiffState {
                            path: full_path.clone(),
                            old_content: String::new(),
                            new_content: String::new(),
                            hunks: Vec::new(),
                            diff_mode: self.settings.diff_view_mode,
                        }),
                        manual_width: None,
                        last_size: (0, 0),
                    };
                    self.insert_pane_entry(entry, at_index);
                    self.pending_diff_panes.insert(full_path, pane_id);
                    self.workers.git_worker.enqueue_diff(&cwd, rel_path);
                    self.activate_pane(pane_id);
                }
            }
            DragAction::InsertNotePane {
                workspace_id,
                at_index,
            } => {
                let ws_id = if workspace_id == 0 {
                    None
                } else {
                    Some(workspace_id)
                };
                let existing = self
                    .pane_state
                    .panes
                    .iter()
                    .find(|p| {
                        matches!(&p.content, PaneContent::NoteEditor(ne) if ne.workspace_id == ws_id)
                    })
                    .map(|p| p.id);
                if let Some(pid) = existing {
                    self.activate_pane(pid);
                } else {
                    let pane_id = self.pane_state.next_pane_id;
                    self.pane_state.next_pane_id += 1;
                    let entry = PaneEntry {
                        id: pane_id,
                        content: PaneContent::NoteEditor(NoteEditorState {
                            workspace_id: ws_id,
                        }),
                        manual_width: None,
                        last_size: (0, 0),
                    };
                    self.insert_pane_entry(entry, at_index);
                    self.activate_pane(pane_id);
                }
            }
            DragAction::FocusExistingTab { pane_id } => {
                self.activate_pane(pane_id);
            }
            DragAction::OpenWorkspaceWindow { workspace_id } => {
                if let Some(ew) = self
                    .extra_windows
                    .iter()
                    .find(|w| w.workspace_id == workspace_id)
                {
                    ctx.send_viewport_cmd_to(ew.viewport_id, egui::ViewportCommand::Focus);
                } else {
                    self.open_workspace_in_new_window(ctx, workspace_id);
                }
            }
        }
        self.save_session();
    }

    fn insert_pane_entry(&mut self, entry: PaneEntry, at_index: Option<usize>) {
        let pane_id = entry.id;
        match at_index {
            Some(idx) => {
                let idx = idx.min(self.pane_state.panes.len());
                self.pane_state.panes.insert(idx, entry);
            }
            None => self.pane_state.panes.push(entry),
        }
        self.pane_state.pane_trees.insert(
            pane_id,
            PaneNode::Leaf {
                pane_id,
                last_size: (0, 0),
            },
        );
    }
}
