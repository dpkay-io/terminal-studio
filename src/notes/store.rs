//! `NoteStore`: the UI's side of the notes. It holds each note's text for
//! the text boxes, sends edits to the note worker a moment after typing
//! stops, takes in edits made by other apps, and keeps which notes are
//! linked to files of the user's choosing. No file is touched on the UI
//! thread except the one-off loading of links at startup.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::links::{NoteLinks, LINKS_FILENAME};
use super::merge::merge_note_texts;
use super::target::{own_note_path, NoteKey, NoteTarget};
use super::worker::{Command, Event, NoteWorker, SyncStatus};
use crate::workspace::Workspace;

/// Name of the notes folder inside the app's data folder.
pub const NOTES_DIR: &str = "notes";
/// Edits are saved once typing pauses this long…
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);
/// …or once they have waited this long, while typing goes on.
pub const SAVE_MAX_DELAY: Duration = Duration::from_secs(2);
/// A save that failed is tried again after this long.
pub const SAVE_RETRY: Duration = Duration::from_secs(5);
/// How long quitting waits for notes to be written.
pub const FLUSH_TIMEOUT: Duration = Duration::from_secs(3);

/// How a note's file is doing, for its header.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NoteStatus {
    #[default]
    Synced,
    /// The linked file is gone; the next edit writes it again.
    Missing,
    /// The file could not be read or written: why.
    Failed(String),
    /// The last link asked for could not be made: why.
    LinkFailed(String),
}

#[derive(Default)]
struct NoteEntry {
    text: String,
    /// The worker's version of the file that `text` is built on.
    base: u64,
    /// When edits not yet sent go to the worker; `None` = none waiting.
    save_at: Option<Instant>,
    /// When the oldest edit not yet sent was made.
    dirty_since: Option<Instant>,
    /// Sent, but not yet confirmed written.
    unsaved: bool,
}

impl NoteEntry {
    fn has_edits(&self) -> bool {
        self.save_at.is_some() || self.unsaved
    }

    /// Saves the text a moment after the last edit, and at the latest
    /// `SAVE_MAX_DELAY` after the first one not sent yet.
    fn schedule_save(&mut self, now: Instant) {
        let first = *self.dirty_since.get_or_insert(now);
        self.save_at = Some((now + SAVE_DEBOUNCE).min(first + SAVE_MAX_DELAY));
    }
}

pub struct NoteStore {
    /// The app's own note files; `None` when there is no data folder (notes
    /// then live only in memory unless linked).
    notes_dir: Option<PathBuf>,
    notes: HashMap<NoteKey, NoteEntry>,
    links: NoteLinks,
    statuses: HashMap<NoteKey, SyncStatus>,
    link_errors: HashMap<NoteKey, String>,
    /// Each watched note's file, as last sent to the worker.
    targets: HashMap<NoteKey, NoteTarget>,
    /// Each watched note's own file in `notes_dir`, linked or not.
    own_paths: HashMap<NoteKey, PathBuf>,
    /// Of the workspaces' ids and names, as `targets` were last made from;
    /// `None` (a link changed, or nothing sent yet) makes them again.
    watch_fingerprint: Option<u64>,
    worker: NoteWorker,
    repaint: Option<egui::Context>,
}

impl NoteStore {
    /// The notes in the app's data folder (made if missing), with legacy
    /// `notes.json` moved into files first. Their text arrives a moment
    /// later from the worker.
    pub fn load(ctx: &egui::Context, workspaces: &[Workspace]) -> Self {
        let notes_dir = crate::util::data_dir().map(|d| d.join(NOTES_DIR));
        if let Some(dir) = &notes_dir {
            if let Err(e) = std::fs::create_dir_all(dir) {
                log::error!("could not create the notes folder {}: {e}", dir.display());
            }
            super::legacy::migrate_from_json(dir, workspaces);
        }
        Self::with_dir(notes_dir, Some(ctx.clone()), workspaces)
    }

