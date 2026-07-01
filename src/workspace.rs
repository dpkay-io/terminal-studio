use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

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

// ── Per-workspace notes (file-backed) ────────────────────────────────────────

const NOTE_CHECK_INTERVAL: Duration = Duration::from_secs(2);
const GENERAL_NOTE_FILENAME: &str = "general.md";

struct CachedNote {
    text: String,
    disk_mtime: Option<SystemTime>,
}

#[derive(Default)]
pub struct NoteStore {
    notes_dir: Option<PathBuf>,
    cache: HashMap<Option<u64>, CachedNote>,
    last_check: Option<Instant>,
}

impl NoteStore {
    pub fn load(workspaces: &[Workspace]) -> Self {
        let notes_dir = util::data_dir().map(|d| d.join("notes"));
        if let Some(ref dir) = notes_dir {
            let _ = std::fs::create_dir_all(dir);
        }

        let mut store = Self {
            notes_dir,
            cache: HashMap::new(),
            last_check: Some(Instant::now()),
        };
        store.migrate_from_json(workspaces);
        store.scan_files(workspaces);
        store
    }

    pub fn get(&self, group: Option<u64>) -> &str {
        self.cache
            .get(&group)
            .map(|c| c.text.as_str())
            .unwrap_or("")
    }

    pub fn set(&mut self, group: Option<u64>, text: String, workspaces: &[Workspace]) {
        let Some(path) = self.resolve_path(group, workspaces) else {
            return;
        };
        if text.is_empty() {
            let _ = std::fs::remove_file(&path);
            self.cache.remove(&group);
        } else {
            if let Err(e) = util::atomic_write(&path, &text) {
                log::error!("failed to save note: {e}");
                return;
            }
            let mtime = file_mtime(&path);
            self.cache.insert(
                group,
                CachedNote {
                    text,
                    disk_mtime: mtime,
                },
            );
        }
    }

    /// Check for external edits. Call once per frame; internally throttled.
    pub fn check_external_changes(&mut self, workspaces: &[Workspace]) {
        if let Some(last) = self.last_check {
            if last.elapsed() < NOTE_CHECK_INTERVAL {
                return;
            }
        }
        self.last_check = Some(Instant::now());

        let Some(ref dir) = self.notes_dir else {
            return;
        };

        // Re-check cached entries
        let groups: Vec<Option<u64>> = self.cache.keys().copied().collect();
        for group in groups {
            let path = resolve_path_in(dir, group, workspaces);
            let current_mtime = file_mtime(&path);
            let cached_mtime = self.cache.get(&group).and_then(|c| c.disk_mtime);
            if current_mtime != cached_mtime {
                match std::fs::read_to_string(&path) {
                    Ok(text) if !text.is_empty() => {
                        self.cache.insert(
                            group,
                            CachedNote {
                                text,
                                disk_mtime: current_mtime,
                            },
                        );
                    }
                    _ => {
                        self.cache.remove(&group);
                    }
                }
            }
        }

        // Detect new files for workspaces not yet in cache
        for ws in workspaces {
            let group = Some(ws.id);
            if self.cache.contains_key(&group) {
                continue;
            }
            let path = dir.join(format!("{}.md", sanitize_name(&ws.name)));
            if let Ok(text) = std::fs::read_to_string(&path) {
                if !text.is_empty() {
                    self.cache.insert(
                        group,
                        CachedNote {
                            text,
                            disk_mtime: file_mtime(&path),
                        },
                    );
                }
            }
        }

        // Detect new general.md
        if let std::collections::hash_map::Entry::Vacant(e) = self.cache.entry(None) {
            let path = dir.join(GENERAL_NOTE_FILENAME);
            if let Ok(text) = std::fs::read_to_string(&path) {
                if !text.is_empty() {
                    e.insert(CachedNote {
                        text,
                        disk_mtime: file_mtime(&path),
                    });
                }
            }
        }
    }

