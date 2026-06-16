use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::util;

fn default_panel_ratio() -> f32 {
    0.35
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedSession {
    pub(super) cwd: PathBuf,
    #[serde(default)]
    pub(super) command: Option<String>,
    #[serde(default)]
    pub(super) title: Option<String>,
    #[serde(default)]
    pub(super) scrollback_file: Option<String>,
    #[serde(default)]
    pub(super) claude_session_id: Option<String>,
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
        #[serde(default)]
        title: Option<String>,
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

#[derive(Serialize, Deserialize, Default)]
pub(super) enum SavedRightTab {
    #[default]
    Directory,
    GitDiff,
    Markdown(PathBuf),
}

#[derive(Serialize, Deserialize, Default)]
pub(super) struct AppSession {
    #[serde(default)]
    pub(super) sessions: Vec<SavedSession>,
    #[serde(default)]
    pub(super) panes: Vec<SavedPane>,
    #[serde(default)]
    pub(super) active_pane_index: Option<usize>,
    #[serde(default)]
    pub(super) active_session_index: Option<usize>,
    #[serde(default)]
    pub(super) active_group: Option<u64>,
    #[serde(default)]
    pub(super) last_pane_per_group: Vec<(Option<u64>, usize)>,
    #[serde(default = "default_panel_ratio")]
    pub(super) workspace_panel_ratio: f32,
    #[serde(default)]
    pub(super) workspace_panel_collapsed: bool,
    #[serde(default = "default_panel_ratio")]
    pub(super) notes_panel_ratio: f32,
    #[serde(default)]
    pub(super) notes_panel_collapsed: bool,
    #[serde(default)]
    pub(super) right_tab: SavedRightTab,
    #[serde(default)]
    pub(super) shown_md_tabs: Vec<PathBuf>,
}

pub(super) fn session_data_path() -> Option<PathBuf> {
    util::data_file("session.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_saved_session_roundtrip() {
        let original = SavedSession {
            cwd: PathBuf::from("/tmp/mydir"),
            command: Some("ls -la".into()),
            title: Some("my session".into()),
            scrollback_file: None,
            claude_session_id: None,
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: SavedSession = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.cwd, original.cwd);
        assert_eq!(restored.command, original.command);
        assert_eq!(restored.title, original.title);
        assert_eq!(restored.claude_session_id, None);
    }

    #[test]
    fn test_saved_session_missing_command_defaults() {
        let json = r#"{"cwd": "/home/user"}"#;
        let s: SavedSession = serde_json::from_str(json).unwrap();
        assert_eq!(s.cwd, PathBuf::from("/home/user"));
        assert_eq!(s.command, None);
        assert_eq!(s.title, None);
        assert_eq!(s.claude_session_id, None);
    }

    #[test]
    fn test_saved_session_with_claude_session_id_roundtrip() {
        let original = SavedSession {
            cwd: PathBuf::from("/home/user/project"),
            command: Some("claude".into()),
            title: Some("Claude session".into()),
            scrollback_file: None,
            claude_session_id: Some("abc-def-123-456".into()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: SavedSession = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.claude_session_id,
            Some("abc-def-123-456".to_string())
        );
    }

    #[test]
    fn test_saved_session_without_claude_session_id_defaults_none() {
        let json = r#"{"cwd": "/home/user"}"#;
        let s: SavedSession = serde_json::from_str(json).unwrap();
        assert_eq!(s.claude_session_id, None);
    }

    #[test]
    fn test_saved_pane_content_terminal_roundtrip() {
        let original = SavedPaneContent::Terminal { session_index: 42 };
        let json = serde_json::to_string(&original).unwrap();
        let restored: SavedPaneContent = serde_json::from_str(&json).unwrap();
        match restored {
            SavedPaneContent::Terminal { session_index } => assert_eq!(session_index, 42),
            _ => panic!("expected Terminal variant"),
        }
    }

    #[test]
    fn test_saved_pane_content_deferred_terminal_roundtrip() {
        let original = SavedPaneContent::DeferredTerminal {
            cwd: PathBuf::from("/usr/local"),
            command: Some("bash".into()),
            title: Some("my terminal".into()),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: SavedPaneContent = serde_json::from_str(&json).unwrap();
        match restored {
            SavedPaneContent::DeferredTerminal {
                cwd,
                command,
                title,
            } => {
                assert_eq!(cwd, PathBuf::from("/usr/local"));
                assert_eq!(command, Some("bash".into()));
                assert_eq!(title, Some("my terminal".into()));
            }
            _ => panic!("expected DeferredTerminal variant"),
        }
    }

    #[test]
    fn test_saved_pane_content_file_editor_roundtrip() {
        let original = SavedPaneContent::FileEditor {
            path: PathBuf::from("/tmp/test.rs"),
            content: "fn main() {}".into(),
            dirty: true,
            workspace_id: Some(99),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: SavedPaneContent = serde_json::from_str(&json).unwrap();
        match restored {
            SavedPaneContent::FileEditor {
                path,
                content,
                dirty,
                workspace_id,
            } => {
                assert_eq!(path, PathBuf::from("/tmp/test.rs"));
                assert_eq!(content, "fn main() {}");
                assert!(dirty);
                assert_eq!(workspace_id, Some(99));
            }
            _ => panic!("expected FileEditor variant"),
        }
    }

    #[test]
    fn test_saved_pane_content_note_editor_roundtrip() {
        let original = SavedPaneContent::NoteEditor {
            workspace_id: Some(7),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: SavedPaneContent = serde_json::from_str(&json).unwrap();
        match restored {
            SavedPaneContent::NoteEditor { workspace_id } => {
                assert_eq!(workspace_id, Some(7));
            }
            _ => panic!("expected NoteEditor variant"),
        }
    }

    #[test]
    fn test_app_session_roundtrip() {
        let original = AppSession {
            sessions: vec![
                SavedSession {
                    cwd: PathBuf::from("/home/user"),
                    command: None,
                    title: None,
                    scrollback_file: None,
                    claude_session_id: None,
                },
                SavedSession {
                    cwd: PathBuf::from("/tmp"),
                    command: Some("vim".into()),
                    title: Some("vim session".into()),
                    scrollback_file: None,
                    claude_session_id: None,
                },
            ],
            panes: vec![
                SavedPane {
                    content: SavedPaneContent::Terminal { session_index: 0 },
                    manual_width: None,
                },
                SavedPane {
                    content: SavedPaneContent::FileEditor {
                        path: PathBuf::from("/tmp/file.txt"),
                        content: "hello".into(),
                        dirty: false,
                        workspace_id: Some(1),
                    },
                    manual_width: Some(400.0),
                },
                SavedPane {
                    content: SavedPaneContent::NoteEditor { workspace_id: None },
                    manual_width: None,
                },
            ],
            active_pane_index: Some(1),
            active_session_index: Some(0),
            active_group: Some(42),
            last_pane_per_group: vec![(None, 0), (Some(42), 1)],
            workspace_panel_ratio: 0.25,
            workspace_panel_collapsed: false,
            notes_panel_ratio: 0.3,
            notes_panel_collapsed: true,
            right_tab: SavedRightTab::Markdown(PathBuf::from("/docs/README.md")),
            shown_md_tabs: vec![
                PathBuf::from("/docs/README.md"),
                PathBuf::from("/docs/CHANGELOG.md"),
            ],
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: AppSession = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.sessions.len(), 2);
        assert_eq!(restored.sessions[0].cwd, PathBuf::from("/home/user"));
        assert_eq!(restored.sessions[1].command, Some("vim".into()));
        assert_eq!(restored.panes.len(), 3);
        assert_eq!(restored.active_pane_index, Some(1));
        assert_eq!(restored.active_session_index, Some(0));
        assert_eq!(restored.active_group, Some(42));
        assert_eq!(restored.last_pane_per_group.len(), 2);
        assert!((restored.workspace_panel_ratio - 0.25).abs() < f32::EPSILON);
        assert!(!restored.workspace_panel_collapsed);
        assert!((restored.notes_panel_ratio - 0.3).abs() < f32::EPSILON);
        assert!(restored.notes_panel_collapsed);
        match &restored.right_tab {
            SavedRightTab::Markdown(p) => assert_eq!(p, &PathBuf::from("/docs/README.md")),
            _ => panic!("expected Markdown variant"),
        }
        assert_eq!(restored.shown_md_tabs.len(), 2);
    }

    #[test]
    fn test_session_data_path_returns_some() {
        let path = session_data_path();
        assert!(path.is_some(), "session_data_path() should return Some");
        let p = path.unwrap();
        assert!(
            p.ends_with("session.json"),
            "path should end with session.json, got: {:?}",
            p
        );
    }

    #[test]
    fn test_corrupt_json_fails_gracefully() {
        let result = serde_json::from_str::<AppSession>("not json");
        assert!(result.is_err(), "corrupt JSON should return Err");

        let result2 = serde_json::from_str::<AppSession>("{\"sessions\": 123}");
        assert!(result2.is_err(), "invalid schema should return Err");
    }
}
