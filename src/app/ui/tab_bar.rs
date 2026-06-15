use super::super::drag;
use super::super::pane::PaneContent;
use super::super::title::effective_title;
use super::super::App;
use crate::pane_tree::SplitDir;
use crate::theme;
use crate::ui_kit;

/// Result of rendering the tab bar, consumed by the caller.
pub(in crate::app) struct TabBarResult {
    pub close_pane_id: Option<u32>,
    pub clicked_pane_id: Option<u32>,
    pub split_request: Option<SplitDir>,
    /// Move the given tab into a split alongside the currently active pane.
    pub move_to_split: Option<(u32, SplitDir)>,
}

impl App {
    /// Render the tab strip (horizontally scrollable) and the action buttons to its right.
    ///
    /// Returns deferred actions (close, click, split) to be applied after the closure.
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
                .rect_filled(tab_bar_rect, 0.0, theme::active().surface0);
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

                            let hbg = theme::header_bg(ws_color, is_active);
                            let title_color = match ws_color {
                                Some(c) => theme::text_on(theme::tinted(
                                    c,
                                    if is_active { 0.75 } else { 0.35 },
                                )),
                                None => {
                                    if is_active {
                                        theme::active().text
                                    } else {
                                        theme::active().subtext1
                                    }
                                }
                            };

                            let painter = ui.painter().clone();
                            painter.rect_filled(tab_rect, 0.0, hbg);

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

                            // Bottom highlight on active tab
                            if is_active {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(
                                            tab_rect.min.x,
                                            tab_rect.max.y - theme::TAB_ACTIVE_HIGHLIGHT_H,
                                        ),
                                        egui::vec2(theme::TAB_W, theme::TAB_ACTIVE_HIGHLIGHT_H),
                                    ),
                                    0.0,
                                    theme::active().accent,
                                );
                                // Active indicator dot
                                let dot_radius = 3.0;
                                let dot_x = tab_rect.min.x
                                    + theme::TAB_PAD_X
                                    + if ws_color.is_some() {
                                        theme::TAB_COLOR_STRIP_W
                                    } else {
                                        0.0
                                    }
                                    + dot_radius;
                                let dot_color = ws_color
                                    .map(theme::from_rgb)
                                    .unwrap_or(theme::active().accent);
                                painter.circle_filled(
                                    egui::pos2(dot_x, tab_rect.center().y),
                                    dot_radius,
                                    dot_color,
                                );
                            }

                            // Flash feedback overlay on tab
                            self.flash.render_on_rect(
                                &painter,
                                tab_rect,
                                crate::app::feedback::FlashTarget::Tab(pane_id),
                            );

                            // Right-edge separator between tabs.
                            // Use a subtle connector for tabs in the same split group.
                            let same_group_next = vis_pos + 1 < visible_roots.len()
                                && visible_roots[vis_pos] == visible_roots[vis_pos + 1]
                                && visible_roots[vis_pos].is_some_and(|r| {
                                    self.pane_state
                                        .pane_trees
                                        .get(&r)
                                        .is_some_and(|t| t.leaf_ids().len() > 1)
                                });
                            if same_group_next {
                                let mid_y = tab_rect.center().y;
                                let dot_h = 4.0;
                                painter.rect_filled(
                                    egui::Rect::from_center_size(
                                        egui::pos2(tab_rect.max.x, mid_y),
                                        egui::vec2(theme::STROKE_THIN, dot_h),
                                    ),
                                    0.0,
                                    theme::active().surface2.gamma_multiply(0.5),
                                );
                            } else {
                                painter.rect_filled(
                                    egui::Rect::from_min_size(
                                        egui::pos2(
                                            tab_rect.max.x - theme::STROKE_THIN,
                                            tab_rect.min.y,
                                        ),
                                        egui::vec2(theme::STROKE_THIN, tab_h),
                                    ),
                                    0.0,
                                    theme::active().surface2,
                                );
                            }

                            // Register tab-wide click first (lower z-order); close button
                            // is registered second so it has higher priority in egui's
                            // last-registered-wins model for overlapping regions.
                            let tab_resp = ui.interact(
                                tab_rect,
                                egui::Id::new(("tab_click", pane_id)),
                                egui::Sense::click_and_drag(),
                            );
                            if tab_resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }

                            // Close button (x)
                            let close_rect = egui::Rect::from_min_size(
                                egui::pos2(tab_rect.max.x - theme::BTN_W, tab_rect.min.y),
                                egui::vec2(theme::BTN_W, tab_h),
                            );
                            let close_resp = ui_kit::icon_button(
                                ui,
                                egui::Id::new(("tab_close", pane_id)),
                                close_rect,
                                "\u{00d7}",
                                theme::FONT_TERM,
                                theme::active().danger_fg,
                                ui_kit::IconButtonStyle::Danger,
                            );

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
                .rect_filled(tab_actions_rect, 0.0, theme::active().surface0);
            // Left separator
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    tab_actions_rect.left_top(),
                    egui::vec2(theme::STROKE_THIN, tab_h),
                ),
                0.0,
                theme::active().surface2,
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
                    .rect_filled(split_h_rect, theme::R_SM, t.surface2);
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
                    .rect_filled(split_v_rect, theme::R_SM, t.surface2);
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
            }
        });

        TabBarResult {
            close_pane_id,
            clicked_pane_id,
            split_request,
            move_to_split,
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
}
