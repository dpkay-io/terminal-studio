use super::super::settings::CursorStyle;
use super::super::App;
use crate::theme;
use crate::updater::UpdateStatus;

impl App {
    pub(in crate::app) fn render_settings_overlay(&mut self, ctx: &egui::Context) {
        if self.show_settings {
            let mut settings_changed = false;
            let mut close_settings = false;
            let screen_rect = ctx.screen_rect();
            let dialog_w = (screen_rect.width() * 0.38).clamp(320.0, 520.0);
            let dialog_h = 520.0_f32;

            egui::Area::new(egui::Id::new("settings_dim"))
                .fixed_pos(screen_rect.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let resp = ui.interact(
                        screen_rect,
                        egui::Id::new("settings_dim_click"),
                        egui::Sense::click(),
                    );
                    ui.painter().rect_filled(
                        screen_rect,
                        0.0,
                        egui::Color32::from_black_alpha(theme::OVERLAY_DIM),
                    );
                    if resp.clicked() {
                        close_settings = true;
                    }
                });

            egui::Area::new(egui::Id::new("settings_dialog"))
                .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, dialog_h / 2.0))
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    egui::Frame::window(&ctx.style()).show(ui, |ui| {
                        ui.set_min_width(dialog_w);

                        // Header
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Settings")
                                    .strong()
                                    .size(theme::DIALOG_TITLE_SZ),
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
                                        close_settings = true;
                                    }
                                },
                            );
                        });
                        ui.separator();
                        ui.add_space(theme::SP_MD);

                        // Restore last session
                        let mut restore = self.settings.restore_last_session;
                        if ui
                            .checkbox(&mut restore, "Restore last session on launch")
                            .changed()
                        {
                            self.settings.restore_last_session = restore;
                            settings_changed = true;
                        }
                        ui.add_space(theme::SP_SM);
                        ui.label(
                            egui::RichText::new(
                                "When disabled, always opens a fresh session on launch.",
                            )
                            .size(11.0)
                            .color(theme::active().overlay0),
                        );

                        ui.add_space(theme::SP_XL);
                        ui.separator();
                        ui.add_space(theme::SP_MD);

                        // Theme picker
                        ui.label(egui::RichText::new("Theme").size(13.0));
                        ui.add_space(theme::SP_SM);
                        let current = self.settings.theme_id;
                        egui::ScrollArea::vertical()
                            .max_height(160.0)
                            .show(ui, |ui| {
                                for &id in theme::ThemeId::ALL {
                                    let t = &theme::all_themes()[id.index()];
                                    let is_selected = id == current;
                                    ui.horizontal(|ui| {
                                        let swatch_size = egui::vec2(48.0, 18.0);
                                        let (swatch_rect, _) = ui
                                            .allocate_exact_size(swatch_size, egui::Sense::hover());
                                        let sw = swatch_rect.width() / 4.0;
                                        for (i, &color) in
                                            [t.base, t.surface0, t.blue, t.green].iter().enumerate()
                                        {
                                            let r = egui::Rect::from_min_size(
                                                egui::pos2(
                                                    swatch_rect.min.x + i as f32 * sw,
                                                    swatch_rect.min.y,
                                                ),
                                                egui::vec2(sw, swatch_size.y),
                                            );
                                            ui.painter().rect_filled(r, 2.0, color);
                                        }
                                        let label = if is_selected {
                                            egui::RichText::new(id.name()).strong()
                                        } else {
                                            egui::RichText::new(id.name())
                                        };
                                        if ui.selectable_label(is_selected, label).clicked()
                                            && !is_selected
                                        {
                                            self.settings.theme_id = id;
                                            theme::set_theme(id);
                                            settings_changed = true;
                                        }
                                    });
                                }
                            });

                        // ── Terminal ──────────────────────────────────────────
                        ui.add_space(theme::SP_XL);
                        ui.separator();
                        ui.add_space(theme::SP_MD);
                        ui.label(egui::RichText::new("Terminal").size(13.0));
                        ui.add_space(theme::SP_SM);

                        // Font size
                        ui.horizontal(|ui| {
                            ui.label("Font size:");
                            let mut fs = self.settings.font_size;
                            if ui
                                .add(egui::DragValue::new(&mut fs).range(8.0..=28.0).speed(0.1))
                                .changed()
                            {
                                self.settings.font_size = fs;
                                settings_changed = true;
                            }
                        });
                        ui.add_space(theme::SP_SM);

                        // Scrollback lines
                        ui.horizontal(|ui| {
                            ui.label("Scrollback lines:");
                            let mut sl = self.settings.scrollback_lines;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut sl)
                                        .range(1000..=100000)
                                        .speed(100.0),
                                )
                                .changed()
                            {
                                self.settings.scrollback_lines = sl;
                                settings_changed = true;
                            }
                        });
                        ui.add_space(theme::SP_SM);

                        // Cursor style
                        ui.horizontal(|ui| {
                            ui.label("Cursor:");
                            let styles = [
                                CursorStyle::Block,
                                CursorStyle::Underline,
                                CursorStyle::Beam,
                            ];
                            let names = ["Block", "Underline", "Beam"];
                            for (style, name) in styles.iter().zip(names.iter()) {
                                if ui
                                    .selectable_label(self.settings.cursor_style == *style, *name)
                                    .clicked()
                                {
                                    self.settings.cursor_style = *style;
                                    settings_changed = true;
                                }
                            }
                        });
                        ui.add_space(theme::SP_SM);

                        // Cursor blink
                        {
                            let mut blink = self.settings.cursor_blink;
                            if ui.checkbox(&mut blink, "Cursor blink").changed() {
                                self.settings.cursor_blink = blink;
                                settings_changed = true;
                            }
                        }
                        ui.add_space(theme::SP_SM);

                        // Scroll on output
                        {
                            let mut soo = self.settings.scroll_on_output;
                            if ui
                                .checkbox(&mut soo, "Scroll to bottom on new output")
                                .changed()
                            {
                                self.settings.scroll_on_output = soo;
                                settings_changed = true;
                            }
                        }
                        ui.add_space(theme::SP_SM);

                        // Default shell
                        {
                            let shell_display = self
                                .settings
                                .default_shell
                                .clone()
                                .unwrap_or_else(|| "Auto-detect".to_string());
                            let shells: Vec<String> = self
                                .available_shells
                                .iter()
                                .map(|s| s.display_name().to_string())
                                .collect();
                            ui.horizontal(|ui| {
                                ui.label("Default shell:");
                                egui::ComboBox::from_id_source("settings_shell")
                                    .selected_text(&shell_display)
                                    .show_ui(ui, |ui| {
                                        if ui
                                            .selectable_label(
                                                self.settings.default_shell.is_none(),
                                                "Auto-detect",
                                            )
                                            .clicked()
                                        {
                                            self.settings.default_shell = None;
                                            settings_changed = true;
                                        }
                                        for name in &shells {
                                            let is_sel = self.settings.default_shell.as_deref()
                                                == Some(name.as_str());
                                            if ui.selectable_label(is_sel, name).clicked() {
                                                self.settings.default_shell = Some(name.clone());
                                                settings_changed = true;
                                            }
                                        }
                                    });
                            });
                        }

                        // ── About / Updates ───────────────────────────────────
                        ui.add_space(theme::SP_XL);
                        ui.separator();
                        ui.add_space(theme::SP_MD);
                        ui.label(egui::RichText::new("About").size(13.0));
                        ui.add_space(theme::SP_SM);
                        ui.horizontal(|ui| {
                            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if let Some(ref uc) = self.workers.update_checker {
                                        let update_state = uc.state();
                                        match &update_state.status {
                                            UpdateStatus::Checking => {
                                                ui.spinner();
                                                ui.label("Checking\u{2026}");
                                            }
                                            UpdateStatus::UpToDate => {
                                                ui.label(
                                                    egui::RichText::new("Up to date")
                                                        .size(12.0)
                                                        .color(theme::active().green),
                                                );
                                            }
                                            UpdateStatus::UpdateAvailable { version, .. } => {
                                                if ui
                                                    .button(format!("Update to v{version}"))
                                                    .clicked()
                                                {
                                                    uc.start_update();
                                                }
                                            }
                                            UpdateStatus::Downloading { progress_pct } => {
                                                ui.add(
                                                    egui::ProgressBar::new(progress_pct / 100.0)
                                                        .text("Downloading\u{2026}"),
                                                );
                                            }
                                            UpdateStatus::RestartRequired => {
                                                if ui
                                                    .button("Restart to finish update")
                                                    .clicked()
                                                {
                                                    crate::updater::restart_app();
                                                }
                                            }
                                            UpdateStatus::Error(msg) => {
                                                ui.label(
                                                    egui::RichText::new(msg)
                                                        .size(11.0)
                                                        .color(theme::active().red),
                                                );
                                                if ui.small_button("Retry").clicked() {
                                                    uc.trigger_check();
                                                }
                                            }
                                            _ => {
                                                if ui.button("Check for updates").clicked() {
                                                    uc.trigger_check();
                                                }
                                            }
                                        }
                                    } else {
                                        ui.label("Update checker unavailable");
                                    }
                                },
                            );
                        });
                    });
                });

            // Persist last_update_check timestamp
            if let Some(ref uc) = self.workers.update_checker {
                let us = uc.state();
                if us.last_check != self.settings.last_update_check {
                    self.settings.last_update_check = us.last_check;
                    settings_changed = true;
                }
            }

            if settings_changed {
                self.settings.save();
                self.apply_theme_visuals(ctx);
                self.cached_cell_size = None;
            }
            if close_settings {
                self.show_settings = false;
            }
        }

        // ── Shortcut help overlay ──────────────────────────────────────────
        if self.show_shortcut_help {
            let mut close_help = false;
            let screen_rect = ctx.screen_rect();
            let dialog_w = (screen_rect.width() * 0.55).clamp(400.0, 680.0);
            let dialog_h = (screen_rect.height() * 0.72).clamp(300.0, 560.0);

            egui::Area::new(egui::Id::new("shortcut_dim"))
                .fixed_pos(screen_rect.min)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let resp = ui.interact(
                        screen_rect,
                        egui::Id::new("shortcut_dim_click"),
                        egui::Sense::click(),
                    );
                    ui.painter().rect_filled(
                        screen_rect,
                        0.0,
                        egui::Color32::from_black_alpha(theme::OVERLAY_DIM),
                    );
                    if resp.clicked() {
                        close_help = true;
                    }
                });

            egui::Area::new(egui::Id::new("shortcut_dialog"))
                .fixed_pos(screen_rect.center() - egui::vec2(dialog_w / 2.0, dialog_h / 2.0))
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    egui::Frame::window(&ctx.style()).show(ui, |ui| {
                        ui.set_min_width(dialog_w);
                        ui.set_max_height(dialog_h);

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Keyboard Shortcuts")
                                    .strong()
                                    .size(theme::DIALOG_TITLE_SZ),
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
                                        close_help = true;
                                    }
                                },
                            );
                        });
                        ui.separator();
                        ui.add_space(theme::SP_SM);

                        egui::ScrollArea::vertical()
                            .max_height(dialog_h - 60.0)
                            .show(ui, |ui| {
                                let t = theme::active();
                                let groups = self.shortcut_registry.groups();
                                let half = groups.len().div_ceil(2);

                                ui.columns(2, |cols| {
                                    for (col_idx, col) in cols.iter_mut().enumerate() {
                                        let start = if col_idx == 0 { 0 } else { half };
                                        let end = if col_idx == 0 { half } else { groups.len() };
                                        for group in &groups[start..end] {
                                            if group.entries.is_empty() {
                                                continue;
                                            }
                                            col.add_space(theme::BAR_PAD_X);
                                            col.label(
                                                egui::RichText::new(group.name)
                                                    .strong()
                                                    .size(12.0)
                                                    .color(t.blue),
                                            );
                                            col.add_space(theme::SP_XS);
                                            for (action, shortcut) in &group.entries {
                                                col.horizontal(|ui| {
                                                    let desc = action.description();
                                                    ui.label(
                                                        egui::RichText::new(desc)
                                                            .size(12.0)
                                                            .color(t.text),
                                                    );
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            let label = shortcut.label();
                                                            let badge = egui::RichText::new(&label)
                                                                .size(11.0)
                                                                .color(t.subtext0)
                                                                .background_color(t.surface1);
                                                            ui.label(badge);
                                                        },
                                                    );
                                                });
                                            }
                                        }
                                    }
                                });

                                ui.add_space(theme::SP_MD);
                                ui.separator();
                                ui.add_space(theme::SP_SM);
                                ui.horizontal(|ui| {
                                    let t = theme::active();
                                    ui.label(
                                        egui::RichText::new("Alt+Arrow")
                                            .size(11.0)
                                            .color(t.subtext0)
                                            .background_color(t.surface1),
                                    );
                                    ui.label(
                                        egui::RichText::new("Move focus between split panes")
                                            .size(11.0)
                                            .color(t.overlay0),
                                    );
                                });
                            });
                    });
                });

            if close_help {
                self.show_shortcut_help = false;
            }
        }
    }
}
