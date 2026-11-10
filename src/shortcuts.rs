#![allow(dead_code)]
use std::collections::HashMap;

use crate::keybindings::KeybindingsConfig;

#[cfg(target_os = "macos")]
fn os_reserved_shortcuts() -> Vec<Shortcut> {
    crate::macos_hotkeys::reserved_shortcuts()
}

#[cfg(not(target_os = "macos"))]
fn os_reserved_shortcuts() -> Vec<Shortcut> {
    Vec::new()
}

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

    // File finder
    OpenFileFinder,

    // Session history
    ReopenClosedSession,

    // Editor group navigation
    FocusNextGroup,
    FocusPrevGroup,
    FocusGroupUp,
    FocusGroupDown,
    MoveTabToNextGroup,
    MoveTabToPrevGroup,
    MoveTabToUpGroup,
    MoveTabToDownGroup,

    // File explorer
    RevealInExplorer,

    // Window
    ToggleFullscreen,
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
            Self::OpenFileFinder => "open_file_finder",
            Self::ReopenClosedSession => "reopen_closed_session",
            Self::FocusNextGroup => "focus_next_group",
            Self::FocusPrevGroup => "focus_prev_group",
            Self::FocusGroupUp => "focus_group_up",
            Self::FocusGroupDown => "focus_group_down",
            Self::MoveTabToNextGroup => "move_tab_to_next_group",
            Self::MoveTabToPrevGroup => "move_tab_to_prev_group",
            Self::MoveTabToUpGroup => "move_tab_to_up_group",
            Self::MoveTabToDownGroup => "move_tab_to_down_group",
            Self::RevealInExplorer => "reveal_in_explorer",
            Self::ToggleFullscreen => "toggle_fullscreen",
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
            "open_file_finder" => Some(Self::OpenFileFinder),
            "reopen_closed_session" => Some(Self::ReopenClosedSession),
            "focus_next_group" => Some(Self::FocusNextGroup),
            "focus_prev_group" => Some(Self::FocusPrevGroup),
            "focus_group_up" => Some(Self::FocusGroupUp),
            "focus_group_down" => Some(Self::FocusGroupDown),
            "move_tab_to_next_group" => Some(Self::MoveTabToNextGroup),
            "move_tab_to_prev_group" => Some(Self::MoveTabToPrevGroup),
            "move_tab_to_up_group" => Some(Self::MoveTabToUpGroup),
            "move_tab_to_down_group" => Some(Self::MoveTabToDownGroup),
            "reveal_in_explorer" => Some(Self::RevealInExplorer),
            "toggle_fullscreen" => Some(Self::ToggleFullscreen),
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
            Self::OpenFileFinder => "Open file finder",
            Self::ReopenClosedSession => "Reopen closed session",
            Self::FocusNextGroup => "Focus next group",
            Self::FocusPrevGroup => "Focus previous group",
            Self::FocusGroupUp => "Focus group above",
            Self::FocusGroupDown => "Focus group below",
            Self::MoveTabToNextGroup => "Move tab to next group",
            Self::MoveTabToPrevGroup => "Move tab to previous group",
            Self::MoveTabToUpGroup => "Move tab to group above",
            Self::MoveTabToDownGroup => "Move tab to group below",
            Self::RevealInExplorer => "Reveal file in explorer",
            Self::ToggleFullscreen => "Toggle full screen",
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

    /// Actions that stay available while a modal or dialog is open, and
    /// regardless of which widget has keyboard focus.
    pub fn is_global(self) -> bool {
        matches!(
            self,
            Self::ToggleShortcutHelp
                | Self::OpenSettings
                | Self::FocusSessionSearch
                | Self::CommandPalette
                | Self::RightTabDirectory
                | Self::OpenQuickSwitcher
                | Self::NewTerminalTab
                | Self::ZoomPane
                | Self::SearchAllSessions
                | Self::ReopenClosedSession
                | Self::OpenFileFinder
                | Self::SearchTerminal
                | Self::ToggleFullscreen
        )
    }
}

const IS_MAC: bool = cfg!(target_os = "macos");

