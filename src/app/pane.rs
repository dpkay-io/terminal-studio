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
    pub(super) display_offset: usize,
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

#[derive(Clone, Debug)]
pub(super) struct NoteEditorState {
    pub(super) workspace_id: Option<u64>,
}

#[derive(Debug)]
pub(super) enum PaneContent {
    Terminal(u32),
    DeferredTerminal {
        cwd: Option<PathBuf>,
        pending_command: Option<String>,
        saved_title: Option<String>,
    },
    FileEditor(FileEditorState),
    FileDiff(FileDiffState),
    NoteEditor(NoteEditorState),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_term_selection_ordered_normal() {
        let sel = TermSelection {
            start_col: 2,
            start_row: 1,
            end_col: 10,
            end_row: 3,
            display_offset: 0,
        };
        assert_eq!(sel.ordered(), (2, 1, 10, 3));
    }

    #[test]
    fn test_term_selection_ordered_reversed() {
        let sel = TermSelection {
            start_col: 10,
            start_row: 5,
            end_col: 2,
            end_row: 1,
            display_offset: 0,
        };
        assert_eq!(sel.ordered(), (2, 1, 10, 5));
    }

    #[test]
    fn test_term_selection_ordered_same_row() {
        let sel = TermSelection {
            start_col: 15,
            start_row: 3,
            end_col: 5,
            end_row: 3,
            display_offset: 0,
        };
        // Same row, start_col > end_col => should swap
        assert_eq!(sel.ordered(), (5, 3, 15, 3));
    }

    #[test]
    fn test_term_selection_ordered_same_position() {
        let sel = TermSelection {
            start_col: 4,
            start_row: 2,
            end_col: 4,
            end_row: 2,
            display_offset: 0,
        };
        assert_eq!(sel.ordered(), (4, 2, 4, 2));
    }

    #[test]
    fn test_file_editor_state_clone() {
        let state = FileEditorState {
            path: PathBuf::from("/tmp/test.rs"),
            content: "fn main() {}".to_string(),
            dirty: true,
            save_error: false,
            workspace_id: Some(42),
            show_preview: false,
        };
        let cloned = state.clone();
        assert_eq!(cloned.path, state.path);
        assert_eq!(cloned.content, state.content);
        assert_eq!(cloned.dirty, state.dirty);
        assert_eq!(cloned.save_error, state.save_error);
        assert_eq!(cloned.workspace_id, state.workspace_id);
        assert_eq!(cloned.show_preview, state.show_preview);
    }

    #[test]
    fn test_pane_content_variants() {
        // Verify each variant can be constructed and supports Debug
        let terminal = PaneContent::Terminal(1);
        let deferred = PaneContent::DeferredTerminal {
            cwd: Some(PathBuf::from("/home")),
            pending_command: Some("ls".to_string()),
            saved_title: None,
        };
        let editor = PaneContent::FileEditor(FileEditorState {
            path: PathBuf::from("test.txt"),
            content: String::new(),
            dirty: false,
            save_error: false,
            workspace_id: None,
            show_preview: false,
        });
        let diff = PaneContent::FileDiff(FileDiffState {
            path: PathBuf::from("file.rs"),
            diff_content: "+added line".to_string(),
        });
        let note = PaneContent::NoteEditor(NoteEditorState {
            workspace_id: Some(99),
        });

        // All variants implement Debug
        assert!(!format!("{:?}", terminal).is_empty());
        assert!(!format!("{:?}", deferred).is_empty());
        assert!(!format!("{:?}", editor).is_empty());
        assert!(!format!("{:?}", diff).is_empty());
        assert!(!format!("{:?}", note).is_empty());
    }

    #[test]
    fn test_right_tab_eq() {
        assert_eq!(RightTab::Directory, RightTab::Directory);
        assert_eq!(RightTab::GitDiff, RightTab::GitDiff);
        assert_eq!(
            RightTab::Markdown(PathBuf::from("README.md")),
            RightTab::Markdown(PathBuf::from("README.md"))
        );
    }

    #[test]
    fn test_right_tab_ne() {
        assert_ne!(RightTab::Directory, RightTab::GitDiff);
        assert_ne!(
            RightTab::Markdown(PathBuf::from("a.md")),
            RightTab::Markdown(PathBuf::from("b.md"))
        );
        assert_ne!(
            RightTab::Directory,
            RightTab::Markdown(PathBuf::from("x.md"))
        );
    }
}
