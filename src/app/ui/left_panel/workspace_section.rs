use std::collections::HashSet;

use super::super::super::App;
use super::{section_header, WorkspaceSectionActions};
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
    last_activated: u64,
    git_branch: String,
    git_diff_count: usize,
}

fn render_workspace_git_row(ui: &mut egui::Ui, branch: &str, changed_count: usize, width: f32) {
    let t = theme::active();
    let font = egui::FontId::proportional(theme::FONT_XS);
    ui.allocate_ui_with_layout(
        egui::vec2(width.max(theme::SP_0), theme::GIT_ROW_H),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = theme::SP_1;
            if changed_count > 0 {
                ui.label(
                    egui::RichText::new(format!("{changed_count} changed"))
                        .font(font.clone())
                        .color(t.warning),
                );
            }
            if !branch.is_empty() {
                if changed_count > 0 {
                    ui.label(
                        egui::RichText::new("\u{00b7}")
                            .font(font.clone())
                            .color(t.fg_muted),
                    );
                }
                crate::ui_kit::truncate_label(ui, branch, font, t.fg_muted, ui.available_width());
            }
        },
    );
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
                ui.label(section_header("WORKSPACES", ws_count));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let arrow = if self.workspace_panel_collapsed {
                        ui_kit::icon::CHEVRON_RIGHT
                    } else {
                        ui_kit::icon::CHEVRON_DOWN
                    };
                    let collapse_resp = ui.add(
                        egui::Button::new(ui_kit::icon(
                            arrow,
                            theme::FONT_SM,
                            theme::active().fg_muted,
                        ))
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
            ui_kit::icon::SEARCH,
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
                    last_activated: w.last_activated,
                    git_branch: git_info
                        .as_ref()
                        .map(|i| i.branch.clone())
                        .unwrap_or_default(),
                    git_diff_count: git_info.map(|i| i.diff_count).unwrap_or(0),
                }
            })
            .collect();

        workspaces.sort_by(|a, b| match (a.has_active_session, b.has_active_session) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (true, true) => b.last_activated.cmp(&a.last_activated),
            (false, false) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });

        // Apply search filter
        if !search_filter.is_empty() {
            workspaces.retain(|w| w.name.to_lowercase().contains(&search_filter));
        }

        let outer_w = ui.available_width();
        ui.spacing_mut().scroll.bar_width = 0.0;
        ui.spacing_mut().scroll.floating_allocated_width = 0.0;
        ui.spacing_mut().scroll.floating_width = 0.0;
        egui::ScrollArea::vertical()
            .id_source(self.vp_id("ws_panel_scroll"))
            .show(ui, |ui| {
                let outer_w = outer_w.max(0.0);
                ui.set_min_width(outer_w);
                ui.set_max_width(outer_w);
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

        let row_resp = ui_kit::selectable_row(ui, active, card_h, |_| {});
        let full_rect = row_resp.rect;
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

        let t = theme::active();
        let fg = if active { t.fg_strong } else { t.fg };

        if ui.is_rect_visible(full_rect) {
            ui.painter().circle_filled(
                egui::pos2(
                    full_rect.left() + theme::SP_3,
                    full_rect.min.y + theme::HEADER_H / 2.0,
                ),
                theme::SP_2,
                theme::from_rgb(data.color),
            );

            // Name text
            let name_x = full_rect.left() + theme::SP_6;
            let name_max_w = (gear_rect.left() - theme::SP_2 - name_x).max(theme::SP_0);
            let name_galley = ui_kit::truncate_galley(
                ui.painter(),
                &data.name,
                egui::FontId::proportional(theme::FONT_MD),
                fg,
                name_max_w,
            );
            let name_h = name_galley.size().y;
            let name_y = if has_git_row {
                full_rect.min.y + (theme::HEADER_H - name_h) / 2.0 - theme::SP_1
            } else {
                full_rect.center().y - name_h / 2.0
            };
            ui.painter()
                .galley(egui::pos2(name_x, name_y), name_galley, fg);

            // Note icon (interactive — supports drag to open as pane)
            if data.has_note {
                let note_galley = ui.fonts(|f| {
                    f.layout_no_wrap(
                        ui_kit::icon::PENCIL.to_string(),
                        egui::FontId::new(theme::FONT_SM, egui::FontFamily::Name("icons".into())),
                        t.fg_muted,
                    )
                });
                let note_x = gear_rect.left() - theme::SP_2 - note_galley.size().x;
                let note_y = full_rect.min.y + (theme::HEADER_H - note_galley.size().y) / 2.0;
                let note_rect =
                    egui::Rect::from_min_size(egui::pos2(note_x, note_y), note_galley.size());
                ui.painter()
                    .galley(egui::pos2(note_x, note_y), note_galley, t.fg_muted);
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
                        format!("Note: {}", data.name),
                    );
                }
            }

            // Gear icon — always visible (muted), brightens on hover
            {
                let gear_fg = if gear_resp.hovered() {
                    t.fg
                } else {
                    t.fg_muted
                };
                ui.painter().text(
                    egui::pos2(
                        gear_rect.center().x,
                        full_rect.min.y + theme::HEADER_H / 2.0,
                    ),
                    egui::Align2::CENTER_CENTER,
                    ui_kit::icon::SETTINGS,
                    egui::FontId::new(theme::FONT_SM, egui::FontFamily::Name("icons".into())),
                    gear_fg,
                );
            }

            // Git info row
            if has_git_row {
                let git_rect = egui::Rect::from_min_max(
                    egui::pos2(name_x, full_rect.min.y + theme::HEADER_H),
                    egui::pos2(gear_rect.left() - theme::SP_2, full_rect.bottom()),
                );
                ui.allocate_ui_at_rect(git_rect, |ui| {
                    render_workspace_git_row(
                        ui,
                        &data.git_branch,
                        data.git_diff_count,
                        git_rect.width(),
                    );
                });
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
        let other_resp = ui_kit::selectable_row(ui, other_active, theme::HEADER_H, |_| {});
        let other_rect = other_resp.rect;
        let t = theme::active();
        let other_fg = if other_active { t.fg_strong } else { t.fg };
        if ui.is_rect_visible(other_rect) {
            let other_galley = ui.fonts(|f| {
                f.layout_no_wrap(
                    "Other".to_string(),
                    egui::FontId::proportional(theme::FONT_MD),
                    other_fg,
                )
            });
            let text_y = other_rect.center().y - other_galley.size().y / 2.0;
            ui.painter().galley(
                egui::pos2(other_rect.left() + theme::SP_4, text_y),
                other_galley,
                other_fg,
            );

            if other_has_note {
                let note_galley = ui.fonts(|f| {
                    f.layout_no_wrap(
                        ui_kit::icon::PENCIL.to_string(),
                        egui::FontId::new(theme::FONT_SM, egui::FontFamily::Name("icons".into())),
                        t.fg_muted,
                    )
                });
                let note_x = other_rect.right() - theme::SP_4 - note_galley.size().x;
                ui.painter()
                    .galley(egui::pos2(note_x, text_y), note_galley, t.fg_muted);
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

#[cfg(test)]
mod tests {
    use crate::theme::{self, ThemeId};
    use crate::ui_kit::test_harness::{ctx_with_fonts, run_ui};

    fn painted_text(out: &egui::FullOutput) -> Vec<String> {
        fn collect(shape: &egui::Shape, text: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text_shape) => {
                    text.extend(
                        text_shape
                            .galley
                            .rows
                            .iter()
                            .map(|row| row.text().to_owned()),
                    );
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, text);
                    }
                }
                _ => {}
            }
        }

        let mut text = Vec::new();
        for clipped in &out.shapes {
            collect(&clipped.shape, &mut text);
        }
        text
    }

    #[test]
    fn workspace_card_changed_count_truncates() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let out = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::render_workspace_git_row(ui, "feature/very-long-branch-name-here", 12, 160.0);
        });
        let text = painted_text(&out);

        assert!(
            text.iter()
                .any(|value| value.ends_with('\u{2026}') && value.starts_with("feature/")),
            "branch should truncate with an ellipsis: {text:?}"
        );
        assert!(
            text.iter().any(|value| value == "12 changed"),
            "changed count should remain whole: {text:?}"
        );
    }
}
