use crate::theme;

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
