pub mod foreground;
pub mod foreground_worker;
pub mod reader;

use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use egui::Context;
use parking_lot::RwLock;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::terminal::Session;

// ── Shell kind ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum ShellKind {
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    PowerShell, // powershell.exe — Windows
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Pwsh, // pwsh.exe — Windows (PowerShell Core)
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Cmd, // cmd.exe — Windows
    Bash, // bash
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Zsh, // zsh — Unix
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Fish, // fish — Unix
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Sh, // sh — Unix
}

impl ShellKind {
    pub fn display_name(&self) -> &str {
        match self {
            ShellKind::PowerShell => "PowerShell",
            ShellKind::Pwsh => "PowerShell Core (pwsh)",
            ShellKind::Cmd => "Command Prompt",
            ShellKind::Bash => "Bash",
            ShellKind::Zsh => "Zsh",
            ShellKind::Fish => "Fish",
            ShellKind::Sh => "Sh",
        }
    }

    fn executable(&self) -> &str {
        match self {
            ShellKind::PowerShell => "powershell.exe",
            ShellKind::Pwsh => {
                if cfg!(target_os = "windows") {
                    "pwsh.exe"
                } else {
                    "pwsh"
                }
            }
            ShellKind::Cmd => "cmd.exe",
            ShellKind::Bash => "bash",
            ShellKind::Zsh => "zsh",
            ShellKind::Fish => "fish",
            ShellKind::Sh => "sh",
        }
    }
}

fn find_in_path(name: &str) -> bool {
    let sep = if cfg!(target_os = "windows") {
        ';'
    } else {
        ':'
    };
    std::env::var("PATH").is_ok_and(|path_var| {
        path_var
            .split(sep)
            .any(|dir| std::path::Path::new(dir).join(name).exists())
    })
}

/// Returns the shells available on this system, in display order.
#[cfg(target_os = "windows")]
pub fn available_shells() -> Vec<ShellKind> {
    let mut shells = vec![ShellKind::PowerShell]; // always present on Windows
    if find_in_path("pwsh.exe") {
        shells.push(ShellKind::Pwsh);
    }
    shells.push(ShellKind::Cmd); // always present on Windows
    if find_in_path("bash.exe") {
        shells.push(ShellKind::Bash);
    }
    shells
}

#[cfg(not(target_os = "windows"))]
pub fn available_shells() -> Vec<ShellKind> {
    [
        ("bash", ShellKind::Bash),
        ("zsh", ShellKind::Zsh),
        ("fish", ShellKind::Fish),
        ("sh", ShellKind::Sh),
    ]
    .into_iter()
    .filter(|(name, _)| find_in_path(name))
    .map(|(_, kind)| kind)
    .collect()
}

/// Returns the most appropriate default shell for this system.
#[cfg(target_os = "windows")]
pub fn default_shell() -> ShellKind {
    ShellKind::PowerShell
}

#[cfg(not(target_os = "windows"))]
pub fn default_shell() -> ShellKind {
    if let Ok(shell) = std::env::var("SHELL") {
        let name = std::path::Path::new(&shell)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        match name {
            "bash" => return ShellKind::Bash,
            "zsh" => return ShellKind::Zsh,
            "fish" => return ShellKind::Fish,
            "sh" => return ShellKind::Sh,
            _ => {}
        }
    }
    if find_in_path("bash") {
        ShellKind::Bash
    } else {
        ShellKind::Sh
    }
}

// ── Type alias ────────────────────────────────────────────────────────────────

