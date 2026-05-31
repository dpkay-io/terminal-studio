use super::super::pane::{PaneContent, PaneEntry};
use super::super::workspace_ui::PRESET_COLORS;
use super::super::{App, CloseAllTarget};
use crate::pane_tree::PaneNode;
use crate::theme;
use crate::ui_kit;
use crate::workspace::Workspace;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

impl App {
    fn switcher_collect_entries(&self, pane: &PaneEntry) -> Option<(String, String)> {
        match &pane.content {
            PaneContent::Terminal(sid) => {
                let sess_entry = self.session_state.find(*sid)?;
                let session = sess_entry.session.read();
                let t_str = session.title();
                let title = if t_str.is_empty() {
                    format!("Session {}", sid)
                } else {
                    t_str.to_string()
                };
                Some((title, theme::short_path(&session.cwd)))
            }
            PaneContent::DeferredTerminal { cwd, .. } => {
                let cwd_str = cwd
                    .as_ref()
                    .map(|c| theme::short_path(c))
                    .unwrap_or_default();
                Some(("Terminal".to_string(), cwd_str))
            }
            PaneContent::FileEditor(ed) => {
                let name = ed
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("editor")
                    .to_string();
                Some((name, theme::short_path(&ed.path)))
            }
            PaneContent::FileDiff(d) => {
                let name = d
                    .path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("diff")
                    .to_string();
                Some((format!("diff: {}", name), theme::short_path(&d.path)))
            }
            PaneContent::NoteEditor(_) => Some(("Notes".to_string(), String::new())),
        }
    }

    pub(in crate::app) fn render_quick_switcher(&mut self, ctx: &egui::Context) {
        if !self.show_quick_switcher {
            return;
        }

        let mut close_switcher = false;
        let mut switch_to_workspace: Option<u64> = None;
        let mut switch_to_pane: Option<u32> = None;
        let screen_rect = ctx.screen_rect();
        let t = theme::active();

        // Dim background
        egui::Area::new(self.vp_id("quick_switcher_dim"))
            .fixed_pos(screen_rect.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let resp = ui.interact(
                    screen_rect,
                    self.vp_id("quick_switcher_dim_click"),
                    egui::Sense::click(),
                );
                ui.painter().rect_filled(
                    screen_rect,
                    0.0,
                    egui::Color32::from_black_alpha(theme::ALPHA_OVERLAY_DIM),
                );
                if resp.clicked() {
                    close_switcher = true;
                }
            });

        let dialog_w = (screen_rect.width() * 0.95)
            .min(screen_rect.width() - 20.0)
            .max(400.0);
        let dialog_h = (screen_rect.height() * 0.95)
            .min(screen_rect.height() - 20.0)
            .max(300.0);

        egui::Area::new(self.vp_id("quick_switcher_dialog"))
            .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, dialog_h / 2.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::window(&ctx.style())
                    .inner_margin(egui::Margin::same(theme::SP_6))
                    .show(ui, |ui| {
                        ui.set_min_width(dialog_w);
                        ui.set_min_height(dialog_h);
                        ui.set_max_height(dialog_h);

                        // ── Consume keys BEFORE TextEdit ─────────────────
                        let esc = ctx
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                        if esc {
                            if self.quick_switcher_search_active
                                && !self.quick_switcher_query.is_empty()
                            {
                                self.quick_switcher_query.clear();
                            } else {
                                close_switcher = true;
                            }
                        }

                        // Consume number keys 1-9 when NOT in search mode
                        let mut number_pressed: Option<usize> = None;
                        if !self.quick_switcher_search_active {
                            let num_keys = [
                                egui::Key::Num1,
                                egui::Key::Num2,
                                egui::Key::Num3,
                                egui::Key::Num4,
                                egui::Key::Num5,
                                egui::Key::Num6,
                                egui::Key::Num7,
                                egui::Key::Num8,
                                egui::Key::Num9,
                            ];
                            for (i, nk) in num_keys.iter().enumerate() {
                                if ctx.input_mut(|inp| inp.consume_key(egui::Modifiers::NONE, *nk))
                                {
                                    number_pressed = Some(i);
                                    break;
                                }
                            }
                            // Also consume the text event so it doesn't go to TextEdit
                            if number_pressed.is_some() {
                                ctx.input_mut(|inp| {
                                    inp.events.retain(|e| !matches!(e, egui::Event::Text(_)));
                                });
                            }
                        }

                        // Consume letter keys a-z when a workspace is selected
                        let mut letter_pressed: Option<u8> = None;
                        if !self.quick_switcher_search_active
                            && self.quick_switcher_selected_ws.is_some()
                        {
                            let letter_keys = [
                                egui::Key::A,
                                egui::Key::B,
                                egui::Key::C,
                                egui::Key::D,
                                egui::Key::E,
                                egui::Key::F,
                                egui::Key::G,
                                egui::Key::H,
                                egui::Key::I,
                                egui::Key::J,
                                egui::Key::K,
                                egui::Key::L,
                                egui::Key::M,
                                egui::Key::N,
                                egui::Key::O,
                                egui::Key::P,
                                egui::Key::Q,
                                egui::Key::R,
                                egui::Key::S,
                                egui::Key::T,
                                egui::Key::U,
                                egui::Key::V,
                                egui::Key::W,
                                egui::Key::X,
                                egui::Key::Y,
                                egui::Key::Z,
                            ];
                            for (i, lk) in letter_keys.iter().enumerate() {
                                if ctx.input_mut(|inp| inp.consume_key(egui::Modifiers::NONE, *lk))
                                {
                                    letter_pressed = Some(i as u8);
                                    break;
                                }
                            }
                            if letter_pressed.is_some() {
                                ctx.input_mut(|inp| {
                                    inp.events.retain(|e| !matches!(e, egui::Event::Text(_)));
                                });
                            }
                        }

                        // '/' activates search mode
                        let slash_pressed = if !self.quick_switcher_search_active {
                            let pressed = ctx.input_mut(|inp| {
                                inp.consume_key(egui::Modifiers::NONE, egui::Key::Slash)
                            });
                            if pressed {
                                ctx.input_mut(|inp| {
                                    inp.events.retain(|e| !matches!(e, egui::Event::Text(_)));
                                });
                            }
                            pressed
                        } else {
                            false
                        };

                        if slash_pressed {
                            self.quick_switcher_search_active = true;
                            self.quick_switcher_selected_ws = None;
                        }

                        // Enter key
                        let enter = ctx
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));

