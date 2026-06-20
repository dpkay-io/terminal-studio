use super::super::settings::CursorStyle;
use super::super::App;
use crate::theme;
use crate::ui_kit;
use crate::updater::UpdateStatus;

impl App {
    pub(in crate::app) fn render_settings_overlay(&mut self, ctx: &egui::Context) {
        if self.show_settings {
            let mut settings_changed = false;
            let mut close_settings = false;
            let dialog_h = (ctx.screen_rect().height() * 0.85).clamp(300.0, 520.0);

            let dialog_resp = ui_kit::dialog(
                ctx,
                self.vp_id("settings"),
                ui_kit::DialogConfig {
                    width: ui_kit::DialogWidth::Responsive {
                        pct: 0.38,
                        min: 320.0,
                        max: 520.0,
                    },
                    max_height: dialog_h,
                    anchor: ui_kit::DialogAnchor::Center,
                    ..Default::default()
                },
                |ui| {
                    if ui_kit::dialog_header_with_close(ui, "Settings") {
                        close_settings = true;
                    }
                    if let Some(saved_at) = self.settings_saved_at {
                        if saved_at.elapsed() < std::time::Duration::from_secs(2) {
                            ui.label(
                                egui::RichText::new("\u{2713} Saved")
                                    .size(theme::FONT_UI_SM)
                                    .color(theme::active().success),
                            );
                        }
                    }

                    egui::ScrollArea::vertical()
                        .id_source("settings_main_scroll")
                        .max_height(dialog_h - 60.0)
                        .show(ui, |ui| {
                            ui.add_space(theme::SP_4);

                            // Restore last session
                            let mut restore = self.settings.restore_last_session;
                            if ui
                                .checkbox(&mut restore, "Restore last session on launch")
                                .changed()
                            {
                                self.settings.restore_last_session = restore;
                                settings_changed = true;
                            }
                            ui.add_space(theme::SP_2);
                            ui.label(ui_kit::label_muted(
                                "When disabled, always opens a fresh session on launch.",
                            ));

                            ui.add_space(theme::SP_4);

                            // Save scrollback on exit
                            {
                                let mut save_exit = self.settings.save_scrollback_on_exit;
                                if ui
                                    .checkbox(&mut save_exit, "Save terminal output on exit")
                                    .changed()
                                {
                                    self.settings.save_scrollback_on_exit = save_exit;
                                    settings_changed = true;
                                }
                                ui.add_space(theme::SP_2);
                                ui.label(ui_kit::label_muted(
                                    "Preserves terminal scrollback so it is visible when the app restarts.",
                                ));
                            }

                            ui.add_space(theme::SP_4);

                            // Save scrollback on close
                            {
                                let mut save_close = self.settings.save_scrollback_on_close;
                                if ui
                                    .checkbox(&mut save_close, "Remember closed sessions")
                                    .changed()
                                {
                                    self.settings.save_scrollback_on_close = save_close;
                                    settings_changed = true;
                                }
                                ui.add_space(theme::SP_2);
                                ui.label(ui_kit::label_muted(
                                    "Saves closed terminal sessions so they can be restored later (Ctrl+Shift+T).",
                                ));
                            }

                            ui.add_space(theme::SP_4);

                            // Max closed sessions
                            ui.horizontal(|ui| {
                                ui.label("Max closed sessions to remember:");
                                let mut max = self.settings.max_closed_sessions;
                                if ui
                                    .add(egui::DragValue::new(&mut max).range(5..=100).speed(1.0))
                                    .changed()
                                {
                                    self.settings.max_closed_sessions = max;
                                    settings_changed = true;
                                }
                            });

                            ui.add_space(theme::SP_4);

                            // Show system monitor
                            {
                                let mut show = self.settings.show_sys_monitor;
                                if ui
                                    .checkbox(&mut show, "Show system monitor in titlebar")
                                    .changed()
                                {
                                    self.settings.show_sys_monitor = show;
                                    if show {
                                        if self.workers.sys_monitor.is_none() {
                                            self.workers.sys_monitor =
                                                crate::sys_monitor::SysMonitor::spawn(
                                                    ctx.clone(),
                                                    std::time::Duration::from_secs(2),
                                                );
                                        }
                                    } else {
                                        self.workers.sys_monitor = None;
                                    }
                                    settings_changed = true;
                                }
                            }

                            ui.add_space(theme::SP_6);
                            {
                                let sep_rect = ui.allocate_space(egui::vec2(ui.available_width(), 1.0)).1;
                                crate::ui_kit::gradient_separator(ui.painter(), sep_rect);
                            }
                            ui.add_space(theme::SP_4);

                            // Theme picker
                            ui.label(
                                egui::RichText::new("THEME")
                                    .size(theme::FONT_UI_SM)
                                    .color(theme::active().fg_secondary)
                                    .strong(),
                            );
                            ui.add_space(theme::SP_2);
                            let current = self.settings.theme_id;
                            egui::ScrollArea::vertical()
                                .id_source("settings_theme_scroll")
                                .max_height(160.0)
                                .show(ui, |ui| {
                                    let swatch_outer_w = 52.0 + theme::SP_2 * 2.0;
                                    let swatch_color_h = 20.0;
                                    let label_h = theme::FONT_UI_SM + theme::SP_1;
                                    let swatch_outer_h = swatch_color_h + label_h + theme::SP_2 * 2.0;
                                    let available_w = ui.available_width();
                                    let cols = ((available_w / swatch_outer_w).floor() as usize).max(1);

                                    let themes: Vec<_> = theme::ThemeId::ALL.to_vec();
                                    for row_themes in themes.chunks(cols) {
                                        ui.horizontal(|ui| {
                                            for &id in row_themes {
                                                let t_data = &theme::all_themes()[id.index()];
                                                let is_selected = id == current;
                                                let t_cur = theme::active();

                                                let (outer_rect, resp) = ui.allocate_exact_size(
                                                    egui::vec2(swatch_outer_w, swatch_outer_h),
                                                    egui::Sense::click(),
                                                );

                                                let bg_color = if resp.hovered() {
                                                    t_cur.surface1
                                                } else {
                                                    t_cur.surface0
                                                };
                                                ui.painter().rect_filled(outer_rect, theme::R_MD, bg_color);

                                                if is_selected {
                                                    ui.painter().rect_stroke(
                                                        outer_rect,
                                                        theme::R_MD,
                                                        egui::Stroke::new(theme::STROKE_THIN, t_cur.accent),
                                                    );
                                                }

                                                let color_rect = egui::Rect::from_min_size(
                                                    egui::pos2(
                                                        outer_rect.min.x + theme::SP_2,
                                                        outer_rect.min.y + theme::SP_2,
                                                    ),
                                                    egui::vec2(52.0, swatch_color_h),
                                                );
                                                let sw = color_rect.width() / 4.0;
                                                for (i, &color) in [t_data.base, t_data.surface0, t_data.blue, t_data.green]
                                                    .iter()
                                                    .enumerate()
                                                {
                                                    let seg = egui::Rect::from_min_size(
                                                        egui::pos2(
                                                            color_rect.min.x + i as f32 * sw,
                                                            color_rect.min.y,
                                                        ),
                                                        egui::vec2(sw, swatch_color_h),
                                                    );
                                                    let rounding = if i == 0 {
                                                        egui::Rounding {
                                                            nw: theme::R_SM,
                                                            sw: theme::R_SM,
                                                            ne: 0.0,
                                                            se: 0.0,
                                                        }
                                                    } else if i == 3 {
                                                        egui::Rounding {
                                                            nw: 0.0,
                                                            sw: 0.0,
                                                            ne: theme::R_SM,
                                                            se: theme::R_SM,
                                                        }
                                                    } else {
                                                        egui::Rounding::ZERO
                                                    };
                                                    ui.painter().rect_filled(seg, rounding, color);
                                                }

                                                let name_color = if is_selected {
                                                    t_cur.text
                                                } else {
                                                    t_cur.fg_muted
                                                };
                                                let name_pos = egui::pos2(
                                                    color_rect.min.x,
                                                    color_rect.max.y + theme::SP_1,
                                                );
                                                ui.painter().text(
                                                    name_pos,
                                                    egui::Align2::LEFT_TOP,
                                                    id.name(),
                                                    egui::FontId::proportional(theme::FONT_UI_SM),
                                                    name_color,
                                                );

                                                if resp.clicked() && !is_selected {
                                                    self.settings.theme_id = id;
                                                    theme::set_theme(id);
                                                    settings_changed = true;
                                                }
                                            }
                                        });
                                        ui.add_space(theme::SP_1);
                                    }
                                });

                            // ── Terminal ──────────────────────────────────────────
                            ui.add_space(theme::SP_6);
                            {
                                let sep_rect = ui.allocate_space(egui::vec2(ui.available_width(), 1.0)).1;
                                crate::ui_kit::gradient_separator(ui.painter(), sep_rect);
                            }
                            ui.add_space(theme::SP_4);
                            ui.label(
                                egui::RichText::new("TERMINAL")
                                    .size(theme::FONT_UI_SM)
                                    .color(theme::active().fg_secondary)
                                    .strong(),
                            );
                            ui.add_space(theme::SP_2);

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
                            ui.add_space(theme::SP_2);

                            // Scrollback lines
                            ui.horizontal(|ui| {
                                ui.label("Scrollback lines:");
                                let mut sl = self.settings.scrollback_lines;
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut sl)
                                            .range(1_000..=1_000_000)
                                            .speed(1000.0),
                                    )
                                    .changed()
                                {
                                    self.settings.scrollback_lines = sl;
                                    settings_changed = true;
                                }
                            });
                            ui.add_space(theme::SP_2);

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
                                        .selectable_label(
                                            self.settings.cursor_style == *style,
                                            *name,
                                        )
                                        .clicked()
                                    {
                                        self.settings.cursor_style = *style;
                                        settings_changed = true;
                                    }
                                }
                            });
                            ui.add_space(theme::SP_2);

                            // Cursor blink
                            {
                                let mut blink = self.settings.cursor_blink;
                                if ui.checkbox(&mut blink, "Cursor blink").changed() {
                                    self.settings.cursor_blink = blink;
                                    settings_changed = true;
                                }
                            }
                            ui.add_space(theme::SP_2);

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
                            ui.add_space(theme::SP_2);

                            // Scroll lines per tick
                            ui.horizontal(|ui| {
                                ui.label("Lines per scroll tick:");
                                let mut sl = self.settings.scroll_lines;
                                if ui
                                    .add(egui::DragValue::new(&mut sl).range(1..=20).speed(0.1))
                                    .changed()
                                {
                                    self.settings.scroll_lines = sl;
                                    settings_changed = true;
                                }
                            });
                            ui.add_space(theme::SP_2);

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
                                    egui::ComboBox::from_id_source(self.vp_id("settings_shell"))
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
                                                    self.settings.default_shell =
                                                        Some(name.clone());
                                                    settings_changed = true;
                                                }
                                            }
                                        });
                                });
                            }

                            // ── About / Updates ───────────────────────────────────
                            ui.add_space(theme::SP_6);
                            {
                                let sep_rect = ui.allocate_space(egui::vec2(ui.available_width(), 1.0)).1;
                                crate::ui_kit::gradient_separator(ui.painter(), sep_rect);
                            }
                            ui.add_space(theme::SP_4);
                            ui.label(
                                egui::RichText::new("ABOUT")
                                    .size(theme::FONT_UI_SM)
                                    .color(theme::active().fg_secondary)
                                    .strong(),
                            );
                            ui.add_space(theme::SP_2);
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
                                                            .size(theme::FONT_UI_MD)
                                                            .color(theme::active().green),
                                                    );
                                                    if ui.small_button("Check again").clicked() {
                                                        uc.trigger_check();
                                                    }
                                                }
                                                UpdateStatus::UpdateAvailable {
                                                    version, ..
                                                } => {
                                                    if ui
                                                        .button(format!("Update to v{version}"))
                                                        .clicked()
                                                    {
                                                        uc.start_update();
                                                    }
                                                }
                                                UpdateStatus::Downloading { progress_pct } => {
                                                    ui.add(
                                                        egui::ProgressBar::new(
                                                            progress_pct / 100.0,
                                                        )
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
                                                            .size(theme::FONT_UI_SM)
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
                        }); // end ScrollArea
                },
            );
            if dialog_resp.dismissed {
                close_settings = true;
            }

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
                self.settings_saved_at = Some(std::time::Instant::now());
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
            let dialog_h = (ctx.screen_rect().height() * 0.72).clamp(300.0, 560.0);

            let help_resp = ui_kit::dialog(
                ctx,
                self.vp_id("shortcut"),
                ui_kit::DialogConfig {
                    width: ui_kit::DialogWidth::Responsive {
                        pct: 0.55,
                        min: 400.0,
                        max: 680.0,
                    },
                    max_height: dialog_h,
                    anchor: ui_kit::DialogAnchor::Center,
                    ..Default::default()
                },
                |ui| {
                    if ui_kit::dialog_header_with_close(ui, "Keyboard Shortcuts") {
                        close_help = true;
                    }
                    ui.add_space(theme::SP_2);

                    egui::ScrollArea::vertical()
                        .id_source("shortcuts_scroll")
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
                                        col.add_space(theme::SP_3);
                                        col.label(
                                            egui::RichText::new(group.name)
                                                .strong()
                                                .size(theme::FONT_UI_MD)
                                                .color(t.blue),
                                        );
                                        col.add_space(theme::SP_1);
                                        for (action, shortcut) in &group.entries {
                                            col.horizontal(|ui| {
                                                let desc = action.description();
                                                ui.label(
                                                    egui::RichText::new(desc)
                                                        .size(theme::FONT_UI_MD)
                                                        .color(t.text),
                                                );
                                                ui.with_layout(
                                                    egui::Layout::right_to_left(
                                                        egui::Align::Center,
                                                    ),
                                                    |ui| {
                                                        let label = shortcut.label();
                                                        let shortcut_fg = theme::ensure_readable(
                                                            t.subtext0_rgb,
                                                            t.surface1_rgb,
                                                        );
                                                        let badge = egui::RichText::new(&label)
                                                            .size(theme::FONT_UI_SM)
                                                            .color(shortcut_fg)
                                                            .background_color(t.surface1);
                                                        ui.label(badge);
                                                    },
                                                );
                                            });
                                        }
                                    }
                                }
                            });

                            ui.add_space(theme::SP_4);
                            {
                                let sep_rect =
                                    ui.allocate_space(egui::vec2(ui.available_width(), 1.0)).1;
                                crate::ui_kit::gradient_separator(ui.painter(), sep_rect);
                            }
                            ui.add_space(theme::SP_2);
                            ui.horizontal(|ui| {
                                let t = theme::active();
                                let split_shortcut_fg =
                                    theme::ensure_readable(t.subtext0_rgb, t.surface1_rgb);
                                ui.label(
                                    egui::RichText::new("Alt+Arrow")
                                        .size(theme::FONT_UI_SM)
                                        .color(split_shortcut_fg)
                                        .background_color(t.surface1),
                                );
                                ui.label(
                                    egui::RichText::new("Move focus between split panes")
                                        .size(theme::FONT_UI_SM)
                                        .color(t.overlay0),
                                );
                            });
                        });
                },
            );
            if help_resp.dismissed {
                close_help = true;
            }
            if close_help {
                self.show_shortcut_help = false;
            }
        }
    }
}
