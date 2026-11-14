//! The view's show/hide actions: sidebar, explorer, tab bar, switcher,
//! recently closed sessions, settings and keyboard shortcuts.
//!
//! The titlebar buttons, keyboard shortcuts, the command palette, the macOS
//! menu bar and the Settings checkbox all run these through
//! [`App::toggle_view`], so each one opens and closes the same way wherever
//! it is triggered from.

use super::App;
use crate::shortcuts::AppAction;

/// Every action [`App::toggle_view`] handles: the titlebar's, in titlebar
/// order, then the tab bar (which has no titlebar button).
pub(super) const VIEW_TOGGLES: [AppAction; 7] = [
    AppAction::ToggleLeftSidebar,
    AppAction::OpenQuickSwitcher,
    AppAction::ReopenClosedSession,
    AppAction::ToggleShortcutHelp,
    AppAction::ToggleRightSidebar,
    AppAction::OpenSettings,
    AppAction::ToggleTabBar,
];

impl App {
    /// Whether the panel or overlay behind `action` is showing now, or None
    /// when `action` is not one of [`VIEW_TOGGLES`]. Read by the macOS menu
    /// bar, which shows each one's state.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn view_is_open(&self, action: AppAction) -> Option<bool> {
        Some(match action {
            AppAction::ToggleLeftSidebar => self.show_left_panel,
            AppAction::ToggleRightSidebar => self.show_right_panel,
            AppAction::ToggleTabBar => self.settings.show_tab_bar,
            AppAction::OpenQuickSwitcher => self.show_quick_switcher,
            AppAction::ReopenClosedSession => self.show_closed_sessions,
            AppAction::ToggleShortcutHelp => self.show_shortcut_help,
            AppAction::OpenSettings => self.show_settings,
            _ => return None,
        })
    }

    /// Shows the panel or overlay behind `action` if hidden, hides it if
    /// shown. Returns false, doing nothing, for any other action.
    pub(super) fn toggle_view(&mut self, action: AppAction) -> bool {
        match action {
            AppAction::ToggleLeftSidebar => self.show_left_panel = !self.show_left_panel,
            AppAction::ToggleRightSidebar => self.show_right_panel = !self.show_right_panel,
            AppAction::ToggleTabBar => {
                self.settings.show_tab_bar = !self.settings.show_tab_bar;
                self.settings.save();
            }
            AppAction::OpenQuickSwitcher => {
                self.show_quick_switcher = !self.show_quick_switcher;
                if !self.show_quick_switcher {
                    self.quick_switcher_query.clear();
                    self.quick_switcher_selected_ws = None;
                    self.quick_switcher_search_active = false;
                }
            }
            AppAction::ReopenClosedSession => {
                self.show_closed_sessions = !self.show_closed_sessions;
                if !self.show_closed_sessions {
                    self.closed_sessions_query.clear();
                    self.closed_sessions_selected = 0;
                    self.closed_sessions_cache = None;
                }
            }
            AppAction::ToggleShortcutHelp => self.show_shortcut_help = !self.show_shortcut_help,
            AppAction::OpenSettings => {
                // Settings opens over the shortcut sheet's spot; never both.
                self.show_shortcut_help = false;
                self.show_settings = !self.show_settings;
            }
            _ => return false,
        }
        log::debug!("view toggled: {action:?}");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_toggles_are_distinct() {
        for (i, action) in VIEW_TOGGLES.iter().enumerate() {
            assert!(!VIEW_TOGGLES[i + 1..].contains(action), "{action:?} twice");
        }
    }
}
