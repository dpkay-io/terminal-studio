use super::super::super::pane::{PaneContent, PaneEntry};
use super::super::super::title::shell_escape_arg;
use super::super::super::workspace_ui::WorkspaceEditDialog;
use super::super::super::App;
use super::{SessionListActions, WorkspaceSectionActions};
use crate::pane_tree::{PaneNode, RemoveResult};
use crate::pty::default_shell;
use crate::pty::foreground::ForegroundProcess;
impl App {
    /// Process all deferred actions collected during left-panel rendering:
    /// open/edit/new-window workspace, quit pane, sidebar click, spawn, duplicate.
    pub(in crate::app) fn process_left_panel_actions(
        &mut self,
        ctx: &egui::Context,
        sess_actions: SessionListActions,
        ws_actions: WorkspaceSectionActions,
        active_fg: Option<ForegroundProcess>,
    ) {
        self.process_workspace_actions(ctx, &ws_actions);
        self.process_quit_pane(sess_actions.quit_pane_id);
        self.process_sidebar_click(sess_actions.clicked_sidebar_pane_id);
        self.process_spawn_session(&sess_actions, &active_fg);
        self.process_duplicate_session(sess_actions.duplicate_session, &active_fg);
    }

    /// Handle open-workspace, edit-workspace, and new-window-workspace actions.
    fn process_workspace_actions(
        &mut self,
        ctx: &egui::Context,
        actions: &WorkspaceSectionActions,
    ) {
        if let Some(ws_id) = actions.open_workspace_id {
            let (cols, rows) = self
                .pane_state
                .panes
                .first()
                .map(|p| p.last_size)
                .unwrap_or((80, 24));
            let group = if ws_id == u64::MAX { None } else { Some(ws_id) };
            self.switch_group(group, cols, rows);
        }

        if let Some(ws_id) = actions.edit_workspace_id {
            if let Some(ws) = self
                .workspace_store
                .workspaces
                .iter()
                .find(|w| w.id == ws_id)
            {
                self.workspace_edit_dialog =
                    Some(WorkspaceEditDialog::new(ws.id, ws.name.clone(), ws.color));
            }
        }

        if let Some(ws_id) = actions.new_window_workspace_id {
            self.open_workspace_in_new_window(ctx, ws_id);
        }

        if let Some(vp) = actions.focus_extra_window_viewport {
            ctx.send_viewport_cmd_to(vp, egui::ViewportCommand::Focus);
        }

        if let Some(ws_id) = actions.reclaim_workspace_id {
            self.close_extra_window_for_workspace(ws_id);
            let (cols, rows) = self
                .pane_state
                .panes
                .first()
                .map(|p| p.last_size)
                .unwrap_or((80, 24));
            self.switch_group(Some(ws_id), cols, rows);
        }
    }

