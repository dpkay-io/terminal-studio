use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line},
    term::{cell::Flags, Term},
    vte::ansi::{Color, NamedColor},
};

use crate::terminal::EventProxy;

/// Extracts the full grid content (history + visible screen) as an ANSI byte stream.
///
/// The returned bytes, when fed through a VTE processor, reproduce the terminal's
/// visual content including colors and text attributes (bold, underline, etc.).
///
/// If `max_lines` is Some(n), only the last `n` lines are captured.
pub fn extract_grid_as_ansi(term: &Term<EventProxy>, max_lines: Option<usize>) -> Vec<u8> {
    let grid = term.grid();
    let cols = term.columns();
    let screen_lines = term.screen_lines();
    let history_size = grid.history_size();
    let total_lines = history_size + screen_lines;

    let lines_to_capture = max_lines.unwrap_or(total_lines).min(total_lines);
    let skip_lines = total_lines.saturating_sub(lines_to_capture);

    let start_line = -(history_size as i32) + skip_lines as i32;
    let nominal_end = screen_lines as i32;

    // Find last non-empty line to avoid trailing blank lines in the capture.
    let mut end_line = start_line;
    for line_idx in (start_line..nominal_end).rev() {
        if find_line_end(grid, line_idx, cols) > 0 {
            end_line = line_idx + 1;
            break;
        }
    }

    let actual_lines = (end_line - start_line).max(0) as usize;
    let estimated_size = actual_lines * cols * 3;
    let mut buf = Vec::with_capacity(estimated_size);

    let mut cur_fg = Color::Named(NamedColor::Foreground);
    let mut cur_bg = Color::Named(NamedColor::Background);
    let mut cur_bold = false;
    let mut cur_dim = false;
    let mut cur_underline = false;
    let mut cur_strike = false;
    let mut cur_inverse = false;

    let mut prev_empty = false;
    for line_idx in start_line..end_line {
        let line_end = find_line_end(grid, line_idx, cols);

        // Collapse runs of empty lines to a single blank line.
        if line_end == 0 {
            if !prev_empty {
                buf.extend_from_slice(b"\r\n");
            }
            prev_empty = true;
            continue;
        }
        prev_empty = false;

        for col in 0..line_end {
            let cell = &grid[Line(line_idx)][Column(col)];

            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }

            let cell_bold = cell.flags.contains(Flags::BOLD);
            let cell_dim = cell.flags.contains(Flags::DIM);
            let cell_underline = cell.flags.contains(Flags::UNDERLINE);
            let cell_strike = cell.flags.contains(Flags::STRIKEOUT);
            let cell_inverse = cell.flags.contains(Flags::INVERSE);

            let needs_sgr = cell.fg != cur_fg
                || cell.bg != cur_bg
                || cell_bold != cur_bold
                || cell_dim != cur_dim
                || cell_underline != cur_underline
                || cell_strike != cur_strike
                || cell_inverse != cur_inverse;

            if needs_sgr {
                emit_sgr(
                    &mut buf,
                    cell.fg,
                    cell.bg,
                    cell_bold,
                    cell_dim,
                    cell_underline,
                    cell_strike,
                    cell_inverse,
                );
                cur_fg = cell.fg;
                cur_bg = cell.bg;
                cur_bold = cell_bold;
                cur_dim = cell_dim;
                cur_underline = cell_underline;
                cur_strike = cell_strike;
                cur_inverse = cell_inverse;
            }

            let mut char_buf = [0u8; 4];
            let encoded = cell.c.encode_utf8(&mut char_buf);
            buf.extend_from_slice(encoded.as_bytes());
        }

        // Reset attributes at end of line if any are active
        let has_attrs = cur_bold
            || cur_dim
            || cur_underline
            || cur_strike
            || cur_inverse
            || !matches!(cur_fg, Color::Named(NamedColor::Foreground))
            || !matches!(cur_bg, Color::Named(NamedColor::Background));

        if has_attrs {
            buf.extend_from_slice(b"\x1b[0m");
            cur_fg = Color::Named(NamedColor::Foreground);
            cur_bg = Color::Named(NamedColor::Background);
            cur_bold = false;
            cur_dim = false;
            cur_underline = false;
            cur_strike = false;
            cur_inverse = false;
        }

        buf.extend_from_slice(b"\r\n");
    }

    buf
}

