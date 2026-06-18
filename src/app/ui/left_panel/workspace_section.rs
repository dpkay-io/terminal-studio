use std::collections::HashSet;

use super::super::super::App;
use super::WorkspaceSectionActions;
use crate::app::ui::search_bar::search_bar_persistent;
use crate::theme;
use crate::ui_kit;

struct WorkspaceCardData {
    id: u64,
    name: String,
    color: [u8; 3],
    has_note: bool,
    other_window_viewport: Option<egui::ViewportId>,
    has_active_session: bool,
    git_branch: String,
    git_diff_count: usize,
}

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
                        .size(theme::FONT_UI_MD),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let arrow = if self.workspace_panel_collapsed {
                        "\u{25b6}"
                    } else {
                        "\u{25bc}"
                    };
                    let collapse_resp = ui.add(
                        egui::Button::new(egui::RichText::new(arrow).size(theme::FONT_UI_MD))
                            .min_size(egui::vec2(theme::HEADER_H, theme::HEADER_H))
                            .frame(false),
                    );
                    if collapse_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if collapse_resp
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

        // Search bar
        let search_id = self.vp_id("workspace_search_input");
        let sb = search_bar_persistent(
            ui,
            &mut self.workspace_search_query,
            "\u{1f50d}",
            "Filter workspaces\u{2026}",
            search_id,
            false,
        );
        if sb.escaped {
            self.workspace_search_query.clear();
        }
        ui.add_space(theme::SP_1);

        let search_filter = self.workspace_search_query.to_lowercase();

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

        // Build snapshot with git info and active status — show all workspaces in every window
        let cur_win = &self.current_window_id;
        let mut workspaces: Vec<WorkspaceCardData> = self
            .workspace_store
            .workspaces
            .iter()
            .map(|w| {
                let git_info = self.workers.workspace_git_worker.get(w.id);
                let other_vp = if let Some(ew) =
                    self.extra_windows.iter().find(|ew| ew.workspace_id == w.id)
                {
                    if cur_win.as_ref() == Some(&ew.id) {
                        None
                    } else {
                        Some(ew.viewport_id)
                    }
                } else if cur_win.is_some() {
                    Some(egui::ViewportId::ROOT)
                } else {
                    None
                };
                WorkspaceCardData {
                    id: w.id,
                    name: w.name.clone(),
                    color: w.color,
                    has_note: !self.note_store.get(Some(w.id)).is_empty(),
                    other_window_viewport: other_vp,
                    has_active_session: active_ws_ids.contains(&w.id),
                    git_branch: git_info
                        .as_ref()
                        .map(|i| i.branch.clone())
                        .unwrap_or_default(),
                    git_diff_count: git_info.map(|i| i.diff_count).unwrap_or(0),
                }
            })
            .collect();

        // Sort: opened workspaces first, then alphabetical within each group
        workspaces.sort_by(|a, b| {
            b.has_active_session
                .cmp(&a.has_active_session)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        // Apply search filter
        if !search_filter.is_empty() {
            workspaces.retain(|w| w.name.to_lowercase().contains(&search_filter));
        }

        egui::ScrollArea::vertical()
            .id_source(self.vp_id("ws_panel_scroll"))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = theme::SP_2;

                if has_active_other && search_filter.is_empty() {
                    self.render_other_group(ui, actions, active_group_snap, has_active_other);
                }

                for card in &workspaces {
                    self.render_workspace_card(ui, actions, card, active_group_snap);
                }
            });
    }

    /// Render a single workspace card with name, git info row, and indicators.
    fn render_workspace_card(
        &mut self,
        ui: &mut egui::Ui,
        actions: &mut WorkspaceSectionActions,
        data: &WorkspaceCardData,
        active_group_snap: Option<u64>,
    ) {
        let active = active_group_snap == Some(data.id);
        let has_git_row = !data.git_branch.is_empty() || data.git_diff_count > 0;
        let card_h = if has_git_row {
            theme::HEADER_H + theme::GIT_ROW_H
        } else {
            theme::HEADER_H
        };

        let base_tint_factor = if active {
            theme::TINT_ACTIVE
        } else {
            theme::TINT_INACTIVE
        };

        let full_w = ui.available_width();
        let stroke_val = if active {
            egui::Stroke::new(theme::STROKE_THIN, theme::active().border_subtle)
        } else {
            egui::Stroke::NONE
        };
        let (full_rect, _) =
            ui.allocate_exact_size(egui::vec2(full_w, card_h), egui::Sense::hover());
        let gear_rect = egui::Rect::from_min_size(
            egui::pos2(full_rect.max.x - theme::CARD_GEAR_W, full_rect.min.y),
            egui::vec2(theme::CARD_GEAR_W, theme::HEADER_H),
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
        if name_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let gear_resp = ui.interact(
            gear_rect,
            egui::Id::new(("ws_gear", data.id)),
            egui::Sense::click(),
        );
        if gear_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        // Animated hover tint: brighten card background when hovered
        let card_hover_id = egui::Id::new(("ws_card_hover", data.id));
        let card_hovered = name_resp.hovered() || gear_resp.hovered();
        let hover_t =
            crate::app::ui::animation::animated_hover(ui.ctx(), card_hover_id, card_hovered);
        let hover_boost = 0.08_f32;
        let effective_tint = (base_tint_factor + hover_boost * hover_t).min(1.0);
        let fill = theme::from_rgb(theme::tinted(data.color, effective_tint));
        let fg = theme::text_on(theme::tinted(data.color, effective_tint));

        if ui.is_rect_visible(full_rect) {
            let rounding = egui::Rounding::same(theme::R_MD);
            ui.painter().rect_filled(full_rect, rounding, fill);
            ui.painter().rect_stroke(full_rect, rounding, stroke_val);

            // Active session indicator: colored bar on left edge
            if data.has_active_session {
                let left_rounding = egui::Rounding {
                    nw: theme::R_MD,
                    sw: theme::R_MD,
                    ne: 0.0,
                    se: 0.0,
                };
                ui_kit::active_bar(ui.painter(), full_rect, left_rounding);
            }

            // Name text
            let name_str = data.name.clone();
            let name_galley = ui.fonts(|f| {
                f.layout_no_wrap(name_str, egui::FontId::proportional(theme::FONT_UI_MD), fg)
            });
            let name_h = name_galley.size().y;
            let name_y = if has_git_row {
                full_rect.min.y + (theme::HEADER_H - name_h) / 2.0 - 1.0
            } else {
                full_rect.center().y - name_h / 2.0
            };
            let name_x = full_rect.left() + theme::SP_4 + theme::CARD_BAR_W;
            ui.painter().with_clip_rect(name_rect).galley(
                egui::pos2(name_x, name_y),
                name_galley,
                fg,
            );

            // Note icon (interactive — supports drag to open as pane)
            if data.has_note {
                let note_galley = ui.fonts(|f| {
                    f.layout_no_wrap(
                        "\u{1f4dd}".to_string(),
                        egui::FontId::proportional(theme::FONT_UI_MD),
                        fg,
                    )
                });
                let note_x = gear_rect.left() - 4.0 - note_galley.size().x;
                let note_y = full_rect.min.y + (theme::HEADER_H - note_galley.size().y) / 2.0;
                let note_rect =
                    egui::Rect::from_min_size(egui::pos2(note_x, note_y), note_galley.size());
                ui.painter()
                    .galley(egui::pos2(note_x, note_y), note_galley, fg);
                let note_resp = ui.interact(
                    note_rect,
                    egui::Id::new(("ws_note", data.id)),
                    egui::Sense::click_and_drag(),
                );
                if note_resp.drag_started() {
                    let origin = note_resp.interact_pointer_pos().unwrap_or_default();
                    self.drag_state.set_payload(
                        crate::app::drag::DragPayload::Note(data.id),
                        origin,
                        format!("\u{1f4dd} {}", &data.name),
                    );
                }
            }

            // Gear icon — fades in on card hover, brightens on direct gear hover
            let gear_anim_t = crate::app::ui::animation::animated_hover(
                ui.ctx(),
                egui::Id::new(("ws_gear_anim", data.id)),
                card_hovered,
            );
            if gear_anim_t > 0.01 {
                let gear_fg = if gear_resp.hovered() {
                    theme::active().text
                } else {
                    theme::active().subtext0
                };
                if gear_resp.hovered() {
                    ui.painter()
                        .rect_filled(gear_rect, theme::R_SM, theme::active().bg_row_hover);
                }
                ui.painter().text(
                    egui::pos2(
                        gear_rect.center().x,
                        full_rect.min.y + theme::HEADER_H / 2.0,
                    ),
                    egui::Align2::CENTER_CENTER,
                    "\u{2699}",
                    egui::FontId::proportional(theme::FONT_UI_MD),
                    gear_fg.gamma_multiply(gear_anim_t),
                );
            }

            // Git info row
            if has_git_row {
                let mut git_job = egui::text::LayoutJob::default();
                let git_font = egui::FontId::proportional(theme::GIT_FONT_SZ);
                let muted_fg = fg.gamma_multiply(0.6);
                if !data.git_branch.is_empty() {
                    git_job.append(
                        &data.git_branch,
                        0.0,
                        egui::TextFormat {
                            font_id: git_font.clone(),
                            color: muted_fg,
                            ..Default::default()
                        },
                    );
                }
                if data.git_diff_count > 0 {
                    if !data.git_branch.is_empty() {
                        git_job.append(
                            " \u{00b7} ",
                            0.0,
                            egui::TextFormat {
                                font_id: git_font.clone(),
                                color: muted_fg,
                                ..Default::default()
                            },
                        );
                    }
                    git_job.append(
                        &format!("{} changed", data.git_diff_count),
                        0.0,
                        egui::TextFormat {
                            font_id: git_font.clone(),
                            color: theme::active().warning,
                            ..Default::default()
                        },
                    );
                }
                let git_galley = ui.fonts(|f| f.layout_job(git_job));
                let git_y = full_rect.min.y + theme::HEADER_H - 2.0;
                ui.painter().with_clip_rect(full_rect).galley(
                    egui::pos2(full_rect.left() + theme::SP_3, git_y),
                    git_galley,
                    egui::Color32::PLACEHOLDER,
                );
            }
        }

        name_resp.clone().on_hover_text(&data.name);
        if name_resp.drag_started() {
            let origin = name_resp.interact_pointer_pos().unwrap_or_default();
            self.drag_state.set_payload(
                crate::app::drag::DragPayload::Workspace(data.id),
                origin,
                &data.name,
            );
        }
        if name_resp.clicked() {
            if let Some(vp) = data.other_window_viewport {
                actions.focus_extra_window_viewport = Some(vp);
            } else {
                actions.open_workspace_id = Some(data.id);
            }
        }
        if gear_resp.clicked() {
            actions.edit_workspace_id = Some(data.id);
        }
        name_resp.context_menu(|ui| {
            if let Some(vp) = data.other_window_viewport {
                if ui.button("Focus window").clicked() {
                    actions.focus_extra_window_viewport = Some(vp);
                    ui.close_menu();
                }
                if ui.button("Open here").clicked() {
                    actions.reclaim_workspace_id = Some(data.id);
                    ui.close_menu();
                }
            } else {
                if ui.button("Open workspace").clicked() {
                    actions.open_workspace_id = Some(data.id);
                    ui.close_menu();
                }
                if ui.button("Open in new window").clicked() {
                    actions.new_window_workspace_id = Some(data.id);
                    ui.close_menu();
                }
            }
            ui.separator();
            if ui.button("Edit workspace\u{2026}").clicked() {
                actions.edit_workspace_id = Some(data.id);
                ui.close_menu();
            }
            if data.has_active_session
                && ui
                    .add(egui::Button::new(
                        egui::RichText::new("Close all sessions").color(theme::active().danger_fg),
                    ))
                    .clicked()
            {
                actions.close_all_workspace_id = Some(Some(data.id));
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
        has_active_session: bool,
    ) {
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
        let (other_rect, other_resp) =
            ui.allocate_exact_size(egui::vec2(other_w, 28.0), egui::Sense::click());
        if other_resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if ui.is_rect_visible(other_rect) {
            let rounding = egui::Rounding::same(theme::R_MD);
            ui.painter().rect_filled(other_rect, rounding, other_fill);
            ui.painter().rect_stroke(other_rect, rounding, other_stroke);

            if has_active_session {
                let left_rounding = egui::Rounding {
                    nw: theme::R_MD,
                    sw: theme::R_MD,
                    ne: 0.0,
                    se: 0.0,
                };
                ui_kit::active_bar(ui.painter(), other_rect, left_rounding);
            }

            let other_name = if other_active {
                "\u{25b6} Other".to_string()
            } else {
                "Other".to_string()
            };
            let other_galley = ui.fonts(|f| {
                f.layout_no_wrap(
                    other_name,
                    egui::FontId::proportional(theme::FONT_UI_LG),
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
                        egui::FontId::proportional(theme::FONT_UI_MD),
                        other_fg,
                    )
                });
                let note_x = other_rect.right() - 8.0 - note_galley.size().x;
                ui.painter()
                    .galley(egui::pos2(note_x, text_y), note_galley, other_fg);
            }
        }
        if other_resp.clicked() {
            actions.open_workspace_id = Some(u64::MAX);
        }
        if has_active_session {
            other_resp.context_menu(|ui| {
                if ui
                    .add(egui::Button::new(
                        egui::RichText::new("Close all sessions").color(theme::active().danger_fg),
                    ))
                    .clicked()
                {
                    actions.close_all_workspace_id = Some(None);
                    ui.close_menu();
                }
            });
        }
    }
}
