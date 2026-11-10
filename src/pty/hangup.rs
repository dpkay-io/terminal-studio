//! Ending a session's shell when its tab closes.
//!
//! The PTY master has several open handles (the session entry, the reader
//! thread's clone, the writer), and the reader blocks in `read()` until the
//! shell exits. Dropping the session entry alone therefore never hangs up the
//! terminal: the shell, its three threads and the scrollback would live until
//! the app quits. Closing a tab sends the shell SIGHUP (TerminateProcess on
//! Windows), as closing a terminal window does; the reader then sees EOF and
//! everything unwinds.
//!
//! On Unix the signal goes to a pid, so the reaper thread disarms the handle
//! right after it has waited on the shell: closing a tab whose shell already
//! exited signals nothing. The only remaining window is between `wait()`
//! returning and `disarm()` on the reaper thread, while the OS hands out pids
//! sequentially. Windows uses a process handle, which cannot be reused.

use std::sync::Arc;

use parking_lot::Mutex;
use portable_pty::ChildKiller;

type Killer = Box<dyn ChildKiller + Send + Sync>;

/// Shared between a session entry (which hangs up) and its reaper thread
/// (which disarms it once the shell has exited).
#[derive(Clone)]
pub struct ShellHangup {
    killer: Arc<Mutex<Option<Killer>>>,
}

impl ShellHangup {
    pub fn new(killer: Killer) -> Self {
        Self {
            killer: Arc::new(Mutex::new(Some(killer))),
        }
    }

    /// Hang up the shell if it is still running. Later calls do nothing.
    pub fn hang_up(&self) {
        if let Some(mut killer) = self.killer.lock().take() {
            if let Err(e) = killer.kill() {
                log::debug!("hanging up shell failed: {e}");
            }
        }
    }

    /// Called by the reaper after the shell has exited and been reaped.
    pub fn disarm(&self) {
        self.killer.lock().take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Clone)]
    struct CountingKiller(Arc<AtomicUsize>);

    impl ChildKiller for CountingKiller {
        fn kill(&mut self) -> std::io::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(self.clone())
        }
    }

    fn counting() -> (ShellHangup, Arc<AtomicUsize>) {
        let kills = Arc::new(AtomicUsize::new(0));
        (
            ShellHangup::new(Box::new(CountingKiller(kills.clone()))),
            kills,
        )
    }

    #[test]
    fn hang_up_kills_once() {
        let (hangup, kills) = counting();
        hangup.hang_up();
        hangup.hang_up();
        assert_eq!(kills.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn disarmed_handle_never_kills() {
        let (hangup, kills) = counting();
        hangup.clone().disarm();
        hangup.hang_up();
        assert_eq!(kills.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn clones_share_state() {
        let (hangup, kills) = counting();
        let reaper_side = hangup.clone();
        hangup.hang_up();
        reaper_side.hang_up();
        assert_eq!(kills.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failing_kill_is_logged_not_panicking() {
        #[derive(Debug)]
        struct Failing;
        impl ChildKiller for Failing {
            fn kill(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::other("gone"))
            }
            fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
                Box::new(Failing)
            }
        }
        ShellHangup::new(Box::new(Failing)).hang_up();
    }

    /// A real shell in a real PTY exits when hung up, even though another
    /// handle to the PTY master is still open (as the reader thread's is).
    #[cfg(unix)]
    #[test]
    fn hang_up_ends_a_real_pty_shell() {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};
        use std::time::{Duration, Instant};

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "sleep 30"]);
        let mut child = pair.slave.spawn_command(cmd).expect("spawn");
        let _reader_handle = pair.master.try_clone_reader().expect("clone reader");
        let hangup = ShellHangup::new(child.clone_killer());

        hangup.hang_up();

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if child.try_wait().expect("try_wait").is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "shell still running after hang-up"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