// macOS modifier glyphs, in Apple's display order (Control, Option, Shift, Command).
const MAC_CONTROL: &str = "\u{2303}";
const MAC_OPTION: &str = "\u{2325}";
const MAC_SHIFT: &str = "\u{21E7}";
const MAC_COMMAND: &str = "\u{2318}";

const PC_CTRL: &str = "Ctrl";
const PC_ALT: &str = "Alt";
const PC_SHIFT: &str = "Shift";
const PC_SEPARATOR: &str = "+";

/// Display name of the Alt/Option modifier, for hints written outside the registry.
pub fn alt_modifier_label() -> &'static str {
    if IS_MAC {
        MAC_OPTION
    } else {
        "Alt+"
    }
}

/// egui reports the shifted symbol for some keys; bindings store the base key.
fn unshifted_key(key: egui::Key) -> egui::Key {
    match key {
        egui::Key::Questionmark => egui::Key::Slash,
        egui::Key::Pipe => egui::Key::Backslash,
        other => other,
    }
}

/// A key chord.
///
/// `ctrl` is the platform's primary modifier: Command on macOS, Ctrl
/// elsewhere. `control` is the physical Control key, needed on macOS for the
/// few standard chords that combine it with Command (such as Control-Command-F
/// for full screen). Off macOS, both flags mean Ctrl.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
    pub key: egui::Key,
}

impl Shortcut {
    /// The bare key with no modifiers.
    pub const fn key(key: egui::Key) -> Self {
        Shortcut {
            ctrl: false,
            shift: false,
            alt: false,
            control: false,
            key,
        }
    }

    /// The platform's primary modifier plus `key`.
    pub const fn primary(key: egui::Key) -> Self {
        Self::key(key).with_primary()
    }

    pub const fn with_primary(mut self) -> Self {
        self.ctrl = true;
        self
    }

    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }

    pub const fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    pub const fn with_control(mut self) -> Self {
        self.control = true;
        self
    }

    pub fn matches(&self, key: &egui::Key, mods: &egui::Modifiers) -> bool {
        self.matches_on(key, mods, IS_MAC)
    }

    /// Exact match: the chord shown in the UI is the only one that fires, so
    /// the label can never disagree with what actually works.
    fn matches_on(&self, key: &egui::Key, mods: &egui::Modifiers, is_mac: bool) -> bool {
        if unshifted_key(*key) != self.key || mods.shift != self.shift || mods.alt != self.alt {
            return false;
        }
        if is_mac {
            mods.mac_cmd == self.ctrl && mods.ctrl == self.control
        } else {
            !mods.mac_cmd && mods.ctrl == (self.ctrl || self.control)
        }
    }

    pub fn label(&self) -> String {
        self.label_on(IS_MAC)
    }

    fn label_on(&self, is_mac: bool) -> String {
        if is_mac {
            let mut label = String::new();
            for (held, glyph) in [
                (self.control, MAC_CONTROL),
                (self.alt, MAC_OPTION),
                (self.shift, MAC_SHIFT),
                (self.ctrl, MAC_COMMAND),
            ] {
                if held {
                    label.push_str(glyph);
                }
            }
            label.push_str(display_key_name(self.key, is_mac));
            label
        } else {
            let mut parts = Vec::new();
            if self.ctrl || self.control {
                parts.push(PC_CTRL);
            }
            if self.alt {
                parts.push(PC_ALT);
            }
            if self.shift {
                parts.push(PC_SHIFT);
            }
            parts.push(display_key_name(self.key, is_mac));
            parts.join(PC_SEPARATOR)
        }
    }
}

/// Key name for display. macOS uses its standard symbols for keys that have
/// them; the persisted name (`key_name`) never changes.
fn display_key_name(key: egui::Key, is_mac: bool) -> &'static str {
    if !is_mac {
        return key_name(key);
    }
    match key {
        egui::Key::Enter => "\u{23CE}",
        egui::Key::ArrowLeft => "\u{2190}",
        egui::Key::ArrowUp => "\u{2191}",
        egui::Key::ArrowRight => "\u{2192}",
        egui::Key::ArrowDown => "\u{2193}",
        other => key_name(other),
    }
}

