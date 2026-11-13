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
        .filter(|w| exclude_id != Some(w.id))
        .map(|w| w.color)
        .collect();
    // Presets first; once every one is taken, generated colours, so a new
    // workspace never starts with a colour the dialogs refuse.
    PRESET_COLORS
        .iter()
        .copied()
        .chain((0..=used.len()).map(generated_color))
        .find(|c| !used.contains(c))
        .unwrap_or(PRESET_COLORS[0])
}

/// Hue step between generated colours: the golden ratio of a full turn, so
/// consecutive colours land far apart on the wheel.
const GENERATED_HUE_STEP: f32 = 0.618_034;
/// Saturation and brightness in the range of the presets, so generated
/// colours sit alongside them.
const GENERATED_SATURATION: f32 = 0.6;
const GENERATED_VALUE: f32 = 0.85;

fn generated_color(index: usize) -> [u8; 3] {
    let hue = (index as f32 * GENERATED_HUE_STEP).fract();
    egui::ecolor::Hsva::new(hue, GENERATED_SATURATION, GENERATED_VALUE, 1.0).to_srgb()
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
            let folder_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // A folder outside every workspace is saved as a new one unless
            // the user unticks it; one inside a workspace already belongs to
            // it, so it isn't. The defaults are unused, so Open is enabled.
            let name = store.unique_name(&folder_name);
            let color = next_unused_color(store, None);
            (parent_ws.is_none(), name, color, None)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    fn store_with(workspaces: &[(&str, &str, [u8; 3])]) -> WorkspaceStore {
        WorkspaceStore {
            workspaces: workspaces
                .iter()
                .enumerate()
                .map(|(i, (name, path, color))| Workspace {
                    id: i as u64 + 1,
                    name: name.to_string(),
                    path: PathBuf::from(path),
                    color: *color,
                    host_window_id: None,
                    last_activated: 0,
                })
                .collect(),
        }
    }

    fn open_dialog(store: &WorkspaceStore, path: &str) -> OpenFolderDialog {
        OpenFolderDialog::new(PathBuf::from(path), ShellKind::Bash, Vec::new(), store)
    }

    #[test]
    fn folder_outside_every_workspace_is_saved_by_default() {
        let store = store_with(&[("proj", "/work/proj", PRESET_COLORS[0])]);
        let dlg = open_dialog(&store, "/work/other");
        assert!(dlg.save_as_workspace);
        assert_eq!(dlg.workspace_name, "other");
        assert_eq!(dlg.existing_workspace_id, None);
        assert!(dlg.parent_workspace.is_none());
    }

    #[test]
    fn folder_inside_a_workspace_is_not_saved_by_default() {
        let store = store_with(&[("proj", "/work/proj", PRESET_COLORS[0])]);
        let dlg = open_dialog(&store, "/work/proj/src");
        assert!(!dlg.save_as_workspace);
        assert_eq!(dlg.parent_workspace.map(|(id, _)| id), Some(1));
    }

    #[test]
    fn existing_workspace_folder_keeps_its_workspace() {
        let store = store_with(&[("proj", "/work/proj", PRESET_COLORS[3])]);
        let dlg = open_dialog(&store, "/work/proj");
        assert!(dlg.save_as_workspace);
        assert_eq!(dlg.existing_workspace_id, Some(1));
        assert_eq!(dlg.workspace_name, "proj");
        assert_eq!(dlg.workspace_color, PRESET_COLORS[3]);
    }

    #[test]
    fn default_name_and_colour_are_unused_so_open_is_enabled() {
        // Another folder with the same name, and every preset colour taken.
        let mut workspaces = vec![("app", "/a/app", PRESET_COLORS[0])];
        let others: Vec<String> = (1..PRESET_COLORS.len()).map(|i| format!("w{i}")).collect();
        for (i, name) in others.iter().enumerate() {
            workspaces.push((name.as_str(), "/elsewhere", PRESET_COLORS[i + 1]));
        }
        let store = store_with(&workspaces);
        let dlg = open_dialog(&store, "/b/app");
        assert!(dlg.save_as_workspace);
        assert!(!store.is_name_taken(&dlg.workspace_name, None));
        assert_eq!(dlg.workspace_name, "app 2");
        assert!(store
            .workspaces
            .iter()
            .all(|w| w.color != dlg.workspace_color));
    }

    #[test]
    fn generated_colours_are_distinct() {
        let colours: Vec<[u8; 3]> = (0..64).map(generated_color).collect();
        for (i, a) in colours.iter().enumerate() {
            assert!(!colours[i + 1..].contains(a), "colour {i} repeats");
        }
    }
}
