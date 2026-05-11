use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// Packed attribute bitmask — 1 byte instead of 8 separate bools.
/// Reduces Cell from 20 bytes to 16 bytes for better cache utilization
/// and lower memory pressure when many sessions have deep scrollback.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellAttrs(pub u8);

#[allow(dead_code)]
impl CellAttrs {
    pub const BOLD:          u8 = 1 << 0;
    pub const DIM:           u8 = 1 << 1;
    pub const ITALIC:        u8 = 1 << 2;
    pub const UNDERLINE:     u8 = 1 << 3;
    pub const BLINK:         u8 = 1 << 4;
    pub const INVERSE:       u8 = 1 << 5;
    pub const INVISIBLE:     u8 = 1 << 6;
    pub const STRIKETHROUGH: u8 = 1 << 7;

    #[inline] pub fn bold(self) -> bool          { self.0 & Self::BOLD != 0 }
    #[inline] pub fn dim(self) -> bool            { self.0 & Self::DIM != 0 }
    #[inline] pub fn italic(self) -> bool         { self.0 & Self::ITALIC != 0 }
    #[inline] pub fn underline(self) -> bool      { self.0 & Self::UNDERLINE != 0 }
    #[inline] pub fn blink(self) -> bool          { self.0 & Self::BLINK != 0 }
    #[inline] pub fn inverse(self) -> bool        { self.0 & Self::INVERSE != 0 }
    #[inline] pub fn invisible(self) -> bool      { self.0 & Self::INVISIBLE != 0 }
    #[inline] pub fn strikethrough(self) -> bool  { self.0 & Self::STRIKETHROUGH != 0 }

    #[inline]
    #[allow(dead_code)]
    pub fn set(&mut self, flag: u8, on: bool) {
        if on { self.0 |= flag } else { self.0 &= !flag }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: CellAttrs,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            c: ' ',
            fg: Color::Default,
            bg: Color::Default,
            attrs: CellAttrs::default(),
        }
    }
}

pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    cells: Vec<Cell>,
    pub scrollback: VecDeque<Vec<Cell>>,
    pub scrollback_limit: usize,
}

impl Grid {
    pub fn new(cols: u16, rows: u16) -> Self {
        let n = cols as usize * rows as usize;
        Grid {
            cols,
            rows,
            cells: vec![Cell::default(); n],
            scrollback: VecDeque::new(),
            scrollback_limit: 2_000,
        }
    }

    #[inline]
    fn idx(&self, row: u16, col: u16) -> usize {
        row as usize * self.cols as usize + col as usize
    }

    pub fn get(&self, row: u16, col: u16) -> &Cell {
        &self.cells[self.idx(row, col)]
    }

    #[allow(dead_code)]
    pub fn get_mut(&mut self, row: u16, col: u16) -> &mut Cell {
        let i = self.idx(row, col);
        &mut self.cells[i]
    }

    pub fn set(&mut self, row: u16, col: u16, cell: Cell) {
        let i = self.idx(row, col);
        self.cells[i] = cell;
    }

    /// Scroll the visible grid up by `count` lines within [top, bottom].
    /// Only saves to scrollback when top == 0 (normal terminal scroll).
    pub fn scroll_up(&mut self, top: u16, bottom: u16, count: u16) {
        let cols = self.cols as usize;
        for _ in 0..count {
            if top == 0 {
                // When at capacity, reuse the evicted front buffer to avoid
                // a malloc+free on every scroll line (the common steady state).
                let saved = if self.scrollback.len() >= self.scrollback_limit {
                    let mut buf = self.scrollback.pop_front().unwrap();
                    buf.clear();
                    buf.extend_from_slice(&self.cells[..cols]);
                    buf
                } else {
                    self.cells[..cols].to_vec()
                };
                self.scrollback.push_back(saved);
            }

            if bottom > top {
                let src = self.idx(top + 1, 0);
                let dst = self.idx(top, 0);
                let n = (bottom - top) as usize * cols;
                self.cells.copy_within(src..src + n, dst);
            }
            let bot = self.idx(bottom, 0);
            self.cells[bot..bot + cols].fill(Cell::default());
        }
    }

