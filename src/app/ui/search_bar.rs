use crate::theme;

/// Outcome returned by [`search_bar`] so the caller can react to user actions.
pub(crate) struct SearchBarResponse {
    /// The inner [`egui::Response`] of the text input.
    pub response: egui::Response,
    /// `true` when the user pressed Escape while the input was focused.
    pub escaped: bool,
}

pub(crate) fn search_bar(
    ui: &mut egui::Ui,
    query: &mut String,
    glyph: char,
    hint: &str,
    id: egui::Id,
) -> SearchBarResponse {
    search_bar_inner(ui, query, glyph, hint, id, false)
}

pub(crate) fn search_bar_persistent(
    ui: &mut egui::Ui,
    query: &mut String,
    glyph: char,
    hint: &str,
    id: egui::Id,
    request_focus: bool,
) -> SearchBarResponse {
    search_bar_inner(ui, query, glyph, hint, id, request_focus)
}

fn search_bar_inner(
    ui: &mut egui::Ui,
    query: &mut String,
    glyph: char,
    hint: &str,
    id: egui::Id,
    request_focus: bool,
) -> SearchBarResponse {
    let mut escaped = false;
    let mut inner_resp: Option<egui::Response> = None;
    let t = theme::active();

    let container_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(container_w, theme::SEARCH_BAR_H),
        egui::Sense::hover(),
    );

    let has_focus = ui.memory(|m| m.has_focus(id));
    let border_color = if has_focus { t.border_focus } else { t.border };

    ui.painter().rect_filled(rect, theme::R_SM, t.bg_base);
    ui.painter().rect_stroke(
        rect,
        theme::R_SM,
        egui::Stroke::new(theme::STROKE_THIN, border_color),
    );

    let inner_rect = rect.shrink2(egui::vec2(theme::SP_3, theme::SP_0));
    ui.allocate_ui_at_rect(inner_rect, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = theme::SP_2;
            ui.label(crate::ui_kit::icon(glyph, theme::FONT_XS, t.fg_muted));
            let te = egui::TextEdit::singleline(query)
                .desired_width(ui.available_width())
                .hint_text(hint)
                .font(egui::FontId::proportional(theme::FONT_XS))
                .frame(false)
                .id(id);
            let r = ui.add(te);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                escaped = true;
            }
            if request_focus {
                r.request_focus();
            }
            inner_resp = Some(r);
        });
    });

    SearchBarResponse {
        response: inner_resp.unwrap(),
        escaped,
    }
}

#[cfg(test)]
mod tests {
    use crate::theme::{self, ThemeId};
    use crate::ui_kit::test_harness::{ctx_with_fonts, run_ui};

    fn has_rect_color(
        shape: &egui::Shape,
        color: egui::Color32,
        select: impl Fn(&egui::epaint::RectShape) -> egui::Color32 + Copy,
    ) -> bool {
        match shape {
            egui::Shape::Rect(rect) => select(rect) == color,
            egui::Shape::Vec(shapes) => shapes
                .iter()
                .any(|shape| has_rect_color(shape, color, select)),
            _ => false,
        }
    }

    #[test]
    fn search_bar_uses_base_fill_and_border() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let mut query = String::new();
        let out = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::search_bar(
                ui,
                &mut query,
                crate::ui_kit::icon::SEARCH,
                "Search",
                egui::Id::new("base-search"),
            );
        });

        assert!(out.shapes.iter().any(|clipped| has_rect_color(
            &clipped.shape,
            theme::active().bg_base,
            |rect| rect.fill,
        )));
        assert!(out.shapes.iter().any(|clipped| has_rect_color(
            &clipped.shape,
            theme::active().border,
            |rect| rect.stroke.color,
        )));
    }
}
