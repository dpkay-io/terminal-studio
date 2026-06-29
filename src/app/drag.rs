use std::path::PathBuf;

use crate::editor_group::GroupId;

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
#[allow(dead_code)]
pub(super) enum DropTarget {
    TabBar(usize),
    PaneArea,
    NewWindow,
    GroupTabBar { group_id: GroupId, position: usize },
    GroupArea(GroupId),
}

#[allow(dead_code)]
pub(super) enum DragAction {
    // ── Old flat-list variants (kept for backward compat) ──────
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

    // ── New group-aware variants ───────────────────────────────
    ReorderTabInGroup {
        pane_id: u32,
        group_id: GroupId,
        to_index: usize,
    },
    MoveTabToGroup {
        pane_id: u32,
        target_group_id: GroupId,
        at_index: Option<usize>,
    },
    InsertTerminalInGroup {
        session_id: u32,
        target_group_id: GroupId,
        at_index: Option<usize>,
    },
    InsertFileEditorInGroup {
        path: PathBuf,
        target_group_id: GroupId,
        at_index: Option<usize>,
    },
    InsertDiffInGroup {
        rel_path: String,
        target_group_id: GroupId,
        at_index: Option<usize>,
    },
    InsertNoteInGroup {
        workspace_id: u64,
        target_group_id: GroupId,
        at_index: Option<usize>,
    },
}

const DRAG_THRESHOLD: f32 = 5.0;

pub(super) struct DragState {
    pub(super) payload: Option<DragPayload>,
    pub(super) drop_target: Option<DropTarget>,
    pub(super) origin_pos: egui::Pos2,
    pub(super) threshold_met: bool,
    pub(super) label: String,
}

impl DragState {
    pub(super) fn new() -> Self {
        Self {
            payload: None,
            drop_target: None,
            origin_pos: egui::pos2(0.0, 0.0),
            threshold_met: false,
            label: String::new(),
        }
    }

    pub(super) fn set_payload(
        &mut self,
        payload: DragPayload,
        origin: egui::Pos2,
        label: impl Into<String>,
    ) {
        self.payload = Some(payload);
        self.origin_pos = origin;
        self.threshold_met = false;
        self.drop_target = None;
        self.label = label.into();
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
        self.label.clear();
    }

    pub(super) fn paint_overlay(&self, ctx: &egui::Context) {
        if !self.is_active() || self.label.is_empty() {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };

        use crate::theme;
        let t = theme::active();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("drag_overlay"),
        ));

        let font = egui::FontId::proportional(theme::FONT_UI_SM);
        let galley = painter.layout_no_wrap(self.label.clone(), font, t.text);
        let text_size = galley.size();
        let padding = egui::vec2(theme::SP_3 + theme::SP_2, theme::SP_2);
        let offset = egui::vec2(12.0, -8.0);
        let rect = egui::Rect::from_min_size(pos + offset, text_size + padding * 2.0);

        let shadow = egui::epaint::Shadow {
            offset: egui::vec2(0.0, 2.0),
            blur: 6.0,
            spread: 0.0,
            color: egui::Color32::from_black_alpha(60),
        };
        painter.add(shadow.as_shape(rect, egui::Rounding::same(theme::R_MD)));
        painter.rect_filled(rect, theme::R_MD, t.surface1);
        painter.rect_stroke(rect, theme::R_MD, egui::Stroke::new(1.0, t.overlay0));
        painter.galley(rect.min + padding, galley, egui::Color32::PLACEHOLDER);
    }
}

pub(super) fn resolve_drag(
    state: &mut DragState,
    pane_state: &super::pane_state::PaneState,
    active_group: Option<u64>,
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

    // Route group targets to new resolver, old targets to old resolver
    match &target {
        DropTarget::GroupTabBar { .. } | DropTarget::GroupArea(_) => {
            resolve_group(payload, target, pane_state)
        }
        _ => resolve(payload, target, pane_state, active_group),
    }
}

