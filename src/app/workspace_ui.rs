//! What the workspace dialogs hold, and the checks their buttons follow:
//! saving a folder as a workspace, editing one, and opening one or more
//! folders (each saved as a workspace unless the user unticks it). Pure — the
//! dialogs themselves are in `ui/dialogs.rs` and `ui/open_folders_dialog.rs`.

use std::path::{Path, PathBuf};

use super::workspace_colors::{pick_distinct_color, pick_distinct_colors, PRESET_COLORS};
use crate::pty::ShellKind;
use crate::workspace::WorkspaceStore;

/// A workspace colour as a dialog edits it: one of the preset swatches, or a
/// custom colour from the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ColorChoice {
    color: [u8; 3],
    /// What the custom swatch and the picker show; the last colour picked
    /// there, or the starting colour.
    custom: [u8; 3],
    /// The colour came from the picker rather than a swatch, so no preset
    /// shows as selected (even one that happens to match).
    custom_selected: bool,
}

impl ColorChoice {
    pub(super) fn new(color: [u8; 3]) -> Self {
        ColorChoice {
            color,
            custom: color,
            custom_selected: !PRESET_COLORS.contains(&color),
        }
    }

    pub(super) fn color(&self) -> [u8; 3] {
        self.color
    }

    pub(super) fn custom(&self) -> [u8; 3] {
        self.custom
    }

    pub(super) fn is_custom(&self) -> bool {
        self.custom_selected
    }

    /// Whether `preset`'s swatch shows as the selected one.
    pub(super) fn is_preset_selected(&self, preset: [u8; 3]) -> bool {
        !self.custom_selected && self.color == preset
    }

    /// The picker keeps the custom colour it had.
    pub(super) fn select_preset(&mut self, preset: [u8; 3]) {
        self.color = preset;
        self.custom_selected = false;
    }

    pub(super) fn set_custom(&mut self, color: [u8; 3]) {
        self.color = color;
        self.custom = color;
        self.custom_selected = true;
    }
}

/// Every colour a workspace uses, other than `exclude_id`'s.
fn used_colors(store: &WorkspaceStore, exclude_id: Option<u64>) -> Vec<[u8; 3]> {
    store
        .workspaces
        .iter()
        .filter(|w| exclude_id != Some(w.id))
        .map(|w| w.color)
        .collect()
}

/// The colour a new workspace starts with: the one least like any in use.
pub(super) fn next_unused_color(store: &WorkspaceStore, exclude_id: Option<u64>) -> [u8; 3] {
    pick_distinct_color(&used_colors(store, exclude_id))
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

pub(super) struct WorkspaceDialog {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) color: ColorChoice,
    pub(super) focus_requested: bool,
}

impl WorkspaceDialog {
    pub(super) fn new(path: PathBuf, store: &WorkspaceStore) -> Self {
        WorkspaceDialog {
            name: store.unique_name(&folder_name(&path)),
            path,
            color: ColorChoice::new(next_unused_color(store, None)),
            focus_requested: false,
        }
    }
}

pub(super) struct WorkspaceEditDialog {
    pub(super) workspace_id: u64,
    pub(super) name: String,
    pub(super) color: ColorChoice,
    pub(super) confirm_delete: bool,
    pub(super) focus_requested: bool,
}

impl WorkspaceEditDialog {
    pub(super) fn new(id: u64, name: String, color: [u8; 3]) -> Self {
        WorkspaceEditDialog {
            workspace_id: id,
            name,
            color: ColorChoice::new(color),
            confirm_delete: false,
            focus_requested: false,
        }
    }
}

/// One folder in the Open Folder dialog.
pub(super) struct FolderEntry {
    pub(super) path: PathBuf,
    pub(super) save_as_workspace: bool,
    pub(super) name: String,
    pub(super) color: ColorChoice,
    /// Set when the path exactly matches an existing workspace.
    pub(super) existing_workspace_id: Option<u64>,
    /// Set when the path is inside an existing workspace.
    pub(super) parent_workspace: Option<(u64, String)>,
}

/// What stops one folder from opening as it is set up now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct EntryCheck {
    pub(super) name_missing: bool,
    /// Another workspace, or another folder in this dialog, has the name.
    pub(super) name_taken: bool,
    /// Another workspace, or another folder in this dialog, has the colour.
    pub(super) color_taken: bool,
}

