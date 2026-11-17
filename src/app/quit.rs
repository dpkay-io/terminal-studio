//! Quitting the app.
//!
//! Every quit closes the main window, which ends the app, from whichever
//! window it was asked in. The quit dialog is drawn in every open window,
//! and a window's frame sends viewport commands to itself, so a plain
//! `send_viewport_cmd` from an extra window's dialog closed only that
//! window (and dropped it from `windows.json`) while the app ran on.

use super::App;

/// Close the main window, which quits the app.
pub(super) fn close_main_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
}

impl App {
    /// Quit now, without asking (again): the main window's close goes
    /// through, and `on_exit` saves the session and every open window.
    pub(super) fn quit_app(&mut self, ctx: &egui::Context) {
        self.show_quit_confirm = false;
        self.quit_confirmed = true;
        close_main_window(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{ViewportCommand, ViewportId};

    /// Whether a frame of `viewport` that quits sent Close to each of `targets`.
    fn closes_sent(viewport: ViewportId, targets: &[ViewportId]) -> Vec<bool> {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput {
            viewport_id: viewport,
            ..Default::default()
        };
        input.viewports.insert(viewport, Default::default());
        let out = crate::egui_test_frames::run_pass(&ctx, input, close_main_window);
        targets
            .iter()
            .map(|id| {
                out.viewport_output
                    .get(id)
                    .is_some_and(|v| v.commands.contains(&ViewportCommand::Close))
            })
            .collect()
    }

    #[test]
    fn quit_from_an_extra_window_closes_the_main_window_only() {
        let extra = ViewportId::from_hash_of("extra window");
        assert_eq!(
            closes_sent(extra, &[ViewportId::ROOT, extra]),
            [true, false]
        );
    }

    #[test]
    fn quit_from_the_main_window_closes_it() {
        assert_eq!(closes_sent(ViewportId::ROOT, &[ViewportId::ROOT]), [true]);
    }
}
