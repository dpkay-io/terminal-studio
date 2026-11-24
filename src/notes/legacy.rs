//! One-off move of the old `notes.json` into note files, at startup before
//! the note worker starts.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::target::{own_note_path, NoteKey};
use crate::workspace::Workspace;

const LEGACY_FILENAME: &str = "notes.json";
/// The old file's key for the "Other" group's note.
const LEGACY_OTHER_KEY: &str = "other";
/// What the old file is renamed to once moved.
const LEGACY_BACKUP_EXTENSION: &str = "json.bak";

/// Legacy format: `{ "notes": { "<id_or_other>": "<text>" } }`.
#[derive(Deserialize)]
struct Legacy {
    notes: HashMap<String, String>,
}

/// Writes each note of `notes.json` (in the app's data folder) to its file
/// in `notes_dir`, then renames `notes.json` to `notes.json.bak`.
pub fn migrate_from_json(notes_dir: &Path, workspaces: &[Workspace]) {
    let Some(json_path) = crate::util::data_file(LEGACY_FILENAME) else {
        return;
    };
    migrate_file(&json_path, notes_dir, workspaces);
}

fn migrate_file(json_path: &Path, notes_dir: &Path, workspaces: &[Workspace]) {
    let Ok(data) = std::fs::read_to_string(json_path) else {
        return;
    };
    let Ok(legacy) = serde_json::from_str::<Legacy>(&data) else {
        return;
    };
    for (saved_key, text) in legacy.notes {
        if text.is_empty() {
            continue;
        }
        let Some(key) = legacy_key(&saved_key) else {
            continue;
        };
        let path = own_note_path(notes_dir, key, workspaces);
        if let Err(e) = crate::util::atomic_write(&path, &text) {
            log::error!("migration: failed to write {}: {e}", path.display());
        }
    }
    let bak = json_path.with_extension(LEGACY_BACKUP_EXTENSION);
    if let Err(e) = std::fs::rename(json_path, &bak) {
        log::warn!("could not rename {LEGACY_FILENAME} to .bak: {e}");
    }
}

fn legacy_key(saved: &str) -> Option<NoteKey> {
    if saved == LEGACY_OTHER_KEY {
        return Some(None);
    }
    saved.parse::<u64>().ok().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::target::tests::workspaces;

    #[test]
    fn old_notes_move_into_files_and_the_old_file_is_kept_as_bak() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join(LEGACY_FILENAME);
        std::fs::write(
            &json,
            r#"{"notes": {"other": "general text", "4": "proj text", "x": "skipped", "5": ""}}"#,
        )
        .unwrap();
        let notes_dir = dir.path().join("notes");
        migrate_file(&json, &notes_dir, &workspaces(&[("Proj", 4)]));
        let read = |name: &str| std::fs::read_to_string(notes_dir.join(name)).ok();
        assert_eq!(read("general.md").as_deref(), Some("general text"));
        assert_eq!(read("proj.md").as_deref(), Some("proj text"));
        assert_eq!(read("orphan_5.md"), None, "empty notes are not written");
        assert!(!json.exists());
        assert!(dir.path().join("notes.json.bak").exists());
    }

    #[test]
    fn a_broken_old_file_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join(LEGACY_FILENAME);
        std::fs::write(&json, "nope").unwrap();
        migrate_file(&json, dir.path(), &[]);
        assert!(json.exists());
    }
}
