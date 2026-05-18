use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::pty::ShellKind;
use crate::terminal::Session;

#[derive(Clone, Copy, Debug)]
pub(super) struct TermSelection {
    pub(super) start_col: u16,
    pub(super) start_row: u16,
    pub(super) end_col: u16,
    pub(super) end_row: u16,
}

impl TermSelection {
    pub(super) fn ordered(&self) -> (u16, u16, u16, u16) {
        if self.start_row < self.end_row
            || (self.start_row == self.end_row && self.start_col <= self.end_col)
        {
            (self.start_col, self.start_row, self.end_col, self.end_row)
        } else {
            (self.end_col, self.end_row, self.start_col, self.start_row)
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub(super) enum RightTab {
    Directory,
    GitDiff,
    Markdown(PathBuf),
}

#[derive(Clone, Debug)]
pub(super) struct FileEditorState {
    pub(super) path: PathBuf,
    pub(super) content: String,
    pub(super) dirty: bool,
    pub(super) save_error: bool,
    pub(super) workspace_id: Option<u64>,
    pub(super) show_preview: bool,
}

#[derive(Clone, Debug)]
pub(super) struct FileDiffState {
    pub(super) path: PathBuf,
    pub(super) diff_content: String,
}

#[derive(Debug)]
pub(super) enum PaneContent {
    Terminal(u32),
    DeferredTerminal {
        cwd: Option<PathBuf>,
        pending_command: Option<String>,
    },
    FileEditor(FileEditorState),
    FileDiff(FileDiffState),
}

pub(super) struct PaneEntry {
    pub(super) id: u32,
    pub(super) content: PaneContent,
    pub(super) manual_width: Option<f32>,
    pub(super) last_size: (u16, u16),
}

pub(super) struct SessionEntry {
    pub(super) id: u32,
    pub(super) session: Arc<RwLock<Session>>,
    pub(super) pty_tx: mpsc::SyncSender<Vec<u8>>,
    pub(super) master: Box<dyn portable_pty::MasterPty + Send>,
    pub(super) shell_pid: u32,
    pub(super) alive: Arc<AtomicBool>,
    pub(super) is_active: Arc<AtomicBool>,
    pub(super) pending_command: Option<String>,
    pub(super) shell: ShellKind,
}
