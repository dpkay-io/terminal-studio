use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use super::file_browser::{list_dir_entries, DirData};

/// Results produced by the watcher worker thread, consumed by the UI thread.
pub(super) enum WatchResult {
    /// A new directory was added to the watch set.
    DirAdded { path: PathBuf, data: DirData },
    /// A directory was removed from the watch set.
    DirRemoved(PathBuf),
    /// Directory entries were refreshed (file create/remove detected).
    DirEntriesRefreshed {
        dir: PathBuf,
        entries: Arc<Vec<super::file_browser::FileEntry>>,
    },
    /// A markdown file was created or modified.
    MdCreated {
        dir: PathBuf,
        path: PathBuf,
        content: Arc<String>,
    },
    /// A markdown file was removed.
    MdRemoved { dir: PathBuf, path: PathBuf },
    /// An open editor file was modified on disk (notification only, no content).
    FileModified(PathBuf),
    /// A directory needs a git refresh (debounced).
    GitRefreshNeeded(PathBuf),
}

/// Commands sent from the UI thread to the watcher worker.
enum WatchCommand {
    /// Update the set of CWDs to watch.
    SyncCwds(Vec<PathBuf>),
    /// Apply git results back (so worker's local state stays consistent for git_refresh_at).
    ApplyGitResult {
        dir: PathBuf,
        diff: String,
        status: String,
    },
    ApplyUnpushedResult {
        dir: PathBuf,
        commits: Vec<(String, String)>,
    },
    SyncEditorFiles(HashSet<PathBuf>),
    Shutdown,
}

pub(super) struct WatchState {
    cmd_tx: mpsc::Sender<WatchCommand>,
    result_rx: mpsc::Receiver<WatchResult>,
    _alive: Arc<std::sync::atomic::AtomicBool>,

    // Owned by UI thread — updated from WatchResults.
    pub(super) dir_data: HashMap<PathBuf, DirData>,
    pub(super) last_sync: Instant,
    pub(super) last_session_count: usize,
}

impl Drop for WatchState {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(WatchCommand::Shutdown);
    }
}

impl WatchState {
    pub(super) fn new(ctx: egui::Context) -> Option<Self> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<WatchCommand>();
        let (result_tx, result_rx) = mpsc::channel::<WatchResult>();
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let alive_clone = Arc::clone(&alive);

        let ctx_clone = ctx.clone();
        thread::Builder::new()
            .name("watcher-sync".into())
            .spawn(move || {
                watcher_thread(cmd_rx, result_tx, ctx_clone, alive_clone);
            })
            .ok()?;

