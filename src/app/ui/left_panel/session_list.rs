use super::super::super::pane::PaneContent;
use super::super::super::title::effective_title;
use super::super::super::{App, CloseAllTarget};
use super::SessionListActions;
use crate::app::ui::search_bar::{search_bar, search_bar_persistent};
use crate::pty::foreground::ForegroundProcess;
use crate::theme;
use crate::ui_kit;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

impl App {
    /// Render the sessions section: header with "+ New" menu, search bars,
    /// workspace filter dropdown, and either global search results or the
    /// normal session list.
    pub(in crate::app) fn render_session_section(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        active_fg: &Option<ForegroundProcess>,
        actions: &mut SessionListActions,
    ) {
        let shells = self.available_shells.clone();

        // ── Header row ───────────────────────────────────────────────
        self.render_session_header(ui, active_fg, actions, &shells);
        ui.separator();

        // ── Session search bar (always visible) ─────────────────────
        if !self.show_global_search {
            let focus = self.session_search_active;
            let search_id = self.vp_id("session_search_input");
            let sb = search_bar_persistent(
                ui,
                &mut self.session_search_query,
                "\u{1f50d}",
                "Filter sessions\u{2026}",
                search_id,
                focus,
            );
            if focus {
                self.session_search_active = false;
            }
            if sb.escaped {
                self.session_search_query.clear();
            }
            ui.add_space(theme::SP_1);
        }

        // ── Global search bar (search across all sessions) ──────────
        if self.show_global_search {
            let search_id = self.vp_id("global_search_input");
            let sb = search_bar(
                ui,
                &mut self.global_search_query,
                "\u{1f50d}",
                "Search in all sessions\u{2026}",
                search_id,
            );
            if sb.response.changed() {
                self.global_search_debouncer
                    .update(&self.global_search_query);
                self.global_search_selected = 0;
            }
            if sb.escaped {
                self.show_global_search = false;
                self.global_search_query.clear();
                self.global_search_debouncer.reset();
                self.global_search_selected = 0;
                self.workers.search_worker.cancel();
            }

            // Status line
            {
                let results = self.workers.search_worker.results();
                let status = if results.query.is_empty() {
                    String::new()
                } else if !results.completed {
                    "Searching\u{2026}".to_string()
                } else {
                    let n = results.matches.len();
                    if n == 0 {
                        "No matches".to_string()
                    } else {
                        format!("{} match{}", n, if n == 1 { "" } else { "es" })
                    }
                };
                if !status.is_empty() {
                    ui.label(
                        egui::RichText::new(&status)
                            .size(theme::FONT_UI_XS)
                            .color(theme::active().subtext0),
                    );
                }
            }
            ui.add_space(theme::SP_1);
        }

        let session_filter = self.session_search_query.clone();

        // ── Workspace filter dropdown ────────────────────────────────
        self.render_workspace_filter_dropdown(ui);

        // ── Session content (global search results or normal list) ───
        if self.show_global_search {
            self.render_global_search_results(ctx, ui, actions);
        } else {
            self.render_normal_session_list(ui, &session_filter, actions);
        }
    }