    fn with_dir(
        notes_dir: Option<PathBuf>,
        repaint: Option<egui::Context>,
        workspaces: &[Workspace],
    ) -> Self {
        let links = notes_dir
            .as_ref()
            .map(|d| NoteLinks::load(&d.join(LINKS_FILENAME)))
            .unwrap_or_default();
        let mut store = Self {
            notes_dir,
            notes: HashMap::new(),
            links,
            statuses: HashMap::new(),
            link_errors: HashMap::new(),
            targets: HashMap::new(),
            own_paths: HashMap::new(),
            watch_fingerprint: None,
            worker: NoteWorker::spawn(repaint.clone()),
            repaint,
        };
        store.sync(workspaces);
        store
    }

    pub fn get(&self, key: NoteKey) -> &str {
        self.notes.get(&key).map(|n| n.text.as_str()).unwrap_or("")
    }

    /// The note's new text, typed here. Saved shortly after.
    pub fn set(&mut self, key: NoteKey, text: String) {
        let entry = self.notes.entry(key).or_default();
        if entry.text == text {
            return;
        }
        entry.text = text;
        entry.schedule_save(Instant::now());
        let save_at = entry.save_at;
        self.link_errors.remove(&key);
        self.wake_at(save_at);
    }

    /// The file the note is linked to, if any.
    pub fn link(&self, key: NoteKey) -> Option<&Path> {
        self.links.get(key)
    }

    /// The file the note is kept in: its linked file, else the app's own
    /// (which may not exist yet: an empty note has none). `None` only
    /// without a notes folder.
    pub fn file(&self, key: NoteKey) -> Option<&Path> {
        self.link(key)
            .or_else(|| self.own_paths.get(&key).map(PathBuf::as_path))
    }

    pub fn status(&self, key: NoteKey) -> NoteStatus {
        if let Some(reason) = self.link_errors.get(&key) {
            return NoteStatus::LinkFailed(reason.clone());
        }
        match self.statuses.get(&key) {
            None | Some(SyncStatus::Synced) => NoteStatus::Synced,
            Some(SyncStatus::Missing) => NoteStatus::Missing,
            Some(SyncStatus::Failed(reason)) => NoteStatus::Failed(reason.clone()),
        }
    }

    /// Keeps the note in `path` from now on (`None`: in the app's own file
    /// again). Both files' text and any edits not saved yet end up in the
    /// new file, nothing appearing twice; the old file is kept. Returns
    /// whether anything changed.
    pub fn set_link(&mut self, key: NoteKey, path: Option<PathBuf>) -> bool {
        let own = self.own_target(key);
        // Linking the note's own file is the same as unlinking it.
        let path = path.filter(|p| own.as_ref().is_none_or(|o| &o.path != p));
        let to = match path.clone() {
            Some(p) => NoteTarget::linked(p),
            None => match own {
                Some(own) => own,
                None => return false,
            },
        };
        let Some(from) = self.target_for(key) else {
            return false;
        };
        if from == to {
            return false;
        }
        self.change_link(key, path);
        self.targets.insert(key, to.clone());
        self.link_errors.remove(&key);
        let entry = self.notes.entry(key).or_default();
        entry.save_at = None;
        entry.dirty_since = None;
        entry.unsaved = true;
        let text = entry.text.clone();
        self.worker.send(Command::Relink {
            key,
            from,
            to,
            text,
        });
        true
    }

    /// Once per frame: takes in the worker's news, tells it which files to
    /// watch when that changed, and sends edits that are due.
    pub fn sync(&mut self, workspaces: &[Workspace]) {
        let events: Vec<Event> = self.worker.events().collect();
        for event in events {
            self.apply(event);
        }
        self.refresh_targets(workspaces);
        self.send_due_saves(Instant::now());
    }

