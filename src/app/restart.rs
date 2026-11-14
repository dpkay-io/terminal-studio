//! Restarting into an installed update.
//!
//! The session is saved first, and only from the main window's frame: during
//! an extra window's frame `App` holds that window's view and saves wait
//! for it to end (`DeferredSaves`), but a restart exits the process at once.
//! So a click on the Restart pill in any window only asks for it, and the
//! main window's next frame saves and restarts.

use super::App;

/// Whether to restart now: an installed update (`update_ready`) restarts by
/// itself once (`attempted` after the first try), and a click on the
/// Restart pill (`requested`) always does, so the user can retry a restart
/// that failed.
fn restart_due(update_ready: bool, attempted: bool, requested: bool) -> bool {
    requested || (update_ready && !attempted)
}

impl App {
    /// Ask for a restart from any window (the Restart pill).
    pub(super) fn request_restart(&mut self, ctx: &egui::Context) {
        self.restart_requested = true;
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    /// Main window's frame only: save and restart if an update or a click
    /// asks for it.
    pub(super) fn restart_if_due(&mut self) {
        // Read the updater's state only until the first try (it is copied
        // out under a lock).
        let update_ready = !self.restart_attempted
            && self.workers.update_checker.as_ref().is_some_and(|uc| {
                matches!(
                    uc.state().status,
                    crate::updater::UpdateStatus::RestartRequired
                )
            });
        let requested = std::mem::take(&mut self.restart_requested);
        if !restart_due(update_ready, self.restart_attempted, requested) {
            return;
        }
        self.restart_attempted = true;
        self.save_session();
        crate::updater::restart_app();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installed_update_restarts_once() {
        assert!(restart_due(true, false, false));
        assert!(!restart_due(true, true, false));
    }

    #[test]
    fn a_restart_click_restarts_even_after_a_failed_try() {
        assert!(restart_due(false, false, true));
        assert!(restart_due(true, true, true));
    }

    #[test]
    fn nothing_restarts_without_an_update_or_a_click() {
        assert!(!restart_due(false, false, false));
        assert!(!restart_due(false, true, false));
    }
}
