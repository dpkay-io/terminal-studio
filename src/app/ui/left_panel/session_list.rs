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
                "\u{1f50e}",
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
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    actions.open_folder_path = Some(path);
                                }
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
        egui::ComboBox::from_id_source(self.vp_id("ws_session_filter"))
            .width(ui.available_width() - 12.0)
            .selected_text(egui::RichText::new(&selected_label).size(theme::FONT_UI_MD))
            .show_ui(ui, |ui| {
                for (val, name) in &ws_names {
                    let is_selected = self.session_workspace_filter == *val;
                    if ui.selectable_label(is_selected, name).clicked() {
                        self.session_workspace_filter = *val;
                    }
                }
            });
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
        egui::ScrollArea::vertical()
            .id_source(self.vp_id("sessions_scroll"))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width() - theme::SCROLLBAR_PAD);
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
                        egui::Sense::click(),
                    );
                    let row_rect = resp.rect;

                    // Paint background first so subsequent elements are visible
                    let row_hovered = resp.hovered();
                    let bg = if is_active {
                        theme::active().bg_row_active
                    } else if row_hovered {
                        theme::active().bg_row_hover
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    painter.rect_filled(row_rect, 0.0, bg);

                    if let Some(c) = ws_color {
                        let border = egui::Rect::from_min_size(
                            row_rect.min,
                            egui::vec2(theme::WS_BORDER_W, row_rect.height()),
                        );
                        painter.rect_filled(border, 0.0, theme::from_rgb(c));
                    }

                    // Quit button — inset from right edge to avoid scrollbar overlap
                    let sb_pad = 14.0_f32;
                    let quit_rect = egui::Rect::from_min_size(
                        egui::pos2(row_rect.max.x - theme::BTN_W - sb_pad, row_rect.min.y),
                        egui::vec2(theme::BTN_W, row_rect.height()),
                    );
                    let quit_resp = ui_kit::icon_button(
                        ui,
                        egui::Id::new(("pane_quit", pane.id)),
                        quit_rect,
                        "\u{00d7}",
                        theme::FONT_TERM,
                        theme::active().danger_fg,
                        ui_kit::IconButtonStyle::Danger,
                    );

                    let win_icon_w: f32 = if in_other_window {
                        theme::FONT_TERM
                    } else {
                        0.0
                    };

                    if in_other_window {
                        painter.text(
                            egui::pos2(
                                quit_rect.min.x - win_icon_w / 2.0 - 1.0,
                                row_rect.center().y,
                            ),
                            egui::Align2::CENTER_CENTER,
                            "\u{2197}",
                            egui::FontId::proportional(theme::FONT_UI_SM),
                            theme::active().blue,
                        );
                    }

                    // Title text clipped to leave room for quit button + window icon
                    let text_x = row_rect.min.x
                        + if ws_color.is_some() {
                            theme::WS_BORDER_W + theme::SP_3
                        } else {
                            theme::SP_3
                        };
                    let clip_max = quit_rect.min.x - win_icon_w - 3.0;
                    let text_color = if dimmed {
                        theme::active().overlay0
                    } else if is_active {
                        theme::active().text
                    } else {
                        theme::active().subtext0
                    };
                    painter
                        .with_clip_rect(egui::Rect::from_min_max(
                            egui::pos2(text_x, row_rect.min.y),
                            egui::pos2(clip_max, row_rect.max.y),
                        ))
                        .text(
                            egui::pos2(text_x, row_rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            &label,
                            egui::FontId::proportional(theme::FONT_UI_MD),
                            text_color,
                        );

                    let resp = if in_other_window {
                        resp.on_hover_text(format!("{} (switch window)", label))
                    } else {
                        resp.on_hover_text(&label)
                    };

                    if quit_resp.clicked() {
                        actions.quit_pane_id = Some(pane.id);
                    } else if resp.clicked() {
                        actions.clicked_sidebar_pane_id = Some(pane.id);
                    }
                }
            });
    }
}
