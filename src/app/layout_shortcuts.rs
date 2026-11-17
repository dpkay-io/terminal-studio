//! Shortcuts that arrange tabs, splits and side panels.
//!
//! A terminal hands every key to its shell unless it is a shortcut; a file
//! editor keeps its keys for typing. These chords mean nothing to a text
//! editor, so they work while one has the keyboard too — without that, ⇧⌘W
//! could not close an edited file, the one close that asks to save first.
//! Both input paths run them through [`App::apply_layout_shortcut`].

use super::input;
use super::pane::PaneContent;
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
    pub(super) fn apply_layout_shortcut(
        &mut self,
        action: AppAction,
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
                // Worked out only when a numbered key is pressed: it filters
                // every pane the way the session list does.
                if let Some(i) = action.tab_index() {
                    if let Some(&pane_id) = self.numbered_tab_targets().get(i) {
                        requests.activate_pane = Some(pane_id);
                    }
                }
            }
        }
    }

    /// Layout chords `raw_input_hook` kept away from a focused text box
    /// (Ctrl+Tab), run like the editor's other layout shortcuts.
    pub(super) fn run_held_layout_keys(&mut self, requests: &mut LayoutRequests) {
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
                self.apply_layout_shortcut(action, requests);
            }
        }
    }

    /// The notes box in the right panel; one id per window.
    pub(super) fn notes_text_id(&self) -> egui::Id {
        self.vp_id("notes_textedit")
    }

    /// The notes box has the keyboard.
    pub(super) fn notes_has_focus(&self, ctx: &egui::Context) -> bool {
        self.keyboard_focus(ctx) == Some(self.notes_text_id())
    }

    /// The widget with the keyboard as the user sees it: while focus is
    /// parked for a held Ctrl+Tab (`hold_extra_window_keys`), the one it
    /// was taken from.
    fn keyboard_focus(&self, ctx: &egui::Context) -> Option<egui::Id> {
        seen_focus(self.parked_focus, ctx.memory(|m| m.focused()))
    }

    /// The active pane is a file or note editor, which keeps every key for
    /// typing unless a layout shortcut claims it.
    pub(super) fn active_pane_is_text_editor(&self) -> bool {
        self.pane_state
            .active_pane_id
            .and_then(|pid| self.pane_state.panes.iter().find(|p| p.id == pid))
            .is_some_and(|p| {
                matches!(
                    p.content,
                    PaneContent::FileEditor(_) | PaneContent::NoteEditor(_)
                )
            })
    }

    /// Layout keys pressed while an editor or the notes box is in use apply
    /// only when the keyboard is in that text box (or nowhere) and no menu
    /// or popup is open.
    pub(super) fn text_box_layout_keys_apply(&self, ctx: &egui::Context) -> bool {
        !super::floating_layer_open(ctx)
            && text_box_keys_reach_layout(
                self.keyboard_focus(ctx),
                super::ui::active_text_box_id(ctx),
                self.notes_text_id(),
            )
    }

    /// Extra windows: eframe calls `raw_input_hook` only for the main window,
    /// so an extra window's frame starts with an empty held-key list and
    /// holds its own Ctrl+Tab back from a text box here, before any panel
    /// draws.
    ///
    /// By then egui (0.28) has already read the Tab as "move the keyboard to
    /// the next widget" (`Focus::begin_frame`), and the notes box, which has
    /// no `lock_focus`, would hand it on as it draws — as would the first
    /// widget drawn when nothing had it. So for this frame the keyboard is
    /// parked on an id no widget uses, and `restore_parked_focus` gives it
    /// back afterwards.
    pub(super) fn hold_extra_window_keys(&mut self, ctx: &egui::Context) {
        self.parked_focus = None;
        if self.current_window_id.is_none() {
            return;
        }
        self.raw_intercepted_keys.clear();
        if self.active_pane_is_text_editor() || self.notes_has_focus(ctx) {
            let mut held = std::mem::take(&mut self.raw_intercepted_keys);
            ctx.input_mut(|i| input::hold_ctrl_tab(&mut i.events, &mut held));
            if !held.is_empty() {
                self.parked_focus = Some(ctx.memory(|m| m.focused()));
                ctx.memory_mut(|m| m.request_focus(parked_focus_id()));
            }
            self.raw_intercepted_keys = held;
        }
    }

    /// End of an extra window's frame: give the keyboard back to the widget
    /// `hold_extra_window_keys` parked it away from, unless something took
    /// it meanwhile.
    pub(super) fn restore_parked_focus(&mut self, ctx: &egui::Context) {
        let Some(parked_from) = self.parked_focus.take() else {
            return;
        };
        let now = ctx.memory(|m| m.focused());
        ctx.memory_mut(|m| match focus_after_park(parked_from, now) {
            Some(FocusAfterPark::Give(id)) => m.request_focus(id),
            Some(FocusAfterPark::Clear) => m.surrender_focus(parked_focus_id()),
            None => {}
        });
    }
}

