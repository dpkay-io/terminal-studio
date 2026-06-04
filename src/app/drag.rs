use std::path::PathBuf;

#[derive(Clone, Debug)]
pub(super) enum DragPayload {
    Tab(u32),
    Session(u32),
    Workspace(u64),
    File(PathBuf),
    Diff(String),
    Note(u64),
}

#[derive(Clone, Debug)]
pub(super) enum DropTarget {
    TabBar(usize),
    PaneArea,
    NewWindow,
}

pub(super) enum DragAction {
    ReorderTab {
        from_pane_id: u32,
        to_index: usize,
    },
    ExtractFromSplitAndInsert {
        pane_id: u32,
        to_index: usize,
    },
    InsertTerminalPane {
        session_id: u32,
        at_index: Option<usize>,
    },
    InsertFileEditorPane {
        path: PathBuf,
        at_index: Option<usize>,
    },
    InsertDiffPane {
        rel_path: String,
        at_index: Option<usize>,
    },
    InsertNotePane {
        workspace_id: u64,
        at_index: Option<usize>,
    },
    OpenWorkspaceWindow {
        workspace_id: u64,
    },
    FocusExistingTab {
        pane_id: u32,
    },
    Noop,
}

const DRAG_THRESHOLD: f32 = 5.0;

pub(super) struct DragState {
    pub(super) payload: Option<DragPayload>,
    pub(super) drop_target: Option<DropTarget>,
    pub(super) origin_pos: egui::Pos2,
    pub(super) threshold_met: bool,
}

impl DragState {
    pub(super) fn new() -> Self {
        Self {
            payload: None,
            drop_target: None,
            origin_pos: egui::pos2(0.0, 0.0),
            threshold_met: false,
        }
    }

    pub(super) fn set_payload(&mut self, payload: DragPayload, origin: egui::Pos2) {
        self.payload = Some(payload);
        self.origin_pos = origin;
        self.threshold_met = false;
        self.drop_target = None;
    }

    pub(super) fn update_threshold(&mut self, current_pos: egui::Pos2) {
        if self.payload.is_some() && !self.threshold_met {
            let delta = current_pos - self.origin_pos;
            if delta.length() >= DRAG_THRESHOLD {
                self.threshold_met = true;
            }
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.payload.is_some() && self.threshold_met
    }

    pub(super) fn clear(&mut self) {
        self.payload = None;
        self.drop_target = None;
        self.threshold_met = false;
    }
}

pub(super) fn resolve_drag(
    state: &mut DragState,
    pane_state: &super::pane_state::PaneState,
) -> DragAction {
    let payload = match state.payload.take() {
        Some(p) => p,
        None => return DragAction::Noop,
    };
    let target = match state.drop_target.take() {
        Some(t) => t,
        None => {
            state.clear();
            return DragAction::Noop;
        }
    };
    state.clear();
    resolve(payload, target, pane_state)
}

fn resolve(
    payload: DragPayload,
    target: DropTarget,
    pane_state: &super::pane_state::PaneState,
) -> DragAction {
    use super::pane::PaneContent;

    match (payload, target) {
        (DragPayload::Tab(pane_id), DropTarget::TabBar(to_index)) => {
            if pane_state.find(pane_id).is_none() {
                return DragAction::Noop;
            }
            let in_split = pane_state
                .root_of(pane_id)
                .is_some_and(|root| root != pane_id);
            if in_split {
                // Extracting a pane from a split is always meaningful — skip same-index guard.
                DragAction::ExtractFromSplitAndInsert { pane_id, to_index }
            } else {
                let Some(from_index) = pane_state.panes.iter().position(|p| p.id == pane_id) else {
                    return DragAction::Noop;
                };
                if from_index == to_index {
                    return DragAction::Noop;
                }
                DragAction::ReorderTab {
                    from_pane_id: pane_id,
                    to_index,
                }
            }
        }
        (DragPayload::Tab(_), _) => DragAction::Noop,

        (DragPayload::Session(session_id), DropTarget::TabBar(idx)) => {
            DragAction::InsertTerminalPane {
                session_id,
                at_index: Some(idx),
            }
        }
        (DragPayload::Session(session_id), DropTarget::PaneArea) => {
            DragAction::InsertTerminalPane {
                session_id,
                at_index: None,
            }
        }
        (DragPayload::Session(_), _) => DragAction::Noop,

        (DragPayload::File(path), DropTarget::TabBar(idx)) => {
            let existing = pane_state
                .panes
                .iter()
                .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path));
            if let Some(pane) = existing {
                return DragAction::FocusExistingTab { pane_id: pane.id };
            }
            DragAction::InsertFileEditorPane {
                path,
                at_index: Some(idx),
            }
        }
        (DragPayload::File(path), DropTarget::PaneArea) => {
            let existing = pane_state
                .panes
                .iter()
                .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path));
            if let Some(pane) = existing {
                return DragAction::FocusExistingTab { pane_id: pane.id };
            }
            DragAction::InsertFileEditorPane {
                path,
                at_index: None,
            }
        }
        (DragPayload::File(_), _) => DragAction::Noop,

        (DragPayload::Diff(rel_path), DropTarget::TabBar(idx)) => DragAction::InsertDiffPane {
            rel_path,
            at_index: Some(idx),
        },
        (DragPayload::Diff(rel_path), DropTarget::PaneArea) => DragAction::InsertDiffPane {
            rel_path,
            at_index: None,
        },
        (DragPayload::Diff(_), _) => DragAction::Noop,

        (DragPayload::Note(ws_id), DropTarget::TabBar(idx)) => DragAction::InsertNotePane {
            workspace_id: ws_id,
            at_index: Some(idx),
        },
        (DragPayload::Note(ws_id), DropTarget::PaneArea) => DragAction::InsertNotePane {
            workspace_id: ws_id,
            at_index: None,
        },
        (DragPayload::Note(_), _) => DragAction::Noop,

        (DragPayload::Workspace(ws_id), DropTarget::NewWindow) => DragAction::OpenWorkspaceWindow {
            workspace_id: ws_id,
        },
        (DragPayload::Workspace(_), _) => DragAction::Noop,
    }
}

