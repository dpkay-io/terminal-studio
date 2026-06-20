use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;

use super::file_browser::run_git_info;

enum Job {
    GitInfo(PathBuf),
    Stage {
        cwd: PathBuf,
        path: String,
    },
    Unstage {
        cwd: PathBuf,
        path: String,
    },
    StageAll(PathBuf),
    UnstageAll(PathBuf),
    Diff {
        cwd: PathBuf,
        rel_path: String,
    },
    UnpushedCommits(PathBuf),
    Commit {
        cwd: PathBuf,
        message: String,
        amend: bool,
    },
    Push {
        cwd: PathBuf,
        force: bool,
    },
    LastCommitMessage(PathBuf),
    Gitignore {
        cwd: PathBuf,
        pattern: String,
    },
    Revert {
        cwd: PathBuf,
        path: String,
    },
}

pub(super) struct DiffResult {
    pub(super) path: PathBuf,
    pub(super) old_content: String,
    pub(super) new_content: String,
    pub(super) raw_diff: String,
    pub(super) old_highlights: Option<Vec<Vec<(egui::Color32, String)>>>,
    pub(super) new_highlights: Option<Vec<Vec<(egui::Color32, String)>>>,
    pub(super) highlight_theme: crate::theme::ThemeId,
}

pub(super) struct WorkerResults {
    pub(super) git: HashMap<PathBuf, (String, String)>,
    pub(super) diff_results: Vec<DiffResult>,
    pub(super) unpushed: HashMap<PathBuf, Vec<(String, String)>>,
    pub(super) commit_results: Vec<Result<PathBuf, String>>,
    pub(super) push_results: Vec<Result<PathBuf, String>>,
    pub(super) last_commit_msg: HashMap<PathBuf, String>,
    pub(super) gitignore_results: Vec<Result<PathBuf, String>>,
    pub(super) revert_results: Vec<Result<PathBuf, String>>,
}

pub(super) struct GitWorker {
    tx: mpsc::Sender<Job>,
    results: Arc<Mutex<WorkerResults>>,
    git_inflight: Arc<Mutex<HashSet<PathBuf>>>,
    manual_git_inflight: Arc<Mutex<HashSet<PathBuf>>>,
    alive: Arc<AtomicBool>,
    ctx: egui::Context,
}

