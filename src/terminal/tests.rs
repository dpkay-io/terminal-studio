// End-to-end tests for the VTE performer → grid pipeline.
//
// Each test feeds raw escape sequences through the same parser stack used by
// the reader thread, then asserts on Session state.  No GUI, no PTY — pure
// logic verification.
#[cfg(test)]
use super::{performer::Performer, MouseMode, Session};
#[cfg(test)]
use vte::Parser;

/// Feed a byte slice through a fresh VTE parser into a Performer that owns
/// `session`, returning the session for inspection.
#[cfg(test)]
fn feed(session: &mut Session, data: &[u8]) {
    let mut parser = Parser::new();
    let mut performer = Performer::new(session);
    for &byte in data {
        parser.advance(&mut performer, byte);
    }
}

/// Return every character on the given row as a String (spaces included).
#[cfg(test)]
fn row_text(session: &Session, row: u16) -> String {
    (0..session.grid.cols)
        .map(|col| session.grid.get(row, col).c)
        .collect()
}

/// Return just the non-space characters on the given row (trimmed).
#[cfg(test)]
fn row_text_trimmed(session: &Session, row: u16) -> String {
    row_text(session, row).trim().to_string()
}

// ── Basic text & cursor ────────────────────────────────────────────────────

#[test]
fn test_print_chars() {
    let mut s = Session::new(1, 10, 5, None);
    feed(&mut s, b"Hello");
    assert_eq!(row_text_trimmed(&s, 0), "Hello");
    assert_eq!(s.cursor_x, 5);
    assert_eq!(s.cursor_y, 0);
}

#[test]
fn test_cr_lf() {
    let mut s = Session::new(1, 10, 5, None);
    feed(&mut s, b"AB\r\nCD");
    assert_eq!(row_text_trimmed(&s, 0), "AB");
    assert_eq!(row_text_trimmed(&s, 1), "CD");
    assert_eq!(s.cursor_x, 2);
    assert_eq!(s.cursor_y, 1);
}

#[test]
fn test_cursor_up_down_left_right() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[5;5H"); // CUP → (row 5, col 5) 1-based → (4,4)
    assert_eq!((s.cursor_y, s.cursor_x), (4, 4));

    feed(&mut s, b"\x1b[2A"); // CUU 2 → row 2
    assert_eq!(s.cursor_y, 2);

    feed(&mut s, b"\x1b[3B"); // CUD 3 → row 5
    assert_eq!(s.cursor_y, 5);

    feed(&mut s, b"\x1b[2D"); // CUB 2 → col 2
    assert_eq!(s.cursor_x, 2);

    feed(&mut s, b"\x1b[4C"); // CUF 4 → col 6
    assert_eq!(s.cursor_x, 6);
}

#[test]
fn test_cup_clamps_to_grid() {
    let mut s = Session::new(1, 10, 5, None);
    feed(&mut s, b"\x1b[999;999H");
    assert_eq!(s.cursor_y, 4); // clamped to rows-1
    assert_eq!(s.cursor_x, 9); // clamped to cols-1
}

// ── Wrap / pending_wrap ────────────────────────────────────────────────────

#[test]
fn test_wrap_at_eol() {
    let mut s = Session::new(1, 5, 5, None);
    // Fill exactly 5 chars — cursor should be at col 4 with pending_wrap=true
    feed(&mut s, b"ABCDE");
    assert_eq!(s.cursor_x, 4);
    assert!(s.pending_wrap);
    // Next char wraps to row 1, col 0
    feed(&mut s, b"F");
    assert_eq!(s.cursor_y, 1);
    assert_eq!(s.cursor_x, 1);
    assert_eq!(row_text_trimmed(&s, 0), "ABCDE");
    assert_eq!(&row_text(&s, 1)[..1], "F");
}

// ── Newline & scroll region ────────────────────────────────────────────────

#[test]
fn test_newline_scrolls_full_screen() {
    // 5-row terminal, fill all rows then add one more LF — row 0 should scroll off
    let mut s = Session::new(1, 10, 5, None);
    for i in 0u8..5 {
        let line = [b'A' + i; 3];
        feed(&mut s, &line);
        feed(&mut s, b"\r\n");
    }
    // After 5 lines the first line (AAA) scrolled into scrollback
    // Row 0 should now hold the second line (BBB)
    assert_eq!(row_text_trimmed(&s, 0), "BBB");
    assert!(!s.grid.scrollback.is_empty());
}

#[test]
fn test_decstbm_sets_region() {
    let mut s = Session::new(1, 80, 24, None);
    // Set scroll region rows 5–20 (1-based)
    feed(&mut s, b"\x1b[5;20r");
    assert_eq!(s.scroll_top, 4); // 0-based
    assert_eq!(s.scroll_bottom, 19);
    // DECSTBM always homes cursor
    assert_eq!((s.cursor_y, s.cursor_x), (0, 0));
}