impl EntryCheck {
    pub(super) fn is_ok(&self) -> bool {
        !(self.name_missing || self.name_taken || self.color_taken)
    }
}

pub(super) struct OpenFolderDialog {
    /// The folders to open, in the order they were picked; never empty.
    pub(super) entries: Vec<FolderEntry>,
    pub(super) selected_shell: ShellKind,
    pub(super) available_shells: Vec<ShellKind>,
    pub(super) focus_requested: bool,
}

impl OpenFolderDialog {
    /// None when `paths` is empty. See [`Self::add_folders`].
    pub(super) fn new(
        paths: Vec<PathBuf>,
        preferred_shell: ShellKind,
        shells: Vec<ShellKind>,
        store: &WorkspaceStore,
    ) -> Option<Self> {
        let mut dialog = OpenFolderDialog {
            entries: Vec::new(),
            selected_shell: preferred_shell,
            available_shells: shells,
            focus_requested: false,
        };
        dialog.add_folders(paths, store);
        (!dialog.entries.is_empty()).then_some(dialog)
    }

    /// Adds `paths` after the folders already listed, skipping any listed
    /// already; the ones listed keep what the user set.
    ///
    /// A folder outside every workspace is saved as a new one unless the user
    /// unticks it; one inside a workspace already belongs to it, so it isn't;
    /// one that is a workspace keeps its name and colour. New names and
    /// colours are free — of the store and of every folder in the dialog — so
    /// Open stays enabled. Folders saved by default get the most distinct
    /// colours first.
    pub(super) fn add_folders(&mut self, paths: Vec<PathBuf>, store: &WorkspaceStore) {
        let first_new = self.entries.len();
        let mut taken_names: Vec<String> = self.entries.iter().map(|e| e.name.clone()).collect();
        for path in paths {
            if self
                .entries
                .iter()
                .any(|e| crate::util::paths_equal(&e.path, &path))
            {
                continue;
            }
            let entry = new_entry(path, store, &taken_names);
            taken_names.push(entry.name.clone());
            self.entries.push(entry);
        }

        let mut used = used_colors(store, None);
        used.extend(self.entries[..first_new].iter().map(|e| e.color.color()));
        let mut fresh: Vec<&mut FolderEntry> = self.entries[first_new..]
            .iter_mut()
            .filter(|e| e.existing_workspace_id.is_none())
            .collect();
        // Stable: folders saved by default first, each group in pick order.
        fresh.sort_by_key(|e| !e.save_as_workspace);
        let colors = pick_distinct_colors(&used, fresh.len());
        for (entry, color) in fresh.into_iter().zip(colors) {
            entry.color = ColorChoice::new(color);
        }
    }

    /// One check per entry, in order. An entry not saved as a workspace has
    /// nothing to check.
    pub(super) fn checks(&self, store: &WorkspaceStore) -> Vec<EntryCheck> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, entry)| self.check(i, entry, store))
            .collect()
    }

    pub(super) fn can_open(&self, store: &WorkspaceStore) -> bool {
        self.checks(store).iter().all(EntryCheck::is_ok)
    }

    fn check(&self, index: usize, entry: &FolderEntry, store: &WorkspaceStore) -> EntryCheck {
        if !entry.save_as_workspace {
            return EntryCheck::default();
        }
        let name = entry.name.trim();
        let others = || {
            self.entries
                .iter()
                .enumerate()
                .filter(move |&(j, other)| j != index && other.save_as_workspace)
                .map(|(_, other)| other)
        };
        let own_id = entry.existing_workspace_id;
        EntryCheck {
            name_missing: name.is_empty(),
            name_taken: !name.is_empty()
                && (store.is_name_taken(name, own_id)
                    || others().any(|o| o.name.trim().eq_ignore_ascii_case(name))),
            color_taken: store.is_color_taken(entry.color.color(), own_id)
                || others().any(|o| o.color.color() == entry.color.color()),
        }
    }
}

/// The entry `path` starts with. A new folder's colour is assigned by the
/// caller, which knows every folder being added.
fn new_entry(path: PathBuf, store: &WorkspaceStore, taken_names: &[String]) -> FolderEntry {
    if let Some(ws) = store.find_for_path(&path) {
        return FolderEntry {
            path,
            save_as_workspace: true,
            name: ws.name.clone(),
            color: ColorChoice::new(ws.color),
            existing_workspace_id: Some(ws.id),
            parent_workspace: None,
        };
    }
    let parent_workspace = store.find_for_cwd(&path).map(|w| (w.id, w.name.clone()));
    FolderEntry {
        save_as_workspace: parent_workspace.is_none(),
        name: unique_name_among(store, taken_names, &folder_name(&path)),
        path,
        color: ColorChoice::new(PRESET_COLORS[0]),
        existing_workspace_id: None,
        parent_workspace,
    }
}

