/// Whether the OS copy key doubles as SIGINT when nothing is selected.
/// Elsewhere it is Ctrl+C; on macOS it is Cmd+C, which must never interrupt.
pub(super) const COPY_KEY_INTERRUPTS: bool = !cfg!(target_os = "macos");

/// Held while clicking to open a URL or path: Cmd on macOS (where Ctrl+click
/// means right-click), Ctrl elsewhere. Shift is reserved for selection.
pub(super) fn is_link_modifier(modifiers: &egui::Modifiers) -> bool {
    modifiers.command && !modifiers.shift
}

/// Option+letter arrives as a key press (sent as a Meta sequence) immediately
/// followed by its composed text. Only the Meta sequence may reach the shell,
/// or a stray character is typed alongside it. Other text in the same frame is
/// genuine typing and must be kept.
pub(super) fn follows_translated_alt_key(events: &[egui::Event], index: usize) -> bool {
    let previous = index.checked_sub(1).and_then(|i| events.get(i));
    matches!(
        previous,
        Some(egui::Event::Key { key, pressed: true, modifiers, .. })
            if modifiers.alt && key_to_pty_bytes(key, modifiers).is_some()
    )
}

/// Ctrl+Tab or Ctrl+Shift+Tab, pressed or released.
pub(super) fn is_ctrl_tab(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { key: egui::Key::Tab, modifiers, .. } if modifiers.ctrl || modifiers.command
    )
}

pub(super) fn key_to_pty_bytes(key: &egui::Key, modifiers: &egui::Modifiers) -> Option<Vec<u8>> {
    use egui::Key::*;
    // Cmd combinations belong to the app and the OS, never to the shell.
    if modifiers.mac_cmd {
        return None;
    }
    let ctrl = modifiers.ctrl;
    let shift = modifiers.shift;
    let alt = modifiers.alt;

    if ctrl && !shift && !alt {
        let byte: Option<u8> = match key {
            A => Some(1),
            B => Some(2),
            C => Some(3),
            D => Some(4),
            E => Some(5),
            F => Some(6),
            G => Some(7),
            H => Some(8),
            I => Some(9),
            J => Some(10),
            K => Some(11),
            L => Some(12),
            M => Some(13),
            N => Some(14),
            O => Some(15),
            P => Some(16),
            Q => Some(17),
            R => Some(18),
            S => Some(19),
            T => Some(20),
            U => Some(21),
            V => Some(22),
            W => Some(23),
            X => Some(24),
            Y => Some(25),
            Z => Some(26),
            Enter => Some(10),
            _ => None,
        };
        if let Some(b) = byte {
            return Some(vec![b]);
        }
    }

    if alt && !ctrl && !shift {
        let ch: Option<u8> = match key {
            A => Some(b'a'),
            B => Some(b'b'),
            C => Some(b'c'),
            D => Some(b'd'),
            E => Some(b'e'),
            F => Some(b'f'),
            G => Some(b'g'),
            H => Some(b'h'),
            I => Some(b'i'),
            J => Some(b'j'),
            K => Some(b'k'),
            L => Some(b'l'),
            M => Some(b'm'),
            N => Some(b'n'),
            O => Some(b'o'),
            P => Some(b'p'),
            Q => Some(b'q'),
            R => Some(b'r'),
            S => Some(b's'),
            T => Some(b't'),
            U => Some(b'u'),
            V => Some(b'v'),
            W => Some(b'w'),
            X => Some(b'x'),
            Y => Some(b'y'),
            Z => Some(b'z'),
            _ => None,
        };
        if let Some(c) = ch {
            return Some(vec![0x1b, c]);
        }
    }

    if alt && !ctrl && *key == Backspace {
        return Some(vec![0x1b, 0x7f]);
    }

    let arrow_mod: Option<u8> = match (shift, alt, ctrl) {
        (true, false, false) => Some(b'2'),
        (false, true, false) => Some(b'3'),
        (true, true, false) => Some(b'4'),
        (false, false, true) => Some(b'5'),
        (true, false, true) => Some(b'6'),
        (false, true, true) => Some(b'7'),
        (true, true, true) => Some(b'8'),
        _ => None,
    };
    if let Some(m) = arrow_mod {
        let dir: Option<u8> = match key {
            ArrowUp => Some(b'A'),
            ArrowDown => Some(b'B'),
            ArrowRight => Some(b'C'),
            ArrowLeft => Some(b'D'),
            _ => None,
        };
        if let Some(d) = dir {
            return Some(vec![0x1b, b'[', b'1', b';', m, d]);
        }
    }

    Some(match key {
        Enter => b"\r".to_vec(),
        Backspace => b"\x7f".to_vec(),
        Tab if !shift => b"\t".to_vec(),
        Tab => b"\x1b[Z".to_vec(),
        Escape => b"\x1b".to_vec(),
        ArrowUp => b"\x1b[A".to_vec(),
        ArrowDown => b"\x1b[B".to_vec(),
        ArrowRight => b"\x1b[C".to_vec(),
        ArrowLeft => b"\x1b[D".to_vec(),
        Home => b"\x1b[H".to_vec(),
        End => b"\x1b[F".to_vec(),
        PageUp => b"\x1b[5~".to_vec(),
        PageDown => b"\x1b[6~".to_vec(),
        Delete => b"\x1b[3~".to_vec(),
        Insert => b"\x1b[2~".to_vec(),
        F1 => b"\x1bOP".to_vec(),
        F2 => b"\x1bOQ".to_vec(),
        F3 => b"\x1bOR".to_vec(),
        F4 => b"\x1bOS".to_vec(),
        F5 => b"\x1b[15~".to_vec(),
        F6 => b"\x1b[17~".to_vec(),
        F7 => b"\x1b[18~".to_vec(),
        F8 => b"\x1b[19~".to_vec(),
        F9 => b"\x1b[20~".to_vec(),
        F10 => b"\x1b[21~".to_vec(),
        F11 => b"\x1b[23~".to_vec(),
        F12 => b"\x1b[24~".to_vec(),
        _ => return None,
    })
}

