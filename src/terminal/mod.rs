pub mod grid;
pub mod performer;
mod tests;

use std::path::PathBuf;
use grid::{CellAttrs, Color, Grid};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MouseMode {
    None,
    Basic,          // ?1000 — click only
    ButtonMotion,   // ?1002 — click + drag
    AllMotion,      // ?1003 — all movement
}

pub struct Session {
    pub id: u32,
    pub grid: Grid,

    // Cursor state
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub cursor_visible: bool,
    pub pending_wrap: bool,
    pub saved_cursor: Option<(u16, u16)>,

    // Current SGR state (applied to newly printed cells)
    pub current_fg: Color,
    pub current_bg: Color,
    pub current_attrs: CellAttrs,

    // Scroll region (DECSTBM) — 0-based inclusive rows
    pub scroll_top: u16,
    pub scroll_bottom: u16,

    // Alternate screen — holds the primary grid while ?47/?1049 is active
    pub saved_primary_grid: Option<Grid>,
    // Cursor position saved on ?1049h entry, restored on ?1049l exit
    pub alt_saved_cursor: Option<(u16, u16)>,

    // Terminal mode flags set by the running application
    pub bracketed_paste: bool,
    pub focus_tracking: bool,
    pub mouse_mode: MouseMode,
    pub mouse_sgr: bool,   // ?1006 — SGR extended mouse coords

    // Response bytes to be written back to the PTY on the next update tick
    pub pending_dsr_response: Vec<String>,

    pub cwd: PathBuf,
    pub title: String,

    // Set to true on the first OSC 7 (shell prompt ready); used to delay command replay.
    pub prompt_ready: bool,
}

impl Session {
    pub fn new(id: u32, cols: u16, rows: u16, cwd: Option<PathBuf>) -> Self {
        Session {
            id,
            grid: Grid::new(cols, rows),
            cursor_x: 0,
            cursor_y: 0,
            cursor_visible: true,
            pending_wrap: false,
            saved_cursor: None,
            current_fg: Color::Default,
            current_bg: Color::Default,
            current_attrs: CellAttrs::default(),
            scroll_top: 0,
            scroll_bottom: rows - 1,
            saved_primary_grid: None,
            alt_saved_cursor: None,
            bracketed_paste: false,
            focus_tracking: false,
            mouse_mode: MouseMode::None,
            mouse_sgr: false,
            pending_dsr_response: Vec::new(),
            cwd: cwd.unwrap_or_default(),
            title: format!("Session {}", id),
            prompt_ready: false,
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.grid.resize(cols, rows);
        // Also resize the stashed primary grid so it stays in sync
        if let Some(ref mut primary) = self.saved_primary_grid {
            primary.resize(cols, rows);
        }
        self.cursor_x = self.cursor_x.min(cols - 1);
        self.cursor_y = self.cursor_y.min(rows - 1);
        self.pending_wrap = false;
        // Clamp scroll region to new dimensions
        self.scroll_bottom = self.scroll_bottom.min(rows - 1);
        if self.scroll_top >= rows {
            self.scroll_top = 0;
        }
    }
}
