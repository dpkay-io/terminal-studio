use crate::theme;

// ── Drag divider ───────────────────────────────────────────────────────────

pub fn drag_divider(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: egui::Rect,
    idle_color: egui::Color32,
    active_color: egui::Color32,
) -> f32 {
    let resp = ui.interact(rect, id, egui::Sense::drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let is_active = resp.hovered() || resp.dragged();
    let anim_t = ui
        .ctx()
        .animate_bool_with_time(id.with("div_anim"), is_active, theme::ANIM_FAST);
    let color = theme::lerp_color(idle_color, active_color, anim_t);

    let line_w = rect.width() * 0.6;
    let line_x = rect.center().x - line_w * 0.5;
    let line_y = rect.center().y;
    ui.painter().line_segment(
        [egui::pos2(line_x, line_y), egui::pos2(line_x + line_w, line_y)],
        egui::Stroke::new(theme::STROKE_THIN, color),
    );
    resp.drag_delta().y
}

// ── Active bar ─────────────────────────────────────────────────────────────

pub fn active_bar(painter: &egui::Painter, rect: egui::Rect, rounding: egui::Rounding) {
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(theme::CARD_BAR_W, rect.height()));
    painter.rect_filled(bar, rounding, theme::active().green);
}

/// Draws a horizontal gradient separator: transparent → color → transparent.
pub fn gradient_separator(painter: &egui::Painter, rect: egui::Rect) {
    let color = theme::active().border_subtle;
    let mid_x = rect.center().x;
    let w = rect.width();
    let steps = 8;
    let step_w = w / (steps as f32 * 2.0);

    for i in 0..steps {
        let t = (i + 1) as f32 / steps as f32;
        let alpha = (color.a() as f32 * t).min(255.0) as u8;
        let c = egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha);

        // Left half (fade in)
        let x0 = rect.min.x + (i as f32) * step_w;
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(x0, rect.min.y),
                egui::vec2(step_w, rect.height()),
            ),
            0.0,
            c,
        );

        // Right half (fade out)
        let x1 = mid_x + (mid_x - x0 - step_w);
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(x1, rect.min.y),
                egui::vec2(step_w, rect.height()),
            ),
            0.0,
            c,
        );
    }
}
