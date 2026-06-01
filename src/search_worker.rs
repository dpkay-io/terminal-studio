use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use parking_lot::{Mutex, RwLock};

use crate::terminal::Session;

#[derive(Clone)]
pub struct GlobalSearchMatch {
    pub session_id: u32,
    pub session_title: String,
    pub line_text: String,
    pub score: i64,
}

pub struct GlobalSearchResults {
    pub matches: Vec<GlobalSearchMatch>,
    pub query: String,
    pub completed: bool,
}

struct SearchJob {
    query: String,
    sessions: Vec<(u32, String, Arc<RwLock<Session>>)>,
    generation: u64,
}

pub struct SearchWorker {
    tx: mpsc::Sender<SearchJob>,
    results: Arc<Mutex<GlobalSearchResults>>,
    generation: Arc<Mutex<u64>>,
    alive: Arc<AtomicBool>,
    thread_running: bool,
}

impl SearchWorker {
    pub fn spawn(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<SearchJob>();
        let results = Arc::new(Mutex::new(GlobalSearchResults {
            matches: Vec::new(),
            query: String::new(),
            completed: true,
        }));
        let generation = Arc::new(Mutex::new(0u64));
        let alive = Arc::new(AtomicBool::new(true));

        let results_bg = Arc::clone(&results);
        let generation_bg = Arc::clone(&generation);
        let alive_bg = Arc::clone(&alive);

        let thread_running =
            match thread::Builder::new()
                .name("search-worker".into())
                .spawn(move || {
                    let matcher = SkimMatcherV2::default();
                    while alive_bg.load(Ordering::Relaxed) {
                        let job = match rx.recv_timeout(Duration::from_secs(1)) {
                            Ok(j) => j,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        };

                        if *generation_bg.lock() != job.generation {
                            continue;
                        }

                        let mut all_matches = Vec::new();

                        for (session_id, session_title, session_lock) in &job.sessions {
                            // Check generation BEFORE acquiring the session lock
                            if *generation_bg.lock() != job.generation {
                                break;
                            }

                            // Extract all needed data under a short-lived read lock,
                            // then release it before doing any further locking or heavy work.
                            let lines: Vec<String> = {
                                let session = session_lock.read();
                                let term = &session.term;
                                let grid = term.grid();
                                let cols = term.columns();
                                let total_lines = term.screen_lines() as i32;
                                let history = grid.history_size() as i32;

                                let mut extracted = Vec::new();
                                for line_idx in (-history)..total_lines {
                                    let mut line_text = String::with_capacity(cols);
                                    for col in 0..cols {
                                        let cell = &grid[Line(line_idx)][Column(col)];
                                        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                                            continue;
                                        }
                                        line_text.push(cell.c);
                                    }
                                    let trimmed = line_text.trim_end().to_string();
                                    if !trimmed.is_empty() {
                                        extracted.push(trimmed);
                                    }
                                }
                                extracted
                            };

                            for trimmed in &lines {
                                if let Some(score) = matcher.fuzzy_match(trimmed, &job.query) {
                                    all_matches.push(GlobalSearchMatch {
                                        session_id: *session_id,
                                        session_title: session_title.clone(),
                                        line_text: trimmed.clone(),
                                        score,
                                    });
                                }
                            }
                        }

                        if *generation_bg.lock() != job.generation {
                            continue;
                        }

                        all_matches.sort_by_key(|b| std::cmp::Reverse(b.score));
                        all_matches.truncate(200);

                        let mut res = results_bg.lock();
                        res.matches = all_matches;
                        res.query = job.query;
                        res.completed = true;
                        ctx.request_repaint();
                    }
                }) {
                Ok(_) => true,
                Err(e) => {
                    log::error!("failed to spawn search-worker thread: {e}");
                    false
                }
            };

