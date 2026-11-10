use crate::app::pane::{PaneContent, PaneEntry};
use crate::app::workspace_ui::PRESET_COLORS;
use crate::app::App;
use crate::pane_tree::PaneNode;
use crate::workspace::Workspace;
use crate::{theme, ui_kit};

impl App {
    pub(in crate::app) fn render_workspace_save_dialog(&mut self, ctx: &egui::Context) {
        if self.workspace_dialog.is_none() {
            return;
        }

        let mut save_it = false;
        let mut cancel_clicked = false;
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
                            .size(theme::FONT_XS)
                            .color(theme::active().error),
                    );
                }
                ui.add_space(theme::SP_4);

                ui.label(
                    egui::RichText::new(theme::short_path(&dlg.path))
                        .size(theme::FONT_XS)
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
                let save_button = if can_save {
                    ui_kit::DialogButton::Primary("Save")
                } else {
                    ui_kit::DialogButton::DisabledPrimary("Save")
                };
                if let Some(index) = ui_kit::dialog_footer(
                    ui,
                    &[ui_kit::DialogButton::Secondary("Cancel"), save_button],
                ) {
                    match index {
                        0 => cancel_clicked = true,
                        1 if can_save => save_it = true,
                        _ => {}
                    }
                }
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
        } else if resp.dismissed || cancel_clicked {
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
                            .size(theme::FONT_XS)
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
                    if let Some(index) = ui_kit::dialog_footer(
                        ui,
                        &[
                            ui_kit::DialogButton::Secondary("Cancel"),
                            ui_kit::DialogButton::Destructive("Delete Workspace"),
                        ],
                    ) {
                        match index {
                            0 => dlg.confirm_delete = false,
                            1 => delete_it = true,
                            _ => {}
                        }
                    }
                } else {
                    let can_save = {
                        let trimmed = dlg.name.trim();
                        !trimmed.is_empty()
                            && !self
                                .workspace_store
                                .is_name_taken(trimmed, Some(dlg.workspace_id))
                    };
                    let save_button = if can_save {
                        ui_kit::DialogButton::Primary("Save")
                    } else {
                        ui_kit::DialogButton::DisabledPrimary("Save")
                    };
                    if let Some(index) = ui_kit::dialog_footer(
                        ui,
                        &[
                            ui_kit::DialogButton::Secondary("Cancel"),
                            save_button,
                            ui_kit::DialogButton::Destructive("Delete"),
                        ],
                    ) {
                        match index {
                            0 => cancel_clicked = true,
                            1 if can_save => save_it = true,
                            2 => dlg.confirm_delete = true,
                            _ => {}
                        }
                    }
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
                let old_name = self
                    .workspace_store
                    .workspaces
                    .iter()
                    .find(|w| w.id == dlg.workspace_id)
                    .map(|w| w.name.clone());
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
                if let Some(old) = old_name {
                    if old != new_name {
                        self.note_store
                            .rename_file(dlg.workspace_id, &old, &new_name);
                    }
                }
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
                        .size(theme::FONT_XS)
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
                                .size(theme::FONT_XS)
                                .color(t.warning),
                        );
                    });
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(format!("Open \"{}\" instead", parent_name))
                                    .size(theme::FONT_XS),
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
                                .size(theme::FONT_XS)
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
                                .size(theme::FONT_XS)
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

                let open_button = if can_open {
                    ui_kit::DialogButton::Primary("Open")
                } else {
                    ui_kit::DialogButton::DisabledPrimary("Open")
                };
                if let Some(index) = ui_kit::dialog_footer(
                    ui,
                    &[ui_kit::DialogButton::Secondary("Cancel"), open_button],
                ) {
                    match index {
                        0 => cancel_clicked = true,
                        1 if can_open => open_it = true,
                        _ => {}
                    }
                }
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
                // Only persist shell preference when the user explicitly changed it
                let current_default = self.settings.default_shell.as_deref();
                let chosen = dlg.selected_shell.display_name();
                if current_default.is_some_and(|d| d != chosen) {
                    self.settings.default_shell = Some(chosen.to_string());
                    self.settings.save();
                }

                let ws_id = if dlg.save_as_workspace {
                    if let Some(existing_id) = dlg.existing_workspace_id {
                        // Update existing workspace
                        let new_name = dlg.workspace_name.trim().to_string();
                        let old_name = self
                            .workspace_store
                            .workspaces
                            .iter()
                            .find(|w| w.id == existing_id)
                            .map(|w| w.name.clone());
                        if let Some(ws) = self
                            .workspace_store
                            .workspaces
                            .iter_mut()
                            .find(|w| w.id == existing_id)
                        {
                            ws.name = new_name.clone();
                            ws.color = dlg.workspace_color;
                        }
                        self.workspace_store.save();
                        if let Some(old) = old_name {
                            if old != new_name {
                                self.note_store.rename_file(existing_id, &old, &new_name);
                            }
                        }
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
                            labels: vec![],
                            last_active_at: crate::util::now_millis(),
                            workspace_id: self.active_group,
                        });
                        self.pane_state.pane_trees.insert(
                            pane_id,
                            PaneNode::Leaf {
                                pane_id,
                                last_size: (cols, rows),
                            },
                        );
                        self.pane_state.add_pane_to_group(
                            self.pane_state.focused_group_id,
                            pane_id,
                            None,
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