        Some(WatchState {
            cmd_tx,
            result_rx,
            _alive: alive,
            dir_data: HashMap::new(),
            last_sync: Instant::now(),
            last_session_count: 0,
        })
    }

    /// Send the current session CWDs to the worker for sync.
    /// This is cheap — just sends a Vec over a channel.
    pub(super) fn request_sync(&self, cwds: Vec<PathBuf>) {
        let _ = self.cmd_tx.send(WatchCommand::SyncCwds(cwds));
    }

    pub(super) fn sync_editor_files(&self, paths: HashSet<PathBuf>) {
        let _ = self.cmd_tx.send(WatchCommand::SyncEditorFiles(paths));
    }

    /// Drain all pending results from the worker thread.
    /// Returns (created_md_paths, removed_md_paths, modified_editor_paths).
    pub(super) fn drain_results(&mut self) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) {
        let mut created_md = Vec::new();
        let mut removed_md = Vec::new();
        let mut git_refreshes = Vec::new();
        let mut modified_files = Vec::new();

        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                WatchResult::DirAdded { path, mut data } => {
                    if let Some(existing) = self.dir_data.get(&path) {
                        data.git_diff = existing.git_diff.clone();
                        data.git_status = existing.git_status.clone();
                        data.git_unpushed = existing.git_unpushed.clone();
                        data.merge_operation = existing.merge_operation.clone();
                    }
                    self.dir_data.insert(path, data);
                }
                WatchResult::DirRemoved(path) => {
                    self.dir_data.remove(&path);
                }
                WatchResult::DirEntriesRefreshed { dir, entries } => {
                    if let Some(data) = self.dir_data.get_mut(&dir) {
                        data.dir_entries = entries;
                    }
                }
                WatchResult::MdCreated { dir, path, content } => {
                    if let Some(data) = self.dir_data.get_mut(&dir) {
                        data.md_files.insert(path.clone(), content);
                    }
                    created_md.push(path);
                }
                WatchResult::MdRemoved { dir, path } => {
                    if let Some(data) = self.dir_data.get_mut(&dir) {
                        data.md_files.remove(&path);
                    }
                    removed_md.push(path);
                }
                WatchResult::FileModified(path) => {
                    modified_files.push(path);
                }
                WatchResult::GitRefreshNeeded(dir) => {
                    git_refreshes.push(dir);
                }
            }
        }

        // Mark git_refresh_at on the UI-side DirData so the existing
        // git worker integration picks them up via take_pending_git_refreshes().
        let now = Instant::now();
        for dir in git_refreshes {
            if let Some(data) = self.dir_data.get_mut(&dir) {
                data.git_refresh_at.get_or_insert(now);
            }
        }

        (created_md, removed_md, modified_files)
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

    pub(super) fn apply_git_result(
        &mut self,
        dir: &Path,
        diff: String,
        status: String,
        merge_op: super::git_worker::MergeOperation,
    ) {
        if let Some(data) = self.dir_data.get_mut(dir) {
            data.git_diff = diff.clone();
            data.git_status = status.clone();
            data.merge_operation = merge_op;
        }
        let _ = self.cmd_tx.send(WatchCommand::ApplyGitResult {
            dir: dir.to_path_buf(),
            diff,
            status,
        });
    }

    pub(super) fn apply_unpushed_result(&mut self, dir: &Path, commits: Vec<(String, String)>) {
        if let Some(data) = self.dir_data.get_mut(dir) {
            data.git_unpushed = commits.clone();
        }
        let _ = self.cmd_tx.send(WatchCommand::ApplyUnpushedResult {
            dir: dir.to_path_buf(),
            commits,
        });
    }
}

#[cfg(test)]
impl WatchState {
    fn new_for_test() -> (Self, mpsc::Sender<WatchResult>) {
        let (cmd_tx, _cmd_rx) = mpsc::channel::<WatchCommand>();
        let (result_tx, result_rx) = mpsc::channel::<WatchResult>();
        let alive = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let ws = WatchState {
            cmd_tx,
            result_rx,
            _alive: alive,
            dir_data: HashMap::new(),
            last_sync: Instant::now(),
            last_session_count: 0,
        };
        (ws, result_tx)
    }
}

// ── Worker thread ────────────────────────────────────────────────────────────

fn cached_git_root(cache: &mut HashMap<PathBuf, Option<PathBuf>>, path: &Path) -> Option<PathBuf> {
    if let Some(cached) = cache.get(path) {
        return cached.clone();
    }
    let result = crate::util::find_git_root(path);
    cache.insert(path.to_path_buf(), result.clone());
    result
}

struct WorkerState {
    watcher: RecommendedWatcher,
    watched: HashSet<PathBuf>,
    git_dirs: HashMap<PathBuf, PathBuf>,
    events: Arc<Mutex<Vec<Event>>>,
}

