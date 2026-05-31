pub(super) fn key_to_pty_bytes(key: &egui::Key, modifiers: &egui::Modifiers) -> Option<Vec<u8>> {
    use egui::Key::*;
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
        let x = ((col + 1) + 32).min(255) as u8;
        let y = ((row + 1) + 32).min(255) as u8;
        vec![0x1b, b'[', b'M', b, x, y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
