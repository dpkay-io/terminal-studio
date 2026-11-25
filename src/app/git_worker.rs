use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;

use super::file_browser::run_git_info;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MergeOperation {
    Merge {
        source_branch: String,
    },
    Rebase {
        onto: String,
        current_step: usize,
        total_steps: usize,
    },
    CherryPick {
        commit: String,
    },
    None,
}

/// Detect any in-progress merge/rebase/cherry-pick operation for the git repo
/// at `dir`. Returns `MergeOperation::None` if the directory is not a git repo
/// or no operation is in progress.
pub(super) fn detect_merge_operation(dir: &std::path::Path) -> MergeOperation {
    let git_root = match crate::util::find_git_root(dir) {
        Some(r) => r,
        Option::None => return MergeOperation::None,
    };
    let dot_git_path = git_root.join(".git");
    let dot_git = if dot_git_path.is_file() {
        // Worktree: .git is a file containing "gitdir: /path/to/actual/git/dir"
        std::fs::read_to_string(&dot_git_path)
            .ok()
            .and_then(|content| {
                content
                    .trim()
                    .strip_prefix("gitdir: ")
                    .map(|p| std::path::PathBuf::from(p.trim()))
            })
            .unwrap_or(dot_git_path)
    } else {
        dot_git_path
    };

    // rebase-merge (interactive rebase)
    let rebase_merge = dot_git.join("rebase-merge");
    if rebase_merge.is_dir() {
        let onto = std::fs::read_to_string(rebase_merge.join("onto"))
            .unwrap_or_default()
            .trim()
            .to_string();
        let current_step = std::fs::read_to_string(rebase_merge.join("msgnum"))
            .unwrap_or_default()
            .trim()
            .parse::<usize>()
            .unwrap_or(0);
        let total_steps = std::fs::read_to_string(rebase_merge.join("end"))
            .unwrap_or_default()
            .trim()
            .parse::<usize>()
            .unwrap_or(0);
        return MergeOperation::Rebase {
            onto,
            current_step,
            total_steps,
        };
    }

    // rebase-apply (am-style rebase)
    let rebase_apply = dot_git.join("rebase-apply");
    if rebase_apply.is_dir() {
        let onto = std::fs::read_to_string(rebase_apply.join("onto"))
            .unwrap_or_default()
            .trim()
            .to_string();
        let current_step = std::fs::read_to_string(rebase_apply.join("next"))
            .unwrap_or_default()
            .trim()
            .parse::<usize>()
            .unwrap_or(0);
        let total_steps = std::fs::read_to_string(rebase_apply.join("last"))
            .unwrap_or_default()
            .trim()
            .parse::<usize>()
            .unwrap_or(0);
        return MergeOperation::Rebase {
            onto,
            current_step,
            total_steps,
        };
    }

    // MERGE_HEAD — merge in progress
    let merge_head = dot_git.join("MERGE_HEAD");
    if merge_head.exists() {
        let source_branch = std::fs::read_to_string(dot_git.join("MERGE_MSG"))
            .unwrap_or_default()
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        return MergeOperation::Merge { source_branch };
    }

    // CHERRY_PICK_HEAD — cherry-pick in progress
    let cherry_pick_head = dot_git.join("CHERRY_PICK_HEAD");
    if cherry_pick_head.exists() {
        let commit = std::fs::read_to_string(&cherry_pick_head)
            .unwrap_or_default()
            .trim()
            .to_string();
        return MergeOperation::CherryPick { commit };
    }

    MergeOperation::None
}

/// The ignore file the git panel's "Add to .gitignore" writes to.
const GITIGNORE_FILE: &str = ".gitignore";

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
    /// The changes to one file (absolute path).
    Diff(PathBuf),
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
    DeleteFile {
        cwd: PathBuf,
        path: String,
    },
    MergeAbort(PathBuf),
    MergeContinue(PathBuf),
    RebaseAbort(PathBuf),
    RebaseContinue(PathBuf),
    CherryPickAbort(PathBuf),
    CherryPickContinue(PathBuf),
}

