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

        if let Err(e) = thread::Builder::new()
            .name("foreground-detector".into())
            .spawn(move || {
                while alive_bg.load(Ordering::Relaxed) {
                    let snapshot: Vec<(u32, u32)> = pids_bg.lock().clone();
                    for (sid, shell_pid) in snapshot {
                        let result = detect_child(shell_pid);
                        cache_bg.lock().insert(sid, result);
                    }
                    thread::sleep(Duration::from_millis(1000));
                }
            })
        {
            log::error!("failed to spawn foreground-detector thread: {e}");
        }

        ForegroundWorker { cache, pids, alive }
    }

    /// Update the set of sessions to poll.  Call whenever sessions are added or removed.
    /// Cheap: just replaces the inner Vec under a brief lock.
    pub fn set_sessions(&self, sessions: Vec<(u32, u32)>) {
        *self.pids.lock() = sessions;
    }

    /// Read the cached result for `session_id`.  Never blocks on OS APIs.
    pub fn get(&self, session_id: u32) -> Option<ForegroundProcess> {
        self.cache.lock().get(&session_id)?.clone()
    }
}

impl Drop for ForegroundWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
    }
}
