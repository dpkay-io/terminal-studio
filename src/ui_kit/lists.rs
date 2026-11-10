use crate::theme;

pub fn selectable_row(
    ui: &mut egui::Ui,
    selected: bool,
    height: f32,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click_and_drag());
    let t = theme::active();
    let fill = if selected {
        Some(t.accent_bg)
    } else if response.hovered() {
        Some(t.bg_raised)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, theme::R_MD, fill);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let _ = ui.allocate_ui_at_rect(rect, |ui| {
        ui.style_mut().interaction.selectable_labels = false;
        add_contents(ui);
    });
    ui.advance_cursor_after_rect(rect);
    response
}

// ── List item (command palette / quick switcher) ───────────────────────────

pub fn list_item(
    ui: &mut egui::Ui,
    id: egui::Id,
    width: f32,
    selected: bool,
    add_contents: impl FnOnce(&egui::Painter, egui::Rect),
) -> egui::Response {
    let response = ui
        .allocate_ui_with_layout(
            egui::vec2(width, theme::DIALOG_ITEM_H),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                ui.push_id(id, |ui| {
                    selectable_row(ui, selected, theme::DIALOG_ITEM_H, |_| {})
                })
                .inner
            },
        )
        .inner;
    add_contents(ui.painter(), response.rect);
    response
}

#[cfg(test)]
mod tests {
    use crate::theme::{self, ThemeId};
    use crate::ui_kit::test_harness::{click_at, ctx_with_fonts, filled_rects, run_ui};

    #[test]
    fn selectable_row_selected_paints_accent_bg() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let out = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::selectable_row(ui, true, theme::DIALOG_ITEM_H, |ui| {
                ui.label("Selected");
            });
        });

        assert!(filled_rects(&out)
            .iter()
            .any(|(_, fill)| *fill == theme::active().accent_bg));
    }

    #[test]
    fn selectable_row_clicks_when_truncated_label_is_clicked() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let mut label_rect = None;
        let _ = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::selectable_row(ui, false, theme::DIALOG_ITEM_H, |ui| {
                label_rect = Some(
                    crate::ui_kit::truncate_label(
                        ui,
                        "Selectable row label",
                        egui::FontId::proportional(theme::FONT_SM),
                        theme::active().fg,
                        120.0,
                    )
                    .rect,
                );
            });
        });
        let label_center = label_rect
            .expect("selectable row renders the truncated label")
            .center();

        let mut row_clicked = false;
        let _ = click_at(&ctx, label_center, |ui| {
            if super::selectable_row(ui, false, theme::DIALOG_ITEM_H, |ui| {
                crate::ui_kit::truncate_label(
                    ui,
                    "Selectable row label",
                    egui::FontId::proportional(theme::FONT_SM),
                    theme::active().fg,
                    120.0,
                );
            })
            .clicked()
            {
                row_clicked = true;
            }
        });

        assert!(row_clicked, "the row must receive clicks on its label");
    }

    #[test]
    fn selectable_row_hovers_when_pointer_is_on_label() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let mut label_rect = None;
        let _ = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::selectable_row(ui, false, theme::DIALOG_ITEM_H, |ui| {
                label_rect = Some(ui.label("Selectable row label").rect);
            });
        });
        let label_center = label_rect
            .expect("selectable row renders the label")
            .center();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::PointerMoved(label_center));
        let mut row_rect = egui::Rect::NOTHING;
        let out = run_ui(&ctx, input, |ui| {
            row_rect = super::selectable_row(ui, false, theme::DIALOG_ITEM_H, |ui| {
                ui.label("Selectable row label");
            })
            .rect;
        });

        assert!(
            filled_rects(&out)
                .iter()
                .any(|(rect, fill)| *rect == row_rect && *fill == theme::active().bg_raised),
            "the row must paint its hover fill while its label is hovered"
        );
    }

    #[test]
    fn selectable_row_contents_keep_button_interaction() {
        let ctx = ctx_with_fonts();
        let mut button_rect = None;
        let _ = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::selectable_row(ui, false, theme::DIALOG_ITEM_H, |ui| {
                button_rect = Some(ui.button("Action").rect);
            });
        });
        let button_center = button_rect
            .expect("selectable row renders the button")
            .center();

        let mut button_clicked = false;
        let _ = click_at(&ctx, button_center, |ui| {
            super::selectable_row(ui, false, theme::DIALOG_ITEM_H, |ui| {
                if ui.button("Action").clicked() {
                    button_clicked = true;
                }
            });
        });

        assert!(button_clicked, "buttons inside a row must remain clickable");
    }

    #[test]
    fn stacked_selectable_rows_do_not_overlap() {
        let ctx = ctx_with_fonts();
        let mut row_rects = Vec::new();
        let _ = run_ui(&ctx, egui::RawInput::default(), |ui| {
            row_rects.push(
                super::selectable_row(ui, false, theme::DIALOG_ITEM_H, |ui| {
                    ui.label("First");
                })
                .rect,
            );
            row_rects.push(
                super::selectable_row(ui, false, theme::DIALOG_ITEM_H, |ui| {
                    ui.label("Second");
                })
                .rect,
            );
        });

        assert!(
            row_rects[1].min.y >= row_rects[0].max.y,
            "stacked rows overlap: first={:?}, second={:?}",
            row_rects[0],
            row_rects[1]
        );
    }

    #[test]
    fn list_item_uses_selectable_row_selected_style() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let out = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::list_item(
                ui,
                egui::Id::new("selected-list-item"),
                200.0,
                true,
                |_, _| {},
            );
        });

        assert!(filled_rects(&out)
            .iter()
            .any(|(_, fill)| *fill == theme::active().accent_bg));
    }
}
