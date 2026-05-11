use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use egui::Context;
use parking_lot::RwLock;
use vte::Parser;

use crate::terminal::{performer::Performer, Session};

/// Spawned as a dedicated OS thread per PTY session.
/// Blocks on PTY reads, feeds bytes through the VTE parser, then requests a repaint.
/// `is_active` controls repaint cadence: focused sessions repaint at 8 ms, background
/// sessions at 50 ms to avoid thrashing the GPU with invisible repaints.
pub fn reader_thread(
    mut reader: Box<dyn Read + Send>,
    session: Arc<RwLock<Session>>,
    ctx: Context,
    alive: Arc<AtomicBool>,
    is_active: Arc<AtomicBool>,
) {
    let mut parser = Parser::new();
    let mut buf = [0u8; 65536];

    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break, // EOF — shell exited
            Ok(n) => n,
            Err(e) => {
                log::debug!("PTY read error: {}", e);
                break;
            }
        };

        {
            let mut session_guard = session.write();
            log::trace!(
                "PTY[{}] IN  {} bytes: {:?}",
                session_guard.id,
                n,
                String::from_utf8_lossy(&buf[..n])
            );
            let mut performer = Performer::new(&mut session_guard);
            for byte in &buf[..n] {
                parser.advance(&mut performer, *byte);
            }
        }

        // Active pane: repaint quickly (8 ms) for responsive output.
        // Background pane: throttle to 50 ms — data is still processed, just
        // not rendered immediately, reducing unnecessary GPU work.
        let repaint_ms = if is_active.load(Ordering::Relaxed) { 8 } else { 50 };
        ctx.request_repaint_after(std::time::Duration::from_millis(repaint_ms));
    }

    alive.store(false, Ordering::SeqCst);
    log::info!("Session {} reader thread exiting", session.read().id);
}