    /// Scroll down (insert blank lines at top, push bottom rows off).
    pub fn scroll_down(&mut self, top: u16, bottom: u16, count: u16) {
        let cols = self.cols as usize;
        for _ in 0..count {
            if bottom > top {
                let src = self.idx(top, 0);
                let dst = self.idx(top + 1, 0);
                let n = (bottom - top) as usize * cols;
                // copy_within uses memmove semantics, safe for overlapping ranges
                self.cells.copy_within(src..src + n, dst);
            }
            let top_start = self.idx(top, 0);
            self.cells[top_start..top_start + cols].fill(Cell::default());
        }
    }

    /// Erase cells in [col_start, col_end] on the given row, inclusive.
    /// Uses `bg` as the erase background color (BCE — background color erase).
    pub fn erase_line_range(&mut self, row: u16, col_start: u16, col_end: u16, bg: Color) {
        let col_end = col_end.min(self.cols - 1);
        let blank = Cell { c: ' ', fg: Color::Default, bg, attrs: CellAttrs::default() };
        let start = self.idx(row, col_start);
        let end = self.idx(row, col_end) + 1;
        self.cells[start..end].fill(blank);
    }

    /// Erase all cells in [row_start, row_end] inclusive, full rows.
    /// Uses `bg` as the erase background color (BCE).
    pub fn erase_row_range(&mut self, row_start: u16, row_end: u16, bg: Color) {
        let row_end = row_end.min(self.rows - 1);
        let blank = Cell { c: ' ', fg: Color::Default, bg, attrs: CellAttrs::default() };
        let start = self.idx(row_start, 0);
        let end = self.idx(row_end, 0) + self.cols as usize;
        self.cells[start..end].fill(blank);
    }

    pub fn resize(&mut self, new_cols: u16, new_rows: u16) {
        let mut new_cells = vec![Cell::default(); new_cols as usize * new_rows as usize];
        let copy_rows = self.rows.min(new_rows) as usize;
        let copy_cols = self.cols.min(new_cols) as usize;
        let old_cols = self.cols as usize;
        let new_cols_usize = new_cols as usize;
        for row in 0..copy_rows {
            let src = row * old_cols;
            let dst = row * new_cols_usize;
            new_cells[dst..dst + copy_cols].copy_from_slice(&self.cells[src..src + copy_cols]);
        }
        self.cols = new_cols;
        self.rows = new_rows;
        self.cells = new_cells;
    }

    pub fn clear_all(&mut self) {
        self.cells.fill(Cell::default());
    }

    /// Clear all cells using `bg` as the erase background color (BCE).
    pub fn clear_all_with_bg(&mut self, bg: Color) {
        let cell = Cell {
            c: ' ',
            fg: Color::Default,
            bg,
            attrs: CellAttrs::default(),
        };
        self.cells.fill(cell);
    }
}

/// Resolve indexed/default colors to RGB for rendering.
pub fn ansi_color(index: u8) -> (u8, u8, u8) {
    match index {
        // Standard 16 colors (dark theme)
        0 => (30, 30, 46),    // black
        1 => (243, 139, 168), // red
        2 => (166, 227, 161), // green
        3 => (249, 226, 175), // yellow
        4 => (137, 180, 250), // blue
        5 => (245, 194, 231), // magenta
        6 => (148, 226, 213), // cyan
        7 => (205, 214, 244), // white
        8 => (88, 91, 112),   // bright black
        9 => (243, 139, 168),
        10 => (166, 227, 161),
        11 => (249, 226, 175),
        12 => (137, 180, 250),
        13 => (245, 194, 231),
        14 => (148, 226, 213),
        15 => (255, 255, 255),
        // 6x6x6 color cube: indices 16-231
        16..=231 => {
            let n = index - 16;
            let b = (n % 6) * 51;
            let g = ((n / 6) % 6) * 51;
            let r = (n / 36) * 51;
            (r, g, b)
        }
        // Grayscale: indices 232-255
        232..=255 => {
            let v = 8 + (index - 232) * 10;
            (v, v, v)
        }
    }
}
