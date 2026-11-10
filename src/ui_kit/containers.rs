use crate::theme;

// ── Dialog types ───────────────────────────────────────────────────────────

pub enum DialogAnchor {
    Center,
}

pub enum DialogWidth {
    Fixed(f32),
    Responsive { pct: f32, min: f32, max: f32 },
}

pub struct DialogConfig {
    pub width: DialogWidth,
    pub max_height: f32,
    pub anchor: DialogAnchor,
    pub margin: f32,
    pub dismiss_on_click_outside: bool,
}

impl Default for DialogConfig {
    fn default() -> Self {
        Self {
            width: DialogWidth::Fixed(340.0),
            max_height: 200.0,
            anchor: DialogAnchor::Center,
            margin: theme::DIALOG_MARGIN,
            dismiss_on_click_outside: true,
        }
    }
}

pub struct DialogResponse {
    pub dismissed: bool,
}

pub fn dialog(
    ctx: &egui::Context,
    id_base: egui::Id,
    config: DialogConfig,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> DialogResponse {
    let screen_rect = ctx.screen_rect();
    let mut dismissed = false;

    let open_t = ctx.animate_bool_with_time(
        egui::Id::new(("dialog_fade", id_base)),
        true,
        theme::ANIM_NORMAL,
    );

    egui::Area::new(id_base.with("_dim"))
        .fixed_pos(screen_rect.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            if config.dismiss_on_click_outside {
                let resp = ui.interact(
                    screen_rect,
                    id_base.with("_dim_click"),
                    egui::Sense::click(),
                );
                if resp.clicked() {
                    dismissed = true;
                }
            }
            let dim_alpha = (theme::ALPHA_OVERLAY_DIM as f32 * open_t) as u8;
            ui.painter()
                .rect_filled(screen_rect, 0.0, egui::Color32::from_black_alpha(dim_alpha));
        });

    let dialog_w = match config.width {
        DialogWidth::Fixed(w) => w,
        DialogWidth::Responsive { pct, min, max } => (screen_rect.width() * pct).clamp(min, max),
    };

    let area = egui::Area::new(id_base.with("_dialog")).order(egui::Order::Tooltip);

    let area = match config.anchor {
        DialogAnchor::Center => area.anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)),
    };

    area.show(ctx, |ui| {
        let base = theme::active().bg_term;
        let frame_fill = egui::Color32::from_rgba_unmultiplied(
            base.r(),
            base.g(),
            base.b(),
            (255.0 * open_t) as u8,
        );

        egui::Frame::none()
            .fill(frame_fill)
            .rounding(egui::Rounding::same(theme::R_LG))
            .stroke(egui::Stroke::new(
                theme::STROKE_THIN,
                theme::active().surface2,
            ))
            .shadow(egui::epaint::Shadow {
                offset: egui::vec2(0.0, 2.0),
                blur: 12.0,
                spread: 4.0,
                color: theme::active().shadow_md,
            })
            .inner_margin(egui::Margin::same(config.margin))
            .show(ui, |ui| {
                ui.set_min_width(dialog_w);
                ui.set_max_height(config.max_height);
                add_contents(ui);
            });
    });

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        dismissed = true;
    }

    DialogResponse { dismissed }
}

// ── Dialog header ──────────────────────────────────────────────────────────

pub fn dialog_header(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .font(egui::FontId::new(theme::FONT_MD, crate::fonts::ui_bold()))
            .color(theme::active().text),
    );
    ui.add_space(theme::SP_4);
}

pub fn dialog_header_with_close(ui: &mut egui::Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .font(egui::FontId::new(theme::FONT_MD, crate::fonts::ui_bold())),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let resp = ui.add(
                egui::Button::new(crate::ui_kit::icon(
                    crate::ui_kit::icon::X,
                    theme::FONT_MD,
                    theme::active().fg_muted,
                ))
                .rounding(theme::R_MD)
                .min_size(egui::vec2(theme::BTN_W, theme::BTN_W)),
            );
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                close = true;
            }
        });
    });
    // Subtle border instead of hard separator
    let sep_rect = ui.allocate_space(egui::vec2(ui.available_width(), 1.0)).1;
    ui.painter()
        .rect_filled(sep_rect, 0.0, theme::active().border_subtle);
    close
}

