use alacritty_terminal::{
    term::Term,
    vte::ansi::{Processor, StdSyncHandler},
};

use crate::terminal::EventProxy;

const SEPARATOR: &[u8] = b"\x1b[90m\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80 restored session \xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\xe2\x94\x80\x1b[0m\r\n";

/// Injects previously captured ANSI scrollback bytes into a terminal.
///
/// The bytes are fed through alacritty's VTE processor, which naturally
/// fills the grid and scrollback buffer. A visual separator line is appended
/// after the injected content.
///
/// Must be called BEFORE the PTY reader thread starts to avoid race conditions.
/// Returns the number of history lines created by the injection (for deferred scroll).
pub fn inject_scrollback(term: &mut Term<EventProxy>, ansi_bytes: &[u8]) -> usize {
    if ansi_bytes.is_empty() {
        return 0;
    }

    // Strip trailing empty lines (\r\n sequences) from the input so
    // restored sessions don't accumulate blank gaps across cycles.
    let trimmed = trim_trailing_empty_lines(ansi_bytes);
    if trimmed.is_empty() {
        return 0;
    }

    let mut processor: Processor<StdSyncHandler> = Processor::new();

    const CHUNK_SIZE: usize = 65536;
    let mut pos = 0;
    while pos < trimmed.len() {
        let end = (pos + CHUNK_SIZE).min(trimmed.len());
        processor.advance(term, &trimmed[pos..end]);
        pos = end;
    }

    // Emit separator line
    processor.advance(term, SEPARATOR);

    // Return the number of content lines injected (including separator).
    // This is used as the scroll-up amount after the shell initializes —
    // we can't use history size here because with few lines the content
    // may fit on screen at injection time (no history yet), but the shell
    // prompt will push it into scrollback later.
    trimmed.windows(2).filter(|w| w == b"\r\n").count() + 1
}

fn trim_trailing_empty_lines(data: &[u8]) -> &[u8] {
    let mut end = data.len();
    // Walk backwards, removing trailing \r\n pairs that have nothing before them
    // (or only other \r\n pairs).
    while end >= 2 && data[end - 2] == b'\r' && data[end - 1] == b'\n' {
        // Check if the line before this \r\n is also empty
        let line_start = if end >= 4 {
            // Find where this line's content starts (after previous \r\n)
            data[..end - 2]
                .windows(2)
                .rposition(|w| w == b"\r\n")
                .map(|p| p + 2)
                .unwrap_or(0)
        } else {
            0
        };
        // If the line between line_start and end-2 is all whitespace or empty, trim it
        let line_content = &data[line_start..end - 2];
        if line_content.iter().all(|&b| b == b' ' || b == b'\t') {
            end = line_start;
        } else {
            break;
        }
    }
    &data[..end]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::Session;
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line};
    use alacritty_terminal::term::cell::Flags;

    fn make_session(cols: u16, rows: u16) -> Session {
        Session::new_for_test(1, cols, rows)
    }

    fn feed_bytes(session: &mut Session, data: &[u8]) {
        let mut processor: Processor<StdSyncHandler> = Processor::new();
        processor.advance(&mut session.term, data);
    }

    fn grid_line_text(session: &Session, line_idx: i32) -> String {
        let grid = session.term.grid();
        let cols = session.term.columns();
        let mut text = String::new();
        for col in 0..cols {
            let cell = &grid[Line(line_idx)][Column(col)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            text.push(cell.c);
        }
        text.trim_end().to_string()
    }

    #[test]
    fn empty_injection_is_noop() {
        let mut session = make_session(80, 24);
        inject_scrollback(&mut session.term, &[]);
        // Grid should still be all spaces
        let text = grid_line_text(&session, 0);
        assert!(text.is_empty());
    }

    #[test]
    fn plain_text_injection() {
        let mut session = make_session(80, 24);
        inject_scrollback(&mut session.term, b"Hello World\r\n");
        // "Hello World" should appear in the grid
        let text = grid_line_text(&session, 0);
        assert_eq!(text, "Hello World");
    }

    #[test]
    fn separator_appended() {
        let mut session = make_session(80, 5);
        inject_scrollback(&mut session.term, b"Content\r\n");
        // The separator contains "restored session"
        let mut found_separator = false;
        let screen_lines = session.term.screen_lines() as i32;
        let history = session.term.grid().history_size() as i32;
        for line_idx in (-history)..screen_lines {
            let text = grid_line_text(&session, line_idx);
            if text.contains("restored session") {
                found_separator = true;
                break;
            }
        }
        assert!(found_separator, "Separator line not found in grid");
    }

    #[test]
    fn colored_text_injection() {
        let mut session = make_session(80, 24);
        inject_scrollback(&mut session.term, b"\x1b[31mRed\x1b[0m\r\n");
        // The text should be present
        let text = grid_line_text(&session, 0);
        assert_eq!(text, "Red");
    }

    #[test]
    fn large_injection_goes_to_scrollback() {
        let mut session = make_session(80, 5);
        let mut data = Vec::new();
        for i in 0..20 {
            data.extend_from_slice(format!("Line {}\r\n", i).as_bytes());
        }
        inject_scrollback(&mut session.term, &data);

        let history = session.term.grid().history_size();
        // With 20 content lines + separator in a 5-row terminal, we should have scrollback
        assert!(history > 0, "Expected scrollback, got history_size=0");
    }

    #[test]
    fn injection_before_shell_output() {
        let mut session = make_session(80, 5);
        // Inject some history
        inject_scrollback(&mut session.term, b"Previous output\r\n");

        // Simulate shell output arriving after injection
        feed_bytes(&mut session, b"$ whoami\r\nuser\r\n$ ");

        // Both should be present somewhere in history + screen
        let mut found_previous = false;
        let mut found_shell = false;
        let screen_lines = session.term.screen_lines() as i32;
        let history = session.term.grid().history_size() as i32;
        for line_idx in (-history)..screen_lines {
            let text = grid_line_text(&session, line_idx);
            if text.contains("Previous output") {
                found_previous = true;
            }
            if text.contains("$ whoami") {
                found_shell = true;
            }
        }
        assert!(found_previous, "Injected content not found");
        assert!(found_shell, "Shell output not found");
    }

    #[test]
    fn chunked_processing() {
        let mut session = make_session(80, 24);
        // Create data larger than CHUNK_SIZE to test chunking
        let mut data = Vec::new();
        for i in 0..1000 {
            data.extend_from_slice(format!("Line {:04}\r\n", i).as_bytes());
        }
        inject_scrollback(&mut session.term, &data);

        // Verify first and last lines are present
        let history = session.term.grid().history_size() as i32;
        let first_line = grid_line_text(&session, -history);
        assert!(first_line.contains("Line 0000"));
    }
}
