use std::mem;
use std::path::PathBuf;
use vte::Perform;
use super::{Session, MouseMode};
use super::grid::{Cell, CellAttrs, Color, Grid};

pub struct Performer<'a> {
    pub session: &'a mut Session,
}

impl<'a> Performer<'a> {
    pub fn new(session: &'a mut Session) -> Self {
        Performer { session }
    }

    fn newline(&mut self) {
        self.session.pending_wrap = false;
        let top    = self.session.scroll_top;
        let bottom = self.session.scroll_bottom;
        let rows   = self.session.grid.rows;
        if self.session.cursor_y == bottom {
            // At the bottom of the scroll region — scroll the region up.
            self.session.grid.scroll_up(top, bottom, 1);
        } else if self.session.cursor_y < rows - 1 {
            // Anywhere else except the very last row — just move down.
            self.session.cursor_y += 1;
        }
        // cursor_y == rows - 1 and outside scroll region: don't move.
    }

    fn enter_alt_screen(&mut self, save_cursor: bool) {
        if self.session.saved_primary_grid.is_some() {
            return; // already in alt screen
        }
        if save_cursor {
            self.session.alt_saved_cursor =
                Some((self.session.cursor_x, self.session.cursor_y));
        }
        let cols = self.session.grid.cols;
        let rows = self.session.grid.rows;
        let old = mem::replace(&mut self.session.grid, Grid::new(cols, rows));
        self.session.saved_primary_grid = Some(old);
        self.session.scroll_top = 0;
        self.session.scroll_bottom = rows - 1;
        // Alt screen always starts cursor at home (xterm behaviour)
        self.session.cursor_x = 0;
        self.session.cursor_y = 0;
        self.session.pending_wrap = false;
    }

    fn leave_alt_screen(&mut self, restore_cursor: bool) {
        if let Some(primary) = self.session.saved_primary_grid.take() {
            self.session.grid = primary;
        }
        if restore_cursor {
            if let Some((x, y)) = self.session.alt_saved_cursor.take() {
                self.session.cursor_x = x;
                self.session.cursor_y = y;
            }
        }
        let rows = self.session.grid.rows;
        self.session.scroll_top = 0;
        self.session.scroll_bottom = rows - 1;
        self.session.pending_wrap = false;
    }

    fn sgr(&mut self, params: &vte::Params) {
        let p: Vec<u16> = params.iter().flat_map(|sub| sub.iter().copied()).collect();

        let mut i = 0;
        while i < p.len() {
            match p[i] {
                0 => {
                    self.session.current_fg = Color::Default;
                    self.session.current_bg = Color::Default;
                    self.session.current_attrs = CellAttrs::default();
                }
                1 => self.session.current_attrs.bold = true,
                2 => self.session.current_attrs.dim = true,
                3 => self.session.current_attrs.italic = true,
                4 => self.session.current_attrs.underline = true,
                5 | 6 => self.session.current_attrs.blink = true,
                7 => self.session.current_attrs.inverse = true,
                8 => self.session.current_attrs.invisible = true,
                9 => self.session.current_attrs.strikethrough = true,
                22 => {
                    self.session.current_attrs.bold = false;
                    self.session.current_attrs.dim = false;
                }
                23 => self.session.current_attrs.italic = false,
                24 => self.session.current_attrs.underline = false,
                25 => self.session.current_attrs.blink = false,
                27 => self.session.current_attrs.inverse = false,
                28 => self.session.current_attrs.invisible = false,
                29 => self.session.current_attrs.strikethrough = false,
                30..=37 => self.session.current_fg = Color::Indexed(p[i] as u8 - 30),
                38 => {
                    if p.get(i + 1).copied() == Some(2) && p.len() > i + 4 {
                        self.session.current_fg = Color::Rgb(
                            p[i + 2] as u8,
                            p[i + 3] as u8,
                            p[i + 4] as u8,
                        );
                        i += 4;
                    } else if p.get(i + 1).copied() == Some(5) && p.len() > i + 2 {
                        self.session.current_fg = Color::Indexed(p[i + 2] as u8);
                        i += 2;
                    }
                }
                39 => self.session.current_fg = Color::Default,
                40..=47 => self.session.current_bg = Color::Indexed(p[i] as u8 - 40),
                48 => {
                    if p.get(i + 1).copied() == Some(2) && p.len() > i + 4 {
                        self.session.current_bg = Color::Rgb(
                            p[i + 2] as u8,
                            p[i + 3] as u8,
                            p[i + 4] as u8,
                        );
                        i += 4;
                    } else if p.get(i + 1).copied() == Some(5) && p.len() > i + 2 {
                        self.session.current_bg = Color::Indexed(p[i + 2] as u8);
                        i += 2;
                    }
                }
                49 => self.session.current_bg = Color::Default,
                90..=97  => self.session.current_fg = Color::Indexed(p[i] as u8 - 90 + 8),
                100..=107 => self.session.current_bg = Color::Indexed(p[i] as u8 - 100 + 8),
                _ => {}
            }
            i += 1;
        }
    }
}

