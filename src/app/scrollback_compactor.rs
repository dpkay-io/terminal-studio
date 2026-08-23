use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use alacritty_terminal::grid::{Dimensions, Row};
use alacritty_terminal::term::cell::Cell;
use parking_lot::{Mutex, RwLock};

use crate::terminal::Session;

const KEEP_LIVE_ROWS: usize = 4096;
const CHUNK_SIZE: usize = 2048;
const CHECK_INTERVAL_SECS: u64 = 10;
const ZSTD_LEVEL: i32 = 3;
// Inject compressed chunk when user scrolls within this many rows of history ceiling
const DECOMPRESS_TRIGGER_ROWS: usize = CHUNK_SIZE;

// ── Types ────────────────────────────────────────────────────────────────────

enum Job {
    Compress {
        session_id: u32,
        rows: Vec<Row<Cell>>,
        columns: usize,
    },
}

#[allow(dead_code)]
pub(super) struct CompressedChunk {
    data: Vec<u8>,
    row_count: usize,
    columns: usize,
    uncompressed_size: usize,
}

struct CompressResult {
    session_id: u32,
    chunk: CompressedChunk,
}

// ── Worker ───────────────────────────────────────────────────────────────────

pub(super) struct ScrollbackCompactor {
    tx: mpsc::Sender<Job>,
    results: Arc<Mutex<Vec<CompressResult>>>,
    alive: Arc<AtomicBool>,
    chunks: HashMap<u32, Vec<CompressedChunk>>,
    last_check: HashMap<u32, Instant>,
}

impl ScrollbackCompactor {
    pub(super) fn spawn(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let alive = Arc::new(AtomicBool::new(true));
        let results: Arc<Mutex<Vec<CompressResult>>> = Arc::new(Mutex::new(Vec::new()));

        let alive_bg = alive.clone();
        let results_bg = results.clone();
        let ctx_bg = ctx;

        std::thread::Builder::new()
            .name("scrollback-compactor".into())
            .spawn(move || {
                while alive_bg.load(Ordering::Acquire) {
                    let job = match rx.recv_timeout(std::time::Duration::from_secs(1)) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };

                    let result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match job {
                            Job::Compress {
                                session_id,
                                rows,
                                columns,
                            } => compress_rows(session_id, rows, columns),
                        }));

                    match result {
                        Ok(Some(r)) => {
                            results_bg.lock().push(r);
                            ctx_bg.request_repaint();
                        }
                        Ok(None) => {}
                        Err(_) => {
                            log::warn!("scrollback-compactor: job panicked");
                        }
                    }
                }
            })
            .ok();

        ScrollbackCompactor {
            tx,
            results,
            alive,
            chunks: HashMap::new(),
            last_check: HashMap::new(),
        }
    }

    /// Check all sessions: compress excess scrollback, decompress on demand.
    /// Called from the UI update loop.
    pub(super) fn tick(&mut self, sessions: &[(u32, Arc<RwLock<Session>>)]) {
        self.poll_results();

        let now = Instant::now();
        let interval = std::time::Duration::from_secs(CHECK_INTERVAL_SECS);

        for &(sid, ref session_arc) in sessions {
            // Decompression: if user has scrolled near the top of live history
            // and we hold compressed chunks, inject one chunk back.
            if self.chunk_count(sid) > 0 {
                let session = session_arc.read();
                let grid = session.term.grid();
                let display_offset = grid.display_offset();
                let history = grid.history_size();
                let near_top = display_offset + DECOMPRESS_TRIGGER_ROWS >= history;
                drop(session);

                if near_top {
                    let mut session = session_arc.write();
                    self.decompress_into(sid, &mut session);
                }
            }

            // Compression: check periodically if history exceeds threshold
            let last = self.last_check.get(&sid).copied();
            if last.is_some_and(|t| now.duration_since(t) < interval) {
                continue;
            }
            self.last_check.insert(sid, now);

            let session = session_arc.read();
            let grid = session.term.grid();
            let history = grid.history_size();

            if history <= KEEP_LIVE_ROWS {
                continue;
            }

            let harvestable = history - KEEP_LIVE_ROWS;
            let to_harvest = (harvestable / CHUNK_SIZE) * CHUNK_SIZE;
            if to_harvest == 0 {
                continue;
            }

            drop(session);

            let mut session = session_arc.write();
            let grid = session.term.grid_mut();
            let columns = grid.columns();
            let rows = grid.harvest_oldest_scrollback(to_harvest);
            drop(session);

            for chunk_rows in rows_to_chunks(rows) {
                let _ = self.tx.send(Job::Compress {
                    session_id: sid,
                    rows: chunk_rows,
                    columns,
                });
            }
        }
    }

    fn poll_results(&mut self) {
        let batch = std::mem::take(&mut *self.results.lock());
        for r in batch {
            self.chunks.entry(r.session_id).or_default().push(r.chunk);
        }
    }

    /// Synchronously decompress and inject the oldest chunk back into a session.
    /// Returns true if a chunk was injected (caller should repaint).
    pub(super) fn decompress_into(&mut self, session_id: u32, session: &mut Session) -> bool {
        let chunks = match self.chunks.get_mut(&session_id) {
            Some(c) if !c.is_empty() => c,
            _ => return false,
        };

        // ponytail: inject from the front (oldest first)
        let chunk = chunks.remove(0);

        match decompress_chunk(&chunk) {
            Ok(rows) => {
                session.term.grid_mut().inject_scrollback(rows);
                true
            }
            Err(e) => {
                log::warn!(
                    "scrollback-compactor: decompress failed for session {}: {}",
                    session_id,
                    e
                );
                false
            }
        }
    }

    /// Number of compressed chunks held for a session.
    pub(super) fn chunk_count(&self, session_id: u32) -> usize {
        self.chunks.get(&session_id).map_or(0, |c| c.len())
    }

    /// Total compressed rows for a session.
    #[allow(dead_code)]
    pub(super) fn compressed_rows(&self, session_id: u32) -> usize {
        self.chunks
            .get(&session_id)
            .map_or(0, |cs| cs.iter().map(|c| c.row_count).sum())
    }

    /// Total memory saved across all sessions (uncompressed - compressed).
    #[allow(dead_code)]
    pub(super) fn total_bytes_saved(&self) -> usize {
        self.chunks
            .values()
            .flat_map(|cs| cs.iter())
            .map(|c| c.uncompressed_size.saturating_sub(c.data.len()))
            .sum()
    }

    /// Remove all compressed data for a session (on session close).
    pub(super) fn remove_session(&mut self, session_id: u32) {
        self.chunks.remove(&session_id);
        self.last_check.remove(&session_id);
    }

    pub(super) fn shutdown(&mut self) {
        self.alive.store(false, Ordering::Release);
    }
}