// ── Default keymaps ─────────────────────────────────────────────────────────

/// The default bindings for this platform: the single source of truth.
pub fn default_bindings() -> Vec<(Shortcut, AppAction)> {
    if IS_MAC {
        mac_default_bindings()
    } else {
        standard_default_bindings()
    }
}

fn tab_bindings(chord: fn(egui::Key) -> Shortcut) -> Vec<(Shortcut, AppAction)> {
    use egui::Key::*;
    [
        (Num1, AppAction::SwitchToTab1),
        (Num2, AppAction::SwitchToTab2),
        (Num3, AppAction::SwitchToTab3),
        (Num4, AppAction::SwitchToTab4),
        (Num5, AppAction::SwitchToTab5),
        (Num6, AppAction::SwitchToTab6),
        (Num7, AppAction::SwitchToTab7),
        (Num8, AppAction::SwitchToTab8),
        (Num9, AppAction::SwitchToTab9),
    ]
    .into_iter()
    .map(|(key, action)| (chord(key), action))
    .collect()
}

/// Windows and Linux: Ctrl+Shift chords, leaving plain Ctrl to the shell.
fn standard_default_bindings() -> Vec<(Shortcut, AppAction)> {
    use egui::Key::*;
    let cs = |key| Shortcut::primary(key).with_shift();
    let cas = |key| Shortcut::primary(key).with_alt();
    let mut bindings = vec![
        (cs(B), AppAction::ToggleLeftSidebar),
        (cs(E), AppAction::ToggleRightSidebar),
        (cs(Backtick), AppAction::FocusTerminal),
        (cs(N), AppAction::NewTerminalTab),
        (cs(W), AppAction::CloseCurrentPane),
        (cs(OpenBracket), AppAction::PreviousTab),
        (cs(CloseBracket), AppAction::NextTab),
        (cs(Backslash), AppAction::SplitHorizontal),
        (cs(Minus), AppAction::SplitVertical),
        (cs(Comma), AppAction::OpenSettings),
        (cs(PageDown), AppAction::NextWorkspace),
        (cs(PageUp), AppAction::PrevWorkspace),
        (cs(D), AppAction::RightTabDirectory),
        (cs(G), AppAction::RightTabGitDiff),
        (cs(J), AppAction::ToggleNotes),
        (cs(K), AppAction::DuplicateSession),
        (cs(C), AppAction::CopySelection),
        (cs(F), AppAction::FocusSessionSearch),
        (cs(P), AppAction::CommandPalette),
        (Shortcut::primary(P), AppAction::OpenFileFinder),
        (cs(Slash), AppAction::ToggleShortcutHelp),
        (cs(Space), AppAction::OpenQuickSwitcher),
        (Shortcut::primary(F), AppAction::SearchTerminal),
        (cs(T), AppAction::SearchAllSessions),
        (cs(Z), AppAction::ZoomPane),
        (cs(R), AppAction::ReopenClosedSession),
        (cas(ArrowRight), AppAction::FocusNextGroup),
        (cas(ArrowLeft), AppAction::FocusPrevGroup),
        (cas(ArrowDown), AppAction::FocusGroupDown),
        (cas(ArrowUp), AppAction::FocusGroupUp),
        (cas(ArrowRight).with_shift(), AppAction::MoveTabToNextGroup),
        (cas(ArrowLeft).with_shift(), AppAction::MoveTabToPrevGroup),
        (cas(ArrowDown).with_shift(), AppAction::MoveTabToDownGroup),
        (cas(ArrowUp).with_shift(), AppAction::MoveTabToUpGroup),
        // VS Code's "Reveal in Explorer"; Ctrl+Shift+R already reopens sessions.
        (
            Shortcut::key(R).with_shift().with_alt(),
            AppAction::RevealInExplorer,
        ),
        (Shortcut::key(F11), AppAction::ToggleFullscreen),
    ];
    bindings.extend(tab_bindings(cs));
    bindings
}