    /// Sends every edit not confirmed written and waits up to `timeout` for
    /// the worker to write them: for quitting and restarting.
    pub fn flush(&self, timeout: Duration) -> bool {
        for (key, entry) in &self.notes {
            if entry.has_edits() {
                self.send_save(*key, entry);
            }
        }
        self.worker.flush(timeout)
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::Disk { key, version, text } => {
                let entry = self.notes.entry(key).or_default();
                entry.base = version;
                let had_edits = entry.has_edits();
                entry.unsaved = false;
                let Some(disk) = text else {
                    return;
                };
                if had_edits {
                    // Edits here the file does not have yet: keep both.
                    entry.text = merge_note_texts(&disk, &entry.text);
                    if entry.text != disk {
                        entry.schedule_save(Instant::now());
                    }
                } else {
                    entry.text = disk;
                }
            }
            Event::Status { key, status } => {
                self.statuses.insert(key, status);
            }
            Event::RelinkFailed {
                key,
                previous,
                reason,
            } => {
                let link = previous.linked.then(|| previous.path.clone());
                self.change_link(key, link);
                self.targets.insert(key, previous);
                self.link_errors.insert(key, reason);
                let entry = self.notes.entry(key).or_default();
                // The relink carried the text: save it where it stays.
                entry.unsaved = false;
                entry.schedule_save(Instant::now());
            }
        }
    }

    fn send_due_saves(&mut self, now: Instant) {
        let mut next_wake: Option<Instant> = None;
        let mut due = Vec::new();
        for (key, entry) in &mut self.notes {
            let failed = matches!(self.statuses.get(key), Some(SyncStatus::Failed(_)));
            if entry.unsaved && failed && entry.save_at.is_none() {
                entry.save_at = Some(now + SAVE_RETRY);
            }
            match entry.save_at {
                Some(at) if at <= now => due.push(*key),
                Some(at) => next_wake = Some(next_wake.map_or(at, |w| w.min(at))),
                None => {}
            }
        }
        for key in due {
            if let Some(entry) = self.notes.get_mut(&key) {
                entry.save_at = None;
                entry.dirty_since = None;
                entry.unsaved = true;
            }
            if let Some(entry) = self.notes.get(&key) {
                self.send_save(key, entry);
            }
        }
        self.wake_at(next_wake);
    }

    fn send_save(&self, key: NoteKey, entry: &NoteEntry) {
        let Some(target) = self.target_for(key) else {
            return;
        };
        self.worker.send(Command::Save {
            key,
            target,
            text: entry.text.clone(),
            base: entry.base,
        });
    }

    /// Sends the files to watch when the workspaces, their names or the
    /// links changed since last sent. Runs every frame: when nothing changed
    /// it only hashes each workspace's id and name.
    fn refresh_targets(&mut self, workspaces: &[Workspace]) {
        let fingerprint = Self::fingerprint(workspaces);
        if self.watch_fingerprint == Some(fingerprint) {
            return;
        }
        self.watch_fingerprint = Some(fingerprint);
        let keys: Vec<NoteKey> = std::iter::once(None)
            .chain(workspaces.iter().map(|w| Some(w.id)))
            .collect();
        self.own_paths = match &self.notes_dir {
            Some(dir) => keys
                .iter()
                .map(|key| (*key, own_note_path(dir, *key, workspaces)))
                .collect(),
            None => HashMap::new(),
        };
        self.targets = keys
            .iter()
            .filter_map(|key| self.resolve(*key, workspaces).map(|t| (*key, t)))
            .collect();
        let watched = self.targets.iter().map(|(k, t)| (*k, t.clone())).collect();
        self.worker.send(Command::Watch(watched));
    }

    /// The links are not in it: they change only through
    /// [`Self::change_link`], which asks for new targets itself. The notes
    /// folder never changes.
    fn fingerprint(workspaces: &[Workspace]) -> u64 {
        let mut hasher = DefaultHasher::new();
        for w in workspaces {
            (w.id, &w.name).hash(&mut hasher);
        }
        hasher.finish()
    }

    /// The note's file: its link, else its own file.
    fn resolve(&self, key: NoteKey, workspaces: &[Workspace]) -> Option<NoteTarget> {
        if let Some(path) = self.links.get(key) {
            return Some(NoteTarget::linked(path.to_path_buf()));
        }
        let dir = self.notes_dir.as_ref()?;
        Some(NoteTarget::own(own_note_path(dir, key, workspaces)))
    }

    /// The note's file as watched; a note of a workspace that no longer
    /// exists still has one (`orphan_<id>.md`, or its link).
    fn target_for(&self, key: NoteKey) -> Option<NoteTarget> {
        self.targets
            .get(&key)
            .cloned()
            .or_else(|| self.resolve(key, &[]))
    }

    /// The app's own file for the note, whether or not it is linked.
    fn own_target(&self, key: NoteKey) -> Option<NoteTarget> {
        let path = match self.own_paths.get(&key) {
            Some(path) => path.clone(),
            None => own_note_path(self.notes_dir.as_ref()?, key, &[]),
        };
        Some(NoteTarget::own(path))
    }

    /// Links the note to `path` (`None`: unlinks it), saves the links and
    /// has the files to watch made again on the next sync.
    fn change_link(&mut self, key: NoteKey, path: Option<PathBuf>) {
        self.links.set(key, path);
        self.save_links();
        self.watch_fingerprint = None;
    }

    fn save_links(&self) {
        let Some(dir) = &self.notes_dir else {
            return;
        };
        self.worker.send(Command::WriteLinks {
            path: dir.join(LINKS_FILENAME),
            json: self.links.to_json(),
        });
    }

    fn wake_at(&self, at: Option<Instant>) {
        let (Some(ctx), Some(at)) = (&self.repaint, at) else {
            return;
        };
        ctx.request_repaint_after(at.saturating_duration_since(Instant::now()));
    }
}

