use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(super) struct SavedSession {
    pub(super) cwd: PathBuf,
    #[serde(default)]
    pub(super) command: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(super) enum SavedPaneContent {
    Terminal {
        session_index: usize,
    },
    DeferredTerminal {
        cwd: PathBuf,
        #[serde(default)]
        command: Option<String>,
    },
    FileEditor {
        path: PathBuf,
        content: String,
        dirty: bool,
        workspace_id: Option<u64>,
    },
    NoteEditor {
        workspace_id: Option<u64>,
    },
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedPane {
    pub(super) content: SavedPaneContent,
    pub(super) manual_width: Option<f32>,
}

#[derive(Serialize, Deserialize)]
pub(super) enum SavedRightTab {
    Directory,
    GitDiff,
    Markdown(PathBuf),
}

#[derive(Serialize, Deserialize)]
pub(super) struct AppSession {
    pub(super) sessions: Vec<SavedSession>,
    pub(super) panes: Vec<SavedPane>,
    pub(super) active_pane_index: Option<usize>,
    pub(super) active_session_index: Option<usize>,
    pub(super) active_group: Option<u64>,
    pub(super) last_pane_per_group: Vec<(Option<u64>, usize)>,
    pub(super) workspace_panel_ratio: f32,
    pub(super) workspace_panel_collapsed: bool,
    pub(super) notes_panel_ratio: f32,
    pub(super) notes_panel_collapsed: bool,
    pub(super) right_tab: SavedRightTab,
    pub(super) shown_md_tabs: Vec<PathBuf>,
}

pub(super) fn session_data_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|base| {
            PathBuf::from(base)
                .join("terminal-studio")
                .join("session.json")
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(|base| {
            PathBuf::from(base)
                .join(".config")
                .join("terminal-studio")
                .join("session.json")
        })
    }
}
