use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;

use super::file_browser::{list_dir_entries, run_git_info, FileEntry};

enum Job {
    GitInfo(PathBuf),
    DirList(PathBuf),
    Stage { cwd: PathBuf, path: String },
    Unstage { cwd: PathBuf, path: String },
    StageAll(PathBuf),
    UnstageAll(PathBuf),
    Diff { cwd: PathBuf, rel_path: String },
}

pub(super) struct WorkerResults {
    pub(super) git: HashMap<PathBuf, (String, String)>,
    pub(super) dirs: HashMap<PathBuf, Arc<Vec<FileEntry>>>,
    pub(super) diff_results: Vec<(PathBuf, String)>,
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
            diff_results: Vec::new(),
        }));
        let git_inflight: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
        let dir_inflight: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
        let alive = Arc::new(AtomicBool::new(true));

        let results_bg = Arc::clone(&results);
        let git_inflight_bg = Arc::clone(&git_inflight);
        let dir_inflight_bg = Arc::clone(&dir_inflight);
        let alive_bg = Arc::clone(&alive);
        let ctx_bg = ctx.clone();

        if let Err(e) = thread::Builder::new()
            .name("git-worker".into())
            .spawn(move || {
                while alive_bg.load(Ordering::Relaxed) {
                    let job = match rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(j) => j,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
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
                        Job::Stage { cwd, path } => {
                            let ok = std::process::Command::new("git")
                                .args(["add", "--", &path])
                                .current_dir(&cwd)
                                .status()
                                .map(|s| s.success())
                                .unwrap_or(false);
                            if ok {
                                let info = run_git_info(&cwd);
                                results_bg.lock().git.insert(cwd.clone(), info);
                            }
                        }
                        Job::Unstage { cwd, path } => {
                            let ok = std::process::Command::new("git")
                                .args(["reset", "HEAD", "--", &path])
                                .current_dir(&cwd)
                                .status()
                                .map(|s| s.success())
                                .unwrap_or(false);
                            if ok {
                                let info = run_git_info(&cwd);
                                results_bg.lock().git.insert(cwd.clone(), info);
                            }
                        }
                        Job::StageAll(cwd) => {
                            let ok = std::process::Command::new("git")
                                .args(["add", "-A"])
                                .current_dir(&cwd)
                                .status()
                                .map(|s| s.success())
                                .unwrap_or(false);
                            if ok {
                                let info = run_git_info(&cwd);
                                results_bg.lock().git.insert(cwd.clone(), info);
                            }
                        }
                        Job::UnstageAll(cwd) => {
                            let ok = std::process::Command::new("git")
                                .args(["reset", "HEAD"])
                                .current_dir(&cwd)
                                .status()
                                .map(|s| s.success())
                                .unwrap_or(false);
                            if ok {
                                let info = run_git_info(&cwd);
                                results_bg.lock().git.insert(cwd.clone(), info);
                            }
                        }
                        Job::Diff { cwd, rel_path } => {
                            let diff_output = std::process::Command::new("git")
                                .args(["diff", "HEAD", "--", &rel_path])
                                .current_dir(&cwd)
                                .output()
                                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                                .unwrap_or_default();
                            let full_path = cwd.join(&rel_path);
                            results_bg
                                .lock()
                                .diff_results
                                .push((full_path, diff_output));
                        }
                    }
                    ctx_bg.request_repaint();
                }
            })
        {
            log::error!("failed to spawn git-worker thread: {e}");
        }

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

    pub(super) fn enqueue_stage(&self, cwd: &Path, path: String) {
        let _ = self.tx.send(Job::Stage {
            cwd: cwd.to_path_buf(),
            path,
        });
    }

    pub(super) fn enqueue_unstage(&self, cwd: &Path, path: String) {
        let _ = self.tx.send(Job::Unstage {
            cwd: cwd.to_path_buf(),
            path,
        });
    }

    pub(super) fn enqueue_stage_all(&self, cwd: &Path) {
        let _ = self.tx.send(Job::StageAll(cwd.to_path_buf()));
    }

    pub(super) fn enqueue_unstage_all(&self, cwd: &Path) {
        let _ = self.tx.send(Job::UnstageAll(cwd.to_path_buf()));
    }

    pub(super) fn enqueue_diff(&self, cwd: &Path, rel_path: String) {
        let _ = self.tx.send(Job::Diff {
            cwd: cwd.to_path_buf(),
            rel_path,
        });
    }

    pub(super) fn take_diff_results(&self) -> Vec<(PathBuf, String)> {
        let mut lock = self.results.lock();
        std::mem::take(&mut lock.diff_results)
    }
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.tx.send(Job::GitInfo(PathBuf::new()));
        self.ctx.request_repaint();
    }
}