#[test]
fn test_decstbm_reset() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b[5;20r");
    // Reset with no params → full screen
    feed(&mut s, b"\x1b[r");
    assert_eq!(s.scroll_top, 0);
    assert_eq!(s.scroll_bottom, 23);
}

#[test]
fn test_newline_at_scroll_bottom_scrolls_region() {
    // 10-row terminal, scroll region rows 3–7 (1-based, 0-based: 2–6)
    let mut s = Session::new(1, 5, 10, None);
    feed(&mut s, b"\x1b[3;7r"); // set region
    assert_eq!(s.scroll_top, 2);
    assert_eq!(s.scroll_bottom, 6);

    // Write a known char on every row of the region
    for row in 2u16..=6 {
        feed(
            &mut s,
            &[0x1b, b'[', b'0' + (row as u8 + 1), b';', b'1', b'H'],
        ); // won't work for >9
           // Use CUP properly: ESC [ row ; col H
        let cup = format!("\x1b[{};1H", row + 1);
        feed(&mut s, cup.as_bytes());
        let ch = b'A' + (row as u8 - 2);
        feed(&mut s, &[ch]);
    }
    // Move cursor to scroll_bottom (row 6, 0-based)
    feed(&mut s, b"\x1b[7;1H"); // 1-based row 7 = 0-based row 6
    assert_eq!(s.cursor_y, 6);

    // LF at scroll bottom should scroll region up, NOT scrollback
    let scrollback_before = s.grid.scrollback.len();
    feed(&mut s, b"\n");
    let scrollback_after = s.grid.scrollback.len();

    // Scrollback must not grow (region scroll, top > 0)
    assert_eq!(
        scrollback_before, scrollback_after,
        "region scroll must not pollute scrollback"
    );

    // Cursor stays at scroll_bottom
    assert_eq!(s.cursor_y, 6);

    // Row below the region (row 7 onward) must be untouched
    // Row 7 was never written so it should still be spaces
    assert_eq!(row_text_trimmed(&s, 7), "");

    // Row 2 (top of region) should now hold what was in row 3 before scroll
    // i.e. 'B' (which was at row 3, 0-based)
    assert_eq!(&row_text(&s, 2)[..1], "B");
}

#[test]
fn test_newline_outside_scroll_region_does_not_scroll() {
    // Cursor BELOW the scroll region — LF must just move the cursor down,
    // never scroll the region.
    let mut s = Session::new(1, 5, 10, None);
    feed(&mut s, b"\x1b[2;5r"); // scroll region rows 2-5 (0-based 1-4)

    // Position cursor at row 7 (below the scroll region)
    feed(&mut s, b"\x1b[8;1H");
    assert_eq!(s.cursor_y, 7);

    // Write something in the scroll region so we can see if it moves
    feed(&mut s, b"\x1b[2;1H"); // top of scroll region
    feed(&mut s, b"MARK");
    feed(&mut s, b"\x1b[8;1H"); // back to row 7

    let before = row_text_trimmed(&s, 1); // row 1 = top of scroll region
    feed(&mut s, b"\n");
    let after = row_text_trimmed(&s, 1);

    assert_eq!(
        before, after,
        "scroll region must not change when cursor is below it"
    );
    assert_eq!(s.cursor_y, 8, "cursor should have moved to row 8");
}

// ── Alternate screen ───────────────────────────────────────────────────────

#[test]
fn test_alt_screen_enter_leave() {
    let mut s = Session::new(1, 10, 5, None);
    // Write on primary screen
    feed(&mut s, b"Primary");
    assert_eq!(row_text_trimmed(&s, 0), "Primary");

    // Enter alt screen (?1049h) — primary should be stashed
    feed(&mut s, b"\x1b[?1049h");
    assert!(s.saved_primary_grid.is_some());

    // Alt screen must be blank
    assert_eq!(row_text_trimmed(&s, 0), "");

    // Cursor must be at (0,0) after entering alt screen
    assert_eq!((s.cursor_y, s.cursor_x), (0, 0));

    // Write on alt screen
    feed(&mut s, b"AltContent");
    assert_eq!(row_text_trimmed(&s, 0), "AltContent");

    // Leave alt screen — primary must be restored
    feed(&mut s, b"\x1b[?1049l");
    assert!(s.saved_primary_grid.is_none());
    assert_eq!(row_text_trimmed(&s, 0), "Primary");
}

#[test]
fn test_alt_screen_1049_saves_and_restores_cursor() {
    let mut s = Session::new(1, 20, 10, None);
    // Position cursor on primary screen
    feed(&mut s, b"\x1b[4;7H"); // row 4, col 7 (1-based) → (3, 6) 0-based
    assert_eq!((s.cursor_y, s.cursor_x), (3, 6));

    // Enter ?1049h — cursor saved, cursor reset to (0,0) on alt
    feed(&mut s, b"\x1b[?1049h");
    assert_eq!(s.alt_saved_cursor, Some((6, 3)));
    assert_eq!((s.cursor_y, s.cursor_x), (0, 0));

    // Move around on alt screen
    feed(&mut s, b"\x1b[9;15H");

    // Leave ?1049l — cursor restored to primary position
    feed(&mut s, b"\x1b[?1049l");
    assert_eq!((s.cursor_y, s.cursor_x), (3, 6));
}

