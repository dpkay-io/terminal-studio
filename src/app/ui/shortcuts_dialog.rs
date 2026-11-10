use std::ops::Range;

use super::super::App;
use crate::{theme, ui_kit};

const DIALOG_MAX_WIDTH: f32 = 800.0;
const DIALOG_MIN_WIDTH: f32 = 560.0;
const HEADER_AND_GAP_HEIGHT: f32 = 48.0;

fn dialog_height(screen_height: f32) -> f32 {
    (screen_height * 0.84).clamp(260.0, 640.0)
}

fn scroll_height(dialog_height: f32) -> f32 {
    (dialog_height - HEADER_AND_GAP_HEIGHT).max(120.0)
}

fn column_ranges(group_count: usize) -> [Range<usize>; 2] {
    let midpoint = group_count.div_ceil(2);
    [0..midpoint, midpoint..group_count]
}

fn header(ui: &mut egui::Ui) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Keyboard Shortcuts")
                .font(egui::FontId::new(theme::FONT_LG, crate::fonts::ui_bold()))
                .color(theme::active().fg_strong),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let response = ui.add(
                egui::Button::new(ui_kit::icon(
                    ui_kit::icon::X,
                    theme::FONT_MD,
                    theme::active().fg_muted,
                ))
                .rounding(theme::R_MD)
                .min_size(egui::vec2(theme::BTN_W, theme::BTN_W)),
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            close = response.clicked();
        });
    });
    let separator = ui.allocate_space(egui::vec2(ui.available_width(), theme::STROKE_THIN));
    ui.painter()
        .rect_filled(separator.1, theme::R_NONE, theme::active().border);
    close
}

fn shortcut_group(ui: &mut egui::Ui, group: &crate::shortcuts::ShortcutGroup, app: &App) {
    if group.entries.is_empty() {
        return;
    }

    let t = theme::active();
    ui.label(
        egui::RichText::new(group.name)
            .font(egui::FontId::new(theme::FONT_XS, crate::fonts::ui_bold()))
            .color(t.accent),
    );
    ui.add_space(theme::SP_1);

    for (action, _) in &group.entries {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), theme::DIALOG_ITEM_H),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new(action.description())
                        .size(theme::FONT_MD)
                        .color(t.fg),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(labels) = app.shortcut_registry.chip_labels_for(*action) {
                        ui_kit::kbd_combo_labels(ui, labels);
                    }
                });
            },
        );
        let separator = ui.allocate_space(egui::vec2(ui.available_width(), theme::STROKE_THIN));
        ui.painter()
            .rect_filled(separator.1, theme::R_NONE, t.border);
    }
}

pub fn render_shortcuts_dialog(app: &mut App, ctx: &egui::Context) {
    if !app.show_shortcut_help {
        return;
    }

    let mut close = false;
    let height = dialog_height(ctx.screen_rect().height());
    let response = ui_kit::dialog(
        ctx,
        app.vp_id("shortcut"),
        ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Responsive {
                pct: 0.72,
                min: DIALOG_MIN_WIDTH,
                max: DIALOG_MAX_WIDTH,
            },
            max_height: height,
            anchor: ui_kit::DialogAnchor::Center,
            ..Default::default()
        },
        |ui| {
            close = header(ui);
            ui.add_space(theme::SP_3);

            egui::ScrollArea::vertical()
                .id_source("shortcuts_scroll")
                .max_height(scroll_height(height))
                .show(ui, |ui| {
                    let groups = app.shortcut_registry.groups();
                    let ranges = column_ranges(groups.len());
                    ui.columns(2, |columns| {
                        for (column, range) in columns.iter_mut().zip(ranges) {
                            column.spacing_mut().item_spacing.y = theme::SP_1;
                            for group in &groups[range] {
                                shortcut_group(column, group, app);
                                column.add_space(theme::SP_4);
                            }
                        }
                    });
                });
        },
    );

    if close || response.dismissed {
        app.show_shortcut_help = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_dialog_is_wide_and_scrollable_at_400px_height() {
        assert_eq!(DIALOG_MAX_WIDTH, 800.0);
        let height = dialog_height(400.0);
        assert!(height < 400.0);
        assert!(scroll_height(height) > 0.0);
    }

    #[test]
    fn shortcut_groups_are_split_across_two_columns() {
        assert_eq!(column_ranges(7), [0..4, 4..7]);
        assert_eq!(column_ranges(6), [0..3, 3..6]);
    }
}