fn watcher_thread(
    cmd_rx: mpsc::Receiver<WatchCommand>,
    result_tx: mpsc::Sender<WatchResult>,
    ctx: egui::Context,
    alive: Arc<std::sync::atomic::AtomicBool>,
) {
    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let ev = Arc::clone(&events);
    let ctx_repaint = ctx.clone();

    const MAX_PENDING_EVENTS: usize = 500;
    let watcher = match notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            let mut events = ev.lock();
            if events.len() < MAX_PENDING_EVENTS {
                events.push(event);
            }
            drop(events);
            ctx_repaint.request_repaint_after(Duration::from_millis(100));
        }
    }) {
        Ok(w) => w,
        Err(_) => return,
    };

    let mut state = WorkerState {
        watcher,
        watched: HashSet::new(),
        git_dirs: HashMap::new(),
        events,
    };

    let debounce = Duration::from_millis(1000);
    // Track git_refresh_at per dir on the worker side for debouncing.
    let mut git_refresh_pending: HashMap<PathBuf, Instant> = HashMap::new();
    // Track which dirs are git repos.
    let mut is_git: HashMap<PathBuf, bool> = HashMap::new();
    // Track known md files for modify detection.
    let mut known_md: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();
    let mut git_root_cache: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    let mut editor_files: HashSet<PathBuf> = HashSet::new();

    while alive.load(std::sync::atomic::Ordering::Relaxed) {
        // Process commands (non-blocking drain).
        loop {
            match cmd_rx.try_recv() {
                Ok(WatchCommand::SyncCwds(cwds)) => {
                    sync_cwds(
                        &mut state,
                        &result_tx,
                        &cwds,
                        &mut is_git,
                        &mut known_md,
                        &mut git_root_cache,
                    );
                }
                Ok(WatchCommand::ApplyGitResult { dir, diff, status }) => {
                    let _ = (&dir, &diff, &status);
                }
                Ok(WatchCommand::ApplyUnpushedResult { dir, commits }) => {
                    let _ = (&dir, &commits);
                }
                Ok(WatchCommand::SyncEditorFiles(paths)) => {
                    editor_files = paths;
                }
                Ok(WatchCommand::Shutdown) => return,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        }

        // Process filesystem events.
        let pending_events: Vec<Event> = {
            let mut g = state.events.lock();
            std::mem::take(&mut *g)
        };

        if !pending_events.is_empty() {
            process_fs_events(
                &state,
                &result_tx,
                pending_events,
                &mut git_refresh_pending,
                &is_git,
                &mut known_md,
                debounce,
                &mut git_root_cache,
                &editor_files,
            );
        }

        // Flush matured git refresh timers.
        let now = Instant::now();
        let matured: Vec<PathBuf> = git_refresh_pending
            .iter()
            .filter(|(_, t)| now >= **t)
            .map(|(d, _)| d.clone())
            .collect();
        for dir in matured {
            git_refresh_pending.remove(&dir);
            let _ = result_tx.send(WatchResult::GitRefreshNeeded(dir));
            ctx.request_repaint();
        }

        // Sleep briefly to avoid busy-spinning; wake on events or commands.
        thread::sleep(Duration::from_millis(50));
    }
}

