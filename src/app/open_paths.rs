//! Opening what the user picked or dropped: folders through the Open Folder
//! dialog (one terminal per folder, each saved as a workspace unless
//! unticked), files in the editor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::feedback::{FlashKind, FlashTarget};
use super::pane::{FileEditorState, PaneContent, PaneEntry};
use super::pending_open::OpenRequest;
use super::workspace_ui::{FolderEntry, OpenFolderDialog};
use super::App;
use crate::pane_tree::PaneNode;
use crate::pty::ShellKind;
use crate::workspace::Workspace;

/// Name of the thread the folder picker runs on (it blocks until closed).
const FOLDER_PICKER_THREAD: &str = "folder-picker";

impl App {
    /// Opens the system folder picker (several folders may be selected) on
    /// its own thread; the folders chosen come back through `open_queue` to
    /// this window.
    pub(super) fn pick_folders_async(&self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        let queue = self.open_queue.clone();
        let window = self.current_window_id.clone();
        let spawned = std::thread::Builder::new()
            .name(FOLDER_PICKER_THREAD.into())
            .spawn(move || {
                let Some(folders) = rfd::FileDialog::new().pick_folders() else {
                    return;
                };
                let request = OpenRequest {
                    window,
                    folders,
                    ..Default::default()
                };
                queue.push(request, &ctx);
            });
        if let Err(e) = spawned {
            log::error!("could not start the folder picker: {e}");
        }
    }

    /// Runs this window's queued open requests: folders join the Open Folder
    /// dialog, files open in the editor (in the split they were dropped on),
    /// and anything that couldn't be opened flashes an error.
    pub(super) fn process_open_requests(&mut self, ctx: &egui::Context) {
        let requests = self.open_queue.take_for(self.current_window_id.as_ref());
        for request in requests {
            if !request.skipped.is_empty() {
                log::warn!("not opened: {:?}", request.skipped);
                self.flash.trigger(FlashTarget::Global, FlashKind::Error);
            }
            self.show_open_folders(request.folders);
            if request.files.is_empty() {
                continue;
            }
            if let Some(gid) = request.target_group {
                if self.pane_state.groups.contains_key(&gid) {
                    self.pane_state.focused_group_id = gid;
                }
            }
            for path in request.files {
                super::persistence::push_recent_file(&mut self.recent_files, &path);
                let preview = self.default_editor_preview(&path);
                self.open_file_in_editor(path, preview, ctx);
            }
            super::persistence::save_recent_files(&self.recent_files);
        }
    }

    /// Shows the Open Folder dialog for `paths`, or adds them to the one
    /// already open. Nothing for no paths.
    pub(super) fn show_open_folders(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        match &mut self.open_folder_dialog {
            Some(dialog) => dialog.add_folders(paths, &self.workspace_store),
            None => {
                self.open_folder_dialog = OpenFolderDialog::new(
                    paths,
                    self.configured_shell(),
                    self.available_shells.clone(),
                    &self.workspace_store,
                );
            }
        }
    }

    /// Opens every folder in `dialog`: saves or updates its workspace when
    /// ticked, and starts a terminal in it. The first folder ends up focused
    /// (its workspace shown), as the only one did before several could be
    /// picked.
    pub(super) fn open_folders(&mut self, dialog: OpenFolderDialog) {
        self.remember_shell_choice(&dialog.selected_shell);
        let mut first: Option<(Option<u64>, Option<u32>)> = None;
        for entry in &dialog.entries {
            let ws_id = entry
                .save_as_workspace
                .then(|| self.save_folder_workspace(entry));
            let pane_id =
                self.spawn_terminal_pane(&dialog.selected_shell, Some(entry.path.clone()));
            first.get_or_insert((ws_id, pane_id));
        }
        let Some((ws_id, pane_id)) = first else {
            return;
        };
        if dialog.entries.len() > 1 {
            if let Some(pid) = pane_id {
                self.activate_and_scroll_to_pane(pid);
            }
        }
        if let Some(ws_id) = ws_id {
            self.navigate_to_workspace(ws_id);
        }
    }

    /// Saves the shell picked in the dialog as the default, but only when
    /// a default is set and this differs from it.
    fn remember_shell_choice(&mut self, shell: &ShellKind) {
        let chosen = shell.display_name();
        if self
            .settings
            .default_shell
            .as_deref()
            .is_some_and(|d| d != chosen)
        {
            self.settings.default_shell = Some(chosen.to_string());
            self.settings.save();
        }
    }

    /// Creates the workspace `entry` describes, or renames and recolours the
    /// one it already is. Returns its id.
    fn save_folder_workspace(&mut self, entry: &FolderEntry) -> u64 {
        let name = entry.name.trim().to_string();
        let color = entry.color.color();
        let Some(id) = entry.existing_workspace_id else {
            let id = self.workspace_store.next_id();
            self.workspace_store.workspaces.push(Workspace {
                id,
                name,
                path: entry.path.clone(),
                color,
                host_window_id: None,
                last_activated: 0,
            });
            self.workspace_store.save();
            return id;
        };
        let mut renamed_from = None;
        if let Some(ws) = self
            .workspace_store
            .workspaces
            .iter_mut()
            .find(|w| w.id == id)
        {
            if ws.name != name {
                renamed_from = Some(std::mem::replace(&mut ws.name, name.clone()));
            }
            ws.color = color;
        }
        self.workspace_store.save();
        if let Some(old) = renamed_from {
            self.note_store.rename_file(id, &old, &name);
        }
        id
    }

    /// Whether `path` opens showing its preview: Markdown does when the user
    /// last preferred previews.
    pub(super) fn default_editor_preview(&self, path: &Path) -> bool {
        let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");
        is_md && self.md_prefer_preview
    }

    /// Focuses the editor already showing `path` in this workspace, or opens
    /// a new one in the focused split and reads the file on a background
    /// thread. Returns the editor's pane.
    pub(super) fn open_file_in_editor(
        &mut self,
        path: PathBuf,
        show_preview: bool,
        ctx: &egui::Context,
    ) -> u32 {
        if let Some(pid) = self.pane_state.find_file_editor(&path, self.active_group) {
            self.activate_and_scroll_to_pane(pid);
            return pid;
        }
        let pane_id = self.pane_state.next_pane_id;
        self.pane_state.next_pane_id += 1;
        self.pane_state.panes.push(PaneEntry {
            id: pane_id,
            content: PaneContent::FileEditor(FileEditorState {
                path: path.clone(),
                content: String::new(),
                dirty: false,
                save_error: false,
                workspace_id: self.active_group,
                show_preview,
                stale: false,
                loading: true,
                untitled: false,
                saved_text_hash: None,
            }),
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
        self.pane_state
            .add_pane_to_group(self.pane_state.focused_group_id, pane_id, None);
        self.activate_and_scroll_to_pane(pane_id);
        let results = Arc::clone(&self.file_load_results);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let content = match std::fs::read(&path) {
                Ok(bytes) => String::from_utf8(bytes)
                    .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
                Err(_) => String::new(),
            };
            results.lock().push((pane_id, content));
            ctx.request_repaint();
        });
        pane_id
    }
}
