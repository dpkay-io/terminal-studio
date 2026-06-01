#![allow(dead_code)]
use std::collections::HashMap;

use crate::keybindings::KeybindingsConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppAction {
    // Panel navigation
    ToggleLeftSidebar,
    ToggleRightSidebar,
    FocusTerminal,

    // Tab management
    NewTerminalTab,
    CloseCurrentPane,
    SwitchToTab1,
    SwitchToTab2,
    SwitchToTab3,
    SwitchToTab4,
    SwitchToTab5,
    SwitchToTab6,
    SwitchToTab7,
    SwitchToTab8,
    SwitchToTab9,
    PreviousTab,
    NextTab,

    // Pane splits
    SplitHorizontal,
    SplitVertical,

    // Workspace
    OpenSettings,
    NextWorkspace,
    PrevWorkspace,

    // Right panel tabs
    RightTabDirectory,
    RightTabGitDiff,
    ToggleNotes,

    // Session
    DuplicateSession,

    // Terminal
    CopySelection,

    // Search
    FocusSessionSearch,
    FocusFileSearch,

    // Help
    ToggleShortcutHelp,

    // Quick Switcher
    OpenQuickSwitcher,

    // Terminal search
    SearchTerminal,

    // Global search across all sessions
    SearchAllSessions,

    // Pane zoom
    ZoomPane,

    // Command palette
    CommandPalette,
}