/// macOS: Command chords in the style of Terminal, iTerm2, VS Code and
/// browsers, chosen to avoid every shortcut macOS reserves by default.
/// Command never reaches the shell, so these never take keys from it.
fn mac_default_bindings() -> Vec<(Shortcut, AppAction)> {
    use egui::Key::*;
    let cmd = Shortcut::primary;
    let shift_cmd = |key| Shortcut::primary(key).with_shift();
    let opt_cmd = |key| Shortcut::primary(key).with_alt();
    let ctrl_cmd = |key| Shortcut::primary(key).with_control();
    let mut bindings = vec![
        (cmd(B), AppAction::ToggleLeftSidebar),
        (opt_cmd(B), AppAction::ToggleRightSidebar),
        (
            Shortcut::key(Backtick).with_control(),
            AppAction::FocusTerminal,
        ),
        (cmd(T), AppAction::NewTerminalTab),
        (cmd(W), AppAction::CloseCurrentPane),
        (shift_cmd(OpenBracket), AppAction::PreviousTab),
        (shift_cmd(CloseBracket), AppAction::NextTab),
        (cmd(D), AppAction::SplitHorizontal),
        (shift_cmd(D), AppAction::SplitVertical),
        (cmd(Comma), AppAction::OpenSettings),
        (ctrl_cmd(CloseBracket), AppAction::NextWorkspace),
        (ctrl_cmd(OpenBracket), AppAction::PrevWorkspace),
        (shift_cmd(E), AppAction::RightTabDirectory),
        (shift_cmd(G), AppAction::RightTabGitDiff),
        (shift_cmd(J), AppAction::ToggleNotes),
        (opt_cmd(T), AppAction::DuplicateSession),
        (cmd(C), AppAction::CopySelection),
        (shift_cmd(L), AppAction::FocusSessionSearch),
        (shift_cmd(P), AppAction::CommandPalette),
        (cmd(P), AppAction::OpenFileFinder),
        (cmd(Slash), AppAction::ToggleShortcutHelp),
        (shift_cmd(O), AppAction::OpenQuickSwitcher),
        (cmd(F), AppAction::SearchTerminal),
        (opt_cmd(F), AppAction::SearchAllSessions),
        (shift_cmd(Enter), AppAction::ZoomPane),
        (shift_cmd(T), AppAction::ReopenClosedSession),
        (opt_cmd(ArrowRight), AppAction::FocusNextGroup),
        (opt_cmd(ArrowLeft), AppAction::FocusPrevGroup),
        (opt_cmd(ArrowDown), AppAction::FocusGroupDown),
        (opt_cmd(ArrowUp), AppAction::FocusGroupUp),
        (
            opt_cmd(ArrowRight).with_shift(),
            AppAction::MoveTabToNextGroup,
        ),
        (
            opt_cmd(ArrowLeft).with_shift(),
            AppAction::MoveTabToPrevGroup,
        ),
        (
            opt_cmd(ArrowDown).with_shift(),
            AppAction::MoveTabToDownGroup,
        ),
        (opt_cmd(ArrowUp).with_shift(), AppAction::MoveTabToUpGroup),
        (opt_cmd(R), AppAction::RevealInExplorer),
        (ctrl_cmd(F), AppAction::ToggleFullscreen),
    ];
    bindings.extend(tab_bindings(cmd));
    bindings
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
    /// Chords the operating system takes before the app sees them.
    os_reserved: Vec<Shortcut>,
}

impl ShortcutRegistry {
    pub fn new() -> Self {
        let config = KeybindingsConfig::load();
        let bindings = config.to_shortcut_pairs();

        // Fall back to defaults if no valid bindings were loaded
        let bindings = if bindings.is_empty() {
            default_bindings()
        } else {
            bindings
        };
        let registry = Self::with_bindings(bindings, os_reserved_shortcuts());
        registry.log_os_conflicts();
        registry
    }

    fn log_os_conflicts(&self) {
        log::info!(
            "{} system keyboard shortcuts are reserved by the OS",
            self.os_reserved.len()
        );
        for (shortcut, action) in &self.bindings {
            if self.os_reserved.contains(shortcut) {
                log::warn!(
                    "shortcut {} for {} is taken by the OS",
                    shortcut.label(),
                    action.name()
                );
            }
        }
    }

