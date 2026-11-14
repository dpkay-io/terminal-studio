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
            AppAction::SwitchToLastSession => {
                let allowed = self.tab_cycle_candidates();
                requests.activate_pane = self
                    .pane_state
                    .previous_active_pane(self.pane_state.active_pane_id, &allowed);
            }
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

    /// Layout chords `raw_input_hook` kept away from a focused text box
    /// (Ctrl+Tab), run like the editor's other layout shortcuts.
    pub(super) fn run_held_layout_keys(&mut self, numbered: &[u32], requests: &mut LayoutRequests) {
        for event in std::mem::take(&mut self.raw_intercepted_keys) {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            if let Some(action) = self
                .shortcut_registry
                .match_event(&key, &modifiers)
                .filter(|action| action.arranges_layout())
            {
                self.apply_layout_shortcut(action, numbered, requests);
            }
        }
    }

    /// The notes box has the keyboard.
    pub(super) fn notes_has_focus(&self, ctx: &egui::Context) -> bool {
        let notes_id = self.vp_id("notes_textedit");
        ctx.memory(|m| m.focused() == Some(notes_id))
    }
}