impl<'a> Perform for Performer<'a> {
    fn print(&mut self, c: char) {
        let cols = self.session.grid.cols;

        let cell = Cell {
            c,
            fg: self.session.current_fg,
            bg: self.session.current_bg,
            attrs: self.session.current_attrs,
        };

        // Deferred wrap: previous char filled the last column — wrap now.
        if self.session.pending_wrap {
            self.session.cursor_x = 0;
            self.newline(); // handles scroll region, clears pending_wrap
        }

        self.session.grid.set(self.session.cursor_y, self.session.cursor_x, cell);
        self.session.cursor_x += 1;

        if self.session.cursor_x >= cols {
            self.session.cursor_x = cols - 1;
            self.session.pending_wrap = true;
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => {} // BEL — ignore
            0x08 => {
                // Backspace
                if self.session.cursor_x > 0 {
                    self.session.cursor_x -= 1;
                }
            }
            0x09 => {
                // Horizontal tab — advance to next 8-col tab stop
                let next = (self.session.cursor_x / 8 + 1) * 8;
                self.session.cursor_x = next.min(self.session.grid.cols - 1);
            }
            0x0A | 0x0B | 0x0C => self.newline(),
            0x0D => {
                self.session.cursor_x = 0;
                self.session.pending_wrap = false;
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, action: char) {
        let mut iter = params.iter();
        let p0 = iter.next().and_then(|p| p.first()).copied().unwrap_or(0);
        let p1 = iter.next().and_then(|p| p.first()).copied().unwrap_or(0);

        let cols = self.session.grid.cols;
        let rows = self.session.grid.rows;

        match action {
            'A' => {
                let n = p0.max(1);
                self.session.pending_wrap = false;
                self.session.cursor_y = self.session.cursor_y.saturating_sub(n);
            }
            'B' => {
                let n = p0.max(1);
                self.session.pending_wrap = false;
                self.session.cursor_y = self.session.cursor_y.saturating_add(n).min(rows - 1);
            }
            'C' => {
                let n = p0.max(1);
                self.session.pending_wrap = false;
                self.session.cursor_x = self.session.cursor_x.saturating_add(n).min(cols - 1);
            }
            'D' => {
                let n = p0.max(1);
                self.session.pending_wrap = false;
                self.session.cursor_x = self.session.cursor_x.saturating_sub(n);
            }
            'E' => {
                let n = p0.max(1);
                self.session.pending_wrap = false;
                self.session.cursor_y = self.session.cursor_y.saturating_add(n).min(rows - 1);
                self.session.cursor_x = 0;
            }
            'F' => {
                let n = p0.max(1);
                self.session.pending_wrap = false;
                self.session.cursor_y = self.session.cursor_y.saturating_sub(n);
                self.session.cursor_x = 0;
            }
            'G' => {
                self.session.pending_wrap = false;
                self.session.cursor_x = (p0.max(1) - 1).min(cols - 1);
            }
            'H' | 'f' => {
                // CUP / HVP — cursor position (1-based)
                self.session.pending_wrap = false;
                self.session.cursor_y = (p0.max(1) - 1).min(rows - 1);
                self.session.cursor_x = (p1.max(1) - 1).min(cols - 1);
            }
            'J' => {
                let bg = self.session.current_bg;
                match p0 {
                    0 => {
                        let cy = self.session.cursor_y;
                        let cx = self.session.cursor_x;
                        self.session.grid.erase_line_range(cy, cx, cols - 1, bg);
                        if cy + 1 < rows {
                            self.session.grid.erase_row_range(cy + 1, rows - 1, bg);
                        }
                    }
                    1 => {
                        let cy = self.session.cursor_y;
                        let cx = self.session.cursor_x;
                        if cy > 0 {
                            self.session.grid.erase_row_range(0, cy - 1, bg);
                        }
                        self.session.grid.erase_line_range(cy, 0, cx, bg);
                    }
                    2 | 3 => {
                        // ED 2/3: erase screen; cursor does NOT move (VT100/xterm spec)
                        self.session.grid.clear_all_with_bg(bg);
                    }
                    _ => {}
                }
            }
            'K' => {
                let cy = self.session.cursor_y;
                let cx = self.session.cursor_x;
                let bg = self.session.current_bg;
                match p0 {
                    0 => self.session.grid.erase_line_range(cy, cx, cols - 1, bg),
                    1 => self.session.grid.erase_line_range(cy, 0, cx, bg),
                    2 => self.session.grid.erase_line_range(cy, 0, cols - 1, bg),
                    _ => {}
                }
            }
            '@' => {
                // ICH — insert characters (shift existing chars right, blank left)
                let n = p0.max(1) as usize;
                let cy = self.session.cursor_y;
                let cx = self.session.cursor_x as usize;
                let end = cols as usize;
                let bg = self.session.current_bg;
                for col in (cx..end).rev() {
                    let cell = if col >= cx + n {
                        *self.session.grid.get(cy, (col - n) as u16)
                    } else {
                        Cell { c: ' ', fg: Color::Default, bg, attrs: CellAttrs::default() }
                    };
                    self.session.grid.set(cy, col as u16, cell);
                }
            }
            'L' => {
                // IL — insert lines (scroll down within scroll region from cursor)
                let n = p0.max(1);
                let cy = self.session.cursor_y;
                self.session.grid.scroll_down(cy, self.session.scroll_bottom, n);
            }
            'M' => {
                // DL — delete lines (scroll up within scroll region from cursor)
                let n = p0.max(1);
                let cy = self.session.cursor_y;
                self.session.grid.scroll_up(cy, self.session.scroll_bottom, n);
            }
            'P' => {
                // DCH — delete characters (shift left, blank right with current bg)
                let n = p0.max(1) as usize;
                let cy = self.session.cursor_y;
                let cx = self.session.cursor_x as usize;
                let end = cols as usize;
                let bg = self.session.current_bg;
                for col in cx..end {
                    let src = col + n;
                    let cell = if src < end {
                        *self.session.grid.get(cy, src as u16)
                    } else {
                        Cell { c: ' ', fg: Color::Default, bg, attrs: CellAttrs::default() }
                    };
                    self.session.grid.set(cy, col as u16, cell);
                }
            }
            'S' => {
                let n = p0.max(1);
                self.session.grid.scroll_up(
                    self.session.scroll_top,
                    self.session.scroll_bottom,
                    n,
                );
            }
            'T' => {
                let n = p0.max(1);
                self.session.grid.scroll_down(
                    self.session.scroll_top,
                    self.session.scroll_bottom,
                    n,
                );
            }
            'X' => {
                // ECH — erase characters (with current background color)
                let n = p0.max(1);
                let cy = self.session.cursor_y;
                let cx = self.session.cursor_x;
                let end = cx.saturating_add(n - 1).min(cols - 1);
                let bg = self.session.current_bg;
                self.session.grid.erase_line_range(cy, cx, end, bg);
            }
            'c' if intermediates == b"" => {
                // DA1 — Primary Device Attributes: claim VT220 with color
                self.session.pending_dsr_response
                    .push("\x1b[?62;1;22c".to_string());
            }
            'c' if intermediates == b">" => {
                // DA2 — Secondary Device Attributes: xterm-compatible
                self.session.pending_dsr_response
                    .push("\x1b[>0;10;1c".to_string());
            }
            'd' => {
                self.session.pending_wrap = false;
                self.session.cursor_y = (p0.max(1) - 1).min(rows - 1);
            }
            'h' if intermediates == b"?" => {
                match p0 {
                    25   => self.session.cursor_visible = true,
                    47 | 1047 => self.enter_alt_screen(false),
                    1049 => self.enter_alt_screen(true),
                    1000 => self.session.mouse_mode = MouseMode::Basic,
                    1002 => self.session.mouse_mode = MouseMode::ButtonMotion,
                    1003 => self.session.mouse_mode = MouseMode::AllMotion,
                    1004 => self.session.focus_tracking = true,
                    1006 => self.session.mouse_sgr = true,
                    2004 => self.session.bracketed_paste = true,
                    _ => {}
                }
            }
            'l' if intermediates == b"?" => {
                match p0 {
                    25   => self.session.cursor_visible = false,
                    47 | 1047 => self.leave_alt_screen(false),
                    1049 => self.leave_alt_screen(true),
                    1000 | 1002 | 1003 => self.session.mouse_mode = MouseMode::None,
                    1004 => self.session.focus_tracking = false,
                    1006 => self.session.mouse_sgr = false,
                    2004 => self.session.bracketed_paste = false,
                    _ => {}
                }
            }
            'm' => self.sgr(params),
            'n' => {
                match p0 {
                    5 => {
                        // DSR — device status: terminal OK
                        self.session.pending_dsr_response.push("\x1b[0n".to_string());
                    }
                    6 => {
                        // CPR — cursor position report (1-based)
                        let row = self.session.cursor_y + 1;
                        let col = self.session.cursor_x + 1;
                        self.session.pending_dsr_response
                            .push(format!("\x1b[{};{}R", row, col));
                    }
                    _ => {}
                }
            }
            'r' => {
                // DECSTBM — set scrolling region (1-based, 0 means default)
                let top = if p0 == 0 { 1 } else { p0 };
                let bot = if p1 == 0 { rows } else { p1 };
                // Validate: top < bottom and both within grid
                if top < bot && bot <= rows {
                    self.session.scroll_top = top - 1;
                    self.session.scroll_bottom = bot - 1;
                } else {
                    self.session.scroll_top = 0;
                    self.session.scroll_bottom = rows - 1;
                }
                // DECSTBM always moves cursor to home
                self.session.cursor_x = 0;
                self.session.cursor_y = 0;
                self.session.pending_wrap = false;
            }
            's' => {
                self.session.saved_cursor =
                    Some((self.session.cursor_x, self.session.cursor_y));
            }
            'u' => {
                if let Some((x, y)) = self.session.saved_cursor {
                    self.session.pending_wrap = false;
                    self.session.cursor_x = x;
                    self.session.cursor_y = y;
                }
            }
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates, byte) {
            (b"", b'7') | (b"", b's') => {
                self.session.saved_cursor =
                    Some((self.session.cursor_x, self.session.cursor_y));
            }
            (b"", b'8') | (b"", b'u') => {
                if let Some((x, y)) = self.session.saved_cursor {
                    self.session.pending_wrap = false;
                    self.session.cursor_x = x;
                    self.session.cursor_y = y;
                }
            }
            (b"", b'M') => {
                // RI — reverse index (scroll down if at top of scroll region)
                self.session.pending_wrap = false;
                if self.session.cursor_y == self.session.scroll_top {
                    self.session.grid.scroll_down(
                        self.session.scroll_top,
                        self.session.scroll_bottom,
                        1,
                    );
                } else if self.session.cursor_y > 0 {
                    self.session.cursor_y -= 1;
                }
            }
            (b"", b'c') => {
                // RIS — reset to initial state
                self.session.grid.clear_all();
                self.session.cursor_x = 0;
                self.session.cursor_y = 0;
                self.session.pending_wrap = false;
                self.session.scroll_top = 0;
                self.session.scroll_bottom = self.session.grid.rows - 1;
                self.session.current_fg = Color::Default;
                self.session.current_bg = Color::Default;
                self.session.current_attrs = CellAttrs::default();
            }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.is_empty() {
            return;
        }

        match params[0] {
            b"0" | b"2" => {
                if let Some(title_bytes) = params.get(1) {
                    if let Ok(title) = std::str::from_utf8(title_bytes) {
                        self.session.title = title.to_owned();
                    }
                }
            }
            b"7" => {
                if let Some(uri_bytes) = params.get(1) {
                    if let Ok(uri) = std::str::from_utf8(uri_bytes) {
                        let path_str = if uri.starts_with("file:///") {
                            uri.trim_start_matches("file:///")
                        } else if uri.starts_with("file://") {
                            let rest = uri.trim_start_matches("file://");
                            rest.find('/').map(|i| &rest[i..]).unwrap_or(rest)
                        } else {
                            return;
                        };

                        #[cfg(target_os = "windows")]
                        let path_str = path_str.replace('/', "\\");

                        self.session.cwd = PathBuf::from(&*path_str);
                        self.session.prompt_ready = true;
                        log::debug!("CWD: {:?}", self.session.cwd);
                    }
                }
            }
            _ => {}
        }
    }
}
