use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::editor_group::GroupNode;
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
    FileDiff {
        path: PathBuf,
    },
    NoteEditor {
        workspace_id: Option<u64>,
    },
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedPane {
    pub(super) content: SavedPaneContent,
    pub(super) manual_width: Option<f32>,
    #[serde(default)]
    pub(super) labels: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedEditorGroup {
    pub(super) id: u32,
    pub(super) pane_indices: Vec<usize>,
    pub(super) active_pane_index: Option<usize>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct SavedGroupLayout {
    pub(super) groups: Vec<SavedEditorGroup>,
    pub(super) layout: GroupNode,
    pub(super) focused_group_id: u32,
    pub(super) next_group_id: u32,
    #[serde(default = "default_next_split_id")]
    pub(super) next_split_id: u32,
}

fn default_next_split_id() -> u32 {
    1
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
    #[serde(default)]
    pub(super) group_layout: Option<SavedGroupLayout>,
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
    fn test_saved_pane_content_file_diff_roundtrip() {
        let original = SavedPaneContent::FileDiff {
            path: PathBuf::from("/tmp/changed.rs"),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: SavedPaneContent = serde_json::from_str(&json).unwrap();
        match restored {
            SavedPaneContent::FileDiff { path } => {
                assert_eq!(path, PathBuf::from("/tmp/changed.rs"));
            }
            _ => panic!("expected FileDiff variant"),
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
                    labels: vec![],
                },
                SavedPane {
                    content: SavedPaneContent::FileEditor {
                        path: PathBuf::from("/tmp/file.txt"),
                        content: "hello".into(),
                        dirty: false,
                        workspace_id: Some(1),
                    },
                    manual_width: Some(400.0),
                    labels: vec![],
                },
                SavedPane {
                    content: SavedPaneContent::NoteEditor { workspace_id: None },
                    manual_width: None,
                    labels: vec![],
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
            group_layout: None,
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

    #[test]
    fn saved_pane_labels_roundtrip() {
        let pane = SavedPane {
            content: SavedPaneContent::Terminal { session_index: 0 },
            manual_width: None,
            labels: vec![1, 9, 100],
        };
        let json = serde_json::to_string(&pane).unwrap();
        let loaded: SavedPane = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.labels, vec![1, 9, 100]);
    }

    #[test]
    fn saved_pane_labels_default_empty() {
        let json = r#"{"content":{"Terminal":{"session_index":0}},"manual_width":null}"#;
        let loaded: SavedPane = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.labels, Vec::<u32>::new());
    }

    // ── Group layout persistence tests ──────────────────────────

    #[test]
    fn saved_editor_group_roundtrip() {
        let g = SavedEditorGroup {
            id: 1,
            pane_indices: vec![0, 2, 3],
            active_pane_index: Some(2),
        };
        let json = serde_json::to_string(&g).unwrap();
        let restored: SavedEditorGroup = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, 1);
        assert_eq!(restored.pane_indices, vec![0, 2, 3]);
        assert_eq!(restored.active_pane_index, Some(2));
    }

    #[test]
    fn saved_group_layout_roundtrip() {
        use crate::editor_group::GroupNode;
        use crate::pane_tree::SplitDir;
        let layout = SavedGroupLayout {
            groups: vec![
                SavedEditorGroup {
                    id: 1,
                    pane_indices: vec![0, 1],
                    active_pane_index: Some(0),
                },
                SavedEditorGroup {
                    id: 2,
                    pane_indices: vec![2],
                    active_pane_index: Some(0),
                },
            ],
            layout: GroupNode::Split {
                split_id: 1,
                dir: SplitDir::Horizontal,
                ratio: 0.5,
                a: Box::new(GroupNode::Leaf { group_id: 1 }),
                b: Box::new(GroupNode::Leaf { group_id: 2 }),
            },
            focused_group_id: 1,
            next_group_id: 3,
            next_split_id: 2,
        };
        let json = serde_json::to_string(&layout).unwrap();
        let restored: SavedGroupLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.groups.len(), 2);
        assert_eq!(restored.groups[0].id, 1);
        assert_eq!(restored.groups[1].id, 2);
        assert_eq!(restored.focused_group_id, 1);
        assert_eq!(restored.next_group_id, 3);
        assert_eq!(restored.next_split_id, 2);
    }

    #[test]
    fn app_session_with_group_layout_roundtrip() {
        use crate::editor_group::GroupNode;
        let original = AppSession {
            sessions: vec![SavedSession {
                cwd: PathBuf::from("/home/user"),
                command: None,
                title: None,
                scrollback_file: None,
                claude_session_id: None,
            }],
            panes: vec![
                SavedPane {
                    content: SavedPaneContent::Terminal { session_index: 0 },
                    manual_width: None,
                    labels: vec![],
                },
                SavedPane {
                    content: SavedPaneContent::Terminal { session_index: 0 },
                    manual_width: None,
                    labels: vec![],
                },
            ],
            active_pane_index: Some(0),
            active_session_index: Some(0),
            active_group: None,
            last_pane_per_group: vec![],
            workspace_panel_ratio: 0.35,
            workspace_panel_collapsed: false,
            notes_panel_ratio: 0.35,
            notes_panel_collapsed: false,
            right_tab: SavedRightTab::Directory,
            shown_md_tabs: vec![],
            group_layout: Some(SavedGroupLayout {
                groups: vec![SavedEditorGroup {
                    id: 1,
                    pane_indices: vec![0, 1],
                    active_pane_index: Some(0),
                }],
                layout: GroupNode::Leaf { group_id: 1 },
                focused_group_id: 1,
                next_group_id: 2,
                next_split_id: 1,
            }),
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: AppSession = serde_json::from_str(&json).unwrap();
        assert!(restored.group_layout.is_some());
        let gl = restored.group_layout.unwrap();
        assert_eq!(gl.groups.len(), 1);
        assert_eq!(gl.groups[0].pane_indices, vec![0, 1]);
        assert_eq!(gl.focused_group_id, 1);
        assert_eq!(gl.next_group_id, 2);
        match &gl.layout {
            GroupNode::Leaf { group_id } => assert_eq!(*group_id, 1),
            _ => panic!("expected Leaf layout"),
        }
    }

    #[test]
    fn app_session_without_group_layout_defaults_none() {
        let json = r#"{"sessions":[],"panes":[]}"#;
        let s: AppSession = serde_json::from_str(json).unwrap();
        assert!(s.group_layout.is_none());
    }

    #[test]
    fn saved_group_layout_missing_next_split_id_defaults() {
        let json = r#"{"groups":[],"layout":{"Leaf":{"group_id":1}},"focused_group_id":1,"next_group_id":2}"#;
        let restored: SavedGroupLayout = serde_json::from_str(json).unwrap();
        assert_eq!(restored.next_split_id, 1);
    }

    #[test]
    fn saved_group_layout_single_group() {
        use crate::editor_group::GroupNode;
        let layout = SavedGroupLayout {
            groups: vec![SavedEditorGroup {
                id: 5,
                pane_indices: vec![0],
                active_pane_index: Some(0),
            }],
            layout: GroupNode::Leaf { group_id: 5 },
            focused_group_id: 5,
            next_group_id: 6,
            next_split_id: 1,
        };
        let json = serde_json::to_string(&layout).unwrap();
        let restored: SavedGroupLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.groups.len(), 1);
        assert_eq!(restored.groups[0].id, 5);
        assert_eq!(restored.focused_group_id, 5);
        match &restored.layout {
            GroupNode::Leaf { group_id } => assert_eq!(*group_id, 5),
            _ => panic!("expected Leaf"),
        }
    }

    #[test]
    fn saved_group_layout_nested_split() {
        use crate::editor_group::GroupNode;
        use crate::pane_tree::SplitDir;
        let layout = SavedGroupLayout {
            groups: vec![
                SavedEditorGroup {
                    id: 1,
                    pane_indices: vec![0],
                    active_pane_index: Some(0),
                },
                SavedEditorGroup {
                    id: 2,
                    pane_indices: vec![1],
                    active_pane_index: Some(0),
                },
                SavedEditorGroup {
                    id: 3,
                    pane_indices: vec![2],
                    active_pane_index: Some(0),
                },
            ],
            layout: GroupNode::Split {
                split_id: 1,
                dir: SplitDir::Horizontal,
                ratio: 0.5,
                a: Box::new(GroupNode::Leaf { group_id: 1 }),
                b: Box::new(GroupNode::Split {
                    split_id: 2,
                    dir: SplitDir::Vertical,
                    ratio: 0.5,
                    a: Box::new(GroupNode::Leaf { group_id: 2 }),
                    b: Box::new(GroupNode::Leaf { group_id: 3 }),
                }),
            },
            focused_group_id: 2,
            next_group_id: 4,
            next_split_id: 3,
        };
        let json = serde_json::to_string(&layout).unwrap();
        let restored: SavedGroupLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.groups.len(), 3);
        assert_eq!(restored.focused_group_id, 2);
        assert_eq!(restored.next_group_id, 4);
        assert_eq!(restored.next_split_id, 3);
        // Verify the nested structure by checking group IDs in the layout
        match &restored.layout {
            GroupNode::Split { a, b, .. } => {
                match a.as_ref() {
                    GroupNode::Leaf { group_id } => assert_eq!(*group_id, 1),
                    _ => panic!("expected Leaf for a"),
                }
                match b.as_ref() {
                    GroupNode::Split { a: ba, b: bb, .. } => {
                        match ba.as_ref() {
                            GroupNode::Leaf { group_id } => assert_eq!(*group_id, 2),
                            _ => panic!("expected Leaf for b.a"),
                        }
                        match bb.as_ref() {
                            GroupNode::Leaf { group_id } => assert_eq!(*group_id, 3),
                            _ => panic!("expected Leaf for b.b"),
                        }
                    }
                    _ => panic!("expected Split for b"),
                }
            }
            _ => panic!("expected Split root"),
        }
    }
}
