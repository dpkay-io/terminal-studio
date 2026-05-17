use std::path::PathBuf;

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
