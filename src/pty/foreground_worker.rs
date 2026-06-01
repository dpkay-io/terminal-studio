use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;

use super::foreground::{detect_child, ForegroundProcess};

/// Background thread that polls foreground-process detection every 500 ms for
/// all registered sessions.  The UI thread reads from the shared cache without
/// ever calling the OS APIs directly, eliminating 10-50 ms UI stalls on
/// CreateToolhelp32Snapshot (Windows) or /proc scans (Linux).
pub struct ForegroundWorker {
    cache: Arc<Mutex<HashMap<u32, Option<ForegroundProcess>>>>,
    pids: Arc<Mutex<Vec<(u32, u32)>>>,
    alive: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl ForegroundWorker {
    pub fn spawn() -> Self {
        let cache: Arc<Mutex<HashMap<u32, Option<ForegroundProcess>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pids: Arc<Mutex<Vec<(u32, u32)>>> = Arc::new(Mutex::new(Vec::new()));
        let alive = Arc::new(AtomicBool::new(true));

        let cache_bg = cache.clone();
        let pids_bg = pids.clone();
        let alive_bg = alive.clone();

        let handle = match thread::Builder::new()
            .name("foreground-detector".into())
            .spawn(move || {
                while alive_bg.load(Ordering::Acquire) {
                    let snapshot: Vec<(u32, u32)> = pids_bg.lock().clone();
                    for (sid, shell_pid) in snapshot {
                        if shell_pid == u32::MAX {
                            continue;
                        }
                        let result = detect_child(shell_pid);
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

        ForegroundWorker { cache, pids, alive, thread: handle }
    }

    /// Update the set of sessions to poll.  Call whenever sessions are added or removed.
    /// Cheap: replaces the inner Vec and prunes stale cache entries.
    pub fn set_sessions(&self, sessions: Vec<(u32, u32)>) {
        let active_ids: std::collections::HashSet<u32> =
            sessions.iter().map(|(sid, _)| *sid).collect();
        *self.pids.lock() = sessions;
        self.cache.lock().retain(|sid, _| active_ids.contains(sid));
    }

    /// Read the cached result for `session_id`.  Never blocks on OS APIs.
    pub fn get(&self, session_id: u32) -> Option<ForegroundProcess> {
        self.cache.lock().get(&session_id)?.clone()
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
