use super::super::drag;
use super::super::pane::PaneContent;
use super::super::title::effective_title;
use super::super::App;
use crate::editor_group::{EditorGroup, GroupId};
use crate::pane_tree::SplitDir;
use crate::theme;
use crate::ui_kit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TabVisual {
    text: egui::Color32,
    bar: Option<egui::Color32>,
    fill: Option<egui::Color32>,
    outline: bool,
}

fn tab_visual(selected: bool, t: &theme::Theme) -> TabVisual {
    TabVisual {
        text: if selected { t.fg_strong } else { t.fg_muted },
        bar: selected.then_some(t.accent),
        fill: None,
        outline: false,
    }
}

fn workspace_strip_rect(tab_rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        tab_rect.min,
        egui::vec2(theme::TAB_COLOR_STRIP_W, tab_rect.height()),
    )
}

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
                .rect_filled(tab_bar_rect, theme::R_NONE, theme::active().bg_panel);
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

                            let tab_w = if is_active {
                                theme::TAB_W_ACTIVE
                            } else {
                                theme::TAB_W
                            };
                            let (_, tab_rect) = ui.allocate_space(egui::vec2(tab_w, tab_h));

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

                            let t = theme::active();
                            let visual = tab_visual(is_active, t);
                            let title_color = visual.text;

                            if let Some(fill) = visual.fill {
                                painter.rect_filled(tab_rect, theme::R_NONE, fill);
                            }
                            if visual.outline {
                                painter.rect_stroke(
                                    tab_rect,
                                    theme::R_NONE,
                                    egui::Stroke::new(theme::STROKE_THIN, t.border),
                                );
                            }
                            if let Some(bar) = visual.bar {
                                painter.rect_filled(
                                    egui::Rect::from_min_max(
                                        egui::pos2(
                                            tab_rect.min.x,
                                            tab_rect.max.y - theme::TAB_SELECTED_BAR_H,
                                        ),
                                        tab_rect.max,
                                    ),
                                    theme::R_NONE,
                                    bar,
                                );
                            } else if tab_hover_t > 0.01 {
                                // Inactive: animated hover background
                                let hover_bg = theme::lerp_color(
                                    egui::Color32::TRANSPARENT,
                                    t.bg_row_hover,
                                    tab_hover_t,
                                );
                                painter.rect_filled(tab_rect, theme::R_NONE, hover_bg);
                            }

                            // Workspace colour strip on left edge
                            if let Some(c) = ws_color {
                                painter.rect_filled(
                                    workspace_strip_rect(tab_rect),
                                    theme::R_NONE,
                                    theme::from_rgb(c),
                                );
                            }

                            let status_color = match &self.pane_state.panes[i].content {
                                PaneContent::Terminal(sid) => {
                                    let alive =
                                        self.session_state.find(*sid).is_some_and(|entry| {
                                            entry.alive.load(std::sync::atomic::Ordering::Relaxed)
                                        });
                                    Some(if alive { t.success } else { t.fg_muted })
                                }
                                PaneContent::DeferredTerminal { .. } => Some(t.fg_muted),
                                _ => None,
                            };

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
                                    ui_kit::icon::X,
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
                            let status_offset = if status_color.is_some() { 10.0 } else { 0.0 };
                            let split_icon_offset = if is_in_split { 12.0 } else { 0.0 };
                            let text_x = tab_rect.min.x
                                + theme::TAB_PAD_X
                                + if ws_color.is_some() {
                                    theme::TAB_COLOR_STRIP_W
                                } else {
                                    0.0
                                }
                                + status_offset
                                + split_icon_offset;

                            if let Some(color) = status_color {
                                painter.circle_filled(
                                    egui::pos2(
                                        tab_rect.min.x
                                            + theme::TAB_PAD_X
                                            + if ws_color.is_some() {
                                                theme::TAB_COLOR_STRIP_W
                                            } else {
                                                0.0
                                            }
                                            + 3.0,
                                        tab_rect.center().y,
                                    ),
                                    3.0,
                                    color,
                                );
                            }

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
                                    egui::Stroke::new(1.0_f32, icon_color),
                                );
                                painter.line_segment(
                                    [
                                        egui::pos2(icon_x + 5.0, icon_y - half),
                                        egui::pos2(icon_x + 5.0, icon_y + half),
                                    ],
                                    egui::Stroke::new(1.0_f32, icon_color),
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
                                                .font(egui::FontId::proportional(theme::FONT_SM))
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
                                let max_text_w =
                                    (close_rect.min.x - theme::SP_1 - text_x).max(theme::SP_0);
                                let galley = ui_kit::truncate_galley(
                                    &painter,
                                    display,
                                    egui::FontId::proportional(theme::FONT_SM),
                                    title_color,
                                    max_text_w,
                                );
                                painter.galley(
                                    egui::pos2(text_x, tab_rect.center().y - galley.size().y * 0.5),
                                    galley,
                                    title_color,
                                );
                            }

                            if close_resp
                                .on_hover_text("Close tab (Ctrl+Shift+W)")
                                .clicked()
                                || tab_resp.clicked_by(egui::PointerButton::Middle)
                            {
                                close_pane_id = Some(pane_id);
                            } else if tab_resp.clicked() {
                                if is_active && !is_renaming {
                                    self.tab_rename_pane_id = Some(pane_id);
                                    self.tab_rename_text = display.clone();
                                } else {
                                    if let PaneContent::Terminal(sid) =
                                        &self.pane_state.panes[i].content
                                    {
                                        self.completed_badges.remove(sid);
                                    }
                                }
                                clicked_pane_id = Some(pane_id);
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
                            let ctx_cwd = self.pane_cwd(i);
                            tab_resp.context_menu(|ui| {
                                if ui.button("Rename tab").clicked() {
                                    self.tab_rename_pane_id = Some(pane_id);
                                    self.tab_rename_text = display.clone();
                                    ui.close_menu();
                                }
                                if let Some(cwd) = &ctx_cwd {
                                    ui.separator();
                                    if ui.button("Copy path").clicked() {
                                        if let Ok(mut clip) = arboard::Clipboard::new() {
                                            let _ =
                                                clip.set_text(cwd.to_string_lossy().into_owned());
                                        }
                                        ui.close_menu();
                                    }
                                    if ui.button("Open in file explorer").clicked() {
                                        let _ = open::that(cwd);
                                        ui.close_menu();
                                    }
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
                                ui.separator();
                                if ui.button("Close").clicked() {
                                    close_pane_id = Some(pane_id);
                                    ui.close_menu();
                                }
                            });
                        }
                    });
                });
            tab_scroll_offset_x = scroll_out.state.offset.x;
        });

        // ── Tab-bar action buttons (split / close-all) ──────────
        ui.allocate_ui_at_rect(tab_actions_rect, |ui| {
            ui.painter()
                .rect_filled(tab_actions_rect, theme::R_NONE, theme::active().bg_panel);
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

            // Split horizontal (side-by-side)
            let split_h_rect =
                egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
            let split_h_resp = ui_kit::icon_button(
                ui,
                self.vp_id("tab_split_h"),
                split_h_rect,
                ui_kit::icon::COLUMNS_2,
                theme::FONT_SM,
                t.subtext1,
                ui_kit::IconButtonStyle::Default,
            );
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
            let split_v_resp = ui_kit::icon_button(
                ui,
                self.vp_id("tab_split_v"),
                split_v_rect,
                ui_kit::icon::ROWS_2,
                theme::FONT_SM,
                t.subtext1,
                ui_kit::IconButtonStyle::Default,
            );
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
                ui_kit::icon::X,
                theme::FONT_SM,
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
        _is_focused: bool,
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
            ui.painter()
                .rect_filled(scroll_rect, theme::R_NONE, t.bg_panel);

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
                                if !self.pane_passes_tab_filter(&self.pane_state.panes[pane_idx]) {
                                    return None;
                                }
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

                            let tab_w = if is_active {
                                theme::TAB_W_ACTIVE
                            } else {
                                theme::TAB_W
                            };
                            let (_, tab_rect) = ui.allocate_space(egui::vec2(tab_w, tab_h));

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

                            let t = theme::active();
                            let visual = tab_visual(is_active, t);
                            let title_color = visual.text;

                            if let Some(fill) = visual.fill {
                                painter.rect_filled(tab_rect, theme::R_NONE, fill);
                            }
                            if visual.outline {
                                painter.rect_stroke(
                                    tab_rect,
                                    theme::R_NONE,
                                    egui::Stroke::new(theme::STROKE_THIN, t.border),
                                );
                            }
                            if let Some(bar) = visual.bar {
                                painter.rect_filled(
                                    egui::Rect::from_min_max(
                                        egui::pos2(
                                            tab_rect.min.x,
                                            tab_rect.max.y - theme::TAB_SELECTED_BAR_H,
                                        ),
                                        tab_rect.max,
                                    ),
                                    theme::R_NONE,
                                    bar,
                                );
                            } else if tab_hover_t > 0.01 {
                                let hover_bg = theme::lerp_color(
                                    egui::Color32::TRANSPARENT,
                                    t.bg_row_hover,
                                    tab_hover_t,
                                );
                                painter.rect_filled(tab_rect, theme::R_NONE, hover_bg);
                            }

                            // Workspace colour strip on left edge
                            if let Some(c) = ws_color {
                                painter.rect_filled(
                                    workspace_strip_rect(tab_rect),
                                    theme::R_NONE,
                                    theme::from_rgb(*c),
                                );
                            }

                            let status_color = match &self.pane_state.panes[pane_idx].content {
                                PaneContent::Terminal(sid) => {
                                    let alive =
                                        self.session_state.find(*sid).is_some_and(|entry| {
                                            entry.alive.load(std::sync::atomic::Ordering::Relaxed)
                                        });
                                    Some(if alive { t.success } else { t.fg_muted })
                                }
                                PaneContent::DeferredTerminal { .. } => Some(t.fg_muted),
                                _ => None,
                            };

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
                                    ui_kit::icon::X,
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
                            let status_offset = if status_color.is_some() { 10.0 } else { 0.0 };
                            let color_strip_offset = if ws_color.is_some() {
                                theme::TAB_COLOR_STRIP_W
                            } else {
                                0.0
                            };
                            let text_x = tab_rect.min.x
                                + theme::TAB_PAD_X
                                + color_strip_offset
                                + status_offset;

                            if let Some(color) = status_color {
                                painter.circle_filled(
                                    egui::pos2(
                                        tab_rect.min.x
                                            + theme::TAB_PAD_X
                                            + color_strip_offset
                                            + 3.0,
                                        tab_rect.center().y,
                                    ),
                                    3.0,
                                    color,
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
                                let edit_id =
                                    egui::Id::new(("gtab_rename_edit", group_id, pane_id));
                                ui.painter().rect_filled(edit_rect, theme::R_SM, t.surface1);
                                let resp = ui
                                    .allocate_ui_at_rect(edit_rect, |ui| {
                                        ui.add(
                                            egui::TextEdit::singleline(&mut self.tab_rename_text)
                                                .id(edit_id)
                                                .desired_width(edit_rect.width())
                                                .font(egui::FontId::proportional(theme::FONT_SM))
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
                                let max_text_w =
                                    (close_rect.min.x - theme::SP_1 - text_x).max(theme::SP_0);
                                let galley = ui_kit::truncate_galley(
                                    &painter,
                                    display,
                                    egui::FontId::proportional(theme::FONT_SM),
                                    title_color,
                                    max_text_w,
                                );
                                painter.galley(
                                    egui::pos2(text_x, tab_rect.center().y - galley.size().y * 0.5),
                                    galley,
                                    title_color,
                                );
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
                            } else if tab_resp.clicked() {
                                if is_active && !is_renaming {
                                    self.tab_rename_pane_id = Some(pane_id);
                                    self.tab_rename_text = display.clone();
                                } else {
                                    if let PaneContent::Terminal(sid) =
                                        &self.pane_state.panes[pane_idx].content
                                    {
                                        self.completed_badges.remove(sid);
                                    }
                                }
                                clicked_pane_id = Some(pane_id);
                                clicked_group_id = Some(group_id);
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
                            let ctx_cwd = self.pane_cwd(pane_idx);
                            tab_resp.context_menu(|ui| {
                                if ui.button("Rename tab").clicked() {
                                    self.tab_rename_pane_id = Some(pane_id);
                                    self.tab_rename_text = display.clone();
                                    ui.close_menu();
                                }
                                if let Some(cwd) = &ctx_cwd {
                                    ui.separator();
                                    if ui.button("Copy path").clicked() {
                                        if let Ok(mut clip) = arboard::Clipboard::new() {
                                            let _ =
                                                clip.set_text(cwd.to_string_lossy().into_owned());
                                        }
                                        ui.close_menu();
                                    }
                                    if ui.button("Open in file explorer").clicked() {
                                        let _ = open::that(cwd);
                                        ui.close_menu();
                                    }
                                }
                                ui.separator();
                                if ui.button("Close").clicked() {
                                    close_pane_id = Some(pane_id);
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
            ui.painter()
                .rect_filled(tab_actions_rect, theme::R_NONE, t.bg_panel);
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

            // Split horizontal (side-by-side)
            let split_h_rect =
                egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
            let split_h_id = self.vp_id(&format!("gtab_split_h_{group_id}"));
            let split_h_resp = ui_kit::icon_button(
                ui,
                split_h_id,
                split_h_rect,
                ui_kit::icon::COLUMNS_2,
                theme::FONT_SM,
                t.subtext1,
                ui_kit::IconButtonStyle::Default,
            );
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
            let split_v_resp = ui_kit::icon_button(
                ui,
                self.vp_id(&format!("gtab_split_v_{group_id}")),
                split_v_rect,
                ui_kit::icon::ROWS_2,
                theme::FONT_SM,
                t.subtext1,
                ui_kit::IconButtonStyle::Default,
            );
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
                ui_kit::icon::X,
                theme::FONT_SM,
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

    /// Directory path for a pane: CWD for terminals, parent dir for file-based panes.
    fn pane_cwd(&self, pane_index: usize) -> Option<std::path::PathBuf> {
        match &self.pane_state.panes[pane_index].content {
            PaneContent::Terminal(sid) => self
                .session_state
                .find(*sid)
                .map(|e| e.session.read().cwd.clone())
                .filter(|p| !p.as_os_str().is_empty()),
            PaneContent::DeferredTerminal { cwd, .. } => {
                cwd.clone().filter(|p| !p.as_os_str().is_empty())
            }
            PaneContent::FileEditor(ed) => ed.path.parent().map(|p| p.to_path_buf()),
            PaneContent::FileDiff(d) => d.path.parent().map(|p| p.to_path_buf()),
            PaneContent::ConflictResolver(cr) => cr.path.parent().map(|p| p.to_path_buf()),
            PaneContent::NoteEditor(_) => None,
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
            PaneContent::FileDiff(d) => d.workspace_id.and_then(|id| {
                self.workspace_store
                    .workspaces
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| w.color)
            }),
            PaneContent::ConflictResolver(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeId;

    #[test]
    fn active_tab_style() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let t = theme::active();

        let selected = tab_visual(true, t);
        assert_eq!(selected.bar, Some(t.accent));
        assert_eq!(selected.fill, None);
        assert!(!selected.outline);
        assert_eq!(selected.text, t.fg_strong);

        let unselected = tab_visual(false, t);
        assert_eq!(unselected.bar, None);
        assert_eq!(unselected.fill, None);
        assert!(!unselected.outline);
        assert_eq!(unselected.text, t.fg_muted);
    }

    #[test]
    fn workspace_strip_is_thin() {
        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(12.0, 8.0), egui::vec2(180.0, theme::TAB_H));

        let strip = workspace_strip_rect(tab_rect);

        assert_eq!(strip.width(), theme::TAB_COLOR_STRIP_W);
        assert_eq!(strip.height(), tab_rect.height());
        assert_eq!(strip.min, tab_rect.min);
    }
}
