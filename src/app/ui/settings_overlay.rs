use super::super::settings::CursorStyle;
use super::super::App;
use super::settings::theme_grid::{grid_columns, theme_card};
use crate::shortcuts::{AppAction, ShortcutRegistry};
use crate::theme;
use crate::ui_kit;
use crate::updater::UpdateStatus;

const SETTINGS_DIALOG_MIN_WIDTH: f32 = 560.0;
const SETTINGS_DIALOG_MAX_WIDTH: f32 = 680.0;

#[derive(Clone, Copy)]
struct ToggleSettingDescriptor {
    label: &'static str,
    hint: &'static str,
}

const TOGGLE_SETTINGS: [ToggleSettingDescriptor; 6] = [
    ToggleSettingDescriptor {
        label: "Restore last session on launch",
        hint: "When disabled, always opens a fresh session on launch.",
    },
    ToggleSettingDescriptor {
        label: "Save terminal output on exit",
        hint: "Preserves terminal scrollback so it is visible when the app restarts.",
    },
    ToggleSettingDescriptor {
        label: "Remember closed sessions",
        hint: "Keeps recently closed terminal sessions available to restore.",
    },
    ToggleSettingDescriptor {
        label: "Show system monitor in titlebar",
        hint: "Displays live CPU, memory, and network activity in the titlebar.",
    },
    ToggleSettingDescriptor {
        label: "Cursor blink",
        hint: "Animates the terminal cursor while the terminal is focused.",
    },
    ToggleSettingDescriptor {
        label: "Scroll to bottom on new output",
        hint: "Follows new terminal output unless you have scrolled up.",
    },
];

fn whole_point_font_size(value: f32) -> f32 {
    value.round().clamp(8.0, 28.0)
}

fn section_header(ui: &mut egui::Ui, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .size(theme::FONT_XS)
            .color(theme::active().fg_muted)
            .strong(),
    );
}

fn section_break(ui: &mut egui::Ui) {
    ui.add_space(theme::SP_6);
    let separator = ui
        .allocate_space(egui::vec2(ui.available_width(), theme::STROKE_THIN))
        .1;
    ui_kit::gradient_separator(ui.painter(), separator);
    ui.add_space(theme::SP_4);
}

fn toggle_setting_row(
    ui: &mut egui::Ui,
    descriptor: ToggleSettingDescriptor,
    hint: impl Into<String>,
    value: &mut bool,
) -> egui::Response {
    let hint = hint.into();
    let previous_value = *value;
    let row = ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(descriptor.label)
                    .size(theme::FONT_MD)
                    .color(theme::active().fg),
            );
            ui.label(
                egui::RichText::new(hint)
                    .size(theme::FONT_SM)
                    .color(theme::active().fg_muted),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui_kit::toggle_switch(ui, value)
        })
        .inner
    });
    let mut response = row.inner;
    if *value != previous_value {
        response.mark_changed();
    }
    response
}

fn setting_control_row(
    ui: &mut egui::Ui,
    label: &str,
    add_control: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(theme::FONT_MD)
                .color(theme::active().fg),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.scope(|ui| {
                let bg = theme::active().bg_base;
                ui.visuals_mut().widgets.inactive.bg_fill = bg;
                ui.visuals_mut().widgets.hovered.bg_fill = bg;
                ui.visuals_mut().widgets.active.bg_fill = bg;
                add_control(ui)
            })
            .inner
        })
        .inner
    })
    .inner
}