        SearchWorker {
            tx,
            results,
            generation,
            alive,
            thread_running,
        }
    }

    pub fn search(&self, query: String, sessions: Vec<(u32, String, Arc<RwLock<Session>>)>) {
        if !self.thread_running {
            log::warn!("search-worker thread not running; search ignored");
            return;
        }
        let gen = {
            let mut g = self.generation.lock();
            *g += 1;
            *g
        };
        {
            let mut res = self.results.lock();
            res.completed = false;
            res.query = query.clone();
        }
        let _ = self.tx.send(SearchJob {
            query,
            sessions,
            generation: gen,
        });
    }

    pub fn results(&self) -> parking_lot::MutexGuard<'_, GlobalSearchResults> {
        self.results.lock()
    }

    pub fn cancel(&self) {
        let mut g = self.generation.lock();
        *g += 1;
        let mut res = self.results.lock();
        res.matches.clear();
        res.completed = true;
    }
}

impl Drop for SearchWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.tx.send(SearchJob {
            query: String::new(),
            sessions: Vec::new(),
            generation: u64::MAX,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
    use std::thread;
    use std::time::Duration;

    fn make_session_with_content(id: u32, content: &[u8]) -> Arc<RwLock<Session>> {
        let mut session = Session::new_for_test(id, 80, 24);
        let mut processor: Processor<StdSyncHandler> = Processor::new();
        processor.advance(&mut session.term, content);
        Arc::new(RwLock::new(session))
    }

    fn wait_for_results(worker: &SearchWorker) {
        for _ in 0..40 {
            thread::sleep(Duration::from_millis(50));
            if worker.results().completed {
                break;
            }
        }
    }

    #[test]
    fn test_spawn_and_drop() {
        let worker = SearchWorker::spawn(egui::Context::default());
        drop(worker);
        // No panic — success
    }

    #[test]
    fn test_cancel_clears_results() {
        let worker = SearchWorker::spawn(egui::Context::default());
        worker.cancel();
        let res = worker.results();
        assert!(res.matches.is_empty());
        assert!(res.completed);
    }

    #[test]
    fn test_search_empty_sessions() {
        let worker = SearchWorker::spawn(egui::Context::default());
        worker.search("anything".into(), Vec::new());

        wait_for_results(&worker);

        let res = worker.results();
        assert!(res.completed);
        assert!(res.matches.is_empty());
    }

    #[test]
    fn test_search_finds_content() {
        let session_arc = make_session_with_content(1, b"Hello World\r\n");
        let sessions = vec![(1, "Test Session".to_string(), session_arc)];

        let worker = SearchWorker::spawn(egui::Context::default());
        worker.search("Hello".into(), sessions);

        wait_for_results(&worker);

        let res = worker.results();
        assert!(res.completed);
        assert!(
            !res.matches.is_empty(),
            "expected at least one match for 'Hello'"
        );
        assert!(
            res.matches.iter().any(|m| m.line_text.contains("Hello")),
            "expected a match containing 'Hello', got: {:?}",
            res.matches.iter().map(|m| &m.line_text).collect::<Vec<_>>()
        );
        assert_eq!(res.matches[0].session_id, 1);
        assert_eq!(res.matches[0].session_title, "Test Session");
    }

    #[test]
    fn test_search_no_match() {
        let session_arc = make_session_with_content(1, b"Hello World\r\n");
        let sessions = vec![(1, "Test Session".to_string(), session_arc)];

        let worker = SearchWorker::spawn(egui::Context::default());
        worker.search("zzzznothere".into(), sessions);

        wait_for_results(&worker);

        let res = worker.results();
        assert!(res.completed);
        assert!(
            res.matches.is_empty(),
            "expected no matches for 'zzzznothere'"
        );
    }

    #[test]
    fn test_search_results_sorted_by_score() {
        // Create sessions with different content to get varying scores
        let s1 = make_session_with_content(1, b"alpha beta gamma\r\n");
        let s2 = make_session_with_content(2, b"alpha\r\n");
        let s3 = make_session_with_content(3, b"alpha beta gamma delta alpha\r\n");

        let sessions = vec![
            (1, "S1".to_string(), s1),
            (2, "S2".to_string(), s2),
            (3, "S3".to_string(), s3),
        ];

        let worker = SearchWorker::spawn(egui::Context::default());
        worker.search("alpha".into(), sessions);

        wait_for_results(&worker);

        let res = worker.results();
        assert!(res.completed);
        // Verify results are sorted by score descending
        for window in res.matches.windows(2) {
            assert!(
                window[0].score >= window[1].score,
                "results not sorted by score descending: {} < {}",
                window[0].score,
                window[1].score
            );
        }
    }
}
