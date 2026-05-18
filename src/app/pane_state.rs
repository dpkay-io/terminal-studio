use std::collections::HashMap;

use crate::pane_tree::PaneNode;

use super::pane::PaneEntry;

/// Holds pane-related state extracted from `App`.
///
/// Contains the list of panes, the active pane ID, the next pane ID counter,
/// the pane split trees, and the next split ID counter.
pub(super) struct PaneState {
    pub(super) panes: Vec<PaneEntry>,
    pub(super) active_pane_id: Option<u32>,
    pub(super) next_pane_id: u32,
    /// Maps root_pane_id → layout tree for that tab.
    pub(super) pane_trees: HashMap<u32, PaneNode>,
    /// Monotonically-increasing counter for generating unique split node IDs.
    pub(super) next_split_id: u32,
}

impl PaneState {
    pub(super) fn new() -> Self {
        PaneState {
            panes: Vec::new(),
            active_pane_id: None,
            next_pane_id: 0,
            pane_trees: HashMap::new(),
            next_split_id: 1,
        }
    }

    pub(super) fn find(&self, id: u32) -> Option<&PaneEntry> {
        self.panes.iter().find(|p| p.id == id)
    }

    pub(super) fn find_mut(&mut self, id: u32) -> Option<&mut PaneEntry> {
        self.panes.iter_mut().find(|p| p.id == id)
    }

    #[allow(dead_code)]
    pub(super) fn remove(&mut self, id: u32) -> Option<PaneEntry> {
        let pos = self.panes.iter().position(|p| p.id == id)?;
        Some(self.panes.remove(pos))
    }

    pub(super) fn root_of(&self, pane_id: u32) -> Option<u32> {
        if self.pane_trees.contains_key(&pane_id) {
            return Some(pane_id);
        }
        self.pane_trees
            .iter()
            .find(|(_, tree)| tree.leaf_ids().contains(&pane_id))
            .map(|(&rpid, _)| rpid)
    }

    #[allow(dead_code)]
    pub(super) fn active(&self) -> Option<&PaneEntry> {
        self.active_pane_id.and_then(|id| self.find(id))
    }

    #[allow(dead_code)]
    pub(super) fn active_mut(&mut self) -> Option<&mut PaneEntry> {
        let id = self.active_pane_id?;
        self.find_mut(id)
    }
}
