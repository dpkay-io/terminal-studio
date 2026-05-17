use std::cell::RefCell;
use std::sync::Arc;

use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line},
    term::{cell::Flags, TermMode},
    vte::ansi::{Color, NamedColor, Rgb},
};
use egui::{vec2, FontId, Pos2, Rect, Sense};
use parking_lot::RwLock;

use crate::app::settings::CursorStyle;
use crate::terminal::Session;
use crate::theme;

pub struct SelectionRange {
    pub start_col: u16,
    pub start_row: u16,
    pub end_col: u16,
    pub end_row: u16,
}

impl SelectionRange {
    fn ordered(&self) -> (u16, u16, u16, u16) {
        if self.start_row < self.end_row
            || (self.start_row == self.end_row && self.start_col <= self.end_col)
        {
            (self.start_col, self.start_row, self.end_col, self.end_row)
        } else {
            (self.end_col, self.end_row, self.start_col, self.start_row)
        }
    }
}

pub struct TerminalGeometry {
    pub rect: Rect,
    pub cell_w: f32,
    pub cell_h: f32,
}

impl TerminalGeometry {
    pub fn to_cell(&self, pos: Pos2) -> Option<(u16, u16)> {
        if !self.rect.contains(pos) {
            return None;
        }
        let col = ((pos.x - self.rect.min.x) / self.cell_w) as u16;
        let row = ((pos.y - self.rect.min.y) / self.cell_h) as u16;
        Some((col, row))
    }
}

/// Pre-resolved per-cell render data. Filled while holding the session
/// read-lock briefly, then read without the lock during paint so the PTY
/// reader thread isn't blocked by the (much slower) text-shaping pass.
#[derive(Clone, Copy)]
struct CellInfo {
    ch: char,
    fg: egui::Color32,
    bg: egui::Color32,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

impl CellInfo {
    const EMPTY: CellInfo = CellInfo {
        ch: ' ',
        fg: egui::Color32::TRANSPARENT,
        bg: egui::Color32::TRANSPARENT,
        bold: false,
        italic: false,
        underline: false,
        strike: false,
    };
}

thread_local! {
    /// Reusable per-frame cell buffer for terminal rendering. UI runs on a
    /// single thread, so a thread-local is safe and avoids re-allocating a
    /// ~150 KB Vec every paint.
    static RENDER_BUF: RefCell<Vec<CellInfo>> = const { RefCell::new(Vec::new()) };
}

pub struct TerminalView {
    session: Arc<RwLock<Session>>,
}

impl TerminalView {
    pub fn new(session: Arc<RwLock<Session>>) -> Self {
        Self { session }
    }

    /// Render the terminal. `cursor_visible` toggles the blink cycle.
    /// Scroll position is read from the term's internal `display_offset`.
    pub fn show(
        &self,
        ui: &mut egui::Ui,
        is_focused: bool,
        cursor_visible: bool,
        selection: Option<&SelectionRange>,
        font_size: f32,
        cursor_style: CursorStyle,
    ) -> TerminalGeometry {
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::active().bg_term);

        let font_id = FontId::monospace(font_size);
        let cell_height = ui.fonts(|f| f.row_height(&font_id));
        let cell_width = ui.fonts(|f| f.glyph_width(&font_id, 'M'));

        let visible_rows = (rect.height() / cell_height) as usize;
        let visible_cols = (rect.width() / cell_width) as usize;

