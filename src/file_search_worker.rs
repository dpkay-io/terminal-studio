use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

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
    thread_running: bool,
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

        let thread_running = match thread::Builder::new()
            .name("file-search-worker".into())
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
            }) {
            Ok(_) => true,
            Err(e) => {
                log::error!("failed to spawn file-search-worker thread: {e}");
                false
            }
        };

        FileSearchWorker {
            tx,
            results,
            generation,
            alive,
            thread_running,
        }
    }

    pub fn search(&self, query: String, root: PathBuf) {
        if !self.thread_running {
            log::warn!("file-search-worker thread not running; search ignored");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::thread;
    use std::time::Duration;

    fn unique_test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("ts_fsw_{}_{}", name, std::process::id()))
    }

    #[test]
    fn test_spawn_and_drop() {
        let worker = FileSearchWorker::spawn(egui::Context::default());
        drop(worker);
        // No panic — success
    }

    #[test]
    fn test_cancel_clears_results() {
        let worker = FileSearchWorker::spawn(egui::Context::default());
        worker.cancel();
        let res = worker.results();
        assert!(res.matches.is_empty());
        assert!(res.completed);
    }

    #[test]
    fn test_search_finds_files() {
        let dir = unique_test_dir("finds_files");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("myfile.txt"), "hello").unwrap();
        fs::write(dir.join("other.rs"), "world").unwrap();

        let worker = FileSearchWorker::spawn(egui::Context::default());
        worker.search("myfile".into(), dir.clone());

        for _ in 0..40 {
            thread::sleep(Duration::from_millis(50));
            if worker.results().completed {
                break;
            }
        }

        let res = worker.results();
        assert!(res.completed);
        assert!(
            res.matches.iter().any(|m| m.name == "myfile.txt"),
            "expected to find myfile.txt in matches: {:?}",
            res.matches.iter().map(|m| &m.name).collect::<Vec<_>>()
        );

        drop(res);
        drop(worker);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_search_hidden_files_filtered() {
        let dir = unique_test_dir("hidden_filter");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".hidden_file"), "secret").unwrap();
        fs::write(dir.join("visible_file"), "public").unwrap();

        let worker = FileSearchWorker::spawn(egui::Context::default());
        worker.search("file".into(), dir.clone());

        for _ in 0..40 {
            thread::sleep(Duration::from_millis(50));
            if worker.results().completed {
                break;
            }
        }

        let res = worker.results();
        assert!(res.completed);
        assert!(
            !res.matches.iter().any(|m| m.name.starts_with('.')),
            "hidden files should be filtered out, but found: {:?}",
            res.matches.iter().map(|m| &m.name).collect::<Vec<_>>()
        );
        assert!(
            res.matches.iter().any(|m| m.name == "visible_file"),
            "expected visible_file in matches"
        );

        drop(res);
        drop(worker);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_collect_files_recursive_basic() {
        let dir = unique_test_dir("collect_basic");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.txt"), "").unwrap();
        fs::write(dir.join("b.rs"), "").unwrap();
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("c.md"), "").unwrap();

        let gen = Arc::new(Mutex::new(1u64));
        let mut out = Vec::new();
        collect_files_recursive(&dir, &mut out, 5, &gen, 1);

        let names: Vec<&str> = out.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"a.txt"), "missing a.txt in {:?}", names);
        assert!(names.contains(&"b.rs"), "missing b.rs in {:?}", names);
        assert!(names.contains(&"c.md"), "missing c.md in {:?}", names);
        assert_eq!(out.len(), 3);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_collect_files_recursive_max_depth() {
        let dir = unique_test_dir("max_depth");
        let _ = fs::remove_dir_all(&dir);

        // Create nested dirs: d1/d2/d3/d4/d5/d6/deep.txt
        let mut nested = dir.clone();
        for i in 1..=6 {
            nested = nested.join(format!("d{}", i));
        }
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("deep.txt"), "").unwrap();

        // Also create a file at depth 1
        fs::write(dir.join("shallow.txt"), "").unwrap();

        let gen = Arc::new(Mutex::new(1u64));
        let mut out = Vec::new();
        collect_files_recursive(&dir, &mut out, 5, &gen, 1);

        let names: Vec<&str> = out.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(
            names.contains(&"shallow.txt"),
            "shallow.txt should be found"
        );
        // deep.txt is at depth 7 (d1/d2/d3/d4/d5/d6/deep.txt), max_depth=5 stops recursion
        assert!(
            !names.contains(&"deep.txt"),
            "deep.txt should NOT be found at depth > 5"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_collect_files_recursive_skips_dotfiles() {
        let dir = unique_test_dir("dotfiles");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".hidden"), "").unwrap();
        let hidden_dir = dir.join(".hidden_dir");
        fs::create_dir_all(&hidden_dir).unwrap();
        fs::write(hidden_dir.join("inside.txt"), "").unwrap();
        fs::write(dir.join("normal.txt"), "").unwrap();

        let gen = Arc::new(Mutex::new(1u64));
        let mut out = Vec::new();
        collect_files_recursive(&dir, &mut out, 5, &gen, 1);

        let names: Vec<&str> = out.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["normal.txt"],
            "only normal.txt expected, got {:?}",
            names
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_collect_files_recursive_empty_dir() {
        let dir = unique_test_dir("empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let gen = Arc::new(Mutex::new(1u64));
        let mut out = Vec::new();
        collect_files_recursive(&dir, &mut out, 5, &gen, 1);

        assert!(out.is_empty(), "empty dir should produce no results");

        let _ = fs::remove_dir_all(&dir);
    }
}
