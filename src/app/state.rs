use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crate::pane_tree::PaneNode;
use crate::pty::foreground_worker::ForegroundWorker;
use crate::pty::{available_shells, default_shell, SessionManager, ShellKind};
use crate::shortcuts::ShortcutRegistry;
use crate::sys_monitor::SysMonitor;
use crate::theme;
use crate::updater::UpdateChecker;
use crate::workspace::{NoteStore, WindowId, Workspace, WorkspaceStore};

use alacritty_terminal::grid::Dimensions;

use super::multi_window::{ExtraWindow, SavedExtraWindow, WindowView};
use super::pane::{FileEditorState, PaneContent, PaneEntry, RightTab, SessionEntry};
use super::persistence::{
    session_data_path, AppSession, SavedPane, SavedPaneContent, SavedRightTab, SavedSession,
};
use super::settings::{windows_data_path, AppSettings};
use super::title::shell_escape_arg;
use super::watcher::WatchState;
use super::App;

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
            vis.window_rounding = Rounding::same(6.0);
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
                state.rounding = Rounding::same(4.0);
            }
            vis.override_text_color = Some(t.text);
            cc.egui_ctx.set_visuals(vis);
        }

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
        let loaded_settings = AppSettings::load();
        let mut app = App {
            session_manager: mgr,
            sessions: vec![],
            active_id: None,
            panes: vec![],
            active_pane_id: None,
            next_pane_id: 0,
            right_tab: RightTab::Directory,
            shown_md_tabs: HashSet::new(),
            watch_state: WatchState::new(ctx.clone()),
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
            show_shortcut_help: false,
            show_quick_switcher: false,
            quick_switcher_query: String::new(),
            shortcut_registry: ShortcutRegistry::new(),
            update_checker: UpdateChecker::spawn(ctx.clone(), loaded_settings.last_update_check),
            settings: loaded_settings,
            active_term_geo: None,
            last_focused_sid: None,
            active_term_ui_id: None,
            resize_debounce: HashMap::new(),
            scroll_accum: HashMap::new(),
            foreground_worker: ForegroundWorker::spawn(),
            was_focused: true,
            available_shells: available_shells(),
            uninit_sessions: HashSet::new(),
            cursor_blink_on: true,
            cursor_blink_last: Instant::now(),
            term_selection: None,
            term_selecting: false,
            term_selection_sid: None,
            extra_windows: Vec::new(),
            next_window_id: 1,
            current_window_id: None,
            pane_trees: HashMap::new(),
            next_split_id: 1,
            subdir_cache: HashMap::new(),
            last_title_sent: None,
            session_search_query: String::new(),
            session_search_active: false,
            dir_search_query: String::new(),
            dir_search_active: false,
            dir_search_debounce_query: String::new(),
            dir_search_debounce_at: None,
            sys_monitor: SysMonitor::spawn(ctx.clone(), Duration::from_secs(2)),
            git_worker: super::git_worker::GitWorker::spawn(ctx.clone()),
            md_prefer_preview: false,
            term_search: crate::search::SearchState::new(),
            detected_urls: Vec::new(),
            tab_drag_source: None,
            deferred_spawn: None,
            deferred_duplicate: false,
            deferred_open_workspace: None,
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
        match self.session_manager.spawn(cols, rows, cwd, shell, self.settings.scrollback_lines) {
            Ok((id, session, master, pty_tx, shell_pid, alive, is_active)) => {
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
                    self.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (cols, rows),
                        },
                    );
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
        let id = self.active_id?;
        self.sessions.iter().position(|e| e.id == id)
    }

    pub(super) fn active_cwd(&self) -> Option<PathBuf> {
        let idx = self.active_session_index()?;
        let p = self.sessions[idx].session.read().cwd.clone();
        if p.as_os_str().is_empty() {
            None
        } else {
            Some(p)
        }
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
        let initial_pane = self
            .panes
            .iter()
            .find(|p| Self::pane_group(&self.sessions, &self.workspace_store, p) == Some(ws_id))
            .map(|p| p.id);
        view.active_pane_id = initial_pane;
        if let Some(pid) = initial_pane {
            view.last_pane_per_group.insert(Some(ws_id), pid);
            if let Some(pane) = self.panes.iter().find(|p| p.id == pid) {
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
            self.active_pane_id = None;
            self.active_id = None;
        }
    }

    pub(super) fn swap_view(&mut self, view: &mut WindowView) {
        use std::mem::swap;
        swap(&mut self.active_group, &mut view.active_group);
        swap(&mut self.active_pane_id, &mut view.active_pane_id);
        swap(&mut self.active_id, &mut view.active_id);
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
        swap(&mut self.quick_switcher_query, &mut view.quick_switcher_query);
        swap(&mut self.workspace_dialog, &mut view.workspace_dialog);
        swap(
            &mut self.workspace_edit_dialog,
            &mut view.workspace_edit_dialog,
        );
        swap(&mut self.active_term_geo, &mut view.active_term_geo);
        swap(&mut self.active_term_ui_id, &mut view.active_term_ui_id);
        swap(&mut self.was_focused, &mut view.was_focused);
    }

    pub(super) fn save_windows(&self) {
        let Some(path) = windows_data_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
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
            let _ = std::fs::write(path, text);
        }
    }

    pub(super) fn load_windows(&mut self) {
        let Some(path) = windows_data_path() else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let saved: Vec<SavedExtraWindow> = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => return,
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
        self.active_pane_id = Some(pid);
        if let Some(pane) = self.panes.iter().find(|p| p.id == pid) {
            if let PaneContent::Terminal(sid) = pane.content {
                self.active_id = Some(sid);
                self.update_is_active_flags();
            }
        }
    }

    pub(super) fn switch_group(&mut self, group: Option<u64>, cols: u16, rows: u16) {
        self.active_group = group;

        let panes_in_group: Vec<u32> = self
            .panes
            .iter()
            .filter(|p| Self::pane_group(&self.sessions, &self.workspace_store, p) == group)
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
            self.active_id = Some(sid);
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
                self.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (cols, rows),
                    },
                );
                self.active_pane_id = Some(pane_id);
            }
            self.update_is_active_flags();
        }
    }

    pub(super) fn spawn_session_no_pane(
        &mut self,
        shell: &ShellKind,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
    ) -> Option<u32> {
        match self.session_manager.spawn(cols, rows, cwd, shell, self.settings.scrollback_lines) {
            Ok((id, session, master, pty_tx, shell_pid, alive, is_active)) => {
                self.uninit_sessions.insert(id);
                self.sessions.push(SessionEntry {
                    id,
                    session,
                    pty_tx,
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

    pub(super) fn update_is_active_flags(&self) {
        let active = self.active_id;
        for entry in &self.sessions {
            entry
                .is_active
                .store(active == Some(entry.id), Ordering::Relaxed);
        }
    }

    pub(super) fn save_session(&self) {
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
                let command = self.foreground_worker.get(e.id).map(|fp| {
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
            .filter(|p| !matches!(&p.content, PaneContent::FileDiff(_)))
            .map(|p| SavedPane {
                content: match &p.content {
                    PaneContent::Terminal(sid) => SavedPaneContent::Terminal {
                        session_index: session_id_to_index.get(sid).copied().unwrap_or(0),
                    },
                    PaneContent::DeferredTerminal {
                        cwd,
                        pending_command,
                    } => SavedPaneContent::DeferredTerminal {
                        cwd: cwd.clone().unwrap_or_default(),
                        command: pending_command.clone(),
                    },
                    PaneContent::FileEditor(ed) => SavedPaneContent::FileEditor {
                        path: ed.path.clone(),
                        content: ed.content.clone(),
                        dirty: ed.dirty,
                        workspace_id: ed.workspace_id,
                    },
                    PaneContent::FileDiff(_) => unreachable!(),
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

    pub(super) fn restore_session(&mut self) -> bool {
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
                        let cwd = state.sessions.get(*session_index).and_then(|s| {
                            if s.cwd.as_os_str().is_empty() {
                                None
                            } else {
                                Some(s.cwd.clone())
                            }
                        });
                        let pending_command = state
                            .sessions
                            .get(*session_index)
                            .and_then(|s| s.command.clone());
                        PaneContent::DeferredTerminal {
                            cwd,
                            pending_command,
                        }
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
            };
            let pane_id = self.next_pane_id;
            self.next_pane_id += 1;
            pane_ids.push(pane_id);
            self.panes.push(PaneEntry {
                id: pane_id,
                content,
                manual_width: saved.manual_width,
                last_size: (0, 0),
            });
            self.pane_trees.insert(
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

    pub(super) fn track_active_pane_group(&mut self) {
        if let Some(pid) = self.active_pane_id {
            if let Some(pane) = self.panes.iter().find(|p| p.id == pid) {
                let group = Self::pane_group(&self.sessions, &self.workspace_store, pane);
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
        vis.window_rounding = Rounding::same(6.0);
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
            state.rounding = Rounding::same(4.0);
        }
        vis.override_text_color = Some(t.text);
        ctx.set_visuals(vis);
    }

    pub(super) fn extract_selected_text(&self, session_idx: usize) -> String {
        let sel = match &self.term_selection {
            Some(s) => s,
            None => return String::new(),
        };
        let (sc, sr, ec, er) = sel.ordered();
        let session = self.sessions[session_idx].session.read();
        let term = &session.term;
        let grid = term.grid();
        let term_cols = term.columns();
        let term_rows = term.screen_lines();
        let display_offset = grid.display_offset();

        let mut result = String::new();
        for screen_row in sr..=er {
            let grid_line = screen_row as i32 - display_offset as i32;
            if grid_line < 0 || grid_line >= term_rows as i32 {
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
                if cell.flags.contains(alacritty_terminal::term::cell::Flags::WIDE_CHAR_SPACER) {
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
        let stats = self.sys_monitor.stats();
        let bar_h = 5.0_f32;
        let bar_w = rect.width() - 8.0;
        let x0 = rect.min.x + 4.0;

        let font = egui::FontId::monospace(9.0);
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
            painter.rect_filled(track, 2.0, t.surface2);

            let fill_w = (track.width() * (row.pct / 100.0).clamp(0.0, 1.0)).max(0.0);
            if fill_w > 0.5 {
                let fill = egui::Rect::from_min_size(track.min, egui::vec2(fill_w, bar_h));
                painter.rect_filled(fill, 2.0, row.color);
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
            egui::FontId::monospace(8.0),
            fg.linear_multiply(0.7),
        );
    }
}
