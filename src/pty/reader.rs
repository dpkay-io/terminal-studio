use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
use base64::Engine;
use egui::Context;
use parking_lot::RwLock;
use vte::Perform;

use crate::terminal::Session;

/// Minimal VTE 0.13 performer for OSC 7 (CWD tracking) and OSC 52
/// (clipboard set) tee — all other sequences are no-ops.  Runs on the raw
/// byte stream before alacritty parses it.
struct CwdPerformer {
    new_cwd: Option<PathBuf>,
    new_prompt_ready: bool,
    /// Base64-decoded clipboard text from OSC 52.
    clipboard_text: Option<String>,
}

impl CwdPerformer {
    fn new() -> Self {
        CwdPerformer {
            new_cwd: None,
            new_prompt_ready: false,
            clipboard_text: None,
        }
    }
}

impl Perform for CwdPerformer {
    fn print(&mut self, _: char) {}
    fn execute(&mut self, _: u8) {}
    fn csi_dispatch(&mut self, _: &vte::Params, _: &[u8], _: bool, _: char) {}
    fn esc_dispatch(&mut self, _: &[u8], _: bool, _: u8) {}
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        match params.first().copied() {
            Some(b"7") => self.handle_osc7(params),
            Some(b"52") => self.handle_osc52(params),
            _ => {}
        }
    }
}

impl CwdPerformer {
    fn handle_osc7(&mut self, params: &[&[u8]]) {
        let Some(&uri_bytes) = params.get(1) else {
            return;
        };
        let Ok(uri) = std::str::from_utf8(uri_bytes) else {
            return;
        };

        let path_str = if uri.starts_with("file:///") {
            uri.trim_start_matches("file:///")
        } else if uri.starts_with("file://") {
            let rest = uri.trim_start_matches("file://");
            rest.find('/').map(|i| &rest[i..]).unwrap_or(rest)
        } else {
            return;
        };

        #[cfg(target_os = "windows")]
        let path_str = path_str.replace('/', "\\");

        self.new_cwd = Some(PathBuf::from(path_str));
        self.new_prompt_ready = true;
    }

    fn handle_osc52(&mut self, params: &[&[u8]]) {
        // OSC 52 format: \x1b]52;<selection>;<base64_data>\x07
        // params[0] = b"52", params[1] = selection target (e.g. b"c"), params[2] = base64 data
        // A query (empty data or "?") is ignored for now.
        let Some(&data_bytes) = params.get(2) else {
            return;
        };
        // Empty data or "?" means query — ignore
        if data_bytes.is_empty() || data_bytes == b"?" {
            return;
        }
        let engine = base64::engine::general_purpose::STANDARD;
        if let Ok(decoded) = engine.decode(data_bytes) {
            if let Ok(text) = String::from_utf8(decoded) {
                self.clipboard_text = Some(text);
            }
        }
    }
}

/// Spawned as a dedicated OS thread per PTY session.
/// Feeds bytes through the alacritty Processor (main emulator) and a tee
/// VTE parser (OSC-7 CWD tracking). Releases the session write lock between
/// 4 KB chunks so the UI thread can render between bursts.
pub fn reader_thread(
    mut reader: Box<dyn Read + Send>,
    session: Arc<RwLock<Session>>,
    ctx: Context,
    alive: Arc<AtomicBool>,
    is_active: Arc<AtomicBool>,
) {
    let mut processor: Processor<StdSyncHandler> = Processor::new();
    let mut cwd_parser = vte::Parser::new();
    let mut buf = [0u8; 65536];

    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                log::debug!("PTY read error: {}", e);
                break;
            }
        };

        // ── Tee: scan for OSC 7 / OSC 52 without holding session lock ────────
        let mut cwd_perf = CwdPerformer::new();
        for b in &buf[..n] {
            cwd_parser.advance(&mut cwd_perf, *b);
        }
        if let Some(cwd) = cwd_perf.new_cwd.take() {
            let mut s = session.write();
            s.cwd = cwd;
            s.prompt_ready = cwd_perf.new_prompt_ready;
        }
        // OSC 52: set system clipboard
        if let Some(text) = cwd_perf.clipboard_text.take() {
            if let Ok(mut clip) = arboard::Clipboard::new() {
                let _ = clip.set_text(text);
            }
        }

        // ── Feed alacritty Term in 4 KB chunks (UI can render between chunks) ─
        let mut pos = 0;
        while pos < n {
            let end = (pos + 4096).min(n);
            {
                let mut s = session.write();
                processor.advance(&mut s.term, &buf[pos..end]);
            }
            pos = end;
        }

        // ── Repaint ──────────────────────────────────────────────────────────
        let repaint_ms = if is_active.load(Ordering::Relaxed) {
            16
        } else {
            50
        };
        ctx.request_repaint_after(std::time::Duration::from_millis(repaint_ms));
    }

    alive.store(false, Ordering::SeqCst);
    log::info!("Session {} reader thread exiting", session.read().id);
}