impl AppAction {
    pub fn name(self) -> &'static str {
        match self {
            Self::ToggleLeftSidebar => "toggle_left_sidebar",
            Self::ToggleRightSidebar => "toggle_right_sidebar",
            Self::FocusTerminal => "focus_terminal",
            Self::NewTerminalTab => "new_terminal_tab",
            Self::CloseCurrentPane => "close_current_pane",
            Self::SwitchToTab1 => "switch_to_tab_1",
            Self::SwitchToTab2 => "switch_to_tab_2",
            Self::SwitchToTab3 => "switch_to_tab_3",
            Self::SwitchToTab4 => "switch_to_tab_4",
            Self::SwitchToTab5 => "switch_to_tab_5",
            Self::SwitchToTab6 => "switch_to_tab_6",
            Self::SwitchToTab7 => "switch_to_tab_7",
            Self::SwitchToTab8 => "switch_to_tab_8",
            Self::SwitchToTab9 => "switch_to_tab_9",
            Self::PreviousTab => "previous_tab",
            Self::NextTab => "next_tab",
            Self::SplitHorizontal => "split_horizontal",
            Self::SplitVertical => "split_vertical",
            Self::OpenSettings => "open_settings",
            Self::NextWorkspace => "next_workspace",
            Self::PrevWorkspace => "prev_workspace",
            Self::RightTabDirectory => "right_tab_directory",
            Self::RightTabGitDiff => "right_tab_git_diff",
            Self::ToggleNotes => "toggle_notes",
            Self::DuplicateSession => "duplicate_session",
            Self::CopySelection => "copy_selection",
            Self::FocusSessionSearch => "focus_session_search",
            Self::FocusFileSearch => "focus_file_search",
            Self::ToggleShortcutHelp => "toggle_shortcut_help",
            Self::OpenQuickSwitcher => "open_quick_switcher",
            Self::SearchTerminal => "search_terminal",
            Self::SearchAllSessions => "search_all_sessions",
            Self::ZoomPane => "zoom_pane",
            Self::CommandPalette => "command_palette",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "toggle_left_sidebar" => Some(Self::ToggleLeftSidebar),
            "toggle_right_sidebar" => Some(Self::ToggleRightSidebar),
            "focus_terminal" => Some(Self::FocusTerminal),
            "new_terminal_tab" => Some(Self::NewTerminalTab),
            "close_current_pane" => Some(Self::CloseCurrentPane),
            "switch_to_tab_1" => Some(Self::SwitchToTab1),
            "switch_to_tab_2" => Some(Self::SwitchToTab2),
            "switch_to_tab_3" => Some(Self::SwitchToTab3),
            "switch_to_tab_4" => Some(Self::SwitchToTab4),
            "switch_to_tab_5" => Some(Self::SwitchToTab5),
            "switch_to_tab_6" => Some(Self::SwitchToTab6),
            "switch_to_tab_7" => Some(Self::SwitchToTab7),
            "switch_to_tab_8" => Some(Self::SwitchToTab8),
            "switch_to_tab_9" => Some(Self::SwitchToTab9),
            "previous_tab" => Some(Self::PreviousTab),
            "next_tab" => Some(Self::NextTab),
            "split_horizontal" => Some(Self::SplitHorizontal),
            "split_vertical" => Some(Self::SplitVertical),
            "open_settings" => Some(Self::OpenSettings),
            "next_workspace" => Some(Self::NextWorkspace),
            "prev_workspace" => Some(Self::PrevWorkspace),
            "right_tab_directory" => Some(Self::RightTabDirectory),
            "right_tab_git_diff" => Some(Self::RightTabGitDiff),
            "toggle_notes" => Some(Self::ToggleNotes),
            "duplicate_session" => Some(Self::DuplicateSession),
            "copy_selection" => Some(Self::CopySelection),
            "focus_session_search" => Some(Self::FocusSessionSearch),
            "focus_file_search" => Some(Self::FocusFileSearch),
            "toggle_shortcut_help" => Some(Self::ToggleShortcutHelp),
            "open_quick_switcher" => Some(Self::OpenQuickSwitcher),
            "search_terminal" => Some(Self::SearchTerminal),
            "search_all_sessions" => Some(Self::SearchAllSessions),
            "zoom_pane" => Some(Self::ZoomPane),
            "command_palette" => Some(Self::CommandPalette),
            _ => None,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ToggleLeftSidebar => "Toggle left sidebar",
            Self::ToggleRightSidebar => "Toggle right sidebar",
            Self::FocusTerminal => "Focus terminal",
            Self::NewTerminalTab => "New terminal tab",
            Self::CloseCurrentPane => "Close current pane",
            Self::SwitchToTab1 => "Switch to tab 1",
            Self::SwitchToTab2 => "Switch to tab 2",
            Self::SwitchToTab3 => "Switch to tab 3",
            Self::SwitchToTab4 => "Switch to tab 4",
            Self::SwitchToTab5 => "Switch to tab 5",
            Self::SwitchToTab6 => "Switch to tab 6",
            Self::SwitchToTab7 => "Switch to tab 7",
            Self::SwitchToTab8 => "Switch to tab 8",
            Self::SwitchToTab9 => "Switch to tab 9",
            Self::PreviousTab => "Previous tab",
            Self::NextTab => "Next tab",
            Self::SplitHorizontal => "Split horizontal",
            Self::SplitVertical => "Split vertical",
            Self::OpenSettings => "Open settings",
            Self::NextWorkspace => "Next workspace",
            Self::PrevWorkspace => "Previous workspace",
            Self::RightTabDirectory => "Search in directory",
            Self::RightTabGitDiff => "Git diff panel",
            Self::ToggleNotes => "Toggle notes",
            Self::DuplicateSession => "Duplicate session",
            Self::CopySelection => "Copy selection",
            Self::FocusSessionSearch => "Search sessions",
            Self::FocusFileSearch => "Search files",
            Self::ToggleShortcutHelp => "Keyboard shortcuts",
            Self::OpenQuickSwitcher => "Quick switcher",
            Self::SearchTerminal => "Search terminal",
            Self::SearchAllSessions => "Search all sessions",
            Self::ZoomPane => "Zoom pane",
            Self::CommandPalette => "Command palette",
        }
    }

    pub fn tab_index(self) -> Option<usize> {
        match self {
            Self::SwitchToTab1 => Some(0),
            Self::SwitchToTab2 => Some(1),
            Self::SwitchToTab3 => Some(2),
            Self::SwitchToTab4 => Some(3),
            Self::SwitchToTab5 => Some(4),
            Self::SwitchToTab6 => Some(5),
            Self::SwitchToTab7 => Some(6),
            Self::SwitchToTab8 => Some(7),
            Self::SwitchToTab9 => Some(8),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: egui::Key,
}

impl Shortcut {
    const fn cs(key: egui::Key) -> Self {
        Shortcut {
            ctrl: true,
            shift: true,
            alt: false,
            key,
        }
    }

    pub fn matches(&self, key: &egui::Key, mods: &egui::Modifiers) -> bool {
        if *key != self.key || mods.shift != self.shift || mods.alt != self.alt {
            return false;
        }
        if self.ctrl {
            // On macOS, accept both Ctrl and Cmd for app shortcuts
            mods.ctrl || mods.mac_cmd
        } else {
            !mods.ctrl && !mods.mac_cmd
        }
    }

    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            if cfg!(target_os = "macos") {
                parts.push("\u{2318}");
            } else {
                parts.push("Ctrl");
            }
        }
        if self.alt {
            if cfg!(target_os = "macos") {
                parts.push("\u{2325}");
            } else {
                parts.push("Alt");
            }
        }
        if self.shift {
            if cfg!(target_os = "macos") {
                parts.push("\u{21E7}");
            } else {
                parts.push("Shift");
            }
        }
        parts.push(key_name(self.key));
        parts.join("+")
    }
}