// ── Dialog footer ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogButton {
    Secondary(&'static str),
    Primary(&'static str),
    DisabledPrimary(&'static str),
    Destructive(&'static str),
}

pub fn dialog_footer(ui: &mut egui::Ui, buttons: &[DialogButton]) -> Option<usize> {
    struct ButtonLayout {
        galley: std::sync::Arc<egui::Galley>,
        width: f32,
        fill: egui::Color32,
        text: egui::Color32,
        enabled: bool,
    }

    let t = theme::active();
    let layouts: Vec<_> = buttons
        .iter()
        .map(|button| {
            let (label, fill, text, enabled) = match button {
                DialogButton::Secondary(label) => (*label, t.bg_raised, t.fg_muted_raised, true),
                DialogButton::Primary(label) => {
                    let text = theme::text_on([t.accent.r(), t.accent.g(), t.accent.b()]);
                    (*label, t.accent, text, true)
                }
                DialogButton::DisabledPrimary(label) => {
                    (*label, t.bg_panel, t.fg_muted_raised, false)
                }
                DialogButton::Destructive(label) => (*label, t.danger, t.danger_text, true),
            };
            let galley = ui.painter().layout_no_wrap(
                label.to_owned(),
                egui::FontId::proportional(theme::FONT_SM),
                text,
            );
            ButtonLayout {
                width: galley.size().x + theme::SP_6 * 2.0,
                galley,
                fill,
                text,
                enabled,
            }
        })
        .collect();
    let gaps = theme::SP_2 * layouts.len().saturating_sub(1) as f32;
    let buttons_width = layouts.iter().map(|layout| layout.width).sum::<f32>() + gaps;
    let footer_height = theme::DIALOG_BTN_H + theme::SP_6 * 2.0;
    let (footer_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), footer_height),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            footer_rect.min,
            egui::vec2(footer_rect.width(), theme::STROKE_THIN),
        ),
        theme::R_NONE,
        t.border,
    );

    let mut x = footer_rect.max.x - theme::SP_6 - buttons_width;
    let y = footer_rect.min.y + theme::SP_6;
    let mut clicked = None;
    for (index, layout) in layouts.into_iter().enumerate() {
        let rect = egui::Rect::from_min_size(
            egui::pos2(x, y),
            egui::vec2(layout.width, theme::DIALOG_BTN_H),
        );
        let response = ui.interact(
            rect,
            ui.id().with(("dialog_footer_button", index)),
            if layout.enabled {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            },
        );
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            clicked = Some(index);
        }
        ui.painter().rect_filled(rect, theme::R_MD, layout.fill);
        ui.painter().galley(
            egui::pos2(
                rect.center().x - layout.galley.size().x * 0.5,
                rect.center().y - layout.galley.size().y * 0.5,
            ),
            layout.galley,
            layout.text,
        );
        x += layout.width + theme::SP_2;
    }
    clicked
}

#[cfg(test)]
mod tests {
    use crate::theme::{self, ThemeId};
    use crate::ui_kit::test_harness::{click_at, ctx_with_fonts, filled_rects, run_ui};

    fn buttons() -> [super::DialogButton; 3] {
        [
            super::DialogButton::Secondary("Cancel"),
            super::DialogButton::Primary("Save"),
            super::DialogButton::Destructive("Delete"),
        ]
    }

    #[test]
    fn dialog_footer_orders_buttons_right_aligned() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let out = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::dialog_footer(ui, &buttons());
        });
        let rects = filled_rects(&out);
        let footer = rects
            .iter()
            .find(|(rect, fill)| {
                *fill == theme::active().border
                    && (rect.height() - theme::STROKE_THIN).abs() < f32::EPSILON
            })
            .map(|(rect, _)| *rect)
            .expect("footer paints its top border");
        let mut button_rects: Vec<_> = rects
            .into_iter()
            .filter(|(rect, fill)| {
                (rect.height() - theme::DIALOG_BTN_H).abs() < f32::EPSILON
                    && [
                        theme::active().bg_raised,
                        theme::active().accent,
                        theme::active().danger,
                    ]
                    .contains(fill)
            })
            .collect();
        button_rects.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x));

        assert_eq!(
            button_rects
                .iter()
                .map(|(_, fill)| *fill)
                .collect::<Vec<_>>(),
            vec![
                theme::active().bg_raised,
                theme::active().accent,
                theme::active().danger,
            ]
        );
        assert!((button_rects[2].0.max.x - (footer.max.x - theme::SP_6)).abs() < f32::EPSILON);
    }

    #[test]
    fn dialog_buttons_are_34px() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let out = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::dialog_footer(ui, &buttons());
        });
        let heights: Vec<_> = filled_rects(&out)
            .into_iter()
            .filter(|(rect, fill)| {
                rect.height() > theme::STROKE_THIN
                    && [
                        theme::active().bg_raised,
                        theme::active().accent,
                        theme::active().danger,
                    ]
                    .contains(fill)
            })
            .map(|(rect, _)| rect.height())
            .collect();
        assert_eq!(heights, vec![34.0, 34.0, 34.0]);
    }

    #[test]
    fn dialog_footer_returns_clicked_index() {
        let _guard = theme::test_support::lock_theme(ThemeId::CatppuccinMocha);
        let ctx = ctx_with_fonts();
        let initial = run_ui(&ctx, egui::RawInput::default(), |ui| {
            super::dialog_footer(ui, &buttons());
        });
        let destructive_center = filled_rects(&initial)
            .into_iter()
            .find(|(rect, fill)| {
                *fill == theme::active().danger
                    && (rect.height() - theme::DIALOG_BTN_H).abs() < f32::EPSILON
            })
            .map(|(rect, _)| rect.center())
            .expect("destructive button is painted");
        let mut clicked = None;
        let _ = click_at(&ctx, destructive_center, |ui| {
            if let Some(index) = super::dialog_footer(ui, &buttons()) {
                clicked = Some(index);
            }
        });
        assert_eq!(clicked, Some(2));
    }
}