#[test]
fn test_alt_screen_47_no_cursor_save() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[4;7H");

    feed(&mut s, b"\x1b[?47h");
    // ?47 does NOT save cursor
    assert_eq!(s.alt_saved_cursor, None);
    // But cursor IS reset to (0,0)
    assert_eq!((s.cursor_y, s.cursor_x), (0, 0));

    feed(&mut s, b"\x1b[?47l");
    // Cursor stays wherever alt left it (no restore)
    assert_eq!((s.cursor_y, s.cursor_x), (0, 0));
}

#[test]
fn test_alt_screen_resets_scroll_region() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b[5;20r"); // set non-default scroll region
    feed(&mut s, b"\x1b[?1049h"); // enter alt
    assert_eq!(s.scroll_top, 0);
    assert_eq!(s.scroll_bottom, 23);
    feed(&mut s, b"\x1b[?1049l"); // leave alt
    assert_eq!(s.scroll_top, 0);
    assert_eq!(s.scroll_bottom, 23);
}

// ── Device responses ───────────────────────────────────────────────────────

#[test]
fn test_da1_response_queued() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b[c"); // DA1 query
    assert!(!s.pending_dsr_response.is_empty());
    assert_eq!(s.pending_dsr_response[0], "\x1b[?62;1;22c");
}

#[test]
fn test_da2_response_queued() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b[>c"); // DA2 query
    assert!(!s.pending_dsr_response.is_empty());
    assert!(s.pending_dsr_response[0].starts_with("\x1b[>"));
}

#[test]
fn test_dsr_cursor_position() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b[5;10H"); // move to row 5, col 10 (1-based) → (4,9)
    feed(&mut s, b"\x1b[6n"); // DSR cursor pos query
    assert!(!s.pending_dsr_response.is_empty());
    assert_eq!(s.pending_dsr_response[0], "\x1b[5;10R");
}

#[test]
fn test_multiple_responses_all_queued() {
    // DA1 + DA2 + DSR in one burst — all three must be queued (not overwritten)
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b[c\x1b[>c\x1b[6n");
    assert_eq!(
        s.pending_dsr_response.len(),
        3,
        "all three responses must be queued independently"
    );
}

// ── Erase operations ───────────────────────────────────────────────────────

#[test]
fn test_ed2_clears_screen_cursor_stays() {
    let mut s = Session::new(1, 10, 5, None);
    // Move to row 2 (0-based), col 8 (after "Hello" at col 4)
    feed(&mut s, b"\x1b[3;5HHello");
    feed(&mut s, b"\x1b[2J");
    // Screen cleared
    assert_eq!(row_text_trimmed(&s, 2), "");
    // ED 2 must NOT move cursor — cursor stays where it was (VT100/xterm spec)
    assert_eq!((s.cursor_y, s.cursor_x), (2, 9));
}

#[test]
fn test_bce_erase_uses_current_bg() {
    use super::grid::Color;
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"ABCDE");
    // Set a non-default background color
    feed(&mut s, b"\x1b[41m"); // bg = red (Color::Indexed(1))
    feed(&mut s, b"\x1b[1;4H"); // col 4 (1-based = col 3 zero-based)
    feed(&mut s, b"\x1b[K"); // EL0: erase to end of line with red bg
                             // Erased cells should carry the red background
    let cell = s.grid.get(0, 4);
    assert!(
        matches!(cell.bg, Color::Indexed(1)),
        "erased cell should have current bg color"
    );
    // Non-erased cells should be unchanged
    let intact = s.grid.get(0, 0);
    assert_eq!(intact.c, 'A');
}

#[test]
fn test_ich_inserts_blank_chars() {
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"ABCDE"); // row 0: ABCDE
    feed(&mut s, b"\x1b[1;2H"); // cursor to col 2 (1-based = col 1)
    feed(&mut s, b"\x1b[2@"); // ICH: insert 2 blank chars
                              // A stays at col 0; B,C,D,E shift right by 2; cols 1-2 become spaces
    assert_eq!(s.grid.get(0, 0).c, 'A');
    assert_eq!(s.grid.get(0, 1).c, ' ');
    assert_eq!(s.grid.get(0, 2).c, ' ');
    assert_eq!(s.grid.get(0, 3).c, 'B');
    assert_eq!(s.grid.get(0, 4).c, 'C');
}

#[test]
fn test_ek_erases_to_eol() {
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"ABCDEFGH"); // 8 chars on row 0
    feed(&mut s, b"\x1b[1;4H"); // move to col 4 (1-based)
    feed(&mut s, b"\x1b[K"); // erase to EOL
    assert_eq!(row_text_trimmed(&s, 0), "ABC");
}

// ── SGR colours ───────────────────────────────────────────────────────────

