//! Starting restored sessions.
//!
//! A session restored from the last run waits as a
//! [`PaneContent::DeferredTerminal`] until it is on screen, so a long list of
//! saved sessions doesn't start dozens of shells at launch. "On screen" means
//! the tab a split shows, in every split — not only the focused one — or a
//! split would sit blank until clicked.

use super::pane::PaneContent;
use super::state;
use super::App;

impl App {
    /// Start every restored session a split is showing in this window. The
    /// focused one becomes the active session; the rest start beside it.
    pub(super) fn start_shown_restored_sessions(&mut self, ctx: &egui::Context) {
        let focused = self.pane_state.active_pane_id;
        let shown: Vec<u32> = self
            .pane_state
            .shown_pane_ids()
            .into_iter()
            .filter(|&pid| {
                self.pane_state.find(pid).is_some_and(|pane| {
                    matches!(pane.content, PaneContent::DeferredTerminal { .. })
                        && self.pane_passes_tab_filter(pane)
                })
            })
            .collect();
        for pid in shown {
            self.start_restored_session(pid, focused == Some(pid));
            ctx.request_repaint();
        }
    }

    /// Start the shell of one restored session with its saved output, name
    /// and pending command. If its shell can't start, a plain one takes its
    /// place; if that fails too the tab goes.
    fn start_restored_session(&mut self, pid: u32, focused: bool) {
        let Some(pane_idx) = self.pane_state.panes.iter().position(|p| p.id == pid) else {
            return;
        };
        let PaneContent::DeferredTerminal {
            cwd,
            pending_command,
            saved_title,
            title_pinned,
            scrollback_file,
        } = &self.pane_state.panes[pane_idx].content
        else {
            return;
        };
        let cwd = cwd.clone();
        let pending_command = pending_command.clone();
        let saved_title = saved_title.clone();
        let title_pinned = *title_pinned;
        let scrollback = scrollback_file
            .as_deref()
            .and_then(state::read_active_scrollback);
        let shell = self.configured_shell();
        let (cols, rows) = state::RESTORED_TERMINAL_SIZE;

        let sid = match self.spawn_session_with_scrollback(
            &shell,
            cols,
            rows,
            cwd.clone(),
            scrollback.as_deref(),
        ) {
            Some(sid) => {
                if let Some(entry) = self.session_state.find_mut(sid) {
                    if let Some(cmd) = pending_command {
                        entry.pending_command = Some(cmd);
                    }
                    if let Some(t) = saved_title.filter(|t| !t.is_empty()) {
                        let session = entry.session.read();
                        if title_pinned {
                            session.pin_title(Some(t));
                        } else {
                            session.set_title(t);
                        }
                    }
                }
                Some(sid)
            }
            None => {
                log::error!(
                    "failed to start restored session (pane {pid}), starting a plain shell instead"
                );
                self.spawn_session(&shell, cols, rows, cwd)
            }
        };

        match sid {
            Some(sid) => {
                self.pane_state.panes[pane_idx].content = PaneContent::Terminal(sid);
                self.pane_state.panes[pane_idx].last_size = (0, 0);
                if focused {
                    self.session_state.active_id = Some(sid);
                }
                self.update_is_active_flags();
            }
            None => {
                log::error!("plain shell also failed to start, removing pane {pid}");
                self.pane_state.panes.remove(pane_idx);
                self.pane_state.pane_trees.remove(&pid);
                if self.pane_state.active_pane_id == Some(pid) {
                    self.pane_state.active_pane_id = self.pane_state.panes.first().map(|p| p.id);
                }
            }
        }
    }
}
