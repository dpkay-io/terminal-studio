use std::path::PathBuf;

use crate::editor_group::GroupId;
use crate::pane_tree::SplitDir;

#[derive(Clone, Debug)]
#[allow(dead_code)]
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
    GroupArea(GroupId),
    /// Drop near a group's edge: create a new split in `dir`, with the new
    /// group placed before (left/top) or after (right/bottom) the target.
    SplitEdge {
        group_id: GroupId,
        dir: SplitDir,
        before: bool,
    },
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
    /// Split `group_id` and put the dragged payload in the newly created group.
    SplitWithPayload {
        payload: DragPayload,
        group_id: GroupId,
        dir: SplitDir,
        before: bool,
    },
}

const DRAG_THRESHOLD: f32 = 5.0;

/// A drop target one window hit-tested, tagged with the window that found it
/// (`None` = the main window).
pub(super) struct PendingDrop {
    pub(super) window_idx: Option<usize>,
    pub(super) target: DropTarget,
}

pub(super) struct DragState {
    pub(super) payload: Option<DragPayload>,
    pub(super) drop_target: Option<DropTarget>,
    /// What `harvest` collected from the windows rendered so far this frame.
    pub(super) pending_drop: Option<PendingDrop>,
    /// Pointer released / Escape pressed in *any* window this frame. Both are
    /// per-viewport in egui, and the release is delivered to the window the
    /// drag started in rather than the one under the pointer, so neither can
    /// be read from a single window's input.
    pub(super) released: bool,
    pub(super) cancelled: bool,
    pub(super) origin_pos: egui::Pos2,
    pub(super) threshold_met: bool,
    pub(super) label: String,
}

impl DragState {
    pub(super) fn new() -> Self {
        Self {
            payload: None,
            drop_target: None,
            pending_drop: None,
            released: false,
            cancelled: false,
            origin_pos: egui::pos2(0.0, 0.0),
            threshold_met: false,
            label: String::new(),
        }
    }

    /// Forget everything the previous frame hit-tested.
    ///
    /// Every `drop_target` writer sets it only while the pointer is over the
    /// thing it belongs to, and none of them clear it on the way out, so
    /// without this a target survives the pointer leaving and a release over
    /// empty space acts on whatever was hovered last.
    pub(super) fn begin_frame(&mut self) {
        self.drop_target = None;
        self.pending_drop = None;
        self.released = false;
        self.cancelled = false;
    }

    /// Fold one window's hit-test result and input into this frame's state.
    ///
    /// Called after each window body renders. Windows only ever *write*
    /// `drop_target`; collecting it here rather than resolving inside the
    /// window body keeps the release and the hit-test from being split across
    /// windows — a body that resolves can only see its own viewport's input,
    /// and the main window always renders first.
    ///
    /// ponytail: last writer wins. Only one window can hold the pointer while
    /// a button is down, so at most one of them hit-tests a target anyway.
    pub(super) fn harvest(&mut self, window_idx: Option<usize>, released: bool, cancelled: bool) {
        self.released |= released;
        self.cancelled |= cancelled;
        if let Some(target) = self.drop_target.take() {
            self.pending_drop = Some(PendingDrop { window_idx, target });
        }
    }

