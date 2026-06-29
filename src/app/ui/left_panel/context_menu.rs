use super::super::super::pane::{PaneContent, PaneEntry};
use super::super::super::title::shell_escape_arg;
use super::super::super::workspace_ui::{OpenFolderDialog, WorkspaceEditDialog};
use super::super::super::{App, CloseAllTarget};
use super::{SessionListActions, WorkspaceSectionActions};
use crate::app::claude_session::is_claude_process;
use crate::pane_tree::{PaneNode, RemoveResult};
use crate::pty::default_shell;
use crate::pty::foreground::ForegroundProcess;
impl App {
    /// Process all deferred actions collected during left-panel rendering:
    /// open/edit/new-window workspace, quit pane, sidebar click, spawn, duplicate.
    pub(in crate::app) fn process_left_panel_actions(
        &mut self,
        ctx: &egui::Context,
        mut sess_actions: SessionListActions,
        ws_actions: WorkspaceSectionActions,
        active_fg: Option<ForegroundProcess>,
    ) {
        self.process_workspace_actions(ctx, &ws_actions);
        self.process_quit_pane(sess_actions.quit_pane_id);
        self.process_label_toggle(sess_actions.toggle_label);
        if let Some(pane_id) = sess_actions.show_new_label_for_pane {
            self.show_new_label_dialog = Some(pane_id);
        }
        self.process_sidebar_click(sess_actions.clicked_sidebar_pane_id);
        // Consume async folder picker result if available
        let async_folder: Option<std::path::PathBuf> = ctx.data_mut(|d| {
            d.remove_temp::<std::path::PathBuf>(egui::Id::new("pending_folder_pick"))
        });
        let folder = sess_actions.open_folder_path.take().or(async_folder);
        self.process_open_folder(folder);
        self.process_spawn_session(&sess_actions, &active_fg);
        self.process_duplicate_session(sess_actions.duplicate_session, &active_fg);
    }

    fn process_open_folder(&mut self, path: Option<std::path::PathBuf>) {
        let Some(path) = path else { return };
        let preferred = self.configured_shell();
        let shells = self.available_shells.clone();
        self.open_folder_dialog = Some(OpenFolderDialog::new(
            path,
            preferred,
            shells,
            &self.workspace_store,
        ));
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

        if let Some(group) = actions.close_all_workspace_id {
            self.close_all_target = CloseAllTarget::Group(group);
            self.show_close_all_confirm = true;
            self.close_all_frames_open = 0;
        }
    }

    /// Handle closing a pane from the sidebar quit button.
    fn process_quit_pane(&mut self, quit_pane_id: Option<u32>) {
        let Some(qpid) = quit_pane_id else { return };
        if !self.pane_state.panes.iter().any(|p| p.id == qpid) {
            return;
        }

        // Collect ALL pane IDs that must be closed.  When the quit target is
        // the root of a split tree we must also close every sibling leaf so
        // their PTY sessions are not leaked (H7).
        let panes_to_close: Vec<u32> = if let Some(tree) = self.pane_state.pane_trees.remove(&qpid)
        {
            // qpid is a root key — collect every leaf in the tree.
            tree.leaf_ids()
        } else {
            // qpid lives inside another root's tree — prune it.
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
            vec![qpid]
        };

        // Clear zoomed state if the zoomed pane is among those being closed (H8).
        if let Some(zpid) = self.zoomed_pane_id {
            if panes_to_close.contains(&zpid) {
                self.zoomed_pane_id = None;
            }
        }

        // Collect session IDs to kill, then remove pane entries.
        let killed_sids: Vec<u32> = self
            .pane_state
            .panes
            .iter()
            .filter(|p| panes_to_close.contains(&p.id))
            .filter_map(|p| match &p.content {
                PaneContent::Terminal(sid) => Some(*sid),
                _ => None,
            })
            .collect();
        self.pane_state
            .panes
            .retain(|p| !panes_to_close.contains(&p.id));

        if self
            .pane_state
            .active_pane_id
            .is_some_and(|id| panes_to_close.contains(&id))
        {
            self.pane_state.active_pane_id = self.pane_state.panes.last().map(|p| p.id);
        }

        for sid in &killed_sids {
            self.remove_session_and_cleanup(*sid);
        }
        if self
            .session_state
            .active_id
            .is_some_and(|id| killed_sids.contains(&id))
        {
            self.session_state.active_id = self.session_state.sessions.first().map(|e| e.id);
            self.update_is_active_flags();
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
                    labels: vec![],
                    last_active_at: crate::util::now_millis(),
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
                    labels: vec![],
                    last_active_at: crate::util::now_millis(),
                });
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (cols, rows),
                    },
                );
                let gid = self.pane_state.focused_group_id;
                self.pane_state.add_pane_to_group(gid, pane_id, None);
                if let Some(g) = self.pane_state.groups.get_mut(&gid) {
                    g.activate(pane_id);
                }
                self.activate_and_scroll_to_pane(pane_id);
                self.flash.trigger(
                    crate::app::feedback::FlashTarget::Tab(pane_id),
                    crate::app::feedback::FlashKind::Success,
                );
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
        let cwd = self.active_pane_cwd().or_else(|| {
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
        let cmd_to_run: Option<String> = active_fg.as_ref().map(|fp| {
            if is_claude_process(&fp.name, &fp.cmdline) {
                return "claude".to_string();
            }
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
                        labels: vec![],
                        last_active_at: crate::util::now_millis(),
                    },
                );
                self.pane_state.pane_trees.insert(
                    pane_id,
                    PaneNode::Leaf {
                        pane_id,
                        last_size: (cols, rows),
                    },
                );
                let gid = self.pane_state.focused_group_id;
                let insert_in_group = self.pane_state.groups.get(&gid).and_then(|g| {
                    g.active_pane_id
                        .and_then(|apid| g.pane_ids.iter().position(|&id| id == apid))
                        .map(|pos| pos + 1)
                });
                self.pane_state
                    .add_pane_to_group(gid, pane_id, insert_in_group);
                if let Some(g) = self.pane_state.groups.get_mut(&gid) {
                    g.activate(pane_id);
                }
                self.activate_and_scroll_to_pane(pane_id);
                self.flash.trigger(
                    crate::app::feedback::FlashTarget::Tab(pane_id),
                    crate::app::feedback::FlashKind::Success,
                );
            }
            if let Some(cmd) = cmd_to_run {
                if let Some(entry) = self.session_state.find_mut(new_id) {
                    entry.pending_command = Some(cmd);
                }
            }
        }
    }

    fn process_label_toggle(&mut self, toggle: Option<(u32, u32)>) {
        let Some((pane_id, label_id)) = toggle else {
            return;
        };
        if let Some(pane) = self.pane_state.panes.iter_mut().find(|p| p.id == pane_id) {
            if let Some(pos) = pane.labels.iter().position(|&l| l == label_id) {
                pane.labels.remove(pos);
            } else {
                pane.labels.push(label_id);
            }
            self.save_session();
        }
    }
}
