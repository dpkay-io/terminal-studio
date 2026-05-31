use std::path::PathBuf;

use crate::pty::ShellKind;
use crate::workspace::WorkspaceStore;

pub(super) const PRESET_COLORS: &[[u8; 3]] = &[
    [100, 140, 230], // blue
    [80, 200, 100],  // green
    [220, 120, 80],  // orange
    [200, 80, 160],  // pink
    [140, 100, 220], // purple
    [80, 200, 200],  // teal
    [220, 200, 60],  // yellow
    [200, 80, 80],   // red
];

pub(super) fn next_unused_color(store: &WorkspaceStore, exclude_id: Option<u64>) -> [u8; 3] {
    let used: Vec<[u8; 3]> = store
        .workspaces
        .iter()
        .filter(|w| exclude_id.map_or(true, |id| w.id != id))
        .map(|w| w.color)
        .collect();
    PRESET_COLORS
        .iter()
        .find(|c| !used.contains(c))
        .copied()
        .unwrap_or(PRESET_COLORS[0])
}

pub(super) struct WorkspaceDialog {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) selected_color: [u8; 3],
    pub(super) custom_color: [f32; 3],
    pub(super) show_custom_picker: bool,
    pub(super) focus_requested: bool,
}

impl WorkspaceDialog {
    pub(super) fn new(path: PathBuf) -> Self {
        WorkspaceDialog {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path,
            selected_color: PRESET_COLORS[0],
            custom_color: [0.4, 0.55, 0.9],
            show_custom_picker: false,
            focus_requested: false,
        }
    }
}

pub(super) struct WorkspaceEditDialog {
    pub(super) workspace_id: u64,
    pub(super) name: String,
    pub(super) selected_color: [u8; 3],
    pub(super) custom_color: [f32; 3],
    pub(super) show_custom_picker: bool,
    pub(super) confirm_delete: bool,
    pub(super) focus_requested: bool,
}

impl WorkspaceEditDialog {
    pub(super) fn new(id: u64, name: String, color: [u8; 3]) -> Self {
        let is_preset = PRESET_COLORS.contains(&color);
        WorkspaceEditDialog {
            workspace_id: id,
            name,
            selected_color: color,
            custom_color: [
                color[0] as f32 / 255.0,
                color[1] as f32 / 255.0,
                color[2] as f32 / 255.0,
            ],
            show_custom_picker: !is_preset,
            confirm_delete: false,
            focus_requested: false,
        }
    }
}

pub(super) struct OpenFolderDialog {
    pub(super) path: PathBuf,
    pub(super) selected_shell: ShellKind,
    pub(super) available_shells: Vec<ShellKind>,
    pub(super) save_as_workspace: bool,
    pub(super) workspace_name: String,
    pub(super) workspace_color: [u8; 3],
    pub(super) custom_color: [f32; 3],
    pub(super) show_custom_picker: bool,
    /// Set when the selected path exactly matches an existing workspace.
    pub(super) existing_workspace_id: Option<u64>,
    /// Set when the selected path is a subdirectory of an existing workspace.
    pub(super) parent_workspace: Option<(u64, String)>,
    pub(super) focus_requested: bool,
}

impl OpenFolderDialog {
    pub(super) fn new(
        path: PathBuf,
        preferred_shell: ShellKind,
        shells: Vec<ShellKind>,
        store: &WorkspaceStore,
    ) -> Self {
        let exact_ws = store.find_for_path(&path);
        let parent_ws = if exact_ws.is_none() {
            store.find_for_cwd(&path)
        } else {
            None
        };

        let (save_as_workspace, name, color, existing_id) = if let Some(ws) = exact_ws {
            (true, ws.name.clone(), ws.color, Some(ws.id))
        } else {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let color = next_unused_color(store, None);
            (false, name, color, None)
        };

        let parent_workspace = parent_ws.map(|w| (w.id, w.name.clone()));

        OpenFolderDialog {
            path,
            selected_shell: preferred_shell,
            available_shells: shells,
            save_as_workspace,
            workspace_name: name,
            workspace_color: color,
            custom_color: [
                color[0] as f32 / 255.0,
                color[1] as f32 / 255.0,
                color[2] as f32 / 255.0,
            ],
            show_custom_picker: false,
            existing_workspace_id: existing_id,
            parent_workspace,
            focus_requested: false,
        }
    }
}
