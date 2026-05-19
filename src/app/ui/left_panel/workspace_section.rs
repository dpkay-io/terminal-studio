use std::collections::HashSet;

use super::super::super::App;
use super::WorkspaceSectionActions;
use crate::theme;
use crate::workspace::WindowId;

struct WorkspaceCardData {
    id: u64,
    name: String,
    color: [u8; 3],
    has_note: bool,
    in_extra_window: bool,
    has_active_session: bool,
    git_branch: String,
    git_diff_count: usize,
}

const GIT_ROW_H: f32 = 14.0;
const GIT_FONT_SZ: f32 = 10.0;

impl App {
    /// Render the workspace section: header with collapse toggle, workspace
    /// cards, and the "Other" group.
    pub(in crate::app) fn render_workspace_section(
        &mut self,
        ui: &mut egui::Ui,
        ws_rect: egui::Rect,
        actions: &mut WorkspaceSectionActions,
    ) {
        ui.painter()
            .rect_filled(ws_rect, 0.0, theme::active().bg_workspace_fill);

        let ws_count = self.workspace_store.workspaces.len();
        self.render_workspace_header(ui, ws_count);

        if !self.workspace_panel_collapsed {
            self.render_workspace_list(ui, actions);
        }
    }

    /// Render the workspace header row with collapse/expand toggle.
    fn render_workspace_header(&mut self, ui: &mut egui::Ui, ws_count: usize) {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), theme::HEADER_H),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new(format!("Workspaces ({})", ws_count))
                        .strong()
                        .size(theme::HEADER_FONT_SZ),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let arrow = if self.workspace_panel_collapsed {
                        "\u{25b6}"
                    } else {
                        "\u{25bc}"
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(arrow).size(theme::HEADER_FONT_SZ),
                            )
                            .min_size(egui::vec2(theme::HEADER_H, theme::HEADER_H))
                            .frame(false),
                        )
                        .on_hover_text("Ctrl+Shift+PgUp / PgDn to switch")
                        .clicked()
                    {
                        self.workspace_panel_collapsed = !self.workspace_panel_collapsed;
                    }
                });
            },
        );
    }

    /// Render the scrollable list of workspace cards and the "Other" group.
    fn render_workspace_list(&mut self, ui: &mut egui::Ui, actions: &mut WorkspaceSectionActions) {
        let active_group_snap = self.active_group;
        let cur_win = self.current_window_id.clone();

        // Which workspaces have panes open across any window
        let active_ws_ids: HashSet<u64> = self
            .pane_state
            .panes
            .iter()
            .filter_map(|p| {
                Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
            })
            .collect();
        let has_active_other = self.pane_state.panes.iter().any(|p| {
            Self::pane_group(&self.session_state.sessions, &self.workspace_store, p).is_none()
        });

        // Trigger lazy git info fetches for all workspaces
        for w in &self.workspace_store.workspaces {
            self.workers
                .workspace_git_worker
                .request_if_stale(w.id, &w.path);
        }

        // Build snapshot with git info and active status
        let mut workspaces: Vec<WorkspaceCardData> = self
            .workspace_store
            .workspaces
            .iter()
            .filter(|w| match (&cur_win, &w.host_window_id) {
                (None, _) => true,
                (Some(this), Some(host)) => this == host,
                (Some(_), None) => false,
            })
            .map(|w| {
                let git_info = self.workers.workspace_git_worker.get(w.id);
                let in_extra = w.host_window_id.is_some()
                    && self.extra_windows.iter().any(|ew| ew.workspace_id == w.id)
                    && cur_win.is_none();
                WorkspaceCardData {
                    id: w.id,
                    name: w.name.clone(),
                    color: w.color,
                    has_note: !self.note_store.get(Some(w.id)).is_empty(),
                    in_extra_window: in_extra,
                    has_active_session: active_ws_ids.contains(&w.id),
                    git_branch: git_info
                        .as_ref()
                        .map(|i| i.branch.clone())
                        .unwrap_or_default(),
                    git_diff_count: git_info.map(|i| i.diff_count).unwrap_or(0),
                }
            })
            .collect();

        // Sort: case-insensitive alphabetical (digits/symbols naturally precede letters)
        workspaces.sort_by_key(|a| a.name.to_lowercase());

        egui::ScrollArea::vertical()
            .id_source(self.vp_id("ws_panel_scroll"))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = theme::SP_SM;

                // "Other" group at the top
                self.render_other_group(ui, actions, active_group_snap, &cur_win, has_active_other);

                for card in &workspaces {
                    self.render_workspace_card(ui, actions, card, active_group_snap, &cur_win);
                }
            });
    }

    /// Render a single workspace card with name, git info row, and indicators.
    fn render_workspace_card(
        &self,
        ui: &mut egui::Ui,
        actions: &mut WorkspaceSectionActions,
        data: &WorkspaceCardData,
        active_group_snap: Option<u64>,
        cur_win: &Option<WindowId>,
    ) {
        let active = active_group_snap == Some(data.id);
        let has_git_row = !data.git_branch.is_empty() || data.git_diff_count > 0;
        let card_h = if has_git_row {
            theme::HEADER_H + GIT_ROW_H
        } else {
            theme::HEADER_H
        };

        let tint_factor = if data.in_extra_window {
            0.20
        } else if active {
            0.65
        } else {
            0.45
        };
        let fill = theme::from_rgb(theme::tinted(data.color, tint_factor));
        let fg = if data.in_extra_window {
            theme::active().overlay0
        } else {
            theme::text_on(theme::tinted(data.color, tint_factor))
        };

        const GEAR_W: f32 = 26.0;
        let full_w = ui.available_width();
        let stroke_val = if active {
            egui::Stroke::new(theme::STROKE_BOLD, theme::active().text)
        } else {
            egui::Stroke::new(1.0, theme::from_rgb(theme::tinted(data.color, 0.30)))
        };
        let (full_rect, _) =
            ui.allocate_exact_size(egui::vec2(full_w, card_h), egui::Sense::hover());
        let gear_rect = egui::Rect::from_min_size(
            egui::pos2(full_rect.max.x - GEAR_W, full_rect.min.y),
            egui::vec2(GEAR_W, theme::HEADER_H),
        );
        let name_rect = egui::Rect::from_min_max(
            full_rect.min,
            egui::pos2(gear_rect.min.x, full_rect.min.y + theme::HEADER_H),
        );
        let name_resp = ui.interact(
            name_rect,
            egui::Id::new(("ws_name", data.id)),
            egui::Sense::click_and_drag(),
        );
        let gear_resp = ui.interact(
            gear_rect,
            egui::Id::new(("ws_gear", data.id)),
            egui::Sense::click(),
        );

        if ui.is_rect_visible(full_rect) {
            let rounding = egui::Rounding::same(theme::ROUNDING);
            ui.painter().rect_filled(full_rect, rounding, fill);
            ui.painter().rect_stroke(full_rect, rounding, stroke_val);

            // Active session indicator: colored bar on left edge
            if data.has_active_session {
                let bar =
                    egui::Rect::from_min_size(full_rect.min, egui::vec2(3.0, full_rect.height()));
                let left_rounding = egui::Rounding {
                    nw: theme::ROUNDING,
                    sw: theme::ROUNDING,
                    ne: 0.0,
                    se: 0.0,
                };
                ui.painter()
                    .rect_filled(bar, left_rounding, theme::active().green);
            }

            // Name text
            let name_str = if data.in_extra_window {
                format!("\u{2192} {} (other window)", data.name)
            } else if active {
                format!("\u{25b6} {}", data.name)
            } else {
                data.name.clone()
            };
            let name_galley = ui.fonts(|f| {
                f.layout_no_wrap(
                    name_str,
                    egui::FontId::proportional(theme::SESSION_FONT_SZ),
                    fg,
                )
            });
            let name_h = name_galley.size().y;
            let name_y = if has_git_row {
                full_rect.min.y + (theme::HEADER_H - name_h) / 2.0 - 1.0
            } else {
                full_rect.center().y - name_h / 2.0
            };
            ui.painter().with_clip_rect(name_rect).galley(
                egui::pos2(full_rect.left() + theme::BAR_PAD_X, name_y),
                name_galley,
                fg,
            );

            // Note icon
            if data.has_note {
                let note_galley = ui.fonts(|f| {
                    f.layout_no_wrap(
                        "\u{1f4dd}".to_string(),
                        egui::FontId::proportional(12.0),
                        fg,
                    )
                });
                let note_x = gear_rect.left() - 4.0 - note_galley.size().x;
                let note_y = full_rect.min.y + (theme::HEADER_H - note_galley.size().y) / 2.0;
                ui.painter()
                    .galley(egui::pos2(note_x, note_y), note_galley, fg);
            }

            // Gear icon
            let gear_fg = if gear_resp.hovered() {
                theme::active().text
            } else {
                theme::active().subtext0
            };
            ui.painter().text(
                egui::pos2(
                    gear_rect.center().x,
                    full_rect.min.y + theme::HEADER_H / 2.0,
                ),
                egui::Align2::CENTER_CENTER,
                "\u{2699}",
                egui::FontId::proportional(12.0),
                gear_fg,
            );

            // Git info row
            if has_git_row {
                let mut git_text = String::new();
                if !data.git_branch.is_empty() {
                    git_text.push_str(&data.git_branch);
                }
                if data.git_diff_count > 0 {
                    if !git_text.is_empty() {
                        git_text.push_str(" \u{00b7} ");
                    }
                    git_text.push_str(&format!("{} changed", data.git_diff_count));
                }
                let git_fg = fg.linear_multiply(0.65);
                let git_galley = ui.fonts(|f| {
                    f.layout_no_wrap(git_text, egui::FontId::proportional(GIT_FONT_SZ), git_fg)
                });
                let git_y = full_rect.min.y + theme::HEADER_H - 2.0;
                ui.painter().with_clip_rect(full_rect).galley(
                    egui::pos2(full_rect.left() + theme::BAR_PAD_X, git_y),
                    git_galley,
                    git_fg,
                );
            }
        }

        name_resp.clone().on_hover_text(&data.name);
        if name_resp.clicked() && !data.in_extra_window {
            actions.open_workspace_id = Some(data.id);
        }
        if gear_resp.clicked() {
            actions.edit_workspace_id = Some(data.id);
        }
        let in_main = cur_win.is_none();
        name_resp.context_menu(|ui| {
            let enabled = !data.in_extra_window;
            if ui
                .add_enabled(enabled, egui::Button::new("Open workspace"))
                .clicked()
            {
                actions.open_workspace_id = Some(data.id);
                ui.close_menu();
            }
            if in_main && !data.in_extra_window && ui.button("Open in new window").clicked() {
                actions.new_window_workspace_id = Some(data.id);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Edit workspace\u{2026}").clicked() {
                actions.edit_workspace_id = Some(data.id);
                ui.close_menu();
            }
        });
    }

    /// Render the "Other" (unaffiliated) workspace group at the top of the list.
    fn render_other_group(
        &self,
        ui: &mut egui::Ui,
        actions: &mut WorkspaceSectionActions,
        active_group_snap: Option<u64>,
        cur_win: &Option<WindowId>,
        has_active_session: bool,
    ) {
        let show_other = cur_win.is_none();
        let other_active = active_group_snap.is_none();
        let other_has_note = !self.note_store.get(None).is_empty();
        let other_fill = if other_active {
            theme::active().surface2
        } else {
            theme::active().surface0
        };
        let other_fg = if other_active {
            theme::active().text
        } else {
            theme::active().subtext0
        };
        let other_stroke = if other_active {
            egui::Stroke::new(theme::STROKE_BOLD, theme::active().text)
        } else {
            egui::Stroke::new(theme::STROKE_THIN, theme::active().overlay0)
        };
        let other_w = ui.available_width();
        let (other_rect, other_resp) = if show_other {
            ui.allocate_exact_size(egui::vec2(other_w, 28.0), egui::Sense::click())
        } else {
            (
                egui::Rect::NOTHING,
                ui.interact(
                    egui::Rect::NOTHING,
                    self.vp_id("other_skip"),
                    egui::Sense::hover(),
                ),
            )
        };
        if show_other && ui.is_rect_visible(other_rect) {
            let rounding = egui::Rounding::same(theme::ROUNDING);
            ui.painter().rect_filled(other_rect, rounding, other_fill);
            ui.painter().rect_stroke(other_rect, rounding, other_stroke);

            // Active session indicator bar
            if has_active_session {
                let bar =
                    egui::Rect::from_min_size(other_rect.min, egui::vec2(3.0, other_rect.height()));
                let left_rounding = egui::Rounding {
                    nw: theme::ROUNDING,
                    sw: theme::ROUNDING,
                    ne: 0.0,
                    se: 0.0,
                };
                ui.painter()
                    .rect_filled(bar, left_rounding, theme::active().green);
            }

            let other_name = if other_active {
                "\u{25b6} Other".to_string()
            } else {
                "Other".to_string()
            };
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
                    f.layout_no_wrap(
                        "\u{1f4dd}".to_string(),
                        egui::FontId::proportional(12.0),
                        other_fg,
                    )
                });
                let note_x = other_rect.right() - 8.0 - note_galley.size().x;
                ui.painter()
                    .galley(egui::pos2(note_x, text_y), note_galley, other_fg);
            }
        }
        if show_other && other_resp.clicked() {
            actions.open_workspace_id = Some(u64::MAX); // sentinel for "Other"
        }
    }
}
