use std::sync::Arc;

use egui::{vec2, FontId, Rect, Sense, Pos2};
use parking_lot::RwLock;

use crate::terminal::Session;
use crate::terminal::grid::{Cell, Color, ansi_color};
use crate::theme;

pub struct TerminalGeometry {
    pub rect:   Rect,
    pub cell_w: f32,
    pub cell_h: f32,
}

impl TerminalGeometry {
    /// Convert a screen position to zero-based (col, row) terminal cell coords.
    /// Returns None if the position is outside the terminal rect.
    pub fn to_cell(&self, pos: Pos2) -> Option<(u16, u16)> {
        if !self.rect.contains(pos) { return None; }
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
    pub fn show(&self, ui: &mut egui::Ui, is_focused: bool, scroll_offset: usize) -> TerminalGeometry {
        let rect = ui.available_rect_before_wrap();

        // Fill background
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::BASE);

        let session = self.session.read();

        let font_id = FontId::monospace(14.0);
        // Measure cell width and height using text metrics for precision
        let cell_height = ui.fonts(|f| f.row_height(&font_id));
        // Measure character width using a long string of identical chars to average out padding
        let cell_width = ui.fonts(|fonts| {
            let test_str = "MMMMMMMMMMMMMMMMMMMM"; // 20 identical wide chars
            let galley = fonts.layout_no_wrap(
                test_str.to_string(),
                font_id.clone(),
                theme::TEXT,
            );
            galley.rect.width() / test_str.len() as f32
        });

        let visible_cols = (rect.width() / cell_width) as u16;
        let visible_rows = (rect.height() / cell_height) as u16;

        let grid_cols = session.grid.cols.min(visible_cols);

        // ── Scrollback setup ─────────────────────────────────────────────────
        let scrollback_len = session.grid.scrollback.len();
        // How many scrollback rows to show at the top of the viewport.
        let show_sb = scroll_offset.min(scrollback_len).min(visible_rows as usize);
        // Index into scrollback where the displayed window starts
        // (scrollback[sb_start] is the oldest visible scrollback line).
        let sb_start = scrollback_len.saturating_sub(show_sb);

        // ── Cell rendering ───────────────────────────────────────────────────
        for screen_row in 0..(visible_rows as usize) {
            for col in 0..grid_cols {
                // Pick cell from scrollback or live grid
                let cell: Cell = if screen_row < show_sb {
                    let sb_idx = sb_start + screen_row;
                    session.grid.scrollback
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
                };

                let cell_rect = Rect::from_min_size(
                    rect.min + vec2(col as f32 * cell_width, screen_row as f32 * cell_height),
                    vec2(cell_width, cell_height),
                );

                // Determine effective fg/bg, respecting inverse attribute
                let (eff_fg, eff_bg) = if cell.attrs.inverse {
                    (cell.bg, cell.fg)
                } else {
                    (cell.fg, cell.bg)
                };

                // Paint background if not transparent
                let bg_color = resolve_color(eff_bg, false);
                if bg_color != egui::Color32::TRANSPARENT {
                    painter.rect_filled(cell_rect, 0.0, bg_color);
                }

                // Paint character if visible and not a space
                if !cell.attrs.invisible && cell.c != ' ' {
                    let mut fg_color = resolve_color(eff_fg, true);
                    if cell.attrs.dim {
                        let [r, g, b, a] = fg_color.to_array();
                        fg_color = egui::Color32::from_rgba_unmultiplied(r, g, b, a / 2);
                    }
                    painter.text(
                        egui::pos2(cell_rect.min.x, cell_rect.min.y),
                        egui::Align2::LEFT_TOP,
                        cell.c.to_string(),
                        font_id.clone(),
                        fg_color,
                    );
                    if cell.attrs.underline {
                        let y = cell_rect.max.y - 1.5;
                        painter.line_segment(
                            [egui::pos2(cell_rect.min.x, y), egui::pos2(cell_rect.max.x, y)],
                            egui::Stroke::new(1.0, fg_color),
                        );
                    }
                    if cell.attrs.strikethrough {
                        let y = cell_rect.center().y;
                        painter.line_segment(
                            [egui::pos2(cell_rect.min.x, y), egui::pos2(cell_rect.max.x, y)],
                            egui::Stroke::new(1.0, fg_color),
                        );
                    }
                }
            }
        }

        // Draw cursor only when in live view (not scrolled back).
        // We always render our own software cursor and ignore cursor_visible —
        // ?25l/?25h control the hardware console cursor, not our GUI block cursor.
        if scroll_offset == 0 {
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
                        egui::Stroke::new(1.5, egui::Color32::from_rgba_premultiplied(255, 255, 255, 160)),
                    );
                }
            }
        }

        // Draw a subtle scrollback indicator bar on the right edge when scrolled.
        if show_sb > 0 && scrollback_len > 0 {
            let bar_w = 3.0_f32;
            let frac = show_sb as f32 / scrollback_len as f32;
            // The indicator thumb covers the fraction of the right edge that
            // corresponds to how far back the user has scrolled.
            let bar_h = rect.height() * frac;
            let bar_rect = Rect::from_min_size(
                egui::pos2(rect.max.x - bar_w, rect.min.y),
                egui::vec2(bar_w, bar_h),
            );
            painter.rect_filled(bar_rect, 0.0, egui::Color32::from_rgba_unmultiplied(180, 180, 180, 120));
        }

        ui.allocate_rect(rect, Sense::click_and_drag());

        TerminalGeometry { rect, cell_w: cell_width, cell_h: cell_height }
    }
}

fn resolve_color(color: Color, is_fg: bool) -> egui::Color32 {
    match color {
        Color::Default => {
            if is_fg { theme::TEXT } else { egui::Color32::TRANSPARENT }
        }
        Color::Indexed(i) => {
            let (r, g, b) = ansi_color(i);
            egui::Color32::from_rgb(r, g, b)
        }
        Color::Rgb(r, g, b) => egui::Color32::from_rgb(r, g, b),
    }
}