#[test]
fn test_sgr_reset() {
    use super::grid::{CellAttrs, Color};
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"\x1b[1;31m"); // bold + red fg
    assert!(s.current_attrs.bold);
    assert!(matches!(s.current_fg, Color::Indexed(1)));
    feed(&mut s, b"\x1b[0m"); // reset
    assert_eq!(s.current_attrs, CellAttrs::default());
    assert!(matches!(s.current_fg, Color::Default));
}

#[test]
fn test_sgr_rgb_fg() {
    use super::grid::Color;
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"\x1b[38;2;200;100;50m");
    assert_eq!(s.current_fg, Color::Rgb(200, 100, 50));
}

// ── Reverse index (RI) ────────────────────────────────────────────────────

#[test]
fn test_ri_at_top_of_region_scrolls_down() {
    let mut s = Session::new(1, 5, 10, None);
    feed(&mut s, b"\x1b[3;7r"); // region rows 3-7 (0-based 2-6)
    feed(&mut s, b"\x1b[3;1H"); // cursor at top of region (row 3 1-based = row 2 0-based)
    feed(&mut s, b"MARK"); // write something
    feed(&mut s, b"\x1b[3;1H"); // back to top of region
    feed(&mut s, b"\x1bM"); // RI — should scroll down, MARK moves to row 3
                            // Row 2 (top of region) should now be blank
    assert_eq!(row_text_trimmed(&s, 2), "");
    // MARK should have moved to row 3
    assert_eq!(row_text_trimmed(&s, 3), "MARK");
}

#[test]
fn test_ri_not_at_top_moves_cursor_up() {
    let mut s = Session::new(1, 5, 10, None);
    feed(&mut s, b"\x1b[5;1H"); // row 5 (0-based 4)
    feed(&mut s, b"\x1bM"); // RI — cursor not at scroll_top (0), just move up
    assert_eq!(s.cursor_y, 3);
}

// ── Save / restore cursor ─────────────────────────────────────────────────

#[test]
fn test_decsc_decrc() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[4;8H"); // (3,7) 0-based
    feed(&mut s, b"\x1b7"); // DECSC save
    feed(&mut s, b"\x1b[1;1H"); // move elsewhere
    feed(&mut s, b"\x1b8"); // DECRC restore
    assert_eq!((s.cursor_y, s.cursor_x), (3, 7));
}

// ── IL / DL (insert/delete lines) ────────────────────────────────────────

#[test]
fn test_il_inserts_blank_line() {
    let mut s = Session::new(1, 5, 5, None);
    feed(&mut s, b"AAAAA\r\nBBBBB\r\nCCCCC");
    feed(&mut s, b"\x1b[2;1H"); // cursor at row 2 (1-based)
    feed(&mut s, b"\x1b[L"); // IL 1 — insert blank at row 1, push B and C down
    assert_eq!(row_text_trimmed(&s, 1), ""); // new blank row
    assert_eq!(row_text_trimmed(&s, 2), "BBBBB");
}

#[test]
fn test_dl_deletes_line() {
    let mut s = Session::new(1, 5, 5, None);
    feed(&mut s, b"AAAAA\r\nBBBBB\r\nCCCCC");
    feed(&mut s, b"\x1b[2;1H"); // cursor at row 2 (1-based) = row 1 (0-based)
    feed(&mut s, b"\x1b[M"); // DL 1 — delete row 1, C moves to row 1
    assert_eq!(row_text_trimmed(&s, 1), "CCCCC");
}

// ── Grid direct tests ─────────────────────────────────────────────────────────

#[test]
fn test_grid_get_mut() {
    use super::grid::{Color, Grid};
    let mut g = Grid::new(5, 3);
    {
        let cell = g.get_mut(1, 2);
        cell.c = 'Z';
        cell.fg = Color::Indexed(3);
    }
    let c = g.get(1, 2);
    assert_eq!(c.c, 'Z');
    assert_eq!(c.fg, Color::Indexed(3));
}

#[test]
fn test_grid_scroll_up_saves_scrollback_when_top_zero() {
    use super::grid::Grid;
    let mut g = Grid::new(5, 3);
    g.get_mut(0, 0).c = 'X';
    let before = g.scrollback.len();
    g.scroll_up(0, 2, 1);
    assert_eq!(g.scrollback.len(), before + 1);
    assert_eq!(g.scrollback.back().unwrap()[0].c, 'X');
}

#[test]
fn test_grid_scroll_up_no_scrollback_when_top_nonzero() {
    use super::grid::Grid;
    let mut g = Grid::new(5, 5);
    g.scroll_up(2, 4, 1);
    assert!(g.scrollback.is_empty());
}

#[test]
fn test_grid_scroll_down_shifts_rows() {
    use super::grid::Grid;
    let mut g = Grid::new(5, 5);
    g.get_mut(0, 0).c = 'A';
    g.get_mut(1, 0).c = 'B';
    g.scroll_down(0, 4, 1);
    // Row 0 should be blank (new blank inserted at top)
    assert_eq!(g.get(0, 0).c, ' ');
    // Row 1 should have what was in row 0
    assert_eq!(g.get(1, 0).c, 'A');
    // Row 2 should have what was in row 1
    assert_eq!(g.get(2, 0).c, 'B');
}

