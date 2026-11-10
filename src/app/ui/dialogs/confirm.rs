use crate::app::pane::{PaneContent, PaneEntry};
use crate::app::{App, CloseAllTarget};
use crate::pane_tree::PaneNode;
use crate::ui_kit;

impl App {
    pub(in crate::app) fn render_close_all_confirm(&mut self, ctx: &egui::Context) {
        if !self.show_close_all_confirm {
            return;
        }

        let mut do_close = false;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Fixed(340.0),
            max_height: 120.0,
            ..Default::default()
        };

        let (title, count) = match self.close_all_target {
            CloseAllTarget::All => (
                "Close All Sessions".to_string(),
                self.pane_state.panes.len(),
            ),
            CloseAllTarget::EditorGroup(gid) => {
                let cnt = self
                    .pane_state
                    .groups
                    .get(&gid)
                    .map(|g| g.pane_ids.len())
                    .unwrap_or(0);
                (
                    format!("Close {} Tab{}", cnt, if cnt == 1 { "" } else { "s" }),
                    cnt,
                )
            }
            _ => {
                let group = match self.close_all_target {
                    CloseAllTarget::Group(g) => g,
                    _ => self.active_group,
                };
                let group_name = match group {
                    None => "Other".to_string(),
                    Some(id) => self
                        .workspace_store
                        .workspaces
                        .iter()
                        .find(|w| w.id == id)
                        .map(|w| w.name.clone())
                        .unwrap_or_else(|| "Unknown".to_string()),
                };
                let cnt = self
                    .pane_state
                    .panes
                    .iter()
                    .filter(|p| {
                        Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
                            == group
                    })
                    .count();
                (format!("Close \"{}\" Sessions", group_name), cnt)
            }
        };

        let mut cancel_clicked = false;
        let resp = ui_kit::dialog(ctx, self.vp_id("close_all"), config, |ui| {
            ui_kit::dialog_header(ui, &title);

            ui.label(format!(
                "This will close {} session{}. Are you sure?",
                count,
                if count == 1 { "" } else { "s" }
            ));

            if let Some(index) = ui_kit::dialog_footer(
                ui,
                &[
                    ui_kit::DialogButton::Secondary("Cancel"),
                    ui_kit::DialogButton::Destructive("Close All"),
                ],
            ) {
                match index {
                    0 => cancel_clicked = true,
                    1 => do_close = true,
                    _ => {}
                }
            }
        });

