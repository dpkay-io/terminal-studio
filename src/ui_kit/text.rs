use crate::theme;
use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, FontId, Galley, Painter, Response, Ui};
use std::sync::Arc;

#[allow(dead_code)] // Consumed by later UI migration tasks.
pub fn truncate_galley(
    painter: &Painter,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(font, color));
    job.wrap = TextWrapping::truncate_at_width(max_w.max(theme::SP_0));
    painter.layout_job(job)
}

#[allow(dead_code)] // Consumed by later UI migration tasks.
pub fn truncate_label(
    ui: &mut Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> Response {
    ui.scope(|ui| {
        ui.set_max_width(max_w.max(theme::SP_0));
        ui.add(egui::Label::new(egui::RichText::new(text).font(font).color(color)).truncate())
    })
    .inner
}

#[allow(dead_code)] // Consumed by later UI migration tasks.
pub fn kbd_chip(ui: &mut Ui, key: &str) -> Response {
    let t = theme::active();
    egui::Frame::none()
        .fill(t.bg_raised)
        .rounding(theme::R_SM)
        .inner_margin(egui::Margin::symmetric(theme::SP_2, theme::SP_1))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(key)
                    .font(FontId::monospace(theme::FONT_SM))
                    .color(t.fg_strong),
            );
        })
        .response
}

#[allow(dead_code)] // Consumed by later UI migration tasks.
pub fn kbd_combo(ui: &mut Ui, shortcut: &crate::shortcuts::Shortcut) {
    let ctrl = shortcut.ctrl.then_some(if cfg!(target_os = "macos") {
        "\u{2318}"
    } else {
        "Ctrl"
    });
    let alt = shortcut.alt.then_some(if cfg!(target_os = "macos") {
        "\u{2325}"
    } else {
        "Alt"
    });
    let shift = shortcut.shift.then_some(if cfg!(target_os = "macos") {
        "\u{21e7}"
    } else {
        "Shift"
    });
    kbd_combo_parts(
        ui,
        [
            ctrl,
            alt,
            shift,
            Some(crate::shortcuts::key_name(shortcut.key)),
        ]
        .into_iter()
        .flatten(),
    );
}

pub fn kbd_combo_labels(ui: &mut Ui, labels: &[String]) {
    kbd_combo_parts(ui, labels.iter().map(String::as_str));
}

fn kbd_combo_parts<'a>(ui: &mut Ui, labels: impl IntoIterator<Item = &'a str>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SP_1;
        for label in labels {
            kbd_chip(ui, label);
        }
    });
}

pub fn label(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text).size(theme::FONT_SM)
}

pub fn label_secondary(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme::FONT_SM)
        .color(theme::active().subtext0)
}

pub fn label_muted(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme::FONT_XS)
        .color(theme::active().overlay0)
}

pub fn hint(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(text)
        .size(theme::FONT_XS)
        .color(theme::active().overlay0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_kit::test_harness::{ctx_with_fonts, filled_rects, run_ui};

    fn truncated(text: &str, max_w: f32) -> std::sync::Arc<egui::Galley> {
        let ctx = ctx_with_fonts();
        let mut galley = None;
        let _ = run_ui(&ctx, egui::RawInput::default(), |ui| {
            galley = Some(super::truncate_galley(
                ui.painter(),
                text,
                egui::FontId::proportional(theme::FONT_SM),
                egui::Color32::WHITE,
                max_w,
            ));
        });
        galley.expect("the test frame lays out one galley")
    }

    #[test]
    fn truncate_short_text_unchanged() {
        let galley = truncated("Short", 200.0);
        assert!(!galley.elided);
        assert_eq!(galley.rows[0].text(), "Short");
        assert_eq!(galley.job.sections.len(), 1);
    }

    #[test]
    fn truncate_long_text_ends_with_ellipsis_and_fits() {
        let max_w = 70.0;
        let galley = truncated("A much longer label than the available width", max_w);
        assert!(galley.elided);
        assert!(galley.rows[0].text().ends_with('\u{2026}'));
        assert!(galley.size().x <= max_w);
    }

    #[test]
    fn truncate_zero_width_returns_ellipsis_only() {
        let galley = truncated("Anything", 0.0);
        assert!(galley.elided);
        assert_eq!(galley.rows[0].text(), "\u{2026}");
    }

    #[test]
    fn truncate_handles_multibyte() {
        let original = "Rosé Pine ✓ 日本語";
        let galley = truncated(original, 65.0);
        let rendered = galley.rows[0].text();
        assert!(galley.elided);
        assert!(rendered.ends_with('\u{2026}'));
        let prefix = rendered.trim_end_matches('\u{2026}');
        assert!(original.starts_with(prefix));
    }

    #[test]
    fn truncate_label_respects_max_width() {
        let ctx = ctx_with_fonts();
        let mut width = f32::INFINITY;
        let _ = run_ui(&ctx, egui::RawInput::default(), |ui| {
            width = super::truncate_label(
                ui,
                "A label that must be elided",
                egui::FontId::proportional(theme::FONT_SM),
                egui::Color32::WHITE,
                45.0,
            )
            .rect
            .width();
        });
        assert!(width <= 45.0);
    }

    #[test]
    fn kbd_chip_uses_raised_fill() {
        let _guard = theme::test_support::lock_theme(theme::ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let out = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::kbd_chip(ui, "Ctrl");
        });
        assert!(filled_rects(&out)
            .iter()
            .any(|(_, fill)| *fill == theme::active().bg_raised));
    }

    #[test]
    fn kbd_combo_renders_each_modifier_and_key_as_a_chip() {
        let _guard = theme::test_support::lock_theme(theme::ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let shortcut = crate::shortcuts::Shortcut {
            ctrl: true,
            shift: true,
            alt: true,
            key: egui::Key::K,
        };
        let out = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::kbd_combo(ui, &shortcut);
        });
        let chips = filled_rects(&out)
            .into_iter()
            .filter(|(_, fill)| *fill == theme::active().bg_raised)
            .count();
        assert_eq!(chips, 4);
    }
}
