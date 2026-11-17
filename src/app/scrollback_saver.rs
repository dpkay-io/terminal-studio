//! Saving running sessions' scrollback in the background, and every write
//! of `session.json`.
//!
//! The UI thread only builds the session JSON. This worker captures a
//! session's output once it has been quiet for [`SETTLE`] (or changing for
//! [`MAX_STALE`]), reading the grid a [`SLICE`] at a time so no read lock
//! is held long, however fast the machine: `parking_lot`'s lock is fair, so a long read here
//! would make the PTY reader wait, and every UI read wait behind it. At quit
//! [`ScrollbackSaver::capture_all_now`] captures what is left, in parallel.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Weak};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use alacritty_terminal::Term;
use parking_lot::{Mutex, RwLock};

use super::scrollback_capture::{extract_grid_as_ansi, AnsiCapture};
use crate::terminal::{EventProxy, Session};

/// How often the worker looks for sessions to save.
pub(super) const SAVER_TICK: Duration = Duration::from_secs(1);
/// Output quiet this long: the session is saved.
pub(super) const SETTLE: Duration = Duration::from_secs(10);
/// Output changing for this long without a save: saved anyway, so a crash
/// under constant output loses at most this much.
pub(super) const MAX_STALE: Duration = Duration::from_secs(60);
/// How much of the grid one read lock covers: lines are captured
/// `step_lines` at a time until `budget` has passed, then the lock is let go.
#[derive(Clone, Copy)]
pub(super) struct Slice {
    pub(super) step_lines: usize,
    pub(super) budget: Duration,
}
/// Lines per step: about 0.05 ms at 200 columns on a fast machine, so a
/// slice overruns its budget by little even on a slow one.
const SLICE_STEP_LINES: usize = 64;
/// Longest a capture holds a session's read lock at a time.
const SLICE_BUDGET: Duration = Duration::from_millis(1);
pub(super) const SLICE: Slice = Slice {
    step_lines: SLICE_STEP_LINES,
    budget: SLICE_BUDGET,
};
/// Longest the app waits at quit for the last captures and writes.
pub(super) const FINAL_SAVE_TIMEOUT: Duration = Duration::from_secs(10);
/// zstd level for scrollback files.
const ZSTD_LEVEL: i32 = 3;

/// Name of a running session's scrollback file, written at `stamp` (ms).
/// Names from earlier saves (and the bare `<index>.zst` of older versions)
/// never repeat, so a restored session's file is never overwritten.
pub(super) fn scrollback_file_name(stamp: u64, session_id: u32) -> String {
    format!("{stamp}-{session_id}.zst")
}

/// Remove every file in the scrollback folder except those in `keep`.
pub(super) fn prune_scrollback_dir(dir: &Path, keep: &HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let kept = entry
            .file_name()
            .to_str()
            .is_some_and(|name| keep.contains(name));
        if !kept {
            std::fs::remove_file(entry.path()).ok();
        }
    }
}

/// When one session's scrollback is due to be saved.
struct SaveClock {
    /// Generation last saved; None before the first save.
    saved: Option<u64>,
    /// Generation last looked at.
    seen: u64,
    /// When `seen` last changed.
    changed_at: Instant,
    /// First change not saved yet.
    dirty_since: Option<Instant>,
}

impl SaveClock {
    fn new(generation: u64, now: Instant) -> Self {
        SaveClock {
            saved: None,
            seen: generation,
            changed_at: now,
            dirty_since: Some(now),
        }
    }

    fn observe(&mut self, generation: u64, now: Instant) {
        if generation != self.seen {
            self.seen = generation;
            self.changed_at = now;
            self.dirty_since.get_or_insert(now);
        }
    }

    fn needs_save(&self) -> bool {
        self.saved != Some(self.seen)
    }

    fn due(&self, now: Instant) -> bool {
        let settled = now.saturating_duration_since(self.changed_at) >= SETTLE;
        let stale = self
            .dirty_since
            .is_some_and(|since| now.saturating_duration_since(since) >= MAX_STALE);
        self.needs_save() && (settled || stale)
    }