    pub fn rename_file(&mut self, ws_id: u64, old_name: &str, new_name: &str) {
        let Some(ref dir) = self.notes_dir else {
            return;
        };
        let old_slug = sanitize_name(old_name);
        let new_slug = sanitize_name(new_name);
        if old_slug == new_slug {
            return;
        }
        let old_path = dir.join(format!("{old_slug}.md"));
        let new_path = dir.join(format!("{new_slug}.md"));
        if old_path.exists() {
            if let Err(e) = std::fs::rename(&old_path, &new_path) {
                log::error!("failed to rename note file: {e}");
                return;
            }
            if let Some(cached) = self.cache.get_mut(&Some(ws_id)) {
                cached.disk_mtime = file_mtime(&new_path);
            }
        }
    }

    fn resolve_path(&self, group: Option<u64>, workspaces: &[Workspace]) -> Option<PathBuf> {
        self.notes_dir
            .as_ref()
            .map(|dir| resolve_path_in(dir, group, workspaces))
    }

    fn scan_files(&mut self, workspaces: &[Workspace]) {
        let Some(ref dir) = self.notes_dir else {
            return;
        };

        // General notes
        let path = dir.join(GENERAL_NOTE_FILENAME);
        if let Ok(text) = std::fs::read_to_string(&path) {
            if !text.is_empty() {
                self.cache.entry(None).or_insert(CachedNote {
                    disk_mtime: file_mtime(&path),
                    text,
                });
            }
        }

        // Workspace notes
        for ws in workspaces {
            let path = dir.join(format!("{}.md", sanitize_name(&ws.name)));
            if let Ok(text) = std::fs::read_to_string(&path) {
                if !text.is_empty() {
                    self.cache.entry(Some(ws.id)).or_insert(CachedNote {
                        disk_mtime: file_mtime(&path),
                        text,
                    });
                }
            }
        }
    }

    fn migrate_from_json(&mut self, workspaces: &[Workspace]) {
        let Some(json_path) = util::data_file("notes.json") else {
            return;
        };
        if !json_path.exists() {
            return;
        }
        let Ok(data) = std::fs::read_to_string(&json_path) else {
            return;
        };

        // Legacy format: { "notes": { "<id_or_other>": "<text>" } }
        #[derive(Deserialize)]
        struct Legacy {
            notes: HashMap<String, String>,
        }
        let Ok(legacy) = serde_json::from_str::<Legacy>(&data) else {
            return;
        };

        for (key, text) in legacy.notes {
            if text.is_empty() {
                continue;
            }
            let group = if key == "other" {
                None
            } else if let Ok(id) = key.parse::<u64>() {
                Some(id)
            } else {
                continue;
            };

            if let Some(path) = self.resolve_path(group, workspaces) {
                if let Err(e) = util::atomic_write(&path, &text) {
                    log::error!("migration: failed to write {}: {e}", path.display());
                    continue;
                }
                self.cache.insert(
                    group,
                    CachedNote {
                        disk_mtime: file_mtime(&path),
                        text,
                    },
                );
            }
        }

        let bak = json_path.with_extension("json.bak");
        if let Err(e) = std::fs::rename(&json_path, &bak) {
            log::warn!("could not rename notes.json to .bak: {e}");
        }
    }
}

// ── Note helpers ─────────────────────────────────────────────────────────────

fn resolve_path_in(dir: &Path, group: Option<u64>, workspaces: &[Workspace]) -> PathBuf {
    match group {
        None => dir.join(GENERAL_NOTE_FILENAME),
        Some(id) => {
            let slug = workspaces
                .iter()
                .find(|w| w.id == id)
                .map(|w| sanitize_name(&w.name))
                .unwrap_or_else(|| format!("orphan_{id}"));
            dir.join(format!("{slug}.md"))
        }
    }
}