impl Drop for ScrollbackCompactor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn rows_to_chunks(rows: Vec<Row<Cell>>) -> Vec<Vec<Row<Cell>>> {
    let mut out = Vec::new();
    let mut iter = rows.into_iter();
    loop {
        let chunk: Vec<Row<Cell>> = iter.by_ref().take(CHUNK_SIZE).collect();
        if chunk.is_empty() {
            break;
        }
        out.push(chunk);
    }
    out
}

fn compress_rows(session_id: u32, rows: Vec<Row<Cell>>, columns: usize) -> Option<CompressResult> {
    let json = match serde_json::to_vec(&rows) {
        Ok(j) => j,
        Err(e) => {
            log::warn!(
                "scrollback-compactor: serialize failed for session {}: {}",
                session_id,
                e
            );
            return None;
        }
    };

    let uncompressed_size = json.len();
    let row_count = rows.len();

    let compressed = match zstd::encode_all(json.as_slice(), ZSTD_LEVEL) {
        Ok(c) => c,
        Err(e) => {
            log::warn!(
                "scrollback-compactor: zstd encode failed for session {}: {}",
                session_id,
                e
            );
            return None;
        }
    };

    Some(CompressResult {
        session_id,
        chunk: CompressedChunk {
            data: compressed,
            row_count,
            columns,
            uncompressed_size,
        },
    })
}