    /// Handle closing a pane from the sidebar quit button.
    fn process_quit_pane(&mut self, quit_pane_id: Option<u32>) {
        let Some(qpid) = quit_pane_id else { return };
        let Some(pos) = self.pane_state.panes.iter().position(|p| p.id == qpid) else {
            return;
        };

        let killed_sid = match &self.pane_state.panes[pos].content {
            PaneContent::Terminal(sid) => Some(*sid),
            _ => None,
        };
        self.pane_state.panes.remove(pos);
        if self.pane_state.active_pane_id == Some(qpid) {
            self.pane_state.active_pane_id = self.pane_state.panes.last().map(|p| p.id);
        }
        // Clean up split tree
        if self.pane_state.pane_trees.remove(&qpid).is_none() {
            let root_pid_opt = self
                .pane_state
                .pane_trees
                .iter()
                .find(|(_, tree)| tree.leaf_ids().contains(&qpid))
                .map(|(&rpid, _)| rpid);
            if let Some(root_pid) = root_pid_opt {
                let result = if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                    tree.remove_pane(qpid)
                } else {
                    RemoveResult::NotFound
                };
                if let RemoveResult::CollapseToSibling(replacement) = result {
                    if let Some(tree) = self.pane_state.pane_trees.get_mut(&root_pid) {
                        *tree = replacement;
                    }
                }
            }
        }
        if let Some(sid) = killed_sid {
            self.session_state.remove(sid);
            if self.session_state.active_id == Some(sid) {
                self.session_state.active_id = self.session_state.sessions.first().map(|e| e.id);
                self.update_is_active_flags();
            }
        }
        // Ensure active session is shown in a pane
        if self.pane_state.panes.is_empty() {
            if let Some(new_sid) = self.session_state.active_id {
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                self.pane_state.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::Terminal(new_sid),
                    manual_width: None,
                    last_size: (0, 0),
                });
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (0, 0),
                    },
                );
                self.pane_state.active_pane_id = Some(pane_id);
            }
        }
        self.save_session();
    }

    /// Handle clicking a session row in the sidebar to activate or switch windows.
    ///
    /// Routing: find which window "owns" the pane's workspace.
    ///  - If an extra window is dedicated to that workspace → switch to it.
    ///  - Otherwise the main window is the home → switch to it.
    ///  - If the owning window is already the current window → activate locally.
    fn process_sidebar_click(&mut self, clicked_sidebar_pane_id: Option<u32>) {
        let Some(qpid) = clicked_sidebar_pane_id else {
            return;
        };
        let group_opt = self
            .pane_state
            .panes
            .iter()
            .find(|p| p.id == qpid)
            .map(|p| Self::pane_group(&self.session_state.sessions, &self.workspace_store, p));
        let Some(group) = group_opt else { return };

        // Find the extra window that owns this workspace (if any).
        let owner_ew = group.and_then(|ws_id| {
            self.extra_windows
                .iter()
                .enumerate()
                .find(|(_, ew)| ew.workspace_id == ws_id)
                .map(|(idx, ew)| (idx, ew.viewport_id, ew.id.clone()))
        });

        use super::super::super::multi_window::PendingWindowFocus;
        let is_main_window = self.current_window_id.is_none();

        match owner_ew {
            Some((idx, viewport_id, ref ew_id))
                if self.current_window_id.as_ref() != Some(ew_id) =>
            {
                // Pane lives in a different extra window → switch to it.
                self.pending_window_focus = Some(PendingWindowFocus {
                    target_viewport_id: viewport_id,
                    target_window_idx: Some(idx),
                    pane_id: qpid,
                    group,
                });
            }
            None if !is_main_window => {
                // No extra window owns this workspace → it lives in the main
                // window, but we're in an extra window → switch to main.
                self.pending_window_focus = Some(PendingWindowFocus {
                    target_viewport_id: egui::ViewportId::ROOT,
                    target_window_idx: None,
                    pane_id: qpid,
                    group,
                });
            }
            _ => {
                // Pane is in the current window → activate locally.
                self.active_group = group;
                self.activate_pane(qpid);
                self.last_pane_per_group.insert(group, qpid);
            }
        }
    }

    /// Handle spawning a new session (from "+ New" or "Open Folder").
    fn process_spawn_session(
        &mut self,
        sess_actions: &SessionListActions,
        _active_fg: &Option<ForegroundProcess>,
    ) {
        let spawn_with_cwd = if let Some(ref new_shell) = sess_actions.spawn_new_session {
            let cwd = self.active_pane_cwd().or_else(|| {
                self.active_group.and_then(|gid| {
                    self.workspace_store
                        .workspaces
                        .iter()
                        .find(|w| w.id == gid)
                        .map(|w| w.path.clone())
                })
            });
            Some((new_shell.clone(), cwd))
        } else {
            sess_actions
                .spawn_new_session_cwd
                .as_ref()
                .map(|(shell, path)| (shell.clone(), Some(path.clone())))
        };

        let Some((new_shell, cwd)) = spawn_with_cwd else {
            return;
        };

        let (cols, rows) = self
            .pane_state
            .panes
            .iter()
            .find(|p| Some(p.id) == self.pane_state.active_pane_id)
            .map(|p| p.last_size)
            .unwrap_or_else(|| {
                self.pane_state
                    .panes
                    .first()
                    .map(|p| p.last_size)
                    .unwrap_or((80, 24))
            });
        if let Some(new_id) = self.spawn_session(&new_shell, cols, rows, cwd) {
            self.session_state.active_id = Some(new_id);
            if !self
                .pane_state
                .panes
                .iter()
                .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == new_id))
            {
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                self.pane_state.panes.push(PaneEntry {
                    id: pane_id,
                    content: PaneContent::Terminal(new_id),
                    manual_width: None,
                    last_size: (cols, rows),
                });
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (cols, rows),
                    },
                );
                self.activate_pane(pane_id);
            }
        }
    }

    /// Handle duplicating the active session.
    fn process_duplicate_session(
        &mut self,
        duplicate: bool,
        active_fg: &Option<ForegroundProcess>,
    ) {
        if !duplicate {
            return;
        }

        let dup_shell = self
            .session_state
            .sessions
            .iter()
            .find(|e| Some(e.id) == self.session_state.active_id)
            .map(|e| e.shell.clone())
            .unwrap_or_else(default_shell);
        let cwd = self.active_cwd().or_else(|| {
            self.active_group.and_then(|gid| {
                self.workspace_store
                    .workspaces
                    .iter()
                    .find(|w| w.id == gid)
                    .map(|w| w.path.clone())
            })
        });
        let (cols, rows) = self
            .pane_state
            .panes
            .iter()
            .find(|p| Some(p.id) == self.pane_state.active_pane_id)
            .map(|p| p.last_size)
            .unwrap_or_else(|| {
                self.pane_state
                    .panes
                    .first()
                    .map(|p| p.last_size)
                    .unwrap_or((80, 24))
            });
        // Build the command string to replay in the new session
        let cmd_to_run: Option<String> = active_fg.as_ref().map(|fp| {
            let parts: Vec<String> = fp.cmdline.iter().map(|a| shell_escape_arg(a)).collect();
            let joined = parts.join(" ");
            #[cfg(target_os = "windows")]
            {
                format!("& {}", joined)
            }
            #[cfg(not(target_os = "windows"))]
            {
                joined
            }
        });
        if let Some(new_id) = self.spawn_session(&dup_shell, cols, rows, cwd) {
            self.session_state.active_id = Some(new_id);
            if !self
                .pane_state
                .panes
                .iter()
                .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == new_id))
            {
                let insert_at = self
                    .pane_state
                    .panes
                    .iter()
                    .position(|p| Some(p.id) == self.pane_state.active_pane_id)
                    .map(|i| i + 1)
                    .unwrap_or(self.pane_state.panes.len());
                let pane_id = self.pane_state.next_pane_id;
                self.pane_state.next_pane_id += 1;
                self.pane_state.panes.insert(
                    insert_at,
                    PaneEntry {
                        id: pane_id,
                        content: PaneContent::Terminal(new_id),
                        manual_width: None,
                        last_size: (cols, rows),
                    },
                );
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (cols, rows),
                    },
                );
                self.activate_pane(pane_id);
            }
            if let Some(cmd) = cmd_to_run {
                if let Some(entry) = self.session_state.find_mut(new_id) {
                    entry.pending_command = Some(cmd);
                }
            }
        }
    }
}