        // Enter shortcut — require at least one repaint cycle so the user
        // has time to read the dialog before a stale Enter key fires.
        self.close_all_frames_open = self.close_all_frames_open.saturating_add(1);
        if self.close_all_frames_open >= 2
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            do_close = true;
        }

        if do_close {
            self.show_close_all_confirm = false;
            self.close_all_frames_open = 0;

            let target = std::mem::take(&mut self.close_all_target);

            let pane_ids_to_close: Vec<u32> = match target {
                CloseAllTarget::All => self.pane_state.panes.iter().map(|p| p.id).collect(),
                CloseAllTarget::EditorGroup(gid) => self
                    .pane_state
                    .groups
                    .get(&gid)
                    .map(|g| g.pane_ids.clone())
                    .unwrap_or_default(),
                _ => {
                    let group = match target {
                        CloseAllTarget::Group(g) => g,
                        _ => self.active_group,
                    };
                    self.pane_state
                        .panes
                        .iter()
                        .filter(|p| {
                            Self::pane_group(&self.session_state.sessions, &self.workspace_store, p)
                                == group
                        })
                        .map(|p| p.id)
                        .collect()
                }
            };

            let session_ids: Vec<u32> = self
                .pane_state
                .panes
                .iter()
                .filter(|p| pane_ids_to_close.contains(&p.id))
                .filter_map(|p| match &p.content {
                    PaneContent::Terminal(sid) => Some(*sid),
                    _ => None,
                })
                .collect();

            self.pane_state
                .panes
                .retain(|p| !pane_ids_to_close.contains(&p.id));
            // Sync editor groups with surviving panes (collapse empty groups).
            self.pane_state.sync_groups_with_panes();
            // Prune closed panes from split trees, preserving surviving siblings.
            let affected_roots: Vec<u32> = self
                .pane_state
                .pane_trees
                .iter()
                .filter(|(_, tree)| {
                    tree.leaf_ids()
                        .iter()
                        .any(|lid| pane_ids_to_close.contains(lid))
                })
                .map(|(&rpid, _)| rpid)
                .collect();
            for rpid in affected_roots {
                let all_leaves_closing = self
                    .pane_state
                    .pane_trees
                    .get(&rpid)
                    .map(|t| {
                        t.leaf_ids()
                            .iter()
                            .all(|lid| pane_ids_to_close.contains(lid))
                    })
                    .unwrap_or(true);
                if all_leaves_closing {
                    self.pane_state.pane_trees.remove(&rpid);
                } else {
                    // Remove only the closing panes, keeping survivors
                    let closing_in_tree: Vec<u32> = self
                        .pane_state
                        .pane_trees
                        .get(&rpid)
                        .map(|t| {
                            t.leaf_ids()
                                .into_iter()
                                .filter(|lid| pane_ids_to_close.contains(lid))
                                .collect()
                        })
                        .unwrap_or_default();
                    let mut needs_rekey = false;
                    for pid in closing_in_tree {
                        if let Some(tree) = self.pane_state.pane_trees.get_mut(&rpid) {
                            use crate::pane_tree::RemoveResult;
                            match tree.remove_pane(pid) {
                                RemoveResult::CollapseToSibling(replacement) => {
                                    self.pane_state.pane_trees.insert(rpid, replacement);
                                    if pid == rpid {
                                        needs_rekey = true;
                                        break;
                                    }
                                }
                                RemoveResult::IsTarget => {
                                    self.pane_state.pane_trees.remove(&rpid);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    if needs_rekey {
                        if let Some(tree) = self.pane_state.pane_trees.remove(&rpid) {
                            let new_key = tree.first_leaf_id();
                            self.pane_state.pane_trees.insert(new_key, tree);
                        }
                    }
                }
            }
            if self
                .pane_state
                .active_pane_id
                .is_some_and(|id| pane_ids_to_close.contains(&id))
            {
                self.pane_state.active_pane_id = self.pane_state.panes.last().map(|p| p.id);
            }

            for sid in &session_ids {
                if self.pane_state.panes_referencing_session(*sid) == 0 {
                    self.remove_session_and_cleanup(*sid);
                }
            }

            self.session_state.active_id = if let Some(apid) = self.pane_state.active_pane_id {
                self.pane_state
                    .panes
                    .iter()
                    .find(|p| p.id == apid)
                    .and_then(|p| match &p.content {
                        PaneContent::Terminal(sid) => Some(*sid),
                        _ => None,
                    })
                    .or_else(|| self.session_state.sessions.first().map(|e| e.id))
            } else {
                self.session_state.sessions.first().map(|e| e.id)
            };
            self.update_is_active_flags();

            if self.pane_state.panes.is_empty() {
                if let Some(sid) = self.session_state.active_id {
                    let pane_id = self.pane_state.next_pane_id;
                    self.pane_state.next_pane_id += 1;
                    self.pane_state.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::Terminal(sid),
                        manual_width: None,
                        last_size: (0, 0),
                        labels: vec![],
                        last_active_at: crate::util::now_millis(),
                        workspace_id: self.active_group,
                    });
                    self.pane_state.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (0, 0),
                        },
                    );
                    if self.pane_state.groups.is_empty() {
                        let gid = self.pane_state.create_group(pane_id);
                        self.pane_state.group_layout =
                            crate::editor_group::GroupNode::Leaf { group_id: gid };
                        self.pane_state.focused_group_id = gid;
                    } else {
                        self.pane_state.add_pane_to_group(
                            self.pane_state.focused_group_id,
                            pane_id,
                            None,
                        );
                    }
                    self.pane_state.active_pane_id = Some(pane_id);
                }
            }

            self.save_session();
        } else if resp.dismissed || cancel_clicked {
            self.show_close_all_confirm = false;
            self.close_all_frames_open = 0;
            self.close_all_target = CloseAllTarget::default();
        }
    }
}
