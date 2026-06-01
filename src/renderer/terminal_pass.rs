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
use crate::theme::{self, Theme};

pub struct SelectionRange {
    pub start_col: u16,
    pub start_row: u16,
    pub end_col: u16,
    pub end_row: u16,
    /// `display_offset` at the time the selection was created. Used to adjust
    /// the highlight if the terminal scrolls while the selection is active.
    pub display_offset: usize,
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
    /// When the user drags the terminal scrollbar, this is set to the target
    /// `display_offset` that the caller should apply to the terminal.
    pub scrollbar_drag_offset: Option<usize>,
    /// True while the pointer hovers over the scrollbar hit region.
    pub scrollbar_hovered: bool,
    /// Session ID this geometry belongs to — used to route mouse/scroll events
    /// to the correct terminal in split pane views.
    pub session_id: Option<u32>,
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
    underline: bool,
    strike: bool,
    wide: bool,
}

impl CellInfo {
    const EMPTY: CellInfo = CellInfo {
        ch: ' ',
        fg: egui::Color32::TRANSPARENT,
        bg: egui::Color32::TRANSPARENT,
        bold: false,
        underline: false,
        strike: false,
        wide: false,
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
        let t = theme::active();
        painter.rect_filled(rect, 0.0, t.bg_term);

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

                    let resolved_bg = resolve_color(eff_bg, false, t);
                    let visible_bg = if resolved_bg == egui::Color32::TRANSPARENT {
                        t.bg_term
                    } else {
                        resolved_bg
                    };

                    let mut fg = resolve_color(eff_fg, true, t);
                    if cell.flags.contains(Flags::BOLD) {
                        fg = match eff_fg {
                            Color::Named(NamedColor::Black) => t.ansi[8],
                            Color::Named(NamedColor::Red) => t.ansi[9],
                            Color::Named(NamedColor::Green) => t.ansi[10],
                            Color::Named(NamedColor::Yellow) => t.ansi[11],
                            Color::Named(NamedColor::Blue) => t.ansi[12],
                            Color::Named(NamedColor::Magenta) => t.ansi[13],
                            Color::Named(NamedColor::Cyan) => t.ansi[14],
                            Color::Named(NamedColor::White) => t.ansi[15],
                            _ => fg,
                        };
                    }
                    if cell.flags.contains(Flags::DIM) {
                        let [fr, fgc, fb, _] = fg.to_array();
                        let [br, bgc, bb, _] = visible_bg.to_array();
                        fg = egui::Color32::from_rgb(
                            ((fr as u16 + br as u16) / 2) as u8,
                            ((fgc as u16 + bgc as u16) / 2) as u8,
                            ((fb as u16 + bb as u16) / 2) as u8,
                        );
                    }
                    fg = theme::ensure_term_contrast(fg, visible_bg);

                    buf.push(CellInfo {
                        ch,
                        fg,
                        bg: resolved_bg,
                        bold: cell.flags.contains(Flags::BOLD),
                        underline: cell.flags.contains(Flags::UNDERLINE),
                        strike: cell.flags.contains(Flags::STRIKEOUT),
                        wide: cell.flags.contains(Flags::WIDE_CHAR),
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
        //
        // Each character is rendered at its exact grid position
        // (col * cell_width) to prevent cumulative glyph-advance drift
        // that occurs when rendering multi-character strings.
        let base_font = FontId::monospace(font_size);

        RENDER_BUF.with(|buf| {
            let buf = buf.borrow();
            for screen_row in 0..visible_rows {
                let y = rect.min.y + screen_row as f32 * cell_height;

                let row_off = screen_row * cols;
                if row_off >= buf.len() {
                    break;
                }

                // ── Background runs ────────────────────────────────────────────
                let mut bg_run_start = 0usize;
                let mut bg_run_color = egui::Color32::TRANSPARENT;

                for col in 0..=cols {
                    let new_bg = if col < cols {
                        buf[row_off + col].bg
                    } else {
                        egui::Color32::TRANSPARENT
                    };
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
                }

                // ── Per-cell character rendering ───────────────────────────────
                for col in 0..cols {
                    let c = buf[row_off + col];
                    if c.ch == ' ' {
                        continue;
                    }
                    let x = rect.min.x + col as f32 * cell_width;
                    let pos = egui::pos2(x, y);
                    if c.wide {
                        let wide_rect = egui::Rect::from_min_size(
                            pos,
                            egui::vec2(2.0 * cell_width, cell_height),
                        );
                        let galley =
                            painter.layout_no_wrap(c.ch.to_string(), base_font.clone(), c.fg);
                        painter.galley(
                            egui::pos2(
                                wide_rect.min.x + (wide_rect.width() - galley.size().x) * 0.5,
                                wide_rect.min.y,
                            ),
                            galley,
                            egui::Color32::TRANSPARENT,
                        );
                    } else {
                        painter.text(pos, egui::Align2::LEFT_TOP, c.ch, base_font.clone(), c.fg);
                        if c.bold {
                            painter.text(
                                egui::pos2(x + 1.0, y),
                                egui::Align2::LEFT_TOP,
                                c.ch,
                                base_font.clone(),
                                c.fg,
                            );
                        }
                    }
                }

                // ── Underline runs ─────────────────────────────────────────────
                let mut ul_start: Option<(usize, egui::Color32)> = None;
                for col in 0..=cols {
                    let (is_ul, fg) = if col < cols {
                        let c = buf[row_off + col];
                        (c.underline, c.fg)
                    } else {
                        (false, egui::Color32::TRANSPARENT)
                    };
                    match ul_start {
                        Some((start, color)) if !is_ul || fg != color => {
                            let uy = y + cell_height - 1.5;
                            painter.line_segment(
                                [
                                    egui::pos2(rect.min.x + start as f32 * cell_width, uy),
                                    egui::pos2(rect.min.x + col as f32 * cell_width, uy),
                                ],
                                egui::Stroke::new(1.0, color),
                            );
                            ul_start = if is_ul { Some((col, fg)) } else { None };
                        }
                        None if is_ul => {
                            ul_start = Some((col, fg));
                        }
                        _ => {}
                    }
                }

                // ── Strikethrough runs ─────────────────────────────────────────
                let mut st_start: Option<(usize, egui::Color32)> = None;
                for col in 0..=cols {
                    let (is_st, fg) = if col < cols {
                        let c = buf[row_off + col];
                        (c.strike, c.fg)
                    } else {
                        (false, egui::Color32::TRANSPARENT)
                    };
                    match st_start {
                        Some((start, color)) if !is_st || fg != color => {
                            let sy = y + cell_height * 0.5;
                            painter.line_segment(
                                [
                                    egui::pos2(rect.min.x + start as f32 * cell_width, sy),
                                    egui::pos2(rect.min.x + col as f32 * cell_width, sy),
                                ],
                                egui::Stroke::new(1.0, color),
                            );
                            st_start = if is_st { Some((col, fg)) } else { None };
                        }
                        None if is_st => {
                            st_start = Some((col, fg));
                        }
                        _ => {}
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
                let cursor_origin =
                    rect.min + vec2(cx as f32 * cell_width, cy as f32 * cell_height);
                if is_focused {
                    match cursor_style {
                        CursorStyle::Block => {
                            let cursor_rect =
                                Rect::from_min_size(cursor_origin, vec2(cell_width, cell_height));
                            painter.rect_filled(cursor_rect, 0.0, t.cursor_color);
                        }
                        CursorStyle::Underline => {
                            let cursor_rect = Rect::from_min_size(
                                egui::pos2(cursor_origin.x, cursor_origin.y + cell_height - 2.0),
                                vec2(cell_width, 2.0),
                            );
                            painter.rect_filled(cursor_rect, 0.0, t.cursor_color);
                        }
                        CursorStyle::Beam => {
                            let cursor_rect =
                                Rect::from_min_size(cursor_origin, vec2(2.0, cell_height));
                            painter.rect_filled(cursor_rect, 0.0, t.cursor_color);
                        }
                    }
                } else {
                    let cursor_rect =
                        Rect::from_min_size(cursor_origin, vec2(cell_width, cell_height));
                    painter.rect_stroke(
                        cursor_rect,
                        0.0,
                        egui::Stroke::new(1.5, t.cursor_dim_color),
                    );
                }
            }
        }

        // ── Selection highlight ────────────────────────────────────────────────
        if let Some(sel) = selection.filter(|_| visible_rows > 0) {
            let sel_color = t.selection_bg;
            let (sc, sr_raw, ec, er_raw) = sel.ordered();
            // Adjust selection rows when display_offset changed since selection start
            let offset_delta = display_offset as i32 - sel.display_offset as i32;
            let sr = (sr_raw as i32 + offset_delta).max(0) as u16;
            let er = (er_raw as i32 + offset_delta).max(0) as u16;
            let (sc, ec) = RENDER_BUF.with(|buf| {
                let buf = buf.borrow();
                let snap_start = |col: u16, row: u16| -> u16 {
                    if col > 0 && (row as usize) < visible_rows {
                        let prev_idx = row as usize * cols + (col as usize - 1);
                        if prev_idx < buf.len() && buf[prev_idx].wide {
                            return col - 1;
                        }
                    }
                    col
                };
                let snap_end = |col: u16, row: u16| -> u16 {
                    if (row as usize) < visible_rows {
                        let idx = row as usize * cols + col as usize;
                        if idx < buf.len() && buf[idx].wide {
                            return col + 1;
                        }
                    }
                    col
                };
                (snap_start(sc, sr), snap_end(ec, er))
            });
            for screen_row in sr..=er.min(visible_rows.saturating_sub(1) as u16) {
                let y = rect.min.y + screen_row as f32 * cell_height;
                let start_col = if screen_row == sr { sc } else { 0 };
                let end_col = if screen_row == er {
                    (ec + 1).min(cols as u16)
                } else {
                    cols as u16
                };
                let x0 = rect.min.x + start_col as f32 * cell_width;
                let x1 = rect.min.x + end_col as f32 * cell_width;
                painter.rect_filled(
                    Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + cell_height)),
                    0.0,
                    sel_color,
                );
            }
        }

        // ── Interactive scrollbar ───────────────────────────────────────────────
        let mut scrollbar_drag_offset = None;
        let mut scrollbar_hovered = false;

        if history > 0 {
            let total_lines = history + visible_rows;

            let bar_w_thin = theme::SCROLLBAR_W_IDLE;
            let bar_w_wide = theme::SCROLLBAR_W_ACTIVE;
            let hit_w = theme::SCROLLBAR_HIT_W;

            let hit_rect =
                egui::Rect::from_min_max(egui::pos2(rect.max.x - hit_w, rect.min.y), rect.max);

            let (pointer_pos, primary_down, any_down) = ui.input(|i| {
                (
                    i.pointer.latest_pos(),
                    i.pointer.primary_down(),
                    i.pointer.any_down(),
                )
            });

            let pointer_in_hit = pointer_pos.map(|p| hit_rect.contains(p)).unwrap_or(false);

            let sb_mem_id = ui.id().with("term_sb_dragging");
            let was_dragging = ui.data_mut(|d| *d.get_temp_mut_or_default::<bool>(sb_mem_id));
            let is_dragging = (was_dragging || (pointer_in_hit && primary_down)) && any_down;
            ui.data_mut(|d| d.insert_temp(sb_mem_id, is_dragging));

            scrollbar_hovered = pointer_in_hit || is_dragging;

            let bar_w = if scrollbar_hovered {
                bar_w_wide
            } else {
                bar_w_thin
            };
            let thumb_frac = (visible_rows as f32 / total_lines as f32).min(1.0);
            let thumb_h = (rect.height() * thumb_frac).max(20.0).min(rect.height());
            let track_h = (rect.height() - thumb_h).max(0.0);

            let lines_above = history.saturating_sub(display_offset);
            let top_frac = lines_above as f32 / (total_lines - visible_rows).max(1) as f32;
            let thumb_y = rect.min.y + track_h * top_frac;

            if is_dragging {
                if let Some(pos) = pointer_pos {
                    let click_y = (pos.y - rect.min.y - thumb_h * 0.5).clamp(0.0, track_h);
                    let frac = if track_h > 0.0 {
                        click_y / track_h
                    } else {
                        0.0
                    };
                    let target_lines_above =
                        (frac * (total_lines - visible_rows) as f32).round() as usize;
                    let target_offset = history.saturating_sub(target_lines_above);
                    scrollbar_drag_offset = Some(target_offset);
                }
            }

            let bar_color = if is_dragging {
                egui::Color32::from_rgba_unmultiplied(
                    t.scrollbar_color.r(),
                    t.scrollbar_color.g(),
                    t.scrollbar_color.b(),
                    theme::ALPHA_SCROLLBAR_DRAG,
                )
            } else if pointer_in_hit {
                egui::Color32::from_rgba_unmultiplied(
                    t.scrollbar_color.r(),
                    t.scrollbar_color.g(),
                    t.scrollbar_color.b(),
                    theme::ALPHA_SCROLLBAR_HOVER,
                )
            } else {
                egui::Color32::from_rgba_unmultiplied(
                    t.scrollbar_color.r(),
                    t.scrollbar_color.g(),
                    t.scrollbar_color.b(),
                    theme::ALPHA_SCROLLBAR_IDLE,
                )
            };

            let bar_rect = egui::Rect::from_min_size(
                egui::pos2(rect.max.x - bar_w, thumb_y),
                egui::vec2(bar_w, thumb_h),
            );
            painter.rect_filled(bar_rect, bar_w * 0.5, bar_color);
        }

        ui.allocate_rect(rect, Sense::click_and_drag());

        TerminalGeometry {
            rect,
            cell_w: cell_width,
            cell_h: cell_height,
            scrollbar_drag_offset,
            scrollbar_hovered,
            session_id: None,
        }
    }
}

fn resolve_color(color: Color, is_fg: bool, t: &Theme) -> egui::Color32 {
    match color {
        Color::Named(named) => resolve_named(named, is_fg, t),
        Color::Spec(Rgb { r, g, b }) => egui::Color32::from_rgb(r, g, b),
        Color::Indexed(i) => {
            let (r, g, b) = ansi_indexed(i, t);
            egui::Color32::from_rgb(r, g, b)
        }
    }
}

fn resolve_named(named: NamedColor, is_fg: bool, t: &Theme) -> egui::Color32 {
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

fn ansi_indexed(index: u8, t: &Theme) -> (u8, u8, u8) {
    match index {
        0..=15 => {
            let c = t.ansi[index as usize];
            let [r, g, b, _] = c.to_array();
            (r, g, b)
        }
        16..=231 => {
            const CUBE_STEPS: [u8; 6] = [0, 95, 135, 175, 215, 255];
            let n = (index - 16) as usize;
            let r = CUBE_STEPS[n / 36];
            let g = CUBE_STEPS[(n / 6) % 6];
            let b = CUBE_STEPS[n % 6];
            (r, g, b)
        }
        232..=255 => {
            let v = 8 + (index - 232) * 10;
            (v, v, v)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2, Rect};

    // ── SelectionRange::ordered ────────────────────────────────────────────

    #[test]
    fn test_selection_ordered_already_ordered() {
        let sel = SelectionRange {
            start_col: 2,
            start_row: 1,
            end_col: 10,
            end_row: 5,
            display_offset: 0,
        };
        assert_eq!(sel.ordered(), (2, 1, 10, 5));
    }

    #[test]
    fn test_selection_ordered_reversed() {
        let sel = SelectionRange {
            start_col: 10,
            start_row: 5,
            end_col: 2,
            end_row: 1,
            display_offset: 0,
        };
        assert_eq!(sel.ordered(), (2, 1, 10, 5));
    }

    #[test]
    fn test_selection_ordered_same_row_reversed() {
        let sel = SelectionRange {
            start_col: 15,
            start_row: 3,
            end_col: 5,
            end_row: 3,
            display_offset: 0,
        };
        assert_eq!(sel.ordered(), (5, 3, 15, 3));
    }

    #[test]
    fn test_selection_ordered_same_position() {
        let sel = SelectionRange {
            start_col: 7,
            start_row: 4,
            end_col: 7,
            end_row: 4,
            display_offset: 0,
        };
        assert_eq!(sel.ordered(), (7, 4, 7, 4));
    }

    // ── TerminalGeometry::to_cell ──────────────────────────────────────────

    fn make_geo() -> TerminalGeometry {
        TerminalGeometry {
            rect: Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 480.0)),
            cell_w: 10.0,
            cell_h: 20.0,
            scrollbar_drag_offset: None,
            scrollbar_hovered: false,
            session_id: None,
        }
    }

    #[test]
    fn test_to_cell_center() {
        let geo = make_geo();
        // Point at (150, 90) → col = (150-100)/10 = 5, row = (90-50)/20 = 2
        let result = geo.to_cell(pos2(150.0, 90.0));
        assert_eq!(result, Some((5, 2)));
    }

    #[test]
    fn test_to_cell_top_left_corner() {
        let geo = make_geo();
        // Exact top-left of the rect → col 0, row 0
        let result = geo.to_cell(pos2(100.0, 50.0));
        assert_eq!(result, Some((0, 0)));
    }

    #[test]
    fn test_to_cell_outside_returns_none() {
        let geo = make_geo();
        // Point before the rect
        assert_eq!(geo.to_cell(pos2(99.0, 50.0)), None);
        // Point below the rect
        assert_eq!(geo.to_cell(pos2(100.0, 531.0)), None);
        // Point to the right of the rect
        assert_eq!(geo.to_cell(pos2(901.0, 50.0)), None);
    }

    // ── ansi_indexed ───────────────────────────────────────────────────────

    #[test]
    fn test_ansi_indexed_standard_colors() {
        let t = theme::active();
        for i in 0..=15u8 {
            let (r, g, b) = ansi_indexed(i, t);
            let expected = t.ansi[i as usize];
            let [er, eg, eb, _] = expected.to_array();
            assert_eq!((r, g, b), (er, eg, eb), "mismatch at ANSI index {i}");
        }
    }

    #[test]
    fn test_ansi_indexed_216_cube() {
        let t = theme::active();
        // Index 16 → first cube entry → (0,0,0)
        assert_eq!(ansi_indexed(16, t), (0, 0, 0));
        // Index 231 → last cube entry → (255,255,255)
        assert_eq!(ansi_indexed(231, t), (255, 255, 255));
        // Index 52 → n=36 → r=CUBE_STEPS[1]=95, g=0, b=0
        assert_eq!(ansi_indexed(52, t), (95, 0, 0));
        // Index 21 → n=5 → r=0, g=0, b=CUBE_STEPS[5]=255
        assert_eq!(ansi_indexed(21, t), (0, 0, 255));
        // Index 196 → n=180 → r=CUBE_STEPS[5]=255, g=0, b=0
        assert_eq!(ansi_indexed(196, t), (255, 0, 0));
    }

    #[test]
    fn test_ansi_indexed_grayscale() {
        let t = theme::active();
        // Index 232 → v = 8 + 0*10 = 8
        assert_eq!(ansi_indexed(232, t), (8, 8, 8));
        // Index 255 → v = 8 + 23*10 = 238
        assert_eq!(ansi_indexed(255, t), (238, 238, 238));
        // Index 243 → v = 8 + 11*10 = 118
        assert_eq!(ansi_indexed(243, t), (118, 118, 118));
    }
}