fn sync_cwds(
    state: &mut WorkerState,
    result_tx: &mpsc::Sender<WatchResult>,
    cwds: &[PathBuf],
    is_git: &mut HashMap<PathBuf, bool>,
    known_md: &mut HashMap<PathBuf, HashSet<PathBuf>>,
    git_root_cache: &mut HashMap<PathBuf, Option<PathBuf>>,
) {
    let current: HashSet<PathBuf> = cwds
        .iter()
        .filter(|p| {
            if p.as_os_str().is_empty() {
                return false;
            }
            state.watched.contains(*p) || p.is_dir()
        })
        .cloned()
        .collect();

    // Remove dirs no longer in session set. Directories still in the desired
    // set are kept even if is_dir() temporarily fails (e.g. Windows I/O race)
    // to avoid evict-and-re-add cycles that reset populated git data.
    let to_remove: Vec<PathBuf> = state
        .watched
        .iter()
        .filter(|d| !current.contains(*d))
        .cloned()
        .collect();
    for dir in to_remove {
        let _ = state.watcher.unwatch(&dir);
        if let Some(gd) = state
            .git_dirs
            .iter()
            .find(|(_, v)| **v == dir)
            .map(|(k, _)| k.clone())
        {
            // Re-assign the .git watch to another CWD sharing the same repo,
            // or unwatch if no other CWD needs it.
            let other = current.iter().find(|d| {
                **d != dir
                    && cached_git_root(git_root_cache, d)
                        .map(|r| r.join(".git") == gd)
                        .unwrap_or(false)
            });
            if let Some(reassign_to) = other {
                state.git_dirs.insert(gd, reassign_to.clone());
            } else {
                let _ = state.watcher.unwatch(&gd);
                state.git_dirs.remove(&gd);
            }
        }
        state.watched.remove(&dir);
        is_git.remove(&dir);
        git_root_cache.remove(&dir);
        known_md.remove(&dir);
        let _ = result_tx.send(WatchResult::DirRemoved(dir));
    }

    // Add new dirs.
    let to_add: Vec<PathBuf> = current
        .into_iter()
        .filter(|d| !state.watched.contains(d))
        .collect();
    for dir in to_add {
        if state.watcher.watch(&dir, RecursiveMode::Recursive).is_ok() {
            let git_root = cached_git_root(git_root_cache, &dir);
            if let Some(ref root) = git_root {
                let gd = root.join(".git");
                if gd.is_dir()
                    && !state.git_dirs.contains_key(&gd)
                    && state
                        .watcher
                        .watch(&gd, RecursiveMode::NonRecursive)
                        .is_ok()
                {
                    state.git_dirs.insert(gd, dir.clone());
                }
            }
            is_git.insert(dir.clone(), git_root.is_some());
            known_md.insert(dir.clone(), HashSet::new());

            let data = DirData::new(&dir);
            let _ = result_tx.send(WatchResult::DirAdded {
                path: dir.clone(),
                data,
            });
            state.watched.insert(dir);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn process_fs_events(
    state: &WorkerState,
    result_tx: &mpsc::Sender<WatchResult>,
    events: Vec<Event>,
    git_refresh_pending: &mut HashMap<PathBuf, Instant>,
    is_git: &HashMap<PathBuf, bool>,
    known_md: &mut HashMap<PathBuf, HashSet<PathBuf>>,
    debounce: Duration,
    git_root_cache: &mut HashMap<PathBuf, Option<PathBuf>>,
    editor_files: &HashSet<PathBuf>,
) {
    let now = Instant::now();
    let mut dirs_needing_refresh: HashSet<PathBuf> = HashSet::new();
    let mut editor_files_sent: HashSet<PathBuf> = HashSet::new();

    for event in events {
        for path in &event.paths {
            if matches!(&event.kind, EventKind::Modify(_) | EventKind::Create(_))
                && editor_files.contains(path)
                && !editor_files_sent.contains(path)
            {
                editor_files_sent.insert(path.clone());
                let _ = result_tx.send(WatchResult::FileModified(path.clone()));
            }
            // If the event is inside a .git directory, resolve to all CWDs
            // sharing that git root so they all get refreshed.
            let dirs: Vec<PathBuf> = if let Some(mut p) = path.parent().map(PathBuf::from) {
                if state.git_dirs.contains_key(&p) {
                    let git_root = p.parent().unwrap_or(&p).to_path_buf();
                    state
                        .watched
                        .iter()
                        .filter(|d| is_git.get(*d).copied().unwrap_or(false))
                        .filter(|d| {
                            cached_git_root(git_root_cache, d)
                                .map(|r| r == git_root)
                                .unwrap_or(false)
                        })
                        .cloned()
                        .collect()
                } else {
                    let resolved = loop {
                        if state.watched.contains(&p) {
                            break p;
                        }
                        match p.parent() {
                            Some(parent) if parent != p => p = parent.to_path_buf(),
                            _ => break p,
                        }
                    };
                    vec![resolved]
                }
            } else {
                continue;
            };

            for dir in dirs {
                if !state.watched.contains(&dir) {
                    continue;
                }

                let dir_is_git = is_git.get(&dir).copied().unwrap_or(false);
                let is_md = path.extension().and_then(|e| e.to_str()) == Some("md");

                match &event.kind {
                    EventKind::Create(_) => {
                        if is_md && path.is_file() {
                            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                            if size <= 1_048_576 {
                                let content = std::fs::read_to_string(path).unwrap_or_default();
                                if let Some(set) = known_md.get_mut(&dir) {
                                    set.insert(path.clone());
                                }
                                let _ = result_tx.send(WatchResult::MdCreated {
                                    dir: dir.clone(),
                                    path: path.clone(),
                                    content: Arc::new(content),
                                });
                            }
                        }
                        dirs_needing_refresh.insert(dir.clone());
                        if dir_is_git {
                            git_refresh_pending
                                .entry(dir.clone())
                                .or_insert(now + debounce);
                        }
                    }
                    EventKind::Modify(_) => {
                        let is_known_md = known_md
                            .get(&dir)
                            .map(|s| s.contains(path))
                            .unwrap_or(false);
                        if is_md && path.is_file() && is_known_md {
                            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                            if size <= 1_048_576 {
                                let content = std::fs::read_to_string(path).unwrap_or_default();
                                let _ = result_tx.send(WatchResult::MdCreated {
                                    dir: dir.clone(),
                                    path: path.clone(),
                                    content: Arc::new(content),
                                });
                            }
                        }
                        if dir_is_git {
                            git_refresh_pending
                                .entry(dir.clone())
                                .or_insert(now + debounce);
                        }
                    }
                    EventKind::Remove(_) => {
                        let was_known = known_md
                            .get_mut(&dir)
                            .map(|s| s.remove(path))
                            .unwrap_or(false);
                        if was_known {
                            let _ = result_tx.send(WatchResult::MdRemoved {
                                dir: dir.clone(),
                                path: path.clone(),
                            });
                        }
                        dirs_needing_refresh.insert(dir.clone());
                        if dir_is_git {
                            git_refresh_pending
                                .entry(dir.clone())
                                .or_insert(now + debounce);
                        }
                    }
                    _ => {}
                }
            } // for dir in dirs
        }
    }

    // Batch dir refreshes — one scan per dir.
    for dir in dirs_needing_refresh {
        let entries = Arc::new(list_dir_entries(&dir));
        let _ = result_tx.send(WatchResult::DirEntriesRefreshed { dir, entries });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::git_worker::MergeOperation;

    fn empty_dir_data() -> DirData {
        DirData {
            is_git: true,
            git_diff: String::new(),
            git_status: String::new(),
            git_unpushed: Vec::new(),
            git_refresh_at: None,
            merge_operation: MergeOperation::None,
            md_files: HashMap::new(),
            dir_entries: Arc::new(Vec::new()),
        }
    }

    #[test]
    fn dir_added_inserts_new_entry() {
        let (mut ws, tx) = WatchState::new_for_test();
        let path = PathBuf::from("/test/project");
        let mut data = empty_dir_data();
        data.git_status = "M file.rs".to_string();
        tx.send(WatchResult::DirAdded {
            path: path.clone(),
            data,
        })
        .unwrap();
        ws.drain_results();
        assert_eq!(ws.dir_data[&path].git_status, "M file.rs");
    }

    #[test]
    fn dir_added_preserves_existing_git_data() {
        let (mut ws, tx) = WatchState::new_for_test();
        let path = PathBuf::from("/test/project");

        let mut existing = empty_dir_data();
        existing.git_status = "M src/main.rs".to_string();
        existing.git_diff = "diff --git a/src/main.rs".to_string();
        existing.git_unpushed = vec![("abc123".into(), "fix bug".into())];
        ws.dir_data.insert(path.clone(), existing);

        let fresh = empty_dir_data();
        assert!(fresh.git_status.is_empty());
        tx.send(WatchResult::DirAdded {
            path: path.clone(),
            data: fresh,
        })
        .unwrap();
        ws.drain_results();

        assert_eq!(ws.dir_data[&path].git_status, "M src/main.rs");
        assert_eq!(ws.dir_data[&path].git_diff, "diff --git a/src/main.rs");
        assert_eq!(ws.dir_data[&path].git_unpushed.len(), 1);
    }

    #[test]
    fn dir_added_updates_non_git_fields() {
        let (mut ws, tx) = WatchState::new_for_test();
        let path = PathBuf::from("/test/project");

        let mut existing = empty_dir_data();
        existing.git_status = "M old.rs".to_string();
        existing.is_git = false;
        ws.dir_data.insert(path.clone(), existing);

        let mut fresh = empty_dir_data();
        fresh.is_git = true;
        tx.send(WatchResult::DirAdded {
            path: path.clone(),
            data: fresh,
        })
        .unwrap();
        ws.drain_results();

        assert!(ws.dir_data[&path].is_git);
        assert_eq!(ws.dir_data[&path].git_status, "M old.rs");
    }

    #[test]
    fn dir_removed_then_added_starts_fresh() {
        let (mut ws, tx) = WatchState::new_for_test();
        let path = PathBuf::from("/test/project");

        let mut existing = empty_dir_data();
        existing.git_status = "M old.rs".to_string();
        ws.dir_data.insert(path.clone(), existing);

        tx.send(WatchResult::DirRemoved(path.clone())).unwrap();
        let fresh = empty_dir_data();
        tx.send(WatchResult::DirAdded {
            path: path.clone(),
            data: fresh,
        })
        .unwrap();
        ws.drain_results();

        assert!(ws.dir_data[&path].git_status.is_empty());
    }

    #[test]
    fn apply_git_result_populates_data() {
        let (mut ws, _tx) = WatchState::new_for_test();
        let path = PathBuf::from("/test/project");
        ws.dir_data.insert(path.clone(), empty_dir_data());

        ws.apply_git_result(
            &path,
            "staged diff".into(),
            "M file.rs".into(),
            MergeOperation::None,
        );

        assert_eq!(ws.dir_data[&path].git_status, "M file.rs");
        assert_eq!(ws.dir_data[&path].git_diff, "staged diff");
    }

    #[test]
    fn apply_git_result_ignores_missing_dir() {
        let (mut ws, _tx) = WatchState::new_for_test();
        let path = PathBuf::from("/nonexistent");
        ws.apply_git_result(&path, "diff".into(), "status".into(), MergeOperation::None);
        assert!(!ws.dir_data.contains_key(&path));
    }

    #[test]
    fn take_pending_git_refreshes_consumes_once() {
        let (mut ws, _tx) = WatchState::new_for_test();
        let path = PathBuf::from("/test/project");
        let mut data = empty_dir_data();
        data.git_refresh_at = Some(Instant::now());
        ws.dir_data.insert(path.clone(), data);

        let pending = ws.take_pending_git_refreshes();
        assert_eq!(pending, vec![path.clone()]);

        let pending2 = ws.take_pending_git_refreshes();
        assert!(pending2.is_empty());
    }

    #[test]
    fn dir_added_after_apply_preserves_git_data() {
        let (mut ws, tx) = WatchState::new_for_test();
        let path = PathBuf::from("/test/project");
        ws.dir_data.insert(path.clone(), empty_dir_data());

        ws.apply_git_result(
            &path,
            "real diff".into(),
            "M real.rs".into(),
            MergeOperation::None,
        );
        assert_eq!(ws.dir_data[&path].git_status, "M real.rs");

        let fresh = empty_dir_data();
        tx.send(WatchResult::DirAdded {
            path: path.clone(),
            data: fresh,
        })
        .unwrap();
        ws.drain_results();

        assert_eq!(ws.dir_data[&path].git_status, "M real.rs");
        assert_eq!(ws.dir_data[&path].git_diff, "real diff");
    }
}
