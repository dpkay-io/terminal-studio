//! Linking a workspace's notes to a file of the user's choosing, so they can
//! be edited in other apps (Obsidian, Notepad++…) too: the notes headers'
//! link menu, the file pickers (on their own thread) and what was picked.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;

use super::feedback::{FlashKind, FlashTarget};
use super::ui::notes_header::{NoteLinkAction, NotesHeaderInfo};
use super::App;
use crate::notes::NoteKey;

/// Name of the thread a note's file picker runs on (it blocks until closed).
const PICKER_THREAD: &str = "note-file-picker";
/// The picker's title when linking an existing file.
const PICK_EXISTING_TITLE: &str = "Link Notes to a File";
/// The picker's title when creating a file to link.
const PICK_NEW_TITLE: &str = "Create a File for These Notes";
/// Suffix of the file name suggested for a new linked file.
const NEW_FILE_SUFFIX: &str = " notes.md";
/// The pickers' file filter: what notes can be kept in.
const NOTE_FILE_FILTER: &str = "Markdown or Text";

/// Which picker to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickKind {
    Existing,
    New,
}

/// Files picked on the picker thread, waiting for a frame to link them.
#[derive(Clone, Default)]
pub(super) struct NoteFilePicks(Arc<Mutex<Vec<(NoteKey, PathBuf)>>>);

impl NoteFilePicks {
    fn push(&self, key: NoteKey, path: PathBuf, ctx: &egui::Context) {
        self.0.lock().push((key, path));
        ctx.request_repaint();
    }

    /// The files picked since last asked, oldest first. Cheap when there
    /// are none, as on almost every frame.
    fn take(&self) -> Vec<(NoteKey, PathBuf)> {
        let mut picks = self.0.lock();
        if picks.is_empty() {
            return Vec::new();
        }
        std::mem::take(&mut *picks)
    }
}

impl App {
    /// What a notes header shows for `key`.
    pub(super) fn notes_header_info(&self, key: NoteKey) -> NotesHeaderInfo {
        NotesHeaderInfo {
            key,
            title: self.note_title(key),
            link: self.note_store.link(key).map(|p| p.to_path_buf()),
            file: self.note_store.file(key).map(|p| p.to_path_buf()),
            status: self.note_store.status(key),
        }
    }

    /// Runs what the notes headers' link menus asked for this frame.
    pub(super) fn run_note_link_actions(
        &mut self,
        actions: Vec<NoteLinkAction>,
        ctx: &egui::Context,
    ) {
        for action in actions {
            match action {
                NoteLinkAction::PickExisting(key) => {
                    self.pick_note_file(key, PickKind::Existing, ctx)
                }
                NoteLinkAction::PickNew(key) => self.pick_note_file(key, PickKind::New, ctx),
                NoteLinkAction::Unlink(key) => {
                    self.note_store.set_link(key, None);
                }
                NoteLinkAction::CopyPath(path) => {
                    ctx.copy_text(path.display().to_string());
                    self.flash.trigger(FlashTarget::Global, FlashKind::Success);
                }
                NoteLinkAction::Reveal(path) => crate::util::reveal_in_file_manager(&path),
            }
        }
    }

    /// Links the files picked since the last frame.
    pub(super) fn link_picked_note_files(&mut self) {
        for (key, path) in self.note_file_picks.take() {
            self.note_store.set_link(key, Some(path));
        }
    }

    /// Opens the system file picker on its own thread; the file chosen
    /// comes back through `note_file_picks`.
    fn pick_note_file(&self, key: NoteKey, kind: PickKind, ctx: &egui::Context) {
        let current = self.note_store.link(key).map(|p| p.to_path_buf());
        let start_dir = current
            .as_deref()
            .and_then(crate::util::parent_dir)
            .or_else(|| {
                key.and_then(|id| self.workspace_store.find(id))
                    .map(|w| w.path.clone())
            });
        let file_name = format!("{}{NEW_FILE_SUFFIX}", self.note_owner_name(key));
        let picks = self.note_file_picks.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name(PICKER_THREAD.into())
            .spawn(move || {
                let extensions: Vec<&str> = crate::util::MARKDOWN_EXTENSIONS
                    .into_iter()
                    .chain(crate::util::PLAIN_TEXT_EXTENSIONS)
                    .collect();
                let mut dialog = rfd::FileDialog::new().add_filter(NOTE_FILE_FILTER, &extensions);
                if let Some(dir) = start_dir {
                    dialog = dialog.set_directory(dir);
                }
                let picked = match kind {
                    PickKind::Existing => dialog.set_title(PICK_EXISTING_TITLE).pick_file(),
                    PickKind::New => dialog
                        .set_title(PICK_NEW_TITLE)
                        .set_file_name(file_name)
                        .save_file(),
                };
                let Some(path) = picked else {
                    return;
                };
                // The file itself, not a link to it: writes replace the
                // file, which would turn a symlink into a plain copy.
                let path = std::fs::canonicalize(&path).unwrap_or(path);
                picks.push(key, path, &ctx);
            });
        if let Err(e) = spawned {
            log::error!("could not start the note file picker: {e}");
        }
    }

    /// The name a note's new file starts from: its workspace's, or "Other".
    fn note_owner_name(&self, key: NoteKey) -> String {
        key.and_then(|id| self.workspace_store.find(id))
            .map(|w| w.name.clone())
            .unwrap_or_else(|| super::ui::OTHER_GROUP_NAME.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_are_taken_once_in_order() {
        let picks = NoteFilePicks::default();
        let ctx = egui::Context::default();
        assert!(picks.take().is_empty());
        picks.push(Some(1), "/a.md".into(), &ctx);
        picks.clone().push(None, "/b.md".into(), &ctx);
        assert_eq!(
            picks.take(),
            vec![
                (Some(1), PathBuf::from("/a.md")),
                (None, PathBuf::from("/b.md"))
            ]
        );
        assert!(picks.take().is_empty());
    }
}
