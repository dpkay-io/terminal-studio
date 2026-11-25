//! The title row every Markdown view in a pane or side panel starts with:
//! the title (ellipsised, whole on hover) and the Markdown preview toggle
//! (👁) beside it. A notes header (`notes_header`) adds its notes file
//! button on the right; a Markdown file in a pane shows just this row
//! (`PreviewHeader`), so switching a note or a file to its preview looks
//! and works the same everywhere.

use crate::theme;

/// The Markdown preview toggle's icon.
pub(in crate::app) const PREVIEW_ICON: &str = "👁";
const PREVIEW_TIP: &str = "Preview Markdown";
const EDIT_TIP: &str = "Back to editing";

/// Gap between a header's icons: they are framed squares, so a wide gap
/// would only take the title's room.
pub(in crate::app) const ICON_GAP: f32 = theme::SP_1;
/// Space before the title, so it lines up with the pane's info bar text.
const TITLE_INSET: f32 = theme::SP_2;
/// Space between the title and the preview toggle, so the toggle's frame
/// never touches the text.
const TITLE_GAP: f32 = theme::SP_2;
/// The preview toggle's width.
const TOGGLE_W: f32 = theme::ICON_LG + 2.0 * theme::ICON_PAD;
/// The 👁 glyph's size: emoji glyphs are drawn smaller than symbols, so it
/// takes a larger size to match the ⚙ beside it.
const PREVIEW_ICON_SIZE: f32 = theme::FONT_STATUS;

/// What hovering a preview toggle says, with its shortcut: one for each
/// state, so a toggle can say what clicking it does.
pub(in crate::app) struct PreviewTips {
    /// While the text shows.
    pub(in crate::app) preview: String,
    /// While the preview shows.
    pub(in crate::app) edit: String,
}

impl PreviewTips {
    /// The tips, each passed through `with_shortcut` (which adds the chord).
    pub(in crate::app) fn new(with_shortcut: impl Fn(&str) -> String) -> Self {
        Self {
            preview: with_shortcut(PREVIEW_TIP),
            edit: with_shortcut(EDIT_TIP),
        }
    }

    /// What clicking the toggle does now.
    pub(in crate::app) fn for_state(&self, previewing: bool) -> &str {
        if previewing {
            &self.edit
        } else {
            &self.preview
        }
    }
}

/// A title row's parts, as drawn this frame.
pub(in crate::app) struct HeaderResponse {
    /// The title does nothing when clicked (it used to collapse the side
    /// panel's notes); the layout tests read where it went.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(in crate::app) title: egui::Response,
    /// The preview toggle, when shown.
    pub(in crate::app) preview: Option<egui::Response>,
}

impl HeaderResponse {
    /// Whether the preview toggle was clicked.
    pub(in crate::app) fn preview_toggled(&self) -> bool {
        self.preview.as_ref().is_some_and(egui::Response::clicked)
    }
}

/// The preview toggle of a title row.
#[derive(Clone, Copy)]
pub(in crate::app) struct PreviewToggle<'a> {
    /// Whether the preview is showing.
    pub(in crate::app) on: bool,
    pub(in crate::app) tips: &'a PreviewTips,
}

/// A title and the preview toggle beside it.
pub(in crate::app) struct TitleRow<'a> {
    pub(in crate::app) title: &'a str,
    pub(in crate::app) title_size: f32,
    /// `None` hides the toggle (not Markdown, nothing shown under the
    /// header, or a view that is always a preview).
    pub(in crate::app) toggle: Option<PreviewToggle<'a>>,
}

/// Something drawn after the toggle only while the whole title still fits
/// beside it (the notes header's ⚠).
pub(in crate::app) struct TitleBadge<'a> {
    pub(in crate::app) width: f32,
    pub(in crate::app) draw: Box<dyn FnOnce(&mut egui::Ui) + 'a>,
}

impl TitleRow<'_> {
    /// Draws the row left to right in the space `ui` has left.
    pub(in crate::app) fn show(
        &self,
        ui: &mut egui::Ui,
        badge: Option<TitleBadge<'_>>,
    ) -> HeaderResponse {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = ICON_GAP;
            ui.add_space(TITLE_INSET);
            let toggle_w = if self.toggle.is_some() {
                TITLE_GAP + TOGGLE_W + ICON_GAP
            } else {
                0.0
            };
            let natural = self.galley(ui, f32::INFINITY).size().x;
            let fit = fit_title(
                ui.available_width() - toggle_w,
                natural,
                badge.as_ref().map(|b| b.width),
            );
            let title = self.label(ui, fit.title_w);
            let preview = self.toggle.map(|toggle| {
                ui.add_space(TITLE_GAP);
                preview_toggle(ui, toggle.on, toggle.tips.for_state(toggle.on))
            });
            if let (true, Some(badge)) = (fit.show_badge, badge) {
                (badge.draw)(ui);
            }
            HeaderResponse { title, preview }
        })
        .inner
    }

    /// The title laid out on one line no wider than `max_width`.
    fn galley(&self, ui: &egui::Ui, max_width: f32) -> std::sync::Arc<egui::Galley> {
        crate::ui_kit::one_line_elided(
            ui,
            self.title,
            egui::FontId::proportional(self.title_size),
            theme::active().text,
            max_width,
        )
    }

    /// The title, ellipsised to `max_width` (whole on hover).
    fn label(&self, ui: &mut egui::Ui, max_width: f32) -> egui::Response {
        let galley = self.galley(ui, max_width);
        let elided = galley.elided;
        // Not selectable text either: a click on the title does nothing.
        let resp = ui.add(
            egui::Label::new(galley)
                .selectable(false)
                .sense(egui::Sense::hover()),
        );
        if elided {
            crate::ui_kit::full_hover_text(resp, self.title)
        } else {
            resp
        }
    }
}

