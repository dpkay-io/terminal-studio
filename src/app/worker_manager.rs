use crate::pty::foreground_worker::ForegroundWorker;
use crate::sys_monitor::SysMonitor;
use crate::updater::UpdateChecker;

use super::git_worker::GitWorker;
use super::workspace_git_worker::WorkspaceGitWorker;

/// Holds background-worker and thread state extracted from `App`.
///
/// Contains the foreground-process detector, git worker, search workers,
/// system monitor, and update checker.
pub(super) struct WorkerManager {
    /// Background thread for foreground-process detection; UI reads from cache only.
    pub(super) foreground_worker: ForegroundWorker,
    /// Background worker for git status/diff and directory listing.
    pub(super) git_worker: GitWorker,
    /// Background worker for workspace git branch/diff info (lazy, 15 s TTL).
    pub(super) workspace_git_worker: WorkspaceGitWorker,
    /// Global search across all sessions (Ctrl+Shift+T).
    pub(super) search_worker: crate::search_worker::SearchWorker,
    /// Background file search worker for directory panel.
    pub(super) file_search_worker: crate::file_search_worker::FileSearchWorker,
    /// System resource monitor (CPU / RAM / Network), polled every 2 s.
    pub(super) sys_monitor: Option<SysMonitor>,
    /// Self-update: background checker for new releases.
    pub(super) update_checker: Option<UpdateChecker>,
}
