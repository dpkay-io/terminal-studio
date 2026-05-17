use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use parking_lot::Mutex;

#[derive(Clone)]
pub struct FileSearchMatch {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub score: i64,
}

pub struct FileSearchResults {
    pub matches: Vec<FileSearchMatch>,
    pub query: String,
    pub root: PathBuf,
    pub completed: bool,
}

struct FileSearchJob {
    query: String,
    root: PathBuf,
    generation: u64,
}

pub struct FileSearchWorker {
    tx: mpsc::Sender<FileSearchJob>,
    results: Arc<Mutex<FileSearchResults>>,
    generation: Arc<Mutex<u64>>,
    alive: Arc<AtomicBool>,
}

impl FileSearchWorker {
    pub fn spawn(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<FileSearchJob>();
        let results = Arc::new(Mutex::new(FileSearchResults {
            matches: Vec::new(),
            query: String::new(),
            root: PathBuf::new(),
            completed: true,
        }));
        let generation = Arc::new(Mutex::new(0u64));
        let alive = Arc::new(AtomicBool::new(true));

        let results_bg = Arc::clone(&results);
        let generation_bg = Arc::clone(&generation);
        let alive_bg = Arc::clone(&alive);

        thread::Builder::new()
            .name("file-search-worker".into())
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

                    let mut all_files = Vec::new();
                    collect_files_recursive(
                        &job.root,
                        &mut all_files,
                        5,
                        &generation_bg,
                        job.generation,
                    );

                    if *generation_bg.lock() != job.generation {
                        continue;
                    }

                    let mut scored: Vec<FileSearchMatch> = all_files
                        .into_iter()
                        .filter_map(|(name, path, is_dir)| {
                            matcher
                                .fuzzy_match(&name, &job.query)
                                .map(|score| FileSearchMatch {
                                    name,
                                    path,
                                    is_dir,
                                    score,
                                })
                        })
                        .collect();

                    scored.sort_by_key(|b| std::cmp::Reverse(b.score));
                    scored.truncate(200);

                    if *generation_bg.lock() != job.generation {
                        continue;
                    }

                    let mut res = results_bg.lock();
                    res.matches = scored;
                    res.query = job.query;
                    res.root = job.root;
                    res.completed = true;
                    ctx.request_repaint();
                }
            })
            .expect("failed to spawn file-search-worker thread");

        FileSearchWorker {
            tx,
            results,
            generation,
            alive,
        }
    }

    pub fn search(&self, query: String, root: PathBuf) {
        let gen = {
            let mut g = self.generation.lock();
            *g += 1;
            *g
        };
        {
            let mut res = self.results.lock();
            res.completed = false;
            res.query = query.clone();
            res.root = root.clone();
        }
        let _ = self.tx.send(FileSearchJob {
            query,
            root,
            generation: gen,
        });
    }

    pub fn results(&self) -> parking_lot::MutexGuard<'_, FileSearchResults> {
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

impl Drop for FileSearchWorker {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.tx.send(FileSearchJob {
            query: String::new(),
            root: PathBuf::new(),
            generation: u64::MAX,
        });
    }
}

fn collect_files_recursive(
    root: &Path,
    out: &mut Vec<(String, PathBuf, bool)>,
    max_depth: usize,
    generation: &Mutex<u64>,
    expected_gen: u64,
) {
    if max_depth == 0 {
        return;
    }
    let rd = match std::fs::read_dir(root) {
        Ok(rd) => rd,
        Err(_) => return,
    };
    for e in rd.flatten() {
        if *generation.lock() != expected_gen {
            return;
        }
        let p = e.path();
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = p.is_dir();
        if is_dir {
            collect_files_recursive(&p, out, max_depth - 1, generation, expected_gen);
        } else {
            out.push((name, p, false));
        }
    }
}
