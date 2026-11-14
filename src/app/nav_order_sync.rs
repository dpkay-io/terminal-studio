//! Feeds the window's left nav order (`NavOrder`) from the panes, once per
//! window frame, whether or not the left panel is drawn: ⌘1–9 read the same
//! order the list shows, so it has to be current while the list is hidden.

use super::ui::left_panel::nav_order::{NavPane, SectionKey};
use super::App;

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
}