#[test]
fn test_grid_resize_grow() {
    use super::grid::Grid;
    let mut g = Grid::new(5, 3);
    g.get_mut(0, 0).c = 'A';
    g.get_mut(2, 4).c = 'Z';
    g.resize(8, 5);
    assert_eq!(g.cols, 8);
    assert_eq!(g.rows, 5);
    assert_eq!(g.get(0, 0).c, 'A');
    assert_eq!(g.get(2, 4).c, 'Z');
    // New cells are default (space)
    assert_eq!(g.get(0, 7).c, ' ');
    assert_eq!(g.get(4, 0).c, ' ');
}

#[test]
fn test_grid_resize_shrink() {
    use super::grid::Grid;
    let mut g = Grid::new(8, 5);
    g.get_mut(0, 0).c = 'A';
    g.get_mut(1, 1).c = 'B';
    g.resize(4, 2);
    assert_eq!(g.cols, 4);
    assert_eq!(g.rows, 2);
    assert_eq!(g.get(0, 0).c, 'A');
    assert_eq!(g.get(1, 1).c, 'B');
}

#[test]
fn test_grid_clear_all() {
    use super::grid::Grid;
    let mut g = Grid::new(5, 3);
    g.get_mut(0, 0).c = 'A';
    g.get_mut(2, 4).c = 'Z';
    g.clear_all();
    assert_eq!(g.get(0, 0).c, ' ');
    assert_eq!(g.get(2, 4).c, ' ');
}

#[test]
fn test_ansi_color_standard_16() {
    use super::grid::ansi_color;
    // Black
    assert_eq!(ansi_color(0), (30, 30, 46));
    // Red
    assert_eq!(ansi_color(1), (243, 139, 168));
    // White
    assert_eq!(ansi_color(7), (205, 214, 244));
    // Bright black
    assert_eq!(ansi_color(8), (88, 91, 112));
    // Bright white
    assert_eq!(ansi_color(15), (255, 255, 255));
}

#[test]
fn test_ansi_color_cube() {
    use super::grid::ansi_color;
    // Index 16 = 0,0,0 (first cube entry)
    assert_eq!(ansi_color(16), (0, 0, 0));
    // Index 231 = last cube entry: r=5,g=5,b=5 → 255,255,255
    assert_eq!(ansi_color(231), (255, 255, 255));
    // Index 17: b=1, g=0, r=0 → (0, 0, 51)
    assert_eq!(ansi_color(17), (0, 0, 51));
}

#[test]
fn test_ansi_color_grayscale() {
    use super::grid::ansi_color;
    // Index 232: 8+0*10=8
    assert_eq!(ansi_color(232), (8, 8, 8));
    // Index 255: 8+23*10=238
    assert_eq!(ansi_color(255), (238, 238, 238));
}

// ── Performer: control characters ─────────────────────────────────────────────

#[test]
fn test_bel_is_ignored() {
    let mut s = Session::new(1, 10, 5, None);
    s.cursor_x = 3;
    feed(&mut s, b"\x07"); // BEL
    assert_eq!(s.cursor_x, 3);
}

#[test]
fn test_tab_advances_to_next_tabstop() {
    let mut s = Session::new(1, 40, 5, None);
    s.cursor_x = 0;
    feed(&mut s, b"\x09"); // HT
    assert_eq!(s.cursor_x, 8);
    feed(&mut s, b"\x09");
    assert_eq!(s.cursor_x, 16);
    // Mid-tab-stop
    s.cursor_x = 5;
    feed(&mut s, b"\x09");
    assert_eq!(s.cursor_x, 8);
}

#[test]
fn test_tab_clamps_at_last_col() {
    let mut s = Session::new(1, 10, 5, None);
    s.cursor_x = 9; // last col
    feed(&mut s, b"\x09");
    assert_eq!(s.cursor_x, 9); // already at edge, clamped
}

// ── Performer: cursor movement CSI ───────────────────────────────────────────

#[test]
fn test_cnl_cursor_next_line() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[5;5H"); // (4,4) 0-based
    feed(&mut s, b"\x1b[2E"); // CNL 2 → row 6, col 0
    assert_eq!(s.cursor_y, 6);
    assert_eq!(s.cursor_x, 0);
}

#[test]
fn test_cpl_cursor_prev_line() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[5;5H"); // (4,4) 0-based
    feed(&mut s, b"\x1b[2F"); // CPL 2 → row 2, col 0
    assert_eq!(s.cursor_y, 2);
    assert_eq!(s.cursor_x, 0);
}

#[test]
fn test_cha_cursor_horizontal_absolute() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[5;5H");
    feed(&mut s, b"\x1b[8G"); // CHA → col 8 (1-based) = 7 (0-based)
    assert_eq!(s.cursor_x, 7);
    assert_eq!(s.cursor_y, 4); // row unchanged
}