/// A file's text at `HEAD`, its text now, and the changes between them.
/// A file git does not know yet (untracked, so `git diff` shows nothing)
/// is all added lines, so the diff view colours it and its change arrows
/// have something to step to.
fn file_diff(full_path: &Path) -> (String, String, Vec<super::diff_parser::DiffHunk>) {
    let new_content = std::fs::read_to_string(full_path).unwrap_or_default();
    // From the repository root, where both `HEAD:<path>` and `-- <path>`
    // read the path from (a terminal may be in any folder inside it).
    let Some((root, rel_path)) = super::git_paths::locate(full_path) else {
        return (String::new(), new_content, Vec::new());
    };
    let in_head = super::git_cmd::git_output(&["show", &format!("HEAD:{rel_path}")], &root);
    let raw_diff =
        super::git_cmd::git_output(&["diff", "HEAD", "--", &rel_path], &root).unwrap_or_default();
    let mut hunks = super::diff_parser::parse_diff(&raw_diff);
    if hunks.is_empty() && in_head.is_none() {
        hunks = super::diff_parser::new_file_hunks(&new_content);
    }
    (in_head.unwrap_or_default(), new_content, hunks)
}

pub(super) struct DiffResult {
    pub(super) path: PathBuf,
    pub(super) old_content: String,
    pub(super) new_content: String,
    /// Parsed here, off the UI thread.
    pub(super) hunks: Vec<super::diff_parser::DiffHunk>,
    pub(super) old_highlights: Option<Vec<Vec<(egui::Color32, String)>>>,
    pub(super) new_highlights: Option<Vec<Vec<(egui::Color32, String)>>>,
    pub(super) highlight_theme: crate::theme::ThemeId,
}

pub(super) struct WorkerResults {
    pub(super) git: HashMap<PathBuf, (String, String, MergeOperation)>,
    pub(super) diff_results: Vec<DiffResult>,
    pub(super) unpushed: HashMap<PathBuf, Vec<(String, String)>>,
    pub(super) commit_results: Vec<Result<PathBuf, String>>,
    pub(super) push_results: Vec<Result<PathBuf, String>>,
    pub(super) last_commit_msg: HashMap<PathBuf, String>,
    pub(super) gitignore_results: Vec<Result<PathBuf, String>>,
    pub(super) revert_results: Vec<Result<PathBuf, String>>,
    pub(super) delete_results: Vec<Result<PathBuf, String>>,
    pub(super) merge_op_results: Vec<Result<PathBuf, String>>,
}

