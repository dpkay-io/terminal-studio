use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;

use super::foreground::{detect_child, find_descendant_pids, ForegroundProcess};
use crate::app::claude_session::{
    is_claude_process, lookup_claude_session_id, lookup_claude_session_id_by_cwd,
};

/// How a terminal's Claude conversation was found.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ClaudeSessionFound {
    /// From the session file of the Claude process the terminal runs: certain.
    ByPid(String),
    /// The newest conversation in the terminal's folder no other terminal
    /// holds: a guess, used while the process's own file can't be read.
    ByFolder(String),
}

/// The conversations terminals other than `sid` hold.
fn held_by_others(map: &HashMap<u32, String>, sid: u32) -> HashSet<String> {
    map.iter()
        .filter(|(other, _)| **other != sid)
        .map(|(_, id)| id.clone())
        .collect()
}

/// Records `sid`'s conversation. A certain match takes the id from any
/// other terminal that had guessed it; a guess never takes one from
/// another terminal (two Claude sessions in one folder would otherwise
/// trade ids on every poll and could lose theirs at close).
fn record_claude_session(map: &mut HashMap<u32, String>, sid: u32, found: ClaudeSessionFound) {
    let id = match found {
        ClaudeSessionFound::ByPid(id) => {
            map.retain(|other, held| *other == sid || *held != id);
            id
        }
        ClaudeSessionFound::ByFolder(id) => {
            if map.iter().any(|(other, held)| *other != sid && *held == id) {
                return;
            }
            id
        }
    };
    map.insert(sid, id);
}

/// Background thread that polls foreground-process detection every 500 ms for
/// all registered sessions.  The UI thread reads from the shared cache without
/// ever calling the OS APIs directly, eliminating 10-50 ms UI stalls on
/// CreateToolhelp32Snapshot (Windows) or /proc scans (Linux).
pub struct ForegroundWorker {
    cache: Arc<Mutex<HashMap<u32, Option<ForegroundProcess>>>>,
    /// (session_id, shell_pid, cwd)
    pids: Arc<Mutex<Vec<(u32, u32, String)>>>,
    alive: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    /// Maps terminal session_id → Claude session UUID. Pruned by `set_sessions()`
    /// on session removal. UI sync clears stale entries the worker no longer confirms.
    claude_sessions: Arc<Mutex<HashMap<u32, String>>>,
}

impl ForegroundWorker {
    pub fn spawn() -> Self {
        let cache: Arc<Mutex<HashMap<u32, Option<ForegroundProcess>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pids: Arc<Mutex<Vec<(u32, u32, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let alive = Arc::new(AtomicBool::new(true));

        let claude_sessions: Arc<Mutex<HashMap<u32, String>>> =
            Arc::new(Mutex::new(HashMap::new()));

        let cache_bg = cache.clone();
        let pids_bg = pids.clone();
        let alive_bg = alive.clone();
        let claude_bg = claude_sessions.clone();

        let handle = match thread::Builder::new()
            .name("foreground-detector".into())
            .spawn(move || {
                while alive_bg.load(Ordering::Acquire) {
                    let snapshot: Vec<(u32, u32, String)> = pids_bg.lock().clone();
                    for (sid, shell_pid, cwd) in snapshot {
                        if shell_pid == u32::MAX {
                            continue;
                        }
                        let result = detect_child(shell_pid);
                        if let Some(ref proc) = result {
                            if is_claude_process(&proc.name, &proc.cmdline) {
                                if let Some(pid) = proc.pid {
                                    let found = lookup_claude_session_id(pid)
                                        .or_else(|| {
                                            find_descendant_pids(pid)
                                                .into_iter()
                                                .find_map(lookup_claude_session_id)
                                        })
                                        .map(ClaudeSessionFound::ByPid)
                                        .or_else(|| {
                                            if cwd.is_empty() {
                                                return None;
                                            }
                                            let held = held_by_others(&claude_bg.lock(), sid);
                                            lookup_claude_session_id_by_cwd(&cwd, &held)
                                                .map(ClaudeSessionFound::ByFolder)
                                        });
                                    if let Some(found) = found {
                                        record_claude_session(&mut claude_bg.lock(), sid, found);
                                    }
                                }
                            } else {
                                claude_bg.lock().remove(&sid);
                            }
                        } else {
                            claude_bg.lock().remove(&sid);
                        }
                        cache_bg.lock().insert(sid, result);
                    }
                    thread::sleep(Duration::from_millis(500));
                }
            }) {
            Ok(h) => Some(h),
            Err(e) => {
                log::error!("failed to spawn foreground-detector thread: {e}");
                None
            }
        };

        ForegroundWorker {
            cache,
            pids,
            alive,
            thread: handle,
            claude_sessions,
        }
    }

    /// Update the set of sessions to poll.  Call whenever sessions are added or removed.
    /// Each entry is `(session_id, shell_pid, cwd)`.
    /// Cheap: replaces the inner Vec and prunes stale cache entries.
    pub fn set_sessions(&self, sessions: Vec<(u32, u32, String)>) {
        let active_ids: std::collections::HashSet<u32> =
            sessions.iter().map(|(sid, _, _)| *sid).collect();
        *self.pids.lock() = sessions;
        self.cache.lock().retain(|sid, _| active_ids.contains(sid));
        self.claude_sessions
            .lock()
            .retain(|sid, _| active_ids.contains(sid));
    }

    /// Read the cached result for `session_id`.  Never blocks on OS APIs.
    pub fn get(&self, session_id: u32) -> Option<ForegroundProcess> {
        self.cache.lock().get(&session_id)?.clone()
    }

    /// True once the worker has polled this session at least once.
    pub fn has_polled(&self, session_id: u32) -> bool {
        self.cache.lock().contains_key(&session_id)
    }

    /// Return the Claude session UUID for `session_id`, if one has been detected.
    pub fn get_claude_session_id(&self, session_id: u32) -> Option<String> {
        self.claude_sessions.lock().get(&session_id).cloned()
    }
}

impl Drop for ForegroundWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_sessions_empty_by_default() {
        let worker = ForegroundWorker::spawn();
        assert!(worker.get_claude_session_id(1).is_none());
    }