/// Find the rightmost non-default column in a line (trims trailing whitespace).
fn find_line_end(
    grid: &alacritty_terminal::grid::Grid<alacritty_terminal::term::cell::Cell>,
    line_idx: i32,
    cols: usize,
) -> usize {
    for col in (0..cols).rev() {
        let cell = &grid[Line(line_idx)][Column(col)];
        if cell.c != ' '
            || !matches!(cell.fg, Color::Named(NamedColor::Foreground))
            || !matches!(cell.bg, Color::Named(NamedColor::Background))
            || cell.flags.intersects(
                Flags::BOLD | Flags::DIM | Flags::UNDERLINE | Flags::STRIKEOUT | Flags::INVERSE,
            )
        {
            return col + 1;
        }
    }
    0
}

/// Emits a full SGR reset + set sequence for the given attributes.
#[allow(clippy::too_many_arguments)]
fn emit_sgr(
    buf: &mut Vec<u8>,
    fg: Color,
    bg: Color,
    bold: bool,
    dim: bool,
    underline: bool,
    strike: bool,
    inverse: bool,
) {
    buf.extend_from_slice(b"\x1b[0");

    if bold {
        buf.extend_from_slice(b";1");
    }
    if dim {
        buf.extend_from_slice(b";2");
    }
    if underline {
        buf.extend_from_slice(b";4");
    }
    if inverse {
        buf.extend_from_slice(b";7");
    }
    if strike {
        buf.extend_from_slice(b";9");
    }

    emit_color_params(buf, fg, true);
    emit_color_params(buf, bg, false);

    buf.push(b'm');
}

/// Appends SGR color parameters to the buffer.
fn emit_color_params(buf: &mut Vec<u8>, color: Color, is_fg: bool) {
    match color {
        Color::Named(named) => {
            if let Some(code) = named_color_sgr(named, is_fg) {
                buf.push(b';');
                push_u8(buf, code);
            }
        }
        Color::Indexed(idx) => {
            if is_fg {
                buf.extend_from_slice(b";38;5;");
            } else {
                buf.extend_from_slice(b";48;5;");
            }
            push_u8(buf, idx);
        }
        Color::Spec(rgb) => {
            if is_fg {
                buf.extend_from_slice(b";38;2;");
            } else {
                buf.extend_from_slice(b";48;2;");
            }
            push_u8(buf, rgb.r);
            buf.push(b';');
            push_u8(buf, rgb.g);
            buf.push(b';');
            push_u8(buf, rgb.b);
        }
    }
}

/// Maps a NamedColor to its SGR code, returning None for Foreground/Background defaults.
fn named_color_sgr(named: NamedColor, is_fg: bool) -> Option<u8> {
    let code = match named {
        NamedColor::Foreground | NamedColor::Background => return None,
        NamedColor::Black => 0,
        NamedColor::Red => 1,
        NamedColor::Green => 2,
        NamedColor::Yellow => 3,
        NamedColor::Blue => 4,
        NamedColor::Magenta => 5,
        NamedColor::Cyan => 6,
        NamedColor::White => 7,
        NamedColor::BrightBlack => 8,
        NamedColor::BrightRed => 9,
        NamedColor::BrightGreen => 10,
        NamedColor::BrightYellow => 11,
        NamedColor::BrightBlue => 12,
        NamedColor::BrightMagenta => 13,
        NamedColor::BrightCyan => 14,
        NamedColor::BrightWhite => 15,
        _ => return None,
    };

    if code < 8 {
        Some(if is_fg { 30 + code } else { 40 + code })
    } else {
        Some(if is_fg { 90 + code - 8 } else { 100 + code - 8 })
    }
}