type SpawnResult = (
    u32,
    Arc<RwLock<Session>>,
    Box<dyn portable_pty::MasterPty + Send>,
    mpsc::SyncSender<Vec<u8>>,
    u32,
    Arc<AtomicBool>,
    Arc<AtomicBool>, // is_active: true when this session is the focused pane
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
        shell: &ShellKind,
        scrollback_lines: usize,
    ) -> anyhow::Result<SpawnResult> {
        let id = self.next_id;
        self.next_id += 1;

        // ConPTY rejects zero dimensions with E_INVALIDARG. Pane `last_size`
        // is initialized to (0, 0) as a "needs layout" sentinel, and several
        // call sites can forward that here before the first frame resizes
        // the pane. Fall back to standard 80x24 — the resize debounce will
        // bring it to the real geometry on the next layout pass.
        let cols = if cols == 0 { 80 } else { cols };
        let rows = if rows == 0 { 24 } else { rows };

        let pty_system = native_pty_system();
        let pty_pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut cmd = self.build_command(shell);
        cmd.env("TERM", "xterm-256color");
        if let Some(ref dir) = cwd {
            cmd.cwd(dir);
        }

        let child = pty_pair.slave.spawn_command(cmd)?;
        let shell_pid = child.process_id().unwrap_or(u32::MAX);
        drop(child);

        let reader = pty_pair.master.try_clone_reader()?;

        // Create PTY writer channel
        let (pty_tx, pty_rx) = mpsc::sync_channel::<Vec<u8>>(64);

        // Spawn PTY writer thread
        let mut pty_writer = pty_pair.master.take_writer()?;
        thread::Builder::new()
            .name(format!("pty-writer-{}", id))
            .spawn(move || {
                use std::io::Write;
                while let Ok(data) = pty_rx.recv() {
                    if pty_writer.write_all(&data).is_err() {
                        break;
                    }
                }
            })?;

        // Create session with the context and pty_tx
        let session = Arc::new(RwLock::new(Session::new(
            id,
            cols,
            rows,
            cwd,
            self.ctx.clone(),
            pty_tx.clone(),
            scrollback_lines,
        )));

        let alive = Arc::new(AtomicBool::new(true));
        let alive_for_thread = Arc::clone(&alive);

        let is_active = Arc::new(AtomicBool::new(false));
        let is_active_for_thread = Arc::clone(&is_active);

        // Spawn the dedicated reader thread
        let session_clone = Arc::clone(&session);
        let ctx_clone = self.ctx.clone();
        thread::Builder::new()
            .name(format!("pty-reader-{}", id))
            .spawn(move || {
                reader::reader_thread(
                    reader,
                    session_clone,
                    ctx_clone,
                    alive_for_thread,
                    is_active_for_thread,
                )
            })?;

        Ok((
            id,
            session,
            pty_pair.master,
            pty_tx,
            shell_pid,
            alive,
            is_active,
        ))
    }

    #[allow(clippy::borrowed_box)]
    pub fn resize(master: &Box<dyn portable_pty::MasterPty + Send>, cols: u16, rows: u16) {
        let _ = master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    fn build_command(&self, shell: &ShellKind) -> CommandBuilder {
        match shell {
            ShellKind::PowerShell | ShellKind::Pwsh => {
                let mut cmd = CommandBuilder::new(shell.executable());
                // -NoExit keeps the shell alive; the prompt function emits OSC 7 CWD
                // notifications so the sidebar can track the working directory.
                cmd.args([
                    "-NoExit",
                    "-Command",
                    concat!(
                        "function prompt {",
                        r#"  "$([char]27)]7;file:///$($PWD.Path.Replace('\','/'))`a" +"#,
                        r#"  "PS $($PWD.Path)> ""#,
                        "}"
                    ),
                ]);
                cmd
            }
            ShellKind::Cmd => CommandBuilder::new("cmd.exe"),
            ShellKind::Bash | ShellKind::Sh => {
                let mut cmd = CommandBuilder::new(shell.executable());
                // PROMPT_COMMAND runs before every prompt — emit OSC 7 for CWD tracking.
                cmd.env(
                    "PROMPT_COMMAND",
                    r#"printf '\e]7;file://%s%s\a' "$HOSTNAME" "$PWD""#,
                );
                cmd
            }
            ShellKind::Zsh => {
                let mut cmd = CommandBuilder::new("zsh");
                // zsh uses precmd; PROMPT_COMMAND works as a compat shim in many setups.
                cmd.env(
                    "PROMPT_COMMAND",
                    r#"printf '\e]7;file://%s%s\a' "$HOST" "$PWD""#,
                );
                cmd
            }
            ShellKind::Fish => CommandBuilder::new("fish"),
        }
    }
}
