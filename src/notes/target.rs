//! Which file holds a note: the app's own file in its notes folder, or a
//! file the user linked (an Obsidian vault page, a text file anywhere).

use std::path::{Path, PathBuf};

use crate::workspace::Workspace;

/// A note's owner: a workspace's id, or `None` for the notes of panes
/// outside every workspace (the "Other" group).
pub type NoteKey = Option<u64>;

/// File name of the "Other" group's own note.
pub const GENERAL_NOTE_FILENAME: &str = "general.md";
/// Extension of the app's own note files.
const NOTE_EXTENSION: &str = "md";
/// Name of the note of a workspace that no longer exists, before its id.
const ORPHAN_PREFIX: &str = "orphan_";
/// Added to an own note file set aside when its note is linked elsewhere.
const BACKUP_EXTENSION: &str = "bak";
/// A workspace name with nothing usable in a file name.
const UNNAMED_SLUG: &str = "unnamed";

/// Where a note is kept.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NoteTarget {
    pub path: PathBuf,
    /// A file the user chose. It is never deleted or set aside, and an
    /// emptied note leaves it empty rather than removing it.
    pub linked: bool,
}

impl NoteTarget {
    pub fn own(path: PathBuf) -> Self {
        Self {
            path,
            linked: false,
        }
    }

    pub fn linked(path: PathBuf) -> Self {
        Self { path, linked: true }
    }
}

/// The app's own file for `key` in `notes_dir`: `general.md`, the
/// workspace's name as a file name, or `orphan_<id>.md` for a workspace that
/// no longer exists.
pub fn own_note_path(notes_dir: &Path, key: NoteKey, workspaces: &[Workspace]) -> PathBuf {
    match key {
        None => notes_dir.join(GENERAL_NOTE_FILENAME),
        Some(id) => {
            let stem = workspaces
                .iter()
                .find(|w| w.id == id)
                .map(|w| sanitize_name(&w.name))
                .unwrap_or_else(|| format!("{ORPHAN_PREFIX}{id}"));
            notes_dir.join(format!("{stem}.{NOTE_EXTENSION}"))
        }
    }
}

/// Where an own note file is set aside once its note moves to a linked file:
/// beside it, as `<name>.md.bak` (one per note; a later one replaces it).
pub fn backup_path(own_path: &Path) -> PathBuf {
    let mut name = own_path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(BACKUP_EXTENSION);
    own_path.with_file_name(name)
}

/// A workspace name as a file name: letters, digits and `_` kept
/// (ASCII ones lowercased), every other run of characters one `-`.
///
/// Workspace names are unique ignoring case, so their file names are unique
/// in practice.
pub fn sanitize_name(name: &str) -> String {
    let mut prev_dash = true; // suppresses a leading dash
    let mut slug = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' {
            // ASCII only, as before: other letters keep their case, so
            // existing note files keep their names.
            slug.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str(UNNAMED_SLUG);
    }
    slug
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn workspaces(names: &[(&str, u64)]) -> Vec<Workspace> {
        names
            .iter()
            .map(|(name, id)| Workspace {
                id: *id,
                name: name.to_string(),
                path: PathBuf::from(format!("/{name}")),
                color: [0, 0, 0],
                host_window_id: None,
                last_activated: 0,
            })
            .collect()
    }

    #[test]
    fn own_paths_are_named_after_the_workspace() {
        let dir = Path::new("/notes");
        let ws = workspaces(&[("My Project", 1)]);
        assert_eq!(own_note_path(dir, None, &ws), dir.join("general.md"));
        assert_eq!(own_note_path(dir, Some(1), &ws), dir.join("my-project.md"));
        assert_eq!(own_note_path(dir, Some(99), &ws), dir.join("orphan_99.md"));
    }

    #[test]
    fn backups_sit_beside_the_own_file() {
        assert_eq!(
            backup_path(Path::new("/notes/proj.md")),
            PathBuf::from("/notes/proj.md.bak")
        );
    }

    #[test]
    fn targets_say_whether_the_user_chose_them() {
        assert!(!NoteTarget::own("/a".into()).linked);
        assert!(NoteTarget::linked("/a".into()).linked);
    }

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("My Project"), "my-project");
        assert_eq!(sanitize_name("hello_world"), "hello_world");
        assert_eq!(sanitize_name("CAPS"), "caps");
        assert_eq!(sanitize_name("a--b"), "a-b");
        assert_eq!(sanitize_name("  spaces  "), "spaces");
        assert_eq!(sanitize_name("foo/bar\\baz"), "foo-bar-baz");
        assert_eq!(sanitize_name(""), "unnamed");
        assert_eq!(sanitize_name("---"), "unnamed");
        // unicode letters are alphanumeric — kept as-is (lowercased)
        assert_eq!(sanitize_name("café"), "café");
        assert_eq!(sanitize_name("проект"), "проект");
    }
}
