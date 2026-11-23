//! Feeds the window's workspace list order (`NavOrder` by workspace id) once
//! per window frame, the same rule the session list follows: the top
//! `workspace_stable_rows` workspaces never move on their own, and one
//! switched to from below them moves to row 1. A switch stamps the
//! workspace's `last_activated` (`App::switch_group`), which is what the
//! order reads as an activation.
//!
//! A window with no saved order (a first launch, an older session file)
//! starts from the order the list used to sort by every frame, so nothing
//! jumps on the first launch after an update: workspaces with open tabs
//! first, most recently used first, then the rest by name.

use super::ui::left_panel::nav_order::{NavItem, NavOrder, SectionKey};
use super::App;
use crate::workspace::Workspace;
use std::cmp::Reverse;
use std::collections::HashSet;

/// The workspace list is one section.
const WORKSPACE_SECTION: SectionKey = SectionKey::Other;

/// What the order needs to know about one workspace.
fn workspace_item(w: &Workspace) -> NavItem<u64> {
    NavItem {
        id: w.id,
        section: WORKSPACE_SECTION,
        last_active_at: w.last_activated,
    }
}

/// The list's old every-frame sort: workspaces with open tabs (`open`) first,
/// most recently switched to first; then the rest by name, ignoring case.
/// Ties fall back to the id, so the result never depends on store order.
fn initial_order(workspaces: &[Workspace], open: &HashSet<u64>) -> Vec<u64> {
    let mut sorted: Vec<&Workspace> = workspaces.iter().collect();
    sorted.sort_by(|a, b| {
        let (a_open, b_open) = (open.contains(&a.id), open.contains(&b.id));
        b_open
            .cmp(&a_open)
            .then_with(|| match a_open {
                true => Reverse(a.last_activated).cmp(&Reverse(b.last_activated)),
                false => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            })
            .then(a.id.cmp(&b.id))
    });
    sorted.iter().map(|w| w.id).collect()
}

/// A saved order to start from, or an empty one when nothing was saved, so
/// the first refresh seeds it from [`initial_order`] with the open tabs in
/// place.
pub(super) fn restored_workspace_order(saved: &[u64], items: &[NavItem<u64>]) -> NavOrder<u64> {
    if saved.is_empty() {
        NavOrder::default()
    } else {
        NavOrder::restore(saved, items)
    }
}

impl App {
    /// One item per workspace, in store order, into `out` (cleared first).
    pub(super) fn workspace_items_into(&self, out: &mut Vec<NavItem<u64>>) {
        out.clear();
        out.extend(self.workspace_store.workspaces.iter().map(workspace_item));
    }

    /// Bring this window's workspace order up to date. Held when the pointer
    /// was over the workspace list last frame; a hidden list never holds it.
    pub(super) fn refresh_workspace_order(&mut self) {
        let list_shown = self.show_left_panel && !self.workspace_panel_collapsed;
        let hold = std::mem::take(&mut self.workspace_list_hovered) && list_shown;
        let mut items = std::mem::take(&mut self.workspace_items_buf);
        self.workspace_items_into(&mut items);
        if self.workspace_order.order().is_empty() && !items.is_empty() {
            let seed = initial_order(&self.workspace_store.workspaces, &self.open_workspace_ids());
            self.workspace_order = NavOrder::restore(&seed, &items);
        } else {
            self.workspace_order
                .refresh(&items, self.settings.workspace_stable_rows, hold);
        }
        self.workspace_items_buf = items;
    }

    /// Workspaces that have a tab open (by `pane_group`, as the cards mark).
    pub(super) fn open_workspace_ids(&self) -> HashSet<u64> {
        self.pane_state
            .panes
            .iter()
            .filter_map(|p| {
                Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ws(id: u64, name: &str, last_activated: u64) -> Workspace {
        Workspace {
            id,
            name: name.to_string(),
            path: PathBuf::from(format!("/w/{id}")),
            color: [0, 0, 0],
            host_window_id: None,
            last_activated,
        }
    }

    fn items(list: &[Workspace]) -> Vec<NavItem<u64>> {
        list.iter().map(workspace_item).collect()
    }

    #[test]
    fn first_order_matches_the_old_sort() {
        let list = vec![
            ws(1, "zeta", 0),
            ws(2, "Alpha", 0),
            ws(3, "open-old", 10),
            ws(4, "open-new", 20),
            ws(5, "beta", 99),
        ];
        let open: HashSet<u64> = [3, 4].into_iter().collect();
        assert_eq!(initial_order(&list, &open), vec![4, 3, 2, 5, 1]);
    }

    #[test]
    fn first_order_ties_never_depend_on_store_order() {
        let a = vec![ws(1, "same", 0), ws(2, "Same", 0)];
        let b = vec![ws(2, "Same", 0), ws(1, "same", 0)];
        let none = HashSet::new();
        assert_eq!(initial_order(&a, &none), initial_order(&b, &none));
    }

    #[test]
    fn nothing_saved_starts_empty_so_the_first_refresh_seeds_it() {
        let list = vec![ws(1, "a", 0)];
        assert!(restored_workspace_order(&[], &items(&list))
            .order()
            .is_empty());
    }

    #[test]
    fn a_saved_order_is_kept_and_new_workspaces_follow_it() {
        let list = vec![ws(1, "a", 5), ws(2, "b", 0), ws(3, "c", 9)];
        let order = restored_workspace_order(&[2, 1, 77], &items(&list));
        assert_eq!(order.order(), &[2, 1, 3]);
    }

    /// The rule the session list follows, on workspaces: the top rows stay,
    /// a switch from below them goes to row 1.
    #[test]
    fn top_rows_stay_and_a_switch_from_below_moves_to_the_top() {
        let stable = 2;
        let mut list = vec![ws(1, "a", 0), ws(2, "b", 0), ws(3, "c", 0), ws(4, "d", 0)];
        let mut order = NavOrder::restore(&[1, 2, 3, 4], &items(&list));

        list[1].last_activated = 10; // row 2: inside the stable rows
        order.refresh(&items(&list), stable, false);
        assert_eq!(order.order(), &[1, 2, 3, 4]);

        list[3].last_activated = 20; // row 4: below them
        order.refresh(&items(&list), stable, false);
        assert_eq!(order.order(), &[4, 1, 2, 3]);
    }

    #[test]
    fn a_switch_waits_while_the_pointer_is_over_the_list() {
        let mut list = vec![ws(1, "a", 0), ws(2, "b", 0), ws(3, "c", 0)];
        let mut order = NavOrder::restore(&[1, 2, 3], &items(&list));
        list[2].last_activated = 5;
        order.refresh(&items(&list), 1, true);
        assert_eq!(order.order(), &[1, 2, 3]);
        assert!(order.held());
        order.refresh(&items(&list), 1, false);
        assert_eq!(order.order(), &[3, 1, 2]);
    }

    #[test]
    fn new_and_deleted_workspaces() {
        let mut list = vec![ws(1, "a", 0), ws(2, "b", 0)];
        let mut order = NavOrder::restore(&[1, 2], &items(&list));
        list.push(ws(3, "new", 0));
        order.refresh(&items(&list), 5, false);
        assert_eq!(order.order(), &[1, 2, 3], "room left: takes the next row");
        list.remove(0);
        order.refresh(&items(&list), 5, false);
        assert_eq!(order.order(), &[2, 3]);
    }
}
