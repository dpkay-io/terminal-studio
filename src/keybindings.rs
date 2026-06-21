#![allow(dead_code)]
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::shortcuts::{key_from_name, key_name, AppAction, Shortcut};
use crate::util;

#[derive(Serialize, Deserialize, Clone)]
pub struct KeyBinding {
    pub action: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: String,
}

pub struct KeybindingsConfig {
    pub bindings: Vec<KeyBinding>,
}

impl KeybindingsConfig {
    pub fn load() -> Self {
        let Some(path) = keybindings_data_path() else {
            return Self {
                bindings: Self::default_bindings(),
            };
        };
        match crate::util::safe_json_load::<Vec<KeyBinding>>(&path) {
            Some(bindings) if !bindings.is_empty() => Self { bindings },
            _ => Self {
                bindings: Self::default_bindings(),
            },
        }
    }

    pub fn save(&self) {
        let Some(path) = keybindings_data_path() else {
            return;
        };
        if let Ok(text) = serde_json::to_string_pretty(&self.bindings) {
            if let Err(e) = util::atomic_write(&path, &text) {
                log::error!("failed to save keybindings: {e}");
            }
        }
    }

    pub fn default_bindings() -> Vec<KeyBinding> {
        let defaults: Vec<(Shortcut, AppAction)> = vec![
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::B,
                },
                AppAction::ToggleLeftSidebar,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::E,
                },
                AppAction::ToggleRightSidebar,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Backtick,
                },
                AppAction::FocusTerminal,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::N,
                },
                AppAction::NewTerminalTab,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::W,
                },
                AppAction::CloseCurrentPane,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Num1,
                },
                AppAction::SwitchToTab1,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Num2,
                },
                AppAction::SwitchToTab2,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Num3,
                },
                AppAction::SwitchToTab3,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Num4,
                },
                AppAction::SwitchToTab4,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Num5,
                },
                AppAction::SwitchToTab5,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Num6,
                },
                AppAction::SwitchToTab6,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Num7,
                },
                AppAction::SwitchToTab7,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Num8,
                },
                AppAction::SwitchToTab8,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Num9,
                },
                AppAction::SwitchToTab9,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::OpenBracket,
                },
                AppAction::PreviousTab,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::CloseBracket,
                },
                AppAction::NextTab,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Backslash,
                },
                AppAction::SplitHorizontal,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Minus,
                },
                AppAction::SplitVertical,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Comma,
                },
                AppAction::OpenSettings,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::PageDown,
                },
                AppAction::NextWorkspace,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::PageUp,
                },
                AppAction::PrevWorkspace,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::D,
                },
                AppAction::RightTabDirectory,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::G,
                },
                AppAction::RightTabGitDiff,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::J,
                },
                AppAction::ToggleNotes,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::K,
                },
                AppAction::DuplicateSession,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::C,
                },
                AppAction::CopySelection,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::F,
                },
                AppAction::FocusSessionSearch,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::P,
                },
                AppAction::CommandPalette,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Slash,
                },
                AppAction::ToggleShortcutHelp,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Space,
                },
                AppAction::OpenQuickSwitcher,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: false,
                    alt: false,
                    key: egui::Key::F,
                },
                AppAction::SearchTerminal,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::T,
                },
                AppAction::SearchAllSessions,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::Z,
                },
                AppAction::ZoomPane,
            ),
            (
                Shortcut {
                    ctrl: true,
                    shift: true,
                    alt: false,
                    key: egui::Key::R,
                },
                AppAction::ReopenClosedSession,
            ),
        ];

        defaults
            .into_iter()
            .map(|(s, a)| KeyBinding {
                action: a.name().to_string(),
                ctrl: s.ctrl,
                shift: s.shift,
                alt: s.alt,
                key: key_name(s.key).to_string(),
            })
            .collect()
    }

    /// Convert loaded bindings into (Shortcut, AppAction) pairs suitable for
    /// ShortcutRegistry. Invalid entries are silently skipped.
    pub fn to_shortcut_pairs(&self) -> Vec<(Shortcut, AppAction)> {
        self.bindings
            .iter()
            .filter_map(|kb| {
                let action = AppAction::from_name(&kb.action)?;
                let key = key_from_name(&kb.key)?;
                Some((
                    Shortcut {
                        ctrl: kb.ctrl,
                        shift: kb.shift,
                        alt: kb.alt,
                        key,
                    },
                    action,
                ))
            })
            .collect()
    }
}

fn keybindings_data_path() -> Option<PathBuf> {
    util::data_file("keybindings.json")
}
