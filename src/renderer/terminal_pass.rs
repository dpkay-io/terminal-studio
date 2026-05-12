use std::sync::Arc;

use alacritty_terminal::{
    grid::Dimensions,
    index::{Column, Line},
    term::{cell::Flags, TermMode},
    vte::ansi::{Color, NamedColor, Rgb},
};
use egui::{vec2, FontId, Pos2, Rect, Sense};
use parking_lot::RwLock;

use crate::terminal::Session;
use crate::theme;

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
    ) -> TerminalGeometry {
        let rect = ui.available_rect_before_wrap();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::BASE);

        let session = self.session.read();
        let font_id = FontId::monospace(14.0);
        let cell_height = ui.fonts(|f| f.row_height(&font_id));
        let cell_width = ui.fonts(|f| f.glyph_width(&font_id, 'M'));

        let visible_rows = (rect.height() / cell_height) as usize;
        let visible_cols = (rect.width() / cell_width) as usize;

        let term = &session.term;
        let grid = term.grid();
        let term_cols = term.columns();
        let term_rows = term.screen_lines();
        let cols = term_cols.min(visible_cols);
        let display_offset = grid.display_offset();
        let history = grid.history_size();

        let mut text_buf = String::with_capacity(cols + 1);

        for screen_row in 0..visible_rows {
            let y = rect.min.y + screen_row as f32 * cell_height;

            // Map screen row → alacritty grid line index (i32).
            // display_offset lines of scrollback are shown above the viewport:
            //   screen_row=0 → grid line (0 - display_offset) = -display_offset (scrollback)
            //   screen_row=display_offset → grid line 0 (top of viewport)
            let grid_line = screen_row as i32 - display_offset as i32;

            // Skip rows that are outside both scrollback and visible buffer
            let min_line = -(history as i32);
            let max_line = term_rows as i32 - 1;
            if grid_line < min_line || grid_line > max_line {
                continue;
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
                        (
                            resolve_color(eff_bg, false),
                            fg,
                            cell.flags.contains(Flags::UNDERLINE),
                            cell.flags.contains(Flags::STRIKEOUT),
                            cell.flags.contains(Flags::BOLD),
                            cell.flags.contains(Flags::ITALIC),
                            ch,
                        )
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
                                egui::pos2(rect.min.x + col as f32 * cell_width, y + cell_height),
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
                                FontId::monospace(14.5)
                            } else {
                                FontId::monospace(14.0)
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

        // ── Cursor ─────────────────────────────────────────────────────────────
        // Only draw cursor in live view (display_offset == 0) and when cursor
        // should be visible per hardware (SHOW_CURSOR mode) and blink phase.
        if display_offset == 0
            && term.mode().contains(TermMode::SHOW_CURSOR)
            && (cursor_visible || !is_focused)
        {
            let cursor_pt = grid.cursor.point;
            let cx = cursor_pt.column.0;
            let cy = cursor_pt.line.0; // should be >= 0 for live cursor
            if cy >= 0 && cx < cols && (cy as usize) < term_rows {
                let cursor_rect = Rect::from_min_size(
                    rect.min + vec2(cx as f32 * cell_width, cy as f32 * cell_height),
                    vec2(cell_width, cell_height),
                );
                if is_focused {
                    painter.rect_filled(
                        cursor_rect,
                        0.0,
                        egui::Color32::from_rgba_premultiplied(255, 255, 255, 200),
                    );
                } else {
                    painter.rect_stroke(
                        cursor_rect,
                        0.0,
                        egui::Stroke::new(
                            1.5,
                            egui::Color32::from_rgba_premultiplied(255, 255, 255, 160),
                        ),
                    );
                }
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
            painter.rect_filled(
                bar_rect,
                1.0,
                egui::Color32::from_rgba_unmultiplied(180, 180, 180, 150),
            );
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
    match named {
        NamedColor::Foreground => theme::TEXT,
        NamedColor::Background => egui::Color32::TRANSPARENT,
        NamedColor::Black => egui::Color32::from_rgb(30, 30, 46),
        NamedColor::Red => egui::Color32::from_rgb(243, 139, 168),
        NamedColor::Green => egui::Color32::from_rgb(166, 227, 161),
        NamedColor::Yellow => egui::Color32::from_rgb(249, 226, 175),
        NamedColor::Blue => egui::Color32::from_rgb(137, 180, 250),
        NamedColor::Magenta => egui::Color32::from_rgb(245, 194, 231),
        NamedColor::Cyan => egui::Color32::from_rgb(148, 226, 213),
        NamedColor::White => egui::Color32::from_rgb(205, 214, 244),
        NamedColor::BrightBlack => egui::Color32::from_rgb(88, 91, 112),
        NamedColor::BrightRed => egui::Color32::from_rgb(243, 139, 168),
        NamedColor::BrightGreen => egui::Color32::from_rgb(166, 227, 161),
        NamedColor::BrightYellow => egui::Color32::from_rgb(249, 226, 175),
        NamedColor::BrightBlue => egui::Color32::from_rgb(137, 180, 250),
        NamedColor::BrightMagenta => egui::Color32::from_rgb(245, 194, 231),
        NamedColor::BrightCyan => egui::Color32::from_rgb(148, 226, 213),
        NamedColor::BrightWhite => egui::Color32::from_rgb(255, 255, 255),
        _ => {
            if is_fg {
                theme::TEXT
            } else {
                egui::Color32::TRANSPARENT
            }
        }
    }
}

fn ansi_indexed(index: u8) -> (u8, u8, u8) {
    match index {
        0 => (30, 30, 46),
        1 => (243, 139, 168),
        2 => (166, 227, 161),
        3 => (249, 226, 175),
        4 => (137, 180, 250),
        5 => (245, 194, 231),
        6 => (148, 226, 213),
        7 => (205, 214, 244),
        8 => (88, 91, 112),
        9 => (243, 139, 168),
        10 => (166, 227, 161),
        11 => (249, 226, 175),
        12 => (137, 180, 250),
        13 => (245, 194, 231),
        14 => (148, 226, 213),
        15 => (255, 255, 255),
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
