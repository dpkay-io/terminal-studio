//! The files notes are linked to, kept in `notes/links.json` beside the
//! app's own note files: `{ "general": "/path", "<workspace id>": "/path" }`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::target::NoteKey;

/// Name of the file links are kept in, inside the notes folder.
pub const LINKS_FILENAME: &str = "links.json";
/// Key of the "Other" group's note in the file.
const GENERAL_KEY: &str = "general";

/// Each linked note's file. A note missing here uses the app's own file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoteLinks(HashMap<NoteKey, PathBuf>);

impl NoteLinks {
    /// Links saved in `path`; none when it is missing or unreadable (logged).
    /// Entries whose key is neither `general` nor a workspace id are skipped.
    pub fn load(path: &Path) -> Self {
        crate::util::safe_json_load::<HashMap<String, PathBuf>>(path)
            .map(Self::from_saved)
            .unwrap_or_default()
    }

    pub fn get(&self, key: NoteKey) -> Option<&Path> {
        self.0.get(&key).map(PathBuf::as_path)
    }

    /// Links `key` to `path`, or back to its own file with `None`.
    pub fn set(&mut self, key: NoteKey, path: Option<PathBuf>) {
        match path {
            Some(path) => self.0.insert(key, path),
            None => self.0.remove(&key),
        };
    }

    /// The text of `links.json`, keys sorted so the file only changes when
    /// a link does.
    pub fn to_json(&self) -> String {
        let saved: std::collections::BTreeMap<String, &PathBuf> =
            self.0.iter().map(|(k, p)| (key_to_saved(*k), p)).collect();
        serde_json::to_string_pretty(&saved).unwrap_or_else(|e| {
            log::error!("could not serialize note links: {e}");
            String::from("{}")
        })
    }

    fn from_saved(saved: HashMap<String, PathBuf>) -> Self {
        Self(
            saved
                .into_iter()
                .filter_map(|(k, p)| match saved_to_key(&k) {
                    Some(key) => Some((key, p)),
                    None => {
                        log::warn!("note links: skipped unknown key {k:?}");
                        None
                    }
                })
                .collect(),
        )
    }
}

fn key_to_saved(key: NoteKey) -> String {
    match key {
        None => GENERAL_KEY.to_string(),
        Some(id) => id.to_string(),
    }
}

/// `None` = not a key: the outer `Option` is "recognised", the inner the
/// note's key.
fn saved_to_key(saved: &str) -> Option<NoteKey> {
    if saved == GENERAL_KEY {
        return Some(None);
    }
    saved.parse::<u64>().ok().map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_survive_a_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LINKS_FILENAME);
        let mut links = NoteLinks::default();
        links.set(None, Some("/vault/general.md".into()));
        links.set(Some(7), Some("/vault/proj.md".into()));
        std::fs::write(&path, links.to_json()).unwrap();
        let loaded = NoteLinks::load(&path);
        assert_eq!(loaded, links);
        assert_eq!(loaded.get(Some(7)), Some(Path::new("/vault/proj.md")));
        assert_eq!(loaded.get(None), Some(Path::new("/vault/general.md")));
        assert_eq!(loaded.get(Some(8)), None);
    }

    #[test]
    fn unlinking_removes_the_entry() {
        let mut links = NoteLinks::default();
        links.set(Some(1), Some("/a.md".into()));
        links.set(Some(1), None);
        assert_eq!(links.get(Some(1)), None);
        assert_eq!(links, NoteLinks::default());
    }

    #[test]
    fn a_missing_or_broken_file_has_no_links() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LINKS_FILENAME);
        assert_eq!(NoteLinks::load(&path), NoteLinks::default());
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(NoteLinks::load(&path), NoteLinks::default());
    }

    #[test]
    fn unknown_keys_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(LINKS_FILENAME);
        std::fs::write(&path, r#"{"3": "/a.md", "nope": "/b.md"}"#).unwrap();
        let links = NoteLinks::load(&path);
        let mut only_known = NoteLinks::default();
        only_known.set(Some(3), Some("/a.md".into()));
        assert_eq!(links, only_known);
    }

    #[test]
    fn saved_text_is_stable() {
        let mut a = NoteLinks::default();
        let mut b = NoteLinks::default();
        for id in 1..20 {
            a.set(Some(id), Some(format!("/{id}.md").into()));
        }
        for id in (1..20).rev() {
            b.set(Some(id), Some(format!("/{id}.md").into()));
        }
        assert_eq!(a.to_json(), b.to_json());
    }
}