// ponytail: no collision guard — workspace names are unique (case-insensitive),
// so sanitized slugs are unique in practice. Add _<id> suffix if collisions arise.
pub fn sanitize_name(name: &str) -> String {
    let mut prev_dash = true; // suppresses leading dash
    let mut slug = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' {
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
        slug.push_str("unnamed");
    }
    slug
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
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

    fn make_note_store(dir: &std::path::Path) -> NoteStore {
        NoteStore {
            notes_dir: Some(dir.to_path_buf()),
            cache: HashMap::new(),
            last_check: None,
        }
    }

    fn make_workspaces(names: &[(&str, u64)]) -> Vec<Workspace> {
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
    fn test_note_store_get_empty() {
        let store = NoteStore::default();
        assert_eq!(store.get(None), "");
        assert_eq!(store.get(Some(1)), "");
    }

    #[test]
    fn test_note_store_set_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_note_store(dir.path());
        let ws = make_workspaces(&[("proj", 42)]);
        store.set(None, "general note".to_string(), &ws);
        assert_eq!(store.get(None), "general note");
        store.set(Some(42), "ws note".to_string(), &ws);
        assert_eq!(store.get(Some(42)), "ws note");
    }

    #[test]
    fn test_note_store_set_empty_removes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_note_store(dir.path());
        let ws = make_workspaces(&[("proj", 1)]);
        store.set(Some(1), "hello".to_string(), &ws);
        assert_eq!(store.get(Some(1)), "hello");
        assert!(dir.path().join("proj.md").exists());
        store.set(Some(1), String::new(), &ws);
        assert_eq!(store.get(Some(1)), "");
        assert!(!dir.path().join("proj.md").exists());
    }

    #[test]
    fn test_note_store_groups_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_note_store(dir.path());
        let ws = make_workspaces(&[("ws1", 1), ("ws2", 2)]);
        store.set(None, "other".to_string(), &ws);
        store.set(Some(1), "note1".to_string(), &ws);
        store.set(Some(2), "note2".to_string(), &ws);
        assert_eq!(store.get(None), "other");
        assert_eq!(store.get(Some(1)), "note1");
        assert_eq!(store.get(Some(2)), "note2");
    }

    #[test]
    fn test_note_store_writes_correct_filenames() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_note_store(dir.path());
        let ws = make_workspaces(&[("My Project", 1)]);
        store.set(None, "general".to_string(), &ws);
        store.set(Some(1), "proj note".to_string(), &ws);
        assert!(dir.path().join("general.md").exists());
        assert!(dir.path().join("my-project.md").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("my-project.md")).unwrap(),
            "proj note"
        );
    }

    #[test]
    fn test_note_store_external_edit_detection() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_note_store(dir.path());
        let ws = make_workspaces(&[("proj", 1)]);
        store.set(Some(1), "original".to_string(), &ws);
        assert_eq!(store.get(Some(1)), "original");

        // Simulate external edit
        std::fs::write(dir.path().join("proj.md"), "edited externally").unwrap();
        store.last_check = None; // force check
        store.check_external_changes(&ws);
        assert_eq!(store.get(Some(1)), "edited externally");
    }

    #[test]
    fn test_note_store_external_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_note_store(dir.path());
        let ws = make_workspaces(&[("proj", 1)]);
        assert_eq!(store.get(Some(1)), "");

        std::fs::write(dir.path().join("proj.md"), "created externally").unwrap();
        store.last_check = None;
        store.check_external_changes(&ws);
        assert_eq!(store.get(Some(1)), "created externally");
    }

    #[test]
    fn test_note_store_rename_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_note_store(dir.path());
        let ws = make_workspaces(&[("old-name", 1)]);
        store.set(Some(1), "my note".to_string(), &ws);
        assert!(dir.path().join("old-name.md").exists());

        store.rename_file(1, "old-name", "new-name");
        assert!(!dir.path().join("old-name.md").exists());
        assert!(dir.path().join("new-name.md").exists());
        assert_eq!(store.get(Some(1)), "my note");
    }

    #[test]
    fn test_note_store_scan_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("general.md"), "general note").unwrap();
        std::fs::write(dir.path().join("proj.md"), "proj note").unwrap();

        let ws = make_workspaces(&[("proj", 1)]);
        let mut store = make_note_store(dir.path());
        store.scan_files(&ws);
        assert_eq!(store.get(None), "general note");
        assert_eq!(store.get(Some(1)), "proj note");
    }

    #[test]
    fn test_note_store_orphan_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = make_note_store(dir.path());
        let ws: Vec<Workspace> = vec![];
        store.set(Some(99), "orphan".to_string(), &ws);
        assert!(dir.path().join("orphan_99.md").exists());
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
