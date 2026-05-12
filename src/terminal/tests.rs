#[cfg(test)]
mod terminal_tests {
    use crate::terminal::Session;
    use alacritty_terminal::{
        grid::Dimensions,
        index::{Column, Line},
        term::TermMode,
        vte::ansi::{Processor, StdSyncHandler},
    };

    fn make_session(cols: u16, rows: u16) -> Session {
        Session::new_for_test(1, cols, rows)
    }

    #[test]
    fn test_session_dimensions() {
        let s = make_session(80, 24);
        assert_eq!(s.term.columns(), 80);
        assert_eq!(s.term.screen_lines(), 24);
    }

    #[test]
    fn test_session_resize() {
        let mut s = make_session(80, 24);
        s.resize(40, 12);
        assert_eq!(s.term.columns(), 40);
        assert_eq!(s.term.screen_lines(), 12);
    }

    #[test]
    fn test_resize_preserves_content() {
        let mut s = make_session(80, 24);
        let mut proc: Processor<StdSyncHandler> = Processor::new();
        proc.advance(&mut s.term, b"hello");
        s.resize(40, 24);
        let grid = s.term.grid();
        assert_eq!(grid[Line(0)][Column(0)].c, 'h');
    }

    #[test]
    fn test_title_via_osc0() {
        let mut s = make_session(80, 24);
        let mut proc: Processor<StdSyncHandler> = Processor::new();
        proc.advance(&mut s.term, b"\x1b]0;My Title\x07");
        assert_eq!(s.title(), "My Title");
    }

    #[test]
    fn test_title_via_osc2() {
        let mut s = make_session(80, 24);
        let mut proc: Processor<StdSyncHandler> = Processor::new();
        proc.advance(&mut s.term, b"\x1b]2;Tab Title\x07");
        assert_eq!(s.title(), "Tab Title");
    }

    #[test]
    fn test_print_advances_cursor() {
        let mut s = make_session(80, 24);
        let mut proc: Processor<StdSyncHandler> = Processor::new();
        proc.advance(&mut s.term, b"hello");
        let cursor = s.term.grid().cursor.point;
        assert_eq!(cursor.column.0, 5);
        assert_eq!(cursor.line.0, 0);
    }

    #[test]
    fn test_bracketed_paste_mode() {
        let mut s = make_session(80, 24);
        let mut proc: Processor<StdSyncHandler> = Processor::new();
        assert!(!s.term.mode().contains(TermMode::BRACKETED_PASTE));
        proc.advance(&mut s.term, b"\x1b[?2004h");
        assert!(s.term.mode().contains(TermMode::BRACKETED_PASTE));
        proc.advance(&mut s.term, b"\x1b[?2004l");
        assert!(!s.term.mode().contains(TermMode::BRACKETED_PASTE));
    }

    #[test]
    fn test_mouse_report_click() {
        let mut s = make_session(80, 24);
        let mut proc: Processor<StdSyncHandler> = Processor::new();
        assert!(!s.term.mode().contains(TermMode::MOUSE_REPORT_CLICK));
        proc.advance(&mut s.term, b"\x1b[?1000h");
        assert!(s.term.mode().contains(TermMode::MOUSE_REPORT_CLICK));
    }

    #[test]
    fn test_sgr_mouse() {
        let mut s = make_session(80, 24);
        let mut proc: Processor<StdSyncHandler> = Processor::new();
        proc.advance(&mut s.term, b"\x1b[?1006h");
        assert!(s.term.mode().contains(TermMode::SGR_MOUSE));
    }

    #[test]
    fn test_cursor_visibility() {
        let mut s = make_session(80, 24);
        let mut proc: Processor<StdSyncHandler> = Processor::new();
        assert!(s.term.mode().contains(TermMode::SHOW_CURSOR));
        proc.advance(&mut s.term, b"\x1b[?25l");
        assert!(!s.term.mode().contains(TermMode::SHOW_CURSOR));
        proc.advance(&mut s.term, b"\x1b[?25h");
        assert!(s.term.mode().contains(TermMode::SHOW_CURSOR));
    }

    #[test]
    fn test_sgr_bold_flag() {
        use alacritty_terminal::term::cell::Flags;
        let mut s = make_session(80, 24);
        let mut proc: Processor<StdSyncHandler> = Processor::new();
        proc.advance(&mut s.term, b"\x1b[1mA\x1b[0m");
        let cell = &s.term.grid()[Line(0)][Column(0)];
        assert!(cell.flags.contains(Flags::BOLD));
    }
}
