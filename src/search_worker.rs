use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use parking_lot::{Mutex, RwLock};

use crate::terminal::Session;

#[derive(Clone)]
#[allow(dead_code)]
pub struct GlobalSearchMatch {
    pub session_id: u32,
    pub session_title: String,
    pub line: i32,
    pub line_text: String,
    pub start_col: usize,
    pub end_col: usize,
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

        thread::Builder::new()
            .name("search-worker".into())
            .spawn(move || {
                let matcher = SkimMatcherV2::default();
                while alive_bg.load(Ordering::Relaxed) {
                    let job = match rx.recv() {
                        Ok(j) => j,
                        Err(_) => break,
                    };

                    if *generation_bg.lock() != job.generation {
                        continue;
                    }

                    let query_lower = job.query.to_lowercase();
                    let mut all_matches = Vec::new();

                    for (session_id, session_title, session) in &job.sessions {
                        if *generation_bg.lock() != job.generation {
                            break;
                        }

                        let session = session.read();
                        let term = &session.term;
                        let grid = term.grid();
                        let cols = term.columns();
                        let total_lines = term.screen_lines() as i32;
                        let history = grid.history_size() as i32;

                        for line_idx in (-history)..total_lines {
                            let mut line_text = String::with_capacity(cols);
                            for col in 0..cols {
                                let cell = &grid[Line(line_idx)][Column(col)];
                                if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                                    continue;
                                }
                                line_text.push(cell.c);
                            }

                            let trimmed = line_text.trim_end();
                            if trimmed.is_empty() {
                                continue;
                            }

                            if let Some(score) = matcher.fuzzy_match(trimmed, &job.query) {
                                let line_lower = trimmed.to_lowercase();
                                let (start, end) = if let Some(pos) = line_lower.find(&query_lower)
                                {
                                    (pos, pos + query_lower.len())
                                } else {
                                    (0, trimmed.len().min(query_lower.len()))
                                };

                                all_matches.push(GlobalSearchMatch {
                                    session_id: *session_id,
                                    session_title: session_title.clone(),
                                    line: line_idx,
                                    line_text: trimmed.to_string(),
                                    start_col: start,
                                    end_col: end,
                                    score,
                                });
                            }
                        }
                    }

                    if *generation_bg.lock() != job.generation {
                        continue;
                    }

                    all_matches.sort_by(|a, b| b.score.cmp(&a.score));
                    all_matches.truncate(200);

                    let mut res = results_bg.lock();
                    res.matches = all_matches;
                    res.query = job.query;
                    res.completed = true;
                    ctx.request_repaint();
                }
            })
            .expect("failed to spawn search-worker thread");

        SearchWorker {
            tx,
            results,
            generation,
            alive,
        }
    }

    pub fn search(&self, query: String, sessions: Vec<(u32, String, Arc<RwLock<Session>>)>) {
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
