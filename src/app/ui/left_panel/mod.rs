mod context_menu;
mod session_list;
mod workspace_section;

use super::super::App;
use crate::pty::foreground::ForegroundProcess;
use crate::pty::ShellKind;
use crate::theme;
use crate::ui_kit;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

/// Deferred actions collected while rendering the session list section.
pub(in crate::app) struct SessionListActions {
    pub spawn_new_session: Option<ShellKind>,
    pub spawn_new_session_cwd: Option<(ShellKind, PathBuf)>,
    pub duplicate_session: bool,
    pub quit_pane_id: Option<u32>,
    pub clicked_sidebar_pane_id: Option<u32>,
    pub open_folder_path: Option<PathBuf>,
}

/// Deferred actions collected while rendering the workspace section.
pub(in crate::app) struct WorkspaceSectionActions {
    pub open_workspace_id: Option<u64>,
    pub edit_workspace_id: Option<u64>,
    pub new_window_workspace_id: Option<u64>,
    pub focus_extra_window_viewport: Option<egui::ViewportId>,
    /// Close the extra window hosting this workspace, then open it in the current window.
    pub reclaim_workspace_id: Option<u64>,
    /// Close all sessions belonging to this workspace (with confirmation).
    pub close_all_workspace_id: Option<Option<u64>>,
}

impl App {
    pub(in crate::app) fn render_left_panel(&mut self, ctx: &egui::Context) {
        // ── Foreground process detection (background worker, 500 ms poll) ────
        // Update the worker's session list so it polls the right PIDs, then
        // read instantly from the shared cache — never blocks the UI thread.
        {
            let pids: Vec<(u32, u32)> = self
                .session_state
                .sessions
                .iter()
                .filter(|e| e.alive.load(Ordering::Relaxed))
                .map(|e| (e.id, e.shell_pid))
                .collect();
            self.workers.foreground_worker.set_sessions(pids);
        }
        let active_fg: Option<ForegroundProcess> = self
            .session_state
            .active_id
            .and_then(|sid| self.workers.foreground_worker.get(sid));

        // ── Left panel: sessions (top) + workspaces (bottom) ───────────────
        let mut sess_actions = SessionListActions {
            spawn_new_session: self.deferred_spawn.take(),
            spawn_new_session_cwd: None,
            duplicate_session: std::mem::replace(&mut self.deferred_duplicate, false),
            quit_pane_id: None,
            clicked_sidebar_pane_id: None,
            open_folder_path: None,
        };
        if let Some(ws_id) = self.deferred_open_workspace.take() {
            self.navigate_to_workspace(ws_id);
        }
        let mut ws_actions = WorkspaceSectionActions {
            open_workspace_id: None,
            edit_workspace_id: None,
            new_window_workspace_id: None,
            focus_extra_window_viewport: None,
            reclaim_workspace_id: None,
            close_all_workspace_id: None,
        };

        if self.show_left_panel {
            egui::SidePanel::left(self.vp_id("sessions"))
                .default_width(theme::LEFT_SIDEBAR_W)
                .width_range(80.0..=400.0)
                .resizable(true)
                .frame(
                    egui::Frame::none()
                        .inner_margin(egui::Margin::symmetric(theme::SP_3, theme::SP_0)),
                )
                .show(ctx, |ui| {
                    let panel_rect = ui.max_rect();
                    let panel_w = panel_rect.width();
                    let total_h = panel_rect.height();

                    const COLLAPSED_H: f32 = theme::HEADER_H;

                    // ── Height allocation ──────────────────────────────────────
                    let (sess_h, ws_h) = if self.workspace_panel_collapsed {
                        (total_h - COLLAPSED_H - theme::PANEL_DIV_H, COLLAPSED_H)
                    } else {
                        let wh = (total_h * self.workspace_panel_ratio).max(60.0);
                        let sh = (total_h - wh - theme::PANEL_DIV_H).max(60.0);
                        (sh, wh)
                    };

                    // Claim the full panel rect so egui's layout system doesn't
                    // re-use this space for anything else.
                    ui.allocate_rect(panel_rect, egui::Sense::hover());

                    // ── Sessions section ───────────────────────────────────────
                    let sess_rect =
                        egui::Rect::from_min_size(panel_rect.min, egui::vec2(panel_w, sess_h));
                    ui.allocate_ui_at_rect(sess_rect, |ui| {
                        self.render_session_section(ctx, ui, &active_fg, &mut sess_actions);
                    });

                    // ── Draggable divider ──────────────────────────────────────
                    let div_top = panel_rect.min.y + sess_h;
                    let div_rect = egui::Rect::from_min_size(
                        egui::pos2(panel_rect.left(), div_top),
                        egui::vec2(panel_w, theme::PANEL_DIV_H),
                    );
                    let delta = ui_kit::drag_divider(
                        ui,
                        self.vp_id("ws_panel_divider"),
                        div_rect,
                        theme::active().ws_div_idle,
                        theme::active().ws_div_active,
                    );
                    if !self.workspace_panel_collapsed && delta != 0.0 {
                        let new_ws_h =
                            (ws_h - delta).clamp(60.0, total_h - 60.0 - theme::PANEL_DIV_H);
                        self.workspace_panel_ratio = new_ws_h / total_h;
                    }

                    // ── Workspaces section ─────────────────────────────────────
                    let ws_top = div_top + theme::PANEL_DIV_H;
                    let ws_rect = egui::Rect::from_min_size(
                        egui::pos2(panel_rect.left(), ws_top),
                        egui::vec2(panel_w, ws_h),
                    );
                    ui.allocate_ui_at_rect(ws_rect, |ui| {
                        self.render_workspace_section(ui, ws_rect, &mut ws_actions);
                    });
                });
        } // end if self.show_left_panel

        // Workspace drag → NewWindow default target (overridden if pointer is
        // over the tab bar or pane area later in the render order).
        if self.drag_state.is_active() {
            if matches!(
                &self.drag_state.payload,
                Some(crate::app::drag::DragPayload::Workspace(_))
            ) {
                if ctx.input(|i| i.pointer.hover_pos()).is_some() {
                    self.drag_state.drop_target =
                        Some(crate::app::drag::DropTarget::NewWindow);
                }
            }
        }

        // ── Process deferred actions ──────────────────────────────────────────
        self.process_left_panel_actions(ctx, sess_actions, ws_actions, active_fg);
    }
}