    #[test]
    fn test_claude_sessions_set_and_get() {
        let worker = ForegroundWorker::spawn();
        worker
            .claude_sessions
            .lock()
            .insert(1, "abc-123".to_string());
        assert_eq!(worker.get_claude_session_id(1), Some("abc-123".to_string()));
    }

    #[test]
    fn test_set_sessions_prunes_claude_cache() {
        let worker = ForegroundWorker::spawn();
        worker
            .claude_sessions
            .lock()
            .insert(1, "abc-123".to_string());
        worker
            .claude_sessions
            .lock()
            .insert(2, "def-456".to_string());
        worker.set_sessions(vec![(1, 100, String::new())]);
        assert_eq!(worker.get_claude_session_id(1), Some("abc-123".to_string()));
        assert!(worker.get_claude_session_id(2).is_none());
    }

    #[test]
    fn test_claude_sessions_never_evicts_on_poll() {
        let worker = ForegroundWorker::spawn();
        worker
            .claude_sessions
            .lock()
            .insert(5, "xyz-789".to_string());
        assert_eq!(worker.get_claude_session_id(5), Some("xyz-789".to_string()));
    }

    fn map(entries: &[(u32, &str)]) -> HashMap<u32, String> {
        entries
            .iter()
            .map(|(sid, id)| (*sid, id.to_string()))
            .collect()
    }

    #[test]
    fn pid_match_takes_the_id_from_a_terminal_that_guessed_it() {
        let mut m = map(&[(1, "x"), (2, "y")]);
        record_claude_session(&mut m, 3, ClaudeSessionFound::ByPid("x".into()));
        assert_eq!(m, map(&[(2, "y"), (3, "x")]));
    }

    #[test]
    fn folder_guess_never_takes_another_terminals_id() {
        let mut m = map(&[(1, "x")]);
        record_claude_session(&mut m, 2, ClaudeSessionFound::ByFolder("x".into()));
        assert_eq!(m, map(&[(1, "x")]));
    }

    #[test]
    fn folder_guess_recorded_when_free_and_replaces_own() {
        let mut m = map(&[(1, "x"), (2, "old")]);
        record_claude_session(&mut m, 2, ClaudeSessionFound::ByFolder("y".into()));
        assert_eq!(m, map(&[(1, "x"), (2, "y")]));
    }

    #[test]
    fn held_by_others_leaves_out_own() {
        let m = map(&[(1, "x"), (2, "y")]);
        assert_eq!(held_by_others(&m, 1), ["y".to_string()].into());
    }
}
