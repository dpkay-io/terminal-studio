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

    let item_id = egui::Id::new(("list_item", rect.min.x as i32, rect.min.y as i32));
    let sel_t = ui
        .ctx()
        .animate_bool_with_time(item_id.with("sel"), selected, theme::ANIM_FAST);
    let hover_t =
        ui.ctx()
            .animate_bool_with_time(item_id.with("hover"), resp.hovered(), theme::ANIM_FAST);

    let bg = if sel_t > 0.01 {
        theme::lerp_color(egui::Color32::TRANSPARENT, t.surface1, sel_t)
    } else {
        theme::lerp_color(egui::Color32::TRANSPARENT, t.surface0, hover_t)
    };
    ui.painter().rect_filled(rect, theme::R_MD, bg);

    add_contents(ui.painter(), rect);
    resp
}
