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
    ui.fonts(|f| f.layout_job(job))
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
    egui::Frame::none()
        .fill(t.surface1)
        .rounding(egui::Rounding::same(theme::R_SM))
        .inner_margin(egui::Margin::symmetric(theme::SP_2, theme::SP_0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(label).size(theme::FONT_UI_SM).color(fg))
        })
        .inner
}
