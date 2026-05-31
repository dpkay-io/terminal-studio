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
    UnpushedCommits(PathBuf),
    Commit { cwd: PathBuf, message: String, amend: bool },
    Push { cwd: PathBuf, force: bool },
    LastCommitMessage(PathBuf),
    Gitignore { cwd: PathBuf, pattern: String },
}

pub(super) struct WorkerResults {
    pub(super) git: HashMap<PathBuf, (String, String)>,
    pub(super) dirs: HashMap<PathBuf, Arc<Vec<FileEntry>>>,
    pub(super) diff_results: Vec<(PathBuf, String)>,
    pub(super) unpushed: HashMap<PathBuf, Vec<(String, String)>>,
    pub(super) commit_result: Option<Result<PathBuf, String>>,
    pub(super) push_result: Option<Result<PathBuf, String>>,
    pub(super) last_commit_msg: HashMap<PathBuf, String>,
    pub(super) gitignore_result: Option<Result<PathBuf, String>>,
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
            unpushed: HashMap::new(),
            commit_result: None,
            push_result: None,
            last_commit_msg: HashMap::new(),
            gitignore_result: None,
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
                        Job::UnpushedCommits(cwd) => {
                            let output = std::process::Command::new("git")
                                .args(["log", "--oneline", "@{upstream}..HEAD"])
                                .current_dir(&cwd)
                                .output()
                                .ok();
                            let commits: Vec<(String, String)> = output
                                .filter(|o| o.status.success())
                                .and_then(|o| String::from_utf8(o.stdout).ok())
                                .map(|s| {
                                    s.lines()
                                        .filter_map(|line| {
                                            let (hash, msg) = line.split_once(' ')?;
                                            Some((hash.to_string(), msg.to_string()))
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();
                            results_bg.lock().unpushed.insert(cwd, commits);
                        }
                        Job::Commit { cwd, message, amend } => {
                            let mut args = vec!["commit".to_string()];
                            if amend {
                                args.push("--amend".to_string());
                            }
                            args.push("-m".to_string());
                            args.push(message);
                            let output = std::process::Command::new("git")
                                .args(&args)
                                .current_dir(&cwd)
                                .output();
                            let result = match output {
                                Ok(o) if o.status.success() => {
                                    let info = run_git_info(&cwd);
                                    results_bg.lock().git.insert(cwd.clone(), info);
                                    Ok(cwd)
                                }
                                Ok(o) => {
                                    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
                                    Err(stderr)
                                }
                                Err(e) => Err(e.to_string()),
                            };
                            results_bg.lock().commit_result = Some(result);
                        }
                        Job::Push { cwd, force } => {
                            let mut args = vec!["push"];
                            if force {
                                args.push("--force");
                            }
                            let output = std::process::Command::new("git")
                                .args(&args)
                                .current_dir(&cwd)
                                .output();
                            let result = match output {
                                Ok(o) if o.status.success() => Ok(cwd),
                                Ok(o) => {
                                    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
                                    Err(stderr)
                                }
                                Err(e) => Err(e.to_string()),
                            };
                            results_bg.lock().push_result = Some(result);
                        }
                        Job::LastCommitMessage(cwd) => {
                            let msg = std::process::Command::new("git")
                                .args(["log", "-1", "--format=%B"])
                                .current_dir(&cwd)
                                .output()
                                .ok()
                                .filter(|o| o.status.success())
                                .and_then(|o| String::from_utf8(o.stdout).ok())
                                .map(|s| s.trim().to_string())
                                .unwrap_or_default();
                            results_bg.lock().last_commit_msg.insert(cwd, msg);
                        }
                        Job::Gitignore { cwd, pattern } => {
                            let gitignore_path = cwd.join(".gitignore");
                            let result = (|| -> Result<PathBuf, String> {
                                let mut content = std::fs::read_to_string(&gitignore_path)
                                    .unwrap_or_default();
                                let already_present = content
                                    .lines()
                                    .any(|line| line.trim() == pattern.trim());
                                if already_present {
                                    return Ok(cwd.clone());
                                }
                                if !content.is_empty() && !content.ends_with('\n') {
                                    content.push('\n');
                                }
                                content.push_str(&pattern);
                                content.push('\n');
                                std::fs::write(&gitignore_path, &content)
                                    .map_err(|e| e.to_string())?;
                                let info = run_git_info(&cwd);
                                results_bg.lock().git.insert(cwd.clone(), info);
                                Ok(cwd.clone())
                            })();
                            results_bg.lock().gitignore_result = Some(result);
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

    pub(super) fn enqueue_unpushed(&self, cwd: &Path) {
        let _ = self.tx.send(Job::UnpushedCommits(cwd.to_path_buf()));
    }

    pub(super) fn take_unpushed(&self, path: &Path) -> Option<Vec<(String, String)>> {
        self.results.lock().unpushed.remove(path)
    }

    pub(super) fn enqueue_commit(&self, cwd: &Path, message: String, amend: bool) {
        let _ = self.tx.send(Job::Commit {
            cwd: cwd.to_path_buf(),
            message,
            amend,
        });
    }

    pub(super) fn take_commit_result(&self) -> Option<Result<PathBuf, String>> {
        self.results.lock().commit_result.take()
    }

    pub(super) fn enqueue_push(&self, cwd: &Path, force: bool) {
        let _ = self.tx.send(Job::Push {
            cwd: cwd.to_path_buf(),
            force,
        });
    }

    pub(super) fn take_push_result(&self) -> Option<Result<PathBuf, String>> {
        self.results.lock().push_result.take()
    }

    pub(super) fn enqueue_last_commit_msg(&self, cwd: &Path) {
        let _ = self.tx.send(Job::LastCommitMessage(cwd.to_path_buf()));
    }

    pub(super) fn take_last_commit_msg(&self, path: &Path) -> Option<String> {
        self.results.lock().last_commit_msg.remove(path)
    }

    pub(super) fn enqueue_gitignore(&self, cwd: &Path, pattern: String) {
        let _ = self.tx.send(Job::Gitignore {
            cwd: cwd.to_path_buf(),
            pattern,
        });
    }

    pub(super) fn take_gitignore_result(&self) -> Option<Result<PathBuf, String>> {
        self.results.lock().gitignore_result.take()
    }
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.tx.send(Job::GitInfo(PathBuf::new()));
        self.ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_and_drop() {
        let worker = GitWorker::spawn(egui::Context::default());
        drop(worker);
        // No panic means success
    }

    #[test]
    fn test_take_git_empty() {
        let worker = GitWorker::spawn(egui::Context::default());
        let result = worker.take_git(Path::new("/nonexistent/path"));
        assert!(result.is_none());
    }

    #[test]
    fn test_take_dir_empty() {
        let worker = GitWorker::spawn(egui::Context::default());
        let result = worker.take_dir(Path::new("/nonexistent/path"));
        assert!(result.is_none());
    }

    #[test]
    fn test_take_diff_results_empty() {
        let worker = GitWorker::spawn(egui::Context::default());
        let results = worker.take_diff_results();
        assert!(results.is_empty());
    }

    #[test]
    fn test_enqueue_git_inflight_dedup() {
        let worker = GitWorker::spawn(egui::Context::default());
        let path = PathBuf::from("/some/fake/path");
        // Enqueue the same path twice — should not panic
        worker.enqueue_git(&path);
        worker.enqueue_git(&path);
    }

    #[test]
    fn test_enqueue_and_take_git_with_real_dir() {
        let cwd = std::env::current_dir().unwrap();
        let worker = GitWorker::spawn(egui::Context::default());
        worker.enqueue_git(&cwd);

        let mut result = None;
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(r) = worker.take_git(&cwd) {
                result = Some(r);
                break;
            }
        }
        assert!(result.is_some(), "git worker should have produced a result");
        let (diff, status) = result.unwrap();
        // diff and status may both be empty on a clean working tree
        let _ = (diff, status);
    }

    #[test]
    fn test_take_gitignore_result_empty() {
        let worker = GitWorker::spawn(egui::Context::default());
        assert!(worker.take_gitignore_result().is_none());
    }

    #[test]
    fn test_gitignore_creates_file() {
        let tmp = std::env::temp_dir().join(format!("git_worker_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // Init a git repo so run_git_info doesn't fail
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&tmp)
            .output()
            .ok();

        let worker = GitWorker::spawn(egui::Context::default());
        worker.enqueue_gitignore(&tmp, "target/".to_string());

        let mut result = None;
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(r) = worker.take_gitignore_result() {
                result = Some(r);
                break;
            }
        }
        assert!(result.is_some(), "gitignore job should produce a result");
        assert!(result.unwrap().is_ok());
        let content = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert!(content.contains("target/"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_gitignore_no_duplicate() {
        let tmp = std::env::temp_dir().join(format!(
            "git_worker_dedup_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&tmp)
            .output()
            .ok();
        std::fs::write(tmp.join(".gitignore"), "target/\n").unwrap();

        let worker = GitWorker::spawn(egui::Context::default());
        worker.enqueue_gitignore(&tmp, "target/".to_string());

        let mut result = None;
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(r) = worker.take_gitignore_result() {
                result = Some(r);
                break;
            }
        }
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
        let content = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert_eq!(content.matches("target/").count(), 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_enqueue_dir_and_take() {
        let cwd = std::env::current_dir().unwrap();
        let worker = GitWorker::spawn(egui::Context::default());
        worker.enqueue_dir(&cwd);

        let mut result = None;
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(r) = worker.take_dir(&cwd) {
                result = Some(r);
                break;
            }
        }
        assert!(
            result.is_some(),
            "dir listing should have produced a result"
        );
        let entries = result.unwrap();
        // The project directory should have at least some entries (Cargo.toml, src/, etc.)
        assert!(!entries.is_empty(), "directory listing should not be empty");
    }
}