fn resolve_group(
    payload: DragPayload,
    target: DropTarget,
    pane_state: &super::pane_state::PaneState,
) -> DragAction {
    use super::pane::PaneContent;

    match (payload, target) {
        // Tab → GroupTabBar: reorder within group or move to different group
        (DragPayload::Tab(pane_id), DropTarget::GroupTabBar { group_id, position }) => {
            if pane_state.find(pane_id).is_none() {
                return DragAction::Noop;
            }
            let source_group = pane_state.group_of(pane_id);
            if source_group == Some(group_id) {
                DragAction::ReorderTabInGroup {
                    pane_id,
                    group_id,
                    to_index: position,
                }
            } else {
                DragAction::MoveTabToGroup {
                    pane_id,
                    target_group_id: group_id,
                    at_index: Some(position),
                }
            }
        }

        // Tab → GroupArea: move tab to that group (append)
        (DragPayload::Tab(pane_id), DropTarget::GroupArea(group_id)) => {
            if pane_state.find(pane_id).is_none() {
                return DragAction::Noop;
            }
            let source_group = pane_state.group_of(pane_id);
            if source_group == Some(group_id) {
                DragAction::Noop
            } else {
                DragAction::MoveTabToGroup {
                    pane_id,
                    target_group_id: group_id,
                    at_index: None,
                }
            }
        }

        // Session → GroupTabBar
        (DragPayload::Session(session_id), DropTarget::GroupTabBar { group_id, position }) => {
            let existing = pane_state
                .panes
                .iter()
                .find(|p| matches!(p.content, PaneContent::Terminal(sid) if sid == session_id));
            if let Some(pane) = existing {
                return DragAction::FocusExistingTab { pane_id: pane.id };
            }
            DragAction::InsertTerminalInGroup {
                session_id,
                target_group_id: group_id,
                at_index: Some(position),
            }
        }

        // Session → GroupArea
        (DragPayload::Session(session_id), DropTarget::GroupArea(group_id)) => {
            let existing = pane_state
                .panes
                .iter()
                .find(|p| matches!(p.content, PaneContent::Terminal(sid) if sid == session_id));
            if let Some(pane) = existing {
                return DragAction::FocusExistingTab { pane_id: pane.id };
            }
            DragAction::InsertTerminalInGroup {
                session_id,
                target_group_id: group_id,
                at_index: None,
            }
        }

        // File → GroupTabBar
        (DragPayload::File(path), DropTarget::GroupTabBar { group_id, position }) => {
            let existing = pane_state
                .panes
                .iter()
                .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path));
            if let Some(pane) = existing {
                return DragAction::FocusExistingTab { pane_id: pane.id };
            }
            DragAction::InsertFileEditorInGroup {
                path,
                target_group_id: group_id,
                at_index: Some(position),
            }
        }

        // File → GroupArea
        (DragPayload::File(path), DropTarget::GroupArea(group_id)) => {
            let existing = pane_state
                .panes
                .iter()
                .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path));
            if let Some(pane) = existing {
                return DragAction::FocusExistingTab { pane_id: pane.id };
            }
            DragAction::InsertFileEditorInGroup {
                path,
                target_group_id: group_id,
                at_index: None,
            }
        }

        // Diff → GroupTabBar / GroupArea
        (DragPayload::Diff(rel_path), DropTarget::GroupTabBar { group_id, position }) => {
            DragAction::InsertDiffInGroup {
                rel_path,
                target_group_id: group_id,
                at_index: Some(position),
            }
        }
        (DragPayload::Diff(rel_path), DropTarget::GroupArea(group_id)) => {
            DragAction::InsertDiffInGroup {
                rel_path,
                target_group_id: group_id,
                at_index: None,
            }
        }

        // Note → GroupTabBar / GroupArea
        (DragPayload::Note(ws_id), DropTarget::GroupTabBar { group_id, position }) => {
            DragAction::InsertNoteInGroup {
                workspace_id: ws_id,
                target_group_id: group_id,
                at_index: Some(position),
            }
        }
        (DragPayload::Note(ws_id), DropTarget::GroupArea(group_id)) => {
            DragAction::InsertNoteInGroup {
                workspace_id: ws_id,
                target_group_id: group_id,
                at_index: None,
            }
        }

        // Workspace → group targets are not meaningful
        (DragPayload::Workspace(_), DropTarget::GroupTabBar { .. } | DropTarget::GroupArea(_)) => {
            DragAction::Noop
        }

        // Fallback (should not be reached due to routing in resolve_drag)
        _ => DragAction::Noop,
    }
}

