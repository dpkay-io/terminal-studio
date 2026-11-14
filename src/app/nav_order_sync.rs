//! Feeds the window's left nav order (`NavOrder`) from the panes, once per
//! window frame, whether or not the left panel is drawn: ⌘1–9 read the same
//! order the list shows, so it has to be current while the list is hidden.

use super::ui::left_panel::nav_order::{NavPane, SectionKey};
use super::ui::left_panel::sections;
use super::App;
use crate::editor_group::GroupId;

/// The section ⌘1–9 count through: the whole list while there is one split
/// (the list is drawn flat then), else the focused split's section.
fn numbered_section(has_splits: bool, focused: GroupId) -> Option<SectionKey> {
    has_splits.then_some(SectionKey::Group(focused))
}

impl App {
    /// One `NavPane` per pane, in `panes` order, into `out` (cleared first).
    pub(super) fn nav_panes_into(&self, out: &mut Vec<NavPane>) {
        out.clear();
        out.extend(self.pane_state.panes.iter().map(|p| {
            NavPane {
                id: p.id,
                section: self
                    .pane_state
                    .group_of(p.id)
                    .map_or(SectionKey::Other, SectionKey::Group),
                last_active_at: p.last_active_at,
            }
        }));
    }

    /// Bring this window's order up to date. Held when the pointer was over
    /// the list last frame; a hidden list never holds it.
    pub(super) fn refresh_nav_order(&mut self) {
        let hold = std::mem::take(&mut self.nav_list_hovered) && self.show_left_panel;
        let mut panes = std::mem::take(&mut self.nav_panes_buf);
        self.nav_panes_into(&mut panes);
        self.nav_order
            .refresh(&panes, self.settings.nav_stable_rows, hold);
        self.nav_panes_buf = panes;
    }

    /// Pane ids ⌘1–9 open, in left nav order: the focused split's rows that
    /// the workspace/label filter shows.
    pub(super) fn numbered_tab_targets(&self) -> Vec<u32> {
        let has_splits = !sections::section_labels(&self.pane_state.group_layout).is_empty();
        let section = numbered_section(has_splits, self.pane_state.focused_group_id);
        self.nav_order.numbered(section, |id| {
            self.pane_state
                .panes
                .iter()
                .find(|p| p.id == id)
                .is_some_and(|p| self.pane_passes_tab_filter(p))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_counts_the_whole_list_without_splits() {
        assert_eq!(numbered_section(false, 4), None);
    }

    #[test]
    fn numbered_counts_the_focused_split_with_splits() {
        assert_eq!(numbered_section(true, 4), Some(SectionKey::Group(4)));
    }
}