    /// Render the session header row with "+ New", "Close All", and "Duplicate" buttons.
    fn render_session_header(
        &mut self,
        ui: &mut egui::Ui,
        active_fg: &Option<ForegroundProcess>,
        actions: &mut SessionListActions,
        shells: &[crate::pty::ShellKind],
    ) {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), theme::HEADER_H),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new("Sessions")
                        .strong()
                        .size(theme::FONT_UI_MD),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(
                        egui::RichText::new("+ New \u{25be}").size(theme::FONT_UI_MD),
                        |ui| {
                            for shell in shells {
                                if ui.button(shell.display_name()).clicked() {
                                    actions.spawn_new_session = Some(shell.clone());
                                    ui.close_menu();
                                }
                            }
                            ui.separator();
                            if ui.button("Open Folder\u{2026}").clicked() {
                                let ctx = ui.ctx().clone();
                                std::thread::Builder::new()
                                    .name("folder-picker".into())
                                    .spawn(move || {
                                        let result = rfd::FileDialog::new().pick_folder();
                                        if let Some(path) = result {
                                            ctx.data_mut(|d| {
                                                d.insert_temp(
                                                    egui::Id::new("pending_folder_pick"),
                                                    path,
                                                );
                                            });
                                            ctx.request_repaint();
                                        }
                                    })
                                    .ok();
                                ui.close_menu();
                            }
                        },
                    )
                    .response
                    .on_hover_text("New terminal (Ctrl+Shift+T)");
                    {
                        let (target, visible_count) =
                            if let Some(ws_filter) = self.session_workspace_filter {
                                let cnt = self
                                    .pane_state
                                    .panes
                                    .iter()
                                    .filter(|p| {
                                        Self::pane_group(
                                            &self.session_state.sessions,
                                            &self.workspace_store,
                                            p,
                                        ) == ws_filter
                                    })
                                    .count();
                                (CloseAllTarget::Group(ws_filter), cnt)
                            } else {
                                (CloseAllTarget::All, self.pane_state.panes.len())
                            };
                        if visible_count > 1 {
                            let btn_label = if self.session_workspace_filter.is_some() {
                                "Close Shown"
                            } else {
                                "Close All"
                            };
                            if ui
                                .button(egui::RichText::new(btn_label).size(theme::FONT_UI_MD))
                                .on_hover_text("Close all visible sessions")
                                .clicked()
                            {
                                self.close_all_target = target;
                                self.show_close_all_confirm = true;
                                self.close_all_frames_open = 0;
                            }
                        }
                    }
                    if let Some(ref fp) = active_fg {
                        if ui
                            .button(egui::RichText::new("Duplicate").size(theme::FONT_UI_MD))
                            .on_hover_text(format!("Duplicate: {} (Ctrl+Shift+K)", fp.name))
                            .clicked()
                        {
                            actions.duplicate_session = true;
                        }
                    }
                });
            },
        );
    }

    /// Render the workspace filter dropdown at the top of the session list.
    fn render_workspace_filter_dropdown(&mut self, ui: &mut egui::Ui) {
        let ws_names: Vec<(Option<Option<u64>>, String)> = {
            let mut items: Vec<(Option<Option<u64>>, String)> =
                vec![(None, "All Workspaces".to_string())];
            for w in &self.workspace_store.workspaces {
                items.push((Some(Some(w.id)), w.name.clone()));
            }
            items.push((Some(None), "Other".to_string()));
            items
        };
        let selected_label = match self.session_workspace_filter {
            None => "All Workspaces".to_string(),
            Some(None) => "Other".to_string(),
            Some(Some(id)) => self
                .workspace_store
                .workspaces
                .iter()
                .find(|w| w.id == id)
                .map(|w| w.name.clone())
                .unwrap_or_else(|| {
                    self.session_workspace_filter = None;
                    "All Workspaces".to_string()
                }),
        };

        let t = theme::active();
        let full_w = ui.available_width();
        let popup_id = self.vp_id("ws_filter_popup");
        let is_open = ui.memory(|m| m.is_popup_open(popup_id));

        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(full_w, theme::SEARCH_BAR_H),
            egui::Sense::click(),
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup_id));
        }

        let border_color = if is_open {
            t.border_focus
        } else {
            t.border_subtle
        };
        ui.painter().rect_filled(rect, theme::R_SM, t.bg_input);
        ui.painter().rect_stroke(
            rect,
            theme::R_SM,
            egui::Stroke::new(theme::STROKE_THIN, border_color),
        );

        let inner = rect.shrink2(egui::vec2(theme::SP_3, theme::SP_0));
        let chevron = if is_open { "\u{25b4}" } else { "\u{25be}" };
        let chevron_galley = ui.fonts(|f| {
            f.layout_no_wrap(
                chevron.to_string(),
                egui::FontId::proportional(theme::FONT_UI_XS),
                t.fg_muted,
            )
        });
        let chevron_w = chevron_galley.size().x;
        let chevron_x = inner.right() - chevron_w;
        let chevron_y = inner.center().y - chevron_galley.size().y / 2.0;
        ui.painter()
            .galley(egui::pos2(chevron_x, chevron_y), chevron_galley, t.fg_muted);

        let text_clip =
            egui::Rect::from_min_max(inner.min, egui::pos2(chevron_x - theme::SP_2, inner.max.y));
        let text_galley = ui.fonts(|f| {
            f.layout_no_wrap(
                selected_label,
                egui::FontId::proportional(theme::FONT_UI_MD),
                t.text,
            )
        });
        let text_y = inner.center().y - text_galley.size().y / 2.0;
        ui.painter().with_clip_rect(text_clip).galley(
            egui::pos2(inner.left(), text_y),
            text_galley,
            t.text,
        );

        if is_open {
            egui::popup::popup_below_widget(
                ui,
                popup_id,
                &resp,
                egui::PopupCloseBehavior::CloseOnClick,
                |ui: &mut egui::Ui| {
                    ui.set_min_width(full_w - 2.0 * theme::SP_3);
                    for (val, name) in &ws_names {
                        let is_selected = self.session_workspace_filter == *val;
                        if ui.selectable_label(is_selected, name).clicked() {
                            self.session_workspace_filter = *val;
                        }
                    }
                },
            );
        }

        ui.add_space(theme::SP_1);
    }

    /// Render the global search results list with keyboard navigation.
    fn render_global_search_results(
        &mut self,
        ctx: &egui::Context,
        ui: &mut egui::Ui,
        actions: &mut SessionListActions,
    ) {
        let results = self.workers.search_worker.results();
        let matches = results.matches.clone();
        drop(results);

        // Arrow key nav + Enter
        if !matches.is_empty() {
            let (up, down, enter) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                )
            });
            if up && self.global_search_selected > 0 {
                self.global_search_selected -= 1;
            }
            if down && self.global_search_selected + 1 < matches.len() {
                self.global_search_selected += 1;
            }
            if enter {
                let sel = self.global_search_selected.min(matches.len() - 1);
                let m = &matches[sel];
                actions.clicked_sidebar_pane_id = self
                    .pane_state
                    .panes
                    .iter()
                    .find(|p| matches!(&p.content, PaneContent::Terminal(sid) if *sid == m.session_id))
                    .map(|p| p.id);
                self.show_global_search = false;
                self.global_search_query.clear();
                self.global_search_debouncer.reset();
                self.global_search_selected = 0;
                self.workers.search_worker.cancel();
            }
        }

        egui::ScrollArea::vertical()
            .id_source(self.vp_id("global_search_scroll"))
            .show(ui, |ui| {
                let t = theme::active();
                let mut current_session: Option<u32> = None;
                for (i, m) in matches.iter().enumerate() {
                    if current_session != Some(m.session_id) {
                        current_session = Some(m.session_id);
                        if i > 0 {
                            ui.add_space(theme::SP_2);
                        }
                        ui.label(
                            egui::RichText::new(&m.session_title)
                                .size(theme::FONT_UI_SM)
                                .strong()
                                .color(t.blue),
                        );
                        ui.add_space(theme::SP_1);
                    }

                    let selected = i == self.global_search_selected;
                    let bg = if selected {
                        t.bg_row_active
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    let (resp, painter) = ui.allocate_painter(
                        egui::vec2(ui.available_width(), 20.0),
                        egui::Sense::click(),
                    );
                    let row_rect = resp.rect;
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }

                    if selected || resp.hovered() {
                        let hover_bg = if selected { bg } else { t.bg_row_hover };
                        painter.rect_filled(row_rect, theme::R_SM, hover_bg);
                    }

                    let text = &m.line_text;
                    let max_chars = ((row_rect.width() / 7.0) as usize).max(20);
                    let display = if text.chars().count() > max_chars {
                        let byte_end = text
                            .char_indices()
                            .nth(max_chars)
                            .map(|(i, _)| i)
                            .unwrap_or(text.len());
                        format!("{}\u{2026}", &text[..byte_end])
                    } else {
                        text.clone()
                    };
                    painter
                        .with_clip_rect(row_rect)
                        .text(
                            egui::pos2(
                                row_rect.min.x + 4.0,
                                row_rect.center().y,
                            ),
                            egui::Align2::LEFT_CENTER,
                            &display,
                            egui::FontId::monospace(theme::FONT_UI_SM),
                            if selected { t.text } else { t.subtext0 },
                        );

                    if resp.clicked() {
                        actions.clicked_sidebar_pane_id = self
                            .pane_state
                            .panes
                            .iter()
                            .find(|p| matches!(&p.content, PaneContent::Terminal(sid) if *sid == m.session_id))
                            .map(|p| p.id);
                        self.show_global_search = false;
                        self.global_search_query.clear();
                        self.global_search_debouncer.reset();
                        self.global_search_selected = 0;
                        self.workers.search_worker.cancel();
                    }
                }
            });
    }

    /// Render the normal (non-search) session list.
    fn render_normal_session_list(
        &mut self,
        ui: &mut egui::Ui,
        session_filter: &str,
        actions: &mut SessionListActions,
    ) {
        let outer_w = ui.available_width();
        ui.spacing_mut().scroll.bar_width = 0.0;
        ui.spacing_mut().scroll.floating_allocated_width = 0.0;
        ui.spacing_mut().scroll.floating_width = 0.0;
        egui::ScrollArea::vertical()
            .id_source(self.vp_id("sessions_scroll"))
            .show(ui, |ui| {
                ui.set_min_width(outer_w);
                ui.set_max_width(outer_w);
                let matcher = SkimMatcherV2::default();
                for pane in self.pane_state.panes.iter() {
                    let (label, ws_color, dimmed): (String, Option<[u8; 3]>, bool) = match &pane
                        .content
                    {
                        PaneContent::Terminal(sid) => {
                            if let Some(e) = self.session_state.find(*sid) {
                                let (title, cwd) = {
                                    let s = e.session.read();
                                    (s.title(), s.cwd.clone())
                                };
                                let ws = if cwd.as_os_str().is_empty() {
                                    None
                                } else {
                                    self.workspace_store.find_for_cwd(&cwd)
                                };
                                let color = ws.map(|w| w.color);
                                let ws_name = ws.map(|w| w.name.as_str());
                                let fg = self.workers.foreground_worker.get(e.id);
                                (
                                    effective_title(
                                        &title,
                                        &cwd,
                                        fg.as_ref(),
                                        Some(&e.shell),
                                        ws_name,
                                    ),
                                    color,
                                    false,
                                )
                            } else {
                                ("(missing)".to_string(), None, true)
                            }
                        }
                        PaneContent::DeferredTerminal {
                            cwd, saved_title, ..
                        } => {
                            let cwd_path = cwd.clone().unwrap_or_default();
                            let ws = if cwd_path.as_os_str().is_empty() {
                                None
                            } else {
                                self.workspace_store.find_for_cwd(&cwd_path)
                            };
                            let color = ws.map(|w| w.color);
                            let ws_name = ws.map(|w| w.name.as_str());
                            let mut text =
                                if let Some(t) = saved_title.as_deref().filter(|s| !s.is_empty()) {
                                    t.to_string()
                                } else {
                                    effective_title("", &cwd_path, None, None, ws_name)
                                };
                            if text.is_empty() {
                                text = "(restored)".to_string();
                            }
                            (text, color, true)
                        }
                        PaneContent::FileEditor(ed) => {
                            let text = ed
                                .path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| ed.path.display().to_string());
                            let color = ed.workspace_id.and_then(|id| {
                                self.workspace_store
                                    .workspaces
                                    .iter()
                                    .find(|w| w.id == id)
                                    .map(|w| w.color)
                            });
                            (text, color, false)
                        }
                        PaneContent::FileDiff(d) => {
                            let name = d
                                .path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| format!("\u{21c4} {}", s))
                                .unwrap_or_else(|| format!("\u{21c4} {}", d.path.display()));
                            (name, None, false)
                        }
                        PaneContent::NoteEditor(ne) => {
                            let color = ne.workspace_id.and_then(|id| {
                                self.workspace_store
                                    .workspaces
                                    .iter()
                                    .find(|w| w.id == id)
                                    .map(|w| w.color)
                            });
                            ("Notes".to_string(), color, false)
                        }
                        PaneContent::ConflictResolver(cr) => {
                            let name = cr
                                .path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| format!("\u{26a0} {}", s))
                                .unwrap_or_else(|| "Conflicts".to_string());
                            (name, None, false)
                        }
                    };

                    if !session_filter.is_empty()
                        && matcher.fuzzy_match(&label, session_filter).is_none()
                    {
                        continue;
                    }

                    let pane_ws =
                        Self::pane_group(&self.session_state.sessions, &self.workspace_store, pane);

                    if let Some(ws_filter) = self.session_workspace_filter {
                        if pane_ws != ws_filter {
                            continue;
                        }
                    }

                    let in_other_window = pane_ws.is_some_and(|ws_id| {
                        let owned_by_extra =
                            self.extra_windows.iter().any(|ew| ew.workspace_id == ws_id);
                        if owned_by_extra {
                            // Workspace has a dedicated extra window — "other" if
                            // that window isn't the one we're currently rendering.
                            self.extra_windows.iter().any(|ew| {
                                ew.workspace_id == ws_id
                                    && self.current_window_id.as_ref() != Some(&ew.id)
                            })
                        } else {
                            // Workspace lives in the main window — "other" only
                            // when we're rendering from an extra window.
                            self.current_window_id.is_some()
                        }
                    });

                    let is_active = self.pane_state.active_pane_id == Some(pane.id);

                    let (resp, painter) = ui.allocate_painter(
                        egui::vec2(ui.available_width(), theme::SESSION_ROW_H),
                        egui::Sense::click_and_drag(),
                    );
                    let row_rect = resp.rect;
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }

                    // Paint background — animated hover transition
                    let row_hovered = resp.hovered();
                    let hover_id = egui::Id::new(("sess_row_hover", pane.id));
                    let hover_t =
                        crate::app::ui::animation::animated_hover(ui.ctx(), hover_id, row_hovered);
                    let t = theme::active();
                    let bg = if is_active {
                        t.bg_row_active
                    } else {
                        theme::lerp_color(egui::Color32::TRANSPARENT, t.bg_row_hover, hover_t)
                    };
                    painter.rect_filled(row_rect, theme::R_MD, bg);

                    // Full-height workspace color bar on the left edge
                    if let Some(c) = ws_color {
                        let bar_rect = egui::Rect::from_min_size(
                            egui::pos2(row_rect.min.x, row_rect.min.y),
                            egui::vec2(theme::WS_BORDER_W - 1.0, row_rect.height()),
                        );
                        let left_rounding = egui::Rounding {
                            nw: theme::R_MD,
                            sw: theme::R_MD,
                            ne: 0.0,
                            se: 0.0,
                        };
                        painter.rect_filled(bar_rect, left_rounding, theme::from_rgb(c));
                    }

                    // Close button — hidden when row is idle, fades in on hover/active
                    let show_close = is_active || row_hovered;
                    let close_anim_t = crate::app::ui::animation::animated_hover(
                        ui.ctx(),
                        egui::Id::new(("sess_close_anim", pane.id)),
                        show_close,
                    );
                    let quit_resp = if close_anim_t > 0.01 {
                        let quit_rect = egui::Rect::from_min_size(
                            egui::pos2(row_rect.max.x - theme::BTN_W, row_rect.min.y),
                            egui::vec2(theme::BTN_W, row_rect.height()),
                        );
                        let close_color = t.danger_fg.gamma_multiply(close_anim_t);
                        let result = ui_kit::icon_button(
                            ui,
                            egui::Id::new(("pane_quit", pane.id)),
                            quit_rect,
                            "\u{00d7}",
                            theme::FONT_TERM,
                            close_color,
                            ui_kit::IconButtonStyle::Danger,
                        );
                        Some((result, quit_rect))
                    } else {
                        None
                    };
                    let (quit_rect_opt, quit_clicked) = match &quit_resp {
                        Some((r, rect)) => (Some(*rect), r.clicked()),
                        None => (None, false),
                    };

                    let win_icon_w: f32 = if in_other_window {
                        theme::FONT_TERM
                    } else {
                        0.0
                    };

                    if in_other_window {
                        let icon_x = row_rect.max.x
                            - quit_rect_opt.map_or(0.0, |_| theme::BTN_W)
                            - win_icon_w / 2.0
                            - 1.0;
                        painter.text(
                            egui::pos2(icon_x, row_rect.center().y),
                            egui::Align2::CENTER_CENTER,
                            "\u{2197}",
                            egui::FontId::proportional(theme::FONT_UI_SM),
                            theme::active().blue,
                        );
                    }

                    // Title text: consistent left padding regardless of workspace bar
                    let text_x = row_rect.min.x + theme::SP_4 + theme::WS_BORDER_W;
                    let effective_btn_w = if close_anim_t > 0.01 {
                        theme::BTN_W
                    } else {
                        0.0
                    };
                    let clip_max = row_rect.max.x - effective_btn_w - win_icon_w - theme::SP_1;
                    let is_being_dragged = matches!(
                        &self.drag_state.payload,
                        Some(crate::app::drag::DragPayload::Session(sid))
                            if matches!(&pane.content, PaneContent::Terminal(psid) if *psid == *sid)
                    );
                    let text_color = if dimmed {
                        theme::active().overlay0
                    } else if is_active {
                        theme::active().text
                    } else {
                        theme::active().subtext0
                    };
                    let text_color = if is_being_dragged {
                        text_color.gamma_multiply(0.4)
                    } else {
                        text_color
                    };
                    let available_w = (clip_max - text_x).max(0.0);
                    let mut job = egui::text::LayoutJob::single_section(
                        label.clone(),
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(theme::FONT_UI_MD),
                            color: text_color,
                            ..Default::default()
                        },
                    );
                    job.wrap = egui::text::TextWrapping {
                        max_width: available_w,
                        max_rows: 1,
                        break_anywhere: true,
                        overflow_character: Some('\u{2026}'),
                    };
                    let galley = ui.fonts(|f| f.layout_job(job));
                    painter.galley(
                        egui::pos2(text_x, row_rect.center().y - galley.rect.height() / 2.0),
                        galley,
                        text_color,
                    );

                    let resp = if in_other_window {
                        resp.on_hover_text(format!("{} (switch window)", label))
                    } else {
                        resp.on_hover_text(&label)
                    };

                    if quit_clicked {
                        actions.quit_pane_id = Some(pane.id);
                    } else if resp.clicked() {
                        actions.clicked_sidebar_pane_id = Some(pane.id);
                    }

                    // Drag source: start dragging this session row
                    if resp.drag_started() {
                        if let PaneContent::Terminal(sid) = &pane.content {
                            let origin = resp.interact_pointer_pos().unwrap_or_default();
                            self.drag_state.set_payload(
                                crate::app::drag::DragPayload::Session(*sid),
                                origin,
                                &label,
                            );
                        }
                    }
                }
            });
    }
}
