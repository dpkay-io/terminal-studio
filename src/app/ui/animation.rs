use crate::theme;

pub fn animated_hover(ctx: &egui::Context, id: egui::Id, hovered: bool) -> f32 {
    ctx.animate_bool_with_time(id.with("hover_anim"), hovered, theme::ANIM_FAST)
}

pub fn animated_bool(ctx: &egui::Context, id: egui::Id, active: bool, duration: f32) -> f32 {
    ctx.animate_bool_with_time(id.with("bool_anim"), active, duration)
}
