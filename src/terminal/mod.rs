mod tests;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
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
    bell: Arc<AtomicBool>,
}

impl EventProxy {
    pub fn new(
        id: u32,
        title: Arc<Mutex<String>>,
        ctx: Context,
        pty_tx: mpsc::SyncSender<Vec<u8>>,
        bell: Arc<AtomicBool>,
    ) -> Self {
        EventProxy {
            id,
            title,
            ctx,
            pty_tx,
            bell,
        }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(s) => {
                *self.title.lock() = s;
            }
            #[allow(clippy::collapsible_match)]
            Event::PtyWrite(s) => {
                if self.pty_tx.try_send(s.into_bytes()).is_err() {
                    log::warn!("pty_tx full — PtyWrite response dropped for session {}", self.id);
                }
            }
            Event::Bell => {
                self.bell.store(true, Ordering::Relaxed);
                self.ctx.request_repaint();
            }
            Event::MouseCursorDirty | Event::CursorBlinkingChange => {
                self.ctx
                    .request_repaint_after(std::time::Duration::from_millis(50));
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
    pub bell: Arc<AtomicBool>,
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
        let bell = Arc::new(AtomicBool::new(false));
        let proxy = EventProxy::new(id, title.clone(), ctx, pty_tx, bell.clone());
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
            bell,
        }
    }

    /// For use in unit tests — uses a dropped channel (PtyWrite silently ignored)
    /// and a headless Context.
    #[cfg(test)]
    pub fn new_for_test(id: u32, cols: u16, rows: u16) -> Self {
        let title = Arc::new(Mutex::new(format!("Session {}", id)));
        let bell = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::sync_channel(64);
        let ctx = Context::default();
        let proxy = EventProxy::new(id, title.clone(), ctx, tx, bell.clone());
        let config = Config {
            scrolling_history: 100_000,
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
            bell,
        }
    }

    pub fn set_title(&self, title: String) {
        *self.title.lock() = title;
    }

    pub fn title(&self) -> String {
        self.title.lock().clone()
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }
        let size = TermSize {
            cols: cols as usize,
            lines: rows as usize,
        };
        self.term.resize(size);
    }
}