pub fn key_name(key: egui::Key) -> &'static str {
    match key {
        egui::Key::Backslash => "\\",
        egui::Key::Minus => "-",
        egui::Key::OpenBracket => "[",
        egui::Key::CloseBracket => "]",
        egui::Key::Backtick => "`",
        egui::Key::Slash => "/",
        egui::Key::Comma => ",",
        egui::Key::Period => ".",
        egui::Key::Semicolon => ";",
        egui::Key::Equals => "=",
        egui::Key::Num0 => "0",
        egui::Key::Num1 => "1",
        egui::Key::Num2 => "2",
        egui::Key::Num3 => "3",
        egui::Key::Num4 => "4",
        egui::Key::Num5 => "5",
        egui::Key::Num6 => "6",
        egui::Key::Num7 => "7",
        egui::Key::Num8 => "8",
        egui::Key::Num9 => "9",
        egui::Key::A => "A",
        egui::Key::B => "B",
        egui::Key::C => "C",
        egui::Key::D => "D",
        egui::Key::E => "E",
        egui::Key::F => "F",
        egui::Key::G => "G",
        egui::Key::H => "H",
        egui::Key::I => "I",
        egui::Key::J => "J",
        egui::Key::K => "K",
        egui::Key::L => "L",
        egui::Key::M => "M",
        egui::Key::N => "N",
        egui::Key::O => "O",
        egui::Key::P => "P",
        egui::Key::Q => "Q",
        egui::Key::R => "R",
        egui::Key::S => "S",
        egui::Key::T => "T",
        egui::Key::U => "U",
        egui::Key::V => "V",
        egui::Key::W => "W",
        egui::Key::X => "X",
        egui::Key::Y => "Y",
        egui::Key::Z => "Z",
        egui::Key::Escape => "Esc",
        egui::Key::Tab => "Tab",
        egui::Key::Backspace => "Backspace",
        egui::Key::Enter => "Enter",
        egui::Key::Space => "Space",
        egui::Key::Insert => "Insert",
        egui::Key::Delete => "Delete",
        egui::Key::Home => "Home",
        egui::Key::End => "End",
        egui::Key::PageUp => "PgUp",
        egui::Key::PageDown => "PgDn",
        egui::Key::ArrowLeft => "Left",
        egui::Key::ArrowRight => "Right",
        egui::Key::ArrowUp => "Up",
        egui::Key::ArrowDown => "Down",
        egui::Key::F1 => "F1",
        egui::Key::F2 => "F2",
        egui::Key::F3 => "F3",
        egui::Key::F4 => "F4",
        egui::Key::F5 => "F5",
        egui::Key::F6 => "F6",
        egui::Key::F7 => "F7",
        egui::Key::F8 => "F8",
        egui::Key::F9 => "F9",
        egui::Key::F10 => "F10",
        egui::Key::F11 => "F11",
        egui::Key::F12 => "F12",
        _ => "?",
    }
}

