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
    let color = if resp.hovered() || resp.dragged() {
        active_color
    } else {
        idle_color
    };
    ui.painter().rect_filled(rect, theme::STROKE_THIN, color);
    resp.drag_delta().y
}

// ── Active bar ─────────────────────────────────────────────────────────────

pub fn active_bar(painter: &egui::Painter, rect: egui::Rect, rounding: egui::Rounding) {
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(theme::CARD_BAR_W, rect.height()));
    painter.rect_filled(bar, rounding, theme::active().green);
}
