use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::util;

/// Opaque identifier for an extra OS window (viewport).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindowId(pub u64);

#[derive(Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: u64,
    pub name: String,
    pub path: PathBuf,
    pub color: [u8; 3],
    /// If `Some`, this workspace is currently hosted in an extra OS window.
    /// `None` means it lives in the main window.
    #[serde(default)]
    pub host_window_id: Option<WindowId>,
    /// Epoch-millis timestamp of last activation (for switcher sort order).
    #[serde(default)]
    pub last_activated: u64,
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
        util::safe_json_load(&path).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::data_path() else {
            return;
        };
        if let Ok(text) = serde_json::to_string_pretty(self) {
            if let Err(e) = util::atomic_write(&path, &text) {
                log::error!("failed to save data: {e}");
            }
        }
    }

    /// The workspace with this id, if it still exists.
    pub fn find(&self, id: u64) -> Option<&Workspace> {
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn find_for_path(&self, path: &Path) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .find(|w| util::paths_equal(&w.path, path))
    }

    /// Finds the most specific workspace whose path is a prefix of `cwd`.
    pub fn find_for_cwd(&self, cwd: &Path) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .filter(|w| util::path_starts_with(cwd, &w.path))
            .max_by_key(|w| w.path.components().count())
    }

    pub fn next_id(&self) -> u64 {
        self.workspaces.iter().map(|w| w.id).max().unwrap_or(0) + 1
    }

    pub fn is_name_taken(&self, name: &str, exclude_id: Option<u64>) -> bool {
        self.workspaces
            .iter()
            .any(|w| w.name.eq_ignore_ascii_case(name) && exclude_id != Some(w.id))
    }

    /// `base`, or `base 2`, `base 3`, … — the first that no workspace uses.
    pub fn unique_name(&self, base: &str) -> String {
        if !self.is_name_taken(base, None) {
            return base.to_string();
        }
        // Every workspace can take at most one candidate, so one more suffix
        // than there are workspaces is always enough.
        (2..=self.workspaces.len() + 1)
            .map(|n| format!("{base} {n}"))
            .find(|name| !self.is_name_taken(name, None))
            .unwrap_or_else(|| format!("{base} {}", self.workspaces.len() + 2))
    }

    #[allow(dead_code)]
    pub fn is_color_taken(&self, color: [u8; 3], exclude_id: Option<u64>) -> bool {
        self.workspaces
            .iter()
            .any(|w| w.color == color && (exclude_id != Some(w.id)))
    }

    fn data_path() -> Option<PathBuf> {
        util::data_file("workspaces.json")
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
                host_window_id: None,
                last_activated: 0,
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
    fn find_by_id_only_finds_workspaces_that_exist() {
        let store = make_store(vec![("A", "/a"), ("B", "/b")]);
        assert_eq!(store.find(2).map(|w| w.name.as_str()), Some("B"));
        assert!(store.find(3).is_none());
        assert!(WorkspaceStore::default().find(1).is_none());
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
    fn test_next_id_after_deletion() {
        // Simulate: created workspaces 1,2,3 then deleted workspace 2.
        // next_id must return max(1,3)+1 = 4, not len()+1 = 3 (which would collide).
        let mut store = make_store(vec![("A", "/a"), ("B", "/b"), ("C", "/c")]);
        store.workspaces.retain(|w| w.id != 2);
        assert_eq!(store.workspaces.len(), 2);
        assert_eq!(store.next_id(), 4);
    }

    #[test]
    fn test_next_id_after_deletion_keeps_only_highest() {
        // Only the highest-id workspace remains after deleting everything else.
        let mut store = make_store(vec![("A", "/a"), ("B", "/b"), ("C", "/c")]);
        store.workspaces.retain(|w| w.id == 3);
        assert_eq!(store.workspaces.len(), 1);
        assert_eq!(store.next_id(), 4);
    }

    #[test]
    fn test_is_name_taken_exact() {
        let store = make_store(vec![("MyProject", "/a")]);
        assert!(store.is_name_taken("MyProject", None));
        assert!(!store.is_name_taken("Other", None));
    }

    #[test]
    fn test_is_name_taken_case_insensitive() {
        let store = make_store(vec![("MyProject", "/a")]);
        assert!(store.is_name_taken("myproject", None));
        assert!(store.is_name_taken("MYPROJECT", None));
    }

    #[test]
    fn test_is_name_taken_excludes_self() {
        let store = make_store(vec![("MyProject", "/a")]);
        assert!(!store.is_name_taken("MyProject", Some(1)));
        assert!(store.is_name_taken("MyProject", Some(99)));
    }

    #[test]
    fn unique_name_skips_every_taken_suffix() {
        let store = make_store(vec![("app", "/a"), ("App 2", "/b"), ("other", "/c")]);
        assert_eq!(store.unique_name("fresh"), "fresh");
        assert_eq!(store.unique_name("app"), "app 3");
    }

    #[test]
    fn test_is_color_taken() {
        let mut store = WorkspaceStore::default();
        store.workspaces.push(Workspace {
            id: 1,
            name: "A".to_string(),
            path: PathBuf::from("/a"),
            color: [100, 140, 230],
            host_window_id: None,
            last_activated: 0,
        });
        assert!(store.is_color_taken([100, 140, 230], None));
        assert!(!store.is_color_taken([0, 0, 0], None));
        assert!(!store.is_color_taken([100, 140, 230], Some(1)));
    }
}