fn resolve(
    payload: DragPayload,
    target: DropTarget,
    pane_state: &super::pane_state::PaneState,
    active_group: Option<u64>,
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
            let existing = pane_state
                .panes
                .iter()
                .find(|p| matches!(p.content, PaneContent::Terminal(sid) if sid == session_id));
            if let Some(pane) = existing {
                return DragAction::FocusExistingTab { pane_id: pane.id };
            }
            DragAction::InsertTerminalPane {
                session_id,
                at_index: Some(idx),
            }
        }
        (DragPayload::Session(session_id), DropTarget::PaneArea) => {
            let existing = pane_state
                .panes
                .iter()
                .find(|p| matches!(p.content, PaneContent::Terminal(sid) if sid == session_id));
            if let Some(pane) = existing {
                return DragAction::FocusExistingTab { pane_id: pane.id };
            }
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
                .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path && ed.workspace_id == active_group));
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
                .find(|p| matches!(&p.content, PaneContent::FileEditor(ed) if ed.path == path && ed.workspace_id == active_group));
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
                    labels: vec![],
                    last_active_at: crate::util::now_millis(),
                })
                .collect(),
            active_pane_id: first_id,
            next_pane_id: next_id,
            pane_trees: std::collections::HashMap::new(),
            next_split_id: 1,
            groups: std::collections::HashMap::new(),
            group_layout: crate::editor_group::GroupNode::Leaf { group_id: 0 },
            focused_group_id: 0,
            next_group_id: 1,
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
        state.set_payload(DragPayload::Tab(1), egui::pos2(10.0, 20.0), "");
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
        state.set_payload(DragPayload::Tab(1), egui::pos2(10.0, 10.0), "");
        state.update_threshold(egui::pos2(13.0, 10.0));
        assert!(!state.is_active());
        state.update_threshold(egui::pos2(15.0, 10.0));
        assert!(state.is_active());
    }

    #[test]
    fn double_set_replaces_payload() {
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.set_payload(DragPayload::Session(5), egui::pos2(50.0, 50.0), "");
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
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(2));
        let action = resolve_drag(&mut state, &ps, None);
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
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps, None);
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn tab_invalid_pane_id_is_noop() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(999), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps, None);
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
            state.set_payload(DragPayload::Tab(2), egui::pos2(0.0, 0.0), "");
            state.threshold_met = true;
            state.drop_target = Some(DropTarget::TabBar(1));
            resolve_drag(&mut state, &ps, None)
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
        state.set_payload(DragPayload::Session(20), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(1));
        let action = resolve_drag(&mut state, &ps, None);
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
        state.set_payload(DragPayload::Session(20), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::PaneArea);
        let action = resolve_drag(&mut state, &ps, None);
        assert!(matches!(
            action,
            DragAction::InsertTerminalPane {
                session_id: 20,
                at_index: None
            }
        ));
    }

    #[test]
    fn session_already_open_focuses_existing_tabbar() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Session(10), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps, None);
        assert!(matches!(
            action,
            DragAction::FocusExistingTab { pane_id: 1 }
        ));
    }

    #[test]
    fn session_already_open_focuses_existing_pane_area() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Session(10), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::PaneArea);
        let action = resolve_drag(&mut state, &ps, None);
        assert!(matches!(
            action,
            DragAction::FocusExistingTab { pane_id: 1 }
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
            "",
        );
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps, None);
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
                stale: false,
                loading: false,
            }),
        )]);
        let mut state = DragState::new();
        state.set_payload(
            DragPayload::File(PathBuf::from("/tmp/test.rs")),
            egui::pos2(0.0, 0.0),
            "",
        );
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps, None);
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
            "",
        );
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::PaneArea);
        let action = resolve_drag(&mut state, &ps, None);
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
        state.set_payload(DragPayload::Note(100), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps, None);
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
        state.set_payload(DragPayload::Workspace(42), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::NewWindow);
        let action = resolve_drag(&mut state, &ps, None);
        assert!(matches!(
            action,
            DragAction::OpenWorkspaceWindow { workspace_id: 42 }
        ));
    }

    #[test]
    fn workspace_to_tabbar_is_noop() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Workspace(42), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps, None);
        assert!(matches!(action, DragAction::Noop));
    }

    // Edge case tests
    #[test]
    fn no_target_is_noop() {
        let ps = make_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let mut state = DragState::new();
        state.set_payload(DragPayload::Session(20), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        let action = resolve_drag(&mut state, &ps, None);
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn cancelled_drag_clears_state() {
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.clear();
        assert!(state.payload.is_none());
        assert!(state.drop_target.is_none());
        assert!(!state.threshold_met);
    }

    // ── Group-aware drag resolution tests ──────────────────────────

    /// Build a PaneState with all panes in a single group.
    fn make_grouped_pane_state(panes: Vec<(u32, PaneContent)>) -> PaneState {
        let ids: Vec<u32> = panes.iter().map(|(id, _)| *id).collect();
        let next_id = ids.iter().map(|id| id + 1).max().unwrap_or(1);

        let mut ps = PaneState {
            panes: panes
                .into_iter()
                .map(|(id, content)| PaneEntry {
                    id,
                    content,
                    manual_width: None,
                    last_size: (80, 24),
                    labels: vec![],
                    last_active_at: crate::util::now_millis(),
                })
                .collect(),
            active_pane_id: ids.first().copied(),
            next_pane_id: next_id,
            pane_trees: std::collections::HashMap::new(),
            next_split_id: 1,
            groups: std::collections::HashMap::new(),
            group_layout: crate::editor_group::GroupNode::Leaf { group_id: 0 },
            focused_group_id: 0,
            next_group_id: 1,
        };

        // Create a single group with all panes
        if let Some(&first) = ids.first() {
            let gid = ps.create_group(first);
            for &id in &ids[1..] {
                ps.add_pane_to_group(gid, id, None);
            }
            ps.focused_group_id = gid;
            ps.group_layout = crate::editor_group::GroupNode::Leaf { group_id: gid };
        }

        // Also set up old pane_trees for backward compat
        for &id in &ids {
            ps.pane_trees.insert(
                id,
                PaneNode::Leaf {
                    pane_id: id,
                    last_size: (80, 24),
                },
            );
        }

        ps
    }

    /// Build a PaneState with panes 1,2 in group g1 and pane 3 in group g2.
    /// Returns (state, g1, g2).
    fn make_two_group_state() -> (PaneState, u32, u32) {
        let mut ps = make_grouped_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
            (3, PaneContent::Terminal(30)),
        ]);
        let g1 = ps.focused_group_id;

        // Move pane 3 to a second group
        let g2 = ps.create_group(3);
        if let Some(group) = ps.groups.get_mut(&g1) {
            group.remove_pane(3);
        }

        // Update layout to show both groups
        let split_id = ps.next_split_id;
        ps.next_split_id += 1;
        ps.group_layout
            .split_group(g1, g2, split_id, crate::pane_tree::SplitDir::Horizontal);

        (ps, g1, g2)
    }

    #[test]
    fn group_tab_reorder_same_group() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Tab(1),
            DropTarget::GroupTabBar {
                group_id: g1,
                position: 1,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::ReorderTabInGroup {
                pane_id: 1,
                group_id,
                to_index: 1,
            } if group_id == g1
        ));
    }

    #[test]
    fn group_tab_move_different_group() {
        let (ps, _g1, g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Tab(1),
            DropTarget::GroupTabBar {
                group_id: g2,
                position: 0,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::MoveTabToGroup {
                pane_id: 1,
                target_group_id,
                at_index: Some(0),
            } if target_group_id == g2
        ));
    }

    #[test]
    fn group_tab_to_area_moves() {
        let (ps, _g1, g2) = make_two_group_state();
        let action = resolve_group(DragPayload::Tab(1), DropTarget::GroupArea(g2), &ps);
        assert!(matches!(
            action,
            DragAction::MoveTabToGroup {
                pane_id: 1,
                target_group_id,
                at_index: None,
            } if target_group_id == g2
        ));
    }

    #[test]
    fn group_tab_to_same_area_noop() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(DragPayload::Tab(1), DropTarget::GroupArea(g1), &ps);
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn group_tab_invalid_pane_noop() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Tab(999),
            DropTarget::GroupTabBar {
                group_id: g1,
                position: 0,
            },
            &ps,
        );
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn group_tab_invalid_pane_area_noop() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(DragPayload::Tab(999), DropTarget::GroupArea(g1), &ps);
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn group_session_to_tab_bar_creates() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Session(50),
            DropTarget::GroupTabBar {
                group_id: g1,
                position: 0,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::InsertTerminalInGroup {
                session_id: 50,
                target_group_id,
                at_index: Some(0),
            } if target_group_id == g1
        ));
    }

    #[test]
    fn group_session_to_area_creates() {
        let (ps, _g1, g2) = make_two_group_state();
        let action = resolve_group(DragPayload::Session(50), DropTarget::GroupArea(g2), &ps);
        assert!(matches!(
            action,
            DragAction::InsertTerminalInGroup {
                session_id: 50,
                target_group_id,
                at_index: None,
            } if target_group_id == g2
        ));
    }

    #[test]
    fn group_session_existing_focuses() {
        let (ps, g1, _g2) = make_two_group_state();
        // Session 10 is already open as pane 1
        let action = resolve_group(
            DragPayload::Session(10),
            DropTarget::GroupTabBar {
                group_id: g1,
                position: 0,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::FocusExistingTab { pane_id: 1 }
        ));
    }

    #[test]
    fn group_file_to_tab_bar_creates() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::File(PathBuf::from("/tmp/new.rs")),
            DropTarget::GroupTabBar {
                group_id: g1,
                position: 1,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::InsertFileEditorInGroup {
                target_group_id,
                at_index: Some(1),
                ..
            } if target_group_id == g1
        ));
    }

    #[test]
    fn group_file_to_area_creates() {
        let (ps, _g1, g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::File(PathBuf::from("/tmp/new.rs")),
            DropTarget::GroupArea(g2),
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::InsertFileEditorInGroup {
                target_group_id,
                at_index: None,
                ..
            } if target_group_id == g2
        ));
    }

    #[test]
    fn group_file_existing_focuses() {
        let mut ps = make_grouped_pane_state(vec![(
            1,
            PaneContent::FileEditor(FileEditorState {
                path: PathBuf::from("/tmp/test.rs"),
                content: String::new(),
                dirty: false,
                save_error: false,
                workspace_id: None,
                show_preview: false,
                stale: false,
                loading: false,
            }),
        )]);
        let g1 = ps.focused_group_id;
        // Create a second group for the drop target
        let g2 = ps.create_group(1); // dummy; we only care about dedup
        let split_id = ps.next_split_id;
        ps.next_split_id += 1;
        ps.group_layout
            .split_group(g1, g2, split_id, crate::pane_tree::SplitDir::Horizontal);

        let action = resolve_group(
            DragPayload::File(PathBuf::from("/tmp/test.rs")),
            DropTarget::GroupTabBar {
                group_id: g2,
                position: 0,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::FocusExistingTab { pane_id: 1 }
        ));
    }

    #[test]
    fn group_diff_to_tab_bar() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Diff("src/main.rs".to_string()),
            DropTarget::GroupTabBar {
                group_id: g1,
                position: 2,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::InsertDiffInGroup {
                target_group_id,
                at_index: Some(2),
                ..
            } if target_group_id == g1
        ));
    }

    #[test]
    fn group_diff_to_area() {
        let (ps, _g1, g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Diff("src/lib.rs".to_string()),
            DropTarget::GroupArea(g2),
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::InsertDiffInGroup {
                target_group_id,
                at_index: None,
                ..
            } if target_group_id == g2
        ));
    }

    #[test]
    fn group_note_to_tab_bar() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Note(100),
            DropTarget::GroupTabBar {
                group_id: g1,
                position: 0,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::InsertNoteInGroup {
                workspace_id: 100,
                target_group_id,
                at_index: Some(0),
            } if target_group_id == g1
        ));
    }

    #[test]
    fn group_note_to_area() {
        let (ps, _g1, g2) = make_two_group_state();
        let action = resolve_group(DragPayload::Note(200), DropTarget::GroupArea(g2), &ps);
        assert!(matches!(
            action,
            DragAction::InsertNoteInGroup {
                workspace_id: 200,
                target_group_id,
                at_index: None,
            } if target_group_id == g2
        ));
    }

    #[test]
    fn group_workspace_to_group_target_noop() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Workspace(42),
            DropTarget::GroupTabBar {
                group_id: g1,
                position: 0,
            },
            &ps,
        );
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn group_workspace_to_new_window_via_resolve_drag() {
        let (ps, _g1, _g2) = make_two_group_state();
        let mut state = DragState::new();
        state.set_payload(DragPayload::Workspace(42), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::NewWindow);
        let action = resolve_drag(&mut state, &ps, None);
        assert!(matches!(
            action,
            DragAction::OpenWorkspaceWindow { workspace_id: 42 }
        ));
    }

    #[test]
    fn resolve_drag_routes_group_targets() {
        let (ps, _g1, g2) = make_two_group_state();
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::GroupTabBar {
            group_id: g2,
            position: 0,
        });
        let action = resolve_drag(&mut state, &ps, None);
        // Pane 1 is in g1, dropping on g2 → MoveTabToGroup
        assert!(matches!(
            action,
            DragAction::MoveTabToGroup {
                pane_id: 1,
                at_index: Some(0),
                ..
            }
        ));
    }

    #[test]
    fn resolve_drag_routes_old_targets() {
        let (ps, _g1, _g2) = make_two_group_state();
        let mut state = DragState::new();
        state.set_payload(DragPayload::Session(50), egui::pos2(0.0, 0.0), "");
        state.threshold_met = true;
        state.drop_target = Some(DropTarget::TabBar(0));
        let action = resolve_drag(&mut state, &ps, None);
        assert!(matches!(
            action,
            DragAction::InsertTerminalPane {
                session_id: 50,
                at_index: Some(0),
            }
        ));
    }
}
