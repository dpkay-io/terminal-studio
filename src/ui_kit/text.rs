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
