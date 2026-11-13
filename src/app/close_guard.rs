//! Asks before closing a tab would throw away edits nobody saved.
//!
//! Every way of closing one tab goes through [`App::request_close_tab`]: the
//! ✖ on a split's header, ⇧⌘W, a pane's own menu, and the session list's ✕,
//! middle click and menu. A tab without unsaved edits closes at once; one
//! with them waits for the "Save changes?" dialog (see `dialogs.rs`).
//! Closing several tabs together is confirmed by the close-all dialog, which
//! counts the unsaved ones.

use super::pane::PaneContent;
use super::App;

/// Which close the user asked for, run once they confirm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TabClose {
    /// The ✖ on a split's header, ⇧⌘W, or a pane's own menu.
    Pane,
    /// The session list's ✕, middle click or menu Close.
    Sidebar,
}

impl App {
    /// Whether closing this tab would lose edits that are in no file.
    pub(super) fn has_unsaved_changes(&self, pane_id: u32) -> bool {
        self.pane_state.panes.iter().any(|p| {
            p.id == pane_id
                && matches!(&p.content, PaneContent::FileEditor(ed) if ed.has_unsaved_changes())
        })
    }

    /// How many of these tabs hold unsaved edits.
    pub(super) fn unsaved_count(&self, pane_ids: &[u32]) -> usize {
        pane_ids
            .iter()
            .filter(|&&pid| self.has_unsaved_changes(pid))
            .count()
    }

    /// Close one tab, first asking to save it when it has unsaved edits.
    pub(super) fn request_close_tab(&mut self, pane_id: u32, via: TabClose) {
        if self.has_unsaved_changes(pane_id) {
            log::debug!("close: asking before closing unsaved tab (pane {pane_id})");
            self.pending_tab_close = Some((pane_id, via));
            return;
        }
        self.close_tab_now(pane_id, via);
    }

    /// Close one tab without asking: it has nothing unsaved, or the user chose.
    pub(super) fn close_tab_now(&mut self, pane_id: u32, via: TabClose) {
        match via {
            TabClose::Pane => self.close_pane_full(pane_id),
            TabClose::Sidebar => self.process_quit_pane(Some(pane_id)),
        }
    }
}
