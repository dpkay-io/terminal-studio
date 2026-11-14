//! Shortcuts that arrange tabs, splits and side panels.
//!
//! A terminal hands every key to its shell unless it is a shortcut; a file
//! editor keeps its keys for typing. These chords mean nothing to a text
//! editor, so they work while one has the keyboard too — without that, ⇧⌘W
//! could not close an edited file, the one close that asks to save first.
//! Both input paths run them through [`App::apply_layout_shortcut`].

use super::App;
use crate::pane_tree::SplitDir;
use crate::shortcuts::AppAction;

/// What a layout shortcut leaves for the rest of the frame to do.
#[derive(Default)]
pub(super) struct LayoutRequests {
    pub(super) split: Option<SplitDir>,
    pub(super) close_pane: bool,
    pub(super) activate_pane: Option<u32>,
}

impl App {
    /// Run one shortcut for which [`AppAction::arranges_layout`] holds.
    /// `numbered` are the pane ids ⌘1–9 open, top first
    /// ([`App::numbered_tab_targets`]).
    pub(super) fn apply_layout_shortcut(
        &mut self,
        action: AppAction,
        numbered: &[u32],
        requests: &mut LayoutRequests,
    ) {
        match action {
            AppAction::SplitHorizontal => requests.split = Some(SplitDir::Horizontal),
            AppAction::SplitVertical => requests.split = Some(SplitDir::Vertical),
            AppAction::CloseCurrentPane => requests.close_pane = true,
            AppAction::ToggleLeftSidebar
            | AppAction::ToggleRightSidebar
            | AppAction::ToggleTabBar => {
                self.toggle_view(action);
            }
            // Both cycle inside the focused split only. Cycling across every
            // visible pane used to yank focus into another split, which has no
            // way back on the keyboard now that each split owns its own order.
            AppAction::PreviousTab | AppAction::NextTab => {
                if let Some(cur) = self.pane_state.active_pane_id {
                    let allowed = self.tab_cycle_candidates();
                    let target = if action == AppAction::PreviousTab {
                        self.pane_state.prev_pane_in_focused_group(cur, &allowed)
                    } else {
                        self.pane_state.next_pane_in_focused_group(cur, &allowed)
                    };
                    if let Some(pid) = target {
                        requests.activate_pane = Some(pid);
                    }
                }
            }
            _ => {
                if let Some(&pane_id) = action.tab_index().and_then(|i| numbered.get(i)) {
                    requests.activate_pane = Some(pane_id);
                }
            }
        }
    }
}