/// Reverse lookup: parse a key name back to egui::Key. Returns None only for
/// the "?" fallback, so round-tripping through key_name/key_from_name is
/// lossless for all keys we can name.
pub fn key_from_name(name: &str) -> Option<egui::Key> {
    match name {
        "\\" => Some(egui::Key::Backslash),
        "-" => Some(egui::Key::Minus),
        "[" => Some(egui::Key::OpenBracket),
        "]" => Some(egui::Key::CloseBracket),
        "`" => Some(egui::Key::Backtick),
        "/" => Some(egui::Key::Slash),
        "," => Some(egui::Key::Comma),
        "." => Some(egui::Key::Period),
        ";" => Some(egui::Key::Semicolon),
        "=" => Some(egui::Key::Equals),
        "0" => Some(egui::Key::Num0),
        "1" => Some(egui::Key::Num1),
        "2" => Some(egui::Key::Num2),
        "3" => Some(egui::Key::Num3),
        "4" => Some(egui::Key::Num4),
        "5" => Some(egui::Key::Num5),
        "6" => Some(egui::Key::Num6),
        "7" => Some(egui::Key::Num7),
        "8" => Some(egui::Key::Num8),
        "9" => Some(egui::Key::Num9),
        "A" => Some(egui::Key::A),
        "B" => Some(egui::Key::B),
        "C" => Some(egui::Key::C),
        "D" => Some(egui::Key::D),
        "E" => Some(egui::Key::E),
        "F" => Some(egui::Key::F),
        "G" => Some(egui::Key::G),
        "H" => Some(egui::Key::H),
        "I" => Some(egui::Key::I),
        "J" => Some(egui::Key::J),
        "K" => Some(egui::Key::K),
        "L" => Some(egui::Key::L),
        "M" => Some(egui::Key::M),
        "N" => Some(egui::Key::N),
        "O" => Some(egui::Key::O),
        "P" => Some(egui::Key::P),
        "Q" => Some(egui::Key::Q),
        "R" => Some(egui::Key::R),
        "S" => Some(egui::Key::S),
        "T" => Some(egui::Key::T),
        "U" => Some(egui::Key::U),
        "V" => Some(egui::Key::V),
        "W" => Some(egui::Key::W),
        "X" => Some(egui::Key::X),
        "Y" => Some(egui::Key::Y),
        "Z" => Some(egui::Key::Z),
        "Esc" => Some(egui::Key::Escape),
        "Tab" => Some(egui::Key::Tab),
        "Backspace" => Some(egui::Key::Backspace),
        "Enter" => Some(egui::Key::Enter),
        "Space" => Some(egui::Key::Space),
        "Insert" => Some(egui::Key::Insert),
        "Delete" => Some(egui::Key::Delete),
        "Home" => Some(egui::Key::Home),
        "End" => Some(egui::Key::End),
        "PgUp" => Some(egui::Key::PageUp),
        "PgDn" => Some(egui::Key::PageDown),
        "Left" => Some(egui::Key::ArrowLeft),
        "Right" => Some(egui::Key::ArrowRight),
        "Up" => Some(egui::Key::ArrowUp),
        "Down" => Some(egui::Key::ArrowDown),
        "F1" => Some(egui::Key::F1),
        "F2" => Some(egui::Key::F2),
        "F3" => Some(egui::Key::F3),
        "F4" => Some(egui::Key::F4),
        "F5" => Some(egui::Key::F5),
        "F6" => Some(egui::Key::F6),
        "F7" => Some(egui::Key::F7),
        "F8" => Some(egui::Key::F8),
        "F9" => Some(egui::Key::F9),
        "F10" => Some(egui::Key::F10),
        "F11" => Some(egui::Key::F11),
        "F12" => Some(egui::Key::F12),
        _ => None,
    }
}

pub struct ShortcutGroup {
    pub name: &'static str,
    pub entries: Vec<(AppAction, Shortcut)>,
}

pub struct ShortcutRegistry {
    bindings: Vec<(Shortcut, AppAction)>,
    labels: HashMap<AppAction, String>,
}

impl ShortcutRegistry {
    pub fn new() -> Self {
        let config = KeybindingsConfig::load();
        let bindings = config.to_shortcut_pairs();

        // Fall back to defaults if no valid bindings were loaded
        let bindings = if bindings.is_empty() {
            Self::default_bindings()
        } else {
            bindings
        };

        let labels: HashMap<AppAction, String> =
            bindings.iter().map(|(s, a)| (*a, s.label())).collect();

        ShortcutRegistry { bindings, labels }
    }

    pub fn find_shortcut(&self, action: AppAction) -> Option<&Shortcut> {
        self.bindings
            .iter()
            .find(|(_, a)| *a == action)
            .map(|(s, _)| s)
    }

