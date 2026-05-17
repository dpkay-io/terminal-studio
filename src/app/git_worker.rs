use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use parking_lot::Mutex;

use super::file_browser::{list_dir_entries, run_git_info, FileEntry};

enum Job {
    GitInfo(PathBuf),
    DirList(PathBuf),
}

pub(super) struct WorkerResults {
    pub(super) git: HashMap<PathBuf, (String, String)>,
    pub(super) dirs: HashMap<PathBuf, Arc<Vec<FileEntry>>>,
}

pub(super) struct GitWorker {
    tx: mpsc::Sender<Job>,
    results: Arc<Mutex<WorkerResults>>,
    git_inflight: Arc<Mutex<HashSet<PathBuf>>>,
    dir_inflight: Arc<Mutex<HashSet<PathBuf>>>,
    alive: Arc<AtomicBool>,
    ctx: egui::Context,
}

impl GitWorker {
    pub(super) fn spawn(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let results = Arc::new(Mutex::new(WorkerResults {
            git: HashMap::new(),
            dirs: HashMap::new(),
        }));
        let git_inflight: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
        let dir_inflight: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
        let alive = Arc::new(AtomicBool::new(true));

        let results_bg = Arc::clone(&results);
        let git_inflight_bg = Arc::clone(&git_inflight);
        let dir_inflight_bg = Arc::clone(&dir_inflight);
        let alive_bg = Arc::clone(&alive);
        let ctx_bg = ctx.clone();

        thread::Builder::new()
            .name("git-worker".into())
            .spawn(move || {
                while alive_bg.load(Ordering::Relaxed) {
                    let job = match rx.recv() {
                        Ok(j) => j,
                        Err(_) => break,
                    };
                    match job {
                        Job::GitInfo(p) => {
                            let info = run_git_info(&p);
                            results_bg.lock().git.insert(p.clone(), info);
                            git_inflight_bg.lock().remove(&p);
                        }
                        Job::DirList(p) => {
                            let entries = Arc::new(list_dir_entries(&p));
                            results_bg.lock().dirs.insert(p.clone(), entries);
                            dir_inflight_bg.lock().remove(&p);
                        }
                    }
                    ctx_bg.request_repaint();
                }
            })
            .expect("failed to spawn git-worker thread");

        GitWorker {
            tx,
            results,
            git_inflight,
            dir_inflight,
            alive,
            ctx,
        }
    }

    pub(super) fn enqueue_git(&self, path: &Path) {
        let mut inflight = self.git_inflight.lock();
        if inflight.contains(path) {
            return;
        }
        inflight.insert(path.to_path_buf());
        let _ = self.tx.send(Job::GitInfo(path.to_path_buf()));
    }

    pub(super) fn enqueue_dir(&self, path: &Path) {
        let mut inflight = self.dir_inflight.lock();
        if inflight.contains(path) {
            return;
        }
        inflight.insert(path.to_path_buf());
        let _ = self.tx.send(Job::DirList(path.to_path_buf()));
    }

    pub(super) fn take_git(&self, path: &Path) -> Option<(String, String)> {
        self.results.lock().git.remove(path)
    }

    pub(super) fn take_dir(&self, path: &Path) -> Option<Arc<Vec<FileEntry>>> {
        self.results.lock().dirs.remove(path)
    }
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.tx.send(Job::GitInfo(PathBuf::new()));
        self.ctx.request_repaint();
    }
}
