use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::Context;
use parking_lot::RwLock;
use vte::Parser;

use crate::terminal::{Session, performer::Performer};

/// Spawned as a dedicated OS thread per PTY session.
/// Blocks on PTY reads, feeds bytes through the VTE parser, then requests a repaint.
pub fn reader_thread(
    mut reader: Box<dyn Read + Send>,
    session: Arc<RwLock<Session>>,
    ctx: Context,
    alive: Arc<AtomicBool>,
) {
    let mut parser = Parser::new();
    let mut buf = [0u8; 8192];

    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,         // EOF — shell exited
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

        // Batch repaints: schedule one repaint 8 ms from now rather than painting
        // immediately. Multiple reads within that window are coalesced, preventing
        // mid-update render artifacts when escape sequences span two reads.
        ctx.request_repaint_after(std::time::Duration::from_millis(8));
    }

    alive.store(false, Ordering::SeqCst);
    log::info!("Session {} reader thread exiting", session.read().id);
}