    fn default_bindings() -> Vec<(Shortcut, AppAction)> {
        vec![
            // Panel navigation
            (Shortcut::cs(egui::Key::B), AppAction::ToggleLeftSidebar),
            (Shortcut::cs(egui::Key::E), AppAction::ToggleRightSidebar),
            (Shortcut::cs(egui::Key::Backtick), AppAction::FocusTerminal),
            // Tab management
            (Shortcut::cs(egui::Key::T), AppAction::NewTerminalTab),
            (Shortcut::cs(egui::Key::W), AppAction::CloseCurrentPane),
            (Shortcut::cs(egui::Key::Num1), AppAction::SwitchToTab1),
            (Shortcut::cs(egui::Key::Num2), AppAction::SwitchToTab2),
            (Shortcut::cs(egui::Key::Num3), AppAction::SwitchToTab3),
            (Shortcut::cs(egui::Key::Num4), AppAction::SwitchToTab4),
            (Shortcut::cs(egui::Key::Num5), AppAction::SwitchToTab5),
            (Shortcut::cs(egui::Key::Num6), AppAction::SwitchToTab6),
            (Shortcut::cs(egui::Key::Num7), AppAction::SwitchToTab7),
            (Shortcut::cs(egui::Key::Num8), AppAction::SwitchToTab8),
            (Shortcut::cs(egui::Key::Num9), AppAction::SwitchToTab9),
            (Shortcut::cs(egui::Key::OpenBracket), AppAction::PreviousTab),
            (Shortcut::cs(egui::Key::CloseBracket), AppAction::NextTab),
            // Pane splits
            (
                Shortcut::cs(egui::Key::Backslash),
                AppAction::SplitHorizontal,
            ),
            (Shortcut::cs(egui::Key::Minus), AppAction::SplitVertical),
            // Workspace
            (Shortcut::cs(egui::Key::Comma), AppAction::OpenSettings),
            (Shortcut::cs(egui::Key::PageDown), AppAction::NextWorkspace),
            (Shortcut::cs(egui::Key::PageUp), AppAction::PrevWorkspace),
            // Right panel tabs
            (Shortcut::cs(egui::Key::D), AppAction::RightTabDirectory),
            (Shortcut::cs(egui::Key::G), AppAction::RightTabGitDiff),
            (Shortcut::cs(egui::Key::J), AppAction::ToggleNotes),
            // Session
            (Shortcut::cs(egui::Key::K), AppAction::DuplicateSession),
            // Terminal
            (Shortcut::cs(egui::Key::C), AppAction::CopySelection),
            // Search
            (Shortcut::cs(egui::Key::F), AppAction::FocusSessionSearch),
            (Shortcut::cs(egui::Key::P), AppAction::CommandPalette),
            // Help
            (
                Shortcut::cs(egui::Key::Slash),
                AppAction::ToggleShortcutHelp,
            ),
            // Quick Switcher
            (Shortcut::cs(egui::Key::Space), AppAction::OpenQuickSwitcher),
            // Terminal search (Ctrl+F)
            (
                Shortcut {
                    ctrl: true,
                    shift: false,
                    alt: false,
                    key: egui::Key::F,
                },
                AppAction::SearchTerminal,
            ),
            // Global search across all sessions
            (Shortcut::cs(egui::Key::N), AppAction::SearchAllSessions),
            // Pane zoom
            (Shortcut::cs(egui::Key::Z), AppAction::ZoomPane),
        ]
    }

    pub fn match_event(&self, key: &egui::Key, mods: &egui::Modifiers) -> Option<AppAction> {
        self.bindings
            .iter()
            .find(|(s, _)| s.matches(key, mods))
            .map(|(_, a)| *a)
    }

    pub fn label_for(&self, action: AppAction) -> Option<&str> {
        self.labels.get(&action).map(|s| s.as_str())
    }