    fn with_bindings(bindings: Vec<(Shortcut, AppAction)>, os_reserved: Vec<Shortcut>) -> Self {
        let labels = bindings.iter().map(|(s, a)| (*a, s.label())).collect();
        ShortcutRegistry {
            bindings,
            labels,
            os_reserved,
        }
    }

    /// True when the OS takes this action's chord, so pressing it never
    /// reaches the app. The UI flags these instead of advertising them.
    pub fn is_os_reserved(&self, action: AppAction) -> bool {
        self.find_shortcut(action)
            .is_some_and(|shortcut| self.os_reserved.contains(shortcut))
    }

    pub fn find_shortcut(&self, action: AppAction) -> Option<&Shortcut> {
        self.bindings
            .iter()
            .find(|(_, a)| *a == action)
            .map(|(s, _)| s)
    }

    pub fn match_event(&self, key: &egui::Key, mods: &egui::Modifiers) -> Option<AppAction> {
        self.bindings
            .iter()
            .find(|(s, _)| s.matches(key, mods))
            .map(|(_, a)| *a)
    }

    /// Removes the first pressed key whose binding satisfies `wanted`.
    pub fn consume_where(
        &self,
        input: &mut egui::InputState,
        wanted: impl Fn(AppAction) -> bool,
    ) -> Option<AppAction> {
        let index = input
            .events
            .iter()
            .position(|event| self.pressed_action(event).is_some_and(&wanted))?;
        let event = input.events.remove(index);
        self.pressed_action(&event)
    }

