//! The workspace colour row every workspace dialog shows: the preset
//! swatches, then a custom swatch that opens a colour picker for any other
//! colour.
//!
//! Everything opens inline, inside the dialog. The dialogs float on egui's
//! top (`Tooltip`) layer and egui's popups (its colour button's included)
//! open one layer below it, behind the dialog, where they can't be seen or
//! clicked.

use crate::app::workspace_colors::PRESET_COLORS;
use crate::app::workspace_ui::ColorChoice;
use crate::theme;
use crate::ui_kit;

const CUSTOM_COLOR_TOOLTIP: &str = "Pick any color";
const CUSTOM_COLOR_GLYPH: &str = "+";

/// How a colour was changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum ColorEdit {
    /// A preset swatch was clicked: a finished choice.
    Preset,
    /// The picker moved: it may keep moving while dragged.
    Custom,
}

/// Draws the swatches, the custom swatch, and the picker when it is open.
/// Returns how the user changed `choice` this frame, if they did.
pub(in crate::app) fn color_choices(
    ui: &mut egui::Ui,
    choice: &mut ColorChoice,
) -> Option<ColorEdit> {
    let open_id = ui.id().with("custom_color_open");
    let mut picker_open = ui.data(|d| d.get_temp::<bool>(open_id).unwrap_or(false));
    let mut edit = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(theme::SP_4, theme::SP_4);
        for &preset in PRESET_COLORS {
            if ui_kit::color_swatch(ui, preset, choice.is_preset_selected(preset)).clicked() {
                choice.select_preset(preset);
                picker_open = false;
                edit = Some(ColorEdit::Preset);
            }
        }
        if custom_swatch(ui, choice).clicked() {
            picker_open = !picker_open;
        }
    });
    if picker_open {
        let mut colour = theme::from_rgb(choice.custom());
        if egui::color_picker::color_picker_color32(
            ui,
            &mut colour,
            egui::color_picker::Alpha::Opaque,
        ) {
            choice.set_custom([colour.r(), colour.g(), colour.b()]);
            edit = Some(ColorEdit::Custom);
        }
    }
    ui.data_mut(|d| d.insert_temp(open_id, picker_open));
    edit
}

/// A swatch in the custom colour, marked "+", selected when the colour came
/// from the picker.
fn custom_swatch(ui: &mut egui::Ui, choice: &ColorChoice) -> egui::Response {
    let resp = ui_kit::color_swatch(ui, choice.custom(), choice.is_custom())
        .on_hover_text(CUSTOM_COLOR_TOOLTIP);
    ui.painter().text(
        resp.rect.center(),
        egui::Align2::CENTER_CENTER,
        CUSTOM_COLOR_GLYPH,
        egui::FontId::proportional(theme::FONT_UI_LG),
        theme::text_on(choice.custom()),
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        choice: &mut ColorChoice,
    ) -> Option<ColorEdit> {
        let mut edit = None;
        let input = egui::RawInput {
            events,
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            // On the dialogs' layer, as in the app.
            egui::Area::new(egui::Id::new("dialog"))
                .order(egui::Order::Tooltip)
                .fixed_pos(egui::pos2(10.0, 10.0))
                .show(ctx, |ui| {
                    ui.set_width(400.0);
                    edit = color_choices(ui, choice);
                });
        });
        edit
    }

    fn click(pos: egui::Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        vec![egui::Event::PointerMoved(pos), button(true), button(false)]
    }

    /// A new area spends its first frame sizing itself, invisible, so
    /// clicks land from the frame after.
    fn warm_up(ctx: &egui::Context, choice: &mut ColorChoice) {
        run(ctx, Vec::new(), choice);
        run(ctx, Vec::new(), choice);
    }

    /// The first swatch's centre: the row starts at the area's origin.
    fn first_swatch_centre() -> egui::Pos2 {
        egui::pos2(10.0 + theme::BTN_W / 2.0, 10.0 + theme::BTN_W / 2.0)
    }

    #[test]
    fn clicking_a_swatch_inside_a_dialog_picks_it() {
        let ctx = egui::Context::default();
        let mut choice = ColorChoice::new([1, 2, 3]);
        warm_up(&ctx, &mut choice);
        let edit = run(&ctx, click(first_swatch_centre()), &mut choice);
        assert_eq!(edit, Some(ColorEdit::Preset));
        assert_eq!(choice.color(), PRESET_COLORS[0]);
    }

    #[test]
    fn the_custom_swatch_opens_the_picker_inline() {
        let ctx = egui::Context::default();
        let mut choice = ColorChoice::new(PRESET_COLORS[0]);
        warm_up(&ctx, &mut choice);
        let height = |ctx: &egui::Context| {
            ctx.memory(|m| m.area_rect(egui::Id::new("dialog")).map(|r| r.height()))
        };
        let closed = height(&ctx).unwrap();
        // The custom swatch follows the presets on the same row.
        let step = theme::BTN_W + theme::SP_4;
        let custom = egui::pos2(
            10.0 + step * PRESET_COLORS.len() as f32 + theme::BTN_W / 2.0,
            10.0 + theme::BTN_W / 2.0,
        );
        run(&ctx, click(custom), &mut choice);
        run(&ctx, Vec::new(), &mut choice);
        let open = height(&ctx).unwrap();
        assert!(
            open > closed + 50.0,
            "picker drawn in the dialog: {closed} -> {open}"
        );
        assert!(
            ctx.memory(|m| !m.any_popup_open()),
            "nothing opens on a layer behind the dialog"
        );
    }
}