    pub fn groups(&self) -> Vec<ShortcutGroup> {
        let lookup = |action: AppAction| -> Option<Shortcut> {
            self.bindings
                .iter()
                .find(|(_, a)| *a == action)
                .map(|(s, _)| s.clone())
        };

        let group_defs: &[(&str, &[AppAction])] = &[
            (
                "Panel Navigation",
                &[
                    AppAction::ToggleLeftSidebar,
                    AppAction::ToggleRightSidebar,
                    AppAction::FocusTerminal,
                ],
            ),
            (
                "Tab Management",
                &[
                    AppAction::NewTerminalTab,
                    AppAction::CloseCurrentPane,
                    AppAction::PreviousTab,
                    AppAction::NextTab,
                    AppAction::SwitchToTab1,
                    AppAction::SwitchToTab9,
                ],
            ),
            (
                "Pane Splits",
                &[
                    AppAction::SplitHorizontal,
                    AppAction::SplitVertical,
                    AppAction::ZoomPane,
                ],
            ),
            (
                "Workspace",
                &[
                    AppAction::OpenQuickSwitcher,
                    AppAction::OpenSettings,
                    AppAction::NextWorkspace,
                    AppAction::PrevWorkspace,
                ],
            ),
            (
                "Right Panel",
                &[AppAction::RightTabGitDiff, AppAction::ToggleNotes],
            ),
            (
                "Session",
                &[AppAction::DuplicateSession, AppAction::CopySelection],
            ),
            (
                "Search",
                &[
                    AppAction::FocusSessionSearch,
                    AppAction::RightTabDirectory,
                    AppAction::SearchTerminal,
                    AppAction::SearchAllSessions,
                ],
            ),
            (
                "Help",
                &[AppAction::ToggleShortcutHelp, AppAction::CommandPalette],
            ),
        ];

        group_defs
            .iter()
            .map(|(name, actions)| ShortcutGroup {
                name,
                entries: actions
                    .iter()
                    .filter_map(|&action| lookup(action).map(|s| (action, s)))
                    .collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_ctrl_shift_b() {
        let reg = ShortcutRegistry::new();
        let mods = egui::Modifiers {
            alt: false,
            ctrl: true,
            shift: true,
            mac_cmd: false,
            command: false,
        };
        assert_eq!(
            reg.match_event(&egui::Key::B, &mods),
            Some(AppAction::ToggleLeftSidebar)
        );
    }

    #[test]
    fn no_match_without_shift() {
        let reg = ShortcutRegistry::new();
        let mods = egui::Modifiers {
            alt: false,
            ctrl: true,
            shift: false,
            mac_cmd: false,
            command: false,
        };
        assert_eq!(reg.match_event(&egui::Key::B, &mods), None);
    }

    #[test]
    fn label_generation() {
        let reg = ShortcutRegistry::new();
        let expected_toggle = if cfg!(target_os = "macos") {
            "\u{2318}+\u{21E7}+B"
        } else {
            "Ctrl+Shift+B"
        };
        assert_eq!(
            reg.label_for(AppAction::ToggleLeftSidebar),
            Some(expected_toggle)
        );
        let expected_split = if cfg!(target_os = "macos") {
            "\u{2318}+\u{21E7}+\\"
        } else {
            "Ctrl+Shift+\\"
        };
        assert_eq!(
            reg.label_for(AppAction::SplitHorizontal),
            Some(expected_split)
        );
    }

    #[test]
    fn tab_index() {
        assert_eq!(AppAction::SwitchToTab1.tab_index(), Some(0));
        assert_eq!(AppAction::SwitchToTab9.tab_index(), Some(8));
        assert_eq!(AppAction::NewTerminalTab.tab_index(), None);
    }

    #[test]
    fn all_actions_have_labels() {
        let reg = ShortcutRegistry::new();
        let actions = [
            AppAction::ToggleLeftSidebar,
            AppAction::ToggleRightSidebar,
            AppAction::FocusTerminal,
            AppAction::NewTerminalTab,
            AppAction::CloseCurrentPane,
            AppAction::SplitHorizontal,
            AppAction::SplitVertical,
            AppAction::OpenSettings,
            AppAction::NextWorkspace,
            AppAction::PrevWorkspace,
            AppAction::RightTabDirectory,
            AppAction::RightTabGitDiff,
            AppAction::ToggleNotes,
            AppAction::DuplicateSession,
            AppAction::FocusSessionSearch,
            AppAction::ToggleShortcutHelp,
            AppAction::OpenQuickSwitcher,
            AppAction::SearchTerminal,
            AppAction::SearchAllSessions,
        ];
        for a in actions {
            assert!(reg.label_for(a).is_some(), "Missing label for {:?}", a);
        }
    }

    #[test]
    fn groups_cover_main_actions() {
        let reg = ShortcutRegistry::new();
        let groups = reg.groups();
        let total_entries: usize = groups.iter().map(|g| g.entries.len()).sum();
        assert!(
            total_entries >= 15,
            "Expected at least 15 entries in help groups"
        );
    }
}