/// How the title and a badge share the room left of the preview toggle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TitleFit {
    title_w: f32,
    show_badge: bool,
}

/// The title comes first: a badge `badge_w` wide shows only when the whole
/// title still fits beside it. Pure.
fn fit_title(room: f32, title_natural_w: f32, badge_w: Option<f32>) -> TitleFit {
    let room = room.max(0.0);
    match badge_w {
        Some(w) if room - (w + ICON_GAP) >= title_natural_w => TitleFit {
            title_w: room - (w + ICON_GAP),
            show_badge: true,
        },
        _ => TitleFit {
            title_w: room,
            show_badge: false,
        },
    }
}

/// The 👁 toggle: lit while the preview shows (the text colour on the
/// selection fill, which an accent icon would barely show on).
fn preview_toggle(ui: &mut egui::Ui, on: bool, tip: &str) -> egui::Response {
    let t = theme::active();
    let color = if on { t.text } else { t.fg_muted };
    // Exactly this size: the title's room was worked out from it. Button
    // padding would make it wider; the square itself pads the glyph.
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::Vec2::ZERO;
        ui.add_sized(
            egui::vec2(TOGGLE_W, theme::HEADER_H - 2.0 * ICON_GAP),
            egui::Button::selectable(
                on,
                egui::RichText::new(PREVIEW_ICON)
                    .size(PREVIEW_ICON_SIZE)
                    .color(color),
            ),
        )
    })
    .inner
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text(tip)
}

/// The icon of the button that opens a side panel's view in a pane.
const OPEN_IN_PANE_ICON: &str = "\u{2B1C}";
const OPEN_IN_PANE_TIP: &str = "Open in main pane";

/// The square button on the right of a side panel's header that opens what
/// it shows (the notes, a Markdown file) in a pane.
pub(in crate::app) fn open_in_pane_button(ui: &mut egui::Ui) -> egui::Response {
    header_icon_button(ui, OPEN_IN_PANE_ICON).on_hover_text(OPEN_IN_PANE_TIP)
}

/// A frameless square icon button one header row high.
pub(in crate::app) fn header_icon_button(ui: &mut egui::Ui, icon: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(icon).size(theme::FONT_UI_MD))
            .min_size(egui::vec2(theme::HEADER_H, theme::HEADER_H))
            .frame(false),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The header above a Markdown file in a pane: its name and the preview
/// toggle, one header row high, as a notes header has them.
pub(in crate::app) struct PreviewHeader<'a> {
    pub(in crate::app) row: TitleRow<'a>,
}

impl PreviewHeader<'_> {
    pub(in crate::app) fn show(self, ui: &mut egui::Ui) -> HeaderResponse {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), theme::HEADER_H),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| self.row.show(ui, None),
        )
        .inner
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_test_frames::run_ui;

    #[test]
    fn the_title_keeps_its_room_before_a_badge_shows() {
        let room = 100.0;
        let badge = 20.0;
        let taken = badge + ICON_GAP;
        // Fits with the badge beside it.
        let fit = fit_title(room, room - taken, Some(badge));
        assert!(fit.show_badge);
        assert_eq!(fit.title_w, room - taken);
        // Would be cut short: the badge gives way.
        let fit = fit_title(room, room - taken + 1.0, Some(badge));
        assert!(!fit.show_badge);
        assert_eq!(fit.title_w, room);
        // No badge, none shown; no room never goes negative.
        assert!(!fit_title(room, 10.0, None).show_badge);
        assert_eq!(fit_title(-5.0, 10.0, Some(badge)).title_w, 0.0);
    }

    #[test]
    fn the_toggle_says_what_a_click_does_with_its_shortcut() {
        let tips = PreviewTips::new(|tip| format!("{tip} (\u{21E7}\u{2318}M)"));
        assert_eq!(
            tips.for_state(false),
            "Preview Markdown (\u{21E7}\u{2318}M)"
        );
        assert_eq!(tips.for_state(true), "Back to editing (\u{21E7}\u{2318}M)");
    }

    fn click(pos: egui::Pos2) -> egui::RawInput {
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        egui::RawInput {
            events: vec![egui::Event::PointerMoved(pos), press(true), press(false)],
            ..Default::default()
        }
    }

    /// One frame of a file's preview header; the title and toggle rects,
    /// and whether the toggle was clicked.
    fn header_frame(ctx: &egui::Context, input: egui::RawInput) -> (egui::Rect, egui::Rect, bool) {
        let tips = PreviewTips::new(str::to_string);
        let mut out = None;
        let _ = run_ui(ctx, input, |ui| {
            let r = PreviewHeader {
                row: TitleRow {
                    title: "README.md",
                    title_size: theme::FONT_UI_LG,
                    toggle: Some(PreviewToggle {
                        on: false,
                        tips: &tips,
                    }),
                },
            }
            .show(ui);
            let toggle = r.preview.as_ref().expect("a toggle");
            out = Some((r.title.rect, toggle.rect, r.preview_toggled()));
        });
        out.expect("the frame ran")
    }

    #[test]
    fn a_file_header_has_the_toggle_beside_its_name() {
        let ctx = egui::Context::default();
        let (title, toggle, _) = header_frame(&ctx, egui::RawInput::default());
        assert!(
            title.right() <= toggle.left(),
            "the toggle follows the name"
        );
        assert!(toggle.height() <= theme::HEADER_H);
        let (.., toggled) = header_frame(&ctx, click(toggle.center()));
        assert!(toggled, "a click on 👁 toggles the preview");
        let (.., toggled) = header_frame(&ctx, click(title.center()));
        assert!(!toggled, "the name itself does nothing");
    }
}