        // ── Snapshot phase: copy out everything we need from the Term under
        //    the read lock, then drop the lock before painting. This keeps
        //    the PTY reader thread (which takes the write lock) from being
        //    blocked for the full paint duration.
        let snapshot = RENDER_BUF.with(|buf| {
            let mut buf = buf.borrow_mut();
            buf.clear();

            let session = self.session.read();
            let term = &session.term;
            let grid = term.grid();
            let term_cols = term.columns();
            let term_rows = term.screen_lines();
            let cols = term_cols.min(visible_cols);
            let display_offset = grid.display_offset();
            let history = grid.history_size();
            let show_cursor = term.mode().contains(TermMode::SHOW_CURSOR);
            buf.reserve(visible_rows.saturating_mul(cols));

            let min_line = -(history as i32);
            let max_line = term_rows as i32 - 1;
            for screen_row in 0..visible_rows {
                let grid_line = screen_row as i32 - display_offset as i32;
                if grid_line < min_line || grid_line > max_line {
                    for _ in 0..cols {
                        buf.push(CellInfo::EMPTY);
                    }
                    continue;
                }
                for col in 0..cols {
                    let cell = &grid[Line(grid_line)][Column(col)];
                    let inv = cell.flags.contains(Flags::INVERSE);
                    let eff_fg = if inv { cell.bg } else { cell.fg };
                    let eff_bg = if inv { cell.fg } else { cell.bg };
                    let hidden = cell.flags.contains(Flags::HIDDEN);
                    let spacer = cell.flags.contains(Flags::WIDE_CHAR_SPACER);
                    let ch = if hidden || spacer { ' ' } else { cell.c };
                    let mut fg = resolve_color(eff_fg, true);
                    if cell.flags.contains(Flags::DIM) {
                        let [r, g, b, a] = fg.to_array();
                        fg = egui::Color32::from_rgba_unmultiplied(r, g, b, a / 2);
                    }
                    buf.push(CellInfo {
                        ch,
                        fg,
                        bg: resolve_color(eff_bg, false),
                        bold: cell.flags.contains(Flags::BOLD),
                        italic: cell.flags.contains(Flags::ITALIC),
                        underline: cell.flags.contains(Flags::UNDERLINE),
                        strike: cell.flags.contains(Flags::STRIKEOUT),
                    });
                }
            }

            let cursor = if show_cursor && display_offset == 0 {
                let pt = grid.cursor.point;
                let cx = pt.column.0;
                let cy = pt.line.0;
                if cy >= 0 && cx < cols && (cy as usize) < term_rows {
                    Some((cx, cy as usize))
                } else {
                    None
                }
            } else {
                None
            };

            // Drop the read lock by leaving the closure body.
            drop(session);

            (cols, display_offset, history, cursor)
        });

        let (cols, display_offset, history, cursor) = snapshot;

        // ── Paint phase: no lock held. Reads from the thread-local buffer. ─
        let mut text_buf = String::with_capacity(cols + 1);

