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