#[test]
fn test_vpa_vertical_position_absolute() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[5;5H");
    feed(&mut s, b"\x1b[3d"); // VPA → row 3 (1-based) = 2 (0-based)
    assert_eq!(s.cursor_y, 2);
    assert_eq!(s.cursor_x, 4); // col unchanged
}

// ── Performer: erase ──────────────────────────────────────────────────────────

#[test]
fn test_ed1_erases_from_top_to_cursor() {
    let mut s = Session::new(1, 10, 5, None);
    feed(&mut s, b"AAAAAAAAAA"); // row 0 full
    feed(&mut s, b"\r\n");
    feed(&mut s, b"BBBBBBBBBB"); // row 1 full
    feed(&mut s, b"\x1b[2;5H"); // cursor row 2 (1-based), col 5
    feed(&mut s, b"\x1b[1J"); // ED1: erase from top to cursor (inclusive)
                              // Row 0 should be blank
    assert_eq!(row_text_trimmed(&s, 0), "");
    // Row 1 should be blank up to col 4 (0-based)
    assert_eq!(&row_text(&s, 1)[..5], "     ");
}

#[test]
fn test_el1_erases_from_bol_to_cursor() {
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"ABCDEFGH"); // 8 chars
    feed(&mut s, b"\x1b[1;4H"); // cursor at col 4 (1-based) = 3 (0-based)
    feed(&mut s, b"\x1b[1K"); // EL1: erase from BOL to cursor
                              // Cols 0..3 should be blank, cols 4..7 should remain
    assert_eq!(&row_text(&s, 0)[..4], "    ");
    assert_eq!(&row_text(&s, 0)[4..8], "EFGH");
}

#[test]
fn test_el2_erases_entire_line() {
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"ABCDEFGH");
    feed(&mut s, b"\x1b[1;4H");
    feed(&mut s, b"\x1b[2K"); // EL2: erase entire line
    assert_eq!(row_text_trimmed(&s, 0), "");
}

#[test]
fn test_dch_delete_characters() {
    // DCH 'P': delete 2 chars at cursor; trailing chars shift left
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"ABCDEFGH"); // row 0: A B C D E F G H _ _
    feed(&mut s, b"\x1b[1;3H"); // cursor col 3 (1-based) = 2 (0-based)
    feed(&mut s, b"\x1b[2P"); // DCH 2: delete C and D, shift EFGH left
    let r = row_text(&s, 0);
    assert_eq!(&r[..6], "ABEFGH");
    assert_eq!(&r[6..8], "  "); // two trailing blank cells
}

#[test]
fn test_ech_erase_characters() {
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"ABCDEFGH");
    feed(&mut s, b"\x1b[1;3H"); // cursor col 3 (1-based) = 2 (0-based)
    feed(&mut s, b"\x1b[3X"); // ECH 3: erase C, D, E
    let r = row_text(&s, 0);
    assert_eq!(&r[0..2], "AB");
    assert_eq!(&r[2..5], "   ");
    assert_eq!(&r[5..8], "FGH");
}

#[test]
fn test_sd_scroll_down() {
    let mut s = Session::new(1, 5, 5, None);
    feed(&mut s, b"AAAAA\r\nBBBBB\r\nCCCCC");
    feed(&mut s, b"\x1b[2T"); // SD 2: scroll visible area down by 2
                              // Row 0 and 1 should be blank (blank lines inserted at top)
    assert_eq!(row_text_trimmed(&s, 0), "");
    assert_eq!(row_text_trimmed(&s, 1), "");
    assert_eq!(row_text_trimmed(&s, 2), "AAAAA");
}

// ── Performer: CSI s/u cursor save-restore ────────────────────────────────────

#[test]
fn test_csi_save_restore_cursor() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[4;8H"); // (3,7) 0-based
    feed(&mut s, b"\x1b[s"); // CSI s — save
    feed(&mut s, b"\x1b[1;1H"); // move elsewhere
    feed(&mut s, b"\x1b[u"); // CSI u — restore
    assert_eq!((s.cursor_y, s.cursor_x), (3, 7));
}

// ── Performer: DSR status ─────────────────────────────────────────────────────

#[test]
fn test_dsr_status_ok() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b[5n"); // DSR: report status OK
    assert!(!s.pending_dsr_response.is_empty());
    assert_eq!(s.pending_dsr_response[0], "\x1b[0n");
}

// ── Performer: RIS reset ──────────────────────────────────────────────────────

