mod tests;

use std::path::PathBuf;
use std::sync::{mpsc, Arc};

use alacritty_terminal::{
    event::{Event, EventListener},
    grid::Dimensions,
    term::{Config, Term},
};
use egui::Context;
use parking_lot::Mutex;

// ── TermSize: adapter so we can construct Term with just cols/rows ────────────

pub struct TermSize {
    pub cols: usize,
    pub lines: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

// ── EventProxy: bridges alacritty events back to our session state ────────────

pub struct EventProxy {
    #[allow(dead_code)]
    pub id: u32,
    title: Arc<Mutex<String>>,
    pub ctx: Context,
    pub pty_tx: mpsc::SyncSender<Vec<u8>>,
}

impl EventProxy {
    pub fn new(
        id: u32,
        title: Arc<Mutex<String>>,
        ctx: Context,
        pty_tx: mpsc::SyncSender<Vec<u8>>,
    ) -> Self {
        EventProxy {
            id,
            title,
            ctx,
            pty_tx,
        }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(s) => {
                *self.title.lock() = s;
            }
            Event::PtyWrite(s) => {
                let _ = self.pty_tx.try_send(s.into_bytes());
            }
            Event::MouseCursorDirty | Event::CursorBlinkingChange => {
                // Coalesce: schedule a repaint at the next ~60 Hz tick rather
                // than firing one immediately on every event. alacritty emits
                // these frequently during fast cursor motion / blinks, and a
                // direct `request_repaint()` per event causes the UI thread
                // to wake up far more often than the human eye can see.
                self.ctx
                    .request_repaint_after(std::time::Duration::from_millis(16));
            }
            _ => {}
        }
    }
}

// ── Session ───────────────────────────────────────────────────────────────────

pub struct Session {
    pub id: u32,
    pub term: Term<EventProxy>,
    pub cwd: PathBuf,
    pub prompt_ready: bool,
    title: Arc<Mutex<String>>,
}

impl Session {
    pub fn new(
        id: u32,
        cols: u16,
        rows: u16,
        cwd: Option<PathBuf>,
        ctx: Context,
        pty_tx: mpsc::SyncSender<Vec<u8>>,
        scrollback_lines: usize,
    ) -> Self {
        let title = Arc::new(Mutex::new(format!("Session {}", id)));
        let proxy = EventProxy::new(id, title.clone(), ctx, pty_tx);
        let config = Config {
            scrolling_history: scrollback_lines,
            ..Config::default()
        };
        let size = TermSize {
            cols: cols as usize,
            lines: rows as usize,
        };
        let term = Term::new(config, &size, proxy);
        Session {
            id,
            term,
            cwd: cwd.unwrap_or_default(),
            prompt_ready: false,
            title,
        }
    }

    /// For use in unit tests — uses a dropped channel (PtyWrite silently ignored)
    /// and a headless Context.
    #[cfg(test)]
    pub fn new_for_test(id: u32, cols: u16, rows: u16) -> Self {
        let title = Arc::new(Mutex::new(format!("Session {}", id)));
        let (tx, _rx) = mpsc::sync_channel(64);
        let ctx = Context::default();
        let proxy = EventProxy::new(id, title.clone(), ctx, tx);
        let config = Config {
            scrolling_history: 10_000,
            ..Config::default()
        };
        let size = TermSize {
            cols: cols as usize,
            lines: rows as usize,
        };
        let term = Term::new(config, &size, proxy);
        Session {
            id,
            term,
            cwd: PathBuf::new(),
            prompt_ready: false,
            title,
        }
    }

    pub fn set_title(&self, title: String) {
        *self.title.lock() = title;
    }

    pub fn title(&self) -> String {
        self.title.lock().clone()
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let size = TermSize {
            cols: cols as usize,
            lines: rows as usize,
        };
        self.term.resize(size);
    }
}