impl GitWorker {
    pub(super) fn spawn(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let results = Arc::new(Mutex::new(WorkerResults {
            git: HashMap::new(),
            diff_results: Vec::new(),
            unpushed: HashMap::new(),
            commit_results: Vec::new(),
            push_results: Vec::new(),
            last_commit_msg: HashMap::new(),
            gitignore_results: Vec::new(),
            revert_results: Vec::new(),
        }));
        let git_inflight: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
        let manual_git_inflight: Arc<Mutex<HashSet<PathBuf>>> =
            Arc::new(Mutex::new(HashSet::new()));
        let alive = Arc::new(AtomicBool::new(true));

        let results_bg = Arc::clone(&results);
        let git_inflight_bg = Arc::clone(&git_inflight);
        let manual_git_inflight_bg = Arc::clone(&manual_git_inflight);
        let alive_bg = Arc::clone(&alive);
        let ctx_bg = ctx.clone();

        if let Err(e) = thread::Builder::new()
            .name("git-worker".into())
            .spawn(move || {
                while alive_bg.load(Ordering::Acquire) {
                    let job = match rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(j) => j,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let git_info_path = if let Job::GitInfo(ref p) = job {
                        Some(p.clone())
                    } else {
                        None
                    };
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        match job {
                            Job::GitInfo(p) => {
                                let info = run_git_info(&p);
                                results_bg.lock().git.insert(p.clone(), info);
                                git_inflight_bg.lock().remove(&p);
                                manual_git_inflight_bg.lock().remove(&p);
                            }
                            Job::Stage { cwd, path } => {
                                if super::git_cmd::git_status_ok(&["add", "--", &path], &cwd) {
                                    let info = run_git_info(&cwd);
                                    results_bg.lock().git.insert(cwd.clone(), info);
                                }
                            }
                            Job::Unstage { cwd, path } => {
                                if super::git_cmd::git_status_ok(
                                    &["reset", "HEAD", "--", &path],
                                    &cwd,
                                ) {
                                    let info = run_git_info(&cwd);
                                    results_bg.lock().git.insert(cwd.clone(), info);
                                }
                            }
                            Job::StageAll(cwd) => {
                                if super::git_cmd::git_status_ok(&["add", "-A"], &cwd) {
                                    let info = run_git_info(&cwd);
                                    results_bg.lock().git.insert(cwd.clone(), info);
                                }
                            }
                            Job::UnstageAll(cwd) => {
                                if super::git_cmd::git_status_ok(&["reset", "HEAD"], &cwd) {
                                    let info = run_git_info(&cwd);
                                    results_bg.lock().git.insert(cwd.clone(), info);
                                }
                            }
                            Job::Diff { cwd, rel_path } => {
                                let old_content = super::git_cmd::git_output(
                                    &["show", &format!("HEAD:{}", rel_path)],
                                    &cwd,
                                )
                                .unwrap_or_default();
                                let raw_diff = super::git_cmd::git_output(
                                    &["diff", "HEAD", "--", &rel_path],
                                    &cwd,
                                )
                                .unwrap_or_default();
                                let new_content = std::fs::read_to_string(cwd.join(&rel_path))
                                    .unwrap_or_default();
                                let full_path = cwd.join(&rel_path);

                                let syntax = crate::syntax::find_syntax_for_file(&full_path);
                                let old_highlights = syntax.and_then(|syn| {
                                    if old_content.is_empty() {
                                        None
                                    } else {
                                        Some(crate::syntax::highlighted_lines(&old_content, syn))
                                    }
                                });
                                let new_highlights = syntax.and_then(|syn| {
                                    if new_content.is_empty() {
                                        None
                                    } else {
                                        Some(crate::syntax::highlighted_lines(&new_content, syn))
                                    }
                                });

                                results_bg.lock().diff_results.push(DiffResult {
                                    path: full_path,
                                    old_content,
                                    new_content,
                                    raw_diff,
                                    old_highlights,
                                    new_highlights,
                                    highlight_theme: crate::theme::active().id,
                                });
                            }
                            Job::UnpushedCommits(cwd) => {
                                let parse_log = |s: String| -> Vec<(String, String)> {
                                    s.lines()
                                        .filter_map(|line| {
                                            let (hash, msg) = line.split_once(' ')?;
                                            Some((hash.to_string(), msg.to_string()))
                                        })
                                        .collect()
                                };
                                let commits = super::git_cmd::git_output(
                                    &["log", "--oneline", "@{upstream}..HEAD"],
                                    &cwd,
                                )
                                .map(&parse_log)
                                .or_else(|| {
                                    for remote_branch in &["origin/main", "origin/master"] {
                                        let range = format!("{remote_branch}..HEAD");
                                        if let Some(commits) = super::git_cmd::git_output(
                                            &["log", "--oneline", &range],
                                            &cwd,
                                        )
                                        .map(&parse_log)
                                        {
                                            return Some(commits);
                                        }
                                    }
                                    None
                                })
                                .unwrap_or_default();
                                results_bg.lock().unpushed.insert(cwd, commits);
                            }
                            Job::Commit {
                                cwd,
                                message,
                                amend,
                            } => {
                                // Spawn on a separate thread so commit doesn't
                                // block status/diff queries on the worker thread.
                                let res = Arc::clone(&results_bg);
                                let ctx_c = ctx_bg.clone();
                                let _ = thread::Builder::new().name("git-commit".into()).spawn(
                                    move || {
                                        let mut args = vec!["commit"];
                                        if amend {
                                            args.push("--amend");
                                        }
                                        args.push("-m");
                                        args.push(&message);
                                        let result =
                                            match super::git_cmd::git_stderr_on_fail(&args, &cwd) {
                                                Ok(_) => {
                                                    let info = run_git_info(&cwd);
                                                    res.lock().git.insert(cwd.clone(), info);
                                                    Ok(cwd)
                                                }
                                                Err(stderr) => Err(stderr),
                                            };
                                        res.lock().commit_results.push(result);
                                        ctx_c.request_repaint();
                                    },
                                );
                            }
                            Job::Push { cwd, force } => {
                                // Spawn on a separate thread so push (which can be
                                // very slow over the network) doesn't block the
                                // fast query loop on the worker thread.
                                let res = Arc::clone(&results_bg);
                                let ctx_p = ctx_bg.clone();
                                let _ = thread::Builder::new().name("git-push".into()).spawn(
                                    move || {
                                        let mut args = vec!["push", "origin", "HEAD"];
                                        if force {
                                            args.push("--force");
                                        }
                                        let result =
                                            match super::git_cmd::git_stderr_on_fail(&args, &cwd) {
                                                Ok(_) => Ok(cwd),
                                                Err(stderr) => Err(stderr),
                                            };
                                        res.lock().push_results.push(result);
                                        ctx_p.request_repaint();
                                    },
                                );
                            }
                            Job::LastCommitMessage(cwd) => {
                                let msg =
                                    super::git_cmd::git_output(&["log", "-1", "--format=%B"], &cwd)
                                        .map(|s| s.trim().to_string())
                                        .unwrap_or_default();
                                results_bg.lock().last_commit_msg.insert(cwd, msg);
                            }
                            Job::Gitignore { cwd, pattern } => {
                                let gitignore_path = cwd.join(".gitignore");
                                let result = (|| -> Result<PathBuf, String> {
                                    let mut content = std::fs::read_to_string(&gitignore_path)
                                        .unwrap_or_default();
                                    let already_present =
                                        content.lines().any(|line| line.trim() == pattern.trim());
                                    if already_present {
                                        return Ok(cwd.clone());
                                    }
                                    if !content.is_empty() && !content.ends_with('\n') {
                                        content.push('\n');
                                    }
                                    content.push_str(&pattern);
                                    content.push('\n');
                                    crate::util::atomic_write(&gitignore_path, &content)
                                        .map_err(|e| e.to_string())?;
                                    let info = run_git_info(&cwd);
                                    results_bg.lock().git.insert(cwd.clone(), info);
                                    Ok(cwd.clone())
                                })();
                                results_bg.lock().gitignore_results.push(result);
                            }
                            Job::Revert { cwd, path } => {
                                let result = match super::git_cmd::git_stderr_on_fail(
                                    &["checkout", "HEAD", "--", &path],
                                    &cwd,
                                ) {
                                    Ok(_) => {
                                        let info = run_git_info(&cwd);
                                        results_bg.lock().git.insert(cwd.clone(), info);
                                        Ok(cwd)
                                    }
                                    Err(stderr) => Err(stderr),
                                };
                                results_bg.lock().revert_results.push(result);
                            }
                        }
                    }))
                    .is_err()
                    {
                        log::error!("panic in git-worker job processing");
                        if let Some(p) = &git_info_path {
                            git_inflight_bg.lock().remove(p);
                            manual_git_inflight_bg.lock().remove(p);
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
            manual_git_inflight,
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

    pub(super) fn enqueue_git_manual(&self, path: &Path) {
        self.manual_git_inflight.lock().insert(path.to_path_buf());
        self.enqueue_git(path);
    }

    pub(super) fn is_manual_git_inflight(&self, path: &Path) -> bool {
        self.manual_git_inflight.lock().contains(path)
    }

    pub(super) fn take_git(&self, path: &Path) -> Option<(String, String)> {
        self.results.lock().git.remove(path)
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

    pub(super) fn take_diff_results(&self) -> Vec<DiffResult> {
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

    pub(super) fn take_commit_results(&self) -> Vec<Result<PathBuf, String>> {
        std::mem::take(&mut self.results.lock().commit_results)
    }

    pub(super) fn enqueue_push(&self, cwd: &Path, force: bool) {
        let _ = self.tx.send(Job::Push {
            cwd: cwd.to_path_buf(),
            force,
        });
    }

    pub(super) fn take_push_results(&self) -> Vec<Result<PathBuf, String>> {
        std::mem::take(&mut self.results.lock().push_results)
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

    pub(super) fn take_gitignore_results(&self) -> Vec<Result<PathBuf, String>> {
        std::mem::take(&mut self.results.lock().gitignore_results)
    }

    pub(super) fn enqueue_revert(&self, cwd: &Path, path: String) {
        let _ = self.tx.send(Job::Revert {
            cwd: cwd.to_path_buf(),
            path,
        });
    }

    pub(super) fn take_revert_results(&self) -> Vec<Result<PathBuf, String>> {
        std::mem::take(&mut self.results.lock().revert_results)
    }
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
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
    fn test_take_diff_results_empty() {
        let worker = GitWorker::spawn(egui::Context::default());
        let results = worker.take_diff_results();
        assert!(results.is_empty());
    }

    #[test]
    fn test_enqueue_git_inflight_dedup() {
        let worker = GitWorker::spawn(egui::Context::default());
        let path = PathBuf::from("/some/fake/path");
        // Enqueue the same path twice -- should not panic
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
    fn test_take_gitignore_results_empty() {
        let worker = GitWorker::spawn(egui::Context::default());
        assert!(worker.take_gitignore_results().is_empty());
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
            let results = worker.take_gitignore_results();
            if let Some(r) = results.into_iter().next() {
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
        let tmp =
            std::env::temp_dir().join(format!("git_worker_dedup_test_{}", std::process::id()));
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
            let results = worker.take_gitignore_results();
            if let Some(r) = results.into_iter().next() {
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
    fn test_take_revert_results_empty() {
        let worker = GitWorker::spawn(egui::Context::default());
        assert!(worker.take_revert_results().is_empty());
    }

    #[test]
    fn test_revert_restores_file() {
        let tmp = std::env::temp_dir().join(format!("git_worker_revert_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&tmp)
            .output()
            .ok();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&tmp)
            .output()
            .ok();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&tmp)
            .output()
            .ok();

        let file_path = tmp.join("test.txt");
        std::fs::write(&file_path, "original\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "test.txt"])
            .current_dir(&tmp)
            .output()
            .ok();
        std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&tmp)
            .output()
            .ok();

        std::fs::write(&file_path, "modified\n").unwrap();
        assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "modified\n");

        let worker = GitWorker::spawn(egui::Context::default());
        worker.enqueue_revert(&tmp, "test.txt".to_string());

        let mut result = None;
        for _ in 0..60 {
            std::thread::sleep(Duration::from_millis(50));
            let results = worker.take_revert_results();
            if let Some(r) = results.into_iter().next() {
                result = Some(r);
                break;
            }
        }
        assert!(result.is_some(), "revert job should produce a result");
        assert!(result.unwrap().is_ok());
        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content.trim(), "original");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
