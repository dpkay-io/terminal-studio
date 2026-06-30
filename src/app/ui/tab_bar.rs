use super::super::drag;
use super::super::pane::PaneContent;
use super::super::title::effective_title;
use super::super::App;
use crate::editor_group::{EditorGroup, GroupId};
use crate::pane_tree::SplitDir;
use crate::theme;
use crate::ui_kit;

/// Result of rendering the tab bar, consumed by the caller.
#[allow(dead_code)]
pub(in crate::app) struct TabBarResult {
    pub close_pane_id: Option<u32>,
    pub clicked_pane_id: Option<u32>,
    pub split_request: Option<SplitDir>,
    /// Move the given tab into a split alongside the currently active pane.
    pub move_to_split: Option<(u32, SplitDir)>,
}

/// Result of rendering a per-group tab bar.
pub(in crate::app) struct GroupTabBarResult {
    pub close_pane_id: Option<u32>,
    pub clicked_pane_id: Option<u32>,
    pub clicked_group_id: Option<GroupId>,
    pub split_request: Option<(GroupId, SplitDir)>,
}

impl App {
    /// Render the tab strip (horizontally scrollable) and the action buttons to its right.
    ///
    /// Returns deferred actions (close, click, split) to be applied after the closure.
    #[allow(dead_code)]
    #[allow(clippy::too_many_arguments)]
    pub(in crate::app) fn render_tab_bar(
        &mut self,
        ui: &mut egui::Ui,
        visible_indices: &[usize],
        active_pane_id_snap: Option<u32>,
        ws_colors: &[Option<[u8; 3]>],
        tab_h: f32,
        tab_bar_rect: egui::Rect,
        tab_actions_rect: egui::Rect,
    ) -> TabBarResult {
        let mut close_pane_id: Option<u32> = None;
        let mut clicked_pane_id: Option<u32> = None;
        let mut split_request: Option<SplitDir> = None;
        let mut move_to_split: Option<(u32, SplitDir)> = None;

        // ── Tab bar (horizontally scrollable) ────────────────────────
        let mut tab_scroll_offset_x: f32 = 0.0;
        ui.allocate_ui_at_rect(tab_bar_rect, |ui| {
            ui.painter()
                .rect_filled(tab_bar_rect, 0.0, theme::active().bg_toolbar);
            ui.spacing_mut().scroll.floating_allocated_width = 0.0;
            let scroll_out = egui::ScrollArea::horizontal()
                .id_source(self.vp_id("tab_bar_scroll"))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        // Pre-compute display texts outside the tight render loop to
                        // avoid acquiring per-session read locks during painting.
                        let display_texts: Vec<(usize, String)> = visible_indices
                            .iter()
                            .map(|&i| (i, self.tab_display_text(i)))
                            .collect();

                        let visible_roots: Vec<Option<u32>> = visible_indices
                            .iter()
                            .map(|&i| self.pane_state.root_of(self.pane_state.panes[i].id))
                            .collect();

                        let painter = ui.painter().clone();

                        for (vis_pos, (i, display)) in display_texts.iter().enumerate() {
                            let i = *i;
                            let pane_id = self.pane_state.panes[i].id;
                            let is_active = active_pane_id_snap == Some(pane_id);
                            let is_in_split = visible_roots
                                .get(vis_pos)
                                .and_then(|r| *r)
                                .is_some_and(|r| {
                                    self.pane_state
                                        .pane_trees
                                        .get(&r)
                                        .is_some_and(|t| t.leaf_ids().len() > 1)
                                });
                            let ws_color = ws_colors[i];

                            let (_, tab_rect) = ui.allocate_space(egui::vec2(theme::TAB_W, tab_h));

                            if self.tab_scroll_to_pane == Some(pane_id) {
                                ui.scroll_to_rect(tab_rect, Some(egui::Align::Center));
                                self.tab_scroll_to_pane = None;
                            }

                            // Register the tab interaction early so hover state is available
                            // for painting the background and controlling close button visibility.
                            let tab_resp = ui.interact(
                                tab_rect,
                                egui::Id::new(("tab_click", pane_id)),
                                egui::Sense::click_and_drag(),
                            );
                            if tab_resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }

                            // Animated hover for inactive tabs
                            let tab_hover_id = egui::Id::new(("tab_hover", pane_id));
                            let tab_hover_t = crate::app::ui::animation::animated_hover(
                                ui.ctx(),
                                tab_hover_id,
                                tab_resp.hovered(),
                            );

                            let tab_rounding = egui::Rounding {
                                nw: theme::R_MD,
                                ne: theme::R_MD,
                                sw: 0.0,
                                se: 0.0,
                            };

                            let title_color = if is_active {
                                theme::active().text
                            } else {
                                theme::lerp_color(
                                    theme::active().subtext0,
                                    theme::active().subtext1,
                                    tab_hover_t,
                                )
                            };

                            if is_active {
                                painter.rect_filled(
                                    tab_rect,
                                    tab_rounding,
                                    theme::active().bg_tab_active,
                                );
                                painter.rect_stroke(
                                    tab_rect,
                                    tab_rounding,
                                    egui::Stroke::new(
                                        theme::STROKE_THIN,
                                        theme::active().border_subtle,
                                    ),
                                );
                                // Erase bottom border to connect tab to content
                                painter.line_segment(
                                    [
                                        egui::pos2(tab_rect.min.x + 1.0, tab_rect.max.y),
                                        egui::pos2(tab_rect.max.x - 1.0, tab_rect.max.y),
                                    ],
                                    egui::Stroke::new(
                                        theme::STROKE_THIN + 0.5,
                                        theme::active().bg_tab_active,
                                    ),
                                );
                            } else if tab_hover_t > 0.01 {
                                // Inactive: animated hover background
                                let hover_bg = theme::lerp_color(
                                    egui::Color32::TRANSPARENT,
                                    theme::active().bg_row_hover,
                                    tab_hover_t,
                                );
                                painter.rect_filled(tab_rect, tab_rounding, hover_bg);
                            }

                            // Workspace colour strip on left edge
                            if let Some(c) = ws_color {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        tab_rect.min,
                                        egui::vec2(theme::TAB_COLOR_STRIP_W, tab_h),
                                    ),
                                    0.0,
                                    theme::from_rgb(c),
                                );
                            }

                            // Active indicator dot
                            if is_active {
                                let dot_radius = 3.0;
                                let dot_x = tab_rect.min.x
                                    + theme::TAB_PAD_X
                                    + if ws_color.is_some() {
                                        theme::TAB_COLOR_STRIP_W
                                    } else {
                                        0.0
                                    }
                                    + dot_radius;
                                let raw_dot = ws_color
                                    .map(theme::from_rgb)
                                    .unwrap_or(theme::active().accent);
                                let tab_bg = theme::active().bg_tab_active;
                                let dot_color = theme::ensure_term_contrast(raw_dot, tab_bg);
                                let dot_center = egui::pos2(dot_x, tab_rect.center().y);
                                let ring_color =
                                    theme::text_on([tab_bg.r(), tab_bg.g(), tab_bg.b()]);
                                painter.circle_stroke(
                                    dot_center,
                                    dot_radius + 1.0,
                                    egui::Stroke::new(1.0, ring_color.gamma_multiply(0.35)),
                                );
                                painter.circle_filled(dot_center, dot_radius, dot_color);
                            }

                            // Flash feedback overlay on tab
                            self.flash.render_on_rect(
                                &painter,
                                tab_rect,
                                crate::app::feedback::FlashTarget::Tab(pane_id),
                            );

                            // Right-edge separator between tabs — unified vertical line.
                            // Same split group: short centred line, low opacity.
                            // Different groups: full-height line, higher opacity.
                            let same_group_next = vis_pos + 1 < visible_roots.len()
                                && visible_roots[vis_pos] == visible_roots[vis_pos + 1]
                                && visible_roots[vis_pos].is_some_and(|r| {
                                    self.pane_state
                                        .pane_trees
                                        .get(&r)
                                        .is_some_and(|t| t.leaf_ids().len() > 1)
                                });
                            let t = theme::active();
                            if same_group_next {
                                let line_h = tab_h * 0.5;
                                let line_y = tab_rect.center().y - line_h * 0.5;
                                let color = t.border_subtle.gamma_multiply(0.4);
                                painter.line_segment(
                                    [
                                        egui::pos2(tab_rect.max.x, line_y),
                                        egui::pos2(tab_rect.max.x, line_y + line_h),
                                    ],
                                    egui::Stroke::new(theme::STROKE_THIN, color),
                                );
                            } else {
                                let color = t.border_subtle.gamma_multiply(0.8);
                                painter.line_segment(
                                    [
                                        egui::pos2(tab_rect.max.x, tab_rect.min.y),
                                        egui::pos2(tab_rect.max.x, tab_rect.max.y),
                                    ],
                                    egui::Stroke::new(theme::STROKE_THIN, color),
                                );
                            }

                            // Close button (x) — always visible on active tab,
                            // fades in on hover, hidden on idle inactive tabs.
                            let show_close = is_active || tab_resp.hovered();
                            let close_t = crate::app::ui::animation::animated_hover(
                                ui.ctx(),
                                egui::Id::new(("tab_close_anim", pane_id)),
                                show_close,
                            );
                            let close_rect = egui::Rect::from_min_size(
                                egui::pos2(tab_rect.max.x - theme::BTN_W, tab_rect.min.y),
                                egui::vec2(theme::BTN_W, tab_h),
                            );
                            let close_resp = if close_t > 0.01 {
                                let close_fg = theme::active().danger_fg.gamma_multiply(close_t);
                                ui_kit::icon_button(
                                    ui,
                                    egui::Id::new(("tab_close", pane_id)),
                                    close_rect,
                                    "\u{00d7}",
                                    theme::FONT_TERM,
                                    close_fg,
                                    ui_kit::IconButtonStyle::Danger,
                                )
                            } else {
                                // Allocate an inert response so close click handling below
                                // still compiles without restructuring.
                                ui.interact(
                                    close_rect,
                                    egui::Id::new(("tab_close", pane_id)),
                                    egui::Sense::hover(),
                                )
                            };

                            // Title text (clipped before close button)
                            let dot_offset = if is_active { 10.0 } else { 0.0 };
                            let split_icon_offset = if is_in_split { 12.0 } else { 0.0 };
                            let text_x = tab_rect.min.x
                                + theme::TAB_PAD_X
                                + if ws_color.is_some() {
                                    theme::TAB_COLOR_STRIP_W
                                } else {
                                    0.0
                                }
                                + dot_offset
                                + split_icon_offset;

                            // Split-group icon (two vertical bars)
                            if is_in_split {
                                let icon_x = text_x - split_icon_offset;
                                let icon_y = tab_rect.center().y;
                                let icon_color = title_color.gamma_multiply(0.6);
                                let half = 3.0;
                                painter.line_segment(
                                    [
                                        egui::pos2(icon_x + 2.0, icon_y - half),
                                        egui::pos2(icon_x + 2.0, icon_y + half),
                                    ],
                                    egui::Stroke::new(1.0, icon_color),
                                );
                                painter.line_segment(
                                    [
                                        egui::pos2(icon_x + 5.0, icon_y - half),
                                        egui::pos2(icon_x + 5.0, icon_y + half),
                                    ],
                                    egui::Stroke::new(1.0, icon_color),
                                );
                            }

                            let is_renaming = self.tab_rename_pane_id == Some(pane_id);
                            if is_renaming {
                                let edit_rect = egui::Rect::from_min_max(
                                    egui::pos2(text_x, tab_rect.min.y + theme::SP_1),
                                    egui::pos2(
                                        close_rect.min.x - theme::SP_1,
                                        tab_rect.max.y - theme::SP_1,
                                    ),
                                );
                                let edit_id = egui::Id::new(("tab_rename_edit", pane_id));
                                ui.painter().rect_filled(
                                    edit_rect,
                                    theme::R_SM,
                                    theme::active().surface1,
                                );
                                let resp = ui
                                    .allocate_ui_at_rect(edit_rect, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.tab_rename_text)
                                                .id(edit_id)
                                                .desired_width(edit_rect.width())
                                                .font(egui::FontId::proportional(theme::FONT_UI_MD))
                                                .frame(true)
                                                .text_color(title_color),
                                        )
                                    })
                                    .inner;
                                let lost_focus = resp.lost_focus()
                                    && !ui.input(|i| i.key_pressed(egui::Key::Escape));
                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if enter || lost_focus {
                                    let new_title = self.tab_rename_text.trim().to_string();
                                    if !new_title.is_empty() {
                                        if let PaneContent::Terminal(sid) =
                                            &self.pane_state.panes[i].content
                                        {
                                            if let Some(entry) = self
                                                .session_state
                                                .sessions
                                                .iter()
                                                .find(|e| e.id == *sid)
                                            {
                                                entry.session.read().set_title(new_title);
                                            }
                                        }
                                    }
                                    self.tab_rename_pane_id = None;
                                    self.tab_rename_text.clear();
                                } else if esc {
                                    self.tab_rename_pane_id = None;
                                    self.tab_rename_text.clear();
                                }
                            } else {
                                painter
                                    .with_clip_rect(egui::Rect::from_min_max(
                                        egui::pos2(text_x, tab_rect.min.y),
                                        egui::pos2(close_rect.min.x - theme::SP_1, tab_rect.max.y),
                                    ))
                                    .text(
                                        egui::pos2(text_x, tab_rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        display,
                                        egui::FontId::proportional(theme::FONT_UI_MD),
                                        title_color,
                                    );
                            }

                            // Completed process badge (green dot)
                            if let PaneContent::Terminal(sid) = &self.pane_state.panes[i].content {
                                if self.completed_badges.contains(sid) {
                                    let dot_r = 3.5;
                                    let dot_pos = egui::pos2(
                                        text_x - theme::SP_1 - dot_r,
                                        tab_rect.center().y,
                                    );
                                    painter.circle_filled(dot_pos, dot_r, theme::active().green);
                                }
                            }

                            if close_resp
                                .on_hover_text("Close tab (Ctrl+Shift+W)")
                                .clicked()
                                || tab_resp.clicked_by(egui::PointerButton::Middle)
                            {
                                close_pane_id = Some(pane_id);
                            } else if tab_resp.double_clicked() && !is_renaming {
                                self.tab_rename_pane_id = Some(pane_id);
                                self.tab_rename_text = display.clone();
                                clicked_pane_id = Some(pane_id);
                            } else if tab_resp.clicked() {
                                clicked_pane_id = Some(pane_id);
                                // Clear badge when tab is clicked
                                if let PaneContent::Terminal(sid) =
                                    &self.pane_state.panes[i].content
                                {
                                    self.completed_badges.remove(sid);
                                }
                            }

                            // Tab drag-to-reorder
                            if tab_resp.drag_started() {
                                let origin =
                                    tab_resp.interact_pointer_pos().unwrap_or(tab_rect.center());
                                self.drag_state.set_payload(
                                    drag::DragPayload::Tab(pane_id),
                                    origin,
                                    display,
                                );
                            }
                            if self.drag_state.is_active() && tab_resp.hovered() {
                                let accepts = match &self.drag_state.payload {
                                    Some(drag::DragPayload::Tab(pid)) => *pid != pane_id,
                                    Some(drag::DragPayload::Workspace(_)) => false,
                                    Some(_) => true,
                                    None => false,
                                };
                                if accepts {
                                    let drag_from_left = match &self.drag_state.payload {
                                        Some(drag::DragPayload::Tab(src_id)) => self
                                            .pane_state
                                            .panes
                                            .iter()
                                            .position(|p| p.id == *src_id)
                                            .map(|src_i| src_i < i)
                                            .unwrap_or(false),
                                        _ => true,
                                    };
                                    let indicator_x = if drag_from_left {
                                        tab_rect.max.x
                                    } else {
                                        tab_rect.min.x
                                    };
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_size(
                                            egui::pos2(indicator_x - 1.5, tab_rect.min.y),
                                            egui::vec2(3.0, tab_h),
                                        ),
                                        0.0,
                                        theme::active().blue,
                                    );
                                    self.drag_state.drop_target = Some(drag::DropTarget::TabBar(i));
                                }
                            }

                            // Right-click context menu for tab operations.
                            let can_move_to_split = visible_indices.len() >= 2;
                            tab_resp.context_menu(|ui| {
                                if ui.button("Rename tab").clicked() {
                                    self.tab_rename_pane_id = Some(pane_id);
                                    self.tab_rename_text = display.clone();
                                    ui.close_menu();
                                }
                                ui.separator();
                                ui.add_enabled_ui(can_move_to_split, |ui| {
                                    if ui.button("Move to split horizontal").clicked() {
                                        move_to_split = Some((pane_id, SplitDir::Horizontal));
                                        ui.close_menu();
                                    }
                                    if ui.button("Move to split vertical").clicked() {
                                        move_to_split = Some((pane_id, SplitDir::Vertical));
                                        ui.close_menu();
                                    }
                                });
                            });
                        }
                    });
                });
            tab_scroll_offset_x = scroll_out.state.offset.x;
        });

        // ── Tab-bar action buttons (split / close-all) ──────────
        ui.allocate_ui_at_rect(tab_actions_rect, |ui| {
            ui.painter()
                .rect_filled(tab_actions_rect, 0.0, theme::active().bg_toolbar);
            // Left separator
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    tab_actions_rect.left_top(),
                    egui::vec2(theme::STROKE_THIN, tab_h),
                ),
                0.0,
                theme::active().border_subtle,
            );
            let icon_sz = egui::vec2(theme::BTN_W, tab_h);
            let t = theme::active();
            let mut x = tab_actions_rect.min.x + theme::SP_1;

            let icon_stroke = egui::Stroke::new(1.2, t.subtext1);
            let icon_hover_stroke = egui::Stroke::new(1.2, t.text);
            let icon_inset = theme::ICON_INSET;

            // Split horizontal (side-by-side)
            let split_h_rect =
                egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
            let split_h_resp = ui.interact(
                split_h_rect,
                self.vp_id("tab_split_h"),
                egui::Sense::click(),
            );
            if split_h_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let sh_stroke = if split_h_resp.hovered() {
                ui.painter()
                    .rect_filled(split_h_rect, theme::R_MD, t.surface2);
                icon_hover_stroke
            } else {
                icon_stroke
            };
            {
                let r = split_h_rect.shrink(icon_inset);
                let p = ui.painter();
                p.rect_stroke(r, 1.0, sh_stroke);
                p.line_segment([r.center_top(), r.center_bottom()], sh_stroke);
            }
            if split_h_resp
                .on_hover_text("Split horizontal (Ctrl+Shift+\\)")
                .clicked()
            {
                split_request = Some(SplitDir::Horizontal);
            }
            x += icon_sz.x + theme::TAB_ACTION_GAP;

            // Split vertical (top-bottom)
            let split_v_rect =
                egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
            let split_v_resp = ui.interact(
                split_v_rect,
                self.vp_id("tab_split_v"),
                egui::Sense::click(),
            );
            if split_v_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let sv_stroke = if split_v_resp.hovered() {
                ui.painter()
                    .rect_filled(split_v_rect, theme::R_MD, t.surface2);
                icon_hover_stroke
            } else {
                icon_stroke
            };
            {
                let r = split_v_rect.shrink(icon_inset);
                let p = ui.painter();
                p.rect_stroke(r, 1.0, sv_stroke);
                p.line_segment([r.left_center(), r.right_center()], sv_stroke);
            }
            if split_v_resp
                .on_hover_text("Split vertical (Ctrl+Shift+-)")
                .clicked()
            {
                split_request = Some(SplitDir::Vertical);
            }
            x += icon_sz.x + theme::TAB_ACTION_GAP;

            // Close all sessions in current workspace group
            let close_all_rect =
                egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
            let close_all_resp = ui_kit::icon_button(
                ui,
                self.vp_id("tab_close_all"),
                close_all_rect,
                "\u{2716}",
                theme::FONT_UI_MD,
                t.danger_fg,
                ui_kit::IconButtonStyle::Danger,
            );
            if close_all_resp
                .on_hover_text("Close all sessions in this workspace")
                .clicked()
            {
                self.show_close_all_confirm = true;
                self.close_all_frames_open = 0;
            }
        });

        TabBarResult {
            close_pane_id,
            clicked_pane_id,
            split_request,
            move_to_split,
        }
    }

    /// Render a tab bar for a single editor group.
    ///
    /// Returns deferred actions to be applied after the closure.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::app) fn render_group_tab_bar(
        &mut self,
        ui: &mut egui::Ui,
        group: &EditorGroup,
        group_id: GroupId,
        is_focused: bool,
        tab_h: f32,
        tab_bar_rect: egui::Rect,
    ) -> GroupTabBarResult {
        let mut close_pane_id: Option<u32> = None;
        let mut clicked_pane_id: Option<u32> = None;
        let mut clicked_group_id: Option<GroupId> = None;
        let mut split_request: Option<(GroupId, SplitDir)> = None;

        let has_multiple_groups = self.pane_state.groups.len() > 1;

        // ── Tab bar (horizontally scrollable) + split buttons ───
        let ctx_menu_extra = if has_multiple_groups {
            theme::BTN_W + theme::TAB_ACTION_GAP
        } else {
            0.0
        };
        let tab_actions_w = theme::TAB_ACTIONS_W + ctx_menu_extra;
        let tab_scroll_w = (tab_bar_rect.width() - tab_actions_w).max(0.0);
        let scroll_rect =
            egui::Rect::from_min_size(tab_bar_rect.min, egui::vec2(tab_scroll_w, tab_h));
        let tab_actions_rect = egui::Rect::from_min_size(
            egui::pos2(tab_bar_rect.min.x + tab_scroll_w, tab_bar_rect.min.y),
            egui::vec2(tab_actions_w, tab_h),
        );

        ui.allocate_ui_at_rect(scroll_rect, |ui| {
            let t = theme::active();
            let bar_bg = if is_focused && has_multiple_groups {
                // Subtle tint to indicate focus when there are multiple groups
                theme::lerp_color(t.bg_toolbar, t.surface1, 0.3)
            } else {
                t.bg_toolbar
            };
            ui.painter().rect_filled(scroll_rect, 0.0, bar_bg);

            // Focus indicator line at bottom when multiple groups exist
            if is_focused && has_multiple_groups {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(scroll_rect.min.x, scroll_rect.max.y - 2.0),
                        egui::vec2(scroll_rect.width(), 2.0),
                    ),
                    0.0,
                    t.accent.gamma_multiply(0.5),
                );
            }

            ui.spacing_mut().scroll.floating_allocated_width = 0.0;
            let scroll_id_source = self.vp_id(&format!("group_tab_scroll_{group_id}"));
            egui::ScrollArea::horizontal()
                .id_source(scroll_id_source)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;

                        // Pre-compute display texts, workspace colors, and tooltips
                        // outside the tight render loop to avoid per-session read locks.
                        struct TabInfo {
                            pane_id: u32,
                            pane_idx: usize,
                            display: String,
                            ws_color: Option<[u8; 3]>,
                            tooltip: String,
                        }
                        let tab_infos: Vec<TabInfo> = group
                            .pane_ids
                            .iter()
                            .filter_map(|&pane_id| {
                                let pane_idx =
                                    self.pane_state.panes.iter().position(|p| p.id == pane_id)?;
                                Some(TabInfo {
                                    pane_id,
                                    pane_idx,
                                    display: self.tab_display_text(pane_idx),
                                    ws_color: self.ws_color_for_pane(pane_idx),
                                    tooltip: self.tab_tooltip_text(pane_idx),
                                })
                            })
                            .collect();

                        let painter = ui.painter().clone();

                        for (vis_pos, info) in tab_infos.iter().enumerate() {
                            let pane_id = info.pane_id;
                            let pane_idx = info.pane_idx;
                            let display = &info.display;
                            let ws_color = &info.ws_color;
                            let tooltip = &info.tooltip;
                            let is_active = group.active_pane_id == Some(pane_id);

                            let (_, tab_rect) = ui.allocate_space(egui::vec2(theme::TAB_W, tab_h));

                            if self.tab_scroll_to_pane == Some(pane_id) {
                                ui.scroll_to_rect(tab_rect, Some(egui::Align::Center));
                                self.tab_scroll_to_pane = None;
                            }

                            let tab_resp = ui.interact(
                                tab_rect,
                                egui::Id::new(("gtab_click", group_id, pane_id)),
                                egui::Sense::click_and_drag(),
                            );
                            if tab_resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }

                            // Animated hover for inactive tabs
                            let tab_hover_id = egui::Id::new(("gtab_hover", group_id, pane_id));
                            let tab_hover_t = crate::app::ui::animation::animated_hover(
                                ui.ctx(),
                                tab_hover_id,
                                tab_resp.hovered(),
                            );

                            let tab_rounding = egui::Rounding {
                                nw: theme::R_MD,
                                ne: theme::R_MD,
                                sw: 0.0,
                                se: 0.0,
                            };

                            let t = theme::active();
                            let title_color = if is_active {
                                t.text
                            } else {
                                theme::lerp_color(t.subtext0, t.subtext1, tab_hover_t)
                            };

                            if is_active {
                                painter.rect_filled(tab_rect, tab_rounding, t.bg_tab_active);
                                painter.rect_stroke(
                                    tab_rect,
                                    tab_rounding,
                                    egui::Stroke::new(theme::STROKE_THIN, t.border_subtle),
                                );
                                // Erase bottom border to connect tab to content
                                painter.line_segment(
                                    [
                                        egui::pos2(tab_rect.min.x + 1.0, tab_rect.max.y),
                                        egui::pos2(tab_rect.max.x - 1.0, tab_rect.max.y),
                                    ],
                                    egui::Stroke::new(theme::STROKE_THIN + 0.5, t.bg_tab_active),
                                );
                            } else if tab_hover_t > 0.01 {
                                let hover_bg = theme::lerp_color(
                                    egui::Color32::TRANSPARENT,
                                    t.bg_row_hover,
                                    tab_hover_t,
                                );
                                painter.rect_filled(tab_rect, tab_rounding, hover_bg);
                            }

                            // Workspace colour strip on left edge
                            if let Some(c) = ws_color {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        tab_rect.min,
                                        egui::vec2(theme::TAB_COLOR_STRIP_W, tab_h),
                                    ),
                                    0.0,
                                    theme::from_rgb(*c),
                                );
                            }

                            // Active indicator dot (tinted by workspace color)
                            if is_active {
                                let color_strip_offset = if ws_color.is_some() {
                                    theme::TAB_COLOR_STRIP_W
                                } else {
                                    0.0
                                };
                                let dot_radius = 3.0;
                                let dot_x = tab_rect.min.x
                                    + theme::TAB_PAD_X
                                    + color_strip_offset
                                    + dot_radius;
                                let raw_dot = ws_color.map(theme::from_rgb).unwrap_or(t.accent);
                                let tab_bg = t.bg_tab_active;
                                let dot_color = theme::ensure_term_contrast(raw_dot, tab_bg);
                                let dot_center = egui::pos2(dot_x, tab_rect.center().y);
                                let ring_color =
                                    theme::text_on([tab_bg.r(), tab_bg.g(), tab_bg.b()]);
                                painter.circle_stroke(
                                    dot_center,
                                    dot_radius + 1.0,
                                    egui::Stroke::new(1.0, ring_color.gamma_multiply(0.35)),
                                );
                                painter.circle_filled(dot_center, dot_radius, dot_color);
                            }

                            // Flash feedback overlay on tab
                            self.flash.render_on_rect(
                                &painter,
                                tab_rect,
                                crate::app::feedback::FlashTarget::Tab(pane_id),
                            );

                            // Right-edge separator between tabs
                            let sep_color = t.border_subtle.gamma_multiply(0.8);
                            painter.line_segment(
                                [
                                    egui::pos2(tab_rect.max.x, tab_rect.min.y),
                                    egui::pos2(tab_rect.max.x, tab_rect.max.y),
                                ],
                                egui::Stroke::new(theme::STROKE_THIN, sep_color),
                            );

                            // Close button
                            let show_close = is_active || tab_resp.hovered();
                            let close_t = crate::app::ui::animation::animated_hover(
                                ui.ctx(),
                                egui::Id::new(("gtab_close_anim", group_id, pane_id)),
                                show_close,
                            );
                            let close_rect = egui::Rect::from_min_size(
                                egui::pos2(tab_rect.max.x - theme::BTN_W, tab_rect.min.y),
                                egui::vec2(theme::BTN_W, tab_h),
                            );
                            let close_resp = if close_t > 0.01 {
                                let close_fg = t.danger_fg.gamma_multiply(close_t);
                                ui_kit::icon_button(
                                    ui,
                                    egui::Id::new(("gtab_close", group_id, pane_id)),
                                    close_rect,
                                    "\u{00d7}",
                                    theme::FONT_TERM,
                                    close_fg,
                                    ui_kit::IconButtonStyle::Danger,
                                )
                            } else {
                                ui.interact(
                                    close_rect,
                                    egui::Id::new(("gtab_close", group_id, pane_id)),
                                    egui::Sense::hover(),
                                )
                            };

                            // Title text (clipped before close button)
                            let dot_offset = if is_active { 10.0 } else { 0.0 };
                            let color_strip_offset = if ws_color.is_some() {
                                theme::TAB_COLOR_STRIP_W
                            } else {
                                0.0
                            };
                            let text_x =
                                tab_rect.min.x + theme::TAB_PAD_X + color_strip_offset + dot_offset;

                            let is_renaming = self.tab_rename_pane_id == Some(pane_id);
                            if is_renaming {
                                let edit_rect = egui::Rect::from_min_max(
                                    egui::pos2(text_x, tab_rect.min.y + theme::SP_1),
                                    egui::pos2(
                                        close_rect.min.x - theme::SP_1,
                                        tab_rect.max.y - theme::SP_1,
                                    ),
                                );
                                let edit_id =
                                    egui::Id::new(("gtab_rename_edit", group_id, pane_id));
                                ui.painter().rect_filled(edit_rect, theme::R_SM, t.surface1);
                                let resp = ui
                                    .allocate_ui_at_rect(edit_rect, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.tab_rename_text)
                                                .id(edit_id)
                                                .desired_width(edit_rect.width())
                                                .font(egui::FontId::proportional(theme::FONT_UI_MD))
                                                .frame(true)
                                                .text_color(title_color),
                                        )
                                    })
                                    .inner;
                                let lost_focus = resp.lost_focus()
                                    && !ui.input(|i| i.key_pressed(egui::Key::Escape));
                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if enter || lost_focus {
                                    let new_title = self.tab_rename_text.trim().to_string();
                                    if !new_title.is_empty() {
                                        if let PaneContent::Terminal(sid) =
                                            &self.pane_state.panes[pane_idx].content
                                        {
                                            if let Some(entry) = self
                                                .session_state
                                                .sessions
                                                .iter()
                                                .find(|e| e.id == *sid)
                                            {
                                                entry.session.read().set_title(new_title);
                                            }
                                        }
                                    }
                                    self.tab_rename_pane_id = None;
                                    self.tab_rename_text.clear();
                                } else if esc {
                                    self.tab_rename_pane_id = None;
                                    self.tab_rename_text.clear();
                                }
                            } else {
                                painter
                                    .with_clip_rect(egui::Rect::from_min_max(
                                        egui::pos2(text_x, tab_rect.min.y),
                                        egui::pos2(close_rect.min.x - theme::SP_1, tab_rect.max.y),
                                    ))
                                    .text(
                                        egui::pos2(text_x, tab_rect.center().y),
                                        egui::Align2::LEFT_CENTER,
                                        display,
                                        egui::FontId::proportional(theme::FONT_UI_MD),
                                        title_color,
                                    );
                            }

                            // Completed process badge (green dot)
                            if let PaneContent::Terminal(sid) =
                                &self.pane_state.panes[pane_idx].content
                            {
                                if self.completed_badges.contains(sid) {
                                    let dot_r = 3.5;
                                    let dot_pos = egui::pos2(
                                        text_x - theme::SP_1 - dot_r,
                                        tab_rect.center().y,
                                    );
                                    painter.circle_filled(dot_pos, dot_r, t.green);
                                }
                            }

                            // Tooltip on hover (skip when renaming or hovering close button)
                            if tab_resp.hovered() && !close_resp.hovered() && !is_renaming {
                                egui::show_tooltip_at_pointer(
                                    ui.ctx(),
                                    ui.layer_id(),
                                    egui::Id::new(("gtab_tooltip", group_id, pane_id)),
                                    |ui| {
                                        ui.label(tooltip);
                                    },
                                );
                            }

                            if close_resp
                                .on_hover_text("Close tab (Ctrl+Shift+W)")
                                .clicked()
                                || tab_resp.clicked_by(egui::PointerButton::Middle)
                            {
                                close_pane_id = Some(pane_id);
                            } else if tab_resp.double_clicked() && !is_renaming {
                                self.tab_rename_pane_id = Some(pane_id);
                                self.tab_rename_text = display.clone();
                                clicked_pane_id = Some(pane_id);
                                clicked_group_id = Some(group_id);
                            } else if tab_resp.clicked() {
                                clicked_pane_id = Some(pane_id);
                                clicked_group_id = Some(group_id);
                                if let PaneContent::Terminal(sid) =
                                    &self.pane_state.panes[pane_idx].content
                                {
                                    self.completed_badges.remove(sid);
                                }
                            }

                            // Tab drag-to-reorder
                            if tab_resp.drag_started() {
                                let origin =
                                    tab_resp.interact_pointer_pos().unwrap_or(tab_rect.center());
                                self.drag_state.set_payload(
                                    drag::DragPayload::Tab(pane_id),
                                    origin,
                                    display,
                                );
                            }
                            if self.drag_state.is_active() && tab_resp.hovered() {
                                let accepts = match &self.drag_state.payload {
                                    Some(drag::DragPayload::Tab(pid)) => *pid != pane_id,
                                    Some(drag::DragPayload::Workspace(_)) => false,
                                    Some(_) => true,
                                    None => false,
                                };
                                if accepts {
                                    let indicator_x = tab_rect.max.x;
                                    ui.painter().rect_filled(
                                        egui::Rect::from_min_size(
                                            egui::pos2(indicator_x - 1.5, tab_rect.min.y),
                                            egui::vec2(3.0, tab_h),
                                        ),
                                        0.0,
                                        t.blue,
                                    );
                                    self.drag_state.drop_target =
                                        Some(drag::DropTarget::GroupTabBar {
                                            group_id,
                                            position: vis_pos,
                                        });
                                }
                            }

                            // Right-click context menu
                            tab_resp.context_menu(|ui| {
                                if ui.button("Rename tab").clicked() {
                                    self.tab_rename_pane_id = Some(pane_id);
                                    self.tab_rename_text = display.clone();
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                });
        });

        // ── Tab-bar action buttons (split / close-all) ──────────
        ui.allocate_ui_at_rect(tab_actions_rect, |ui| {
            let t = theme::active();
            let bar_bg = if is_focused && has_multiple_groups {
                theme::lerp_color(t.bg_toolbar, t.surface1, 0.3)
            } else {
                t.bg_toolbar
            };
            ui.painter().rect_filled(tab_actions_rect, 0.0, bar_bg);
            // Left separator
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    tab_actions_rect.left_top(),
                    egui::vec2(theme::STROKE_THIN, tab_h),
                ),
                0.0,
                t.border_subtle,
            );
            let icon_sz = egui::vec2(theme::BTN_W, tab_h);
            let mut x = tab_actions_rect.min.x + theme::SP_1;

            let icon_stroke = egui::Stroke::new(1.2, t.subtext1);
            let icon_hover_stroke = egui::Stroke::new(1.2, t.text);
            let icon_inset = theme::ICON_INSET;

            // Split horizontal (side-by-side)
            let split_h_rect =
                egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
            let split_h_id = self.vp_id(&format!("gtab_split_h_{group_id}"));
            let split_h_resp = ui.interact(split_h_rect, split_h_id, egui::Sense::click());
            if split_h_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let sh_stroke = if split_h_resp.hovered() {
                ui.painter()
                    .rect_filled(split_h_rect, theme::R_MD, t.surface2);
                icon_hover_stroke
            } else {
                icon_stroke
            };
            {
                let r = split_h_rect.shrink(icon_inset);
                let p = ui.painter();
                p.rect_stroke(r, 1.0, sh_stroke);
                p.line_segment([r.center_top(), r.center_bottom()], sh_stroke);
            }
            if split_h_resp
                .on_hover_text("Split horizontal (Ctrl+Shift+\\)")
                .clicked()
            {
                split_request = Some((group_id, SplitDir::Horizontal));
            }
            x += icon_sz.x + theme::TAB_ACTION_GAP;

            // Split vertical (top-bottom)
            let split_v_rect =
                egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
            let split_v_resp = ui.interact(
                split_v_rect,
                self.vp_id(&format!("gtab_split_v_{group_id}")),
                egui::Sense::click(),
            );
            if split_v_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let sv_stroke = if split_v_resp.hovered() {
                ui.painter()
                    .rect_filled(split_v_rect, theme::R_MD, t.surface2);
                icon_hover_stroke
            } else {
                icon_stroke
            };
            {
                let r = split_v_rect.shrink(icon_inset);
                let p = ui.painter();
                p.rect_stroke(r, 1.0, sv_stroke);
                p.line_segment([r.left_center(), r.right_center()], sv_stroke);
            }
            if split_v_resp
                .on_hover_text("Split vertical (Ctrl+Shift+-)")
                .clicked()
            {
                split_request = Some((group_id, SplitDir::Vertical));
            }
            x += icon_sz.x + theme::TAB_ACTION_GAP;

            // 3-dot context menu (always visible when multiple groups exist)
            if has_multiple_groups {
                let menu_rect =
                    egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
                let menu_popup_id = self.vp_id(&format!("group_ctx_popup_{group_id}"));
                let popup_open = ui.memory(|m| m.is_popup_open(menu_popup_id));
                let menu_btn_id = self.vp_id(&format!("group_ctx_btn_{group_id}"));
                let btn_resp = ui_kit::dot_menu_button(ui, menu_btn_id, menu_rect, popup_open);
                if btn_resp.clicked() {
                    ui.memory_mut(|m| m.toggle_popup(menu_popup_id));
                }
                egui::containers::popup::popup_below_widget(
                    ui,
                    menu_popup_id,
                    &btn_resp,
                    egui::containers::popup::PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
                        ui.set_min_width(140.0);
                        if ui.button("Close pane").clicked() {
                            if let Some(pid) = group.active_pane_id {
                                close_pane_id = Some(pid);
                            }
                            ui.memory_mut(|m| m.close_popup());
                        }
                    },
                );
                x += icon_sz.x + theme::TAB_ACTION_GAP;
            }

            // Close all sessions button
            let close_all_rect =
                egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
            let close_all_resp = ui_kit::icon_button(
                ui,
                self.vp_id(&format!("gtab_close_all_{group_id}")),
                close_all_rect,
                "\u{2716}",
                theme::FONT_UI_MD,
                t.danger_fg,
                ui_kit::IconButtonStyle::Danger,
            );
            if close_all_resp
                .on_hover_text("Close all tabs in this pane")
                .clicked()
            {
                self.close_all_target = super::super::CloseAllTarget::EditorGroup(group_id);
                self.show_close_all_confirm = true;
                self.close_all_frames_open = 0;
            }
        });

        GroupTabBarResult {
            close_pane_id,
            clicked_pane_id,
            clicked_group_id,
            split_request,
        }
    }

    /// Compute the display text for a tab at the given pane index.
    pub(in crate::app) fn tab_display_text(&self, pane_index: usize) -> String {
        match &self.pane_state.panes[pane_index].content {
            PaneContent::Terminal(sid) => {
                let sid = *sid;
                self.session_state
                    .sessions
                    .iter()
                    .find(|e| e.id == sid)
                    .map(|e| {
                        let s = e.session.read();
                        let title = s.title();
                        let cwd = s.cwd.clone();
                        drop(s);
                        let fg = self.workers.foreground_worker.get(e.id);
                        let ws_name = if cwd.as_os_str().is_empty() {
                            None
                        } else {
                            self.workspace_store
                                .find_for_cwd(&cwd)
                                .map(|w| w.name.clone())
                        };
                        effective_title(
                            &title,
                            &cwd,
                            fg.as_ref(),
                            Some(&e.shell),
                            ws_name.as_deref(),
                        )
                    })
                    .unwrap_or_else(|| format!("Terminal {sid}"))
            }
            PaneContent::DeferredTerminal {
                cwd, saved_title, ..
            } => {
                if let Some(t) = saved_title.as_deref().filter(|s| !s.is_empty()) {
                    return t.to_string();
                }
                let cwd_path = cwd.clone().unwrap_or_default();
                let ws_name = if cwd_path.as_os_str().is_empty() {
                    None
                } else {
                    self.workspace_store
                        .find_for_cwd(&cwd_path)
                        .map(|w| w.name.clone())
                };
                effective_title("", &cwd_path, None, None, ws_name.as_deref())
            }
            PaneContent::FileEditor(ed) => {
                let fname = ed
                    .path
                    .file_name()
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
                let fname = d
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                format!("\u{21c4} {fname}")
            }
            PaneContent::NoteEditor(_) => "Notes".to_string(),
            PaneContent::ConflictResolver(cr) => cr
                .path
                .file_name()
                .map(|n| format!("\u{26a0} {}", n.to_string_lossy()))
                .unwrap_or_else(|| "Conflicts".to_string()),
        }
    }

    /// Compute the tooltip text for a tab: full title + CWD + workspace name.
    fn tab_tooltip_text(&self, pane_index: usize) -> String {
        match &self.pane_state.panes[pane_index].content {
            PaneContent::Terminal(sid) => {
                let sid = *sid;
                let Some(e) = self.session_state.sessions.iter().find(|e| e.id == sid) else {
                    return format!("Terminal {sid}");
                };
                let s = e.session.read();
                let title = s.title();
                let cwd = s.cwd.clone();
                drop(s);
                let ws_name = if cwd.as_os_str().is_empty() {
                    None
                } else {
                    self.workspace_store
                        .find_for_cwd(&cwd)
                        .map(|w| w.name.clone())
                };
                let fg = self.workers.foreground_worker.get(e.id);
                let display = effective_title(
                    &title,
                    &cwd,
                    fg.as_ref(),
                    Some(&e.shell),
                    ws_name.as_deref(),
                );
                let mut tip = display;
                let cwd_str = cwd.to_string_lossy();
                if !cwd_str.is_empty() {
                    tip.push('\n');
                    tip.push_str(&cwd_str);
                }
                if let Some(name) = &ws_name {
                    tip.push('\n');
                    tip.push_str(name);
                }
                tip
            }
            PaneContent::DeferredTerminal {
                cwd, saved_title, ..
            } => {
                let mut tip = saved_title
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Terminal")
                    .to_string();
                if let Some(c) = cwd {
                    let s = c.to_string_lossy();
                    if !s.is_empty() {
                        tip.push('\n');
                        tip.push_str(&s);
                    }
                }
                tip
            }
            PaneContent::FileEditor(ed) => ed.path.to_string_lossy().into_owned(),
            PaneContent::FileDiff(d) => format!("Diff: {}", d.path.to_string_lossy()),
            PaneContent::NoteEditor(_) => "Notes".to_string(),
            PaneContent::ConflictResolver(cr) => {
                format!("Conflicts: {}", cr.path.to_string_lossy())
            }
        }
    }

    /// Workspace color for a pane (by index), if the pane belongs to a workspace.
    pub(in crate::app) fn ws_color_for_pane(&self, pane_index: usize) -> Option<[u8; 3]> {
        match &self.pane_state.panes[pane_index].content {
            PaneContent::Terminal(sid) => self.session_state.find(*sid).and_then(|e| {
                let cwd = e.session.read().cwd.clone();
                if cwd.as_os_str().is_empty() {
                    return None;
                }
                self.workspace_store.find_for_cwd(&cwd).map(|w| w.color)
            }),
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
            PaneContent::NoteEditor(ne) => ne.workspace_id.and_then(|id| {
                self.workspace_store
                    .workspaces
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| w.color)
            }),
            _ => None,
        }
    }
}
