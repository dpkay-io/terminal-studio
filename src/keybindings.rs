#![allow(dead_code)]
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::shortcuts::{default_bindings, key_from_name, key_name, AppAction, Shortcut};
use crate::util;

#[derive(Serialize, Deserialize, Clone)]
pub struct KeyBinding {
    pub action: String,
    /// The primary modifier: Command on macOS, Ctrl elsewhere.
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// The physical Control key on macOS (as in Control-Command-F). Absent in
    /// older files, where it defaults to off.
    #[serde(default)]
    pub control: bool,
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

    /// The platform defaults from `shortcuts::default_bindings`, in file form.
    pub fn default_bindings() -> Vec<KeyBinding> {
        default_bindings()
            .into_iter()
            .map(|(s, a)| KeyBinding {
                action: a.name().to_string(),
                ctrl: s.ctrl,
                shift: s.shift,
                alt: s.alt,
                control: s.control,
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
                        control: kb.control,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_the_file_form() {
        let config = KeybindingsConfig {
            bindings: KeybindingsConfig::default_bindings(),
        };
        assert_eq!(config.to_shortcut_pairs(), default_bindings());
    }

    #[test]
    fn older_files_without_control_still_load() {
        let json =
            r#"[{"action":"new_terminal_tab","ctrl":true,"shift":true,"alt":false,"key":"N"}]"#;
        let bindings: Vec<KeyBinding> = serde_json::from_str(json).expect("parses");
        let pairs = KeybindingsConfig { bindings }.to_shortcut_pairs();
        assert_eq!(
            pairs,
            vec![(
                Shortcut::primary(egui::Key::N).with_shift(),
                AppAction::NewTerminalTab
            )]
        );
    }

    #[test]
    fn unknown_actions_and_keys_are_skipped() {
        let json = r#"[
            {"action":"no_such_action","ctrl":true,"shift":false,"alt":false,"key":"N"},
            {"action":"new_terminal_tab","ctrl":true,"shift":false,"alt":false,"key":"NoSuchKey"}
        ]"#;
        let bindings: Vec<KeyBinding> = serde_json::from_str(json).expect("parses");
        assert!(KeybindingsConfig { bindings }
            .to_shortcut_pairs()
            .is_empty());
    }
}
