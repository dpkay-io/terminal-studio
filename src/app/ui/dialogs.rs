use super::super::pane::{PaneContent, PaneEntry};
use super::super::workspace_ui::PRESET_COLORS;
use super::super::App;
use crate::pane_tree::PaneNode;
use crate::theme;
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
                    egui::Color32::from_black_alpha(theme::OVERLAY_DIM),
                );
                if resp.clicked() {
                    close_switcher = true;
                }
            });

        let dialog_w = (screen_rect.width() * 0.95).max(600.0);
        let dialog_h = (screen_rect.height() * 0.95).max(400.0);

        egui::Area::new(self.vp_id("quick_switcher_dialog"))
            .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, dialog_h / 2.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::window(&ctx.style())
                    .inner_margin(egui::Margin::same(theme::SP_XL))
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
                            ui.label(
                                egui::RichText::new("Quick Switcher")
                                    .strong()
                                    .size(theme::DIALOG_TITLE_SZ)
                                    .color(t.text),
                            );
                            ui.add_space(theme::SP_XL);
                            let shortcut_fg =
                                theme::ensure_readable(t.subtext0_rgb, t.surface1_rgb);
                            ui.label(
                                egui::RichText::new("Ctrl+Shift+Space")
                                    .size(11.0)
                                    .color(shortcut_fg)
                                    .background_color(t.surface1),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("×")
                                                    .size(theme::DIALOG_CLOSE_SZ),
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
                        ui.add_space(theme::SP_MD);

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
                            ui.add_space(theme::SP_SM);
                        }

                        // ── Hotkey hints ─────────────────────────────────
                        ui.horizontal(|ui| {
                            let hint = |ui: &mut egui::Ui, key: &str, desc: &str| {
                                let hint_fg = theme::ensure_readable(t.base_rgb, t.overlay0_rgb);
                                ui.label(
                                    egui::RichText::new(key)
                                        .size(11.0)
                                        .strong()
                                        .color(hint_fg)
                                        .background_color(t.overlay0),
                                );
                                ui.label(egui::RichText::new(desc).size(11.0).color(t.subtext0));
                                ui.add_space(theme::SP_LG);
                            };
                            if let Some(ws_idx) = self.quick_switcher_selected_ws {
                                hint(ui, " a-z ", "select session");
                                let badge = format!(" {} ", ws_idx + 1);
                                let ws_badge_fg = theme::ensure_readable(t.base_rgb, t.green_rgb);
                                ui.label(
                                    egui::RichText::new(&badge)
                                        .size(11.0)
                                        .strong()
                                        .color(ws_badge_fg)
                                        .background_color(t.green),
                                );
                                ui.label(
                                    egui::RichText::new("selected").size(11.0).color(t.subtext0),
                                );
                                ui.add_space(theme::SP_LG);
                            } else {
                                hint(ui, " 1-9 ", "select workspace");
                            }
                            hint(ui, " / ", "search");
                            hint(ui, " Enter ", "first match");
                            hint(ui, " Esc ", "close");
                        });

                        ui.add_space(theme::SP_SM);
                        ui.separator();
                        ui.add_space(theme::SP_MD);

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
                        let scroll_h = dialog_h - 120.0;
                        egui::ScrollArea::horizontal()
                            .min_scrolled_height(scroll_h)
                            .max_height(scroll_h)
                            .show(ui, |ui| {
                                ui.set_min_height(scroll_h);
                                if groups.is_empty() {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(
                                            egui::RichText::new("No active sessions")
                                                .size(14.0)
                                                .color(t.overlay0),
                                        );
                                    });
                                    return;
                                }

                                let num_cols = groups.len();
                                let available_w = dialog_w
                                    - 40.0
                                    - (num_cols.saturating_sub(1) as f32 * theme::SP_SM);
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
                                                .rounding(theme::ROUNDING)
                                                .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                                                .show(ui, |ui| {
                                                    ui.horizontal(|ui| {
                                                        ui.label(
                                                            egui::RichText::new(format!(
                                                                " {} ",
                                                                ws_number
                                                            ))
                                                            .size(11.0)
                                                            .strong()
                                                            .color(header_fg)
                                                            .background_color(
                                                                egui::Color32::from_black_alpha(40),
                                                            ),
                                                        );
                                                        ui.label(
                                                            egui::RichText::new(&group.name)
                                                                .strong()
                                                                .size(13.0)
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

                                            ui.add_space(4.0);

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
                                                    .rounding(theme::ROUNDING)
                                                    .inner_margin(egui::Margin::symmetric(6.0, 3.0))
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
                                                                .size(11.0)
                                                                .strong()
                                                                .color(session_badge_fg)
                                                                .background_color(badge_bg),
                                                            );
                                                            ui.vertical(|ui| {
                                                                ui.label(
                                                                    egui::RichText::new(
                                                                        &entry.label,
                                                                    )
                                                                    .size(12.0)
                                                                    .color(t.text),
                                                                );
                                                                if !entry.cwd.is_empty() {
                                                                    ui.label(
                                                                        egui::RichText::new(
                                                                            &entry.cwd,
                                                                        )
                                                                        .size(10.0)
                                                                        .color(t.overlay0),
                                                                    );
                                                                }
                                                            });
                                                        });
                                                    })
                                                    .response;

                                                if resp.interact(egui::Sense::click()).clicked() {
                                                    switch_to_pane = Some(entry.pane_id);
                                                }
                                                ui.add_space(2.0);
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
        // ── Workspace save dialog (modal overlay) ──────────────────────────
        if self.workspace_dialog.is_some() {
            let mut save_it = false;
            let mut cancel = false;
            let screen_rect = ctx.screen_rect();
            let dialog_w = (screen_rect.width() * 0.4).clamp(300.0, 480.0);

            egui::Area::new(self.vp_id("ws_dialog_dim"))
                .fixed_pos(screen_rect.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.painter().rect_filled(
                        screen_rect,
                        0.0,
                        egui::Color32::from_black_alpha(theme::OVERLAY_DIM),
                    );
                });

            egui::Area::new(self.vp_id("ws_dialog"))
                .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, 140.0))
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    egui::Frame::window(&ctx.style()).show(ui, |ui| {
                        ui.set_min_width(dialog_w);

                        ui.label(
                            egui::RichText::new("Save Workspace")
                                .strong()
                                .size(theme::DIALOG_TITLE_SZ),
                        );
                        ui.add_space(theme::SP_MD);

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
                            ui.add_space(theme::SP_MD);

                            ui.label(
                                egui::RichText::new(theme::short_path(&dlg.path))
                                    .monospace()
                                    .size(11.0)
                                    .color(theme::active().fg_path),
                            )
                            .on_hover_text(dlg.path.display().to_string());
                            ui.add_space(theme::SP_MD);

                            ui.label("Color");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing =
                                    egui::vec2(theme::BAR_PAD_X, theme::BAR_PAD_X);
                                for &preset in PRESET_COLORS {
                                    let selected =
                                        dlg.selected_color == preset && !dlg.show_custom_picker;
                                    let swatch = egui::Button::new("")
                                        .fill(theme::from_rgb(preset))
                                        .stroke(if selected {
                                            egui::Stroke::new(
                                                theme::STROKE_BOLD,
                                                egui::Color32::WHITE,
                                            )
                                        } else {
                                            egui::Stroke::new(
                                                theme::STROKE_THIN,
                                                egui::Color32::from_gray(60),
                                            )
                                        })
                                        .min_size(egui::vec2(24.0, 24.0))
                                        .rounding(theme::ROUNDING);
                                    if ui.add(swatch).clicked() {
                                        dlg.selected_color = preset;
                                        dlg.show_custom_picker = false;
                                        dlg.custom_color = [
                                            preset[0] as f32 / 255.0,
                                            preset[1] as f32 / 255.0,
                                            preset[2] as f32 / 255.0,
                                        ];
                                    }
                                }

                                // Custom — inline picker, one click opens directly
                                let picker_resp = egui::color_picker::color_edit_button_rgb(
                                    ui,
                                    &mut dlg.custom_color,
                                );
                                if picker_resp.changed() {
                                    dlg.show_custom_picker = true;
                                    dlg.selected_color = [
                                        (dlg.custom_color[0] * 255.0) as u8,
                                        (dlg.custom_color[1] * 255.0) as u8,
                                        (dlg.custom_color[2] * 255.0) as u8,
                                    ];
                                }
                            });

                            ui.add_space(theme::SP_LG);
                            ui.horizontal(|ui| {
                                let can_save = !dlg.name.trim().is_empty();
                                if ui
                                    .add_enabled(can_save, egui::Button::new("Save"))
                                    .clicked()
                                {
                                    save_it = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    cancel = true;
                                }
                            });
                        }
                    });
                });

            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                cancel = true;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                && self
                    .workspace_dialog
                    .as_ref()
                    .is_some_and(|d| !d.name.trim().is_empty())
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
            } else if cancel {
                self.workspace_dialog = None;
            }
        }
    }

    pub(in crate::app) fn render_workspace_edit_dialog(&mut self, ctx: &egui::Context) {
        // ── Workspace edit dialog (modal overlay) ──────────────────────────
        if self.workspace_edit_dialog.is_some() {
            let mut save_it = false;
            let mut delete_it = false;
            let mut cancel = false;
            let screen_rect = ctx.screen_rect();
            let dialog_w = (screen_rect.width() * 0.4).clamp(300.0, 480.0);

            egui::Area::new(self.vp_id("ws_edit_dim"))
                .fixed_pos(screen_rect.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.painter().rect_filled(
                        screen_rect,
                        0.0,
                        egui::Color32::from_black_alpha(theme::OVERLAY_DIM),
                    );
                });

            egui::Area::new(self.vp_id("ws_edit_dialog"))
                .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, 140.0))
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    egui::Frame::window(&ctx.style()).show(ui, |ui| {
                        ui.set_min_width(dialog_w);

                        ui.label(
                            egui::RichText::new("Workspace Settings")
                                .strong()
                                .size(theme::DIALOG_TITLE_SZ),
                        );
                        ui.add_space(theme::SP_MD);

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
                            ui.add_space(theme::SP_MD);

                            ui.label("Color");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing =
                                    egui::vec2(theme::BAR_PAD_X, theme::BAR_PAD_X);
                                for &preset in PRESET_COLORS {
                                    let selected =
                                        dlg.selected_color == preset && !dlg.show_custom_picker;
                                    let swatch = egui::Button::new("")
                                        .fill(theme::from_rgb(preset))
                                        .stroke(if selected {
                                            egui::Stroke::new(
                                                theme::STROKE_BOLD,
                                                egui::Color32::WHITE,
                                            )
                                        } else {
                                            egui::Stroke::new(
                                                theme::STROKE_THIN,
                                                egui::Color32::from_gray(60),
                                            )
                                        })
                                        .min_size(egui::vec2(24.0, 24.0))
                                        .rounding(theme::ROUNDING);
                                    if ui.add(swatch).clicked() {
                                        dlg.selected_color = preset;
                                        dlg.show_custom_picker = false;
                                        dlg.custom_color = [
                                            preset[0] as f32 / 255.0,
                                            preset[1] as f32 / 255.0,
                                            preset[2] as f32 / 255.0,
                                        ];
                                    }
                                }

                                // Custom — inline picker, one click opens directly
                                let picker_resp = egui::color_picker::color_edit_button_rgb(
                                    ui,
                                    &mut dlg.custom_color,
                                );
                                if picker_resp.changed() {
                                    dlg.show_custom_picker = true;
                                    dlg.selected_color = [
                                        (dlg.custom_color[0] * 255.0) as u8,
                                        (dlg.custom_color[1] * 255.0) as u8,
                                        (dlg.custom_color[2] * 255.0) as u8,
                                    ];
                                }
                            });

                            ui.add_space(theme::SP_LG);
                            ui.separator();
                            ui.add_space(theme::SP_MD);

                            if dlg.confirm_delete {
                                ui.colored_label(
                                    egui::Color32::from_rgb(220, 70, 70),
                                    "Are you sure? This cannot be undone.",
                                );
                                ui.add_space(theme::SP_MD);
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                egui::RichText::new("Delete Workspace")
                                                    .color(egui::Color32::WHITE),
                                            )
                                            .fill(egui::Color32::from_rgb(180, 40, 40)),
                                        )
                                        .clicked()
                                    {
                                        delete_it = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        dlg.confirm_delete = false;
                                    }
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    let can_save = !dlg.name.trim().is_empty();
                                    if ui
                                        .add_enabled(can_save, egui::Button::new("Save"))
                                        .clicked()
                                    {
                                        save_it = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        cancel = true;
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        egui::RichText::new("Delete").color(
                                                            egui::Color32::from_rgb(220, 80, 80),
                                                        ),
                                                    )
                                                    .stroke(egui::Stroke::new(
                                                        1.0,
                                                        egui::Color32::from_rgb(220, 80, 80),
                                                    )),
                                                )
                                                .clicked()
                                            {
                                                dlg.confirm_delete = true;
                                            }
                                        },
                                    );
                                });
                            }
                        }
                    });
                });

            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                cancel = true;
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                let in_confirm = self
                    .workspace_edit_dialog
                    .as_ref()
                    .is_some_and(|d| d.confirm_delete);
                if !in_confirm
                    && self
                        .workspace_edit_dialog
                        .as_ref()
                        .is_some_and(|d| !d.name.trim().is_empty())
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
            } else if cancel {
                self.workspace_edit_dialog = None;
            }
        }
    }

    pub(in crate::app) fn render_close_all_confirm(&mut self, ctx: &egui::Context) {
        if !self.show_close_all_confirm {
            return;
        }

        let mut do_close = false;
        let mut cancel = false;
        let screen_rect = ctx.screen_rect();
        let dialog_w = 340.0_f32;

        egui::Area::new(self.vp_id("close_all_dim"))
            .fixed_pos(screen_rect.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let resp = ui.interact(
                    screen_rect,
                    self.vp_id("close_all_dim_click"),
                    egui::Sense::click(),
                );
                ui.painter().rect_filled(
                    screen_rect,
                    0.0,
                    egui::Color32::from_black_alpha(theme::OVERLAY_DIM),
                );
                if resp.clicked() {
                    cancel = true;
                }
            });

        egui::Area::new(self.vp_id("close_all_dialog"))
            .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, 60.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::window(&ctx.style())
                    .inner_margin(egui::Margin::same(theme::SP_XL))
                    .show(ui, |ui| {
                        ui.set_min_width(dialog_w);

                        let (title, count) = if let Some(ws_filter) = self.session_workspace_filter
                        {
                            let filter_name = match ws_filter {
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
                                    Self::pane_group(
                                        &self.session_state.sessions,
                                        &self.workspace_store,
                                        p,
                                    ) == ws_filter
                                })
                                .count();
                            (format!("Close \"{}\" Sessions", filter_name), cnt)
                        } else {
                            (
                                "Close All Sessions".to_string(),
                                self.pane_state.panes.len(),
                            )
                        };
                        ui.label(
                            egui::RichText::new(&title)
                                .strong()
                                .size(theme::DIALOG_TITLE_SZ),
                        );
                        ui.add_space(theme::SP_MD);

                        ui.label(format!(
                            "This will close {} session{}. Are you sure?",
                            count,
                            if count == 1 { "" } else { "s" }
                        ));
                        ui.add_space(theme::SP_LG);

                        if ctx
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
                        {
                            cancel = true;
                        }
                        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                        {
                            do_close = true;
                        }

                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        egui::RichText::new("Close All")
                                            .color(egui::Color32::WHITE),
                                    )
                                    .fill(egui::Color32::from_rgb(180, 40, 40)),
                                )
                                .clicked()
                            {
                                do_close = true;
                            }
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                        });
                    });
            });

        if do_close {
            self.show_close_all_confirm = false;

            let pane_ids_to_close: Vec<u32> = if let Some(ws_filter) = self.session_workspace_filter
            {
                self.pane_state
                    .panes
                    .iter()
                    .filter(|p| {
                        Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
                            == ws_filter
                    })
                    .map(|p| p.id)
                    .collect()
            } else {
                self.pane_state.panes.iter().map(|p| p.id).collect()
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

            self.session_workspace_filter = None;
            self.save_session();
        } else if cancel {
            self.show_close_all_confirm = false;
        }
    }
}