#[cfg(test)]
mod tests {
    use super::super::pane::{FileEditorState, PaneContent, PaneEntry};
    use super::super::pane_state::PaneState;
    use super::*;
    use crate::pane_tree::PaneNode;
    use std::path::PathBuf;

    fn make_pane_state(panes: Vec<(u32, PaneContent)>) -> PaneState {
        // Collect IDs first so we can build pane_trees after consuming the vec.
        let ids: Vec<u32> = panes.iter().map(|(id, _)| *id).collect();
        let first_id = ids.first().copied();
        let next_id = ids.iter().map(|id| id + 1).max().unwrap_or(1);

        let mut ps = PaneState {
            panes: panes
                .into_iter()
                .map(|(id, content)| PaneEntry {
                    id,
                    content,
                    manual_width: None,
                    last_size: (80, 24),
                })
                .collect(),
            active_pane_id: first_id,
            next_pane_id: next_id,
            pane_trees: std::collections::HashMap::new(),
            next_split_id: 1,
        };
        for id in &ids {
            ps.pane_trees.insert(
                *id,
                PaneNode::Leaf {
                    pane_id: *id,
                    last_size: (80, 24),
                },
            );
        }
        ps
    }

    // Lifecycle tests
    #[test]
    fn set_payload_and_clear() {
        let mut state = DragState::new();
        assert!(state.payload.is_none());
        state.set_payload(DragPayload::Tab(1), egui::pos2(10.0, 20.0));
        assert!(state.payload.is_some());
        assert!(!state.threshold_met);
        state.clear();
        assert!(state.payload.is_none());
        assert!(state.drop_target.is_none());
        assert!(!state.threshold_met);
    }

