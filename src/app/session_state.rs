use std::collections::HashSet;

use super::pane::SessionEntry;

/// Holds session-related state extracted from `App`.
///
/// Contains the list of terminal sessions, the currently active session ID,
/// and the set of sessions that have not yet received their first OSC 7.
pub(super) struct SessionState {
    pub(super) sessions: Vec<SessionEntry>,
    pub(super) active_id: Option<u32>,
    /// IDs of sessions that have not yet received their first OSC 7 (CWD not set).
    pub(super) uninit_sessions: HashSet<u32>,
}

impl SessionState {
    pub(super) fn new() -> Self {
        SessionState {
            sessions: Vec::new(),
            active_id: None,
            uninit_sessions: HashSet::new(),
        }
    }

    pub(super) fn find(&self, id: u32) -> Option<&SessionEntry> {
        self.sessions.iter().find(|e| e.id == id)
    }

    pub(super) fn find_mut(&mut self, id: u32) -> Option<&mut SessionEntry> {
        self.sessions.iter_mut().find(|e| e.id == id)
    }

    pub(super) fn remove(&mut self, id: u32) -> Option<SessionEntry> {
        let pos = self.sessions.iter().position(|e| e.id == id)?;
        self.uninit_sessions.remove(&id);
        let entry = self.sessions.remove(pos);
        entry
            .alive
            .store(false, std::sync::atomic::Ordering::Release);
        Some(entry)
    }

    #[allow(dead_code)]
    pub(super) fn set_active(&mut self, id: Option<u32>) {
        self.active_id = id;
    }

    #[allow(dead_code)]
    pub(super) fn active(&self) -> Option<&SessionEntry> {
        self.active_id.and_then(|id| self.find(id))
    }

    #[allow(dead_code)]
    pub(super) fn active_mut(&mut self) -> Option<&mut SessionEntry> {
        let id = self.active_id?;
        self.find_mut(id)
    }

    #[allow(dead_code)]
    pub(super) fn mark_initialized(&mut self, id: u32) {
        self.uninit_sessions.remove(&id);
    }
}