fn closed_sessions_hint(registry: &ShortcutRegistry) -> String {
    let shortcut = registry
        .label_for(AppAction::ReopenClosedSession)
        .unwrap_or("unassigned");
    format!("Saves closed terminal sessions so they can be restored later ({shortcut}).")
}

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
                        min: SETTINGS_DIALOG_MIN_WIDTH,
                        max: SETTINGS_DIALOG_MAX_WIDTH,
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
                                    .size(theme::FONT_XS)
                                    .color(theme::active().success),
                            );
                        }
                    }

                    egui::ScrollArea::vertical()
                        .id_source("settings_main_scroll")
                        .max_height(dialog_h - 60.0)
                        .show(ui, |ui| {
                            ui.add_space(theme::SP_4);

                            let mut restore = self.settings.restore_last_session;
                            if toggle_setting_row(
                                ui,
                                TOGGLE_SETTINGS[0],
                                TOGGLE_SETTINGS[0].hint,
                                &mut restore,
                            )
                            .changed()
                            {
                                self.settings.restore_last_session = restore;
                                settings_changed = true;
                            }
                            ui.add_space(theme::SP_2);

                            {
                                let mut save_exit = self.settings.save_scrollback_on_exit;
                                if toggle_setting_row(
                                    ui,
                                    TOGGLE_SETTINGS[1],
                                    TOGGLE_SETTINGS[1].hint,
                                    &mut save_exit,
                                )
                                .changed()
                                {
                                    self.settings.save_scrollback_on_exit = save_exit;
                                    settings_changed = true;
                                }
                            }

                            ui.add_space(theme::SP_2);

                            {
                                let mut save_close = self.settings.save_scrollback_on_close;
                                if toggle_setting_row(
                                    ui,
                                    TOGGLE_SETTINGS[2],
                                    closed_sessions_hint(&self.shortcut_registry),
                                    &mut save_close,
                                )
                                .changed()
                                {
                                    self.settings.save_scrollback_on_close = save_close;
                                    settings_changed = true;
                                }
                            }

                            ui.add_space(theme::SP_4);

                            let mut max = self.settings.max_closed_sessions;
                            if setting_control_row(ui, "Max closed sessions to remember", |ui| {
                                ui.add(egui::DragValue::new(&mut max).range(5..=100).speed(1.0))
                            })
                            .changed()
                            {
                                self.settings.max_closed_sessions = max;
                                settings_changed = true;
                            }

                            ui.add_space(theme::SP_4);

                            {
                                let mut show = self.settings.show_sys_monitor;
                                if toggle_setting_row(
                                    ui,
                                    TOGGLE_SETTINGS[3],
                                    TOGGLE_SETTINGS[3].hint,
                                    &mut show,
                                )
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

                            section_break(ui);

                            section_header(ui, "THEME");
                            ui.add_space(theme::SP_2);
                            let current = self.settings.theme_id;
                            let columns = grid_columns(theme::ThemeId::ALL.len());
                            let card_width = (ui.available_width()
                                - theme::SP_2 * (columns.saturating_sub(1)) as f32)
                                / columns as f32;
                            for row in theme::all_themes().chunks(columns) {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = theme::SP_2;
                                    for theme_data in row {
                                        ui.allocate_ui(egui::vec2(card_width, 0.0), |ui| {
                                            ui.set_width(card_width);
                                            if theme_card(ui, theme_data, theme_data.id == current)
                                                .clicked()
                                                && theme_data.id != current
                                            {
                                                self.settings.theme_id = theme_data.id;
                                                theme::set_theme(theme_data.id);
                                                settings_changed = true;
                                            }
                                        });
                                    }
                                });
                                ui.add_space(theme::SP_1);
                            }

                            // ── Terminal ──────────────────────────────────────────
                            section_break(ui);
                            section_header(ui, "TERMINAL");
                            ui.add_space(theme::SP_2);

                            let mut font_size = self.settings.font_size;
                            let font_response = setting_control_row(ui, "Font size", |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut font_size)
                                        .range(8.0..=28.0)
                                        .speed(1.0)
                                        .fixed_decimals(0),
                                )
                            });
                            if font_response.changed() {
                                self.settings.font_size = whole_point_font_size(font_size);
                                self.cached_cell_size = None;
                            }
                            if font_response.drag_stopped()
                                || (font_response.changed() && !font_response.dragged())
                            {
                                settings_changed = true;
                            }
                            ui.add_space(theme::SP_2);

                            let mut scrollback_lines = self.settings.scrollback_lines;
                            if setting_control_row(ui, "Scrollback lines", |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut scrollback_lines)
                                        .range(1_000..=1_000_000)
                                        .speed(1000.0),
                                )
                            })
                            .changed()
                            {
                                self.settings.scrollback_lines = scrollback_lines;
                                settings_changed = true;
                            }
                            ui.add_space(theme::SP_2);

                            let previous_cursor_style = self.settings.cursor_style;
                            setting_control_row(ui, "Cursor", |ui| {
                                ui_kit::segmented(
                                    ui,
                                    &mut self.settings.cursor_style,
                                    &[
                                        (CursorStyle::Block, "Block"),
                                        (CursorStyle::Beam, "Beam"),
                                        (CursorStyle::Underline, "Underline"),
                                    ],
                                )
                            });
                            if self.settings.cursor_style != previous_cursor_style {
                                settings_changed = true;
                            }
                            ui.add_space(theme::SP_2);

                            {
                                let mut blink = self.settings.cursor_blink;
                                if toggle_setting_row(
                                    ui,
                                    TOGGLE_SETTINGS[4],
                                    TOGGLE_SETTINGS[4].hint,
                                    &mut blink,
                                )
                                .changed()
                                {
                                    self.settings.cursor_blink = blink;
                                    settings_changed = true;
                                }
                            }
                            ui.add_space(theme::SP_2);

                            {
                                let mut scroll_on_output = self.settings.scroll_on_output;
                                if toggle_setting_row(
                                    ui,
                                    TOGGLE_SETTINGS[5],
                                    TOGGLE_SETTINGS[5].hint,
                                    &mut scroll_on_output,
                                )
                                .changed()
                                {
                                    self.settings.scroll_on_output = scroll_on_output;
                                    settings_changed = true;
                                }
                            }
                            ui.add_space(theme::SP_2);

                            let mut scroll_lines = self.settings.scroll_lines;
                            if setting_control_row(ui, "Lines per scroll tick", |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut scroll_lines)
                                        .range(1..=20)
                                        .speed(1.0),
                                )
                            })
                            .changed()
                            {
                                self.settings.scroll_lines = scroll_lines;
                                settings_changed = true;
                            }
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

                            // ── Logging ──────────────────────────────────────────
                            section_break(ui);
                            section_header(ui, "LOGGING");
                            ui.add_space(theme::SP_2);

                            ui.horizontal(|ui| {
                                ui.label("Log level:");
                                let current_level = self.settings.log_level;
                                egui::ComboBox::from_id_source(self.vp_id("settings_log_level"))
                                    .selected_text(current_level.name())
                                    .show_ui(ui, |ui| {
                                        for &level in crate::logging::LogLevel::ALL {
                                            let resp = ui.selectable_label(
                                                current_level == level,
                                                format!(
                                                    "{} — {}",
                                                    level.name(),
                                                    level.description()
                                                ),
                                            );
                                            if resp.clicked() && current_level != level {
                                                self.settings.log_level = level;
                                                settings_changed = true;
                                            }
                                        }
                                    });
                            });
                            ui.add_space(theme::SP_2);
                            ui.label(ui_kit::label_muted(
                                "Changes take effect on restart. Override with RUST_LOG env var.",
                            ));
                            ui.add_space(theme::SP_2);
                            if ui.small_button("Open log folder").clicked() {
                                if let Some(dir) = crate::logging::log_dir() {
                                    let _ = std::fs::create_dir_all(&dir);
                                    let _ = open::that(&dir);
                                }
                            }

                            // ── About / Updates ───────────────────────────────────
                            section_break(ui);
                            section_header(ui, "ABOUT");
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
                                                            .size(theme::FONT_SM)
                                                            .color(theme::active().success),
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
                                                        self.save_session();
                                                        crate::updater::restart_app();
                                                    }
                                                }
                                                UpdateStatus::Error(msg) => {
                                                    ui.label(
                                                        egui::RichText::new(msg)
                                                            .size(theme::FONT_XS)
                                                            .color(theme::active().danger_text),
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

        super::shortcuts_dialog::render_shortcuts_dialog(self, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keybindings::KeybindingsConfig;
    use crate::shortcuts::ShortcutRegistry;
    use crate::theme::{self, ThemeId};
    use crate::ui_kit::test_harness::{ctx_with_fonts, run_ui};

    #[test]
    fn theme_grid_is_four_columns_for_sixteen() {
        let columns = grid_columns(16);
        let rows = 16_usize.div_ceil(columns);

        assert_eq!(columns, 4);
        assert_eq!(rows, 4);
    }

    #[test]
    fn theme_card_shows_full_name() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let available_card_width =
            (SETTINGS_DIALOG_MIN_WIDTH - theme::SP_4 * 2.0 - theme::SP_2 * 3.0) / 4.0;

        for theme_data in theme::all_themes() {
            let expected_width = ctx.fonts(|fonts| {
                fonts
                    .layout_no_wrap(
                        theme_data.id.name().to_owned(),
                        egui::FontId::proportional(theme::FONT_MD),
                        theme::active().fg,
                    )
                    .size()
                    .x
            });
            assert!(
                expected_width <= available_card_width,
                "{} needs {expected_width}px but only {available_card_width}px is available",
                theme_data.id.name()
            );

            let output = run_ui(&ctx, egui::RawInput::default(), |ui| {
                ui.set_width(available_card_width);
                let response = theme_card(ui, theme_data, false);
                assert!(response.rect.width() >= expected_width);
            });
            assert!(!output.shapes.is_empty());
        }
    }

    #[test]
    fn settings_setting_rows_have_hints() {
        assert_eq!(TOGGLE_SETTINGS.len(), 6);
        assert!(TOGGLE_SETTINGS
            .iter()
            .all(|setting| !setting.hint.trim().is_empty()));
    }

    #[test]
    fn font_size_uses_whole_point_steps() {
        assert_eq!(whole_point_font_size(13.49), 13.0);
        assert_eq!(whole_point_font_size(13.5), 14.0);
    }

    #[test]
    fn closed_sessions_hint_uses_registry_display_string() {
        let registry = ShortcutRegistry::from_config(KeybindingsConfig::from_json(
            r#"[{
                "action": "reopen_closed_session",
                "ctrl": true,
                "shift": true,
                "alt": false,
                "key": "R"
            }]"#,
        ));

        let expected = if cfg!(target_os = "macos") {
            "\u{2318}+\u{21e7}+R"
        } else {
            "Ctrl+Shift+R"
        };
        assert!(closed_sessions_hint(&registry).contains(expected));
    }
}