    fn pressed_action(&self, event: &egui::Event) -> Option<AppAction> {
        match event {
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => self.match_event(key, modifiers),
            _ => None,
        }
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
                    AppAction::ToggleFullscreen,
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
                    AppAction::FocusNextGroup,
                    AppAction::FocusPrevGroup,
                    AppAction::FocusGroupUp,
                    AppAction::FocusGroupDown,
                    AppAction::MoveTabToNextGroup,
                    AppAction::MoveTabToPrevGroup,
                    AppAction::MoveTabToUpGroup,
                    AppAction::MoveTabToDownGroup,
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
                &[
                    AppAction::RightTabGitDiff,
                    AppAction::ToggleNotes,
                    AppAction::RevealInExplorer,
                ],
            ),
            (
                "Session",
                &[
                    AppAction::DuplicateSession,
                    AppAction::CopySelection,
                    AppAction::ReopenClosedSession,
                ],
            ),
            (
                "Search",
                &[
                    AppAction::FocusSessionSearch,
                    AppAction::RightTabDirectory,
                    AppAction::SearchTerminal,
                    AppAction::SearchAllSessions,
                    AppAction::OpenFileFinder,
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

    const MAC: bool = true;
    const OTHER: bool = false;

    /// Every action, so tables can be checked for completeness.
    const ALL_ACTIONS: &[AppAction] = &[
        AppAction::ToggleLeftSidebar,
        AppAction::ToggleRightSidebar,
        AppAction::FocusTerminal,
        AppAction::NewTerminalTab,
        AppAction::CloseCurrentPane,
        AppAction::SwitchToTab1,
        AppAction::SwitchToTab2,
        AppAction::SwitchToTab3,
        AppAction::SwitchToTab4,
        AppAction::SwitchToTab5,
        AppAction::SwitchToTab6,
        AppAction::SwitchToTab7,
        AppAction::SwitchToTab8,
        AppAction::SwitchToTab9,
        AppAction::PreviousTab,
        AppAction::NextTab,
        AppAction::SplitHorizontal,
        AppAction::SplitVertical,
        AppAction::OpenSettings,
        AppAction::NextWorkspace,
        AppAction::PrevWorkspace,
        AppAction::RightTabDirectory,
        AppAction::RightTabGitDiff,
        AppAction::ToggleNotes,
        AppAction::DuplicateSession,
        AppAction::CopySelection,
        AppAction::FocusSessionSearch,
        AppAction::FocusFileSearch,
        AppAction::ToggleShortcutHelp,
        AppAction::OpenQuickSwitcher,
        AppAction::SearchTerminal,
        AppAction::SearchAllSessions,
        AppAction::ZoomPane,
        AppAction::CommandPalette,
        AppAction::OpenFileFinder,
        AppAction::ReopenClosedSession,
        AppAction::FocusNextGroup,
        AppAction::FocusPrevGroup,
        AppAction::FocusGroupUp,
        AppAction::FocusGroupDown,
        AppAction::MoveTabToNextGroup,
        AppAction::MoveTabToPrevGroup,
        AppAction::MoveTabToUpGroup,
        AppAction::MoveTabToDownGroup,
        AppAction::RevealInExplorer,
        AppAction::ToggleFullscreen,
    ];

    /// Actions deliberately shipped without a default chord.
    const UNBOUND_BY_DEFAULT: &[AppAction] = &[AppAction::FocusFileSearch];

    /// Shortcuts macOS reserves by default (Spotlight, app and window
    /// switching, hide, minimize, quit, screenshots, Dock, character viewer,
    /// dictionary lookup and the Help menu search).
    fn macos_system_chords() -> Vec<Shortcut> {
        use egui::Key::*;
        let cmd = Shortcut::primary;
        let shift_cmd = |k| cmd(k).with_shift();
        let opt_cmd = |k| cmd(k).with_alt();
        let ctrl_cmd = |k| cmd(k).with_control();
        vec![
            cmd(Space),
            cmd(Tab),
            cmd(Backtick),
            cmd(H),
            cmd(M),
            cmd(Q),
            opt_cmd(H),
            opt_cmd(D),
            opt_cmd(Escape),
            opt_cmd(Space),
            ctrl_cmd(Space),
            ctrl_cmd(D),
            ctrl_cmd(Q),
            shift_cmd(Num3),
            shift_cmd(Num4),
            shift_cmd(Num5),
            shift_cmd(Num6),
            shift_cmd(Slash),
            shift_cmd(Space),
            shift_cmd(Tab),
            shift_cmd(Q),
        ]
    }

    fn mods_for(shortcut: &Shortcut, is_mac: bool) -> egui::Modifiers {
        let mut mods = egui::Modifiers {
            shift: shortcut.shift,
            alt: shortcut.alt,
            ..Default::default()
        };
        if is_mac {
            mods.mac_cmd = shortcut.ctrl;
            mods.command = shortcut.ctrl;
            mods.ctrl = shortcut.control;
        } else {
            mods.ctrl = shortcut.ctrl || shortcut.control;
            mods.command = mods.ctrl;
        }
        mods
    }

    fn table(is_mac: bool) -> Vec<(Shortcut, AppAction)> {
        if is_mac {
            mac_default_bindings()
        } else {
            standard_default_bindings()
        }
    }

    // ── Default tables ─────────────────────────────────────────────────────

    #[test]
    fn every_action_has_a_default_on_both_platforms() {
        for is_mac in [MAC, OTHER] {
            let bound: Vec<AppAction> = table(is_mac).into_iter().map(|(_, a)| a).collect();
            for action in ALL_ACTIONS {
                if UNBOUND_BY_DEFAULT.contains(action) {
                    continue;
                }
                assert!(bound.contains(action), "{action:?} unbound (mac={is_mac})");
            }
        }
    }

    #[test]
    fn no_two_actions_share_a_chord() {
        for is_mac in [MAC, OTHER] {
            let bindings = table(is_mac);
            for (i, (shortcut, action)) in bindings.iter().enumerate() {
                for (other, other_action) in &bindings[i + 1..] {
                    assert_ne!(
                        shortcut, other,
                        "{action:?} and {other_action:?} share a chord (mac={is_mac})"
                    );
                }
            }
        }
    }

    #[test]
    fn no_action_is_bound_twice() {
        for is_mac in [MAC, OTHER] {
            let bindings = table(is_mac);
            for (i, (_, action)) in bindings.iter().enumerate() {
                assert!(
                    !bindings[i + 1..].iter().any(|(_, a)| a == action),
                    "{action:?} bound twice (mac={is_mac})"
                );
            }
        }
    }

    #[test]
    fn mac_defaults_avoid_macos_system_shortcuts() {
        let reserved = macos_system_chords();
        for (shortcut, action) in mac_default_bindings() {
            assert!(
                !reserved.contains(&shortcut),
                "{action:?} uses a macOS system chord {}",
                shortcut.label_on(MAC)
            );
        }
    }

    #[test]
    fn mac_defaults_never_use_plain_control_letters() {
        // Plain Control chords belong to the shell (readline, job control).
        for (shortcut, action) in mac_default_bindings() {
            let control_only = shortcut.control && !shortcut.ctrl;
            let is_letter = key_name(shortcut.key).len() == 1
                && key_name(shortcut.key)
                    .chars()
                    .all(|c| c.is_ascii_alphabetic());
            assert!(!(control_only && is_letter), "{action:?} takes a shell key");
        }
    }

    #[test]
    fn familiar_mac_chords() {
        let find = |action| {
            mac_default_bindings()
                .into_iter()
                .find(|(_, a)| *a == action)
                .map(|(s, _)| s.label_on(MAC))
        };
        assert_eq!(
            find(AppAction::NewTerminalTab).as_deref(),
            Some("\u{2318}T")
        );
        assert_eq!(
            find(AppAction::CloseCurrentPane).as_deref(),
            Some("\u{2318}W")
        );
        assert_eq!(find(AppAction::SwitchToTab1).as_deref(), Some("\u{2318}1"));
        assert_eq!(find(AppAction::OpenSettings).as_deref(), Some("\u{2318},"));
        assert_eq!(
            find(AppAction::ToggleFullscreen).as_deref(),
            Some("\u{2303}\u{2318}F")
        );
    }

    // ── Matching ───────────────────────────────────────────────────────────

    #[test]
    fn every_default_matches_its_own_chord() {
        for is_mac in [MAC, OTHER] {
            for (shortcut, action) in table(is_mac) {
                let mods = mods_for(&shortcut, is_mac);
                assert!(
                    shortcut.matches_on(&shortcut.key, &mods, is_mac),
                    "{action:?} does not match its own chord (mac={is_mac})"
                );
            }
        }
    }

    #[test]
    fn mac_matching_is_exact() {
        let new_tab = Shortcut::primary(egui::Key::T);
        let cmd = mods_for(&new_tab, MAC);
        let ctrl = egui::Modifiers::CTRL;
        let cmd_ctrl = egui::Modifiers { ctrl: true, ..cmd };
        assert!(new_tab.matches_on(&egui::Key::T, &cmd, MAC));
        assert!(!new_tab.matches_on(&egui::Key::T, &ctrl, MAC));
        assert!(!new_tab.matches_on(&egui::Key::T, &cmd_ctrl, MAC));
    }

    #[test]
    fn mac_control_command_needs_both() {
        let full = Shortcut::primary(egui::Key::F).with_control();
        let both = mods_for(&full, MAC);
        let cmd_only = mods_for(&Shortcut::primary(egui::Key::F), MAC);
        assert!(full.matches_on(&egui::Key::F, &both, MAC));
        assert!(!full.matches_on(&egui::Key::F, &cmd_only, MAC));
    }

    #[test]
    fn other_platforms_match_ctrl_and_ignore_super() {
        let finder = Shortcut::primary(egui::Key::P);
        let ctrl = mods_for(&finder, OTHER);
        let with_super = egui::Modifiers {
            mac_cmd: true,
            ..ctrl
        };
        assert!(finder.matches_on(&egui::Key::P, &ctrl, OTHER));
        assert!(!finder.matches_on(&egui::Key::P, &with_super, OTHER));
    }

    #[test]
    fn shifted_symbols_match_their_base_key() {
        let help = Shortcut::primary(egui::Key::Slash).with_shift();
        let split = Shortcut::primary(egui::Key::Backslash).with_shift();
        for is_mac in [MAC, OTHER] {
            let mods = mods_for(&help, is_mac);
            assert!(help.matches_on(&egui::Key::Questionmark, &mods, is_mac));
            assert!(split.matches_on(&egui::Key::Pipe, &mods, is_mac));
        }
    }

    // ── Labels ─────────────────────────────────────────────────────────────

    #[test]
    fn mac_labels_use_apple_order_and_symbols() {
        let all = Shortcut::primary(egui::Key::ArrowLeft)
            .with_shift()
            .with_alt()
            .with_control();
        assert_eq!(
            all.label_on(MAC),
            "\u{2303}\u{2325}\u{21E7}\u{2318}\u{2190}"
        );
        assert_eq!(
            Shortcut::primary(egui::Key::Enter)
                .with_shift()
                .label_on(MAC),
            "\u{21E7}\u{2318}\u{23CE}"
        );
    }

    #[test]
    fn standard_labels_spell_out_modifiers() {
        let split = Shortcut::primary(egui::Key::Backslash).with_shift();
        assert_eq!(split.label_on(OTHER), "Ctrl+Shift+\\");
        let reveal = Shortcut::key(egui::Key::R).with_shift().with_alt();
        assert_eq!(reveal.label_on(OTHER), "Alt+Shift+R");
        assert_eq!(Shortcut::key(egui::Key::F11).label_on(OTHER), "F11");
    }

    // ── Registry ───────────────────────────────────────────────────────────

    fn registry() -> ShortcutRegistry {
        ShortcutRegistry::with_bindings(default_bindings(), Vec::new())
    }

    fn key_event(shortcut: &Shortcut, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key: shortcut.key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: mods_for(shortcut, IS_MAC),
        }
    }

    fn chord_of(action: AppAction) -> Shortcut {
        registry().find_shortcut(action).cloned().expect("bound")
    }

    #[test]
    fn consume_removes_only_the_matching_event() {
        let quick = chord_of(AppAction::OpenQuickSwitcher);
        let mut input = egui::InputState::default();
        input.events.push(egui::Event::Text("x".into()));
        input.events.push(key_event(&quick, true));
        let action = registry().consume_where(&mut input, AppAction::is_global);
        assert_eq!(action, Some(AppAction::OpenQuickSwitcher));
        assert_eq!(input.events, vec![egui::Event::Text("x".into())]);
    }

    #[test]
    fn consume_ignores_key_release() {
        let quick = chord_of(AppAction::OpenQuickSwitcher);
        let mut input = egui::InputState::default();
        input.events.push(key_event(&quick, false));
        assert_eq!(
            registry().consume_where(&mut input, AppAction::is_global),
            None
        );
        assert_eq!(input.events.len(), 1);
    }

    #[test]
    fn consume_skips_actions_the_caller_does_not_want() {
        let sidebar = chord_of(AppAction::ToggleLeftSidebar);
        let mut input = egui::InputState::default();
        input.events.push(key_event(&sidebar, true));
        assert_eq!(
            registry().consume_where(&mut input, AppAction::is_global),
            None
        );
        assert_eq!(input.events.len(), 1);
    }

    #[test]
    fn os_reserved_actions_are_reported() {
        let quick = chord_of(AppAction::OpenQuickSwitcher);
        let reg = ShortcutRegistry::with_bindings(default_bindings(), vec![quick]);
        assert!(reg.is_os_reserved(AppAction::OpenQuickSwitcher));
        assert!(!reg.is_os_reserved(AppAction::NewTerminalTab));
        assert!(!registry().is_os_reserved(AppAction::OpenQuickSwitcher));
    }

    #[test]
    fn labels_cover_every_bound_action() {
        let reg = registry();
        for (_, action) in default_bindings() {
            assert!(
                reg.label_for(action).is_some(),
                "missing label for {action:?}"
            );
        }
    }

    #[test]
    fn help_groups_show_bound_actions() {
        let total: usize = registry().groups().iter().map(|g| g.entries.len()).sum();
        assert!(total >= 15, "expected at least 15 entries in help groups");
    }

    #[test]
    fn action_names_round_trip() {
        for action in ALL_ACTIONS {
            assert_eq!(AppAction::from_name(action.name()), Some(*action));
        }
    }

    #[test]
    fn tab_index() {
        assert_eq!(AppAction::SwitchToTab1.tab_index(), Some(0));
        assert_eq!(AppAction::SwitchToTab9.tab_index(), Some(8));
        assert_eq!(AppAction::NewTerminalTab.tab_index(), None);
    }
}