impl Drop for NoteStore {
    /// Edits not written yet are written before the worker stops; a worker
    /// stuck on a slow disk is left behind rather than holding up quitting.
    fn drop(&mut self) {
        let finished = self.flush(FLUSH_TIMEOUT);
        self.worker.stop(finished);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::target::tests::workspaces;

    /// Long enough for a debounced save and a poll on a slow CI machine.
    const WAIT: Duration = Duration::from_secs(10);
    const STEP: Duration = Duration::from_millis(10);

    fn store_in(dir: &Path, ws: &[Workspace]) -> NoteStore {
        NoteStore::with_dir(Some(dir.to_path_buf()), None, ws)
    }

    /// Syncs until `done` holds; false when it never did.
    fn wait_until(
        store: &mut NoteStore,
        ws: &[Workspace],
        mut done: impl FnMut(&NoteStore) -> bool,
    ) -> bool {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            store.sync(ws);
            if done(store) {
                return true;
            }
            std::thread::sleep(STEP);
        }
        false
    }

    fn read(path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    #[test]
    fn existing_note_files_are_loaded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("general.md"), "general note").unwrap();
        std::fs::write(dir.path().join("proj.md"), "proj note").unwrap();
        let ws = workspaces(&[("proj", 1)]);
        let mut store = store_in(dir.path(), &ws);
        assert!(wait_until(&mut store, &ws, |s| s.get(None)
            == "general note"
            && s.get(Some(1)) == "proj note"));
        assert_eq!(store.get(Some(2)), "");
    }

    #[test]
    fn typing_is_saved_once_it_pauses() {
        let dir = tempfile::tempdir().unwrap();
        let ws = workspaces(&[("proj", 1)]);
        let mut store = store_in(dir.path(), &ws);
        store.set(Some(1), "typed".into());
        store.sync(&ws);
        assert_eq!(store.get(Some(1)), "typed");
        let file = dir.path().join("proj.md");
        assert!(wait_until(&mut store, &ws, |_| read(&file).as_deref() == Some("typed")));
        assert_eq!(store.status(Some(1)), NoteStatus::Synced);
    }

    #[test]
    fn edits_made_elsewhere_show_up() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("proj.md");
        std::fs::write(&file, "before").unwrap();
        let ws = workspaces(&[("proj", 1)]);
        let mut store = store_in(dir.path(), &ws);
        assert!(wait_until(&mut store, &ws, |s| s.get(Some(1)) == "before"));
        std::fs::write(&file, "edited in obsidian").unwrap();
        assert!(wait_until(&mut store, &ws, |s| s.get(Some(1)) == "edited in obsidian"));
    }

    #[test]
    fn note_files_deleted_while_running_keep_the_notes() {
        let dir = tempfile::tempdir().unwrap();
        let own = dir.path().join("proj.md");
        let linked = dir.path().join("vault.md");
        std::fs::write(&own, "own note").unwrap();
        std::fs::write(&linked, "linked note").unwrap();
        let ws = workspaces(&[("proj", 1)]);
        let mut store = store_in(dir.path(), &ws);
        assert!(store.set_link(None, Some(linked.clone())));
        assert!(wait_until(&mut store, &ws, |s| s.get(Some(1))
            == "own note"
            && s.get(None).contains("linked note")));
        std::fs::remove_file(&own).unwrap();
        std::fs::remove_file(&linked).unwrap();
        std::fs::remove_file(dir.path().join(LINKS_FILENAME)).unwrap();
        // The linked file is said to be missing; both notes keep their text.
        assert!(wait_until(&mut store, &ws, |s| s.status(None) == NoteStatus::Missing));
        assert_eq!(store.get(Some(1)), "own note");
        assert!(store.get(None).contains("linked note"));
        assert_eq!(store.status(Some(1)), NoteStatus::Synced);
        // Typing writes both files again.
        store.set(Some(1), "own note, typed".into());
        store.set(None, "linked note, typed".into());
        assert!(wait_until(&mut store, &ws, |s| {
            read(&own).as_deref() == Some("own note, typed")
                && read(&linked).as_deref() == Some("linked note, typed")
                && s.status(None) == NoteStatus::Synced
        }));
        assert_eq!(store.link(None), Some(linked.as_path()));
    }

    #[test]
    fn edits_not_saved_yet_are_merged_with_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_in(dir.path(), &[]);
        store.set(None, "typed here".into());
        store.apply(Event::Disk {
            key: None,
            version: 7,
            text: Some("from the file".into()),
        });
        assert_eq!(store.get(None), "from the file\n\ntyped here\n");
        assert!(store.notes[&None].save_at.is_some(), "the merge is saved");
        assert_eq!(store.notes[&None].base, 7);
    }

    #[test]
    fn a_note_without_edits_takes_the_file_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_in(dir.path(), &[]);
        store.apply(Event::Disk {
            key: Some(3),
            version: 2,
            text: Some("disk".into()),
        });
        assert_eq!(store.get(Some(3)), "disk");
        assert!(store.notes[&Some(3)].save_at.is_none());
    }

    #[test]
    fn a_failed_save_is_tried_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = store_in(dir.path(), &[]);
        store.set(None, "text".into());
        let now = Instant::now();
        store.send_due_saves(now + SAVE_MAX_DELAY);
        assert!(store.notes[&None].unsaved);
        store.apply(Event::Status {
            key: None,
            status: SyncStatus::Failed("disk full".into()),
        });
        assert_eq!(store.status(None), NoteStatus::Failed("disk full".into()));
        store.send_due_saves(now);
        assert_eq!(store.notes[&None].save_at, Some(now + SAVE_RETRY));
    }

    #[test]
    fn linking_merges_both_texts_and_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let vault = tempfile::tempdir().unwrap();
        let vault_file = vault.path().join("proj.md");
        std::fs::write(dir.path().join("proj.md"), "own text").unwrap();
        std::fs::write(&vault_file, "# Vault page").unwrap();
        let ws = workspaces(&[("proj", 1)]);
        let mut store = store_in(dir.path(), &ws);
        assert!(wait_until(&mut store, &ws, |s| s.get(Some(1)) == "own text"));

        assert!(store.set_link(Some(1), Some(vault_file.clone())));
        let merged = "# Vault page\n\nown text\n";
        assert!(wait_until(&mut store, &ws, |s| s.get(Some(1)) == merged));
        assert_eq!(read(&vault_file).as_deref(), Some(merged));
        assert_eq!(store.link(Some(1)), Some(vault_file.as_path()));
        // The own file is kept aside, not deleted.
        assert_eq!(
            read(&dir.path().join("proj.md.bak")).as_deref(),
            Some("own text")
        );

        // Typing here goes to the linked file.
        store.set(Some(1), format!("{merged}- typed"));
        assert!(wait_until(&mut store, &ws, |_| read(&vault_file)
            .is_some_and(|t| t.ends_with("- typed"))));

        drop(store);
        let mut again = store_in(dir.path(), &ws);
        assert_eq!(again.link(Some(1)), Some(vault_file.as_path()));
        assert!(wait_until(&mut again, &ws, |s| s
            .get(Some(1))
            .ends_with("- typed")));
    }

    #[test]
    fn unlinking_brings_the_text_back_and_keeps_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let vault_file = dir.path().join("vault.md");
        std::fs::write(&vault_file, "vault text").unwrap();
        let ws = workspaces(&[("proj", 1)]);
        let mut store = store_in(dir.path(), &ws);
        store.set_link(Some(1), Some(vault_file.clone()));
        assert!(wait_until(&mut store, &ws, |s| s.get(Some(1)) == "vault text"));
        assert!(store.set_link(Some(1), None));
        let own = dir.path().join("proj.md");
        assert!(wait_until(&mut store, &ws, |_| read(&own).as_deref()
            == Some("vault text")));
        assert_eq!(store.link(Some(1)), None);
        assert_eq!(read(&vault_file).as_deref(), Some("vault text"));
    }

    #[test]
    fn linking_the_same_file_or_the_own_file_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let ws = workspaces(&[("proj", 1)]);
        let mut store = store_in(dir.path(), &ws);
        assert!(!store.set_link(Some(1), None), "already in its own file");
        assert!(!store.set_link(Some(1), Some(dir.path().join("proj.md"))));
        let other = dir.path().join("other.md");
        assert!(store.set_link(Some(1), Some(other.clone())));
        assert!(!store.set_link(Some(1), Some(other)));
    }

    #[test]
    fn a_link_that_fails_is_undone_and_said() {
        let dir = tempfile::tempdir().unwrap();
        let ws = workspaces(&[("proj", 1)]);
        let mut store = store_in(dir.path(), &ws);
        store.set(Some(1), "keep".into());
        let binary = dir.path().join("image.png");
        std::fs::write(&binary, [0x89, 0x50, 0xff, 0x00]).unwrap();
        store.set_link(Some(1), Some(binary.clone()));
        assert!(wait_until(&mut store, &ws, |s| matches!(
            s.status(Some(1)),
            NoteStatus::LinkFailed(_)
        )));
        assert_eq!(store.link(Some(1)), None);
        assert_eq!(store.get(Some(1)), "keep");
        assert_eq!(
            std::fs::read(&binary).unwrap(),
            vec![0x89, 0x50, 0xff, 0x00]
        );
        let own = dir.path().join("proj.md");
        assert!(wait_until(&mut store, &ws, |_| read(&own).as_deref() == Some("keep")));
        // Typing again clears the message.
        store.set(Some(1), "keep going".into());
        assert_eq!(store.status(Some(1)), NoteStatus::Synced);
    }

    #[test]
    fn a_renamed_workspace_takes_its_note_file_along() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("old.md"), "note").unwrap();
        let mut ws = workspaces(&[("old", 1)]);
        let mut store = store_in(dir.path(), &ws);
        assert!(wait_until(&mut store, &ws, |s| s.get(Some(1)) == "note"));
        ws[0].name = "new".into();
        let new = dir.path().join("new.md");
        assert!(wait_until(&mut store, &ws, |_| read(&new).as_deref() == Some("note")));
        assert!(!dir.path().join("old.md").exists());
        assert_eq!(store.get(Some(1)), "note");
    }

    #[test]
    fn edits_are_written_when_the_store_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let ws = workspaces(&[("proj", 1)]);
        let mut store = store_in(dir.path(), &ws);
        store.set(Some(1), "typed just before quitting".into());
        drop(store);
        assert_eq!(
            read(&dir.path().join("proj.md")).as_deref(),
            Some("typed just before quitting")
        );
    }

    #[test]
    fn saves_wait_for_typing_to_pause_but_not_forever() {
        let mut entry = NoteEntry::default();
        let start = Instant::now();
        entry.schedule_save(start);
        assert_eq!(entry.save_at, Some(start + SAVE_DEBOUNCE));
        // Still typing near the limit: the save is not pushed past it.
        entry.schedule_save(start + SAVE_MAX_DELAY - Duration::from_millis(1));
        assert_eq!(entry.save_at, Some(start + SAVE_MAX_DELAY));
    }

    #[test]
    fn without_a_notes_folder_nothing_is_written() {
        let mut store = NoteStore::with_dir(None, None, &[]);
        store.set(None, "in memory".into());
        store.sync(&[]);
        assert!(store.flush(WAIT));
        assert_eq!(store.get(None), "in memory");
        assert!(!store.set_link(None, None));
    }
}
