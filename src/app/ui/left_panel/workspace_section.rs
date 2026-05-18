use super::WorkspaceSectionActions;
use super::super::super::App;
use crate::theme;
use crate::workspace::WindowId;

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
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let arrow = if self.workspace_panel_collapsed {
                            "\u{25b6}"
                        } else {
                            "\u{25bc}"
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
                            .on_hover_text("Ctrl+Shift+PgUp / PgDn to switch")
                            .clicked()
                        {
                            self.workspace_panel_collapsed =
                                !self.workspace_panel_collapsed;
                        }
                    },
                );
            },
        );
    }

    /// Render the scrollable list of workspace cards and the "Other" group.
    fn render_workspace_list(
        &mut self,
        ui: &mut egui::Ui,
        actions: &mut WorkspaceSectionActions,
    ) {
        let active_group_snap = self.active_group;
        let cur_win = self.current_window_id.clone();

        // Snapshot: (id, name, color, has_note, in_extra_window)
        let workspaces: Vec<(u64, String, [u8; 3], bool, bool)> = self
            .workspace_store
            .workspaces
            .iter()
            .filter(|w| match (&cur_win, &w.host_window_id) {
                (None, _) => true,
                (Some(this), Some(host)) => this == host,
                (Some(_), None) => false,
            })
            .map(|w| {
                let in_extra = w.host_window_id.is_some()
                    && self
                        .extra_windows
                        .iter()
                        .any(|ew| ew.workspace_id == w.id)
                    && cur_win.is_none();
                (
                    w.id,
                    w.name.clone(),
                    w.color,
                    !self.note_store.get(Some(w.id)).is_empty(),
                    in_extra,
                )
            })
            .collect();

        egui::ScrollArea::vertical()
            .id_source("ws_panel_scroll")
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = theme::SP_SM;
                for (id, name, color, has_note, in_extra_window) in &workspaces {
                    self.render_workspace_card(
                        ui,
                        actions,
                        *id,
                        name,
                        *color,
                        *has_note,
                        *in_extra_window,
                        active_group_snap,
                        &cur_win,
                    );
                }

                // "Other" group
                self.render_other_group(
                    ui,
                    actions,
                    active_group_snap,
                    &cur_win,
                );
            });
    }

    /// Render a single workspace card with name, gear icon, and context menu.
    #[allow(clippy::too_many_arguments)]
    fn render_workspace_card(
        &self,
        ui: &mut egui::Ui,
        actions: &mut WorkspaceSectionActions,
        id: u64,
        name: &str,
        color: [u8; 3],
        has_note: bool,
        in_extra_window: bool,
        active_group_snap: Option<u64>,
        cur_win: &Option<WindowId>,
    ) {
        let active = active_group_snap == Some(id);
        let tint_factor = if in_extra_window {
            0.20
        } else if active {
            0.65
        } else {
            0.45
        };
        let fill = theme::from_rgb(theme::tinted(color, tint_factor));
        let fg = if in_extra_window {
            theme::active().overlay0
        } else {
            theme::text_on(theme::tinted(color, tint_factor))
        };

        const GEAR_W: f32 = 26.0;
        let full_w = ui.available_width();
        let stroke_val = if active {
            egui::Stroke::new(theme::STROKE_BOLD, theme::active().text)
        } else {
            egui::Stroke::new(
                1.0,
                theme::from_rgb(theme::tinted(color, 0.30)),
            )
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
        let name_resp = ui.interact(
            name_rect,
            egui::Id::new(("ws_name", id)),
            egui::Sense::click_and_drag(),
        );
        let gear_resp = ui.interact(
            gear_rect,
            egui::Id::new(("ws_gear", id)),
            egui::Sense::click(),
        );

        if ui.is_rect_visible(full_rect) {
            let rounding = egui::Rounding::same(theme::ROUNDING);
            ui.painter().rect_filled(full_rect, rounding, fill);
            ui.painter().rect_stroke(full_rect, rounding, stroke_val);

            let name_str = if in_extra_window {
                format!("\u{2192} {} (other window)", name)
            } else if active {
                format!("\u{25b6} {}", name)
            } else {
                name.to_string()
            };
            let name_galley = ui.fonts(|f| {
                f.layout_no_wrap(
                    name_str,
                    egui::FontId::proportional(theme::SESSION_FONT_SZ),
                    fg,
                )
            });
            let text_y = full_rect.center().y - name_galley.size().y / 2.0;
            ui.painter().with_clip_rect(name_rect).galley(
                egui::pos2(full_rect.left() + theme::BAR_PAD_X, text_y),
                name_galley,
                fg,
            );

            if has_note {
                let note_galley = ui.fonts(|f| {
                    f.layout_no_wrap(
                        "\u{1f4dd}".to_string(),
                        egui::FontId::proportional(12.0),
                        fg,
                    )
                });
                let note_x = gear_rect.left() - 4.0 - note_galley.size().x;
                ui.painter().galley(
                    egui::pos2(note_x, text_y),
                    note_galley,
                    fg,
                );
            }

            let gear_fg = if gear_resp.hovered() {
                theme::active().text
            } else {
                theme::active().subtext0
            };
            ui.painter().text(
                gear_rect.center(),
                egui::Align2::CENTER_CENTER,
                "\u{2699}",
                egui::FontId::proportional(12.0),
                gear_fg,
            );
        }

        name_resp.clone().on_hover_text(name);
        if name_resp.clicked() && !in_extra_window {
            actions.open_workspace_id = Some(id);
        }
        if gear_resp.clicked() {
            actions.edit_workspace_id = Some(id);
        }
        let in_main = cur_win.is_none();
        name_resp.context_menu(|ui| {
            let enabled = !in_extra_window;
            if ui
                .add_enabled(enabled, egui::Button::new("Open workspace"))
                .clicked()
            {
                actions.open_workspace_id = Some(id);
                ui.close_menu();
            }
            if in_main
                && !in_extra_window
                && ui.button("Open in new window").clicked()
            {
                actions.new_window_workspace_id = Some(id);
                ui.close_menu();
            }
            ui.separator();
            if ui.button("Edit workspace\u{2026}").clicked() {
                actions.edit_workspace_id = Some(id);
                ui.close_menu();
            }
        });
    }

    /// Render the "Other" (unaffiliated) workspace group at the bottom of the list.
    fn render_other_group(
        &self,
        ui: &mut egui::Ui,
        actions: &mut WorkspaceSectionActions,
        active_group_snap: Option<u64>,
        cur_win: &Option<WindowId>,
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
            ui.allocate_exact_size(
                egui::vec2(other_w, 28.0),
                egui::Sense::click(),
            )
        } else {
            (
                egui::Rect::NOTHING,
                ui.interact(
                    egui::Rect::NOTHING,
                    egui::Id::new("other_skip"),
                    egui::Sense::hover(),
                ),
            )
        };
        if show_other && ui.is_rect_visible(other_rect) {
            let rounding = egui::Rounding::same(theme::ROUNDING);
            ui.painter().rect_filled(other_rect, rounding, other_fill);
            ui.painter()
                .rect_stroke(other_rect, rounding, other_stroke);

            let other_name = if other_active {
                "\u{25b6} Other".to_string()
            } else {
                "Other".to_string()
            };
            let other_galley = ui.fonts(|f| {
                f.layout_no_wrap(
                    other_name,
                    egui::FontId::proportional(13.0),
                    other_fg,
                )
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
                ui.painter().galley(
                    egui::pos2(note_x, text_y),
                    note_galley,
                    other_fg,
                );
            }
        }
        if show_other && other_resp.clicked() {
            actions.open_workspace_id = Some(u64::MAX); // sentinel for "Other"
        }
    }
}
