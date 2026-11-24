//! The note worker: the one thread that touches note files.
//!
//! It keeps, per note, the file's text and stamp as last read or written,
//! and a version that changes whenever that text does. The UI saves with
//! the version its text was built on; when the file changed since (another
//! app wrote it, or the UI has not seen a change yet), the save is merged
//! into the file's text instead of replacing it, so no side's text is lost.
//! Every `POLL_INTERVAL` it looks for files changed by other apps and sends
//! their text.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::disk::{self, DiskStamp};
use super::merge::merge_note_texts;
use super::target::{backup_path, NoteKey, NoteTarget};

/// How often files are checked for edits made by other apps.
pub const POLL_INTERVAL: Duration = Duration::from_secs(1);
const THREAD_NAME: &str = "note-sync";

/// Asked of the worker, handled in order.
pub enum Command {
    /// The notes to keep in sync and their files. A note whose own file
    /// moved (its workspace was renamed) has the file moved with it; notes
    /// left out stop being checked.
    Watch(Vec<(NoteKey, NoteTarget)>),
    /// Write `text`, built on the file's text at `base`.
    Save {
        key: NoteKey,
        target: NoteTarget,
        text: String,
        base: u64,
    },
    /// Move the note from `from` to `to`, keeping the text of both files
    /// and `text` (the UI's, which may hold edits not saved yet).
    Relink {
        key: NoteKey,
        from: NoteTarget,
        to: NoteTarget,
        text: String,
    },
    /// Write the links file.
    WriteLinks {
        path: PathBuf,
        json: String,
    },
    /// Answer once every command before it is done.
    Flush(mpsc::Sender<()>),
    Shutdown,
}

/// Sent to the UI.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// The file's text is at `version`. `text` is set when it differs from
    /// what the UI last sent: the UI takes it (merging edits it made since).
    Disk {
        key: NoteKey,
        version: u64,
        text: Option<String>,
    },
    Status {
        key: NoteKey,
        status: SyncStatus,
    },
    /// The note could not move to the file asked for; it stays in
    /// `previous`.
    RelinkFailed {
        key: NoteKey,
        previous: NoteTarget,
        reason: String,
    },
}

/// How a note's file is doing, as shown in the notes header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SyncStatus {
    #[default]
    Synced,
    /// The linked file is gone (moved or deleted elsewhere); the text is
    /// kept and the next edit writes the file again.
    Missing,
    /// The file could not be read or written; the reason, as a phrase.
    Failed(String),
}

/// What the worker knows of one note's file.
struct Tracked {
    target: NoteTarget,
    /// Stamp when last read or written; `None` while there is no file.
    stamp: Option<DiskStamp>,
    /// The file's text as last read or written.
    text: String,
    version: u64,
    /// Why the file at `stamp` could not be read. Nothing is written over
    /// a file the worker could not read.
    unreadable: Option<String>,
    /// Why the last write failed, until one succeeds.
    write_error: Option<String>,
    /// The status the UI was last told.
    reported: SyncStatus,
}

impl Tracked {
    fn status(&self) -> SyncStatus {
        if let Some(reason) = self.unreadable.as_ref().or(self.write_error.as_ref()) {
            return SyncStatus::Failed(reason.clone());
        }
        if self.target.linked && self.stamp.is_none() {
            return SyncStatus::Missing;
        }
        SyncStatus::Synced
    }

    /// Re-reads the file if it changed since it was last seen. `Ok(true)`
    /// when its text changed; a file that disappeared keeps its last text.
    fn refresh(&mut self) -> Result<bool, String> {
        let now = disk::stamp(&self.target.path);
        if now == self.stamp {
            return self.unreadable.clone().map_or(Ok(false), Err);
        }
        match disk::read_note(&self.target.path) {
            Ok(Some((text, stamp))) => {
                self.stamp = Some(stamp);
                self.unreadable = None;
                let changed = text != self.text;
                self.text = text;
                Ok(changed)
            }
            Ok(None) => {
                self.stamp = None;
                self.unreadable = None;
                Ok(false)
            }
            Err(e) => {
                self.stamp = now;
                let reason = e.to_string();
                self.unreadable = Some(reason.clone());
                Err(reason)
            }
        }
    }
}

