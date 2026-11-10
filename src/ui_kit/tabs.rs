use crate::theme;

pub fn tab_item(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    badge: Option<&str>,
) -> egui::Response {
    let t = theme::active();
    let label_color = if selected { t.fg_strong } else { t.fg_muted };
    let label_galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme::FONT_MD),
        label_color,
    );
    let badge_galley = badge.map(|badge| {
        ui.painter().layout_no_wrap(
            badge.to_owned(),
            egui::FontId::proportional(theme::FONT_XS),
            t.fg_muted_raised,
        )
    });
    let badge_width = badge_galley
        .as_ref()
        .map_or(theme::SP_0, |galley| galley.size().x + theme::SP_2 * 2.0);
    let badge_gap = if badge_galley.is_some() {
        theme::SP_2
    } else {
        theme::SP_0
    };
    let width = theme::SP_4 * 2.0 + label_galley.size().x + badge_gap + badge_width;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, theme::TAB_H), egui::Sense::click());

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x, rect.max.y - theme::TAB_SELECTED_BAR_H),
                rect.max,
            ),
            theme::R_NONE,
            t.accent,
        );
    }

    let label_pos = egui::pos2(
        rect.min.x + theme::SP_4,
        rect.center().y - label_galley.size().y * 0.5,
    );
    let label_right = label_pos.x + label_galley.size().x;
    ui.painter().galley(label_pos, label_galley, label_color);

    if let Some(badge_galley) = badge_galley {
        let badge_rect = egui::Rect::from_center_size(
            egui::pos2(label_right + badge_gap + badge_width * 0.5, rect.center().y),
            egui::vec2(badge_width, badge_galley.size().y + theme::SP_1 * 2.0),
        );
        ui.painter()
            .rect_filled(badge_rect, theme::R_SM, t.bg_raised);
        ui.painter().galley(
            egui::pos2(
                badge_rect.center().x - badge_galley.size().x * 0.5,
                badge_rect.center().y - badge_galley.size().y * 0.5,
            ),
            badge_galley,
            t.fg_muted_raised,
        );
    }

    response
}

#[cfg(test)]
mod tests {
    use crate::theme::{self, ThemeId};
    use crate::ui_kit::test_harness::{ctx_with_fonts, filled_rects, run_ui};

    #[test]
    fn tab_item_selected_has_bottom_bar() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let out = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::tab_item(ui, "Terminal", true, Some("2"));
        });

        let bars: Vec<_> = filled_rects(&out)
            .into_iter()
            .filter(|(rect, fill)| {
                *fill == theme::active().accent
                    && (rect.height() - theme::TAB_SELECTED_BAR_H).abs() < f32::EPSILON
            })
            .collect();
        assert_eq!(bars.len(), 1);
    }
}
