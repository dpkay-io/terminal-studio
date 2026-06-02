use crate::theme;

// ── List item (command palette / quick switcher) ───────────────────────────

pub fn list_item(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    add_contents: impl FnOnce(&egui::Painter, egui::Rect),
) -> egui::Response {
    let t = theme::active();
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(width, theme::DIALOG_ITEM_H),
        egui::Sense::click(),
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let bg = if selected {
        t.surface1
    } else if resp.hovered() {
        t.surface0
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, theme::R_SM, bg);

    add_contents(ui.painter(), rect);
    resp
}
