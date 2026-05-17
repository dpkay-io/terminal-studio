use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorStyle {
    Block,
    Underline,
    Beam,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub(super) struct AppSettings {
    pub(super) default_workspace_id: Option<u64>,
    pub(super) restore_last_session: bool,
    pub(super) theme_id: theme::ThemeId,
    pub(super) last_update_check: Option<u64>,
    pub(super) skip_version: Option<String>,
    pub(super) font_size: f32,
    pub(super) scrollback_lines: usize,
    pub(super) cursor_style: CursorStyle,
    pub(super) cursor_blink: bool,
    pub(super) scroll_on_output: bool,
    pub(super) default_shell: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            default_workspace_id: None,
            restore_last_session: true,
            theme_id: theme::ThemeId::CatppuccinMocha,
            last_update_check: None,
            skip_version: None,
            font_size: 14.0,
            scrollback_lines: 10_000,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            scroll_on_output: false,
            default_shell: None,
        }
    }
}

impl AppSettings {
    pub(super) fn load() -> Self {
        let Some(path) = settings_data_path() else {
            let s = Self::default();
            theme::set_theme(s.theme_id);
            return s;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            let s = Self::default();
            theme::set_theme(s.theme_id);
            return s;
        };
        let s: Self = serde_json::from_str(&text).unwrap_or_default();
        theme::set_theme(s.theme_id);
        s
    }

    pub(super) fn save(&self) {
        let Some(path) = settings_data_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

pub(super) fn windows_data_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|base| {
            PathBuf::from(base)
                .join("terminal-studio")
                .join("windows.json")
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(|base| {
            PathBuf::from(base)
                .join(".config")
                .join("terminal-studio")
                .join("windows.json")
        })
    }
}

fn settings_data_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|base| {
            PathBuf::from(base)
                .join("terminal-studio")
                .join("settings.json")
        })
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(|base| {
            PathBuf::from(base)
                .join(".config")
                .join("terminal-studio")
                .join("settings.json")
        })
    }
}
