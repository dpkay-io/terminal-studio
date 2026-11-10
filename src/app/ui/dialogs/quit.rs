use crate::app::pane::PaneContent;
use crate::app::App;
use crate::{theme, ui_kit};

fn quit_summary(terminals: usize, files: usize) -> String {
    let total = terminals + files;
    let tabs = if total == 1 { "tab" } else { "tabs" };
    let terminals_label = if terminals == 1 {
        "1 terminal".to_owned()
    } else {
        format!("{terminals} terminals")
    };
    let files_label = if files == 1 {
        "1 file".to_owned()
    } else {
        format!("{files} files")
    };
    let items = match (terminals, files) {
        (0, _) => files_label,
        (_, 0) => terminals_label,
        _ => format!("{terminals_label} and {files_label}"),
    };
    if terminals == 0 {
        format!("You have {total} open {tabs}: {items}.")
    } else {
        let those = if terminals == 1 {
            "that terminal"
        } else {
            "those terminals"
        };
        format!("You have {total} open {tabs}: {items}. Running processes in {those} will stop.")
    }
}

impl App {
    pub(in crate::app) fn render_quit_confirm(&mut self, ctx: &egui::Context) {
        if !self.show_quit_confirm {
            return;
        }

        let mut terminal_count = 0;
        let mut labels = Vec::with_capacity(self.pane_state.panes.len());
        for pane in &self.pane_state.panes {
            let label = match &pane.content {
                PaneContent::Terminal(session_id) => {
                    terminal_count += 1;
                    self.session_state
                        .find(*session_id)
                        .map(|entry| {
                            entry
                                .session
                                .try_read()
                                .map(|session| {
                                    let title = session.title();
                                    if title.is_empty() || title.starts_with("Session ") {
                                        entry.shell.display_name().to_owned()
                                    } else {
                                        title
                                    }
                                })
                                .unwrap_or_else(|| entry.shell.display_name().to_owned())
                        })
                        .unwrap_or_else(|| "Terminal".to_owned())
                }
                PaneContent::DeferredTerminal { saved_title, .. } => {
                    terminal_count += 1;
                    saved_title
                        .as_deref()
                        .filter(|title| !title.is_empty())
                        .unwrap_or("Terminal")
                        .to_owned()
                }
                PaneContent::FileEditor(editor) => editor
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("File")
                    .to_owned(),
                PaneContent::FileDiff(diff) => format!(
                    "Diff: {}",
                    diff.path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("file")
                ),
                PaneContent::NoteEditor(_) => "Notes".to_owned(),
                PaneContent::ConflictResolver(resolver) => format!(
                    "Conflicts: {}",
                    resolver
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("file")
                ),
            };
            labels.push(label);
        }
        let file_count = labels.len().saturating_sub(terminal_count);
        let summary = quit_summary(terminal_count, file_count);
        let mut do_quit = false;
        let mut cancel_clicked = false;
        let response = ui_kit::dialog(
            ctx,
            self.vp_id("quit"),
            ui_kit::DialogConfig {
                width: ui_kit::DialogWidth::Fixed(theme::QUIT_DIALOG_W),
                max_height: theme::QUIT_DIALOG_MAX_H,
                ..Default::default()
            },
            |ui| {
                ui.horizontal(|ui| {
                    egui::Frame::none()
                        .fill(theme::blend_colors(
                            theme::active().bg_raised,
                            theme::active().danger,
                            theme::BLEND_SUBTLE,
                        ))
                        .rounding(theme::R_MD)
                        .inner_margin(egui::Margin::same(theme::SP_3))
                        .show(ui, |ui| {
                            ui.label(ui_kit::icon(
                                ui_kit::icon::POWER,
                                theme::ICON_LG,
                                theme::active().danger,
                            ));
                        });
                    ui.add_space(theme::SP_2);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Quit Terminal Studio?")
                                .font(egui::FontId::new(theme::FONT_LG, crate::fonts::ui_bold()))
                                .color(theme::active().fg_strong),
                        );
                        ui.add_space(theme::SP_1);
                        ui.label(
                            egui::RichText::new(&summary)
                                .size(theme::FONT_MD)
                                .color(theme::active().fg_muted_raised),
                        );
                    });
                });

                if !labels.is_empty() {
                    ui.add_space(theme::SP_5);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(theme::SP_2, theme::SP_2);
                        for label in labels.iter().take(6) {
                            egui::Frame::none()
                                .fill(theme::active().bg_raised)
                                .rounding(theme::R_SM)
                                .inner_margin(egui::Margin::symmetric(theme::SP_2, theme::SP_1))
                                .show(ui, |ui| {
                                    ui_kit::truncate_label(
                                        ui,
                                        label,
                                        egui::FontId::proportional(theme::FONT_SM),
                                        theme::active().fg_muted_raised,
                                        theme::QUIT_TAB_CHIP_MAX_W,
                                    );
                                });
                        }
                        if labels.len() > 6 {
                            egui::Frame::none()
                                .fill(theme::active().bg_raised)
                                .rounding(theme::R_SM)
                                .inner_margin(egui::Margin::symmetric(theme::SP_2, theme::SP_1))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(format!("+{} more", labels.len() - 6))
                                            .size(theme::FONT_SM)
                                            .color(theme::active().fg_muted_raised),
                                    );
                                });
                        }
                    });
                }

                if let Some(index) = ui_kit::dialog_footer(
                    ui,
                    &[
                        ui_kit::DialogButton::Secondary("Cancel"),
                        ui_kit::DialogButton::Destructive("Quit"),
                    ],
                ) {
                    match index {
                        0 => cancel_clicked = true,
                        1 => do_quit = true,
                        _ => {}
                    }
                }
            },
        );

        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            do_quit = true;
        }

        if do_quit {
            self.show_quit_confirm = false;
            self.quit_confirmed = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if response.dismissed || cancel_clicked {
            self.show_quit_confirm = false;
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn quit_summary_text() {
        assert_eq!(
            super::quit_summary(2, 1),
            "You have 3 open tabs: 2 terminals and 1 file. Running processes in those terminals will stop."
        );
        assert_eq!(
            super::quit_summary(1, 0),
            "You have 1 open tab: 1 terminal. Running processes in that terminal will stop."
        );
        assert_eq!(super::quit_summary(0, 1), "You have 1 open tab: 1 file.");
    }
}
