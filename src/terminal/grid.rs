use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellAttrs {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub blink: bool,
    pub inverse: bool,
    pub invisible: bool,
    pub strikethrough: bool,
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
            scrollback_limit: 10_000,
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
        for _ in 0..count {
            if top == 0 {
                let row_end = self.cols as usize;
                let saved: Vec<Cell> = self.cells[..row_end].to_vec();
                self.scrollback.push_back(saved);
                if self.scrollback.len() > self.scrollback_limit {
                    self.scrollback.pop_front();
                }
            }

            // Shift rows up within [top, bottom]
            for row in top..bottom {
                for col in 0..self.cols {
                    let above = self.idx(row, col);
                    let below = self.idx(row + 1, col);
                    self.cells[above] = self.cells[below];
                }
            }
            // Clear the bottom row
            for col in 0..self.cols {
                let i = self.idx(bottom, col);
                self.cells[i] = Cell::default();
            }
        }
    }

    /// Scroll down (insert blank lines at top, push bottom rows off).
    pub fn scroll_down(&mut self, top: u16, bottom: u16, count: u16) {
        for _ in 0..count {
            for row in (top..bottom).rev() {
                for col in 0..self.cols {
                    let src = self.idx(row, col);
                    let dst = self.idx(row + 1, col);
                    self.cells[dst] = self.cells[src];
                }
            }
            for col in 0..self.cols {
                let i = self.idx(top, col);
                self.cells[i] = Cell::default();
            }
        }
    }

    /// Erase cells in [col_start, col_end] on the given row, inclusive.
    /// Uses `bg` as the erase background color (BCE — background color erase).
    pub fn erase_line_range(&mut self, row: u16, col_start: u16, col_end: u16, bg: Color) {
        let end = col_end.min(self.cols - 1);
        for col in col_start..=end {
            let i = self.idx(row, col);
            self.cells[i] = Cell {
                c: ' ',
                fg: Color::Default,
                bg,
                attrs: CellAttrs::default(),
            };
        }
    }

    /// Erase all cells in [row_start, row_end] inclusive, full rows.
    /// Uses `bg` as the erase background color (BCE).
    pub fn erase_row_range(&mut self, row_start: u16, row_end: u16, bg: Color) {
        let end = row_end.min(self.rows - 1);
        for row in row_start..=end {
            for col in 0..self.cols {
                let i = self.idx(row, col);
                self.cells[i] = Cell {
                    c: ' ',
                    fg: Color::Default,
                    bg,
                    attrs: CellAttrs::default(),
                };
            }
        }
    }

    pub fn resize(&mut self, new_cols: u16, new_rows: u16) {
        let mut new_cells = vec![Cell::default(); new_cols as usize * new_rows as usize];
        let copy_rows = self.rows.min(new_rows) as usize;
        let copy_cols = self.cols.min(new_cols) as usize;
        for row in 0..copy_rows {
            for col in 0..copy_cols {
                let src = row * self.cols as usize + col;
                let dst = row * new_cols as usize + col;
                new_cells[dst] = self.cells[src];
            }
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
