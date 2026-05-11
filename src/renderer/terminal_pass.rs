use std::sync::Arc;

use egui::{vec2, FontId, Pos2, Rect, Sense};
use parking_lot::RwLock;

use crate::terminal::grid::{ansi_color, Cell, Color};
use crate::terminal::Session;
use crate::theme;

pub struct TerminalGeometry {
    pub rect: Rect,
    pub cell_w: f32,
    pub cell_h: f32,
}

impl TerminalGeometry {
    /// Convert a screen position to zero-based (col, row) terminal cell coords.
    /// Returns None if the position is outside the terminal rect.
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

    /// Render the terminal into the available UI rect.
    ///
    /// `scroll_offset` is the number of scrollback lines to show above the
    /// live grid (0 = live view, >0 = scrolled back). The cursor is hidden
    /// and mouse-coordinate mapping still uses the full live rect.
    pub fn show(
        &self,
        ui: &mut egui::Ui,
        is_focused: bool,
        scroll_offset: usize,
        cursor_visible: bool,
    ) -> TerminalGeometry {
        let rect = ui.available_rect_before_wrap();

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::BASE);

        let session = self.session.read();

        let font_id = FontId::monospace(14.0);
        let cell_height = ui.fonts(|f| f.row_height(&font_id));
        // glyph_width reads directly from font metrics — no galley allocation
        let cell_width = ui.fonts(|f| f.glyph_width(&font_id, 'M'));

        let visible_cols = (rect.width() / cell_width) as u16;
        let visible_rows = (rect.height() / cell_height) as u16;
        let grid_cols = session.grid.cols.min(visible_cols);

        // ── Scrollback setup ─────────────────────────────────────────────────
        let scrollback_len = session.grid.scrollback.len();
        let show_sb = scroll_offset.min(scrollback_len).min(visible_rows as usize);
        let sb_start = scrollback_len.saturating_sub(show_sb);

        // ── Cell rendering ───────────────────────────────────────────────────
        // Batch adjacent cells with the same background color into one rect_filled
        // and adjacent cells with the same foreground style into one painter.text.
        // This reduces draw calls from O(cols*rows) to O(style_changes*rows).
        let mut text_buf = String::with_capacity(grid_cols as usize);

        for screen_row in 0..(visible_rows as usize) {
            let y = rect.min.y + screen_row as f32 * cell_height;

            // Closure to fetch a cell from scrollback or live grid
            let get_cell = |col: u16| -> Cell {
                if screen_row < show_sb {
                    let sb_idx = sb_start + screen_row;
                    session
                        .grid
                        .scrollback
                        .get(sb_idx)
                        .and_then(|r| r.get(col as usize))
                        .copied()
                        .unwrap_or_default()
                } else {
                    let grid_row = (screen_row - show_sb) as u16;
                    if grid_row < session.grid.rows {
                        *session.grid.get(grid_row, col)
                    } else {
                        Cell::default()
                    }
                }
            };

            // ── Single merged pass: BG and FG in one cell read per column ────
            // BG run state
            let mut bg_run_start = 0u16;
            let mut bg_run_color = egui::Color32::TRANSPARENT;

            // FG span state
            let mut span_start = 0u16;
            let mut span_fg = egui::Color32::TRANSPARENT;
            let mut span_underline = false;
            let mut span_strike = false;
            let mut span_bold = false;
            let mut span_italic = false;

            for col in 0..=grid_cols {
                let is_end = col == grid_cols;

                // Read cell once; derive all render attributes from it.
                let (new_bg, cell_fg, cell_underline, cell_strike, cell_bold, cell_italic, cell_char) = if !is_end {
                    let cell = get_cell(col);
                    let inv = cell.attrs.inverse();
                    let eff_bg = if inv { cell.fg } else { cell.bg };
                    let eff_fg = if inv { cell.bg } else { cell.fg };
                    let mut fg = resolve_color(eff_fg, true);
                    if cell.attrs.dim() {
                        let [r, g, b, a] = fg.to_array();
                        fg = egui::Color32::from_rgba_unmultiplied(r, g, b, a / 2);
                    }
                    let ch = if cell.attrs.invisible() { ' ' } else { cell.c };
                    (
                        resolve_color(eff_bg, false),
                        fg,
                        cell.attrs.underline(),
                        cell.attrs.strikethrough(),
                        cell.attrs.bold(),
                        cell.attrs.italic(),
                        ch,
                    )
                } else {
                    (egui::Color32::TRANSPARENT, egui::Color32::TRANSPARENT, false, false, false, false, ' ')
                };

                // ── BG flush ────────────────────────────────────────────────────
                if new_bg != bg_run_color {
                    if bg_run_color != egui::Color32::TRANSPARENT {
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
                    || cell_underline != span_underline
                    || cell_strike != span_strike
                    || cell_bold != span_bold
                    || cell_italic != span_italic;

                if fg_changed {
                    if !text_buf.is_empty() {
                        let visible = text_buf.trim_end_matches(' ');
                        if !visible.is_empty() {
                            // Bold uses a slight font-size bump (0.5 pt) as a visual-weight
                            // proxy until a real bold font face is loaded. Italic reuses the
                            // regular monospace face (no italic variant is bundled).
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
                    span_underline = cell_underline;
                    span_strike = cell_strike;
                    span_bold = cell_bold;
                    span_italic = cell_italic;
                }

                if !is_end {
                    text_buf.push(cell_char);
                }
            }
        }

        // Draw cursor only when in live view (not scrolled back).
        // When focused, hide cursor during the "off" phase of the blink cycle.
        // When unfocused, always show the cursor (outline style).
        if scroll_offset == 0 && session.cursor_visible && (cursor_visible || !is_focused) {
            let cx = session.cursor_x;
            let cy = session.cursor_y;
            if cx < session.grid.cols && cy < session.grid.rows {
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

        // Scrollback indicator: a proper thumb showing position within total content.
        if scrollback_len > 0 {
            let bar_w = 3.0_f32;
            let total_lines = scrollback_len + visible_rows as usize;
            // Thumb height = fraction of content that is visible
            let thumb_frac = (visible_rows as f32 / total_lines as f32).min(1.0);
            let thumb_h = (rect.height() * thumb_frac).max(4.0);
            // Thumb top = fraction of content above the viewport
            // When scroll_offset == 0 (live view) the content above is all of scrollback.
            // When scroll_offset == scrollback_len the content above is 0.
            let lines_above = scrollback_len.saturating_sub(scroll_offset);
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
        Color::Default => {
            if is_fg {
                theme::TEXT
            } else {
                egui::Color32::TRANSPARENT
            }
        }
        Color::Indexed(i) => {
            let (r, g, b) = ansi_color(i);
            egui::Color32::from_rgb(r, g, b)
        }
        Color::Rgb(r, g, b) => egui::Color32::from_rgb(r, g, b),
    }
}