        RENDER_BUF.with(|buf| {
            let buf = buf.borrow();
            for screen_row in 0..visible_rows {
                let y = rect.min.y + screen_row as f32 * cell_height;

                let row_off = screen_row * cols;
                if row_off >= buf.len() {
                    break;
                }

                let mut bg_run_start = 0usize;
                let mut bg_run_color = egui::Color32::TRANSPARENT;

                let mut span_start = 0usize;
                let mut span_fg = egui::Color32::TRANSPARENT;
                let mut span_underline = false;
                let mut span_strike = false;
                let mut span_bold = false;
                let mut span_italic = false;

                for col in 0..=cols {
                    let is_end = col == cols;

                    let (new_bg, cell_fg, cell_ul, cell_st, cell_bold, cell_italic, cell_char) =
                        if !is_end {
                            let c = buf[row_off + col];
                            (c.bg, c.fg, c.underline, c.strike, c.bold, c.italic, c.ch)
                        } else {
                            (
                                egui::Color32::TRANSPARENT,
                                egui::Color32::TRANSPARENT,
                                false,
                                false,
                                false,
                                false,
                                ' ',
                            )
                        };

                    // ── BG flush ────────────────────────────────────────────────────
                    if new_bg != bg_run_color {
                        if bg_run_color != egui::Color32::TRANSPARENT && bg_run_start < col {
                            painter.rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(rect.min.x + bg_run_start as f32 * cell_width, y),
                                    egui::pos2(
                                        rect.min.x + col as f32 * cell_width,
                                        y + cell_height,
                                    ),
                                ),
                                0.0,
                                bg_run_color,
                            );
                        }
                        bg_run_start = col;
                        bg_run_color = new_bg;
                    }

                    // ── FG flush ────────────────────────────────────────────────────
                    let fg_changed = is_end
                        || cell_fg != span_fg
                        || cell_ul != span_underline
                        || cell_st != span_strike
                        || cell_bold != span_bold
                        || cell_italic != span_italic;

                    if fg_changed {
                        if !text_buf.is_empty() {
                            let visible = text_buf.trim_end_matches(' ');
                            if !visible.is_empty() {
                                let span_font = if span_bold {
                                    FontId::monospace(font_size + 0.5)
                                } else {
                                    FontId::monospace(font_size)
                                };
                                painter.text(
                                    egui::pos2(rect.min.x + span_start as f32 * cell_width, y),
                                    egui::Align2::LEFT_TOP,
                                    visible,
                                    span_font,
                                    span_fg,
                                );
                            }
                            if span_underline && col > span_start {
                                let uy = y + cell_height - 1.5;
                                painter.line_segment(
                                    [
                                        egui::pos2(rect.min.x + span_start as f32 * cell_width, uy),
                                        egui::pos2(rect.min.x + col as f32 * cell_width, uy),
                                    ],
                                    egui::Stroke::new(1.0, span_fg),
                                );
                            }
                            if span_strike && col > span_start {
                                let sy = y + cell_height * 0.5;
                                painter.line_segment(
                                    [
                                        egui::pos2(rect.min.x + span_start as f32 * cell_width, sy),
                                        egui::pos2(rect.min.x + col as f32 * cell_width, sy),
                                    ],
                                    egui::Stroke::new(1.0, span_fg),
                                );
                            }
                            text_buf.clear();
                        }
                        span_start = col;
                        span_fg = cell_fg;
                        span_underline = cell_ul;
                        span_strike = cell_st;
                        span_bold = cell_bold;
                        span_italic = cell_italic;
                    }

                    if !is_end {
                        text_buf.push(cell_char);
                    }
                }
            }
        });

        // ── Cursor ─────────────────────────────────────────────────────────────
        // Only draw cursor in live view (display_offset == 0) and when cursor
        // should be visible per hardware (SHOW_CURSOR mode) and blink phase.
        // The snapshot phase already filtered for SHOW_CURSOR + live-view +
        // bounds, so `cursor` is Some only when we should consider drawing.
        if let Some((cx, cy)) = cursor {
            if cursor_visible || !is_focused {
                let cursor_origin = rect.min + vec2(cx as f32 * cell_width, cy as f32 * cell_height);
                if is_focused {
                    match cursor_style {
                        CursorStyle::Block => {
                            let cursor_rect = Rect::from_min_size(
                                cursor_origin,
                                vec2(cell_width, cell_height),
                            );
                            painter.rect_filled(cursor_rect, 0.0, theme::active().cursor_color);
                        }
                        CursorStyle::Underline => {
                            let cursor_rect = Rect::from_min_size(
                                egui::pos2(cursor_origin.x, cursor_origin.y + cell_height - 2.0),
                                vec2(cell_width, 2.0),
                            );
                            painter.rect_filled(cursor_rect, 0.0, theme::active().cursor_color);
                        }
                        CursorStyle::Beam => {
                            let cursor_rect = Rect::from_min_size(
                                cursor_origin,
                                vec2(2.0, cell_height),
                            );
                            painter.rect_filled(cursor_rect, 0.0, theme::active().cursor_color);
                        }
                    }
                } else {
                    let cursor_rect = Rect::from_min_size(
                        cursor_origin,
                        vec2(cell_width, cell_height),
                    );
                    painter.rect_stroke(
                        cursor_rect,
                        0.0,
                        egui::Stroke::new(1.5, theme::active().cursor_dim_color),
                    );
                }
            }
        }

        // ── Selection highlight ────────────────────────────────────────────────
        if let Some(sel) = selection {
            let sel_color = theme::active().selection_bg;
            let (sc, sr, ec, er) = sel.ordered();
            for screen_row in sr..=er.min(visible_rows as u16 - 1) {
                let y = rect.min.y + screen_row as f32 * cell_height;
                let start_col = if screen_row == sr { sc } else { 0 };
                let end_col = if screen_row == er { ec + 1 } else { cols as u16 };
                let x0 = rect.min.x + start_col as f32 * cell_width;
                let x1 = rect.min.x + end_col as f32 * cell_width;
                painter.rect_filled(
                    Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + cell_height)),
                    0.0,
                    sel_color,
                );
            }
        }

        // ── Scrollback indicator ───────────────────────────────────────────────
        if history > 0 {
            let bar_w = 3.0_f32;
            let total_lines = history + visible_rows;
            let thumb_frac = (visible_rows as f32 / total_lines as f32).min(1.0);
            let thumb_h = (rect.height() * thumb_frac).max(4.0);
            let lines_above = history.saturating_sub(display_offset);
            let top_frac = lines_above as f32 / total_lines as f32;
            let thumb_y = rect.min.y + rect.height() * top_frac;
            let bar_rect = egui::Rect::from_min_size(
                egui::pos2(rect.max.x - bar_w, thumb_y),
                egui::vec2(bar_w, thumb_h),
            );
            painter.rect_filled(bar_rect, 1.0, theme::active().scrollbar_color);
        }

        ui.allocate_rect(rect, Sense::click_and_drag());

        TerminalGeometry {
            rect,
            cell_w: cell_width,
            cell_h: cell_height,
        }
    }
}