/// The worker's state; driven by [`Worker::run`] on its thread, and
/// directly by tests.
pub struct Worker {
    tracked: HashMap<NoteKey, Tracked>,
    next_version: u64,
    events: mpsc::Sender<Event>,
    /// Woken after each event so the UI takes it without waiting for input.
    repaint: Option<egui::Context>,
}

impl Worker {
    pub fn new(events: mpsc::Sender<Event>, repaint: Option<egui::Context>) -> Self {
        Self {
            tracked: HashMap::new(),
            next_version: 0,
            events,
            repaint,
        }
    }

    fn run(mut self, commands: mpsc::Receiver<Command>) {
        let mut last_poll = Instant::now();
        loop {
            let wait = POLL_INTERVAL.saturating_sub(last_poll.elapsed());
            match commands.recv_timeout(wait) {
                Ok(command) => {
                    if !self.handle(command) {
                        return;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if last_poll.elapsed() >= POLL_INTERVAL {
                self.poll();
                last_poll = Instant::now();
            }
        }
    }

    /// Handles one command; `false` once asked to stop.
    pub fn handle(&mut self, command: Command) -> bool {
        match command {
            Command::Watch(notes) => self.watch(notes),
            Command::Save {
                key,
                target,
                text,
                base,
            } => self.save(key, target, text, base),
            Command::Relink {
                key,
                from,
                to,
                text,
            } => self.relink(key, from, to, text),
            Command::WriteLinks { path, json } => {
                if let Err(e) = crate::util::atomic_write(&path, &json) {
                    log::error!("could not save note links to {}: {e}", path.display());
                }
            }
            Command::Flush(done) => {
                done.send(()).ok();
            }
            Command::Shutdown => return false,
        }
        true
    }

    /// Sends the text of every file another app changed since last seen.
    pub fn poll(&mut self) {
        let keys: Vec<NoteKey> = self.tracked.keys().copied().collect();
        for key in keys {
            let Some(t) = self.tracked.get_mut(&key) else {
                continue;
            };
            if let Ok(true) = t.refresh() {
                t.version = bump(&mut self.next_version);
                let event = Event::Disk {
                    key,
                    version: t.version,
                    text: Some(t.text.clone()),
                };
                self.emit(event);
            }
            self.report_status(key);
        }
    }

    fn watch(&mut self, notes: Vec<(NoteKey, NoteTarget)>) {
        let wanted: HashMap<NoteKey, NoteTarget> = notes.into_iter().collect();
        self.tracked.retain(|key, _| wanted.contains_key(key));
        for (key, target) in wanted {
            match self.tracked.get(&key) {
                Some(t) if t.target == target => continue,
                // Only a relink moves a note to or from a linked file; a
                // watch naming another was sent before the relink's outcome
                // (a failed one keeps the note where it was) was known.
                Some(t) if t.target.linked || target.linked => continue,
                Some(t) => move_own_file(&t.target.path, &target.path),
                None => {}
            }
            let t = self.track(key, target);
            let event = Event::Disk {
                key,
                version: t.version,
                text: Some(t.text.clone()),
            };
            self.emit(event);
            self.report_status(key);
        }
    }

    fn save(&mut self, key: NoteKey, target: NoteTarget, text: String, base: u64) {
        if self.tracked.get(&key).is_none_or(|t| t.target != target) {
            self.track(key, target);
        }
        let Some(t) = self.tracked.get_mut(&key) else {
            return;
        };
        let changed = match t.refresh() {
            Ok(changed) => changed,
            Err(_) => {
                // Never written over: the file may hold what we cannot read.
                self.report_status(key);
                return;
            }
        };
        let up_to_date = !changed && base == t.version;
        let to_write = if up_to_date {
            text.clone()
        } else {
            merge_note_texts(&t.text, &text)
        };
        let needs_write = to_write != t.text || (t.stamp.is_none() && !to_write.is_empty());
        if needs_write {
            match disk::write_note(&t.target, &to_write) {
                Ok(stamp) => {
                    t.stamp = stamp;
                    t.text = to_write;
                    t.write_error = None;
                    t.version = bump(&mut self.next_version);
                }
                Err(e) => {
                    log::error!("could not save note to {}: {e}", t.target.path.display());
                    t.write_error = Some(e.to_string());
                    self.report_status(key);
                    return;
                }
            }
        } else if changed {
            t.version = bump(&mut self.next_version);
        }
        let reply = (t.text != text).then(|| t.text.clone());
        let event = Event::Disk {
            key,
            version: t.version,
            text: reply,
        };
        self.emit(event);
        self.report_status(key);
    }

    fn relink(&mut self, key: NoteKey, from: NoteTarget, to: NoteTarget, text: String) {
        if from == to {
            return;
        }
        // The old file's latest text: it may have changed since last polled.
        if self.tracked.get(&key).is_none_or(|t| t.target != from) {
            self.track(key, from.clone());
        }
        let old_text = self
            .tracked
            .get_mut(&key)
            .map(|t| {
                let _ = t.refresh();
                t.text.clone()
            })
            .unwrap_or_default();

        let fail = |worker: &mut Self, reason: String| {
            log::warn!("could not link note to {}: {reason}", to.path.display());
            worker.emit(Event::RelinkFailed {
                key,
                previous: from.clone(),
                reason,
            });
        };
        let (new_text, exists) = match disk::read_note(&to.path) {
            Ok(Some((t, _))) => (t, true),
            Ok(None) => (String::new(), false),
            Err(e) => return fail(self, e.to_string()),
        };
        // The new file's text first, then what only the note had.
        let merged = merge_note_texts(&merge_note_texts(&new_text, &old_text), &text);
        let needs_write = merged != new_text || (!exists && to.linked);
        let stamp = if needs_write {
            match disk::write_note(&to, &merged) {
                Ok(stamp) => stamp,
                Err(e) => return fail(self, e.to_string()),
            }
        } else {
            disk::stamp(&to.path)
        };

        if !from.linked && from.path != to.path {
            set_aside(&from.path);
        }
        let version = bump(&mut self.next_version);
        let reported = self.reported_status(key);
        self.tracked.insert(
            key,
            Tracked {
                target: to,
                stamp,
                text: merged.clone(),
                version,
                unreadable: None,
                write_error: None,
                reported,
            },
        );
        self.emit(Event::Disk {
            key,
            version,
            text: Some(merged),
        });
        self.report_status(key);
    }

    /// Starts tracking `key` in `target` (in place of what it was tracked
    /// in), reading its file.
    fn track(&mut self, key: NoteKey, target: NoteTarget) -> &Tracked {
        let mut t = Tracked {
            target,
            stamp: None,
            text: String::new(),
            version: bump(&mut self.next_version),
            unreadable: None,
            write_error: None,
            reported: self.reported_status(key),
        };
        match disk::read_note(&t.target.path) {
            Ok(Some((text, stamp))) => {
                t.text = text;
                t.stamp = Some(stamp);
            }
            Ok(None) => {}
            Err(e) => {
                t.stamp = disk::stamp(&t.target.path);
                t.unreadable = Some(e.to_string());
            }
        }
        self.tracked.entry(key).insert_entry(t).into_mut()
    }

    /// What the UI was last told of `key`, so a note tracked anew still
    /// reports a change from it.
    fn reported_status(&self, key: NoteKey) -> SyncStatus {
        self.tracked
            .get(&key)
            .map(|t| t.reported.clone())
            .unwrap_or_default()
    }

    fn report_status(&mut self, key: NoteKey) {
        let Some(t) = self.tracked.get_mut(&key) else {
            return;
        };
        let status = t.status();
        if status == t.reported {
            return;
        }
        t.reported = status.clone();
        self.emit(Event::Status { key, status });
    }

    fn emit(&self, event: Event) {
        if self.events.send(event).is_ok() {
            if let Some(ctx) = &self.repaint {
                ctx.request_repaint();
            }
        }
    }
}

fn bump(counter: &mut u64) -> u64 {
    *counter += 1;
    *counter
}

/// A renamed workspace's own note file follows it. When a file is already
/// at the new name, both texts are kept there and the old file set aside.
fn move_own_file(from: &Path, to: &Path) {
    let Ok(Some((old_text, _))) = disk::read_note(from) else {
        return;
    };
    let result = match disk::read_note(to) {
        Ok(None) => std::fs::rename(from, to).map_err(|e| e.to_string()),
        Ok(Some((existing, _))) => {
            let merged = merge_note_texts(&existing, &old_text);
            disk::write_note(&NoteTarget::own(to.to_path_buf()), &merged)
                .map(|_| set_aside(from))
                .map_err(|e| e.to_string())
        }
        Err(e) => Err(e.to_string()),
    };
    if let Err(e) = result {
        log::error!(
            "could not move note {} to {}: {e}",
            from.display(),
            to.display()
        );
    }
}

/// Keeps an own note file whose text now lives elsewhere as `<name>.bak`,
/// so the old text is still on disk but is not read as the note again.
fn set_aside(own_path: &Path) {
    if !own_path.exists() {
        return;
    }
    let backup = backup_path(own_path);
    if let Err(e) = std::fs::rename(own_path, &backup) {
        log::error!("could not set aside note {}: {e}", own_path.display());
    }
}

/// The UI's handle to the worker thread.
pub struct NoteWorker {
    commands: mpsc::Sender<Command>,
    events: mpsc::Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}

impl NoteWorker {
    pub fn spawn(repaint: Option<egui::Context>) -> Self {
        let (commands, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let worker = Worker::new(event_tx, repaint);
        let thread = std::thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || worker.run(command_rx))
            .map_err(|e| log::error!("could not start the note worker: {e}"))
            .ok();
        Self {
            commands,
            events,
            thread,
        }
    }

    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            log::warn!("note worker is not running");
        }
    }

    /// Events sent since last asked, oldest first. Cheap when there are none.
    pub fn events(&self) -> mpsc::TryIter<'_, Event> {
        self.events.try_iter()
    }