    /// A capture of `generation` was saved (it may be newer than `seen`).
    fn saved_at(&mut self, generation: u64) {
        self.saved = Some(generation);
        if generation >= self.seen {
            self.seen = generation;
            self.dirty_since = None;
        }
    }
}

/// Capture a session's grid a slice at a time, letting go of the lock
/// between slices (`between_slices` runs then). Output waits on the
/// session's output `gate` meanwhile, so only a change from the UI (a
/// resize) can move the lines: then the capture would be torn and None is
/// returned.
fn capture_sliced(
    session: &RwLock<Session>,
    generation: &AtomicU64,
    gate: &Mutex<()>,
    slice: Slice,
    mut between_slices: impl FnMut(),
) -> Option<(u64, Vec<u8>)> {
    let _gate = gate.lock();
    let guard = session.read();
    let start = generation.load(Ordering::Acquire);
    let mut capture = AnsiCapture::begin(&guard.term, None);
    let mut done = capture_slice(&mut capture, &guard.term, slice);
    drop(guard);
    while !done {
        between_slices();
        let guard = session.read();
        if generation.load(Ordering::Acquire) != start {
            return None;
        }
        done = capture_slice(&mut capture, &guard.term, slice);
    }
    Some((start, capture.finish()))
}

/// One slice under a read lock the caller holds. True once every line is in.
fn capture_slice(capture: &mut AnsiCapture, term: &Term<EventProxy>, slice: Slice) -> bool {
    let started = Instant::now();
    loop {
        if capture.step(term, slice.step_lines.max(1)) {
            return true;
        }
        if started.elapsed() >= slice.budget {
            return false;
        }
    }
}

/// One read of the whole grid, for the final save.
fn capture_whole(id: u32, session: &RwLock<Session>) -> (u32, u64, Vec<u8>) {
    let guard = session.read();
    let seen = guard.content_generation().load(Ordering::Acquire);
    (id, seen, extract_grid_as_ansi(&guard.term, None))
}

/// State the UI thread reads.
#[derive(Default)]
struct Shared {
    /// Each tracked session's newest scrollback file.
    latest: Mutex<HashMap<u32, String>>,
    enabled: AtomicBool,
    /// A new file was written: `session.json` should point at it.
    session_save_wanted: AtomicBool,
}

enum Msg {
    Track {
        id: u32,
        session: Weak<RwLock<Session>>,
        generation: Arc<AtomicU64>,
        gate: Arc<Mutex<()>>,
    },
    Untrack(u32),
    Adopt(u32),
    SetEnabled(bool),
    WriteSession {
        path: PathBuf,
        text: String,
        referenced: HashSet<String>,
    },
    CaptureAllNow(mpsc::Sender<()>),
    Flush(mpsc::Sender<()>),
    Shutdown,
}

struct Tracked {
    /// Weak, so a closed session is never kept alive here.
    session: Weak<RwLock<Session>>,
    generation: Arc<AtomicU64>,
    gate: Arc<Mutex<()>>,
    clock: SaveClock,
}

/// The worker thread's state; methods run on that thread (tests call them
/// directly with their own clock).
struct Worker {
    dir: Option<PathBuf>,
    shared: Arc<Shared>,
    ctx: egui::Context,
    tracked: HashMap<u32, Tracked>,
}

