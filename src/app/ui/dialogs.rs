use super::super::pane::{PaneContent, PaneEntry};
use super::super::workspace_ui::PRESET_COLORS;
use super::super::App;
use crate::pane_tree::PaneNode;
use crate::theme;
use crate::workspace::Workspace;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

impl App {
    pub(in crate::app) fn render_quick_switcher(&mut self, ctx: &egui::Context) {
        // ── Quick Switcher overlay ────────────────────────────────────────────
        if self.show_quick_switcher {
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

            let dialog_w = (screen_rect.width() * 0.90).clamp(600.0, 1800.0);
            let dialog_h = (screen_rect.height() * 0.90).clamp(400.0, 1200.0);

            egui::Area::new(self.vp_id("quick_switcher_dialog"))
                .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, dialog_h / 2.0))
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    egui::Frame::window(&ctx.style())
                        .inner_margin(egui::Margin::same(theme::SP_XL))
                        .show(ui, |ui| {
                            ui.set_min_width(dialog_w);
                            ui.set_max_height(dialog_h);

                            // Header with search
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Quick Switcher")
                                        .strong()
                                        .size(theme::DIALOG_TITLE_SZ)
                                        .color(t.text),
                                );
                                ui.add_space(theme::SP_XL);
                                ui.label(
                                    egui::RichText::new("Ctrl+Shift+Space")
                                        .size(11.0)
                                        .color(t.subtext0)
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

                            // Escape must be consumed before TextEdit (which eats Escape to unfocus)
                            let esc = ctx.input_mut(|i| {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                            });
                            if esc {
                                close_switcher = true;
                            }

                            // Search input
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

                            // Hotkey hints
                            ui.horizontal(|ui| {
                                let hint = |ui: &mut egui::Ui, key: &str, desc: &str| {
                                    ui.label(
                                        egui::RichText::new(key)
                                            .size(11.0)
                                            .strong()
                                            .color(t.base)
                                            .background_color(t.overlay0),
                                    );
                                    ui.label(
                                        egui::RichText::new(desc).size(11.0).color(t.subtext0),
                                    );
                                    ui.add_space(theme::SP_LG);
                                };
                                hint(ui, " 1-9 ", "workspace");
                                hint(ui, " a-z ", "session");
                                hint(ui, " Enter ", "first match");
                                hint(ui, " Esc ", "close");
                            });

                            ui.add_space(theme::SP_SM);
                            ui.separator();
                            ui.add_space(theme::SP_MD);

                            // Build data: group panes by workspace
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
                            }

                            let mut groups: Vec<SwitcherGroup> = Vec::new();

                            // Collect workspaces
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
                                        let (label, cwd) = match &p.content {
                                            PaneContent::Terminal(sid) => {
                                                let sess_entry = self.session_state.find(*sid)?;
                                                let session = sess_entry.session.read();
                                                let t_str = session.title();
                                                let title = if t_str.is_empty() {
                                                    format!("Session {}", sid)
                                                } else {
                                                    t_str.to_string()
                                                };
                                                let cwd_str = theme::short_path(&session.cwd);
                                                (title, cwd_str)
                                            }
                                            PaneContent::DeferredTerminal { cwd, .. } => {
                                                let cwd_str = cwd
                                                    .as_ref()
                                                    .map(|c| theme::short_path(c))
                                                    .unwrap_or_default();
                                                ("(deferred)".to_string(), cwd_str)
                                            }
                                            PaneContent::FileEditor(ed) => {
                                                let name = ed
                                                    .path
                                                    .file_name()
                                                    .and_then(|n| n.to_str())
                                                    .unwrap_or("editor")
                                                    .to_string();
                                                (name, theme::short_path(&ed.path))
                                            }
                                            PaneContent::FileDiff(d) => {
                                                let name = d
                                                    .path
                                                    .file_name()
                                                    .and_then(|n| n.to_str())
                                                    .unwrap_or("diff")
                                                    .to_string();
                                                (
                                                    format!("diff: {}", name),
                                                    theme::short_path(&d.path),
                                                )
                                            }
                                            PaneContent::NoteEditor(_) => {
                                                ("Notes".to_string(), String::new())
                                            }
                                        };
                                        // Apply fuzzy filter
                                        if !query.is_empty() {
                                            let haystack = format!("{} {} {}", ws.name, label, cwd);
                                            matcher.fuzzy_match(&haystack, &query)?;
                                        }
                                        Some(SwitcherEntry {
                                            pane_id: p.id,
                                            label,
                                            cwd,
                                            is_active: self.pane_state.active_pane_id == Some(p.id),
                                        })
                                    })
                                    .collect();

                                if !panes_in_ws.is_empty() || (query.is_empty()) {
                                    groups.push(SwitcherGroup {
                                        ws_id: Some(ws.id),
                                        name: ws.name.clone(),
                                        color: ws.color,
                                        entries: panes_in_ws,
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
                                    let (label, cwd) = match &p.content {
                                        PaneContent::Terminal(sid) => {
                                            let sess_entry = self.session_state.find(*sid)?;
                                            let session = sess_entry.session.read();
                                            let t_str = session.title();
                                            let title = if t_str.is_empty() {
                                                format!("Session {}", sid)
                                            } else {
                                                t_str.to_string()
                                            };
                                            let cwd_str = theme::short_path(&session.cwd);
                                            (title, cwd_str)
                                        }
                                        PaneContent::DeferredTerminal { cwd, .. } => {
                                            let cwd_str = cwd
                                                .as_ref()
                                                .map(|c| theme::short_path(c))
                                                .unwrap_or_default();
                                            ("(deferred)".to_string(), cwd_str)
                                        }
                                        PaneContent::FileEditor(ed) => {
                                            let name = ed
                                                .path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("editor")
                                                .to_string();
                                            (name, theme::short_path(&ed.path))
                                        }
                                        PaneContent::FileDiff(d) => {
                                            let name = d
                                                .path
                                                .file_name()
                                                .and_then(|n| n.to_str())
                                                .unwrap_or("diff")
                                                .to_string();
                                            (format!("diff: {}", name), theme::short_path(&d.path))
                                        }
                                        PaneContent::NoteEditor(_) => {
                                            ("Notes".to_string(), String::new())
                                        }
                                    };
                                    if !query.is_empty() {
                                        let haystack = format!("Other {} {}", label, cwd);
                                        matcher.fuzzy_match(&haystack, &query)?;
                                    }
                                    Some(SwitcherEntry {
                                        pane_id: p.id,
                                        label,
                                        cwd,
                                        is_active: self.pane_state.active_pane_id == Some(p.id),
                                    })
                                })
                                .collect();

                            if !other_panes.is_empty()
                                || (query.is_empty()
                                    && self.pane_state.panes.iter().any(|p| {
                                        Self::pane_group(
                                            &self.session_state.sessions,
                                            &self.workspace_store,
                                            p,
                                        )
                                        .is_none()
                                    }))
                            {
                                groups.push(SwitcherGroup {
                                    ws_id: None,
                                    name: "Other".to_string(),
                                    color: [127, 127, 127],
                                    entries: other_panes,
                                });
                            }

                            // Filter out empty groups when searching
                            if !query.is_empty() {
                                groups.retain(|g| !g.entries.is_empty());
                            }

                            // Render columns: one per workspace
                            egui::ScrollArea::both()
                                .max_height(dialog_h - 120.0)
                                .show(ui, |ui| {
                                    if groups.is_empty() {
                                        ui.centered_and_justified(|ui| {
                                            ui.label(
                                                egui::RichText::new("No matching sessions")
                                                    .size(14.0)
                                                    .color(t.overlay0),
                                            );
                                        });
                                        return;
                                    }

                                    let num_cols = groups.len();
                                    let col_width =
                                        ((dialog_w - 40.0) / num_cols as f32).clamp(180.0, 400.0);

                                    ui.horizontal_top(|ui| {
                                        for (ws_number, group) in (1u8..).zip(groups.iter()) {
                                            ui.vertical(|ui| {
                                                ui.set_min_width(col_width);
                                                ui.set_max_width(col_width);

                                                // Workspace header with color
                                                let ws_color = egui::Color32::from_rgb(
                                                    group.color[0],
                                                    group.color[1],
                                                    group.color[2],
                                                );
                                                ui.horizontal(|ui| {
                                                    // Number badge for workspace
                                                    let badge_text = format!("{}", ws_number);
                                                    let badge = egui::RichText::new(&badge_text)
                                                        .size(11.0)
                                                        .strong()
                                                        .color(t.base)
                                                        .background_color(ws_color);
                                                    ui.label(badge);
                                                    ui.add_space(theme::SP_SM);

                                                    // Color dot
                                                    let (dot_rect, _) = ui.allocate_exact_size(
                                                        egui::vec2(10.0, 10.0),
                                                        egui::Sense::hover(),
                                                    );
                                                    ui.painter().circle_filled(
                                                        dot_rect.center(),
                                                        5.0,
                                                        ws_color,
                                                    );
                                                    ui.add_space(theme::SP_SM);

                                                    // Workspace name — clickable to switch
                                                    let ws_label = egui::RichText::new(&group.name)
                                                        .strong()
                                                        .size(14.0)
                                                        .color(ws_color);
                                                    let ws_resp = ui.add(
                                                        egui::Label::new(ws_label)
                                                            .sense(egui::Sense::click()),
                                                    );
                                                    if ws_resp.clicked() {
                                                        if let Some(wid) = group.ws_id {
                                                            switch_to_workspace = Some(wid);
                                                        } else {
                                                            switch_to_workspace = Some(u64::MAX);
                                                        }
                                                    }
                                                });
                                                ui.add_space(theme::BAR_PAD_X);

                                                // Session entries with letter keys
                                                let mut letter_idx = 0u8;
                                                for entry in &group.entries {
                                                    let letter = (b'a' + letter_idx) as char;
                                                    letter_idx += 1;
                                                    if letter_idx > 25 {
                                                        break;
                                                    }

                                                    let frame_fill = if entry.is_active {
                                                        t.surface1
                                                    } else {
                                                        t.surface0
                                                    };
                                                    let frame = egui::Frame::none()
                                                        .fill(frame_fill)
                                                        .inner_margin(egui::Margin::same(
                                                            theme::SP_MD,
                                                        ))
                                                        .rounding(theme::ROUNDING);

                                                    let resp = frame
                                                        .show(ui, |ui| {
                                                            ui.set_min_width(col_width - 20.0);
                                                            ui.horizontal(|ui| {
                                                                // Letter badge
                                                                let key_badge =
                                                                    egui::RichText::new(format!(
                                                                        "{}",
                                                                        letter
                                                                    ))
                                                                    .size(11.0)
                                                                    .strong()
                                                                    .color(t.base)
                                                                    .background_color(t.blue);
                                                                ui.label(key_badge);
                                                                ui.add_space(theme::BAR_PAD_X);
                                                                ui.vertical(|ui| {
                                                                    ui.label(
                                                                        egui::RichText::new(
                                                                            &entry.label,
                                                                        )
                                                                        .size(13.0)
                                                                        .color(t.text),
                                                                    );
                                                                    if !entry.cwd.is_empty() {
                                                                        ui.label(
                                                                            egui::RichText::new(
                                                                                &entry.cwd,
                                                                            )
                                                                            .size(11.0)
                                                                            .color(t.overlay0),
                                                                        );
                                                                    }
                                                                });
                                                            });
                                                        })
                                                        .response;

                                                    if resp.interact(egui::Sense::click()).clicked()
                                                    {
                                                        switch_to_pane = Some(entry.pane_id);
                                                    }
                                                    ui.add_space(theme::SP_SM);
                                                }

                                                if group.entries.is_empty() {
                                                    ui.label(
                                                        egui::RichText::new("(no sessions)")
                                                            .size(12.0)
                                                            .italics()
                                                            .color(t.overlay0),
                                                    );
                                                }
                                            });
                                            ui.add_space(theme::SP_LG);
                                        }
                                    });
                                });

                            // Direct-jump shortcuts only when search is empty
                            if query.is_empty() {
                                // Number keys 1-9 to jump to workspace
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
                                    let pressed = ctx.input_mut(|inp| {
                                        inp.consume_key(egui::Modifiers::NONE, *nk)
                                    });
                                    if pressed && i < groups.len() {
                                        if let Some(wid) = groups[i].ws_id {
                                            switch_to_workspace = Some(wid);
                                        } else {
                                            switch_to_workspace = Some(u64::MAX);
                                        }
                                    }
                                }
                            }

                            // Enter key: select first visible entry
                            let enter = ctx.input_mut(|i| {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                            });
                            if enter {
                                if let Some(first_entry) =
                                    groups.iter().flat_map(|g| g.entries.iter()).next()
                                {
                                    switch_to_pane = Some(first_entry.pane_id);
                                }
                            }
                        });
                });

            // Process actions
            if let Some(ws_id) = switch_to_workspace {
                let (cols, rows) = self
                    .pane_state
                    .panes
                    .first()
                    .map(|p| p.last_size)
                    .unwrap_or((80, 24));
                let group = if ws_id == u64::MAX { None } else { Some(ws_id) };
                self.switch_group(group, cols, rows);
                close_switcher = true;
            }
            if let Some(pane_id) = switch_to_pane {
                // Find which workspace this pane belongs to and switch there
                if let Some(pane) = self.pane_state.panes.iter().find(|p| p.id == pane_id) {
                    let group =
                        Self::pane_group(&self.session_state.sessions, &self.workspace_store, pane);
                    if group != self.active_group {
                        let (cols, rows) = self
                            .pane_state
                            .panes
                            .first()
                            .map(|p| p.last_size)
                            .unwrap_or((80, 24));
                        self.switch_group(group, cols, rows);
                    }
                }
                self.activate_pane(pane_id);
                close_switcher = true;
            }
            if close_switcher {
                self.show_quick_switcher = false;
                self.quick_switcher_query.clear();
            }
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
                    });
                    self.workspace_store.save();
                    let (cols, rows) = self
                        .pane_state
                        .panes
                        .first()
                        .map(|p| p.last_size)
                        .unwrap_or((80, 24));
                    self.switch_group(Some(id), cols, rows);
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
                    if let Some(ws) = self
                        .workspace_store
                        .workspaces
                        .iter_mut()
                        .find(|w| w.id == dlg.workspace_id)
                    {
                        ws.name = dlg.name.trim().to_string();
                        ws.color = dlg.selected_color;
                    }
                    self.workspace_store.save();
                }
            } else if delete_it {
                if let Some(dlg) = self.workspace_edit_dialog.take() {
                    self.workspace_store
                        .workspaces
                        .retain(|w| w.id != dlg.workspace_id);
                    self.workspace_store.save();
                    if self.active_group == Some(dlg.workspace_id) {
                        self.active_group = None;
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