fn resolve_color(color: Color, is_fg: bool) -> egui::Color32 {
    match color {
        Color::Named(named) => resolve_named(named, is_fg),
        Color::Spec(Rgb { r, g, b }) => egui::Color32::from_rgb(r, g, b),
        Color::Indexed(i) => {
            let (r, g, b) = ansi_indexed(i);
            egui::Color32::from_rgb(r, g, b)
        }
    }
}

fn resolve_named(named: NamedColor, is_fg: bool) -> egui::Color32 {
    let t = theme::active();
    match named {
        NamedColor::Foreground => t.text,
        NamedColor::Background => egui::Color32::TRANSPARENT,
        NamedColor::Black => t.ansi[0],
        NamedColor::Red => t.ansi[1],
        NamedColor::Green => t.ansi[2],
        NamedColor::Yellow => t.ansi[3],
        NamedColor::Blue => t.ansi[4],
        NamedColor::Magenta => t.ansi[5],
        NamedColor::Cyan => t.ansi[6],
        NamedColor::White => t.ansi[7],
        NamedColor::BrightBlack => t.ansi[8],
        NamedColor::BrightRed => t.ansi[9],
        NamedColor::BrightGreen => t.ansi[10],
        NamedColor::BrightYellow => t.ansi[11],
        NamedColor::BrightBlue => t.ansi[12],
        NamedColor::BrightMagenta => t.ansi[13],
        NamedColor::BrightCyan => t.ansi[14],
        NamedColor::BrightWhite => t.ansi[15],
        _ => {
            if is_fg {
                t.text
            } else {
                egui::Color32::TRANSPARENT
            }
        }
    }
}

fn ansi_indexed(index: u8) -> (u8, u8, u8) {
    match index {
        0..=15 => {
            let c = theme::active().ansi[index as usize];
            let [r, g, b, _] = c.to_array();
            (r, g, b)
        }
        16..=231 => {
            let n = index - 16;
            let b = (n % 6) * 51;
            let g = ((n / 6) % 6) * 51;
            let r = (n / 36) * 51;
            (r, g, b)
        }
        232..=255 => {
            let v = 8 + (index - 232) * 10;
            (v, v, v)
        }
    }
}