pub(super) fn shell_quote_path(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();

    #[cfg(target_os = "windows")]
    {
        let needs_quoting = s.contains(' ')
            || s.contains('&')
            || s.contains('(')
            || s.contains(')')
            || s.contains('^')
            || s.contains('|')
            || s.contains('$')
            || s.contains('`')
            || s.contains('{')
            || s.contains('}')
            || s.contains(';');
        if needs_quoting {
            // Use single quotes for PowerShell safety (no variable expansion)
            format!("'{}'", s.replace('\'', "''"))
        } else {
            s.into_owned()
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let needs_quoting = s.chars().any(|c| {
            matches!(
                c,
                ' ' | '\t'
                    | '!'
                    | '"'
                    | '#'
                    | '$'
                    | '&'
                    | '\''
                    | '('
                    | ')'
                    | '*'
                    | ';'
                    | '<'
                    | '>'
                    | '?'
                    | '['
                    | '\\'
                    | ']'
                    | '^'
                    | '`'
                    | '{'
                    | '|'
                    | '}'
                    | '~'
            )
        });
        if needs_quoting {
            format!("'{}'", s.replace('\'', "'\\''"))
        } else {
            s.into_owned()
        }
    }
}

pub(super) fn mouse_event_bytes(btn: u8, col: u16, row: u16, pressed: bool, sgr: bool) -> Vec<u8> {
    if sgr {
        let final_char = if pressed { b'M' } else { b'm' };
        format!(
            "\x1b[<{};{};{}{}",
            btn,
            col + 1,
            row + 1,
            final_char as char
        )
        .into_bytes()
    } else {
        let b = btn + 32;
        let raw_x = (col + 1) + 32;
        let raw_y = (row + 1) + 32;
        if raw_x > 255 || raw_y > 255 {
            log::warn!(
                "X11 mouse encoding: coordinates ({col},{row}) exceed protocol limit of 222; truncating"
            );
        }
        let x = raw_x.min(255) as u8;
        let y = raw_y.min(255) as u8;
        vec![0x1b, b'[', b'M', b, x, y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ctrl_tab_counts_as_ctrl_tab() {
        let tab = |m| key_press(egui::Key::Tab, m);
        assert!(is_ctrl_tab(&tab(egui::Modifiers::CTRL)));
        assert!(is_ctrl_tab(&tab(
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT
        )));
        assert!(!is_ctrl_tab(&tab(egui::Modifiers::NONE)), "Tab indents");
        assert!(
            !is_ctrl_tab(&tab(egui::Modifiers::SHIFT)),
            "Shift+Tab outdents"
        );
        assert!(!is_ctrl_tab(&egui::Event::Text("\t".into())));
    }

    fn key_press(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn test_only_text_after_option_key_is_dropped() {
        let events = [
            egui::Event::Text("x".into()),
            key_press(egui::Key::B, mods(false, false, true)),
            egui::Event::Text("\u{222B}".into()),
            egui::Event::Text("y".into()),
        ];
        assert!(!follows_translated_alt_key(&events, 0));
        assert!(follows_translated_alt_key(&events, 2));
        assert!(!follows_translated_alt_key(&events, 3));
    }

    #[test]
    fn test_plain_typing_keeps_its_text() {
        let events = [
            key_press(egui::Key::B, mods(false, false, false)),
            egui::Event::Text("b".into()),
        ];
        assert!(!follows_translated_alt_key(&events, 1));
    }

    #[test]
    fn test_cmd_keys_never_reach_the_shell() {
        let cmd = egui::Modifiers {
            mac_cmd: true,
            command: true,
            ..Default::default()
        };
        for key in [egui::Key::C, egui::Key::ArrowLeft, egui::Key::Enter] {
            assert_eq!(key_to_pty_bytes(&key, &cmd), None);
        }
    }

    #[test]
    fn test_link_modifier_excludes_shift() {
        let command = egui::Modifiers {
            command: true,
            ..Default::default()
        };
        let with_shift = egui::Modifiers {
            shift: true,
            ..command
        };
        assert!(is_link_modifier(&command));
        assert!(!is_link_modifier(&with_shift));
        assert!(!is_link_modifier(&egui::Modifiers::CTRL));
    }

    fn mods(ctrl: bool, shift: bool, alt: bool) -> egui::Modifiers {
        egui::Modifiers {
            alt,
            ctrl,
            shift,
            mac_cmd: false,
            command: false,
        }
    }

    // ── Ctrl+letter ────────────────────────────────────────────────────────

    #[test]
    fn test_ctrl_a() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::A, &mods(true, false, false)),
            Some(vec![1])
        );
    }

    #[test]
    fn test_ctrl_c() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::C, &mods(true, false, false)),
            Some(vec![3])
        );
    }

    #[test]
    fn test_ctrl_z() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Z, &mods(true, false, false)),
            Some(vec![26])
        );
    }

    #[test]
    fn test_ctrl_enter() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Enter, &mods(true, false, false)),
            Some(vec![10])
        );
    }

    // ── Alt+letter ─────────────────────────────────────────────────────────

    #[test]
    fn test_alt_a() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::A, &mods(false, false, true)),
            Some(vec![0x1b, b'a'])
        );
    }

    #[test]
    fn test_alt_z() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Z, &mods(false, false, true)),
            Some(vec![0x1b, b'z'])
        );
    }

    #[test]
    fn test_alt_backspace() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Backspace, &mods(false, false, true)),
            Some(vec![0x1b, 0x7f])
        );
    }

    // ── Arrow keys with modifiers ──────────────────────────────────────────

    #[test]
    fn test_arrow_shift() {
        // Shift+Up → \x1b[1;2A
        assert_eq!(
            key_to_pty_bytes(&egui::Key::ArrowUp, &mods(false, true, false)),
            Some(vec![0x1b, b'[', b'1', b';', b'2', b'A'])
        );
    }

    #[test]
    fn test_arrow_ctrl() {
        // Ctrl+Right → \x1b[1;5C
        assert_eq!(
            key_to_pty_bytes(&egui::Key::ArrowRight, &mods(true, false, false)),
            Some(vec![0x1b, b'[', b'1', b';', b'5', b'C'])
        );
    }

    #[test]
    fn test_arrow_alt() {
        // Alt+Left → \x1b[1;3D
        assert_eq!(
            key_to_pty_bytes(&egui::Key::ArrowLeft, &mods(false, false, true)),
            Some(vec![0x1b, b'[', b'1', b';', b'3', b'D'])
        );
    }

    // ── Bare keys ──────────────────────────────────────────────────────────

    #[test]
    fn test_bare_enter() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Enter, &mods(false, false, false)),
            Some(b"\r".to_vec())
        );
    }

    #[test]
    fn test_bare_tab() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Tab, &mods(false, false, false)),
            Some(b"\t".to_vec())
        );
    }

    #[test]
    fn test_shift_tab() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Tab, &mods(false, true, false)),
            Some(b"\x1b[Z".to_vec())
        );
    }

    #[test]
    fn test_escape() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Escape, &mods(false, false, false)),
            Some(b"\x1b".to_vec())
        );
    }

    // ── Function keys ──────────────────────────────────────────────────────

    #[test]
    fn test_f1() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::F1, &mods(false, false, false)),
            Some(b"\x1bOP".to_vec())
        );
    }

    #[test]
    fn test_f12() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::F12, &mods(false, false, false)),
            Some(b"\x1b[24~".to_vec())
        );
    }

    // ── Navigation keys ────────────────────────────────────────────────────

    #[test]
    fn test_home_end_page() {
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Home, &mods(false, false, false)),
            Some(b"\x1b[H".to_vec())
        );
        assert_eq!(
            key_to_pty_bytes(&egui::Key::End, &mods(false, false, false)),
            Some(b"\x1b[F".to_vec())
        );
        assert_eq!(
            key_to_pty_bytes(&egui::Key::PageUp, &mods(false, false, false)),
            Some(b"\x1b[5~".to_vec())
        );
        assert_eq!(
            key_to_pty_bytes(&egui::Key::PageDown, &mods(false, false, false)),
            Some(b"\x1b[6~".to_vec())
        );
    }

    // ── Unknown key ────────────────────────────────────────────────────────

    #[test]
    fn test_unknown_key_returns_none() {
        // Num0 with no modifiers is not mapped
        assert_eq!(
            key_to_pty_bytes(&egui::Key::Num0, &mods(false, false, false)),
            None
        );
    }

    // ── Mouse SGR mode ─────────────────────────────────────────────────────

    #[test]
    fn test_mouse_sgr_press() {
        let bytes = mouse_event_bytes(0, 5, 10, true, true);
        // \x1b[<0;6;11M
        assert_eq!(bytes, b"\x1b[<0;6;11M".to_vec());
    }

    #[test]
    fn test_mouse_sgr_release() {
        let bytes = mouse_event_bytes(0, 5, 10, false, true);
        // \x1b[<0;6;11m
        assert_eq!(bytes, b"\x1b[<0;6;11m".to_vec());
    }

    // ── Mouse X11 mode ─────────────────────────────────────────────────────

    #[test]
    fn test_mouse_x11() {
        let bytes = mouse_event_bytes(0, 5, 10, true, false);
        // btn+32=32, col+1+32=38, row+1+32=43
        assert_eq!(bytes, vec![0x1b, b'[', b'M', 32, 38, 43]);
    }

    // ── shell_quote_path ──────────────────────────────────────────────────

    #[test]
    fn test_quote_simple_path() {
        use std::path::Path;
        #[cfg(target_os = "windows")]
        assert_eq!(
            shell_quote_path(Path::new(r"C:\Users\test\file.txt")),
            r"C:\Users\test\file.txt"
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            shell_quote_path(Path::new("/home/test/file.txt")),
            "/home/test/file.txt"
        );
    }

    #[test]
    fn test_quote_path_with_spaces() {
        use std::path::Path;
        #[cfg(target_os = "windows")]
        assert_eq!(
            shell_quote_path(Path::new(r"C:\My Files\doc.txt")),
            r"'C:\My Files\doc.txt'"
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            shell_quote_path(Path::new("/home/my files/doc.txt")),
            "'/home/my files/doc.txt'"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_quote_path_with_single_quote() {
        use std::path::Path;
        assert_eq!(
            shell_quote_path(Path::new("/home/it's a file")),
            "'/home/it'\\''s a file'"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_quote_path_with_ampersand() {
        use std::path::Path;
        assert_eq!(
            shell_quote_path(Path::new(r"C:\Tom & Jerry\file.txt")),
            r"'C:\Tom & Jerry\file.txt'"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_quote_path_with_special_chars() {
        use std::path::Path;
        assert_eq!(
            shell_quote_path(Path::new("/home/test/file (1).txt")),
            "'/home/test/file (1).txt'"
        );
    }
}
