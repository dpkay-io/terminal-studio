use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

const REFRESH_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Clone, Debug)]
pub(super) struct WorkspaceGitInfo {
    pub branch: String,
    pub diff_count: usize,
}

struct CachedEntry {
    info: WorkspaceGitInfo,
    fetched_at: Instant,
}

pub(super) struct WorkspaceGitWorker {
    tx: mpsc::Sender<(u64, PathBuf)>,
    cache: Arc<Mutex<HashMap<u64, CachedEntry>>>,
    inflight: Arc<Mutex<HashSet<u64>>>,
    alive: Arc<AtomicBool>,
}

impl WorkspaceGitWorker {
    pub(super) fn spawn(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<(u64, PathBuf)>();
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let inflight = Arc::new(Mutex::new(HashSet::new()));
        let alive = Arc::new(AtomicBool::new(true));

        let cache_bg = Arc::clone(&cache);
        let inflight_bg = Arc::clone(&inflight);
        let alive_bg = Arc::clone(&alive);

        if let Err(e) = thread::Builder::new()
            .name("workspace-git".into())
            .spawn(move || {
                while alive_bg.load(Ordering::Acquire) {
                    let (ws_id, path) = match rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let info = fetch_git_info(&path);
                    cache_bg.lock().insert(
                        ws_id,
                        CachedEntry {
                            info,
                            fetched_at: Instant::now(),
                        },
                    );
                    inflight_bg.lock().remove(&ws_id);
                    ctx.request_repaint();
                }
            })
        {
            log::error!("failed to spawn workspace-git worker: {e}");
        }

        WorkspaceGitWorker {
            tx,
            cache,
            inflight,
            alive,
        }
    }

    pub(super) fn request_if_stale(&self, ws_id: u64, path: &Path) {
        let mut inflight = self.inflight.lock();
        if inflight.contains(&ws_id) {
            return;
        }
        let is_fresh = self
            .cache
            .lock()
            .get(&ws_id)
            .is_some_and(|e| e.fetched_at.elapsed() < REFRESH_INTERVAL);
        if is_fresh {
            return;
        }
        inflight.insert(ws_id);
        let _ = self.tx.send((ws_id, path.to_path_buf()));
    }

    pub(super) fn get(&self, ws_id: u64) -> Option<WorkspaceGitInfo> {
        self.cache.lock().get(&ws_id).map(|e| e.info.clone())
    }

    pub(super) fn is_loading(&self, ws_id: u64) -> bool {
        self.inflight.lock().contains(&ws_id)
    }
}

impl Drop for WorkspaceGitWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        let _ = self.tx.send((0, PathBuf::new()));
    }
}

fn fetch_git_info(path: &Path) -> WorkspaceGitInfo {
    use std::process::Command;

    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let diff_count = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.lines().filter(|l| !l.is_empty()).count())
        .unwrap_or(0);

    WorkspaceGitInfo { branch, diff_count }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_and_drop() {
        let worker = WorkspaceGitWorker::spawn(egui::Context::default());
        drop(worker);
    }

    #[test]
    fn test_get_empty_cache() {
        let worker = WorkspaceGitWorker::spawn(egui::Context::default());
        assert!(worker.get(999).is_none());
    }

    #[test]
    fn test_request_if_stale_dedup() {
        let worker = WorkspaceGitWorker::spawn(egui::Context::default());
        let path = PathBuf::from("/nonexistent/path");
        worker.request_if_stale(1, &path);
        worker.request_if_stale(1, &path);
    }

    #[test]
    fn test_fetch_git_info_current_dir() {
        let cwd = std::env::current_dir().unwrap();
        let info = fetch_git_info(&cwd);
        assert!(!info.branch.is_empty(), "should detect branch in git repo");
    }

    #[test]
    fn test_fetch_git_info_non_git_dir() {
        let info = fetch_git_info(Path::new(if cfg!(windows) {
            "C:\\Windows"
        } else {
            "/tmp"
        }));
        assert!(info.branch.is_empty());
        assert_eq!(info.diff_count, 0);
    }

    #[test]
    fn test_enqueue_and_get_result() {
        let cwd = std::env::current_dir().unwrap();
        let worker = WorkspaceGitWorker::spawn(egui::Context::default());
        worker.request_if_stale(42, &cwd);

        let mut result = None;
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(info) = worker.get(42) {
                result = Some(info);
                break;
            }
        }
        assert!(result.is_some(), "worker should produce a result");
        let info = result.unwrap();
        assert!(!info.branch.is_empty());
    }

    #[test]
    fn test_is_loading_during_inflight() {
        let worker = WorkspaceGitWorker::spawn(egui::Context::default());
        assert!(!worker.is_loading(99));
        let path = PathBuf::from("/nonexistent/test/path");
        worker.request_if_stale(99, &path);
        assert!(worker.is_loading(99));
    }

    #[test]
    fn test_cache_freshness_skips_re_enqueue() {
        let cwd = std::env::current_dir().unwrap();
        let worker = WorkspaceGitWorker::spawn(egui::Context::default());
        worker.request_if_stale(1, &cwd);

        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(50));
            if worker.get(1).is_some() {
                break;
            }
        }
        assert!(worker.get(1).is_some());

        assert!(!worker.inflight.lock().contains(&1));
        worker.request_if_stale(1, &cwd);
        assert!(
            !worker.inflight.lock().contains(&1),
            "fresh cache should prevent re-enqueue"
        );
    }
}