fn decompress_chunk(chunk: &CompressedChunk) -> Result<Vec<Row<Cell>>, String> {
    let json =
        zstd::decode_all(chunk.data.as_slice()).map_err(|e| format!("zstd decode: {}", e))?;
    let rows: Vec<Row<Cell>> =
        serde_json::from_slice(&json).map_err(|e| format!("deserialize: {}", e))?;
    Ok(rows)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::grid::Row;
    use alacritty_terminal::index::Column;

    fn make_default_rows(count: usize, cols: usize) -> Vec<Row<Cell>> {
        (0..count)
            .map(|_| {
                let mut inner = vec![Cell::default(); cols];
                inner[0].c = 'A';
                Row::from_vec(inner, cols)
            })
            .collect()
    }

    #[test]
    fn rows_to_chunks_exact() {
        let rows = make_default_rows(CHUNK_SIZE * 3, 80);
        let chunks = rows_to_chunks(rows);
        assert_eq!(chunks.len(), 3);
        assert!(chunks.iter().all(|c| c.len() == CHUNK_SIZE));
    }

    #[test]
    fn rows_to_chunks_remainder() {
        let rows = make_default_rows(CHUNK_SIZE + 500, 80);
        let chunks = rows_to_chunks(rows);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), CHUNK_SIZE);
        assert_eq!(chunks[1].len(), 500);
    }

    #[test]
    fn rows_to_chunks_empty() {
        let chunks = rows_to_chunks(Vec::new());
        assert!(chunks.is_empty());
    }

    #[test]
    fn compress_decompress_roundtrip() {
        let rows = make_default_rows(100, 80);
        let result = compress_rows(1, rows.clone(), 80).expect("compress should succeed");
        assert_eq!(result.session_id, 1);
        assert_eq!(result.chunk.row_count, 100);
        assert_eq!(result.chunk.columns, 80);
        assert!(result.chunk.data.len() < result.chunk.uncompressed_size);

        let restored = decompress_chunk(&result.chunk).expect("decompress should succeed");
        assert_eq!(restored.len(), 100);
        assert_eq!(restored[0][Column(0)].c, 'A');
    }

    #[test]
    fn compress_empty_rows() {
        let result = compress_rows(1, Vec::new(), 80).expect("should handle empty");
        assert_eq!(result.chunk.row_count, 0);
        let restored = decompress_chunk(&result.chunk).expect("decompress empty");
        assert!(restored.is_empty());
    }

    #[test]
    fn compressed_chunk_saves_memory() {
        let rows = make_default_rows(CHUNK_SIZE, 200);
        let result = compress_rows(1, rows, 200).expect("compress");
        assert!(
            result.chunk.data.len() < result.chunk.uncompressed_size / 2,
            "compressed {} vs uncompressed {}",
            result.chunk.data.len(),
            result.chunk.uncompressed_size
        );
    }

    #[test]
    fn roundtrip_preserves_unicode_and_flags() {
        use alacritty_terminal::term::cell::Flags;
        use alacritty_terminal::vte::ansi::Color;

        let mut inner = vec![Cell::default(); 80];
        inner[0].c = '日';
        inner[0].flags = Flags::WIDE_CHAR;
        inner[1].c = ' ';
        inner[1].flags = Flags::WIDE_CHAR_SPACER;
        inner[2].c = 'é';
        inner[3].c = '🦀';
        inner[3].fg = Color::Indexed(196);
        inner[3].bg = Color::Spec(alacritty_terminal::vte::ansi::Rgb {
            r: 0,
            g: 255,
            b: 128,
        });

        let rows = vec![Row::from_vec(inner, 80)];
        let result = compress_rows(1, rows, 80).expect("compress");
        let restored = decompress_chunk(&result.chunk).expect("decompress");

        assert_eq!(restored[0][Column(0)].c, '日');
        assert!(restored[0][Column(0)].flags.contains(Flags::WIDE_CHAR));
        assert!(restored[0][Column(1)]
            .flags
            .contains(Flags::WIDE_CHAR_SPACER));
        assert_eq!(restored[0][Column(2)].c, 'é');
        assert_eq!(restored[0][Column(3)].c, '🦀');
        assert_eq!(restored[0][Column(3)].fg, Color::Indexed(196));
    }

    #[test]
    fn rows_to_chunks_single_row() {
        let rows = make_default_rows(1, 80);
        let chunks = rows_to_chunks(rows);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 1);
    }

    #[test]
    fn multiple_compress_results_accumulate() {
        let r1 = compress_rows(1, make_default_rows(100, 80), 80).unwrap();
        let r2 = compress_rows(1, make_default_rows(200, 80), 80).unwrap();
        let r3 = compress_rows(2, make_default_rows(50, 80), 80).unwrap();

        let mut chunks: HashMap<u32, Vec<CompressedChunk>> = HashMap::new();
        chunks.entry(r1.session_id).or_default().push(r1.chunk);
        chunks.entry(r2.session_id).or_default().push(r2.chunk);
        chunks.entry(r3.session_id).or_default().push(r3.chunk);

        assert_eq!(chunks[&1].len(), 2);
        assert_eq!(chunks[&2].len(), 1);
        assert_eq!(chunks[&1][0].row_count, 100);
        assert_eq!(chunks[&1][1].row_count, 200);
    }

    #[test]
    fn decompress_invalid_data_returns_error() {
        let bad_chunk = CompressedChunk {
            data: vec![0xFF, 0xFE, 0xFD],
            row_count: 10,
            columns: 80,
            uncompressed_size: 1000,
        };
        assert!(decompress_chunk(&bad_chunk).is_err());
    }
}
