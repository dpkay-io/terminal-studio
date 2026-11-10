use crate::app::App;
use crate::{theme, ui_kit};

impl App {
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
                if let Some(cwd) = self.active_pane_cwd() {
                    self.workers.git_worker.enqueue_last_commit_msg(&cwd);
                }
            }

            let can_commit = !self.commit_message.trim().is_empty();
            ui.label(ui_kit::hint("Ctrl+Enter to commit"));
            let commit_button = if can_commit {
                ui_kit::DialogButton::Primary("Commit")
            } else {
                ui_kit::DialogButton::DisabledPrimary("Commit")
            };
            if let Some(index) = ui_kit::dialog_footer(
                ui,
                &[ui_kit::DialogButton::Secondary("Cancel"), commit_button],
            ) {
                match index {
                    0 => cancel_clicked = true,
                    1 if can_commit => do_commit = true,
                    _ => {}
                }
            }
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
            if let Some(cwd) = self.active_pane_cwd() {
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
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Fixed(340.0),
            max_height: 180.0,
            ..Default::default()
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("push"), config, |ui| {
            ui_kit::dialog_header(ui, "Push to Remote");

            ui.label("Push commits to the remote branch?");
            ui.add_space(theme::SP_3);

            ui.checkbox(&mut self.push_force, "Force push");
            if self.push_force {
                ui.add_space(theme::SP_1);
                ui.label(
                    egui::RichText::new("\u{26a0} This will overwrite remote history. Commits others have pulled may be lost.")
                        .size(theme::FONT_XS)
                        .color(theme::active().error),
                );
            }

            let push_button = if self.push_force {
                ui_kit::DialogButton::Destructive("Force Push")
            } else {
                ui_kit::DialogButton::Primary("Push")
            };
            if let Some(index) = ui_kit::dialog_footer(
                ui,
                &[ui_kit::DialogButton::Secondary("Cancel"), push_button],
            ) {
                match index {
                    0 => cancel_clicked = true,
                    1 => do_push = true,
                    _ => {}
                }
            }
        });

        // Enter shortcut
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            do_push = true;
        }

        if do_push {
            let force = self.push_force;
            if let Some(cwd) = self.active_pane_cwd() {
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

            if let Some(index) = ui_kit::dialog_footer(
                ui,
                &[
                    ui_kit::DialogButton::Secondary("No"),
                    ui_kit::DialogButton::Primary("Yes"),
                ],
            ) {
                match index {
                    0 => cancel_clicked = true,
                    1 => do_stage = true,
                    _ => {}
                }
            }
        });

        // Enter shortcut
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            do_stage = true;
        }

        if do_stage {
            if let Some(cwd) = self.active_pane_cwd() {
                self.workers.git_worker.enqueue_stage_all(&cwd);
            }
            self.show_stage_all_confirm = false;
        } else if resp.dismissed || cancel_clicked {
            self.show_stage_all_confirm = false;
        }
    }

    pub(in crate::app) fn render_revert_confirm(&mut self, ctx: &egui::Context) {
        let file_path = match &self.revert_confirm_file {
            Some(p) => p.clone(),
            None => return,
        };

        let mut do_revert = false;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Fixed(380.0),
            max_height: 140.0,
            ..Default::default()
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("revert_confirm"), config, |ui| {
            ui_kit::dialog_header(ui, "Revert Changes");

            ui.label(
                egui::RichText::new(format!("Discard all changes to \"{}\"?", file_path))
                    .size(crate::theme::FONT_SM),
            );
            ui.add_space(crate::theme::SP_1);
            ui.label(
                egui::RichText::new("This cannot be undone.")
                    .size(crate::theme::FONT_XS)
                    .color(crate::theme::active().error),
            );

            if let Some(index) = ui_kit::dialog_footer(
                ui,
                &[
                    ui_kit::DialogButton::Secondary("No"),
                    ui_kit::DialogButton::Destructive("Yes"),
                ],
            ) {
                match index {
                    0 => cancel_clicked = true,
                    1 => do_revert = true,
                    _ => {}
                }
            }
        });

        if do_revert {
            if let Some(cwd) = self.active_pane_cwd() {
                self.workers.git_worker.enqueue_revert(&cwd, file_path);
            }
            self.revert_confirm_file = None;
        } else if resp.dismissed || cancel_clicked {
            self.revert_confirm_file = None;
        }
    }

    pub(in crate::app) fn render_delete_confirm(&mut self, ctx: &egui::Context) {
        let file_path = match &self.delete_confirm_file {
            Some(p) => p.clone(),
            None => return,
        };

        let mut do_delete = false;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Fixed(380.0),
            max_height: 140.0,
            ..Default::default()
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("delete_confirm"), config, |ui| {
            ui_kit::dialog_header(ui, "Delete File");

            ui.label(
                egui::RichText::new(format!("Delete \"{}\"?", file_path))
                    .size(crate::theme::FONT_SM),
            );
            ui.add_space(crate::theme::SP_1);
            ui.label(
                egui::RichText::new("This cannot be undone.")
                    .size(crate::theme::FONT_XS)
                    .color(crate::theme::active().error),
            );

            if let Some(index) = ui_kit::dialog_footer(
                ui,
                &[
                    ui_kit::DialogButton::Secondary("Cancel"),
                    ui_kit::DialogButton::Destructive("Delete"),
                ],
            ) {
                match index {
                    0 => cancel_clicked = true,
                    1 => do_delete = true,
                    _ => {}
                }
            }
        });

        if do_delete {
            if let Some(cwd) = self.active_pane_cwd() {
                self.workers.git_worker.enqueue_delete(&cwd, file_path);
            }
            self.delete_confirm_file = None;
        } else if resp.dismissed || cancel_clicked {
            self.delete_confirm_file = None;
        }
    }
}
