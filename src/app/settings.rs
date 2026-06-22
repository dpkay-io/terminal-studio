use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::diff_parser::DiffViewMode;
use crate::logging::LogLevel;
use crate::theme;
use crate::util;

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
    pub(super) scroll_lines: u32,
    pub(super) default_shell: Option<String>,
    pub(super) show_sys_monitor: bool,
    pub(super) diff_view_mode: DiffViewMode,
    pub(super) max_closed_sessions: usize,
    pub(super) save_scrollback_on_close: bool,
    pub(super) save_scrollback_on_exit: bool,
    pub(super) log_level: LogLevel,
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
            scrollback_lines: 100_000,
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            scroll_on_output: false,
            scroll_lines: 3,
            default_shell: None,
            show_sys_monitor: true,
            diff_view_mode: DiffViewMode::Inline,
            max_closed_sessions: 50,
            save_scrollback_on_close: true,
            save_scrollback_on_exit: true,
            log_level: LogLevel::default(),
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
        let s: Self = crate::util::safe_json_load(&path).unwrap_or_default();
        theme::set_theme(s.theme_id);
        s
    }

    pub(super) fn save(&self) {
        let Some(path) = settings_data_path() else {
            return;
        };
        if let Ok(text) = serde_json::to_string_pretty(self) {
            if let Err(e) = util::atomic_write(&path, &text) {
                log::error!("failed to save settings: {e}");
            }
        }
    }
}

pub(super) fn windows_data_path() -> Option<PathBuf> {
    util::data_file("windows.json")
}

fn settings_data_path() -> Option<PathBuf> {
    util::data_file("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings_values() {
        let s = AppSettings::default();
        assert_eq!(s.default_workspace_id, None);
        assert!(s.restore_last_session);
        assert_eq!(s.theme_id, theme::ThemeId::CatppuccinMocha);
        assert_eq!(s.last_update_check, None);
        assert_eq!(s.skip_version, None);
        assert!((s.font_size - 14.0).abs() < f32::EPSILON);
        assert_eq!(s.scrollback_lines, 100_000);
        assert_eq!(s.cursor_style, CursorStyle::Block);
        assert!(s.cursor_blink);
        assert!(!s.scroll_on_output);
        assert_eq!(s.default_shell, None);
        assert!(s.show_sys_monitor);
    }

    #[test]
    fn test_cursor_style_roundtrip() {
        for style in [
            CursorStyle::Block,
            CursorStyle::Underline,
            CursorStyle::Beam,
        ] {
            let json = serde_json::to_string(&style).unwrap();
            let restored: CursorStyle = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, style);
        }
    }

    #[test]
    fn test_settings_roundtrip() {
        let original = AppSettings {
            font_size: 18.0,
            scrollback_lines: 5000,
            cursor_style: CursorStyle::Beam,
            cursor_blink: false,
            restore_last_session: false,
            default_workspace_id: Some(42),
            scroll_on_output: true,
            default_shell: Some("/bin/zsh".into()),
            show_sys_monitor: false,
            ..AppSettings::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let restored: AppSettings = serde_json::from_str(&json).unwrap();
        assert!((restored.font_size - 18.0).abs() < f32::EPSILON);
        assert_eq!(restored.scrollback_lines, 5000);
        assert_eq!(restored.cursor_style, CursorStyle::Beam);
        assert!(!restored.cursor_blink);
        assert!(!restored.restore_last_session);
        assert_eq!(restored.default_workspace_id, Some(42));
        assert!(restored.scroll_on_output);
        assert_eq!(restored.default_shell, Some("/bin/zsh".into()));
        assert!(!restored.show_sys_monitor);
    }

    #[test]
    fn test_settings_missing_fields_use_defaults() {
        let json = "{}";
        let s: AppSettings = serde_json::from_str(json).unwrap();
        let d = AppSettings::default();
        assert!((s.font_size - d.font_size).abs() < f32::EPSILON);
        assert_eq!(s.scrollback_lines, d.scrollback_lines);
        assert_eq!(s.cursor_style, d.cursor_style);
        assert_eq!(s.cursor_blink, d.cursor_blink);
        assert_eq!(s.restore_last_session, d.restore_last_session);
        assert_eq!(s.default_workspace_id, d.default_workspace_id);
        assert_eq!(s.theme_id, d.theme_id);
    }

    #[test]
    fn test_settings_partial_json() {
        let json = r#"{"font_size": 16.0, "cursor_blink": false}"#;
        let s: AppSettings = serde_json::from_str(json).unwrap();
        // Explicit values from JSON
        assert!((s.font_size - 16.0).abs() < f32::EPSILON);
        assert!(!s.cursor_blink);
        // Remaining fields fall back to defaults
        assert_eq!(s.scrollback_lines, 100_000);
        assert_eq!(s.cursor_style, CursorStyle::Block);
        assert!(s.restore_last_session);
        assert_eq!(s.theme_id, theme::ThemeId::CatppuccinMocha);
    }

    #[test]
    fn test_windows_data_path_returns_some() {
        let path = windows_data_path();
        assert!(path.is_some(), "windows_data_path() should return Some");
        let p = path.unwrap();
        assert!(
            p.ends_with("windows.json"),
            "path should end with windows.json, got: {:?}",
            p
        );
    }
}
