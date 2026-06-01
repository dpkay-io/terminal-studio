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

    /// Drain all pending results from the worker thread.
    /// Returns (created_md_paths, removed_md_paths).
    pub(super) fn drain_results(&mut self) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut created_md = Vec::new();
        let mut removed_md = Vec::new();
        let mut git_refreshes = Vec::new();

        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                WatchResult::DirAdded { path, data } => {
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

        (created_md, removed_md)
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
            data.git_diff = diff.clone();
            data.git_status = status.clone();
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

// ── Worker thread ────────────────────────────────────────────────────────────

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

    let watcher = match notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            ev.lock().push(event);
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

    let debounce = Duration::from_millis(500);
    // Track git_refresh_at per dir on the worker side for debouncing.
    let mut git_refresh_pending: HashMap<PathBuf, Instant> = HashMap::new();
    // Track which dirs are git repos.
    let mut is_git: HashMap<PathBuf, bool> = HashMap::new();
    // Track known md files for modify detection.
    let mut known_md: HashMap<PathBuf, HashSet<PathBuf>> = HashMap::new();

    while alive.load(std::sync::atomic::Ordering::Relaxed) {
        // Process commands (non-blocking drain).
        loop {
            match cmd_rx.try_recv() {
                Ok(WatchCommand::SyncCwds(cwds)) => {
                    sync_cwds(&mut state, &result_tx, &cwds, &mut is_git, &mut known_md);
                }
                Ok(WatchCommand::ApplyGitResult { dir, diff, status }) => {
                    // Keep worker-side state consistent (not strictly needed but avoids drift).
                    let _ = (&dir, &diff, &status);
                }
                Ok(WatchCommand::ApplyUnpushedResult { dir, commits }) => {
                    let _ = (&dir, &commits);
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

    // Remove dirs no longer in session set or deleted from disk.
    let to_remove: Vec<PathBuf> = state
        .watched
        .iter()
        .filter(|d| !current.contains(*d) || !d.is_dir())
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
            let _ = state.watcher.unwatch(&gd);
            state.git_dirs.remove(&gd);
        }
        state.watched.remove(&dir);
        is_git.remove(&dir);
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
            let gd = dir.join(".git");
            if gd.is_dir()
                && state
                    .watcher
                    .watch(&gd, RecursiveMode::NonRecursive)
                    .is_ok()
            {
                state.git_dirs.insert(gd, dir.clone());
            }
            let dir_is_git = dir.join(".git").exists();
            is_git.insert(dir.clone(), dir_is_git);
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

fn process_fs_events(
    state: &WorkerState,
    result_tx: &mpsc::Sender<WatchResult>,
    events: Vec<Event>,
    git_refresh_pending: &mut HashMap<PathBuf, Instant>,
    is_git: &HashMap<PathBuf, bool>,
    known_md: &mut HashMap<PathBuf, HashSet<PathBuf>>,
    debounce: Duration,
) {
    let now = Instant::now();
    let mut dirs_needing_refresh: HashSet<PathBuf> = HashSet::new();

    for event in events {
        for path in &event.paths {
            let dir: PathBuf = if let Some(mut p) = path.parent().map(PathBuf::from) {
                if state.git_dirs.contains_key(&p) {
                    state.git_dirs[&p].clone()
                } else {
                    loop {
                        if state.watched.contains(&p) {
                            break p;
                        }
                        match p.parent() {
                            Some(parent) if parent != p => p = parent.to_path_buf(),
                            _ => break p,
                        }
                    }
                }
            } else {
                continue;
            };

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
                        let recently_modified = std::fs::metadata(path)
                            .and_then(|m| m.modified())
                            .map(|t| t.elapsed().unwrap_or_default() < Duration::from_millis(50))
                            .unwrap_or(false);
                        if !recently_modified {
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
        }
    }

    // Batch dir refreshes — one scan per dir.
    for dir in dirs_needing_refresh {
        let entries = Arc::new(list_dir_entries(&dir));
        let _ = result_tx.send(WatchResult::DirEntriesRefreshed { dir, entries });
    }
}
