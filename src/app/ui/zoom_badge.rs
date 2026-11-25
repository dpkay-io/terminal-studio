//! Zooming a document pane from the pointer (⌘ / Ctrl + scroll, or a
//! pinch) and the badge in its corner that shows the zoom while it is not
//! 100 % — a click on it goes back to 100 %, as a browser's zoom indicator
//! does. What zoom does to the pane is `content_zoom`'s.

use super::super::content_zoom::{percent_label, ZoomRequest, ZOOM_CHORD, ZOOM_DEFAULT};
use crate::theme;

/// Space between the badge and the pane's edges, clear of its scroll bars.
const BADGE_INSET: f32 = theme::SP_6;
/// The badge's height.
const BADGE_H: f32 = theme::HEADER_H - 2.0 * theme::SP_1;

/// The zoom asked for with the pointer over `pane_rect` this frame, if any.
/// Nothing while something floats above the pane (a menu, a dialog).
pub(in crate::app) fn zoom_input(ui: &egui::Ui, pane_rect: egui::Rect) -> Option<ZoomRequest> {
    if !ui.rect_contains_pointer(pane_rect) {
        return None;
    }
    let factor = ui.input(|i| i.zoom_delta());
    (factor != ZOOM_DEFAULT).then_some(ZoomRequest::By(factor))
}

/// What hovering the badge says.
fn badge_tip(zoom: f32) -> String {
    format!(
        "Zoom {}: click to go back to 100%.\n{ZOOM_CHORD} to zoom.",
        percent_label(zoom)
    )
}

/// The badge in the bottom-right corner of `pane_rect` while `zoom` is not
/// 100 %. Draw it after the pane's content so it sits on top. Whether it
/// was clicked (back to 100 %).
pub(in crate::app) fn zoom_badge(ui: &mut egui::Ui, pane_rect: egui::Rect, zoom: f32) -> bool {
    if zoom == ZOOM_DEFAULT {
        return false;
    }
    let t = theme::active();
    let text = egui::RichText::new(percent_label(zoom))
        .size(theme::FONT_UI_SM)
        .color(t.text);
    let label_w = ui
        .fonts_mut(|f| {
            f.layout_no_wrap(
                percent_label(zoom),
                egui::FontId::proportional(theme::FONT_UI_SM),
                t.text,
            )
        })
        .size()
        .x;
    let size = egui::vec2(label_w + 2.0 * theme::SP_3, BADGE_H);
    let rect = egui::Rect::from_min_size(
        pane_rect.right_bottom() - size - egui::vec2(BADGE_INSET, BADGE_INSET),
        size,
    );
    ui.put(
        rect,
        egui::Button::new(text)
            .fill(t.surface1)
            .stroke(egui::Stroke::new(theme::STROKE_THIN, t.border_subtle))
            .corner_radius(theme::R_MD),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text(badge_tip(zoom))
    .clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_test_frames::run_ui;

    const PANE: egui::Rect = egui::Rect {
        min: egui::pos2(0.0, 0.0),
        max: egui::pos2(400.0, 300.0),
    };

    fn input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(PANE),
            events,
            ..Default::default()
        }
    }

    /// What one frame with `events` asks of a pane covering the screen.
    fn asked(ctx: &egui::Context, events: Vec<egui::Event>) -> Option<ZoomRequest> {
        let mut out = None;
        let _ = run_ui(ctx, input(events), |ui| out = zoom_input(ui, PANE));
        out
    }

    #[test]
    fn a_pinch_over_the_pane_zooms_it() {
        let ctx = egui::Context::default();
        let over = egui::Event::PointerMoved(PANE.center());
        assert_eq!(asked(&ctx, vec![over.clone()]), None, "no zoom, no request");
        assert_eq!(
            asked(&ctx, vec![over, egui::Event::Zoom(1.25)]),
            Some(ZoomRequest::By(1.25))
        );
    }

    #[test]
    fn a_pinch_elsewhere_leaves_the_pane_alone() {
        let ctx = egui::Context::default();
        let mut out = Some(ZoomRequest::Reset);
        let elsewhere = egui::Rect::from_min_size(egui::pos2(500.0, 500.0), egui::vec2(10.0, 10.0));
        let _ = run_ui(
            &ctx,
            input(vec![
                egui::Event::PointerMoved(PANE.center()),
                egui::Event::Zoom(1.25),
            ]),
            |ui| out = zoom_input(ui, elsewhere),
        );
        assert_eq!(out, None);
    }

    #[test]
    fn the_badge_shows_only_while_zoomed_and_a_click_resets() {
        let ctx = egui::Context::default();
        // Where the badge sits: bottom right, inset.
        let at = PANE.right_bottom() - egui::vec2(BADGE_INSET + 4.0, BADGE_INSET + BADGE_H / 2.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let click = || {
            input(vec![
                egui::Event::PointerMoved(at),
                press(true),
                press(false),
            ])
        };
        let mut clicked = false;
        for zoom in [ZOOM_DEFAULT, 1.5] {
            let _ = run_ui(&ctx, input(vec![]), |ui| {
                clicked = zoom_badge(ui, PANE, zoom)
            });
            let _ = run_ui(&ctx, click(), |ui| clicked = zoom_badge(ui, PANE, zoom));
            assert_eq!(clicked, zoom != ZOOM_DEFAULT, "zoom {zoom}");
        }
    }

    #[test]
    fn the_badge_says_how_to_zoom() {
        let tip = badge_tip(1.5);
        assert!(tip.starts_with("Zoom 150%: click to go back to 100%."));
        assert!(tip.contains(ZOOM_CHORD));
    }
}