/// Pushes a u8 as decimal ASCII digits.
fn push_u8(buf: &mut Vec<u8>, val: u8) {
    if val >= 100 {
        buf.push(b'0' + val / 100);
        buf.push(b'0' + (val / 10) % 10);
        buf.push(b'0' + val % 10);
    } else if val >= 10 {
        buf.push(b'0' + val / 10);
        buf.push(b'0' + val % 10);
    } else {
        buf.push(b'0' + val);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::Session;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    fn make_session(cols: u16, rows: u16) -> Session {
        Session::new_for_test(1, cols, rows)
    }

    fn feed_bytes(session: &mut Session, data: &[u8]) {
        let mut processor: Processor<StdSyncHandler> = Processor::new();
        processor.advance(&mut session.term, data);
    }

    #[test]
    fn empty_grid_produces_empty_output() {
        let session = make_session(80, 24);
        let result = extract_grid_as_ansi(&session.term, None);
        assert!(result.is_empty());
    }

    #[test]
    fn plain_text_roundtrip() {
        let mut session = make_session(80, 24);
        feed_bytes(&mut session, b"Hello, World!\r\n");
        let ansi = extract_grid_as_ansi(&session.term, None);
        let output = String::from_utf8_lossy(&ansi);
        assert!(output.contains("Hello, World!"));
    }

    #[test]
    fn colored_text_produces_sgr() {
        let mut session = make_session(80, 24);
        // Red foreground text
        feed_bytes(&mut session, b"\x1b[31mRed Text\x1b[0m\r\n");
        let ansi = extract_grid_as_ansi(&session.term, None);
        let output = String::from_utf8_lossy(&ansi);
        assert!(output.contains("\x1b[0;31m"));
        assert!(output.contains("Red Text"));
    }

    #[test]
    fn bold_text_produces_sgr() {
        let mut session = make_session(80, 24);
        feed_bytes(&mut session, b"\x1b[1mBold\x1b[0m\r\n");
        let ansi = extract_grid_as_ansi(&session.term, None);
        let output = String::from_utf8_lossy(&ansi);
        assert!(output.contains("\x1b[0;1m"));
        assert!(output.contains("Bold"));
    }

    #[test]
    fn trailing_whitespace_trimmed() {
        let mut session = make_session(80, 24);
        feed_bytes(&mut session, b"Hi\r\n");
        let ansi = extract_grid_as_ansi(&session.term, None);
        // Split on \r\n to get individual lines
        let text = std::str::from_utf8(&ansi).unwrap();
        let lines: Vec<&str> = text.split("\r\n").collect();
        // First line should be "Hi" only, not "Hi" + 78 spaces
        assert_eq!(lines[0], "Hi");
    }

    #[test]
    fn max_lines_caps_output() {
        let mut session = make_session(80, 24);
        for i in 0..30 {
            let line = format!("Line {}\r\n", i);
            feed_bytes(&mut session, line.as_bytes());
        }
        let ansi = extract_grid_as_ansi(&session.term, Some(5));
        let lines: Vec<&str> = std::str::from_utf8(&ansi)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .collect();
        assert!(lines.len() <= 5);
    }

    #[test]
    fn rgb_color_produces_24bit_sgr() {
        let mut session = make_session(80, 24);
        // 24-bit color: ESC[38;2;255;128;0m
        feed_bytes(&mut session, b"\x1b[38;2;255;128;0mOrange\x1b[0m\r\n");
        let ansi = extract_grid_as_ansi(&session.term, None);
        let output = String::from_utf8_lossy(&ansi);
        assert!(output.contains("38;2;255;128;0"));
        assert!(output.contains("Orange"));
    }

    #[test]
    fn indexed_color_produces_256_sgr() {
        let mut session = make_session(80, 24);
        // 256-color: ESC[38;5;202m
        feed_bytes(&mut session, b"\x1b[38;5;202mIndexed\x1b[0m\r\n");
        let ansi = extract_grid_as_ansi(&session.term, None);
        let output = String::from_utf8_lossy(&ansi);
        assert!(output.contains("38;5;202"));
        assert!(output.contains("Indexed"));
    }

    #[test]
    fn underline_and_strikethrough() {
        let mut session = make_session(80, 24);
        feed_bytes(&mut session, b"\x1b[4;9mStyled\x1b[0m\r\n");
        let ansi = extract_grid_as_ansi(&session.term, None);
        let output = String::from_utf8_lossy(&ansi);
        assert!(output.contains(";4"));
        assert!(output.contains(";9"));
        assert!(output.contains("Styled"));
    }

    #[test]
    fn wide_char_handled() {
        let mut session = make_session(80, 24);
        feed_bytes(&mut session, "日本語\r\n".as_bytes());
        let ansi = extract_grid_as_ansi(&session.term, None);
        let output = String::from_utf8_lossy(&ansi);
        assert!(output.contains("日本語"));
    }

    #[test]
    fn extract_inject_roundtrip_plain_text() {
        let mut original = make_session(80, 24);
        feed_bytes(&mut original, b"Line 1\r\nLine 2\r\nLine 3\r\n");

        let ansi = extract_grid_as_ansi(&original.term, None);

        let mut restored = make_session(80, 24);
        feed_bytes(&mut restored, &ansi);

        // Verify the text appears in the restored grid
        let restored_ansi = extract_grid_as_ansi(&restored.term, None);
        let output = String::from_utf8_lossy(&restored_ansi);
        assert!(output.contains("Line 1"));
        assert!(output.contains("Line 2"));
        assert!(output.contains("Line 3"));
    }

    #[test]
    fn extract_inject_roundtrip_colored() {
        let mut original = make_session(80, 24);
        feed_bytes(&mut original, b"\x1b[32mGreen\x1b[0m Normal\r\n");

        let ansi = extract_grid_as_ansi(&original.term, None);
        let mut restored = make_session(80, 24);
        feed_bytes(&mut restored, &ansi);

        let restored_ansi = extract_grid_as_ansi(&restored.term, None);
        let output = String::from_utf8_lossy(&restored_ansi);
        assert!(output.contains("Green"));
        assert!(output.contains("Normal"));
        // Green SGR should be present
        assert!(output.contains(";32m"));
    }

    #[test]
    fn scrollback_captured() {
        let mut session = make_session(80, 5);
        // Write more lines than the screen fits to push into scrollback
        for i in 0..20 {
            let line = format!("Scrollback line {}\r\n", i);
            feed_bytes(&mut session, line.as_bytes());
        }
        let ansi = extract_grid_as_ansi(&session.term, None);
        let output = String::from_utf8_lossy(&ansi);
        // Earlier lines should be in the captured output
        assert!(output.contains("Scrollback line 0"));
        assert!(output.contains("Scrollback line 19"));
    }

    #[test]
    fn push_u8_correctness() {
        let mut buf = Vec::new();
        push_u8(&mut buf, 0);
        assert_eq!(&buf, b"0");
        buf.clear();
        push_u8(&mut buf, 9);
        assert_eq!(&buf, b"9");
        buf.clear();
        push_u8(&mut buf, 42);
        assert_eq!(&buf, b"42");
        buf.clear();
        push_u8(&mut buf, 255);
        assert_eq!(&buf, b"255");
    }

    #[test]
    fn named_color_sgr_mapping() {
        assert_eq!(named_color_sgr(NamedColor::Foreground, true), None);
        assert_eq!(named_color_sgr(NamedColor::Background, false), None);
        assert_eq!(named_color_sgr(NamedColor::Red, true), Some(31));
        assert_eq!(named_color_sgr(NamedColor::Red, false), Some(41));
        assert_eq!(named_color_sgr(NamedColor::BrightRed, true), Some(91));
        assert_eq!(named_color_sgr(NamedColor::BrightRed, false), Some(101));
    }
}
