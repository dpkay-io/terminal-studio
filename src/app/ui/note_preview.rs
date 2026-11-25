//! A note's body as the right panel and a notes pane show it: the text box's
//! placeholder, and the Markdown preview (the file editor's renderer).

use super::super::markdown::render_markdown_zoomed;
use crate::theme;

/// The notes text box's placeholder.
pub(in crate::app) const NOTES_PLACEHOLDER: &str = "Notes for this workspace\u{2026}";
/// What an empty note's preview says.
const EMPTY_PREVIEW: &str = "Nothing to preview yet. Turn off the preview (👁) to write.";

/// `text` rendered as Markdown at `zoom` (1.0: as designed); a hint when
/// there is nothing to show. The parse is cached per calling `Ui`, so this
/// is cheap while `text` stays the same.
pub(in crate::app) fn render_note_preview(ui: &mut egui::Ui, text: &str, zoom: f32) {
    if text.trim().is_empty() {
        ui.add_space(theme::SP_4 * zoom);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(EMPTY_PREVIEW)
                    .size(theme::FONT_UI_SM * zoom)
                    .color(theme::active().fg_muted),
            );
        });
        return;
    }
    render_markdown_zoomed(ui, text, zoom);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::content_zoom::ZOOM_DEFAULT;
    use crate::egui_test_frames::run_ui;

    /// How tall the preview of `text` comes out.
    fn preview_height(text: &str) -> f32 {
        zoomed_height(text, ZOOM_DEFAULT)
    }

    /// How tall the preview of `text` comes out at `zoom`.
    fn zoomed_height(text: &str, zoom: f32) -> f32 {
        let ctx = egui::Context::default();
        let mut h = 0.0;
        let _ = run_ui(&ctx, egui::RawInput::default(), |ui| {
            h = ui
                .vertical(|ui| render_note_preview(ui, text, zoom))
                .response
                .rect
                .height();
        });
        h
    }

    #[test]
    fn markdown_and_empty_notes_both_render() {
        let empty = preview_height("  \n");
        assert!(empty > 0.0, "an empty note says so");
        let doc = preview_height("# Title\n\n- one\n- two\n\n```\ncode\n```\n");
        assert!(doc > empty, "a note's Markdown is laid out");
    }

    #[test]
    fn a_zoomed_preview_is_bigger() {
        const DOC: &str = "# Title\n\nSome text.\n\n```\ncode\n```\n";
        let normal = preview_height(DOC);
        let zoomed = zoomed_height(DOC, 2.0);
        assert!(zoomed > normal * 1.6, "{zoomed} vs {normal}");
        assert!(
            zoomed_height("", 2.0) > preview_height(""),
            "the empty hint too"
        );
    }
}