                        // ── Header ───────────────────────────────────────
                        ui.horizontal(|ui| {
                            ui.label(ui_kit::heading("Quick Switcher").color(t.text));
                            ui.add_space(theme::SP_6);
                            let shortcut_fg =
                                theme::ensure_readable(t.subtext0_rgb, t.surface1_rgb);
                            ui.label(
                                egui::RichText::new("Ctrl+Shift+Space")
                                    .size(theme::FONT_UI_SM)
                                    .color(shortcut_fg)
                                    .background_color(t.surface1),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("×").size(theme::FONT_UI_LG),
                                            )
                                            .min_size(egui::vec2(theme::BTN_W, theme::BTN_W)),
                                        )
                                        .clicked()
                                    {
                                        close_switcher = true;
                                    }
                                },
                            );
                        });
                        ui.add_space(theme::SP_4);

                        // ── Search input (only visible/focused in search mode) ───
                        if self.quick_switcher_search_active {
                            let search_id = self.vp_id("quick_switcher_search");
                            let search_resp = ui.add(
                                egui::TextEdit::singleline(&mut self.quick_switcher_query)
                                    .id(search_id)
                                    .desired_width(dialog_w - 40.0)
                                    .hint_text("Type to search sessions...")
                                    .font(egui::TextStyle::Body),
                            );
                            if !search_resp.has_focus() {
                                search_resp.request_focus();
                            }
                            ui.add_space(theme::SP_2);
                        }

                        // ── Hotkey hints ─────────────────────────────────
                        ui.horizontal(|ui| {
                            let hint = |ui: &mut egui::Ui, key: &str, desc: &str| {
                                let hint_fg = theme::ensure_readable(t.base_rgb, t.overlay0_rgb);
                                ui.label(
                                    egui::RichText::new(key)
                                        .size(theme::FONT_UI_SM)
                                        .strong()
                                        .color(hint_fg)
                                        .background_color(t.overlay0),
                                );
                                ui.label(
                                    egui::RichText::new(desc)
                                        .size(theme::FONT_UI_SM)
                                        .color(t.subtext0),
                                );
                                ui.add_space(theme::SP_5);
                            };
                            if let Some(ws_idx) = self.quick_switcher_selected_ws {
                                hint(ui, " a-z ", "select session");
                                let badge = format!(" {} ", ws_idx + 1);
                                let ws_badge_fg = theme::ensure_readable(t.base_rgb, t.green_rgb);
                                ui.label(
                                    egui::RichText::new(&badge)
                                        .size(theme::FONT_UI_SM)
                                        .strong()
                                        .color(ws_badge_fg)
                                        .background_color(t.green),
                                );
                                ui.label(
                                    egui::RichText::new("selected")
                                        .size(theme::FONT_UI_SM)
                                        .color(t.subtext0),
                                );
                                ui.add_space(theme::SP_5);
                            } else {
                                hint(ui, " 1-9 ", "select workspace");
                            }
                            hint(ui, " / ", "search");
                            hint(ui, " Enter ", "first match");
                            hint(ui, " Esc ", "close");
                        });

                        ui.add_space(theme::SP_2);
                        ui.separator();
                        ui.add_space(theme::SP_4);

                        // ── Build data: groups with sessions only ────────
                        let matcher = SkimMatcherV2::default();
                        let query = self.quick_switcher_query.trim().to_lowercase();

                        struct SwitcherEntry {
                            pane_id: u32,
                            label: String,
                            cwd: String,
                            is_active: bool,
                        }
                        struct SwitcherGroup {
                            ws_id: Option<u64>,
                            name: String,
                            color: [u8; 3],
                            entries: Vec<SwitcherEntry>,
                            last_activated: u64,
                        }

                        let mut groups: Vec<SwitcherGroup> = Vec::new();

                        // Collect workspaces — only those with sessions
                        for ws in &self.workspace_store.workspaces {
                            let panes_in_ws: Vec<SwitcherEntry> = self
                                .pane_state
                                .panes
                                .iter()
                                .filter(|p| {
                                    Self::pane_group(
                                        &self.session_state.sessions,
                                        &self.workspace_store,
                                        p,
                                    ) == Some(ws.id)
                                })
                                .filter_map(|p| {
                                    let (label, cwd) = self.switcher_collect_entries(p)?;
                                    if !query.is_empty() {
                                        let haystack = format!("{} {} {}", ws.name, label, cwd);
                                        matcher.fuzzy_match(&haystack, &query)?;
                                    }
                                    Some(SwitcherEntry {
                                        pane_id: p.id,
                                        label,
                                        cwd,
                                        is_active: self.is_pane_active_in_any_window(p.id),
                                    })
                                })
                                .collect();

                            if !panes_in_ws.is_empty() {
                                groups.push(SwitcherGroup {
                                    ws_id: Some(ws.id),
                                    name: ws.name.clone(),
                                    color: ws.color,
                                    entries: panes_in_ws,
                                    last_activated: ws.last_activated,
                                });
                            }
                        }

                        // "Other" group (unaffiliated panes)
                        let other_panes: Vec<SwitcherEntry> = self
                            .pane_state
                            .panes
                            .iter()
                            .filter(|p| {
                                Self::pane_group(
                                    &self.session_state.sessions,
                                    &self.workspace_store,
                                    p,
                                )
                                .is_none()
                            })
                            .filter_map(|p| {
                                let (label, cwd) = self.switcher_collect_entries(p)?;
                                if !query.is_empty() {
                                    let haystack = format!("Other {} {}", label, cwd);
                                    matcher.fuzzy_match(&haystack, &query)?;
                                }
                                Some(SwitcherEntry {
                                    pane_id: p.id,
                                    label,
                                    cwd,
                                    is_active: self.is_pane_active_in_any_window(p.id),
                                })
                            })
                            .collect();

                        if !other_panes.is_empty() {
                            groups.push(SwitcherGroup {
                                ws_id: None,
                                name: "Other".to_string(),
                                color: [127, 127, 127],
                                entries: other_panes,
                                last_activated: 0,
                            });
                        }

                        // Filter out empty groups when searching
                        if !query.is_empty() {
                            groups.retain(|g| !g.entries.is_empty());
                        }

                        // Sort by last_activated descending (most recent first)
                        groups.sort_by_key(|b| std::cmp::Reverse(b.last_activated));

                        // ── Process number key → select workspace column ─
                        if let Some(idx) = number_pressed {
                            if idx < groups.len() {
                                self.quick_switcher_selected_ws = Some(idx);
                            }
                        }

                        // ── Process letter key → select session in selected ws ─
                        if let Some(letter_idx) = letter_pressed {
                            if let Some(ws_idx) = self.quick_switcher_selected_ws {
                                if let Some(group) = groups.get(ws_idx) {
                                    if let Some(entry) = group.entries.get(letter_idx as usize) {
                                        switch_to_pane = Some(entry.pane_id);
                                    }
                                }
                            }
                        }

                        // Enter: select first visible entry
                        if enter {
                            if let Some(first_entry) =
                                groups.iter().flat_map(|g| g.entries.iter()).next()
                            {
                                switch_to_pane = Some(first_entry.pane_id);
                            }
                        }

                        // ── Render columns ───────────────────────────────
                        let scroll_h = (dialog_h - 120.0).max(60.0);
                        egui::ScrollArea::horizontal()
                            .min_scrolled_height(scroll_h)
                            .max_height(scroll_h)
                            .show(ui, |ui| {
                                ui.set_min_height(scroll_h);
                                if groups.is_empty() {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(
                                            egui::RichText::new("No active sessions")
                                                .size(theme::FONT_TERM)
                                                .color(t.overlay0),
                                        );
                                    });
                                    return;
                                }

                                let num_cols = groups.len();
                                let available_w = dialog_w
                                    - 40.0
                                    - (num_cols.saturating_sub(1) as f32 * theme::SP_2);
                                let col_width = (available_w / num_cols as f32).clamp(160.0, 240.0);

                                ui.horizontal_top(|ui| {
                                    for (ws_number, group) in (1u8..).zip(groups.iter()) {
                                        let col_idx = (ws_number - 1) as usize;
                                        let is_selected =
                                            self.quick_switcher_selected_ws == Some(col_idx);

                                        ui.vertical(|ui| {
                                            ui.set_min_width(col_width);
                                            ui.set_max_width(col_width);

                                            // ── Workspace header ─────────────
                                            let header_bg_rgb = theme::tinted(
                                                group.color,
                                                if is_selected { 0.75 } else { 0.55 },
                                            );
                                            let header_fg = theme::text_on(header_bg_rgb);

                                            let hdr_resp = egui::Frame::none()
                                                .fill(theme::from_rgb(header_bg_rgb))
                                                .rounding(theme::R_MD)
                                                .inner_margin(egui::Margin::symmetric(
                                                    theme::SP_3,
                                                    theme::SP_2,
                                                ))
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            egui::RichText::new(format!(
                                                                " {} ",
                                                                ws_number
                                                            ))
                                                            .size(theme::FONT_UI_SM)
                                                            .strong()
                                                            .color(header_fg)
                                                            .background_color(
                                                                egui::Color32::from_black_alpha(40),
                                                            ),
                                                        );
                                                        ui.label(
                                                            ui_kit::heading(&group.name)
                                                                .color(header_fg),
                                                        );
                                                    });
                                                })
                                                .response;

                                            if hdr_resp.interact(egui::Sense::click()).clicked() {
                                                if let Some(wid) = group.ws_id {
                                                    switch_to_workspace = Some(wid);
                                                } else {
                                                    switch_to_workspace = Some(u64::MAX);
                                                }
                                            }

                                            ui.add_space(theme::SP_2);

                                            // ── Session entries ──────────────
                                            for (letter_idx, entry) in
                                                group.entries.iter().enumerate()
                                            {
                                                if letter_idx >= 26 {
                                                    break;
                                                }
                                                let letter = (b'a' + letter_idx as u8) as char;
                                                let fill = if entry.is_active {
                                                    t.surface1
                                                } else {
                                                    t.surface0
                                                };

                                                let resp = egui::Frame::none()
                                                    .fill(fill)
                                                    .rounding(theme::R_MD)
                                                    .inner_margin(egui::Margin::symmetric(
                                                        theme::SP_3,
                                                        theme::SP_2,
                                                    ))
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            let badge_bg = if is_selected {
                                                                t.blue
                                                            } else {
                                                                t.overlay0
                                                            };
                                                            let badge_bg_rgb = [
                                                                badge_bg.r(),
                                                                badge_bg.g(),
                                                                badge_bg.b(),
                                                            ];
                                                            let session_badge_fg =
                                                                theme::ensure_readable(
                                                                    t.base_rgb,
                                                                    badge_bg_rgb,
                                                                );
                                                            ui.label(
                                                                egui::RichText::new(format!(
                                                                    " {} ",
                                                                    letter
                                                                ))
                                                                .size(theme::FONT_UI_SM)
                                                                .strong()
                                                                .color(session_badge_fg)
                                                                .background_color(badge_bg),
                                                            );
                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    ui_kit::label(&entry.label)
                                                                        .color(t.text),
                                                                );
                                                                if !entry.cwd.is_empty() {
                                                                    ui.label(ui_kit::hint(
                                                                        &entry.cwd,
                                                                    ));
                                                                }
                                                            });
                                                        });
                                                    })
                                                    .response;

                                                if resp.interact(egui::Sense::click()).clicked() {
                                                    switch_to_pane = Some(entry.pane_id);
                                                }
                                                ui.add_space(theme::SP_1);
                                            }
                                        });
                                    }
                                });
                            });
                    });
            });

        // Process actions
        if let Some(ws_id) = switch_to_workspace {
            self.navigate_to_workspace(ws_id);
            close_switcher = true;
        }
        if let Some(pane_id) = switch_to_pane {
            self.navigate_to_pane(pane_id);
            close_switcher = true;
        }
        if close_switcher {
            self.show_quick_switcher = false;
            self.quick_switcher_query.clear();
            self.quick_switcher_selected_ws = None;
            self.quick_switcher_search_active = false;
        }
    }

    pub(in crate::app) fn render_workspace_save_dialog(&mut self, ctx: &egui::Context) {
        if self.workspace_dialog.is_none() {
            return;
        }

        let mut save_it = false;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Responsive {
                pct: 0.4,
                min: 300.0,
                max: 480.0,
            },
            max_height: 280.0,
            ..Default::default()
        };

        let resp = ui_kit::dialog(ctx, self.vp_id("ws_dialog"), config, |ui| {
            ui_kit::dialog_header(ui, "Save Workspace");

            if let Some(dlg) = &mut self.workspace_dialog {
                ui.label("Name");
                let name_resp = ui.add(
                    egui::TextEdit::singleline(&mut dlg.name)
                        .hint_text("e.g. My Project")
                        .desired_width(f32::INFINITY),
                );
                if !dlg.focus_requested {
                    name_resp.request_focus();
                    dlg.focus_requested = true;
                }
                let ws_name_conflict = {
                    let trimmed = dlg.name.trim();
                    !trimmed.is_empty() && self.workspace_store.is_name_taken(trimmed, None)
                };
                if ws_name_conflict {
                    ui.label(
                        egui::RichText::new("A workspace with this name already exists")
                            .size(theme::FONT_UI_XS)
                            .color(theme::active().error),
                    );
                }
                ui.add_space(theme::SP_4);

                ui.label(
                    egui::RichText::new(theme::short_path(&dlg.path))
                        .monospace()
                        .size(theme::FONT_UI_SM)
                        .color(theme::active().fg_path),
                )
                .on_hover_text(dlg.path.display().to_string());
                ui.add_space(theme::SP_4);

                ui.label("Color");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(theme::SP_4, theme::SP_4);
                    for &preset in PRESET_COLORS {
                        let selected = dlg.selected_color == preset && !dlg.show_custom_picker;
                        if ui_kit::color_swatch(ui, preset, selected).clicked() {
                            dlg.selected_color = preset;
                            dlg.show_custom_picker = false;
                            dlg.custom_color = [
                                preset[0] as f32 / 255.0,
                                preset[1] as f32 / 255.0,
                                preset[2] as f32 / 255.0,
                            ];
                        }
                    }
                    let picker_resp =
                        egui::color_picker::color_edit_button_rgb(ui, &mut dlg.custom_color);
                    if picker_resp.changed() {
                        dlg.show_custom_picker = true;
                        dlg.selected_color = [
                            (dlg.custom_color[0] * 255.0) as u8,
                            (dlg.custom_color[1] * 255.0) as u8,
                            (dlg.custom_color[2] * 255.0) as u8,
                        ];
                    }
                });

                let can_save = {
                    let trimmed = dlg.name.trim();
                    !trimmed.is_empty() && !self.workspace_store.is_name_taken(trimmed, None)
                };
                ui_kit::dialog_footer(ui, |ui| {
                    if ui_kit::action_button(
                        ui,
                        "Save",
                        can_save,
                        ui_kit::ActionButtonStyle::Primary,
                    )
                    .clicked()
                    {
                        save_it = true;
                    }
                    if ui_kit::action_button(ui, "Cancel", true, ui_kit::ActionButtonStyle::Cancel)
                        .clicked()
                    {
                        save_it = false;
                    }
                });
            }
        });

        // Enter shortcut (outside dialog closure)
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            && self.workspace_dialog.as_ref().is_some_and(|d| {
                let trimmed = d.name.trim();
                !trimmed.is_empty() && !self.workspace_store.is_name_taken(trimmed, None)
            })
        {
            save_it = true;
        }

        if save_it {
            if let Some(dlg) = self.workspace_dialog.take() {
                let id = self.workspace_store.next_id();
                self.workspace_store.workspaces.push(Workspace {
                    id,
                    name: dlg.name.trim().to_string(),
                    path: dlg.path,
                    color: dlg.selected_color,
                    host_window_id: None,
                    last_activated: 0,
                });
                self.workspace_store.save();
                self.navigate_to_workspace(id);
            }
        } else if resp.dismissed {
            self.workspace_dialog = None;
        }
    }

    pub(in crate::app) fn render_workspace_edit_dialog(&mut self, ctx: &egui::Context) {
        if self.workspace_edit_dialog.is_none() {
            return;
        }

        let mut save_it = false;
        let mut delete_it = false;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Responsive {
                pct: 0.4,
                min: 300.0,
                max: 480.0,
            },
            max_height: 280.0,
            ..Default::default()
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("ws_edit"), config, |ui| {
            ui_kit::dialog_header(ui, "Workspace Settings");

            if let Some(dlg) = &mut self.workspace_edit_dialog {
                ui.label("Name");
                let name_resp = ui.add(
                    egui::TextEdit::singleline(&mut dlg.name)
                        .hint_text("Workspace name")
                        .desired_width(f32::INFINITY),
                );
                if !dlg.focus_requested {
                    name_resp.request_focus();
                    dlg.focus_requested = true;
                }
                let edit_name_conflict = {
                    let trimmed = dlg.name.trim();
                    !trimmed.is_empty()
                        && self
                            .workspace_store
                            .is_name_taken(trimmed, Some(dlg.workspace_id))
                };
                if edit_name_conflict {
                    ui.label(
                        egui::RichText::new("A workspace with this name already exists")
                            .size(theme::FONT_UI_XS)
                            .color(theme::active().error),
                    );
                }
                ui.add_space(theme::SP_4);

                ui.label("Color");
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(theme::SP_4, theme::SP_4);
                    for &preset in PRESET_COLORS {
                        let selected = dlg.selected_color == preset && !dlg.show_custom_picker;
                        if ui_kit::color_swatch(ui, preset, selected).clicked() {
                            dlg.selected_color = preset;
                            dlg.show_custom_picker = false;
                            dlg.custom_color = [
                                preset[0] as f32 / 255.0,
                                preset[1] as f32 / 255.0,
                                preset[2] as f32 / 255.0,
                            ];
                        }
                    }
                    let picker_resp =
                        egui::color_picker::color_edit_button_rgb(ui, &mut dlg.custom_color);
                    if picker_resp.changed() {
                        dlg.show_custom_picker = true;
                        dlg.selected_color = [
                            (dlg.custom_color[0] * 255.0) as u8,
                            (dlg.custom_color[1] * 255.0) as u8,
                            (dlg.custom_color[2] * 255.0) as u8,
                        ];
                    }
                });

                ui.add_space(theme::SP_5);
                ui.separator();
                ui.add_space(theme::SP_4);

                if dlg.confirm_delete {
                    ui.colored_label(
                        theme::active().error,
                        "Are you sure? This cannot be undone.",
                    );
                    ui.add_space(theme::SP_4);
                    ui.horizontal(|ui| {
                        if ui_kit::action_button(
                            ui,
                            "Delete Workspace",
                            true,
                            ui_kit::ActionButtonStyle::Danger,
                        )
                        .clicked()
                        {
                            delete_it = true;
                        }
                        if ui_kit::action_button(
                            ui,
                            "Cancel",
                            true,
                            ui_kit::ActionButtonStyle::Cancel,
                        )
                        .clicked()
                        {
                            dlg.confirm_delete = false;
                        }
                    });
                } else {
                    let can_save = {
                        let trimmed = dlg.name.trim();
                        !trimmed.is_empty()
                            && !self
                                .workspace_store
                                .is_name_taken(trimmed, Some(dlg.workspace_id))
                    };
                    ui_kit::dialog_footer(ui, |ui| {
                        if ui_kit::action_button(
                            ui,
                            "Save",
                            can_save,
                            ui_kit::ActionButtonStyle::Primary,
                        )
                        .clicked()
                        {
                            save_it = true;
                        }
                        if ui_kit::action_button(
                            ui,
                            "Cancel",
                            true,
                            ui_kit::ActionButtonStyle::Cancel,
                        )
                        .clicked()
                        {
                            cancel_clicked = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let t = theme::active();
                            if ui
                                .add(
                                    egui::Button::new(egui::RichText::new("Delete").color(t.error))
                                        .stroke(egui::Stroke::new(1.0, t.error)),
                                )
                                .clicked()
                            {
                                dlg.confirm_delete = true;
                            }
                        });
                    });
                }
            }
        });

        // Enter shortcut (outside dialog closure)
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            let in_confirm = self
                .workspace_edit_dialog
                .as_ref()
                .is_some_and(|d| d.confirm_delete);
            if !in_confirm
                && self.workspace_edit_dialog.as_ref().is_some_and(|d| {
                    let trimmed = d.name.trim();
                    !trimmed.is_empty()
                        && !self
                            .workspace_store
                            .is_name_taken(trimmed, Some(d.workspace_id))
                })
            {
                save_it = true;
            }
        }

        if save_it {
            if let Some(dlg) = self.workspace_edit_dialog.take() {
                let new_name = dlg.name.trim().to_string();
                if let Some(ws) = self
                    .workspace_store
                    .workspaces
                    .iter_mut()
                    .find(|w| w.id == dlg.workspace_id)
                {
                    ws.name = new_name.clone();
                    ws.color = dlg.selected_color;
                }
                self.workspace_store.save();
                if let Some(ew) = self
                    .extra_windows
                    .iter_mut()
                    .find(|ew| ew.workspace_id == dlg.workspace_id)
                {
                    ew.title = format!("{} \u{2014} Terminal Studio", new_name);
                }
            }
        } else if delete_it {
            if let Some(dlg) = self.workspace_edit_dialog.take() {
                self.close_extra_window_for_workspace(dlg.workspace_id);
                self.workspace_store
                    .workspaces
                    .retain(|w| w.id != dlg.workspace_id);
                self.workspace_store.save();
                if self.active_group == Some(dlg.workspace_id) {
                    self.active_group = None;
                }
                for ew in &mut self.extra_windows {
                    if ew.view.active_group == Some(dlg.workspace_id) {
                        ew.view.active_group = None;
                    }
                }
            }
        } else if resp.dismissed || cancel_clicked {
            self.workspace_edit_dialog = None;
        }
    }

    pub(in crate::app) fn render_close_all_confirm(&mut self, ctx: &egui::Context) {
        if !self.show_close_all_confirm {
            return;
        }

        let mut do_close = false;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Fixed(340.0),
            max_height: 120.0,
            ..Default::default()
        };

        let (title, count) = match self.close_all_target {
            CloseAllTarget::All => (
                "Close All Sessions".to_string(),
                self.pane_state.panes.len(),
            ),
            _ => {
                let group = match self.close_all_target {
                    CloseAllTarget::Group(g) => g,
                    _ => self.active_group,
                };
                let group_name = match group {
                    None => "Other".to_string(),
                    Some(id) => self
                        .workspace_store
                        .workspaces
                        .iter()
                        .find(|w| w.id == id)
                        .map(|w| w.name.clone())
                        .unwrap_or_else(|| "Unknown".to_string()),
                };
                let cnt = self
                    .pane_state
                    .panes
                    .iter()
                    .filter(|p| {
                        Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
                            == group
                    })
                    .count();
                (format!("Close \"{}\" Sessions", group_name), cnt)
            }
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("close_all"), config, |ui| {
            ui_kit::dialog_header(ui, &title);

            ui.label(format!(
                "This will close {} session{}. Are you sure?",
                count,
                if count == 1 { "" } else { "s" }
            ));

            ui_kit::dialog_footer(ui, |ui| {
                if ui_kit::action_button(ui, "Close All", true, ui_kit::ActionButtonStyle::Danger)
                    .clicked()
                {
                    do_close = true;
                }
                if ui_kit::action_button(ui, "Cancel", true, ui_kit::ActionButtonStyle::Cancel)
                    .clicked()
                {
                    cancel_clicked = true;
                }
            });
        });

        // Enter shortcut
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            do_close = true;
        }

        if do_close {
            self.show_close_all_confirm = false;

            let target = std::mem::take(&mut self.close_all_target);

            let pane_ids_to_close: Vec<u32> = match target {
                CloseAllTarget::All => self.pane_state.panes.iter().map(|p| p.id).collect(),
                _ => {
                    let group = match target {
                        CloseAllTarget::Group(g) => g,
                        _ => self.active_group,
                    };
                    self.pane_state
                        .panes
                        .iter()
                        .filter(|p| {
                            Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
                                == group
                        })
                        .map(|p| p.id)
                        .collect()
                }
            };

            let session_ids: Vec<u32> = self
                .pane_state
                .panes
                .iter()
                .filter(|p| pane_ids_to_close.contains(&p.id))
                .filter_map(|p| match &p.content {
                    PaneContent::Terminal(sid) => Some(*sid),
                    _ => None,
                })
                .collect();

            self.pane_state
                .panes
                .retain(|p| !pane_ids_to_close.contains(&p.id));
            for pid in &pane_ids_to_close {
                self.pane_state.pane_trees.remove(pid);
            }
            if self
                .pane_state
                .active_pane_id
                .is_some_and(|id| pane_ids_to_close.contains(&id))
            {
                self.pane_state.active_pane_id = self.pane_state.panes.last().map(|p| p.id);
            }

            for sid in &session_ids {
                self.session_state.remove(*sid);
            }

            self.session_state.active_id = if let Some(apid) = self.pane_state.active_pane_id {
                self.pane_state
                    .panes
                    .iter()
                    .find(|p| p.id == apid)
                    .and_then(|p| match &p.content {
                        PaneContent::Terminal(sid) => Some(*sid),
                        _ => None,
                    })
                    .or_else(|| self.session_state.sessions.first().map(|e| e.id))
            } else {
                self.session_state.sessions.first().map(|e| e.id)
            };
            self.update_is_active_flags();

            if self.pane_state.panes.is_empty() {
                if let Some(sid) = self.session_state.active_id {
                    let pane_id = self.pane_state.next_pane_id;
                    self.pane_state.next_pane_id += 1;
                    self.pane_state.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::Terminal(sid),
                        manual_width: None,
                        last_size: (0, 0),
                    });
                    self.pane_state.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (0, 0),
                        },
                    );
                    self.pane_state.active_pane_id = Some(pane_id);
                }
            }

            self.save_session();
        } else if resp.dismissed || cancel_clicked {
            self.show_close_all_confirm = false;
            self.close_all_target = CloseAllTarget::default();
        }
    }

    pub(in crate::app) fn render_quit_confirm(&mut self, ctx: &egui::Context) {
        if !self.show_quit_confirm {
            return;
        }

        let mut do_quit = false;
        let session_count = self.session_state.sessions.len();
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Fixed(340.0),
            max_height: 120.0,
            ..Default::default()
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("quit"), config, |ui| {
            ui_kit::dialog_header(ui, "Quit Terminal Studio?");

            ui.label(format!(
                "You have {} active session{}. Are you sure you want to quit?",
                session_count,
                if session_count == 1 { "" } else { "s" }
            ));

            ui_kit::dialog_footer(ui, |ui| {
                if ui_kit::action_button(ui, "Quit", true, ui_kit::ActionButtonStyle::Danger)
                    .clicked()
                {
                    do_quit = true;
                }
                if ui_kit::action_button(ui, "Cancel", true, ui_kit::ActionButtonStyle::Cancel)
                    .clicked()
                {
                    cancel_clicked = true;
                }
            });
        });

        // Enter shortcut
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            do_quit = true;
        }

        if do_quit {
            self.show_quit_confirm = false;
            self.quit_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if resp.dismissed || cancel_clicked {
            self.show_quit_confirm = false;
        }
    }

    pub(in crate::app) fn render_commit_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_commit_dialog {
            return;
        }

        let mut do_commit = false;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Responsive {
                pct: 0.4,
                min: 340.0,
                max: 480.0,
            },
            max_height: 200.0,
            ..Default::default()
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("commit"), config, |ui| {
            ui_kit::dialog_header(ui, "Commit");

            ui.label(ui_kit::label_secondary("Message"));
            let msg_id = self.vp_id("commit_msg_input");
            let msg_resp = ui.add(
                egui::TextEdit::multiline(&mut self.commit_message)
                    .id(msg_id)
                    .desired_width(f32::INFINITY)
                    .desired_rows(2)
                    .hint_text("Commit message...")
                    .font(egui::TextStyle::Monospace),
            );
            if !self.commit_dialog_focus_requested {
                msg_resp.request_focus();
                self.commit_dialog_focus_requested = true;
            }
            ui.add_space(theme::SP_3);

            let prev_amend = self.commit_amend;
            ui.checkbox(&mut self.commit_amend, "Amend last commit");
            if self.commit_amend && !prev_amend {
                if let Some(cwd) = self.active_cwd() {
                    self.workers.git_worker.enqueue_last_commit_msg(&cwd);
                }
            }

            let can_commit = !self.commit_message.trim().is_empty();
            ui_kit::dialog_footer(ui, |ui| {
                if ui_kit::action_button(
                    ui,
                    "Commit",
                    can_commit,
                    ui_kit::ActionButtonStyle::Primary,
                )
                .clicked()
                {
                    do_commit = true;
                }
                if ui_kit::action_button(ui, "Cancel", true, ui_kit::ActionButtonStyle::Cancel)
                    .clicked()
                {
                    cancel_clicked = true;
                }
                ui.label(ui_kit::hint("Ctrl+Enter to commit"));
            });
        });

        // Ctrl+Enter shortcut (outside dialog closure)
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Enter))
            && !self.commit_message.trim().is_empty()
        {
            do_commit = true;
        }

        if do_commit {
            let message = self.commit_message.trim().to_string();
            let amend = self.commit_amend;
            if let Some(cwd) = self.active_cwd() {
                self.workers.git_worker.enqueue_commit(&cwd, message, amend);
            }
            self.show_commit_dialog = false;
            self.commit_message.clear();
            self.commit_amend = false;
            self.commit_dialog_focus_requested = false;
        } else if resp.dismissed || cancel_clicked {
            self.show_commit_dialog = false;
            self.commit_message.clear();
            self.commit_amend = false;
            self.commit_dialog_focus_requested = false;
        }
    }

    pub(in crate::app) fn render_push_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_push_dialog {
            return;
        }

        let mut do_push = false;
        let push_force = self.push_force;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Fixed(340.0),
            max_height: 140.0,
            ..Default::default()
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("push"), config, |ui| {
            ui_kit::dialog_header(ui, "Push to Remote");

            ui.label("Push commits to the remote branch?");
            ui.add_space(theme::SP_3);

            ui.checkbox(&mut self.push_force, "Force push");

            ui_kit::dialog_footer(ui, |ui| {
                if push_force {
                    if ui_kit::action_button(
                        ui,
                        "Force Push",
                        true,
                        ui_kit::ActionButtonStyle::Danger,
                    )
                    .clicked()
                    {
                        do_push = true;
                    }
                } else if ui_kit::action_button(
                    ui,
                    "Push",
                    true,
                    ui_kit::ActionButtonStyle::Primary,
                )
                .clicked()
                {
                    do_push = true;
                }
                if ui_kit::action_button(ui, "Cancel", true, ui_kit::ActionButtonStyle::Cancel)
                    .clicked()
                {
                    cancel_clicked = true;
                }
            });
        });

        // Enter shortcut
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            do_push = true;
        }

        if do_push {
            let force = self.push_force;
            if let Some(cwd) = self.active_cwd() {
                self.push_in_progress = true;
                self.push_error = None;
                self.workers.git_worker.enqueue_push(&cwd, force);
            }
            self.show_push_dialog = false;
            self.push_force = false;
        } else if resp.dismissed || cancel_clicked {
            self.show_push_dialog = false;
            self.push_force = false;
        }
    }

    pub(in crate::app) fn render_stage_all_confirm(&mut self, ctx: &egui::Context) {
        if !self.show_stage_all_confirm {
            return;
        }

        let mut do_stage = false;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Fixed(340.0),
            max_height: 120.0,
            ..Default::default()
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("stage_all"), config, |ui| {
            ui_kit::dialog_header(ui, "Stage All Changes");

            ui.label("Stage all modified and untracked files?");

            ui_kit::dialog_footer(ui, |ui| {
                if ui_kit::action_button(ui, "Yes", true, ui_kit::ActionButtonStyle::Primary)
                    .clicked()
                {
                    do_stage = true;
                }
                if ui_kit::action_button(ui, "No", true, ui_kit::ActionButtonStyle::Cancel)
                    .clicked()
                {
                    cancel_clicked = true;
                }
            });
        });

        // Enter shortcut
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            do_stage = true;
        }

        if do_stage {
            if let Some(cwd) = self.active_cwd() {
                self.workers.git_worker.enqueue_stage_all(&cwd);
            }
            self.show_stage_all_confirm = false;
        } else if resp.dismissed || cancel_clicked {
            self.show_stage_all_confirm = false;
        }
    }

    pub(in crate::app) fn render_open_folder_dialog(&mut self, ctx: &egui::Context) {
        if self.open_folder_dialog.is_none() {
            return;
        }

        let mut open_it = false;
        let mut navigate_parent: Option<u64> = None;
        let t = theme::active();

        let title_text = if self
            .open_folder_dialog
            .as_ref()
            .and_then(|d| d.existing_workspace_id)
            .is_some()
        {
            "Open Workspace Folder"
        } else {
            "Open Folder"
        };

        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Responsive {
                pct: 0.45,
                min: 360.0,
                max: 520.0,
            },
            max_height: 400.0,
            ..Default::default()
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("open_folder"), config, |ui| {
            ui_kit::dialog_header(ui, title_text);

            if let Some(dlg) = &mut self.open_folder_dialog {
                // Path display
                ui.label(
                    egui::RichText::new(theme::short_path(&dlg.path))
                        .monospace()
                        .size(theme::FONT_UI_SM)
                        .color(t.fg_path),
                )
                .on_hover_text(dlg.path.display().to_string());
                ui.add_space(theme::SP_4);

                // Subdirectory warning
                if let Some((parent_id, ref parent_name)) = dlg.parent_workspace {
                    let warn_text = format!("This folder is inside workspace \"{}\"", parent_name);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(warn_text)
                                .size(theme::FONT_UI_SM)
                                .color(t.warning),
                        );
                    });
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!("Open \"{}\" instead", parent_name))
                                    .size(theme::FONT_UI_SM),
                            )
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        navigate_parent = Some(parent_id);
                    }
                    ui.add_space(theme::SP_4);
                }

                // Terminal selection
                ui.label(ui_kit::label_secondary("Terminal"));
                ui.add_space(theme::SP_1);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(theme::SP_3, theme::SP_3);
                    for shell in &dlg.available_shells.clone() {
                        let selected = dlg.selected_shell == *shell;
                        if ui_kit::toggle_chip(ui, shell.display_name(), selected).clicked() {
                            dlg.selected_shell = shell.clone();
                        }
                    }
                });
                ui.add_space(theme::SP_5);

                // Save as Workspace toggle
                ui.checkbox(
                    &mut dlg.save_as_workspace,
                    ui_kit::label("Save as Workspace"),
                );
                ui.add_space(theme::SP_3);

                // Workspace name + color (shown only when save is checked)
                if dlg.save_as_workspace {
                    ui.label(ui_kit::label_secondary("Name"));
                    let name_resp = ui.add(
                        egui::TextEdit::singleline(&mut dlg.workspace_name)
                            .hint_text("e.g. My Project")
                            .desired_width(f32::INFINITY),
                    );
                    if !dlg.focus_requested {
                        name_resp.request_focus();
                        dlg.focus_requested = true;
                    }

                    let name_trimmed = dlg.workspace_name.trim();
                    let name_conflict = if !name_trimmed.is_empty() {
                        self.workspace_store.workspaces.iter().any(|w| {
                            w.name.eq_ignore_ascii_case(name_trimmed)
                                && Some(w.id) != dlg.existing_workspace_id
                        })
                    } else {
                        false
                    };
                    if name_conflict {
                        ui.label(
                            egui::RichText::new("A workspace with this name already exists")
                                .size(theme::FONT_UI_XS)
                                .color(t.error),
                        );
                    }
                    ui.add_space(theme::SP_3);

                    ui.label(ui_kit::label_secondary("Color"));
                    let color_conflict = self.workspace_store.workspaces.iter().any(|w| {
                        w.color == dlg.workspace_color && Some(w.id) != dlg.existing_workspace_id
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(theme::SP_4, theme::SP_4);
                        for &preset in PRESET_COLORS {
                            let selected = dlg.workspace_color == preset && !dlg.show_custom_picker;
                            if ui_kit::color_swatch(ui, preset, selected).clicked() {
                                dlg.workspace_color = preset;
                                dlg.show_custom_picker = false;
                                dlg.custom_color = [
                                    preset[0] as f32 / 255.0,
                                    preset[1] as f32 / 255.0,
                                    preset[2] as f32 / 255.0,
                                ];
                            }
                        }
                        let picker_resp =
                            egui::color_picker::color_edit_button_rgb(ui, &mut dlg.custom_color);
                        if picker_resp.changed() {
                            dlg.show_custom_picker = true;
                            dlg.workspace_color = [
                                (dlg.custom_color[0] * 255.0) as u8,
                                (dlg.custom_color[1] * 255.0) as u8,
                                (dlg.custom_color[2] * 255.0) as u8,
                            ];
                        }
                    });
                    if color_conflict {
                        ui.label(
                            egui::RichText::new("This color is already used by another workspace")
                                .size(theme::FONT_UI_XS)
                                .color(t.warning),
                        );
                    }
                    ui.add_space(theme::SP_3);
                }

                // Open / Cancel buttons
                let can_open = if dlg.save_as_workspace {
                    let name_trimmed = dlg.workspace_name.trim();
                    let name_ok = !name_trimmed.is_empty()
                        && !self.workspace_store.workspaces.iter().any(|w| {
                            w.name.eq_ignore_ascii_case(name_trimmed)
                                && Some(w.id) != dlg.existing_workspace_id
                        });
                    let color_ok = !self.workspace_store.workspaces.iter().any(|w| {
                        w.color == dlg.workspace_color && Some(w.id) != dlg.existing_workspace_id
                    });
                    name_ok && color_ok
                } else {
                    true
                };

                ui_kit::dialog_footer(ui, |ui| {
                    if ui_kit::action_button(
                        ui,
                        "Open",
                        can_open,
                        ui_kit::ActionButtonStyle::Primary,
                    )
                    .clicked()
                    {
                        open_it = true;
                    }
                    if ui_kit::action_button(ui, "Cancel", true, ui_kit::ActionButtonStyle::Cancel)
                        .clicked()
                    {
                        cancel_clicked = true;
                    }
                });
            }
        });

        // Enter shortcut (outside dialog closure)
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            let can_open = self.open_folder_dialog.as_ref().is_some_and(|dlg| {
                if dlg.save_as_workspace {
                    let name_trimmed = dlg.workspace_name.trim();
                    let name_ok = !name_trimmed.is_empty()
                        && !self.workspace_store.workspaces.iter().any(|w| {
                            w.name.eq_ignore_ascii_case(name_trimmed)
                                && Some(w.id) != dlg.existing_workspace_id
                        });
                    let color_ok = !self.workspace_store.workspaces.iter().any(|w| {
                        w.color == dlg.workspace_color && Some(w.id) != dlg.existing_workspace_id
                    });
                    name_ok && color_ok
                } else {
                    true
                }
            });
            if can_open {
                open_it = true;
            }
        }

        if let Some(parent_id) = navigate_parent {
            self.open_folder_dialog = None;
            self.navigate_to_workspace(parent_id);
        } else if open_it {
            if let Some(dlg) = self.open_folder_dialog.take() {
                // Save preferred shell
                self.settings.default_shell = Some(dlg.selected_shell.display_name().to_string());
                self.settings.save();

                let ws_id = if dlg.save_as_workspace {
                    if let Some(existing_id) = dlg.existing_workspace_id {
                        // Update existing workspace
                        if let Some(ws) = self
                            .workspace_store
                            .workspaces
                            .iter_mut()
                            .find(|w| w.id == existing_id)
                        {
                            ws.name = dlg.workspace_name.trim().to_string();
                            ws.color = dlg.workspace_color;
                        }
                        self.workspace_store.save();
                        Some(existing_id)
                    } else {
                        // Create new workspace
                        let id = self.workspace_store.next_id();
                        self.workspace_store.workspaces.push(Workspace {
                            id,
                            name: dlg.workspace_name.trim().to_string(),
                            path: dlg.path.clone(),
                            color: dlg.workspace_color,
                            host_window_id: None,
                            last_activated: 0,
                        });
                        self.workspace_store.save();
                        Some(id)
                    }
                } else {
                    None
                };

                // Spawn terminal session
                let (cols, rows) = self
                    .pane_state
                    .panes
                    .iter()
                    .find(|p| Some(p.id) == self.pane_state.active_pane_id)
                    .map(|p| p.last_size)
                    .unwrap_or_else(|| {
                        self.pane_state
                            .panes
                            .first()
                            .map(|p| p.last_size)
                            .unwrap_or((80, 24))
                    });
                if let Some(new_id) =
                    self.spawn_session(&dlg.selected_shell, cols, rows, Some(dlg.path))
                {
                    self.session_state.active_id = Some(new_id);
                    if !self
                        .pane_state
                        .panes
                        .iter()
                        .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == new_id))
                    {
                        let pane_id = self.pane_state.next_pane_id;
                        self.pane_state.next_pane_id += 1;
                        self.pane_state.panes.push(PaneEntry {
                            id: pane_id,
                            content: PaneContent::Terminal(new_id),
                            manual_width: None,
                            last_size: (cols, rows),
                        });
                        self.pane_state.pane_trees.insert(
                            pane_id,
                            PaneNode::Leaf {
                                pane_id,
                                last_size: (cols, rows),
                            },
                        );
                        self.activate_pane(pane_id);
                        self.flash.trigger(
                            crate::app::feedback::FlashTarget::Tab(pane_id),
                            crate::app::feedback::FlashKind::Success,
                        );
                    }
                }

                // Navigate to workspace if created/updated
                if let Some(ws_id) = ws_id {
                    self.navigate_to_workspace(ws_id);
                }
            }
        } else if resp.dismissed || cancel_clicked {
            self.open_folder_dialog = None;
        }
    }
}