    /// Waits up to `timeout` for every command sent so far; `true` if they
    /// were all done.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (done, wait) = mpsc::channel();
        if self.commands.send(Command::Flush(done)).is_err() {
            return false;
        }
        let finished = wait.recv_timeout(timeout).is_ok();
        if !finished {
            log::warn!("note worker did not finish within {timeout:?}");
        }
        finished
    }

    /// Stops the thread after the commands sent so far. Waits for it only
    /// when `wait` (a thread stuck on a slow disk would hold up quitting).
    pub fn stop(&mut self, wait: bool) {
        self.commands.send(Command::Shutdown).ok();
        if let Some(thread) = self.thread.take() {
            if wait {
                thread.join().ok();
            }
        }
    }
}

impl Drop for NoteWorker {
    fn drop(&mut self) {
        self.stop(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Harness {
        worker: Worker,
        events: mpsc::Receiver<Event>,
        dir: tempfile::TempDir,
    }

    impl Harness {
        fn new() -> Self {
            let (tx, events) = mpsc::channel();
            Self {
                worker: Worker::new(tx, None),
                events,
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        fn own(&self, name: &str) -> NoteTarget {
            NoteTarget::own(self.path(name))
        }

        fn linked(&self, name: &str) -> NoteTarget {
            NoteTarget::linked(self.path(name))
        }

        fn write(&self, name: &str, text: &str) {
            std::fs::write(self.path(name), text).unwrap();
        }

        fn read(&self, name: &str) -> Option<String> {
            std::fs::read_to_string(self.path(name)).ok()
        }

        fn events(&self) -> Vec<Event> {
            self.events.try_iter().collect()
        }

        /// The last Disk event for `key`: (version, text).
        fn last_disk(&self, key: NoteKey) -> Option<(u64, Option<String>)> {
            self.events()
                .into_iter()
                .filter_map(|e| match e {
                    Event::Disk {
                        key: k,
                        version,
                        text,
                    } if k == key => Some((version, text)),
                    _ => None,
                })
                .next_back()
        }

        fn watch(&mut self, notes: Vec<(NoteKey, NoteTarget)>) {
            self.worker.handle(Command::Watch(notes));
        }

        fn save(&mut self, key: NoteKey, target: NoteTarget, text: &str, base: u64) {
            self.worker.handle(Command::Save {
                key,
                target,
                text: text.into(),
                base,
            });
        }
    }

    #[test]
    fn watching_sends_each_file_text() {
        let mut h = Harness::new();
        h.write("a.md", "hello");
        h.watch(vec![(Some(1), h.own("a.md")), (None, h.own("none.md"))]);
        let events = h.events();
        assert!(events.iter().any(|e| matches!(e,
            Event::Disk { key: Some(1), text: Some(t), .. } if t == "hello")));
        assert!(events.iter().any(|e| matches!(e,
            Event::Disk { key: None, text: Some(t), .. } if t.is_empty())));
    }

    #[test]
    fn a_save_on_the_latest_version_is_written_as_is() {
        let mut h = Harness::new();
        h.write("a.md", "old");
        h.watch(vec![(Some(1), h.own("a.md"))]);
        let (version, _) = h.last_disk(Some(1)).unwrap();
        h.save(Some(1), h.own("a.md"), "new text", version);
        assert_eq!(h.read("a.md").as_deref(), Some("new text"));
        let (next, text) = h.last_disk(Some(1)).unwrap();
        assert!(next > version);
        assert_eq!(text, None, "the UI already has the text it saved");
    }

    #[test]
    fn a_save_racing_an_edit_elsewhere_keeps_both() {
        let mut h = Harness::new();
        h.write("a.md", "first");
        h.watch(vec![(Some(1), h.linked("a.md"))]);
        let (version, _) = h.last_disk(Some(1)).unwrap();
        // Obsidian wrote the file; the UI has not seen it yet.
        h.write("a.md", "first\n\nfrom obsidian");
        h.save(Some(1), h.linked("a.md"), "first\n\ntyped here", version);
        let on_disk = h.read("a.md").unwrap();
        assert_eq!(on_disk, "first\n\nfrom obsidian\n\ntyped here\n");
        let (_, text) = h.last_disk(Some(1)).unwrap();
        assert_eq!(text.as_deref(), Some(on_disk.as_str()));
    }

    #[test]
    fn a_save_built_on_an_old_version_is_merged() {
        let mut h = Harness::new();
        h.write("a.md", "disk text");
        h.watch(vec![(Some(1), h.own("a.md"))]);
        // Typed before the file's text arrived (base 0).
        h.save(Some(1), h.own("a.md"), "typed early", 0);
        assert_eq!(h.read("a.md").unwrap(), "disk text\n\ntyped early\n");
    }

    #[test]
    fn polling_sends_edits_made_elsewhere_once() {
        let mut h = Harness::new();
        h.write("a.md", "v1");
        h.watch(vec![(Some(1), h.linked("a.md"))]);
        h.events();
        h.write("a.md", "v2 from outside");
        h.worker.poll();
        let (_, text) = h.last_disk(Some(1)).unwrap();
        assert_eq!(text.as_deref(), Some("v2 from outside"));
        h.worker.poll();
        assert!(h.events().is_empty(), "nothing new: nothing sent");
    }

    #[test]
    fn a_touched_file_with_the_same_text_sends_nothing() {
        let mut h = Harness::new();
        h.write("a.md", "same");
        h.watch(vec![(Some(1), h.own("a.md"))]);
        h.events();
        // Saved again elsewhere without a change: a new stamp, the same text.
        let file = std::fs::File::options()
            .write(true)
            .open(h.path("a.md"))
            .unwrap();
        file.set_modified(std::time::SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        drop(file);
        h.worker.poll();
        assert!(h.events().is_empty());
    }

    #[test]
    fn a_deleted_linked_file_is_missing_and_its_text_kept() {
        let mut h = Harness::new();
        h.write("a.md", "keep me");
        h.watch(vec![(Some(1), h.linked("a.md"))]);
        let (version, _) = h.last_disk(Some(1)).unwrap();
        std::fs::remove_file(h.path("a.md")).unwrap();
        h.worker.poll();
        let events = h.events();
        assert!(events.contains(&Event::Status {
            key: Some(1),
            status: SyncStatus::Missing
        }));
        assert!(!events.iter().any(|e| matches!(e, Event::Disk { .. })));
        // The next edit writes the file again.
        h.save(Some(1), h.linked("a.md"), "keep me\nmore", version);
        assert_eq!(h.read("a.md").as_deref(), Some("keep me\nmore"));
        assert!(h.events().contains(&Event::Status {
            key: Some(1),
            status: SyncStatus::Synced
        }));
    }

    #[test]
    fn a_deleted_own_file_keeps_its_text_and_is_written_again() {
        let mut h = Harness::new();
        h.write("a.md", "keep me");
        h.watch(vec![(Some(1), h.own("a.md"))]);
        let (version, _) = h.last_disk(Some(1)).unwrap();
        std::fs::remove_file(h.path("a.md")).unwrap();
        h.worker.poll();
        // Nothing to take in, nothing wrong to say: the note keeps its text.
        assert!(h.events().is_empty());
        h.save(Some(1), h.own("a.md"), "keep me", version);
        assert_eq!(h.read("a.md").as_deref(), Some("keep me"));
    }

    #[test]
    fn a_deleted_notes_folder_is_made_again_on_the_next_save() {
        let mut h = Harness::new();
        let folder = h.path("notes");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("a.md"), "note").unwrap();
        let target = NoteTarget::own(folder.join("a.md"));
        h.watch(vec![(None, target.clone())]);
        let (version, _) = h.last_disk(None).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
        h.worker.poll();
        h.save(None, target.clone(), "note again", version);
        assert_eq!(
            std::fs::read_to_string(&target.path).ok().as_deref(),
            Some("note again")
        );
    }

    #[test]
    fn a_file_replaced_by_a_folder_fails_until_the_folder_is_gone() {
        let mut h = Harness::new();
        h.write("a.md", "text");
        h.watch(vec![(Some(1), h.linked("a.md"))]);
        let (version, _) = h.last_disk(Some(1)).unwrap();
        std::fs::remove_file(h.path("a.md")).unwrap();
        std::fs::create_dir(h.path("a.md")).unwrap();
        h.worker.poll();
        assert!(h.events().contains(&Event::Status {
            key: Some(1),
            status: SyncStatus::Failed("it is not a file".into())
        }));
        // Never written into or over while the folder is there.
        h.save(Some(1), h.linked("a.md"), "text\nmore", version);
        assert!(h.path("a.md").is_dir());
        std::fs::remove_dir(h.path("a.md")).unwrap();
        h.worker.poll();
        assert!(h.events().contains(&Event::Status {
            key: Some(1),
            status: SyncStatus::Missing
        }));
        h.save(Some(1), h.linked("a.md"), "text\nmore", version);
        assert_eq!(h.read("a.md").as_deref(), Some("text\nmore"));
    }

    #[test]
    fn a_deleted_linked_file_can_still_be_unlinked() {
        let mut h = Harness::new();
        h.write("vault.md", "from the vault");
        h.watch(vec![(Some(1), h.linked("vault.md"))]);
        std::fs::remove_file(h.path("vault.md")).unwrap();
        h.worker.handle(Command::Relink {
            key: Some(1),
            from: h.linked("vault.md"),
            to: h.own("own.md"),
            text: "from the vault".into(),
        });
        assert_eq!(h.read("own.md").as_deref(), Some("from the vault"));
        assert!(!h
            .events()
            .iter()
            .any(|e| matches!(e, Event::RelinkFailed { .. })));
    }

    #[test]
    fn an_unreadable_file_is_never_written_over() {
        let mut h = Harness::new();
        std::fs::write(h.path("bin.dat"), [0xff, 0x00, 0xfe]).unwrap();
        h.watch(vec![(Some(1), h.linked("bin.dat"))]);
        let (version, _) = h.last_disk(Some(1)).unwrap_or((0, None));
        h.save(Some(1), h.linked("bin.dat"), "text", version);
        assert_eq!(
            std::fs::read(h.path("bin.dat")).unwrap(),
            vec![0xff, 0x00, 0xfe]
        );
    }

    #[test]
    fn unreadable_files_report_why() {
        let mut h = Harness::new();
        std::fs::write(h.path("bin.dat"), [0xff, 0x00, 0xfe]).unwrap();
        h.watch(vec![(Some(1), h.linked("bin.dat"))]);
        assert!(h.events().contains(&Event::Status {
            key: Some(1),
            status: SyncStatus::Failed("it is not a text file".into())
        }));
    }

    #[test]
    fn a_failed_write_reports_and_clears_once_one_succeeds() {
        let mut h = Harness::new();
        h.watch(vec![(Some(1), h.linked("dir/a.md"))]);
        let (version, _) = h.last_disk(Some(1)).unwrap();
        // A file where the folder should be: the write cannot happen.
        h.write("dir", "blocking");
        h.save(Some(1), h.linked("dir/a.md"), "text", version);
        assert!(h.events().iter().any(|e| matches!(
            e,
            Event::Status {
                key: Some(1),
                status: SyncStatus::Failed(_)
            }
        )));
        std::fs::remove_file(h.path("dir")).unwrap();
        h.save(Some(1), h.linked("dir/a.md"), "text", version);
        assert_eq!(h.read("dir/a.md").as_deref(), Some("text"));
        assert!(h.events().contains(&Event::Status {
            key: Some(1),
            status: SyncStatus::Synced
        }));
    }

    #[test]
    fn linking_merges_the_file_the_note_and_unsaved_edits() {
        let mut h = Harness::new();
        h.write("own.md", "own note\n\nshared");
        h.write("vault.md", "# Vault\n\nshared");
        h.watch(vec![(Some(1), h.own("own.md"))]);
        h.worker.handle(Command::Relink {
            key: Some(1),
            from: h.own("own.md"),
            to: h.linked("vault.md"),
            text: "own note\n\nshared\n\nnot saved yet".into(),
        });
        let merged = h.read("vault.md").unwrap();
        assert_eq!(merged, "# Vault\n\nshared\n\nown note\n\nnot saved yet\n");
        let (_, text) = h.last_disk(Some(1)).unwrap();
        assert_eq!(text.as_deref(), Some(merged.as_str()));
        // The own file is set aside, not deleted.
        assert!(h.read("own.md").is_none());
        assert_eq!(h.read("own.md.bak").as_deref(), Some("own note\n\nshared"));
    }

    #[test]
    fn linking_a_new_file_creates_it() {
        let mut h = Harness::new();
        h.watch(vec![(None, h.own("general.md"))]);
        h.worker.handle(Command::Relink {
            key: None,
            from: h.own("general.md"),
            to: h.linked("new.md"),
            text: String::new(),
        });
        assert_eq!(h.read("new.md").as_deref(), Some(""));
    }

    #[test]
    fn linking_an_identical_file_leaves_it_untouched() {
        let mut h = Harness::new();
        h.write("vault.md", "same");
        let before = disk::stamp(&h.path("vault.md"));
        h.worker.handle(Command::Relink {
            key: Some(1),
            from: h.own("own.md"),
            to: h.linked("vault.md"),
            text: "same".into(),
        });
        assert_eq!(disk::stamp(&h.path("vault.md")), before);
    }

    #[test]
    fn unlinking_brings_the_text_back_and_leaves_the_linked_file() {
        let mut h = Harness::new();
        h.write("vault.md", "from the vault");
        h.watch(vec![(Some(1), h.linked("vault.md"))]);
        h.worker.handle(Command::Relink {
            key: Some(1),
            from: h.linked("vault.md"),
            to: h.own("own.md"),
            text: "from the vault".into(),
        });
        assert_eq!(h.read("own.md").as_deref(), Some("from the vault"));
        assert_eq!(h.read("vault.md").as_deref(), Some("from the vault"));
    }

    #[test]
    fn a_link_that_cannot_be_made_keeps_the_old_file() {
        let mut h = Harness::new();
        h.write("own.md", "safe");
        std::fs::write(h.path("bin.dat"), [0xff, 0x00]).unwrap();
        h.watch(vec![(Some(1), h.own("own.md"))]);
        h.events();
        h.worker.handle(Command::Relink {
            key: Some(1),
            from: h.own("own.md"),
            to: h.linked("bin.dat"),
            text: "safe".into(),
        });
        let events = h.events();
        assert_eq!(
            events,
            vec![Event::RelinkFailed {
                key: Some(1),
                previous: h.own("own.md"),
                reason: "it is not a text file".into()
            }]
        );
        assert_eq!(h.read("own.md").as_deref(), Some("safe"));
    }

    #[test]
    fn a_watch_never_undoes_a_relink() {
        let mut h = Harness::new();
        h.write("own.md", "safe");
        std::fs::write(h.path("bin.dat"), [0xff, 0x00]).unwrap();
        h.watch(vec![(Some(1), h.own("own.md"))]);
        h.worker.handle(Command::Relink {
            key: Some(1),
            from: h.own("own.md"),
            to: h.linked("bin.dat"),
            text: "safe".into(),
        });
        h.events();
        // Sent by the UI before it heard the link failed.
        h.watch(vec![(Some(1), h.linked("bin.dat"))]);
        assert!(h.events().is_empty(), "the note stays in its own file");
        // Typed on a version it never saw: merged, and in the own file.
        h.save(Some(1), h.own("own.md"), "safe\nmore", 0);
        assert_eq!(h.read("own.md").as_deref(), Some("safe\nmore"));
    }

    #[test]
    fn a_note_tracked_anew_still_reports_a_recovery() {
        let mut h = Harness::new();
        h.watch(vec![(Some(1), h.linked("gone.md"))]);
        assert!(h.events().contains(&Event::Status {
            key: Some(1),
            status: SyncStatus::Missing
        }));
        h.write("there.md", "x");
        h.worker.handle(Command::Relink {
            key: Some(1),
            from: h.linked("gone.md"),
            to: h.linked("there.md"),
            text: String::new(),
        });
        assert!(h.events().contains(&Event::Status {
            key: Some(1),
            status: SyncStatus::Synced
        }));
    }

    #[test]
    fn a_renamed_workspace_takes_its_note_file_along() {
        let mut h = Harness::new();
        h.write("old.md", "note");
        h.watch(vec![(Some(1), h.own("old.md"))]);
        h.watch(vec![(Some(1), h.own("new.md"))]);
        assert!(h.read("old.md").is_none());
        assert_eq!(h.read("new.md").as_deref(), Some("note"));
        let (_, text) = h.last_disk(Some(1)).unwrap();
        assert_eq!(text.as_deref(), Some("note"));
    }

    #[test]
    fn a_rename_onto_an_existing_file_keeps_both() {
        let mut h = Harness::new();
        h.write("old.md", "mine");
        h.write("new.md", "already here");
        h.watch(vec![(Some(1), h.own("old.md"))]);
        h.watch(vec![(Some(1), h.own("new.md"))]);
        assert_eq!(h.read("new.md").as_deref(), Some("already here\n\nmine\n"));
        assert_eq!(h.read("old.md.bak").as_deref(), Some("mine"));
    }

    #[test]
    fn notes_left_out_of_watch_stop_being_checked() {
        let mut h = Harness::new();
        h.write("a.md", "v1");
        h.watch(vec![(Some(1), h.own("a.md"))]);
        h.watch(vec![]);
        h.events();
        h.write("a.md", "v2 changed");
        h.worker.poll();
        assert!(h.events().is_empty());
    }

    #[test]
    fn an_emptied_own_note_removes_its_file() {
        let mut h = Harness::new();
        h.write("a.md", "x");
        h.watch(vec![(Some(1), h.own("a.md"))]);
        let (version, _) = h.last_disk(Some(1)).unwrap();
        h.save(Some(1), h.own("a.md"), "", version);
        assert!(h.read("a.md").is_none());
    }

    #[test]
    fn links_are_written_and_flush_answers() {
        let mut h = Harness::new();
        let path = h.path("links.json");
        h.worker.handle(Command::WriteLinks {
            path: path.clone(),
            json: "{}".into(),
        });
        assert_eq!(std::fs::read_to_string(path).unwrap(), "{}");
        let (done, wait) = mpsc::channel();
        assert!(h.worker.handle(Command::Flush(done)));
        assert!(wait.try_recv().is_ok());
        assert!(!h.worker.handle(Command::Shutdown));
    }

    #[test]
    fn the_thread_saves_and_stops_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let target = NoteTarget::own(dir.path().join("a.md"));
        let mut worker = NoteWorker::spawn(None);
        worker.send(Command::Save {
            key: None,
            target: target.clone(),
            text: "threaded".into(),
            base: 0,
        });
        assert!(worker.flush(Duration::from_secs(5)));
        assert_eq!(std::fs::read_to_string(&target.path).unwrap(), "threaded");
        assert!(worker
            .events()
            .any(|e| matches!(e, Event::Disk { key: None, .. })));
        worker.stop(true);
        assert!(!worker.flush(Duration::from_millis(10)));
    }
}
