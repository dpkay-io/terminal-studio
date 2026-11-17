use std::sync::Arc;

use crate::theme;

/// Hover text shown whole, each of its lines on one line. egui keeps a
/// tooltip at the size it first had and wrapping text never widens it, so a
/// text that grows (a renamed tab) would otherwise break between its words.
pub fn full_hover_text(resp: egui::Response, text: &str) -> egui::Response {
    resp.on_hover_ui(|ui| {
        ui.add(egui::Label::new(text).extend());
    })
}

/// A tooltip next to the pointer for a spot that is not a widget of its own
/// (an icon painted inside a row), shown for as long as this is called.
/// `id` names the spot; it must be unique in the window.
pub fn tooltip_at_pointer(ui: &egui::Ui, id: egui::Id, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Tooltip::always_open(
        ui.ctx().clone(),
        ui.layer_id(),
        id,
        egui::PopupAnchor::Pointer,
    )
    .show(add_contents);
}

/// Lay `text` out on one line no wider than `max_width`, ending in "…" when
/// it does not fit (`Galley::elided` then says so, for a hover with the
/// whole text).
pub fn one_line_elided(
    ui: &egui::Ui,
    text: &str,
    font_id: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id,
            color,
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: max_width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    ui.fonts_mut(|f| f.layout_job(job))
}

pub fn heading(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).strong().size(theme::FONT_UI_LG)
}

pub fn label(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).size(theme::FONT_UI_MD)
}

pub fn label_secondary(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme::FONT_UI_MD)
        .color(theme::active().subtext0)
}

pub fn label_muted(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme::FONT_UI_SM)
        .color(theme::active().overlay0)
}

pub fn hint(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme::FONT_UI_XS)
        .color(theme::active().overlay0)
}

/// A keyboard-shortcut chip. A padded frame, rather than an inline text
/// background, keeps fallback-font symbols such as the Option and Shift glyphs
/// fully inside the chip.
pub fn shortcut_badge(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let t = theme::active();
    let fg = theme::ensure_readable(t.subtext0_rgb, t.surface1_rgb);
    egui::Frame::NONE
        .fill(t.surface1)
        .corner_radius(egui::CornerRadius::same(theme::R_SM))
        .inner_margin(theme::margin_xy(theme::SP_2, theme::SP_0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).size(theme::FONT_UI_SM).color(fg))
        })
        .inner
}

/// A text box's font together with its placeholder in that same font.
/// egui 0.32+ draws `hint_text` in the body font whatever `TextEdit::font`
/// says (0.28 used the box's font), so a box that names a font and shows a
/// placeholder sets both here.
pub trait TextEditFontHint {
    fn font_with_hint(self, font: impl Into<egui::FontSelection>, hint: impl Into<String>) -> Self;
}

impl TextEditFontHint for egui::TextEdit<'_> {
    fn font_with_hint(self, font: impl Into<egui::FontSelection>, hint: impl Into<String>) -> Self {
        let font = font.into();
        let hint = egui::RichText::new(hint);
        let hint = match &font {
            egui::FontSelection::FontId(id) => hint.font(id.clone()),
            egui::FontSelection::Style(style) => hint.text_style(style.clone()),
            egui::FontSelection::Default => hint,
        };
        self.font(font).hint_text(hint)
    }
}

#[cfg(test)]
mod tests {
    use super::TextEditFontHint as _;
    use crate::egui_test_frames::run_ui;

    /// Font sizes of every text shape a frame painted.
    fn painted_font_sizes(output: &egui::FullOutput) -> Vec<f32> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => text.galley.job.sections.first(),
                _ => None,
            })
            .map(|section| section.format.font_id.size)
            .collect()
    }

    #[test]
    fn a_placeholder_is_drawn_in_its_text_box_font() {
        const BOX_FONT: f32 = 21.0;
        let ctx = egui::Context::default();
        let mut text = String::new();
        let output = run_ui(&ctx, egui::RawInput::default(), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut text)
                    .font_with_hint(egui::FontId::monospace(BOX_FONT), "Search"),
            );
        });
        let sizes = painted_font_sizes(&output);
        assert!(!sizes.is_empty(), "the placeholder was painted");
        assert!(sizes.iter().all(|s| *s == BOX_FONT), "{sizes:?}");
    }

    #[test]
    fn a_placeholder_follows_a_text_style_font() {
        let ctx = egui::Context::default();
        let mono = egui::TextStyle::Monospace.resolve(&ctx.global_style()).size;
        let mut text = String::new();
        let output = run_ui(&ctx, egui::RawInput::default(), |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .font_with_hint(egui::TextStyle::Monospace, "Notes"),
            );
        });
        let sizes = painted_font_sizes(&output);
        assert!(!sizes.is_empty(), "the placeholder was painted");
        assert!(sizes.iter().all(|s| *s == mono), "{sizes:?}");
    }
}