    /// Move the harvested target back into `drop_target` for `resolve_drag`,
    /// returning the window that hit-tested it (`None` = the main window, and
    /// also the no-target case, which resolves to a no-op).
    pub(super) fn arm_pending_drop(&mut self) -> Option<usize> {
        match self.pending_drop.take() {
            Some(p) => {
                self.drop_target = Some(p.target);
                p.window_idx
            }
            None => {
                self.drop_target = None;
                None
            }
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
        self.pending_drop = None;
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
        self.pending_drop = None;
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
        painter.rect_stroke(rect, theme::R_MD, egui::Stroke::new(1.0_f32, t.overlay0));
        painter.galley(rect.min + padding, galley, egui::Color32::PLACEHOLDER);
    }
}

/// Fraction of each dimension, measured from the edge inward, that arms a
/// split rather than a plain "add to this group" drop.
///
/// A third splits the pane into a 3×3 grid: the centre cell joins, the eight
/// cells around it split towards their edge. Since the join region is
/// `(1 - 2f)²` of the area, this is the last value that still leaves a
/// comfortable centre target — 11% of the pane, against 4% at 0.4 and nothing
/// at all at 0.5.
const EDGE_ZONE_FRACTION: f32 = 1.0 / 3.0;

/// Resolve a pointer position inside `rect` to a split edge zone.
///
/// Returns `None` for the centre region and for points outside `rect`, meaning
/// the caller should fall back to `DropTarget::GroupArea`.
///
/// Distances are compared as a *fraction* of each dimension so that corners of
/// non-square rects resolve to the edge the pointer is proportionally nearest,
/// and every point in the rect maps to exactly one zone.
pub(super) fn edge_zone(rect: egui::Rect, pos: egui::Pos2) -> Option<(SplitDir, bool)> {
    if rect.width() <= 0.0 || rect.height() <= 0.0 || !rect.contains(pos) {
        return None;
    }

    let left = (pos.x - rect.min.x) / rect.width();
    let right = (rect.max.x - pos.x) / rect.width();
    let top = (pos.y - rect.min.y) / rect.height();
    let bottom = (rect.max.y - pos.y) / rect.height();

    let nearest = left.min(right).min(top).min(bottom);
    if nearest >= EDGE_ZONE_FRACTION {
        return None;
    }

    // ponytail: `nearest` is bit-identical to one of these four bindings via
    // f32::min, not derived from arithmetic on it — exact comparison is safe.
    // On an exact tie, the if/else-if order below breaks left, then top.
    #[allow(clippy::float_cmp)]
    if nearest == left {
        Some((SplitDir::Horizontal, true))
    } else if nearest == right {
        Some((SplitDir::Horizontal, false))
    } else if nearest == top {
        Some((SplitDir::Vertical, true))
    } else {
        Some((SplitDir::Vertical, false))
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
        DropTarget::GroupArea(_) | DropTarget::SplitEdge { .. } => {
            resolve_group(payload, target, pane_state)
        }
        _ => resolve(payload, target, pane_state, active_group),
    }
}

pub(super) fn resolve_group(
    payload: DragPayload,
    target: DropTarget,
    pane_state: &super::pane_state::PaneState,
) -> DragAction {
    use super::pane::PaneContent;

    match (payload, target) {
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

        // Diff → GroupArea
        (DragPayload::Diff(rel_path), DropTarget::GroupArea(group_id)) => {
            DragAction::InsertDiffInGroup {
                rel_path,
                target_group_id: group_id,
                at_index: None,
            }
        }

        // Note → GroupArea
        (DragPayload::Note(ws_id), DropTarget::GroupArea(group_id)) => {
            DragAction::InsertNoteInGroup {
                workspace_id: ws_id,
                target_group_id: group_id,
                at_index: None,
            }
        }

        // Workspace → group targets are not meaningful
        (DragPayload::Workspace(_), DropTarget::GroupArea(_)) => DragAction::Noop,

        // Workspace drags never create splits.
        (DragPayload::Workspace(_), DropTarget::SplitEdge { .. }) => DragAction::Noop,

        // A tab that is the sole occupant of the target group would leave that
        // group empty the instant it moved out of it.
        (
            DragPayload::Tab(pane_id),
            DropTarget::SplitEdge {
                group_id,
                dir,
                before,
            },
        ) => {
            if pane_state.find(pane_id).is_none() {
                return DragAction::Noop;
            }
            let sole_occupant = pane_state.group_of(pane_id) == Some(group_id)
                && pane_state
                    .groups
                    .get(&group_id)
                    .map(|g| g.pane_ids.len() <= 1)
                    .unwrap_or(true);
            if sole_occupant {
                DragAction::Noop
            } else {
                DragAction::SplitWithPayload {
                    payload: DragPayload::Tab(pane_id),
                    group_id,
                    dir,
                    before,
                }
            }
        }

        // One pane per session: a session that is already open moves its pane
        // into the new split instead of opening a second pane on the same PTY.
        (
            DragPayload::Session(session_id),
            DropTarget::SplitEdge {
                group_id,
                dir,
                before,
            },
        ) => {
            let existing = pane_state
                .panes
                .iter()
                .find(|p| matches!(p.content, PaneContent::Terminal(sid) if sid == session_id))
                .map(|p| p.id);
            match existing {
                // Delegating to the Tab arm reuses its sole-occupant guard.
                Some(pane_id) => resolve_group(
                    DragPayload::Tab(pane_id),
                    DropTarget::SplitEdge {
                        group_id,
                        dir,
                        before,
                    },
                    pane_state,
                ),
                None => DragAction::SplitWithPayload {
                    payload: DragPayload::Session(session_id),
                    group_id,
                    dir,
                    before,
                },
            }
        }

        // File / Diff / Note onto an edge: always a new split.
        (
            payload,
            DropTarget::SplitEdge {
                group_id,
                dir,
                before,
            },
        ) => DragAction::SplitWithPayload {
            payload,
            group_id,
            dir,
            before,
        },

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
                    workspace_id: None,
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
                untitled: false,
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
                    workspace_id: None,
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
        ps.group_layout.split_group(
            g1,
            g2,
            split_id,
            crate::pane_tree::SplitDir::Horizontal,
            false,
        );

        (ps, g1, g2)
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
    fn group_tab_invalid_pane_area_noop() {
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(DragPayload::Tab(999), DropTarget::GroupArea(g1), &ps);
        assert!(matches!(action, DragAction::Noop));
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
        let action = resolve_group(DragPayload::Session(10), DropTarget::GroupArea(g1), &ps);
        assert!(matches!(
            action,
            DragAction::FocusExistingTab { pane_id: 1 }
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
                untitled: false,
            }),
        )]);
        let g1 = ps.focused_group_id;
        // Create a second group for the drop target
        let g2 = ps.create_group(1); // dummy; we only care about dedup
        let split_id = ps.next_split_id;
        ps.next_split_id += 1;
        ps.group_layout.split_group(
            g1,
            g2,
            split_id,
            crate::pane_tree::SplitDir::Horizontal,
            false,
        );