/// Where the keyboard is parked during a frame that held a Ctrl+Tab.
const PARKED_FOCUS_SLOT: &str = "ctrl_tab_parked_focus";

fn parked_focus_id() -> egui::Id {
    egui::Id::unique(PARKED_FOCUS_SLOT)
}

/// The focus the user sees: `parked` (the focus before parking) while the
/// keyboard is parked, else `focused`.
fn seen_focus(parked: Option<Option<egui::Id>>, focused: Option<egui::Id>) -> Option<egui::Id> {
    parked.unwrap_or(focused)
}

/// What to do with the keyboard at the end of a parked frame.
#[derive(Debug, PartialEq, Eq)]
enum FocusAfterPark {
    /// Give it back to this widget.
    Give(egui::Id),
    /// Nothing had it before: leave nothing with it.
    Clear,
}

/// `None` when something took the keyboard during the frame: that stays.
fn focus_after_park(
    parked_from: Option<egui::Id>,
    now: Option<egui::Id>,
) -> Option<FocusAfterPark> {
    if now != Some(parked_focus_id()) {
        return None;
    }
    Some(parked_from.map_or(FocusAfterPark::Clear, FocusAfterPark::Give))
}

/// Whether a layout key pressed while a file editor or the notes box is in
/// use belongs to the layout: only when the keyboard is in the active pane's
/// own text box, the notes box, or nowhere. Never while a rename field, a
/// filter box or any other input has it.
fn text_box_keys_reach_layout(
    focused: Option<egui::Id>,
    active_text_box: Option<egui::Id>,
    notes: egui::Id,
) -> bool {
    match focused {
        None => true,
        Some(id) => Some(id) == active_text_box || id == notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> egui::Id {
        egui::Id::unique(name)
    }

    #[test]
    fn parked_focus_is_seen_as_where_it_was() {
        assert_eq!(
            seen_focus(Some(Some(id("notes"))), Some(parked_focus_id())),
            Some(id("notes"))
        );
        assert_eq!(seen_focus(Some(None), Some(parked_focus_id())), None);
    }

    #[test]
    fn unparked_focus_is_seen_as_it_is() {
        assert_eq!(seen_focus(None, Some(id("filter"))), Some(id("filter")));
        assert_eq!(seen_focus(None, None), None);
    }

    #[test]
    fn parked_focus_goes_back_where_it_was() {
        assert_eq!(
            focus_after_park(Some(id("notes")), Some(parked_focus_id())),
            Some(FocusAfterPark::Give(id("notes")))
        );
        assert_eq!(
            focus_after_park(None, Some(parked_focus_id())),
            Some(FocusAfterPark::Clear)
        );
    }

    #[test]
    fn focus_taken_during_a_parked_frame_is_kept() {
        assert_eq!(
            focus_after_park(Some(id("notes")), Some(id("editor"))),
            None
        );
        assert_eq!(focus_after_park(Some(id("notes")), None), None);
    }

    #[test]
    fn layout_keys_reach_layout_when_nothing_has_the_keyboard() {
        assert!(text_box_keys_reach_layout(
            None,
            Some(id("editor")),
            id("notes")
        ));
    }

    #[test]
    fn layout_keys_reach_layout_from_the_active_text_box() {
        assert!(text_box_keys_reach_layout(
            Some(id("editor")),
            Some(id("editor")),
            id("notes")
        ));
    }

    #[test]
    fn layout_keys_reach_layout_from_the_notes_box() {
        assert!(text_box_keys_reach_layout(
            Some(id("notes")),
            Some(id("editor")),
            id("notes")
        ));
    }

    #[test]
    fn layout_keys_stay_in_a_rename_or_filter_field() {
        assert!(!text_box_keys_reach_layout(
            Some(id("rename")),
            Some(id("editor")),
            id("notes")
        ));
        assert!(!text_box_keys_reach_layout(
            Some(id("rename")),
            None,
            id("notes")
        ));
    }
}