/// `base`, or `base 2`, `base 3`, … — the first name neither a workspace
/// nor `taken` uses.
fn unique_name_among(store: &WorkspaceStore, taken: &[String], base: &str) -> String {
    let free = |name: &str| {
        !store.is_name_taken(name, None) && !taken.iter().any(|t| t.eq_ignore_ascii_case(name))
    };
    if free(base) {
        return base.to_string();
    }
    // Each workspace and each taken name rules out at most one candidate.
    let limit = store.workspaces.len() + taken.len() + 2;
    (2..=limit)
        .map(|n| format!("{base} {n}"))
        .find(|name| free(name))
        .unwrap_or_else(|| format!("{base} {}", limit + 1))
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

    fn open_dialog(store: &WorkspaceStore, paths: &[&str]) -> OpenFolderDialog {
        OpenFolderDialog::new(
            paths.iter().map(PathBuf::from).collect(),
            ShellKind::Bash,
            Vec::new(),
            store,
        )
        .expect("at least one folder")
    }

    fn only(dlg: &OpenFolderDialog) -> &FolderEntry {
        assert_eq!(dlg.entries.len(), 1);
        &dlg.entries[0]
    }

    #[test]
    fn folder_outside_every_workspace_is_saved_by_default() {
        let store = store_with(&[("proj", "/work/proj", PRESET_COLORS[0])]);
        let dlg = open_dialog(&store, &["/work/other"]);
        let entry = only(&dlg);
        assert!(entry.save_as_workspace);
        assert_eq!(entry.name, "other");
        assert_eq!(entry.existing_workspace_id, None);
        assert!(entry.parent_workspace.is_none());
    }

    #[test]
    fn folder_inside_a_workspace_is_not_saved_by_default() {
        let store = store_with(&[("proj", "/work/proj", PRESET_COLORS[0])]);
        let dlg = open_dialog(&store, &["/work/proj/src"]);
        let entry = only(&dlg);
        assert!(!entry.save_as_workspace);
        assert_eq!(entry.parent_workspace.as_ref().map(|(id, _)| *id), Some(1));
    }

    #[test]
    fn existing_workspace_folder_keeps_its_workspace() {
        let store = store_with(&[("proj", "/work/proj", PRESET_COLORS[3])]);
        let dlg = open_dialog(&store, &["/work/proj"]);
        let entry = only(&dlg);
        assert!(entry.save_as_workspace);
        assert_eq!(entry.existing_workspace_id, Some(1));
        assert_eq!(entry.name, "proj");
        assert_eq!(entry.color.color(), PRESET_COLORS[3]);
        assert!(
            dlg.can_open(&store),
            "its own name and colour are not taken"
        );
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
        let dlg = open_dialog(&store, &["/b/app"]);
        let entry = only(&dlg);
        assert_eq!(entry.name, "app 2");
        assert!(!store.is_color_taken(entry.color.color(), None));
        assert!(dlg.can_open(&store));
    }

    #[test]
    fn several_folders_get_free_distinct_names_and_colours() {
        let store = store_with(&[("app", "/a/app", PRESET_COLORS[0])]);
        let dlg = open_dialog(&store, &["/b/app", "/c/app", "/d/api", "/e/web"]);
        let names: Vec<&str> = dlg.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["app 2", "app 3", "api", "web"]);
        let colours: Vec<[u8; 3]> = dlg.entries.iter().map(|e| e.color.color()).collect();
        for (i, c) in colours.iter().enumerate() {
            assert_ne!(*c, PRESET_COLORS[0], "entry {i} reuses the store's colour");
            assert!(!colours[i + 1..].contains(c), "entry {i} repeats a colour");
        }
        assert!(dlg.can_open(&store));
    }

    #[test]
    fn a_folder_picked_twice_is_listed_once() {
        let store = WorkspaceStore::default();
        let dlg = open_dialog(&store, &["/x/app", "/x/app", "/x/api"]);
        assert_eq!(dlg.entries.len(), 2);
    }

    #[test]
    fn no_folders_is_no_dialog() {
        let store = WorkspaceStore::default();
        assert!(OpenFolderDialog::new(Vec::new(), ShellKind::Bash, Vec::new(), &store).is_none());
    }

    #[test]
    fn folders_saved_by_default_get_the_first_picks() {
        let store = store_with(&[("proj", "/work/proj", PRESET_COLORS[0])]);
        // The nested one comes first but is not saved by default.
        let dlg = open_dialog(&store, &["/work/proj/src", "/other"]);
        let expected = pick_distinct_colors(&[PRESET_COLORS[0]], 2);
        assert_eq!(dlg.entries[1].color.color(), expected[0]);
        assert_eq!(dlg.entries[0].color.color(), expected[1]);
    }

    #[test]
    fn names_and_colours_clash_with_other_folders_in_the_dialog() {
        let store = WorkspaceStore::default();
        let mut dlg = open_dialog(&store, &["/x/app", "/x/api"]);
        dlg.entries[1].name = "APP".to_string();
        let colour = dlg.entries[0].color.color();
        dlg.entries[1].color = ColorChoice::new(colour);
        let checks = dlg.checks(&store);
        assert!(checks[0].name_taken && checks[1].name_taken);
        assert!(checks[0].color_taken && checks[1].color_taken);
        assert!(!dlg.can_open(&store));

        // Unticking one leaves nothing for the other to clash with.
        dlg.entries[1].save_as_workspace = false;
        assert!(dlg.can_open(&store));
    }

    #[test]
    fn a_blank_name_or_a_used_colour_blocks_open() {
        let store = store_with(&[("proj", "/work/proj", PRESET_COLORS[2])]);
        let mut dlg = open_dialog(&store, &["/new"]);
        dlg.entries[0].name = "   ".to_string();
        assert!(dlg.checks(&store)[0].name_missing);
        dlg.entries[0].name = "new".to_string();
        dlg.entries[0].color.select_preset(PRESET_COLORS[2]);
        assert!(dlg.checks(&store)[0].color_taken);
        assert!(!dlg.can_open(&store));
    }

    #[test]
    fn adding_folders_keeps_edits_and_picks_fresh_names_and_colours() {
        let store = WorkspaceStore::default();
        let mut dlg = open_dialog(&store, &["/x/app"]);
        dlg.entries[0].name = "renamed".to_string();
        let first_colour = dlg.entries[0].color.color();
        dlg.add_folders(
            vec![PathBuf::from("/x/app"), PathBuf::from("/y/renamed")],
            &store,
        );
        assert_eq!(dlg.entries.len(), 2, "the folder already listed is skipped");
        assert_eq!(dlg.entries[0].name, "renamed");
        assert_eq!(dlg.entries[0].color.color(), first_colour);
        assert_eq!(dlg.entries[1].name, "renamed 2");
        assert_ne!(dlg.entries[1].color.color(), first_colour);
        assert!(dlg.can_open(&store));
    }

    #[test]
    fn workspace_dialog_starts_with_a_free_name_and_colour() {
        let store = store_with(&[("app", "/a/app", PRESET_COLORS[0])]);
        let dlg = WorkspaceDialog::new(PathBuf::from("/b/app"), &store);
        assert_eq!(dlg.name, "app 2");
        assert!(!store.is_color_taken(dlg.color.color(), None));
    }

    #[test]
    fn colour_choice_tracks_swatch_or_picker() {
        let mut choice = ColorChoice::new(PRESET_COLORS[1]);
        assert!(choice.is_preset_selected(PRESET_COLORS[1]));
        assert!(!choice.is_custom());
        choice.set_custom([255, 128, 0]);
        assert_eq!(choice.color(), [255, 128, 0]);
        assert!(choice.is_custom());
        assert!(!choice.is_preset_selected(PRESET_COLORS[1]));
        choice.select_preset(PRESET_COLORS[4]);
        assert!(choice.is_preset_selected(PRESET_COLORS[4]));
        assert_eq!(
            choice.custom(),
            [255, 128, 0],
            "the picker keeps its colour"
        );
        // A colour that is no preset starts as custom.
        let other = ColorChoice::new([1, 2, 3]);
        assert!(other.is_custom() && !other.is_preset_selected([1, 2, 3]));
    }
}