    #[test]
    fn threshold_not_met_within_5px() {
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(1), egui::pos2(10.0, 10.0));
        state.update_threshold(egui::pos2(13.0, 10.0));
        assert!(!state.is_active());
        state.update_threshold(egui::pos2(15.0, 10.0));
        assert!(state.is_active());
    }

    #[test]
    fn double_set_replaces_payload() {
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.set_payload(DragPayload::Session(5), egui::pos2(50.0, 50.0));
        assert!(!state.threshold_met);
        assert!(matches!(state.payload, Some(DragPayload::Session(5))));
    }

    // Tab resolution tests
    #[test]
    fn tab_reorder_standalone() {
        let ps = make_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
            (3, PaneContent::Terminal(30)),
        ]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(2));
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(
            action,
            DragAction::ReorderTab {
                from_pane_id: 1,
                to_index: 2
            }
        ));
    }

    #[test]
    fn tab_drag_to_own_position_is_noop() {
        let ps = make_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
        ]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn tab_invalid_pane_id_is_noop() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(999), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn tab_from_split_extracts() {
        let mut ps = make_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
            (3, PaneContent::Terminal(30)),
        ]);
        ps.pane_trees.remove(&2);
        let tree = ps.pane_trees.get_mut(&1).unwrap();
        tree.split_pane(1, 2, 1, crate::pane_tree::SplitDir::Horizontal);
        let action = {
            let mut state = DragState::new();
            state.set_payload(DragPayload::Tab(2), egui::pos2(0.0, 0.0));
            state.threshold_met = true;
            state.drop_target = Some(DropTarget::TabBar(1));
            resolve_drag(&mut state, &ps)
        };
        assert!(matches!(
            action,
            DragAction::ExtractFromSplitAndInsert {
                pane_id: 2,
                to_index: 1
            }
        ));
    }

    // Session tests
    #[test]
    fn session_to_tabbar_creates_pane() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Session(20), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(1));
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(
            action,
            DragAction::InsertTerminalPane {
                session_id: 20,
                at_index: Some(1)
            }
        ));
    }

    #[test]
    fn session_to_pane_area_creates_appended_pane() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Session(20), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::PaneArea);
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(
            action,
            DragAction::InsertTerminalPane {
                session_id: 20,
                at_index: None
            }
        ));
    }

    // File tests
    #[test]
    fn file_to_tabbar_creates_editor_pane() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(
            DragPayload::File(PathBuf::from("/tmp/test.rs")),
            egui::pos2(0.0, 0.0),
        );
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(
            action,
            DragAction::InsertFileEditorPane {
                at_index: Some(0),
                ..
            }
        ));
    }

    #[test]
    fn file_already_open_focuses_existing() {
        let ps = make_pane_state(vec![(
            1,
            PaneContent::FileEditor(FileEditorState {
                path: PathBuf::from("/tmp/test.rs"),
                content: String::new(),
                dirty: false,
                save_error: false,
                workspace_id: None,
                show_preview: false,
            }),
        )]);
        let mut state = DragState::new();
        state.set_payload(
            DragPayload::File(PathBuf::from("/tmp/test.rs")),
            egui::pos2(0.0, 0.0),
        );
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(
            action,
            DragAction::FocusExistingTab { pane_id: 1 }
        ));
    }

    // Diff test
    #[test]
    fn diff_to_pane_area_creates_diff_pane() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(
            DragPayload::Diff("src/main.rs".to_string()),
            egui::pos2(0.0, 0.0),
        );
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::PaneArea);
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(
            action,
            DragAction::InsertDiffPane { at_index: None, .. }
        ));
    }

    // Note test
    #[test]
    fn note_to_tabbar_creates_note_pane() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Note(100), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(
            action,
            DragAction::InsertNotePane {
                workspace_id: 100,
                at_index: Some(0)
            }
        ));
    }

    // Workspace tests
    #[test]
    fn workspace_to_new_window() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Workspace(42), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::NewWindow);
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(
            action,
            DragAction::OpenWorkspaceWindow { workspace_id: 42 }
        ));
    }

    #[test]
    fn workspace_to_tabbar_is_noop() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Workspace(42), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(action, DragAction::Noop));
    }

    // Edge case tests
    #[test]
    fn no_target_is_noop() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Session(20), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        let action = resolve_drag(&mut state, &ps);
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn cancelled_drag_clears_state() {
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0));
        state.threshold_met = true;
        state.clear();
        assert!(state.payload.is_none());
        assert!(state.drop_target.is_none());
        assert!(!state.threshold_met);
    }
}