        let action = resolve_group(
            DragPayload::File(PathBuf::from("/tmp/test.rs")),
            DropTarget::GroupArea(g2),
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::FocusExistingTab { pane_id: 1 }
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
        let action = resolve_group(DragPayload::Workspace(42), DropTarget::GroupArea(g1), &ps);
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
        state.drop_target = Some(DropTarget::GroupArea(g2));
        let action = resolve_drag(&mut state, &ps, None);
        // Pane 1 is in g1, dropping on g2 → MoveTabToGroup
        assert!(matches!(
            action,
            DragAction::MoveTabToGroup {
                pane_id: 1,
                at_index: None,
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

    // ── SplitEdge drag resolution tests ─────────────────────────────

    #[test]
    fn session_to_split_edge_creates_split() {
        let ps = make_grouped_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
        ]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::Session(50),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Horizontal,
                before: true,
            },
            &ps,
        );
        match action {
            DragAction::SplitWithPayload {
                payload: DragPayload::Session(sid),
                group_id,
                dir,
                before,
            } => {
                assert_eq!(sid, 50);
                assert_eq!(group_id, g1);
                assert_eq!(dir, SplitDir::Horizontal);
                assert!(before);
            }
            _ => panic!("expected SplitWithPayload"),
        }
    }

    #[test]
    fn file_to_split_edge_creates_split() {
        let ps = make_grouped_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
        ]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::File(PathBuf::from("/tmp/new.rs")),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Vertical,
                before: false,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::SplitWithPayload {
                payload: DragPayload::File(_),
                ..
            }
        ));
    }

    #[test]
    fn diff_to_split_edge_creates_split() {
        let ps = make_grouped_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
        ]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::Diff("src/main.rs".to_string()),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Horizontal,
                before: false,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::SplitWithPayload {
                payload: DragPayload::Diff(_),
                ..
            }
        ));
    }

    #[test]
    fn note_to_split_edge_creates_split() {
        let ps = make_grouped_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
        ]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::Note(100),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Vertical,
                before: true,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::SplitWithPayload {
                payload: DragPayload::Note(100),
                ..
            }
        ));
    }

    #[test]
    fn workspace_to_split_edge_is_noop() {
        let ps = make_grouped_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
        ]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::Workspace(42),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Horizontal,
                before: true,
            },
            &ps,
        );
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn tab_to_split_edge_of_own_lone_group_is_noop() {
        // Dragging the only pane of a group onto that group's own edge would
        // split a group and immediately empty it. Reject it.
        let ps = make_grouped_pane_state(vec![(1, PaneContent::Terminal(10))]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::Tab(1),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Horizontal,
                before: true,
            },
            &ps,
        );
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn tab_to_split_edge_of_shared_group_creates_split() {
        let ps = make_grouped_pane_state(vec![
            (1, PaneContent::Terminal(10)),
            (2, PaneContent::Terminal(20)),
        ]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::Tab(1),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Horizontal,
                before: true,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::SplitWithPayload {
                payload: DragPayload::Tab(1),
                ..
            }
        ));
    }

    #[test]
    fn tab_sole_occupant_to_other_group_edge_creates_split() {
        // Pane 3 is the sole occupant of g2. Dragging it onto g1's edge
        // leaves g2 empty — intentional; the executor is responsible for
        // collapsing the now-empty source group.
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Tab(3),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Horizontal,
                before: true,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::SplitWithPayload {
                payload: DragPayload::Tab(3),
                group_id,
                ..
            } if group_id == g1
        ));
    }

    #[test]
    fn session_already_open_to_split_edge_moves_existing_pane() {
        // Session 50 is already open as pane 1. Splitting must move that pane,
        // not open a second pane on the same PTY.
        let ps = make_grouped_pane_state(vec![
            (1, PaneContent::Terminal(50)),
            (2, PaneContent::Terminal(20)),
        ]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::Session(50),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Horizontal,
                before: true,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::SplitWithPayload {
                payload: DragPayload::Tab(1),
                group_id,
                ..
            } if group_id == g1
        ));
    }

    #[test]
    fn session_not_open_to_split_edge_keeps_session_payload() {
        let ps = make_grouped_pane_state(vec![
            (1, PaneContent::Terminal(50)),
            (2, PaneContent::Terminal(20)),
        ]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::Session(999),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Vertical,
                before: false,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::SplitWithPayload {
                payload: DragPayload::Session(999),
                group_id,
                ..
            } if group_id == g1
        ));
    }

    #[test]
    fn session_open_as_sole_occupant_of_target_group_is_noop() {
        // Pane 1 is the only pane of g1. Moving it out of g1 removes g1 from
        // the layout, leaving the split with nothing to attach to — same
        // reason the Tab arm rejects this.
        let ps = make_grouped_pane_state(vec![(1, PaneContent::Terminal(50))]);
        let g1 = ps.focused_group_id;
        let action = resolve_group(
            DragPayload::Session(50),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Horizontal,
                before: true,
            },
            &ps,
        );
        assert!(matches!(action, DragAction::Noop));
    }

    #[test]
    fn session_open_in_other_group_to_split_edge_moves_pane() {
        // Session 30 is open as pane 3, the sole occupant of g2. Dropping it on
        // g1's edge is fine: g1 survives the detach and hosts the split.
        let (ps, g1, _g2) = make_two_group_state();
        let action = resolve_group(
            DragPayload::Session(30),
            DropTarget::SplitEdge {
                group_id: g1,
                dir: SplitDir::Horizontal,
                before: false,
            },
            &ps,
        );
        assert!(matches!(
            action,
            DragAction::SplitWithPayload {
                payload: DragPayload::Tab(3),
                group_id,
                ..
            } if group_id == g1
        ));
    }

    // ── edge_zone geometry tests ────────────────────────────────────

    fn zone_rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 200.0))
    }

    #[test]
    fn edge_zone_centre_is_none() {
        assert_eq!(edge_zone(zone_rect(), egui::pos2(200.0, 100.0)), None);
    }

    #[test]
    fn edge_zone_left_quarter() {
        assert_eq!(
            edge_zone(zone_rect(), egui::pos2(20.0, 100.0)),
            Some((SplitDir::Horizontal, true))
        );
    }

    #[test]
    fn edge_zone_right_quarter() {
        assert_eq!(
            edge_zone(zone_rect(), egui::pos2(380.0, 100.0)),
            Some((SplitDir::Horizontal, false))
        );
    }

    #[test]
    fn edge_zone_top_quarter() {
        assert_eq!(
            edge_zone(zone_rect(), egui::pos2(200.0, 10.0)),
            Some((SplitDir::Vertical, true))
        );
    }

    #[test]
    fn edge_zone_bottom_quarter() {
        assert_eq!(
            edge_zone(zone_rect(), egui::pos2(200.0, 190.0)),
            Some((SplitDir::Vertical, false))
        );
    }

    #[test]
    fn edge_zone_corner_picks_proportionally_nearer_edge() {
        // Rect is 400x200. Point (10, 20): 2.5% from left, 10% from top.
        // Left is proportionally nearer, so horizontal wins.
        assert_eq!(
            edge_zone(zone_rect(), egui::pos2(10.0, 20.0)),
            Some((SplitDir::Horizontal, true))
        );
        // Point (60, 4): 15% from left, 2% from top. Top wins.
        assert_eq!(
            edge_zone(zone_rect(), egui::pos2(60.0, 4.0)),
            Some((SplitDir::Vertical, true))
        );
    }

    #[test]
    fn edge_zone_outside_rect_is_none() {
        assert_eq!(edge_zone(zone_rect(), egui::pos2(-5.0, 100.0)), None);
        assert_eq!(edge_zone(zone_rect(), egui::pos2(500.0, 100.0)), None);
    }

    #[test]
    fn edge_zone_degenerate_rect_is_none() {
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(0.0, 0.0));
        assert_eq!(edge_zone(r, egui::pos2(0.0, 0.0)), None);
    }

    #[test]
    fn edge_zone_off_origin_rect_matches_translated_points() {
        // Same 400x200 rect as `zone_rect`, translated to a non-zero origin.
        // The formula only uses `pos - rect.min` and `rect.max - pos`, so
        // results must be translation-invariant.
        let offset = egui::vec2(1000.0, 500.0);
        let rect =
            egui::Rect::from_min_size(egui::pos2(0.0, 0.0) + offset, egui::vec2(400.0, 200.0));

        // left-quarter point
        assert_eq!(
            edge_zone(rect, egui::pos2(20.0, 100.0) + offset),
            Some((SplitDir::Horizontal, true))
        );
        // centre point
        assert_eq!(edge_zone(rect, egui::pos2(200.0, 100.0) + offset), None);
        // top-quarter point
        assert_eq!(
            edge_zone(rect, egui::pos2(200.0, 10.0) + offset),
            Some((SplitDir::Vertical, true))
        );
    }

    #[test]
    fn edge_zone_boundary_sits_at_the_outer_third() {
        // 400x200: the join region is the middle third of each dimension, so
        // the vertical boundary is at x = 400/3 ≈ 133.3. Pins the constant —
        // the other cases here all sit far enough in to pass at any fraction.
        let r = zone_rect();
        assert_eq!(
            edge_zone(r, egui::pos2(130.0, 100.0)),
            Some((SplitDir::Horizontal, true))
        );
        assert_eq!(edge_zone(r, egui::pos2(140.0, 100.0)), None);
        // Same on the vertical axis: boundary at y = 200/3 ≈ 66.7.
        assert_eq!(
            edge_zone(r, egui::pos2(200.0, 64.0)),
            Some((SplitDir::Vertical, true))
        );
        assert_eq!(edge_zone(r, egui::pos2(200.0, 70.0)), None);
    }

    #[test]
    fn edge_zone_exact_tie_breaks_left_before_top() {
        // 200x200 rect: point (10, 10) is 0.05 from both the left and top
        // edges — an exact tie. The if/else-if chain checks left first.
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 200.0));
        assert_eq!(
            edge_zone(r, egui::pos2(10.0, 10.0)),
            Some((SplitDir::Horizontal, true))
        );
    }

    #[test]
    fn edge_zone_nan_position_is_none() {
        // Pointer position comes from egui's live input at runtime; NaN must
        // not panic or produce a bogus zone. `Rect::contains` uses `<=`
        // comparisons, which are false for NaN, so it returns false.
        assert_eq!(edge_zone(zone_rect(), egui::pos2(f32::NAN, 100.0)), None);
        assert_eq!(edge_zone(zone_rect(), egui::pos2(200.0, f32::NAN)), None);
    }

    // ── Per-frame harvesting across windows ────────────────────────────
    //
    // Each window body writes `drop_target` while it renders and `update()`
    // harvests it afterwards. These cover the ordering rules that a single
    // window body cannot see.

    fn dragging() -> DragState {
        let mut state = DragState::new();
        state.set_payload(DragPayload::Tab(1), egui::pos2(0.0, 0.0), "t");
        state.threshold_met = true;
        state
    }

    fn target_group(state: &DragState) -> Option<GroupId> {
        match state.drop_target {
            Some(DropTarget::GroupArea(g)) => Some(g),
            _ => None,
        }
    }

    #[test]
    fn begin_frame_forgets_last_frames_hit_test() {
        let mut state = dragging();
        state.drop_target = Some(DropTarget::GroupArea(7));
        state.harvest(None, true, false);
        assert!(state.released);
        assert!(state.pending_drop.is_some());

        state.begin_frame();
        assert!(state.drop_target.is_none());
        assert!(state.pending_drop.is_none());
        assert!(!state.released);
        assert!(!state.cancelled);
        // The drag itself survives — only the frame's hit-test is discarded.
        assert!(state.is_active());
    }

    #[test]
    fn harvest_tags_the_target_with_its_window() {
        let mut state = dragging();
        state.drop_target = Some(DropTarget::GroupArea(3));
        state.harvest(Some(2), false, false);

        let pending = state.pending_drop.as_ref().expect("target harvested");
        assert_eq!(pending.window_idx, Some(2));
        assert!(matches!(pending.target, DropTarget::GroupArea(3)));
        // Taken out of `drop_target` so the next window starts clean.
        assert!(state.drop_target.is_none());
    }

    #[test]
    fn harvest_from_a_window_with_no_target_keeps_the_earlier_one() {
        // The regression this whole split exists for: the main window renders
        // first and finds nothing, an extra window renders second and finds
        // the target. Neither may erase the other's result.
        let mut state = dragging();
        state.drop_target = Some(DropTarget::GroupArea(5));
        state.harvest(None, false, false);
        // Extra window renders, pointer is not over it, sets nothing.
        state.harvest(Some(0), true, false);

        let pending = state.pending_drop.as_ref().expect("main's target kept");
        assert_eq!(pending.window_idx, None);
        assert!(matches!(pending.target, DropTarget::GroupArea(5)));
    }

    #[test]
    fn release_in_one_window_is_visible_to_all() {
        // The release lands in the window the drag started in, which is not
        // the window that hit-tests the target.
        let mut state = dragging();
        state.harvest(None, true, false);
        state.drop_target = Some(DropTarget::GroupArea(9));
        state.harvest(Some(0), false, false);

        assert!(state.released, "release from window 0 must survive");
        assert_eq!(state.pending_drop.as_ref().unwrap().window_idx, Some(0));
    }

    #[test]
    fn escape_in_any_window_cancels() {
        let mut state = dragging();
        state.harvest(None, false, false);
        state.harvest(Some(0), false, true);
        assert!(state.cancelled);
    }

    #[test]
    fn arm_pending_drop_returns_the_owning_window() {
        let mut state = dragging();
        state.drop_target = Some(DropTarget::GroupArea(4));
        state.harvest(Some(1), true, false);

        assert_eq!(state.arm_pending_drop(), Some(1));
        assert_eq!(target_group(&state), Some(4));
        assert!(state.pending_drop.is_none());
    }

    #[test]
    fn arm_pending_drop_with_nothing_harvested_is_a_noop_drop() {
        // Released over empty space: no window found a target, so the drag
        // must resolve to Noop against the main window rather than reusing a
        // target left over from an earlier hover.
        let mut state = dragging();
        state.drop_target = Some(DropTarget::GroupArea(6));
        state.begin_frame();
        state.harvest(None, true, false);

        assert_eq!(state.arm_pending_drop(), None);
        assert!(state.drop_target.is_none());

        let ps = make_pane_state(vec![(1, PaneContent::Terminal(1))]);
        assert!(matches!(
            resolve_drag(&mut state, &ps, None),
            DragAction::Noop
        ));
        assert!(state.payload.is_none(), "drag is over either way");
    }

    #[test]
    fn armed_target_resolves_normally() {
        // Round-trip: a target harvested from an extra window survives
        // `arm_pending_drop` intact and resolves exactly as if it had been set
        // directly on `drop_target`.
        let (ps, _g1, g2) = make_two_group_state();

        let mut state = dragging();
        state.drop_target = Some(DropTarget::GroupArea(g2));
        state.harvest(Some(0), true, false);

        assert_eq!(state.arm_pending_drop(), Some(0));
        assert!(matches!(
            resolve_drag(&mut state, &ps, None),
            DragAction::MoveTabToGroup {
                pane_id: 1,
                target_group_id,
                ..
            } if target_group_id == g2
        ));
    }

    #[test]
    fn set_payload_clears_a_stale_pending_drop() {
        let mut state = dragging();
        state.drop_target = Some(DropTarget::GroupArea(1));
        state.harvest(None, false, false);
        state.set_payload(DragPayload::Session(9), egui::pos2(0.0, 0.0), "s");
        assert!(state.pending_drop.is_none());
        assert!(state.drop_target.is_none());
    }

    #[test]
    fn clear_drops_the_pending_drop() {
        let mut state = dragging();
        state.drop_target = Some(DropTarget::GroupArea(1));
        state.harvest(None, false, false);
        state.clear();
        assert!(state.pending_drop.is_none());
    }
}
