//! Zooming the whole window (every panel and pane, egui's `zoom_factor`)
//! from the keyboard, as a browser does: ⌘= or ⌘+ in, ⌘- out, ⌘0 back to
//! 100 % (Ctrl off macOS). A document pane's own zoom (⌘ / Ctrl + scroll,
//! `content_zoom`) is separate and multiplies with it.
//!
//! egui has these keys built in, but it matches them ignoring Shift and Alt,
//! and only after the frame has run: ⇧⌘- split the pane (Split vertical)
//! and zoomed the window out as well. So egui's are turned off and the keys
//! are the app's own shortcuts (`AppAction::ZoomWindow*`), which match
//! exactly, like every other chord. egui's quit key goes too where it is
//! Ctrl+Q, which belongs to the terminal (XON, many editors' quit).

use crate::shortcuts::AppAction;

/// The window's zoom as designed.
const WINDOW_ZOOM_DEFAULT: f32 = 1.0;

/// egui's quit key is ⌘Q on macOS, where it belongs to the app, and Ctrl+Q
/// elsewhere, where it belongs to the terminal.
const EGUI_QUIT_KEY_IS_THE_TERMINALS: bool = !cfg!(target_os = "macos");

/// Turns off egui's own end-of-frame shortcuts that would answer chords
/// the app or a terminal uses (see the module docs).
pub(super) fn disable_builtin_shortcuts(ctx: &egui::Context) {
    ctx.options_mut(|o| {
        o.zoom_with_keyboard = false;
        if EGUI_QUIT_KEY_IS_THE_TERMINALS {
            o.quit_shortcuts.clear();
        }
    });
}

/// Runs `action` when it is a window zoom; whether it was one.
pub(super) fn run(ctx: &egui::Context, action: AppAction) -> bool {
    match action {
        AppAction::ZoomWindowIn => egui::gui_zoom::zoom_in(ctx),
        AppAction::ZoomWindowOut => egui::gui_zoom::zoom_out(ctx),
        AppAction::ZoomWindowReset => ctx.set_zoom_factor(WINDOW_ZOOM_DEFAULT),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_test_frames::run_pass;
    use crate::shortcuts::{default_bindings, ShortcutRegistry};

    /// Modifiers as egui-winit reports `shortcut` on this platform.
    fn mods_of(shortcut: &crate::shortcuts::Shortcut) -> egui::Modifiers {
        let mut mods = egui::Modifiers {
            shift: shortcut.shift,
            alt: shortcut.alt,
            ..Default::default()
        };
        if cfg!(target_os = "macos") {
            mods.mac_cmd = shortcut.ctrl;
            mods.command = shortcut.ctrl;
            mods.ctrl = shortcut.control;
        } else {
            mods.ctrl = shortcut.ctrl || shortcut.control;
            mods.command = mods.ctrl;
        }
        mods
    }

    fn press(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
        egui::RawInput {
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        }
    }

    /// What egui itself does at the end of a frame in which `input` came and
    /// nothing took it: the zoom it leaves, and whether it closed the window.
    fn egui_reaction(ctx: &egui::Context, input: egui::RawInput) -> (f32, bool) {
        let out = run_pass(ctx, input, |_| {});
        let closed = out
            .viewport_output
            .values()
            .any(|v| v.commands.contains(&egui::ViewportCommand::Close));
        // A zoom egui asks for takes effect at the start of the next frame.
        let _ = run_pass(ctx, egui::RawInput::default(), |_| {});
        (ctx.zoom_factor(), closed)
    }

    #[test]
    fn egui_left_alone_zooms_on_the_split_chord() {
        // The bug: egui's own zoom ignores Shift, so ⇧⌘- (Split vertical)
        // zoomed out too. Shown here so the test below means something.
        let ctx = egui::Context::default();
        let split = egui::Modifiers {
            shift: true,
            ..egui::Modifiers::COMMAND
        };
        let (zoom, _) = egui_reaction(&ctx, press(egui::Key::Minus, split));
        assert!(zoom < WINDOW_ZOOM_DEFAULT, "egui's built-in zoom fired");
    }

    #[test]
    fn egui_answers_no_app_chord_once_its_own_are_off() {
        for (shortcut, action) in default_bindings() {
            let ctx = egui::Context::default();
            disable_builtin_shortcuts(&ctx);
            let (zoom, closed) = egui_reaction(&ctx, press(shortcut.key, mods_of(&shortcut)));
            assert_eq!(zoom, WINDOW_ZOOM_DEFAULT, "egui zoomed on {action:?}");
            assert!(!closed, "egui closed the window on {action:?}");
        }
    }

    #[test]
    fn ctrl_q_reaches_the_terminal_off_macos() {
        let ctx = egui::Context::default();
        disable_builtin_shortcuts(&ctx);
        let (_, closed) = egui_reaction(&ctx, press(egui::Key::Q, egui::Modifiers::COMMAND));
        assert_eq!(closed, !EGUI_QUIT_KEY_IS_THE_TERMINALS);
    }

    #[test]
    fn the_zoom_actions_zoom_the_window() {
        let ctx = egui::Context::default();
        let zoom_after = |action| {
            let mut ran = false;
            let _ = run_pass(&ctx, egui::RawInput::default(), |ctx| {
                ran = run(ctx, action)
            });
            let _ = run_pass(&ctx, egui::RawInput::default(), |_| {});
            assert!(ran, "{action:?} is a window zoom");
            ctx.zoom_factor()
        };
        assert!(zoom_after(AppAction::ZoomWindowIn) > WINDOW_ZOOM_DEFAULT);
        assert_eq!(zoom_after(AppAction::ZoomWindowReset), WINDOW_ZOOM_DEFAULT);
        assert!(zoom_after(AppAction::ZoomWindowOut) < WINDOW_ZOOM_DEFAULT);
        let mut ran = true;
        let _ = run_pass(&ctx, egui::RawInput::default(), |ctx| {
            ran = run(ctx, AppAction::SplitVertical)
        });
        assert!(!ran, "other actions are not window zooms");
    }

    #[test]
    fn the_zoom_keys_are_the_apps_and_exact() {
        let reg = ShortcutRegistry::for_tests(default_bindings());
        let primary = mods_of(&crate::shortcuts::Shortcut::primary(egui::Key::A));
        let with = |shift, alt| egui::Modifiers {
            shift,
            alt,
            ..primary
        };
        let cases = [
            (
                egui::Key::Equals,
                with(false, false),
                Some(AppAction::ZoomWindowIn),
            ),
            // ⌘+ where + is Shift+=, and a keypad or layout's own +.
            (
                egui::Key::Plus,
                with(true, false),
                Some(AppAction::ZoomWindowIn),
            ),
            (
                egui::Key::Plus,
                with(false, false),
                Some(AppAction::ZoomWindowIn),
            ),
            (
                egui::Key::Minus,
                with(false, false),
                Some(AppAction::ZoomWindowOut),
            ),
            (
                egui::Key::Num0,
                with(false, false),
                Some(AppAction::ZoomWindowReset),
            ),
            // The split chord splits, and nothing else.
            (
                egui::Key::Minus,
                with(true, false),
                Some(AppAction::SplitVertical),
            ),
            (egui::Key::Minus, with(false, true), None),
            (egui::Key::Num0, with(true, false), None),
        ];
        for (key, mods, want) in cases {
            assert_eq!(reg.match_event(&key, &mods), want, "{key:?} {mods:?}");
        }
    }
}
