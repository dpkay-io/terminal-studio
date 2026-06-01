use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::file_browser::{list_dir_entries, DirData};
use super::pane::SessionEntry;

pub(super) struct WatchState {
    pub(super) watcher: RecommendedWatcher,
    pub(super) watched: HashSet<PathBuf>,
    pub(super) git_dirs: HashMap<PathBuf, PathBuf>,
    pub(super) events: Arc<Mutex<Vec<Event>>>,
    pub(super) dir_data: HashMap<PathBuf, DirData>,
    pub(super) last_sync: Instant,
    pub(super) last_session_count: usize,
}

impl WatchState {
    pub(super) fn new(ctx: egui::Context) -> Option<Self> {
        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let ev = Arc::clone(&events);
        let watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                ev.lock().push(event);
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        })
        .ok()?;
        Some(WatchState {
            watcher,
            watched: HashSet::new(),
            git_dirs: HashMap::new(),
            events,
            dir_data: HashMap::new(),
            last_sync: Instant::now(),
            last_session_count: 0,
        })
    }

    pub(super) fn sync(&mut self, sessions: &[SessionEntry]) {
        let current: HashSet<PathBuf> = sessions
            .iter()
            .map(|e| e.session.read().cwd.clone())
            .filter(|p| {
                if p.as_os_str().is_empty() {
                    return false;
                }
                // Skip the is_dir() syscall for paths we already watch.
                self.watched.contains(p) || p.is_dir()
            })
            .collect();

        // Also remove watched dirs that no longer exist on disk (L22)
        let to_remove: Vec<PathBuf> = self
            .watched
            .iter()
            .filter(|d| !current.contains(*d) || !d.is_dir())
            .cloned()
            .collect();
        for dir in to_remove {
            let _ = self.watcher.unwatch(&dir);
            if let Some(gd) = self
                .git_dirs
                .iter()
                .find(|(_, v)| **v == dir)
                .map(|(k, _)| k.clone())
            {
                let _ = self.watcher.unwatch(&gd);
                self.git_dirs.remove(&gd);
            }
            self.watched.remove(&dir);
            self.dir_data.remove(&dir);
        }

        let to_add: Vec<PathBuf> = current
            .into_iter()
            .filter(|d| !self.watched.contains(d))
            .collect();
        for dir in to_add {
            if self.watcher.watch(&dir, RecursiveMode::Recursive).is_ok() {
                let gd = dir.join(".git");
                if gd.is_dir() && self.watcher.watch(&gd, RecursiveMode::NonRecursive).is_ok() {
                    self.git_dirs.insert(gd, dir.clone());
                }
                self.dir_data.insert(dir.clone(), DirData::new(&dir));
                self.watched.insert(dir);
            }
        }
    }

    pub(super) fn take_pending_git_refreshes(&mut self) -> Vec<PathBuf> {
        let now = Instant::now();
        let mut dirs = Vec::new();
        for (dir, data) in &mut self.dir_data {
            if data.git_refresh_at.map(|t| now >= t).unwrap_or(false) {
                data.git_refresh_at = None;
                dirs.push(dir.clone());
            }
        }
        dirs
    }

    pub(super) fn apply_git_result(&mut self, dir: &Path, diff: String, status: String) {
        if let Some(data) = self.dir_data.get_mut(dir) {
            data.git_diff = diff;
            data.git_status = status;
        }
    }

    pub(super) fn apply_unpushed_result(&mut self, dir: &Path, commits: Vec<(String, String)>) {
        if let Some(data) = self.dir_data.get_mut(dir) {
            data.git_unpushed = commits;
        }
    }

    pub(super) fn process_events(&mut self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let events: Vec<Event> = {
            let mut g = self.events.lock();
            std::mem::take(&mut *g)
        };

        if events.is_empty() {
            return (Vec::new(), Vec::new());
        }

        let now = Instant::now();
        let debounce = Duration::from_millis(500);
        let mut created_md: Vec<PathBuf> = Vec::new();
        let mut removed_md: Vec<PathBuf> = Vec::new();
        let mut dirs_needing_refresh: HashSet<PathBuf> = HashSet::new();

        for event in events {
            for path in &event.paths {
                // Walk up from the event path to find the watched root directory.
                // With recursive watching, events arrive from subdirectories too.
                let dir: PathBuf = if let Some(mut p) = path.parent().map(PathBuf::from) {
                    if self.git_dirs.contains_key(&p) {
                        self.git_dirs[&p].clone()
                    } else {
                        loop {
                            if self.watched.contains(&p) {
                                break p;
                            }
                            match p.parent() {
                                Some(parent) if parent != p => p = parent.to_path_buf(),
                                _ => break p, // will fail the dir_data lookup below
                            }
                        }
                    }
                } else {
                    continue;
                };
                let Some(data) = self.dir_data.get_mut(&dir) else {
                    continue;
                };
                let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");

                match &event.kind {
                    EventKind::Create(_) => {
                        if is_md && path.is_file() {
                            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                            if size <= 1_048_576 {
                                let content = std::fs::read_to_string(path).unwrap_or_default();
                                data.md_files.insert(path.clone(), Arc::new(content));
                                created_md.push(path.clone());
                            }
                        }
                        dirs_needing_refresh.insert(dir.clone());
                        if data.is_git {
                            data.git_refresh_at.get_or_insert(now + debounce);
                        }
                    }
                    EventKind::Modify(_) => {
                        if is_md && path.is_file() && data.md_files.contains_key(path) {
                            // Debounce: skip if file was modified very recently (still being written) (L23)
                            let recently_modified = std::fs::metadata(path)
                                .and_then(|m| m.modified())
                                .map(|t| {
                                    t.elapsed().unwrap_or_default() < Duration::from_millis(50)
                                })
                                .unwrap_or(false);
                            if !recently_modified {
                                let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                                if size <= 1_048_576 {
                                    let content = std::fs::read_to_string(path).unwrap_or_default();
                                    data.md_files.insert(path.clone(), Arc::new(content));
                                }
                            }
                        }
                        if data.is_git {
                            data.git_refresh_at.get_or_insert(now + debounce);
                        }
                    }
                    EventKind::Remove(_) => {
                        if data.md_files.remove(path).is_some() {
                            removed_md.push(path.clone());
                        }
                        dirs_needing_refresh.insert(dir.clone());
                        if data.is_git {
                            data.git_refresh_at.get_or_insert(now + debounce);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Batch dir refreshes -- one scan per dir instead of one per event.
        for dir in dirs_needing_refresh {
            if let Some(data) = self.dir_data.get_mut(&dir) {
                data.dir_entries = Arc::new(list_dir_entries(&dir));
            }
        }

        (created_md, removed_md)
    }
}
