use crate::theme::{self, Theme};

const THEME_CARD_HEIGHT: f32 = theme::BTN_H_ACTION + theme::FONT_MD + theme::SP_1 + theme::SP_4;
const PALETTE_HEIGHT: f32 = theme::BTN_H_ACTION;

pub(in crate::app::ui) fn grid_columns(_theme_count: usize) -> usize {
    4
}

pub(in crate::app::ui) fn theme_card(
    ui: &mut egui::Ui,
    theme_data: &Theme,
    selected: bool,
) -> egui::Response {
    let size = egui::vec2(ui.available_width(), THEME_CARD_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let roles = theme::active();

    let fill = if selected {
        roles.bg_raised
    } else if response.hovered() {
        roles.bg_panel
    } else {
        roles.bg_base
    };
    ui.painter().rect_filled(rect, theme::R_MD, fill);
    if selected {
        ui.painter().rect_stroke(
            rect,
            theme::R_MD,
            egui::Stroke::new(theme::STROKE_MEDIUM, roles.accent),
        );
    }

    let content_rect = rect.shrink(theme::SP_2);
    let palette_rect = egui::Rect::from_min_size(
        content_rect.min,
        egui::vec2(content_rect.width(), PALETTE_HEIGHT),
    );
    let segment_width = palette_rect.width() / 3.0;
    for (index, color) in [theme_data.bg_base, theme_data.accent, theme_data.success]
        .into_iter()
        .enumerate()
    {
        let segment_rect = egui::Rect::from_min_size(
            egui::pos2(
                palette_rect.min.x + index as f32 * segment_width,
                palette_rect.min.y,
            ),
            egui::vec2(segment_width, palette_rect.height()),
        );
        let rounding = match index {
            0 => egui::Rounding {
                nw: theme::R_SM,
                sw: theme::R_SM,
                ..Default::default()
            },
            2 => egui::Rounding {
                ne: theme::R_SM,
                se: theme::R_SM,
                ..Default::default()
            },
            _ => egui::Rounding::ZERO,
        };
        ui.painter().rect_filled(segment_rect, rounding, color);
    }

    ui.painter().text(
        egui::pos2(content_rect.min.x, palette_rect.max.y + theme::SP_1),
        egui::Align2::LEFT_TOP,
        theme_data.id.name(),
        egui::FontId::proportional(theme::FONT_MD),
        if selected { roles.fg_strong } else { roles.fg },
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}