#[test]
fn test_ris_resets_state() {
    use super::grid::{CellAttrs, Color};
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[5;5H"); // move cursor
    feed(&mut s, b"\x1b[1;31m"); // bold + red
    feed(&mut s, b"\x1b[3;7r"); // set scroll region
    feed(&mut s, b"Hello"); // put chars on screen
    feed(&mut s, b"\x1bc"); // RIS
    assert_eq!((s.cursor_y, s.cursor_x), (0, 0));
    assert_eq!(s.scroll_top, 0);
    assert_eq!(s.scroll_bottom, 9);
    assert_eq!(s.current_fg, Color::Default);
    assert_eq!(s.current_attrs, CellAttrs::default());
    assert_eq!(row_text_trimmed(&s, 0), "");
}

// ── Performer: OSC ───────────────────────────────────────────────────────────

#[test]
fn test_osc_sets_title() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b]0;My Title\x07");
    assert_eq!(s.title, "My Title");
}

#[test]
fn test_osc2_sets_title() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b]2;Another Title\x07");
    assert_eq!(s.title, "Another Title");
}

#[test]
fn test_osc7_sets_cwd_file_uri() {
    let mut s = Session::new(1, 80, 24, None);
    // Use a Unix-style file:// URI (cross-platform path logic in performer)
    feed(&mut s, b"\x1b]7;file:///home/user/projects\x07");
    let cwd = s.cwd.to_string_lossy().into_owned();
    assert!(
        cwd.contains("home") || cwd.contains("projects"),
        "CWD not set correctly: {cwd}"
    );
}

#[test]
fn test_osc_empty_params_no_crash() {
    let mut s = Session::new(1, 80, 24, None);
    // OSC with no params should not panic
    feed(&mut s, b"\x1b]\x07");
}

// ── Performer: SGR extended ───────────────────────────────────────────────────

#[test]
fn test_sgr_all_attributes() {
    let mut s = Session::new(1, 10, 3, None);
    // Set all attributes
    feed(&mut s, b"\x1b[2m"); // dim
    assert!(s.current_attrs.dim);
    feed(&mut s, b"\x1b[3m"); // italic
    assert!(s.current_attrs.italic);
    feed(&mut s, b"\x1b[4m"); // underline
    assert!(s.current_attrs.underline);
    feed(&mut s, b"\x1b[5m"); // blink
    assert!(s.current_attrs.blink);
    feed(&mut s, b"\x1b[6m"); // blink (alt)
    assert!(s.current_attrs.blink);
    feed(&mut s, b"\x1b[7m"); // inverse
    assert!(s.current_attrs.inverse);
    feed(&mut s, b"\x1b[8m"); // invisible
    assert!(s.current_attrs.invisible);
    feed(&mut s, b"\x1b[9m"); // strikethrough
    assert!(s.current_attrs.strikethrough);
}

#[test]
fn test_sgr_individual_reset_codes() {
    let mut s = Session::new(1, 10, 3, None);
    // Set all attrs then individually unset each
    feed(&mut s, b"\x1b[1;2;3;4;5;7;8;9m");
    assert!(s.current_attrs.bold && s.current_attrs.italic);

    feed(&mut s, b"\x1b[22m"); // un-bold and un-dim
    assert!(!s.current_attrs.bold);
    assert!(!s.current_attrs.dim);
    feed(&mut s, b"\x1b[23m");
    assert!(!s.current_attrs.italic);
    feed(&mut s, b"\x1b[24m");
    assert!(!s.current_attrs.underline);
    feed(&mut s, b"\x1b[25m");
    assert!(!s.current_attrs.blink);
    feed(&mut s, b"\x1b[27m");
    assert!(!s.current_attrs.inverse);
    feed(&mut s, b"\x1b[28m");
    assert!(!s.current_attrs.invisible);
    feed(&mut s, b"\x1b[29m");
    assert!(!s.current_attrs.strikethrough);
}

#[test]
fn test_sgr_bg_colors() {
    use super::grid::Color;
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"\x1b[41m"); // red bg
    assert_eq!(s.current_bg, Color::Indexed(1));
    feed(&mut s, b"\x1b[47m"); // white bg
    assert_eq!(s.current_bg, Color::Indexed(7));
    feed(&mut s, b"\x1b[49m"); // default bg
    assert_eq!(s.current_bg, Color::Default);
}

#[test]
fn test_sgr_bright_fg() {
    use super::grid::Color;
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"\x1b[90m"); // bright black fg (index 8)
    assert_eq!(s.current_fg, Color::Indexed(8));
    feed(&mut s, b"\x1b[97m"); // bright white fg (index 15)
    assert_eq!(s.current_fg, Color::Indexed(15));
}

#[test]
fn test_sgr_bright_bg() {
    use super::grid::Color;
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"\x1b[100m"); // bright black bg (index 8)
    assert_eq!(s.current_bg, Color::Indexed(8));
    feed(&mut s, b"\x1b[107m"); // bright white bg (index 15)
    assert_eq!(s.current_bg, Color::Indexed(15));
}

#[test]
fn test_sgr_256_fg() {
    use super::grid::Color;
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"\x1b[38;5;200m"); // 256-color fg index 200
    assert_eq!(s.current_fg, Color::Indexed(200));
}

