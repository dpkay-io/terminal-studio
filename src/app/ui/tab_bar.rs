use super::super::pane::PaneContent;
use super::super::title::effective_title;
use super::super::App;
use crate::pane_tree::SplitDir;
use crate::theme;

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
        ui.allocate_ui_at_rect(tab_bar_rect, |ui| {
            ui.painter()
                .rect_filled(tab_bar_rect, 0.0, theme::active().surface0);
            egui::ScrollArea::horizontal()
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

                        for (i, display) in &display_texts {
                            let i = *i;
                            let pane_id = self.pane_state.panes[i].id;
                            let is_active = active_pane_id_snap.is_some_and(|apid| {
                                pane_id == apid
                                    || self
                                        .pane_state
                                        .pane_trees
                                        .get(&pane_id)
                                        .is_some_and(|t| t.leaf_ids().contains(&apid))
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
                                    theme::active().text,
                                );
                            }

                            // Flash feedback overlay on tab
                            self.flash.render_on_rect(
                                &painter,
                                tab_rect,
                                crate::app::feedback::FlashTarget::Tab(pane_id),
                            );

                            // Right-edge separator between tabs
                            painter.rect_filled(
                                egui::Rect::from_min_size(
                                    egui::pos2(tab_rect.max.x - theme::STROKE_THIN, tab_rect.min.y),
                                    egui::vec2(theme::STROKE_THIN, tab_h),
                                ),
                                0.0,
                                theme::active().surface2,
                            );

                            // Register tab-wide click first (lower z-order); close button
                            // is registered second so it has higher priority in egui's
                            // last-registered-wins model for overlapping regions.
                            let tab_resp = ui.interact(
                                tab_rect,
                                egui::Id::new(("tab_click", pane_id)),
                                egui::Sense::click_and_drag(),
                            );

                            // Close button (x)
                            let close_rect = egui::Rect::from_min_size(
                                egui::pos2(tab_rect.max.x - theme::BTN_W, tab_rect.min.y),
                                egui::vec2(theme::BTN_W, tab_h),
                            );
                            let close_resp = ui.interact(
                                close_rect,
                                egui::Id::new(("tab_close", pane_id)),
                                egui::Sense::click(),
                            );
                            if close_resp.hovered() {
                                painter.rect_filled(close_rect, 0.0, theme::active().danger_bg);
                            }
                            painter.text(
                                close_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "\u{00d7}",
                                egui::FontId::proportional(14.0),
                                theme::active().danger_fg,
                            );

                            // Title text (clipped before close button)
                            let text_x = tab_rect.min.x
                                + theme::TAB_PAD_X
                                + if ws_color.is_some() {
                                    theme::TAB_COLOR_STRIP_W
                                } else {
                                    0.0
                                };

                            let is_renaming = self.tab_rename_pane_id == Some(pane_id);
                            if is_renaming {
                                let edit_rect = egui::Rect::from_min_max(
                                    egui::pos2(text_x, tab_rect.min.y + 2.0),
                                    egui::pos2(close_rect.min.x - theme::SP_1, tab_rect.max.y - 2.0),
                                );
                                let edit_id = egui::Id::new(("tab_rename_edit", pane_id));
                                let resp = ui.allocate_ui_at_rect(edit_rect, |ui| {
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.tab_rename_text)
                                            .id(edit_id)
                                            .desired_width(edit_rect.width())
                                            .font(egui::FontId::proportional(theme::FONT_UI_MD))
                                            .frame(false)
                                            .text_color(title_color),
                                    )
                                }).inner;
                                if !resp.has_focus() {
                                    ui.memory_mut(|m| m.request_focus(edit_id));
                                }
                                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                                if enter {
                                    let new_title = self.tab_rename_text.trim().to_string();
                                    if !new_title.is_empty() {
                                        if let PaneContent::Terminal(sid) = &self.pane_state.panes[i].content {
                                            if let Some(entry) = self.session_state.sessions.iter().find(|e| e.id == *sid) {
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
                                if let PaneContent::Terminal(sid) = &self.pane_state.panes[i].content {
                                    self.completed_badges.remove(sid);
                                }
                            }

                            // Tab drag-to-reorder
                            if tab_resp.drag_started() {
                                self.tab_drag_source = Some(i);
                            }
                            if let Some(drag_idx) = self.tab_drag_source {
                                if tab_resp.hovered() && drag_idx != i {
                                    let indicator_x = if drag_idx < i {
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
                                }
                            }

                            // Right-click context menu for tab operations.
                            let can_move_to_split = visible_indices.len() >= 2;
                            let extra_window_names: Vec<(u64, String)> = self
                                .extra_windows
                                .iter()
                                .map(|w| (w.workspace_id, w.title.clone()))
                                .collect();
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

                                ui.separator();

                                let t = theme::active();
                                ui.label(
                                    egui::RichText::new("Move tab to window\u{2026}")
                                        .size(theme::FONT_UI_MD)
                                        .color(t.fg_secondary),
                                );
                                ui.separator();
                                if extra_window_names.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No other windows")
                                            .italics()
                                            .color(t.fg_muted),
                                    );
                                } else {
                                    for (_, win_title) in &extra_window_names {
                                        ui.add_enabled_ui(false, |ui| {
                                            let _ = ui.button(win_title);
                                        });
                                    }
                                    ui.label(
                                        egui::RichText::new("(tab move coming in Phase D)")
                                            .italics()
                                            .size(theme::FONT_UI_SM)
                                            .color(t.fg_muted),
                                    );
                                }
                            });
                        }
                    });
                });
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
            let mut x = tab_actions_rect.min.x + 2.0;

            let icon_stroke = egui::Stroke::new(1.2, t.subtext1);
            let icon_hover_stroke = egui::Stroke::new(1.2, t.text);
            let icon_inset = 6.0_f32;

            // Split horizontal (side-by-side)
            let split_h_rect =
                egui::Rect::from_min_size(egui::pos2(x, tab_actions_rect.min.y), icon_sz);
            let split_h_resp = ui.interact(
                split_h_rect,
                self.vp_id("tab_split_h"),
                egui::Sense::click(),
            );
            let sh_stroke = if split_h_resp.hovered() {
                ui.painter().rect_filled(split_h_rect, theme::R_SM, t.surface2);
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
            let sv_stroke = if split_v_resp.hovered() {
                ui.painter().rect_filled(split_v_rect, theme::R_SM, t.surface2);
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
            let close_all_resp = ui.interact(
                close_all_rect,
                self.vp_id("tab_close_all"),
                egui::Sense::click(),
            );
            if close_all_resp.hovered() {
                ui.painter().rect_filled(close_all_rect, theme::R_SM, t.danger_bg);
            }
            ui.painter().text(
                close_all_rect.center(),
                egui::Align2::CENTER_CENTER,
                "\u{2716}",
                egui::FontId::proportional(12.0),
                t.danger_fg,
            );
            if close_all_resp.on_hover_text("Close all sessions in this workspace").clicked() {
                self.show_close_all_confirm = true;
            }
        });

        // Tab drag-to-reorder: finalize on pointer release
        if self.tab_drag_source.is_some() && ui.input(|i| i.pointer.any_released()) {
            if let Some(drag_idx) = self.tab_drag_source.take() {
                let pointer_pos = ui.input(|i| i.pointer.hover_pos());
                if let Some(pos) = pointer_pos {
                    // Find which tab the pointer is over by index
                    let tab_count = visible_indices.len();
                    if tab_count > 1 {
                        let tab_bar_x = ui.min_rect().min.x;
                        let rel_x = pos.x - tab_bar_x;
                        let target_i = ((rel_x / theme::TAB_W) as usize).min(tab_count - 1);
                        let target_vis = visible_indices.get(target_i).copied();
                        let drag_vis = visible_indices.get(drag_idx).copied();
                        if let (Some(from), Some(to)) = (drag_vis, target_vis) {
                            if from != to {
                                let pane = self.pane_state.panes.remove(from);
                                let insert_at = if to > from { to - 1 } else { to };
                                self.pane_state.panes.insert(insert_at, pane);
                            }
                        }
                    }
                }
            }
        }

        TabBarResult {
            close_pane_id,
            clicked_pane_id,
            split_request,
            move_to_split,
        }
    }

    /// Compute the display text for a tab at the given pane index.
    fn tab_display_text(&self, pane_index: usize) -> String {
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
        }
    }
}