pub(super) struct GitWorker {
    tx: mpsc::Sender<Job>,
    results: Arc<Mutex<WorkerResults>>,
    git_inflight: Arc<Mutex<HashSet<PathBuf>>>,
    manual_git_inflight: Arc<Mutex<HashSet<PathBuf>>>,
    stage_ops_inflight: Arc<AtomicUsize>,
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
            delete_results: Vec::new(),
            merge_op_results: Vec::new(),
        }));
        let git_inflight: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
        let manual_git_inflight: Arc<Mutex<HashSet<PathBuf>>> =
            Arc::new(Mutex::new(HashSet::new()));
        let stage_ops_inflight = Arc::new(AtomicUsize::new(0));
        let alive = Arc::new(AtomicBool::new(true));

        let results_bg = Arc::clone(&results);
        let git_inflight_bg = Arc::clone(&git_inflight);
        let manual_git_inflight_bg = Arc::clone(&manual_git_inflight);
        let stage_ops_bg = Arc::clone(&stage_ops_inflight);
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
                                let (diff, status) = run_git_info(&p);
                                let merge_op = detect_merge_operation(&p);
                                results_bg
                                    .lock()
                                    .git
                                    .insert(p.clone(), (diff, status, merge_op));
                                git_inflight_bg.lock().remove(&p);
                                manual_git_inflight_bg.lock().remove(&p);
                            }
                            Job::Stage { cwd, path } => {
                                let root = super::git_paths::repo_root_or_self(&cwd);
                                if super::git_cmd::git_status_ok(&["add", "--", &path], &root) {
                                    let (diff, status) = run_git_info(&cwd);
                                    let merge_op = detect_merge_operation(&cwd);
                                    results_bg
                                        .lock()
                                        .git
                                        .insert(cwd.clone(), (diff, status, merge_op));
                                }
                                stage_ops_bg.fetch_sub(1, Ordering::Release);
                            }
                            Job::Unstage { cwd, path } => {
                                let root = super::git_paths::repo_root_or_self(&cwd);
                                if super::git_cmd::git_status_ok(
                                    &["reset", "HEAD", "--", &path],
                                    &root,
                                ) {
                                    let (diff, status) = run_git_info(&cwd);
                                    let merge_op = detect_merge_operation(&cwd);
                                    results_bg
                                        .lock()
                                        .git
                                        .insert(cwd.clone(), (diff, status, merge_op));
                                }
                                stage_ops_bg.fetch_sub(1, Ordering::Release);
                            }
                            Job::StageAll(cwd) => {
                                if super::git_cmd::git_status_ok(&["add", "-A"], &cwd) {
                                    let (diff, status) = run_git_info(&cwd);
                                    let merge_op = detect_merge_operation(&cwd);
                                    results_bg
                                        .lock()
                                        .git
                                        .insert(cwd.clone(), (diff, status, merge_op));
                                }
                                stage_ops_bg.fetch_sub(1, Ordering::Release);
                            }
                            Job::UnstageAll(cwd) => {
                                if super::git_cmd::git_status_ok(&["reset", "HEAD"], &cwd) {
                                    let (diff, status) = run_git_info(&cwd);
                                    let merge_op = detect_merge_operation(&cwd);
                                    results_bg
                                        .lock()
                                        .git
                                        .insert(cwd.clone(), (diff, status, merge_op));
                                }
                                stage_ops_bg.fetch_sub(1, Ordering::Release);
                            }
                            Job::Diff(full_path) => {
                                let (old_content, new_content, hunks) = file_diff(&full_path);

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
                                    hunks,
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
                                .map(parse_log)
                                .or_else(|| {
                                    for remote_branch in &["origin/main", "origin/master"] {
                                        let range = format!("{remote_branch}..HEAD");
                                        if let Some(commits) = super::git_cmd::git_output(
                                            &["log", "--oneline", &range],
                                            &cwd,
                                        )
                                        .map(parse_log)
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
                                                    let (diff, status) = run_git_info(&cwd);
                                                    let merge_op = detect_merge_operation(&cwd);
                                                    res.lock().git.insert(
                                                        cwd.clone(),
                                                        (diff, status, merge_op),
                                                    );
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
                                // The repository's own `.gitignore`: the
                                // pattern is a path from its root.
                                let gitignore_path =
                                    super::git_paths::repo_root_or_self(&cwd).join(GITIGNORE_FILE);
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
                                    let (diff, status) = run_git_info(&cwd);
                                    let merge_op = detect_merge_operation(&cwd);
                                    results_bg
                                        .lock()
                                        .git
                                        .insert(cwd.clone(), (diff, status, merge_op));
                                    Ok(cwd.clone())
                                })();
                                results_bg.lock().gitignore_results.push(result);
                            }
                            Job::Revert { cwd, path } => {
                                let root = super::git_paths::repo_root_or_self(&cwd);
                                let result = match super::git_cmd::git_stderr_on_fail(
                                    &["checkout", "HEAD", "--", &path],
                                    &root,
                                ) {
                                    Ok(_) => {
                                        let (diff, status) = run_git_info(&cwd);
                                        let merge_op = detect_merge_operation(&cwd);
                                        results_bg
                                            .lock()
                                            .git
                                            .insert(cwd.clone(), (diff, status, merge_op));
                                        Ok(cwd)
                                    }
                                    Err(stderr) => Err(stderr),
                                };
                                results_bg.lock().revert_results.push(result);
                            }
                            Job::DeleteFile { cwd, path } => {
                                let full = super::git_paths::repo_root_or_self(&cwd).join(&path);
                                let result = if full.is_dir() {
                                    std::fs::remove_dir_all(&full)
                                } else {
                                    std::fs::remove_file(&full)
                                };
                                let result = match result {
                                    Ok(()) => {
                                        let (diff, status) = run_git_info(&cwd);
                                        let merge_op = detect_merge_operation(&cwd);
                                        results_bg
                                            .lock()
                                            .git
                                            .insert(cwd.clone(), (diff, status, merge_op));
                                        Ok(cwd)
                                    }
                                    Err(e) => Err(e.to_string()),
                                };
                                results_bg.lock().delete_results.push(result);
                            }
                            Job::MergeAbort(cwd) => {
                                let result = match super::git_cmd::git_stderr_on_fail(
                                    &["merge", "--abort"],
                                    &cwd,
                                ) {
                                    Ok(_) => {
                                        let (diff, status) = run_git_info(&cwd);
                                        let merge_op = detect_merge_operation(&cwd);
                                        results_bg
                                            .lock()
                                            .git
                                            .insert(cwd.clone(), (diff, status, merge_op));
                                        Ok(cwd)
                                    }
                                    Err(stderr) => Err(stderr),
                                };
                                results_bg.lock().merge_op_results.push(result);
                            }
                            Job::MergeContinue(cwd) => {
                                let result = match super::git_cmd::git_stderr_on_fail(
                                    &["merge", "--continue"],
                                    &cwd,
                                ) {
                                    Ok(_) => {
                                        let (diff, status) = run_git_info(&cwd);
                                        let merge_op = detect_merge_operation(&cwd);
                                        results_bg
                                            .lock()
                                            .git
                                            .insert(cwd.clone(), (diff, status, merge_op));
                                        Ok(cwd)
                                    }
                                    Err(stderr) => Err(stderr),
                                };
                                results_bg.lock().merge_op_results.push(result);
                            }
                            Job::RebaseAbort(cwd) => {
                                let result = match super::git_cmd::git_stderr_on_fail(
                                    &["rebase", "--abort"],
                                    &cwd,
                                ) {
                                    Ok(_) => {
                                        let (diff, status) = run_git_info(&cwd);
                                        let merge_op = detect_merge_operation(&cwd);
                                        results_bg
                                            .lock()
                                            .git
                                            .insert(cwd.clone(), (diff, status, merge_op));
                                        Ok(cwd)
                                    }
                                    Err(stderr) => Err(stderr),
                                };
                                results_bg.lock().merge_op_results.push(result);
                            }
                            Job::RebaseContinue(cwd) => {
                                let result = match super::git_cmd::git_stderr_on_fail(
                                    &["rebase", "--continue"],
                                    &cwd,
                                ) {
                                    Ok(_) => {
                                        let (diff, status) = run_git_info(&cwd);
                                        let merge_op = detect_merge_operation(&cwd);
                                        results_bg
                                            .lock()
                                            .git
                                            .insert(cwd.clone(), (diff, status, merge_op));
                                        Ok(cwd)
                                    }
                                    Err(stderr) => Err(stderr),
                                };
                                results_bg.lock().merge_op_results.push(result);
                            }
                            Job::CherryPickAbort(cwd) => {
                                let result = match super::git_cmd::git_stderr_on_fail(
                                    &["cherry-pick", "--abort"],
                                    &cwd,
                                ) {
                                    Ok(_) => {
                                        let (diff, status) = run_git_info(&cwd);
                                        let merge_op = detect_merge_operation(&cwd);
                                        results_bg
                                            .lock()
                                            .git
                                            .insert(cwd.clone(), (diff, status, merge_op));
                                        Ok(cwd)
                                    }
                                    Err(stderr) => Err(stderr),
                                };
                                results_bg.lock().merge_op_results.push(result);
                            }
                            Job::CherryPickContinue(cwd) => {
                                let result = match super::git_cmd::git_stderr_on_fail(
                                    &["cherry-pick", "--continue"],
                                    &cwd,
                                ) {
                                    Ok(_) => {
                                        let (diff, status) = run_git_info(&cwd);
                                        let merge_op = detect_merge_operation(&cwd);
                                        results_bg
                                            .lock()
                                            .git
                                            .insert(cwd.clone(), (diff, status, merge_op));
                                        Ok(cwd)
                                    }
                                    Err(stderr) => Err(stderr),
                                };
                                results_bg.lock().merge_op_results.push(result);
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
            stage_ops_inflight,
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

    pub(super) fn take_all_git(&self) -> HashMap<PathBuf, (String, String, MergeOperation)> {
        std::mem::take(&mut self.results.lock().git)
    }

    pub(super) fn take_all_unpushed(&self) -> HashMap<PathBuf, Vec<(String, String)>> {
        std::mem::take(&mut self.results.lock().unpushed)
    }

    pub(super) fn enqueue_stage(&self, cwd: &Path, path: String) {
        self.stage_ops_inflight.fetch_add(1, Ordering::Release);
        let _ = self.tx.send(Job::Stage {
            cwd: cwd.to_path_buf(),
            path,
        });
    }

    pub(super) fn enqueue_unstage(&self, cwd: &Path, path: String) {
        self.stage_ops_inflight.fetch_add(1, Ordering::Release);
        let _ = self.tx.send(Job::Unstage {
            cwd: cwd.to_path_buf(),
            path,
        });
    }

    pub(super) fn enqueue_stage_all(&self, cwd: &Path) {
        self.stage_ops_inflight.fetch_add(1, Ordering::Release);
        let _ = self.tx.send(Job::StageAll(cwd.to_path_buf()));
    }

    pub(super) fn enqueue_unstage_all(&self, cwd: &Path) {
        self.stage_ops_inflight.fetch_add(1, Ordering::Release);
        let _ = self.tx.send(Job::UnstageAll(cwd.to_path_buf()));
    }

    pub(super) fn has_stage_ops_inflight(&self) -> bool {
        self.stage_ops_inflight.load(Ordering::Acquire) > 0
    }

    /// Asks for the changes to `file` (absolute); the result carries the
    /// same path.
    pub(super) fn enqueue_diff(&self, file: PathBuf) {
        let _ = self.tx.send(Job::Diff(file));
    }

    pub(super) fn take_diff_results(&self) -> Vec<DiffResult> {
        let mut lock = self.results.lock();
        std::mem::take(&mut lock.diff_results)
    }

    pub(super) fn enqueue_unpushed(&self, cwd: &Path) {
        let _ = self.tx.send(Job::UnpushedCommits(cwd.to_path_buf()));
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

    pub(super) fn enqueue_delete(&self, cwd: &Path, path: String) {
        let _ = self.tx.send(Job::DeleteFile {
            cwd: cwd.to_path_buf(),
            path,
        });
    }

    pub(super) fn take_delete_results(&self) -> Vec<Result<PathBuf, String>> {
        std::mem::take(&mut self.results.lock().delete_results)
    }

    pub(super) fn enqueue_merge_abort(&self, cwd: &Path) {
        let _ = self.tx.send(Job::MergeAbort(cwd.to_path_buf()));
    }

    pub(super) fn enqueue_merge_continue(&self, cwd: &Path) {
        let _ = self.tx.send(Job::MergeContinue(cwd.to_path_buf()));
    }

    pub(super) fn enqueue_rebase_abort(&self, cwd: &Path) {
        let _ = self.tx.send(Job::RebaseAbort(cwd.to_path_buf()));
    }

    pub(super) fn enqueue_rebase_continue(&self, cwd: &Path) {
        let _ = self.tx.send(Job::RebaseContinue(cwd.to_path_buf()));
    }

    pub(super) fn enqueue_cherry_pick_abort(&self, cwd: &Path) {
        let _ = self.tx.send(Job::CherryPickAbort(cwd.to_path_buf()));
    }

    pub(super) fn enqueue_cherry_pick_continue(&self, cwd: &Path) {
        let _ = self.tx.send(Job::CherryPickContinue(cwd.to_path_buf()));
    }

    pub(super) fn take_merge_op_results(&self) -> Vec<Result<PathBuf, String>> {
        std::mem::take(&mut self.results.lock().merge_op_results)
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
    fn test_take_all_git_empty() {
        let worker = GitWorker::spawn(egui::Context::default());
        let results = worker.take_all_git();
        assert!(results.is_empty());
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
            let results = worker.take_all_git();
            if let Some(r) = results.into_iter().find(|(p, _)| *p == cwd) {
                result = Some(r.1);
                break;
            }
        }
        assert!(result.is_some(), "git worker should have produced a result");
        let (diff, status, merge_op) = result.unwrap();
        // diff and status may both be empty on a clean working tree
        let _ = (diff, status, merge_op);
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

    fn init_temp_git_repo(suffix: &str) -> std::path::PathBuf {
        let tmp =
            std::env::temp_dir().join(format!("merge_op_test_{}_{}", suffix, std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(&tmp)
            .output()
            .ok();
        tmp
    }

    #[test]
    fn detect_merge_state_none() {
        let tmp = init_temp_git_repo("none");
        let op = detect_merge_operation(&tmp);
        assert_eq!(op, MergeOperation::None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_merge_state_merge() {
        let tmp = init_temp_git_repo("merge");
        let dot_git = tmp.join(".git");
        std::fs::write(dot_git.join("MERGE_HEAD"), "abc123\n").unwrap();
        std::fs::write(dot_git.join("MERGE_MSG"), "Merge branch 'feature'\n").unwrap();
        let op = detect_merge_operation(&tmp);
        assert!(
            matches!(op, MergeOperation::Merge { ref source_branch } if source_branch == "Merge branch 'feature'"),
            "expected Merge, got {:?}",
            op
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_rebase_interactive() {
        let tmp = init_temp_git_repo("rebase_interactive");
        let rebase_merge = tmp.join(".git").join("rebase-merge");
        std::fs::create_dir_all(&rebase_merge).unwrap();
        std::fs::write(rebase_merge.join("onto"), "deadbeef\n").unwrap();
        std::fs::write(rebase_merge.join("msgnum"), "2\n").unwrap();
        std::fs::write(rebase_merge.join("end"), "5\n").unwrap();
        std::fs::write(rebase_merge.join("head-name"), "refs/heads/main\n").unwrap();
        let op = detect_merge_operation(&tmp);
        assert!(
            matches!(
                op,
                MergeOperation::Rebase { ref onto, current_step: 2, total_steps: 5 }
                if onto == "deadbeef"
            ),
            "expected Rebase (interactive), got {:?}",
            op
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_rebase_am_style() {
        let tmp = init_temp_git_repo("rebase_am");
        let rebase_apply = tmp.join(".git").join("rebase-apply");
        std::fs::create_dir_all(&rebase_apply).unwrap();
        std::fs::write(rebase_apply.join("onto"), "cafebabe\n").unwrap();
        std::fs::write(rebase_apply.join("next"), "1\n").unwrap();
        std::fs::write(rebase_apply.join("last"), "3\n").unwrap();
        let op = detect_merge_operation(&tmp);
        assert!(
            matches!(
                op,
                MergeOperation::Rebase { ref onto, current_step: 1, total_steps: 3 }
                if onto == "cafebabe"
            ),
            "expected Rebase (am-style), got {:?}",
            op
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_cherry_pick() {
        let tmp = init_temp_git_repo("cherry_pick");
        let dot_git = tmp.join(".git");
        std::fs::write(dot_git.join("CHERRY_PICK_HEAD"), "feedface\n").unwrap();
        let op = detect_merge_operation(&tmp);
        assert!(
            matches!(op, MergeOperation::CherryPick { ref commit } if commit == "feedface"),
            "expected CherryPick, got {:?}",
            op
        );
        let _ = std::fs::remove_dir_all(&tmp);
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

    /// A repo with `tracked.txt` and `sub/deep.txt` committed ("a b c")
    /// then edited (b → B), and `new.txt` never added.
    fn repo_with_changes(suffix: &str) -> PathBuf {
        let tmp = std::env::temp_dir().join(format!("file_diff_{suffix}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(&tmp)
                .output()
                .unwrap();
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "test@test.com"]);
        git(&["config", "user.name", "Test"]);
        // The tests compare exact bytes; Git for Windows converts line
        // endings on checkout unless the repository says not to.
        git(&["config", "core.autocrlf", "false"]);
        std::fs::create_dir_all(tmp.join(SUBFOLDER)).unwrap();
        for file in ["tracked.txt", DEEP_FILE] {
            std::fs::write(tmp.join(file), "a\nb\nc\n").unwrap();
        }
        git(&["add", "."]);
        git(&["commit", "-qm", "init"]);
        for file in ["tracked.txt", DEEP_FILE] {
            std::fs::write(tmp.join(file), "a\nB\nc\n").unwrap();
        }
        std::fs::write(tmp.join("new.txt"), "x\ny\n").unwrap();
        tmp
    }

    #[test]
    fn an_edited_file_has_its_changed_hunk() {
        let repo = repo_with_changes("edited");
        let (old, new, hunks) = file_diff(&repo.join("tracked.txt"));
        assert_eq!(old, "a\nb\nc\n");
        assert_eq!(new, "a\nB\nc\n");
        assert_eq!(hunks.len(), 1);
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn a_new_untracked_file_is_one_added_hunk() {
        let repo = repo_with_changes("untracked");
        let (old, new, hunks) = file_diff(&repo.join("new.txt"));
        assert_eq!(old, "", "nothing at HEAD");
        assert_eq!(new, "x\ny\n");
        assert_eq!(hunks.len(), 1, "the arrows have a change to step to");
        assert_eq!(hunks[0].new_count, 2);
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn the_diff_job_sends_its_hunks() {
        let repo = repo_with_changes("job");
        let worker = GitWorker::spawn(egui::Context::default());
        worker.enqueue_diff(repo.join("new.txt"));
        let mut result = None;
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            if let Some(r) = worker.take_diff_results().into_iter().next() {
                result = Some(r);
                break;
            }
        }
        let result = result.expect("the diff job finished");
        assert_eq!(result.path, repo.join("new.txt"));
        assert_eq!(result.hunks.len(), 1);
        let _ = std::fs::remove_dir_all(&repo);
    }

    /// A folder inside the test repo, and a committed file in it.
    const SUBFOLDER: &str = "sub";
    const DEEP_FILE: &str = "sub/deep.txt";

    /// What `git` prints in `repo` for `args`.
    fn git_in(repo: &Path, args: &[&str]) -> String {
        super::super::git_cmd::git_output(args, repo).unwrap_or_default()
    }

    /// Waits for `take` to give something, up to five seconds.
    fn wait_for<T>(mut take: impl FnMut() -> Option<T>) -> Option<T> {
        for _ in 0..100 {
            if let Some(v) = take() {
                return Some(v);
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    #[test]
    fn a_file_in_a_subfolder_has_its_diff() {
        let repo = repo_with_changes("deep");
        let (old, new, hunks) = file_diff(&repo.join(DEEP_FILE));
        assert_eq!(old, "a\nb\nc\n", "read at HEAD from the repository root");
        assert_eq!(new, "a\nB\nc\n");
        assert_eq!(hunks.len(), 1);
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[test]
    fn status_paths_act_on_the_right_file_from_a_subfolder() {
        // A terminal in `sub/`: the panel's paths are from the repository
        // root (`sub/deep.txt`), as `git status --porcelain` gives them.
        let repo = repo_with_changes("from_sub");
        let status = git_in(&repo.join(SUBFOLDER), &["status", "--porcelain"]);
        assert!(
            status.contains(DEEP_FILE),
            "porcelain paths are root-relative: {status}"
        );
        let cwd = repo.join(SUBFOLDER);
        let worker = GitWorker::spawn(egui::Context::default());

        worker.enqueue_stage(&cwd, DEEP_FILE.to_string());
        assert!(wait_for(|| worker.take_all_git().remove(&cwd)).is_some());
        assert_eq!(
            git_in(&repo, &["diff", "--cached", "--name-only"]).trim(),
            DEEP_FILE
        );

        worker.enqueue_unstage(&cwd, DEEP_FILE.to_string());
        assert!(wait_for(|| worker.take_all_git().remove(&cwd)).is_some());
        assert_eq!(
            git_in(&repo, &["diff", "--cached", "--name-only"]).trim(),
            ""
        );

        worker.enqueue_revert(&cwd, DEEP_FILE.to_string());
        let reverted = wait_for(|| worker.take_revert_results().into_iter().next());
        assert!(matches!(reverted, Some(Ok(_))), "{reverted:?}");
        assert_eq!(
            std::fs::read_to_string(repo.join(DEEP_FILE)).unwrap(),
            "a\nb\nc\n"
        );

        worker.enqueue_gitignore(&cwd, "new.txt".to_string());
        assert!(wait_for(|| worker.take_gitignore_results().into_iter().next()).is_some());
        assert!(
            repo.join(GITIGNORE_FILE).is_file(),
            "the repository's own ignore file"
        );
        assert!(!cwd.join(GITIGNORE_FILE).exists());

        worker.enqueue_delete(&cwd, "new.txt".to_string());
        let deleted = wait_for(|| worker.take_delete_results().into_iter().next());
        assert!(matches!(deleted, Some(Ok(_))), "{deleted:?}");
        assert!(!repo.join("new.txt").exists());
        let _ = std::fs::remove_dir_all(&repo);
    }
}