#[test]
fn test_sgr_256_bg() {
    use super::grid::Color;
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"\x1b[48;5;100m"); // 256-color bg index 100
    assert_eq!(s.current_bg, Color::Indexed(100));
}

#[test]
fn test_sgr_rgb_bg() {
    use super::grid::Color;
    let mut s = Session::new(1, 10, 3, None);
    feed(&mut s, b"\x1b[48;2;10;20;30m");
    assert_eq!(s.current_bg, Color::Rgb(10, 20, 30));
}

// ── Performer: private mode flags ────────────────────────────────────────────

#[test]
fn test_mouse_modes() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b[?1000h");
    assert_eq!(s.mouse_mode, MouseMode::Basic);
    feed(&mut s, b"\x1b[?1000l");
    assert_eq!(s.mouse_mode, MouseMode::None);
    feed(&mut s, b"\x1b[?1002h");
    assert_eq!(s.mouse_mode, MouseMode::ButtonMotion);
    feed(&mut s, b"\x1b[?1002l");
    assert_eq!(s.mouse_mode, MouseMode::None);
    feed(&mut s, b"\x1b[?1003h");
    assert_eq!(s.mouse_mode, MouseMode::AllMotion);
    feed(&mut s, b"\x1b[?1003l");
    assert_eq!(s.mouse_mode, MouseMode::None);
}

#[test]
fn test_focus_tracking_flag() {
    let mut s = Session::new(1, 80, 24, None);
    assert!(!s.focus_tracking);
    feed(&mut s, b"\x1b[?1004h");
    assert!(s.focus_tracking);
    feed(&mut s, b"\x1b[?1004l");
    assert!(!s.focus_tracking);
}

#[test]
fn test_mouse_sgr_flag() {
    let mut s = Session::new(1, 80, 24, None);
    assert!(!s.mouse_sgr);
    feed(&mut s, b"\x1b[?1006h");
    assert!(s.mouse_sgr);
    feed(&mut s, b"\x1b[?1006l");
    assert!(!s.mouse_sgr);
}

#[test]
fn test_bracketed_paste_flag() {
    let mut s = Session::new(1, 80, 24, None);
    assert!(!s.bracketed_paste);
    feed(&mut s, b"\x1b[?2004h");
    assert!(s.bracketed_paste);
    feed(&mut s, b"\x1b[?2004l");
    assert!(!s.bracketed_paste);
}

#[test]
fn test_cursor_visibility() {
    let mut s = Session::new(1, 80, 24, None);
    assert!(s.cursor_visible);
    feed(&mut s, b"\x1b[?25l");
    assert!(!s.cursor_visible);
    feed(&mut s, b"\x1b[?25h");
    assert!(s.cursor_visible);
}

// ── Performer: alt screen ?47 ─────────────────────────────────────────────────

#[test]
fn test_alt_screen_47_enter_leave_preserves_primary() {
    let mut s = Session::new(1, 10, 5, None);
    feed(&mut s, b"Primary");
    feed(&mut s, b"\x1b[?47h");
    assert!(s.saved_primary_grid.is_some());
    feed(&mut s, b"AltText");
    feed(&mut s, b"\x1b[?47l");
    assert!(s.saved_primary_grid.is_none());
    assert_eq!(row_text_trimmed(&s, 0), "Primary");
}

// ── Session::resize ───────────────────────────────────────────────────────────

#[test]
fn test_session_resize_clamps_cursor() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[8;18H"); // 0-based: (7, 17)
    s.resize(5, 5);
    assert!(s.cursor_x <= 4);
    assert!(s.cursor_y <= 4);
}

#[test]
fn test_session_resize_resets_pending_wrap() {
    let mut s = Session::new(1, 5, 5, None);
    feed(&mut s, b"ABCDE"); // fills row 0, sets pending_wrap
    assert!(s.pending_wrap);
    s.resize(10, 5);
    assert!(!s.pending_wrap);
}

#[test]
fn test_session_resize_clamps_scroll_region() {
    let mut s = Session::new(1, 80, 24, None);
    feed(&mut s, b"\x1b[5;20r"); // scroll region 4-19 (0-based)
    s.resize(80, 10);
    // scroll_bottom must be clamped to rows-1
    assert!(s.scroll_bottom <= 9);
}

#[test]
fn test_session_resize_syncs_alt_screen() {
    let mut s = Session::new(1, 20, 10, None);
    feed(&mut s, b"\x1b[?1049h"); // enter alt screen (saves primary)
    s.resize(40, 20);
    // Primary grid (saved) should also have been resized
    let primary = s.saved_primary_grid.as_ref().unwrap();
    assert_eq!(primary.cols, 40);
    assert_eq!(primary.rows, 20);
}

#[test]
fn test_session_resize_scroll_top_reset_when_too_large() {
    let mut s = Session::new(1, 80, 24, None);
    // Manually push scroll_top to a large value to simulate edge case
    s.scroll_top = 20;
    s.resize(80, 5); // rows = 5, so scroll_top >= rows
    assert_eq!(s.scroll_top, 0);
}
