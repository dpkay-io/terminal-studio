use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::renderer::terminal_pass::TerminalGeometry;
use crate::workspace::WindowId;

use super::pane::RightTab;
use super::workspace_ui::{OpenFolderDialog, WorkspaceDialog, WorkspaceEditDialog};

pub(super) struct WindowView {
    pub(super) active_group: Option<u64>,
    pub(super) active_pane_id: Option<u32>,
    pub(super) active_id: Option<u32>,
    pub(super) last_pane_per_group: HashMap<Option<u64>, u32>,
    pub(super) last_focused_sid: Option<u32>,

    pub(super) right_tab: RightTab,
    pub(super) shown_md_tabs: HashSet<PathBuf>,

    pub(super) workspace_panel_ratio: f32,
    pub(super) workspace_panel_collapsed: bool,
    pub(super) notes_panel_ratio: f32,
    pub(super) notes_panel_collapsed: bool,

    pub(super) show_left_panel: bool,
    pub(super) show_right_panel: bool,
    pub(super) show_settings: bool,
    pub(super) show_shortcut_help: bool,
    pub(super) show_quick_switcher: bool,
    pub(super) quick_switcher_query: String,
    pub(super) quick_switcher_selected_ws: Option<usize>,
    pub(super) quick_switcher_search_active: bool,

    pub(super) workspace_dialog: Option<WorkspaceDialog>,
    pub(super) workspace_edit_dialog: Option<WorkspaceEditDialog>,
    pub(super) open_folder_dialog: Option<OpenFolderDialog>,

    pub(super) active_term_geo: Option<TerminalGeometry>,
    pub(super) active_term_ui_id: Option<egui::Id>,
    pub(super) was_focused: bool,
    pub(super) session_workspace_filter: Option<Option<u64>>,
}

impl WindowView {
    pub(super) fn new_for_workspace(ws_id: u64) -> Self {
        WindowView {
            active_group: Some(ws_id),
            active_pane_id: None,
            active_id: None,
            last_pane_per_group: HashMap::new(),
            last_focused_sid: None,
            right_tab: RightTab::Directory,
            shown_md_tabs: HashSet::new(),
            workspace_panel_ratio: 0.35,
            workspace_panel_collapsed: false,
            notes_panel_ratio: 0.30,
            notes_panel_collapsed: false,
            show_left_panel: true,
            show_right_panel: true,
            show_settings: false,
            show_shortcut_help: false,
            show_quick_switcher: false,
            quick_switcher_query: String::new(),
            quick_switcher_selected_ws: None,
            quick_switcher_search_active: false,
            workspace_dialog: None,
            workspace_edit_dialog: None,
            open_folder_dialog: None,
            active_term_geo: None,
            active_term_ui_id: None,
            was_focused: true,
            session_workspace_filter: None,
        }
    }
}

pub(super) struct PendingWindowFocus {
    pub(super) target_viewport_id: egui::ViewportId,
    /// `None` = main window, `Some(idx)` = extra window at that index.
    pub(super) target_window_idx: Option<usize>,
    pub(super) pane_id: u32,
    pub(super) group: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedExtraWindow {
    pub(super) id: WindowId,
    pub(super) workspace_id: u64,
    #[serde(default = "default_inner_size")]
    pub(super) inner_size: [f32; 2],
    #[serde(default)]
    pub(super) workspace_panel_ratio: Option<f32>,
    #[serde(default)]
    pub(super) workspace_panel_collapsed: Option<bool>,
    #[serde(default)]
    pub(super) notes_panel_ratio: Option<f32>,
    #[serde(default)]
    pub(super) notes_panel_collapsed: Option<bool>,
}

pub(super) fn default_inner_size() -> [f32; 2] {
    [1280.0, 800.0]
}

pub(super) struct ExtraWindow {
    pub(super) id: WindowId,
    pub(super) workspace_id: u64,
    pub(super) viewport_id: egui::ViewportId,
    pub(super) title: String,
    pub(super) inner_size: [f32; 2],
    pub(super) view: WindowView,
    pub(super) close_requested: bool,
}