impl Worker {
    fn new(dir: Option<PathBuf>, shared: Arc<Shared>, ctx: egui::Context) -> Self {
        Worker {
            dir,
            shared,
            ctx,
            tracked: HashMap::new(),
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Msg>) {
        let mut next_pass = Instant::now() + SAVER_TICK;
        loop {
            let wait = next_pass.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok(Msg::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(msg) => self.handle(msg),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let now = Instant::now();
            if now >= next_pass {
                self.pass(now);
                next_pass = now + SAVER_TICK;
            }
        }
    }

    fn handle(&mut self, msg: Msg) {
        match msg {
            Msg::Track {
                id,
                session,
                generation,
                gate,
            } => self.track(id, session, generation, gate, Instant::now()),
            Msg::Untrack(id) => self.forget(id),
            Msg::Adopt(id) => self.adopt(id),
            Msg::SetEnabled(on) => self.set_enabled(on),
            Msg::WriteSession {
                path,
                text,
                referenced,
            } => self.write_session(&path, &text, &referenced),
            Msg::CaptureAllNow(reply) => {
                self.capture_all_now();
                reply.send(()).ok();
            }
            Msg::Flush(reply) => {
                reply.send(()).ok();
            }
            Msg::Shutdown => {}
        }
    }

    fn track(
        &mut self,
        id: u32,
        session: Weak<RwLock<Session>>,
        generation: Arc<AtomicU64>,
        gate: Arc<Mutex<()>>,
        now: Instant,
    ) {
        let clock = SaveClock::new(generation.load(Ordering::Acquire), now);
        self.tracked.insert(
            id,
            Tracked {
                session,
                generation,
                gate,
                clock,
            },
        );
    }

    /// The session's content is what its `latest` file holds: not due
    /// until it changes.
    fn adopt(&mut self, id: u32) {
        if let Some(tracked) = self.tracked.get_mut(&id) {
            let generation = tracked.generation.load(Ordering::Acquire);
            tracked.clock.saved_at(generation);
        }
    }

    fn forget(&mut self, id: u32) {
        self.tracked.remove(&id);
        self.shared.latest.lock().remove(&id);
    }

    fn set_enabled(&mut self, on: bool) {
        self.shared.enabled.store(on, Ordering::Release);
        if !on {
            // Files written so far go with the next session write's prune;
            // turning saving back on captures every session again.
            self.shared.latest.lock().clear();
            for tracked in self.tracked.values_mut() {
                tracked.clock.saved = None;
            }
        }
    }

    fn enabled(&self) -> bool {
        self.shared.enabled.load(Ordering::Acquire)
    }

    /// Forget sessions that are gone and note every session's generation.
    fn observe_all(&mut self, now: Instant) {
        let gone: Vec<u32> = self
            .tracked
            .iter()
            .filter(|(_, t)| t.session.strong_count() == 0)
            .map(|(&id, _)| id)
            .collect();
        for id in gone {
            self.forget(id);
        }
        for tracked in self.tracked.values_mut() {
            tracked
                .clock
                .observe(tracked.generation.load(Ordering::Acquire), now);
        }
    }

    /// Save every session that is due, a slice at a time.
    fn pass(&mut self, now: Instant) {
        self.observe_all(now);
        if !self.enabled() || self.dir.is_none() {
            return;
        }
        let due: Vec<u32> = self
            .tracked
            .iter()
            .filter(|(_, t)| t.clock.due(now))
            .map(|(&id, _)| id)
            .collect();
        let stamp = crate::util::now_millis();
        let mut wrote = false;
        for id in due {
            let Some(tracked) = self.tracked.get(&id) else {
                continue;
            };
            let Some(session) = tracked.session.upgrade() else {
                continue;
            };
            let generation = Arc::clone(&tracked.generation);
            let gate = Arc::clone(&tracked.gate);
            // Resized mid-capture: tried again on a later pass.
            if let Some((seen, bytes)) =
                capture_sliced(&session, &generation, &gate, SLICE, std::thread::yield_now)
            {
                drop(session);
                wrote |= self.store(id, seen, &bytes, stamp);
            }
        }
        if wrote {
            self.request_session_save();
        }
    }

    /// Capture every session changed since its last save, now and in
    /// parallel: the app is quitting, so holding read locks is fine.
    fn capture_all_now(&mut self) {
        self.observe_all(Instant::now());
        if !self.enabled() || self.dir.is_none() {
            return;
        }
        let pending: Vec<(u32, Arc<RwLock<Session>>)> = self
            .tracked
            .iter()
            .filter(|(_, t)| t.clock.needs_save())
            .filter_map(|(&id, t)| Some((id, t.session.upgrade()?)))
            .collect();
        let workers = std::thread::available_parallelism().map_or(1, |n| n.get());
        let stamp = crate::util::now_millis();
        let mut wrote = false;
        for batch in pending.chunks(workers) {
            let captured: Vec<(u32, u64, Vec<u8>)> = std::thread::scope(|scope| {
                let mut inline = Vec::new();
                let handles: Vec<_> = batch
                    .iter()
                    .filter_map(|(id, session)| {
                        let spawned = std::thread::Builder::new()
                            .name("scrollback-final".into())
                            .spawn_scoped(scope, move || capture_whole(*id, session));
                        // No thread to spare: capture here instead.
                        spawned
                            .map_err(|_| inline.push(capture_whole(*id, session)))
                            .ok()
                    })
                    .collect();
                inline.extend(handles.into_iter().filter_map(|h| h.join().ok()));
                inline
            });
            for (id, seen, bytes) in captured {
                wrote |= self.store(id, seen, &bytes, stamp);
            }
        }
        if wrote {
            self.request_session_save();
        }
    }

    /// Write one capture and make it the session's latest file. True when
    /// `latest` changed. A failed write is logged and not retried until the
    /// output changes again.
    fn store(&mut self, id: u32, seen: u64, bytes: &[u8], stamp: u64) -> bool {
        let Some(tracked) = self.tracked.get_mut(&id) else {
            return false;
        };
        tracked.clock.saved_at(seen);
        if bytes.is_empty() {
            return self.shared.latest.lock().remove(&id).is_some();
        }
        let Some(dir) = self.dir.as_deref() else {
            return false;
        };
        if let Err(e) = std::fs::create_dir_all(dir) {
            log::warn!("scrollback folder {}: {e}", dir.display());
            return false;
        }
        let name = scrollback_file_name(stamp, id);
        let written = zstd::encode_all(bytes, ZSTD_LEVEL)
            .and_then(|compressed| std::fs::write(dir.join(&name), compressed));
        match written {
            Ok(()) => {
                self.shared.latest.lock().insert(id, name);
                true
            }
            Err(e) => {
                log::warn!("failed to save scrollback for session {id}: {e}");
                false
            }
        }
    }

    fn request_session_save(&self) {
        self.shared
            .session_save_wanted
            .store(true, Ordering::Release);
        self.ctx.request_repaint();
    }

    /// Write `session.json`, then remove every scrollback file it doesn't
    /// reference and that isn't a session's latest (written after the JSON
    /// was built).
    fn write_session(&self, path: &Path, text: &str, referenced: &HashSet<String>) {
        if let Err(e) = crate::util::atomic_write(path, text) {
            log::error!("failed to save session state: {e}");
            return;
        }
        let Some(dir) = self.dir.as_deref() else {
            return;
        };
        let mut keep = referenced.clone();
        keep.extend(self.shared.latest.lock().values().cloned());
        prune_scrollback_dir(dir, &keep);
    }
}

/// Handle to the saver thread, owned by `WorkerManager`.
pub(super) struct ScrollbackSaver {
    tx: mpsc::Sender<Msg>,
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

/// Held by a session's entry: dropping it (the session leaving the app in
/// any way) stops the saver tracking that session.
pub(super) struct SaverTracking {
    id: u32,
    tx: mpsc::Sender<Msg>,
}

impl Drop for SaverTracking {
    fn drop(&mut self) {
        self.tx.send(Msg::Untrack(self.id)).ok();
    }
}

impl ScrollbackSaver {
    /// Start the thread. `dir` is the scrollback folder (None: nothing is
    /// captured, `session.json` is still written). Saving starts off until
    /// [`Self::set_enabled`].
    pub(super) fn spawn(dir: Option<PathBuf>, ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared::default());
        let (tx, rx) = mpsc::channel();
        let worker = Worker::new(dir, Arc::clone(&shared), ctx);
        let handle = std::thread::Builder::new()
            .name("scrollback-saver".into())
            .spawn(move || worker.run(rx))
            .map_err(|e| log::error!("failed to start the scrollback saver: {e}"))
            .ok();
        ScrollbackSaver { tx, shared, handle }
    }

    pub(super) fn track(&self, id: u32, session: &Arc<RwLock<Session>>) -> SaverTracking {
        let (generation, gate) = {
            let s = session.read();
            (s.content_generation(), s.output_gate())
        };
        self.send(Msg::Track {
            id,
            session: Arc::downgrade(session),
            generation,
            gate,
        });
        SaverTracking {
            id,
            tx: self.tx.clone(),
        }
    }

    /// A restored session started with the output kept in `file`: that
    /// file stays its latest (so it is never pruned) until its output
    /// changes and is captured again. Call it right after [`Self::track`].
    pub(super) fn adopt(&self, id: u32, file: String) {
        if !self.shared.enabled.load(Ordering::Acquire) {
            return;
        }
        // Set here so a session write right after already names it.
        self.shared.latest.lock().insert(id, file);
        self.send(Msg::Adopt(id));
    }

    pub(super) fn set_enabled(&self, on: bool) {
        // Seen at once by the UI; the worker clears its state in order.
        self.shared.enabled.store(on, Ordering::Release);
        self.send(Msg::SetEnabled(on));
    }

    /// Each tracked session's newest scrollback file.
    pub(super) fn latest_files(&self) -> HashMap<u32, String> {
        self.shared.latest.lock().clone()
    }

    /// True once after the saver wrote a file `session.json` doesn't name.
    pub(super) fn take_session_save_request(&self) -> bool {
        self.shared
            .session_save_wanted
            .swap(false, Ordering::AcqRel)
    }

    /// Queue a write of `session.json`; `referenced` are the scrollback
    /// files it names.
    pub(super) fn write_session(&self, path: PathBuf, text: String, referenced: HashSet<String>) {
        self.send(Msg::WriteSession {
            path,
            text,
            referenced,
        });
    }

    /// Capture every changed session now; waits up to `timeout`.
    pub(super) fn capture_all_now(&self, timeout: Duration) {
        self.round_trip(Msg::CaptureAllNow, timeout);
    }

    /// Wait up to `timeout` for every queued write.
    pub(super) fn flush(&self, timeout: Duration) {
        self.round_trip(Msg::Flush, timeout);
    }

    fn round_trip(&self, msg: impl FnOnce(mpsc::Sender<()>) -> Msg, timeout: Duration) {
        let (reply_tx, reply_rx) = mpsc::channel();
        if self.tx.send(msg(reply_tx)).is_ok() && reply_rx.recv_timeout(timeout).is_err() {
            log::warn!("scrollback saver did not finish within {timeout:?}");
        }
    }

    fn send(&self, msg: Msg) {
        if self.tx.send(msg).is_err() {
            log::warn!("scrollback saver is not running");
        }
    }
}

impl Drop for ScrollbackSaver {
    fn drop(&mut self) {
        self.tx.send(Msg::Shutdown).ok();
        if let Some(handle) = self.handle.take() {
            handle.join().ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ts_saver_{tag}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn session_with(text: &str) -> Arc<RwLock<Session>> {
        let mut session = Session::new_for_test(1, 40, 10);
        let mut processor: Processor<StdSyncHandler> = Processor::new();
        processor.advance(&mut session.term, text.as_bytes());
        session.mark_content_changed();
        Arc::new(RwLock::new(session))
    }

    fn many_lines(count: usize) -> String {
        (0..count).map(|i| format!("line {i}\r\n")).collect()
    }

    fn worker(dir: &Path) -> Worker {
        let shared = Arc::new(Shared::default());
        shared.enabled.store(true, Ordering::Relaxed);
        Worker::new(Some(dir.to_path_buf()), shared, egui::Context::default())
    }

    fn track(worker: &mut Worker, id: u32, session: &Arc<RwLock<Session>>, now: Instant) {
        let (generation, gate) = {
            let s = session.read();
            (s.content_generation(), s.output_gate())
        };
        worker.track(id, Arc::downgrade(session), generation, gate, now);
    }

    // ── SaveClock ─────────────────────────────────────────────────────────

    #[test]
    fn due_after_settle_not_before() {
        let t0 = Instant::now();
        let mut clock = SaveClock::new(1, t0);
        clock.observe(2, t0);
        assert!(!clock.due(t0 + SETTLE - Duration::from_millis(1)));
        assert!(clock.due(t0 + SETTLE));
    }

    #[test]
    fn due_after_max_stale_under_constant_output() {
        let t0 = Instant::now();
        let mut clock = SaveClock::new(0, t0);
        clock.saved_at(0);
        let step = SETTLE / 2;
        let mut now = t0;
        let mut generation = 0;
        while now < t0 + MAX_STALE {
            now += step;
            generation += 1;
            clock.observe(generation, now);
            let stale = now.duration_since(t0 + step) >= MAX_STALE;
            assert_eq!(clock.due(now), stale, "at {:?}", now - t0);
        }
        clock.observe(generation + 1, t0 + step + MAX_STALE);
        assert!(clock.due(t0 + step + MAX_STALE));
    }

    #[test]
    fn not_due_once_saved() {
        let t0 = Instant::now();
        let mut clock = SaveClock::new(3, t0);
        clock.saved_at(3);
        assert!(!clock.due(t0 + MAX_STALE * 2));
        // A capture that saw newer content than the last look counts too.
        clock.saved_at(5);
        clock.observe(5, t0 + MAX_STALE * 2);
        assert!(!clock.due(t0 + MAX_STALE * 3));
    }

    // ── Sliced capture ────────────────────────────────────────────────────

    /// One 7-line step per read lock, however fast the machine.
    const ONE_STEP_PER_LOCK: Slice = Slice {
        step_lines: 7,
        budget: Duration::ZERO,
    };

    #[test]
    fn time_budget_decides_how_many_steps_share_a_lock() {
        let session = session_with(&many_lines(500));
        let generation = session.read().content_generation();
        let gate = session.read().output_gate();
        let whole = extract_grid_as_ansi(&session.read().term, None);
        let mut releases = 0;
        let (_, bytes) = capture_sliced(&session, &generation, &gate, ONE_STEP_PER_LOCK, || {
            releases += 1
        })
        .unwrap();
        assert_eq!(bytes, whole);
        assert!(
            releases > 500 / 7 - 2,
            "a zero budget lets go after each step ({releases})"
        );

        let mut releases = 0;
        let unbounded = Slice {
            step_lines: 7,
            budget: Duration::MAX,
        };
        let (_, bytes) =
            capture_sliced(&session, &generation, &gate, unbounded, || releases += 1).unwrap();
        assert_eq!(bytes, whole);
        assert_eq!(releases, 0, "an endless budget reads it all under one lock");
    }

    #[test]
    fn sliced_capture_matches_one_read() {
        let session = session_with(&many_lines(500));
        let generation = session.read().content_generation();
        let whole = extract_grid_as_ansi(&session.read().term, None);
        let gate = session.read().output_gate();
        let (seen, bytes) =
            capture_sliced(&session, &generation, &gate, ONE_STEP_PER_LOCK, || {}).unwrap();
        assert_eq!(bytes, whole);
        assert_eq!(seen, generation.load(Ordering::Acquire));
    }

    #[test]
    fn sliced_capture_aborts_when_content_changes() {
        let session = session_with(&many_lines(500));
        let generation = session.read().content_generation();
        let gate = session.read().output_gate();
        // A resize from the UI: it doesn't wait on the output gate.
        let result = capture_sliced(&session, &generation, &gate, ONE_STEP_PER_LOCK, || {
            session.write().mark_content_changed();
        });
        assert!(result.is_none());
    }

    #[test]
    fn sliced_capture_completes_under_constant_output() {
        let session = session_with(&many_lines(5_000));
        let generation = session.read().content_generation();
        let gate = session.read().output_gate();
        let stop = Arc::new(AtomicBool::new(false));
        let output = {
            let (session, gate, stop) =
                (Arc::clone(&session), Arc::clone(&gate), Arc::clone(&stop));
            std::thread::spawn(move || {
                let mut processor: Processor<StdSyncHandler> = Processor::new();
                while !stop.load(Ordering::Relaxed) {
                    // As the PTY reader feeds: under the output gate.
                    let _gate = gate.lock();
                    let mut s = session.write();
                    processor.advance(&mut s.term, b"spinner\r\n");
                    s.mark_content_changed();
                }
            })
        };
        let result = capture_sliced(
            &session,
            &generation,
            &gate,
            ONE_STEP_PER_LOCK,
            std::thread::yield_now,
        );
        stop.store(true, Ordering::Relaxed);
        output.join().unwrap();
        assert!(result.is_some());
    }

    // ── Worker passes ─────────────────────────────────────────────────────

    #[test]
    fn pass_writes_changed_session_and_requests_session_save() {
        let dir = temp_dir("pass");
        let mut w = worker(&dir);
        let session = session_with("hello saver\r\n");
        let t0 = Instant::now();
        track(&mut w, 7, &session, t0);

        w.pass(t0 + Duration::from_secs(1));
        assert!(files_in(&dir).is_empty(), "not settled yet");

        w.pass(t0 + SETTLE);
        let latest = w.shared.latest.lock().clone();
        let name = latest.get(&7).expect("saved").clone();
        assert!(name.ends_with("-7.zst"));
        assert_eq!(files_in(&dir), std::slice::from_ref(&name));
        let text = zstd::decode_all(std::fs::read(dir.join(&name)).unwrap().as_slice()).unwrap();
        assert!(String::from_utf8_lossy(&text).contains("hello saver"));
        assert!(w.shared.session_save_wanted.swap(false, Ordering::AcqRel));

        // Nothing changed since: the next pass writes nothing.
        w.pass(t0 + SETTLE * 3);
        assert_eq!(files_in(&dir), [name]);
        assert!(!w.shared.session_save_wanted.load(Ordering::Acquire));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopted_file_is_latest_and_not_captured_again() {
        let dir = temp_dir("adopt");
        let saver = ScrollbackSaver::spawn(Some(dir.clone()), egui::Context::default());
        saver.set_enabled(true);
        let session = session_with("restored output\r\n");
        let _guard = saver.track(3, &session);
        saver.adopt(3, "5-0.zst".into());
        assert_eq!(
            saver.latest_files().get(&3).map(String::as_str),
            Some("5-0.zst")
        );
        saver.capture_all_now(FINAL_SAVE_TIMEOUT);
        assert!(
            files_in(&dir).is_empty(),
            "the restored file already holds it"
        );
        assert_eq!(
            saver.latest_files().get(&3).map(String::as_str),
            Some("5-0.zst")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopt_does_nothing_with_saving_off() {
        let saver = ScrollbackSaver::spawn(None, egui::Context::default());
        let session = session_with("x\r\n");
        let _guard = saver.track(3, &session);
        saver.adopt(3, "5-0.zst".into());
        assert!(saver.latest_files().is_empty());
    }

    #[test]
    fn empty_session_writes_no_file() {
        let dir = temp_dir("empty");
        let mut w = worker(&dir);
        let session = Arc::new(RwLock::new(Session::new_for_test(1, 40, 10)));
        let t0 = Instant::now();
        track(&mut w, 1, &session, t0);
        w.pass(t0 + SETTLE);
        assert!(files_in(&dir).is_empty());
        assert!(w.shared.latest.lock().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dropped_session_is_forgotten() {
        let dir = temp_dir("dropped");
        let mut w = worker(&dir);
        let session = session_with("soon gone\r\n");
        let t0 = Instant::now();
        track(&mut w, 2, &session, t0);
        w.capture_all_now();
        assert!(w.shared.latest.lock().contains_key(&2));
        drop(session);
        w.pass(t0 + MAX_STALE);
        assert!(w.tracked.is_empty());
        assert!(!w.shared.latest.lock().contains_key(&2));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_all_now_writes_only_changed_sessions() {
        let dir = temp_dir("final");
        let mut w = worker(&dir);
        let a = session_with("session a\r\n");
        let b = session_with("session b\r\n");
        let t0 = Instant::now();
        track(&mut w, 1, &a, t0);
        track(&mut w, 2, &b, t0);
        w.pass(t0 + SETTLE);
        assert_eq!(files_in(&dir).len(), 2);
        let a_before = w.shared.latest.lock()[&1].clone();

        {
            let mut s = b.write();
            let mut processor: Processor<StdSyncHandler> = Processor::new();
            processor.advance(&mut s.term, b"more for b\r\n");
            s.mark_content_changed();
        }
        // New files are named by the millisecond they were written.
        std::thread::sleep(Duration::from_millis(2));
        w.capture_all_now();
        assert_eq!(w.shared.latest.lock()[&1], a_before);
        assert_eq!(files_in(&dir).len(), 3, "only b written again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn disabled_saver_captures_nothing() {
        let dir = temp_dir("disabled");
        let mut w = worker(&dir);
        let session = session_with("kept to itself\r\n");
        let t0 = Instant::now();
        track(&mut w, 1, &session, t0);
        w.capture_all_now();
        assert_eq!(files_in(&dir).len(), 1);

        w.set_enabled(false);
        assert!(w.shared.latest.lock().is_empty());
        w.pass(t0 + MAX_STALE);
        w.capture_all_now();
        assert_eq!(files_in(&dir).len(), 1, "nothing new written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_session_prunes_everything_not_referenced_or_latest() {
        let dir = temp_dir("prune");
        let session_file = temp_dir("prune_json").join("session.json");
        let w = worker(&dir);
        for name in ["referenced.zst", "latest.zst", "stale.zst", "1.zst"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        w.shared.latest.lock().insert(9, "latest.zst".into());
        w.write_session(&session_file, "{}", &["referenced.zst".to_string()].into());
        assert_eq!(std::fs::read_to_string(&session_file).unwrap(), "{}");
        assert_eq!(files_in(&dir), ["latest.zst", "referenced.zst"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_scrollback_names_never_reuse_an_earlier_saves_names() {
        // Older versions wrote `<index>.zst`; every save now writes under its
        // own stamp, so neither an old name nor an earlier save's is reused.
        assert_ne!(scrollback_file_name(5, 1), "1.zst");
        assert_ne!(scrollback_file_name(5, 1), scrollback_file_name(6, 1));
        assert_ne!(scrollback_file_name(5, 1), scrollback_file_name(5, 2));
    }

    // ── Through the thread ────────────────────────────────────────────────

    #[test]
    fn flush_waits_for_a_queued_session_write() {
        let dir = temp_dir("flush");
        let session_file = temp_dir("flush_json").join("session.json");
        let saver = ScrollbackSaver::spawn(Some(dir.clone()), egui::Context::default());
        saver.write_session(session_file.clone(), "{\"a\":1}".into(), HashSet::new());
        saver.flush(FINAL_SAVE_TIMEOUT);
        assert_eq!(std::fs::read_to_string(&session_file).unwrap(), "{\"a\":1}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracking_guard_untracks_on_drop() {
        let dir = temp_dir("guard");
        let saver = ScrollbackSaver::spawn(Some(dir.clone()), egui::Context::default());
        saver.set_enabled(true);
        let session = session_with("tracked\r\n");
        let guard = saver.track(4, &session);
        saver.capture_all_now(FINAL_SAVE_TIMEOUT);
        assert!(saver.latest_files().contains_key(&4));
        drop(guard);
        saver.flush(FINAL_SAVE_TIMEOUT);
        assert!(!saver.latest_files().contains_key(&4));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
