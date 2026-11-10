use crate::theme;

pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(theme::TOGGLE_W, theme::TOGGLE_H),
        egui::Sense::click(),
    );
    if response.clicked() {
        *on = !*on;
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let t = theme::active();
    ui.painter().rect_filled(
        rect,
        theme::TOGGLE_H * 0.5,
        if *on { t.accent } else { t.bg_raised },
    );
    let thumb_radius = (theme::TOGGLE_H - theme::SP_2) * 0.5;
    let thumb_x = if *on {
        rect.max.x - theme::SP_1 - thumb_radius
    } else {
        rect.min.x + theme::SP_1 + thumb_radius
    };
    ui.painter().circle_filled(
        egui::pos2(thumb_x, rect.center().y),
        thumb_radius,
        t.fg_strong,
    );
    response
}

pub fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> egui::Response {
    let inner = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SP_0;
        options
            .iter()
            .map(|(option, label)| {
                let selected = *value == *option;
                let t = theme::active();
                let text_color = if selected {
                    t.fg_strong
                } else {
                    t.fg_muted_raised
                };
                let galley = ui.painter().layout_no_wrap(
                    (*label).to_owned(),
                    egui::FontId::proportional(theme::FONT_SM),
                    text_color,
                );
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(galley.size().x + theme::SP_4 * 2.0, theme::BTN_H_ACTION),
                    egui::Sense::click(),
                );
                if response.clicked() {
                    *value = *option;
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                ui.painter().rect_filled(
                    rect,
                    theme::R_SM,
                    if selected { t.accent_bg } else { t.bg_raised },
                );
                ui.painter().galley(
                    egui::pos2(
                        rect.center().x - galley.size().x * 0.5,
                        rect.center().y - galley.size().y * 0.5,
                    ),
                    galley,
                    text_color,
                );
                response
            })
            .collect::<Vec<_>>()
    });

    let mut response = inner.response;
    for segment in inner.inner {
        response |= segment;
    }
    response
}

#[cfg(test)]
mod tests {
    use crate::theme::{self, ThemeId};
    use crate::ui_kit::test_harness::{click_at, ctx_with_fonts, filled_rects};

    #[test]
    fn toggle_switch_click_flips_value() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let mut on = false;
        let out = click_at(&ctx, egui::pos2(26.0, 18.0), |ui| {
            super::toggle_switch(ui, &mut on);
        });

        assert!(on);
        assert!(filled_rects(&out).iter().any(|(rect, fill)| {
            *fill == theme::active().accent
                && (rect.width() - theme::TOGGLE_W).abs() < f32::EPSILON
                && (rect.height() - theme::TOGGLE_H).abs() < f32::EPSILON
        }));
    }

    #[test]
    fn segmented_click_sets_value() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let mut value = 1_u8;
        let options = [(1_u8, "One"), (2_u8, "Two")];
        let _ = click_at(&ctx, egui::pos2(70.0, 20.0), |ui| {
            super::segmented(ui, &mut value, &options);
        });

        assert_eq!(value, 2);
    }
}
