pub mod foreground;
pub mod reader;

use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use egui::Context;
use parking_lot::RwLock;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::terminal::Session;

/// On each prompt display, PowerShell emits an OSC 7 CWD notification followed by
/// the standard "PS path> " prompt text. `-NoExit` keeps the shell alive after init.
#[cfg(target_os = "windows")]
const SHELL: &str = "powershell.exe";
#[cfg(target_os = "windows")]
const SHELL_INIT_ARGS: &[&str] = &[
    "-NoExit",
    "-Command",
    concat!(
        "function prompt {",
        r#"  "$([char]27)]7;file:///$($PWD.Path.Replace('\','/'))`a" +"#,
        r#"  "PS $($PWD.Path)> ""#,
        "}"
    ),
];

#[cfg(not(target_os = "windows"))]
const SHELL: &str = "bash";

type SpawnResult = (
    u32,
    Arc<RwLock<Session>>,
    Box<dyn portable_pty::MasterPty + Send>,
    Box<dyn Write + Send>,
    u32,
    Arc<AtomicBool>,
);

pub struct SessionManager {
    ctx: Context,
    next_id: u32,
}

impl SessionManager {
    pub fn new(ctx: Context) -> Self {
        SessionManager { ctx, next_id: 1 }
    }

    /// Spawn a new shell session. Returns (session_id, Arc<RwLock<Session>>, master, writer, shell_pid, alive).
    pub fn spawn(
        &mut self,
        cols: u16,
        rows: u16,
        cwd: Option<std::path::PathBuf>,
    ) -> anyhow::Result<SpawnResult> {
        let id = self.next_id;
        self.next_id += 1;

        let pty_system = native_pty_system();
        let pty_pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = self.build_command();
        cmd.env("TERM", "xterm-256color");
        if let Some(ref dir) = cwd {
            cmd.cwd(dir);
        }

        let child = pty_pair.slave.spawn_command(cmd)?;
        let shell_pid = child.process_id().unwrap_or(u32::MAX);
        drop(child);

        let writer = pty_pair.master.take_writer()?;
        let reader = pty_pair.master.try_clone_reader()?;

        let session = Arc::new(RwLock::new(Session::new(id, cols, rows, cwd)));

        let alive = Arc::new(AtomicBool::new(true));
        let alive_for_thread = Arc::clone(&alive);

        // Spawn the dedicated reader thread
        let session_clone = Arc::clone(&session);
        let ctx_clone = self.ctx.clone();
        thread::Builder::new()
            .name(format!("pty-reader-{}", id))
            .spawn(move || {
                reader::reader_thread(reader, session_clone, ctx_clone, alive_for_thread)
            })?;

        Ok((id, session, pty_pair.master, writer, shell_pid, alive))
    }

    pub fn write(writer: &mut Box<dyn Write + Send>, data: &[u8]) {
        let _ = writer.write_all(data);
    }

    pub fn resize(master: &(dyn portable_pty::MasterPty + Send), cols: u16, rows: u16) {
        let _ = master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    #[cfg(target_os = "windows")]
    fn build_command(&self) -> CommandBuilder {
        let mut cmd = CommandBuilder::new(SHELL);
        cmd.args(SHELL_INIT_ARGS);
        cmd
    }

    #[cfg(not(target_os = "windows"))]
    fn build_command(&self) -> CommandBuilder {
        let mut cmd = CommandBuilder::new(SHELL);
        // Set PROMPT_COMMAND to emit OSC 7 before every prompt
        cmd.env(
            "PROMPT_COMMAND",
            r#"printf '\e]7;file://%s%s\a' "$HOSTNAME" "$PWD""#,
        );
        cmd
    }
}
