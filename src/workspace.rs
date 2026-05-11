use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: u64,
    pub name: String,
    pub path: PathBuf,
    pub color: [u8; 3],
}

#[derive(Default, Serialize, Deserialize)]
pub struct WorkspaceStore {
    pub workspaces: Vec<Workspace>,
}

impl WorkspaceStore {
    pub fn load() -> Self {
        let Some(path) = Self::data_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::data_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }

    pub fn find_for_path(&self, path: &PathBuf) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| &w.path == path)
    }

    /// Finds the most specific workspace whose path is a prefix of `cwd`.
    pub fn find_for_cwd(&self, cwd: &Path) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .filter(|w| cwd.starts_with(&w.path))
            .max_by_key(|w| w.path.components().count())
    }

    pub fn next_id(&self) -> u64 {
        self.workspaces.iter().map(|w| w.id).max().unwrap_or(0) + 1
    }

    fn data_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            std::env::var("APPDATA").ok().map(|base| {
                PathBuf::from(base)
                    .join("terminal-studio")
                    .join("workspaces.json")
            })
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::env::var("HOME").ok().map(|base| {
                PathBuf::from(base)
                    .join(".config")
                    .join("terminal-studio")
                    .join("workspaces.json")
            })
        }
    }
}

// ── Per-workspace notes ───────────────────────────────────────────────────────

#[derive(Default, Serialize, Deserialize)]
pub struct NoteStore {
    // key: workspace id as decimal string, "other" for the None group
    notes: HashMap<String, String>,
}

impl NoteStore {
    pub fn load() -> Self {
        let Some(path) = Self::data_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::data_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }

    pub fn get(&self, group: Option<u64>) -> &str {
        self.notes
            .get(&Self::key(group))
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    pub fn set(&mut self, group: Option<u64>, text: String) {
        let key = Self::key(group);
        if text.is_empty() {
            self.notes.remove(&key);
        } else {
            self.notes.insert(key, text);
        }
    }

    fn key(group: Option<u64>) -> String {
        match group {
            Some(id) => id.to_string(),
            None => "other".to_string(),
        }
    }

    fn data_path() -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        {
            std::env::var("APPDATA").ok().map(|base| {
                PathBuf::from(base)
                    .join("terminal-studio")
                    .join("notes.json")
            })
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::env::var("HOME").ok().map(|base| {
                PathBuf::from(base)
                    .join(".config")
                    .join("terminal-studio")
                    .join("notes.json")
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_store(workspaces: Vec<(&str, &str)>) -> WorkspaceStore {
        let workspaces = workspaces
            .into_iter()
            .enumerate()
            .map(|(i, (name, path))| Workspace {
                id: i as u64 + 1,
                name: name.to_string(),
                path: PathBuf::from(path),
                color: [0, 0, 0],
            })
            .collect();
        WorkspaceStore { workspaces }
    }

    #[test]
    fn test_find_for_path_exact_match() {
        let store = make_store(vec![("A", "/home/user/proj")]);
        let result = store.find_for_path(&PathBuf::from("/home/user/proj"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "A");
    }

    #[test]
    fn test_find_for_path_no_match() {
        let store = make_store(vec![("A", "/home/user/proj")]);
        let result = store.find_for_path(&PathBuf::from("/home/user/other"));
        assert!(result.is_none());
    }

    #[test]
    fn test_find_for_cwd_most_specific_prefix() {
        let store = make_store(vec![
            ("Root", "/home/user"),
            ("Proj", "/home/user/projects/myapp"),
        ]);
        let cwd = PathBuf::from("/home/user/projects/myapp/src");
        let result = store.find_for_cwd(&cwd);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "Proj");
    }

    #[test]
    fn test_find_for_cwd_no_match() {
        let store = make_store(vec![("A", "/home/user")]);
        let result = store.find_for_cwd(&PathBuf::from("/tmp/other"));
        assert!(result.is_none());
    }

    #[test]
    fn test_next_id_empty() {
        let store = WorkspaceStore::default();
        assert_eq!(store.next_id(), 1);
    }

    #[test]
    fn test_next_id_with_items() {
        let store = make_store(vec![("A", "/a"), ("B", "/b")]);
        assert_eq!(store.next_id(), 3);
    }

    #[test]
    fn test_note_store_get_empty() {
        let store = NoteStore::default();
        assert_eq!(store.get(None), "");
        assert_eq!(store.get(Some(1)), "");
    }

    #[test]
    fn test_note_store_set_and_get() {
        let mut store = NoteStore::default();
        store.set(None, "general note".to_string());
        assert_eq!(store.get(None), "general note");
        store.set(Some(42), "ws note".to_string());
        assert_eq!(store.get(Some(42)), "ws note");
    }

    #[test]
    fn test_note_store_set_empty_removes() {
        let mut store = NoteStore::default();
        store.set(Some(1), "hello".to_string());
        assert_eq!(store.get(Some(1)), "hello");
        store.set(Some(1), String::new());
        assert_eq!(store.get(Some(1)), "");
    }

    #[test]
    fn test_note_store_groups_are_independent() {
        let mut store = NoteStore::default();
        store.set(None, "other".to_string());
        store.set(Some(1), "ws1".to_string());
        store.set(Some(2), "ws2".to_string());
        assert_eq!(store.get(None), "other");
        assert_eq!(store.get(Some(1)), "ws1");
        assert_eq!(store.get(Some(2)), "ws2");
    }
}
