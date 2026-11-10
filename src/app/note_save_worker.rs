use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub(super) const NOTE_SAVE_DELAY: Duration = Duration::from_millis(500);

pub(super) struct ReadyNoteSave {
    pub(super) group: Option<u64>,
    pub(super) text: String,
    pub(super) generation: u64,
}

pub(super) struct NoteAutosaveState {
    delay: Duration,
    debouncers: HashMap<Option<u64>, super::ui::debounce::Debouncer>,
    pending: HashMap<Option<u64>, String>,
    latest_generation: HashMap<Option<u64>, u64>,
    completed_generation: HashMap<Option<u64>, u64>,
    next_generation: u64,
}

impl NoteAutosaveState {
    pub(super) fn new(delay: Duration) -> Self {
        Self {
            delay,
            debouncers: HashMap::new(),
            pending: HashMap::new(),
            latest_generation: HashMap::new(),
            completed_generation: HashMap::new(),
            next_generation: 1,
        }
    }

    pub(super) fn changed(&mut self, group: Option<u64>, text: String, previous: &str) {
        let debouncer = self.debouncers.entry(group).or_insert_with(|| {
            let mut debouncer = super::ui::debounce::Debouncer::new(self.delay);
            debouncer.update(previous);
            debouncer
        });
        debouncer.update(&text);
        self.pending.insert(group, text);
    }

    pub(super) fn take_ready(&mut self) -> Vec<ReadyNoteSave> {
        let ready_groups: Vec<Option<u64>> = self
            .debouncers
            .iter_mut()
            .filter_map(|(group, debouncer)| debouncer.ready().then_some(*group))
            .collect();
        self.take_groups(ready_groups)
    }

    pub(super) fn take_all_pending(&mut self) -> Vec<ReadyNoteSave> {
        let groups = self.pending.keys().copied().collect();
        self.take_groups(groups)
    }

    fn take_groups(&mut self, groups: Vec<Option<u64>>) -> Vec<ReadyNoteSave> {
        groups
            .into_iter()
            .filter_map(|group| {
                let text = self.pending.remove(&group)?;
                let generation = self.next_generation;
                self.next_generation = self.next_generation.saturating_add(1);
                self.latest_generation.insert(group, generation);
                Some(ReadyNoteSave {
                    group,
                    text,
                    generation,
                })
            })
            .collect()
    }

    pub(super) fn mark_completed(&mut self, group: Option<u64>, generation: u64) {
        self.completed_generation
            .entry(group)
            .and_modify(|completed| *completed = (*completed).max(generation))
            .or_insert(generation);
    }

    pub(super) fn is_saving(&self, group: Option<u64>) -> bool {
        if self.pending.contains_key(&group) {
            return true;
        }
        let latest = self.latest_generation.get(&group).copied().unwrap_or(0);
        let completed = self.completed_generation.get(&group).copied().unwrap_or(0);
        latest > completed
    }

    pub(super) fn has_unsaved_changes(&self) -> bool {
        !self.pending.is_empty()
            || self.latest_generation.iter().any(|(group, latest)| {
                *latest > self.completed_generation.get(group).copied().unwrap_or(0)
            })
    }
}

pub(super) struct NoteSaveRequest {
    pub(super) group: Option<u64>,
    pub(super) path: PathBuf,
    pub(super) text: String,
    pub(super) generation: u64,
}

pub(super) struct NoteSaveCompletion {
    pub(super) group: Option<u64>,
    pub(super) path: PathBuf,
    pub(super) text: String,
    pub(super) generation: u64,
    pub(super) result: Result<(), String>,
}

pub(super) struct NoteSaveWorker {
    tx: mpsc::Sender<NoteSaveRequest>,
    completed_rx: mpsc::Receiver<NoteSaveCompletion>,
    alive: Arc<AtomicBool>,
    ctx: egui::Context,
}

impl NoteSaveWorker {
    pub(super) fn spawn(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<NoteSaveRequest>();
        let (completed_tx, completed_rx) = mpsc::channel::<NoteSaveCompletion>();
        let alive = Arc::new(AtomicBool::new(true));
        let alive_bg = Arc::clone(&alive);
        let ctx_bg = ctx.clone();

        if let Err(error) = std::thread::Builder::new()
            .name("note-save-worker".into())
            .spawn(move || {
                while alive_bg.load(Ordering::Acquire) {
                    let request = match rx.recv_timeout(Duration::from_secs(1)) {
                        Ok(request) => request,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let result = if request.text.is_empty() {
                        match std::fs::remove_file(&request.path) {
                            Ok(()) => Ok(()),
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                            Err(error) => Err(error.to_string()),
                        }
                    } else {
                        crate::util::atomic_write(&request.path, &request.text)
                            .map_err(|error| error.to_string())
                    };
                    let _ = completed_tx.send(NoteSaveCompletion {
                        group: request.group,
                        path: request.path,
                        text: request.text,
                        generation: request.generation,
                        result,
                    });
                    ctx_bg.request_repaint();
                }
            })
        {
            log::error!("failed to spawn note save worker: {error}");
        }

        Self {
            tx,
            completed_rx,
            alive,
            ctx,
        }
    }

    pub(super) fn enqueue(&self, request: NoteSaveRequest) -> bool {
        self.tx.send(request).is_ok()
    }

    pub(super) fn take_completions(&self) -> Vec<NoteSaveCompletion> {
        self.completed_rx.try_iter().collect()
    }

    pub(super) fn receive_completion(&self, timeout: Duration) -> Option<NoteSaveCompletion> {
        self.completed_rx.recv_timeout(timeout).ok()
    }
}

impl Drop for NoteSaveWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        self.ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for_completion(worker: &NoteSaveWorker) -> NoteSaveCompletion {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(completion) = worker.take_completions().into_iter().next() {
                return completion;
            }
            assert!(Instant::now() < deadline, "note save did not complete");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn background_note_save_reports_completion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        let worker = NoteSaveWorker::spawn(egui::Context::default());

        worker.enqueue(NoteSaveRequest {
            group: Some(7),
            path: path.clone(),
            text: "saved off thread".to_owned(),
            generation: 3,
        });
        let completion = wait_for_completion(&worker);

        assert!(completion.result.is_ok());
        assert_eq!(completion.group, Some(7));
        assert_eq!(completion.generation, 3);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "saved off thread");
    }

    #[test]
    fn background_empty_note_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "old").unwrap();
        let worker = NoteSaveWorker::spawn(egui::Context::default());

        worker.enqueue(NoteSaveRequest {
            group: None,
            path: path.clone(),
            text: String::new(),
            generation: 1,
        });
        let completion = wait_for_completion(&worker);

        assert!(completion.result.is_ok());
        assert!(!path.exists());
    }

    #[test]
    fn autosave_state_is_saving_until_latest_generation_completes() {
        let mut state = NoteAutosaveState::new(Duration::ZERO);
        state.changed(Some(9), "draft".to_owned(), "");

        assert!(state.is_saving(Some(9)));
        let ready = state.take_ready();
        assert_eq!(ready.len(), 1);
        assert!(state.is_saving(Some(9)));

        state.mark_completed(Some(9), ready[0].generation);
        assert!(!state.is_saving(Some(9)));
    }

    #[test]
    fn autosave_state_can_flush_before_debounce_expires() {
        let mut state = NoteAutosaveState::new(Duration::from_secs(60));
        state.changed(None, "closing".to_owned(), "");

        let ready = state.take_all_pending();

        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].text, "closing");
    }
}
