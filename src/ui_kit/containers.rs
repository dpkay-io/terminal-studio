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
    let screen_rect = ctx.content_rect();
    let mut dismissed = false;

    let open_t = ctx.animate_bool_with_time(
        egui::Id::unique(("dialog_fade", id_base)),
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
        let frame_fill = crate::theme::with_alpha(base, (255.0 * open_t) as u8);

        egui::Frame::NONE
            .fill(frame_fill)
            .corner_radius(egui::CornerRadius::same(theme::R_LG))
            .stroke(egui::Stroke::new(
                theme::STROKE_THIN,
                theme::active().surface2,
            ))
            .shadow(egui::epaint::Shadow {
                offset: [0, 2],
                blur: 12,
                spread: 4,
                color: theme::active().shadow_md,
            })
            .inner_margin(theme::margin_same(config.margin))
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
            .strong()
            .size(theme::FONT_UI_LG)
            .color(theme::active().text),
    );
    ui.add_space(theme::SP_4);
}

pub fn dialog_header_with_close(ui: &mut egui::Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong().size(theme::FONT_UI_LG));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let resp = ui.add(
                egui::Button::new(
                    egui::RichText::new("\u{00d7}")
                        .size(theme::FONT_UI_LG)
                        .color(theme::active().fg_muted),
                )
                .corner_radius(theme::R_MD)
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

pub fn dialog_footer(ui: &mut egui::Ui, add_buttons: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(theme::SP_4);
    ui.horizontal(add_buttons);
}

/// A popup list under `anchor`, laid out as egui 0.28's `popup_below_widget`
/// was (egui 0.37's `Popup` sizes each item to its text instead): items fill
/// the list's width, which is at least the anchor's. `open` opens, closes or
/// toggles it (`None` leaves egui's memory as it is); returns what
/// `add_contents` returned while it is open.
pub fn popup_below<R>(
    anchor: &egui::Response,
    id: egui::Id,
    open: Option<egui::SetOpenCommand>,
    close: egui::PopupCloseBehavior,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let frame_margin = egui::Frame::popup(&anchor.ctx.global_style()).total_margin();
    let inner_width = (anchor.rect.width() - frame_margin.sum().x).max(0.0);
    egui::Popup::from_response(anchor)
        .id(id)
        .open_memory(open)
        .close_behavior(close)
        .layout(egui::Layout::top_down_justified(egui::Align::LEFT))
        .width(inner_width)
        .show(|ui| {
            ui.set_min_width(inner_width);
            add_contents(ui)
        })
        .map(|r| r.inner)
}

#[cfg(test)]
mod tests {
    use crate::egui_test_frames::run_ui;

    #[test]
    fn popup_items_fill_the_list_at_least_as_wide_as_its_anchor() {
        const ANCHOR_W: f32 = 240.0;
        let ctx = egui::Context::default();
        let popup_id = egui::Id::unique("popup_below_test");
        egui::Popup::open_id(&ctx, popup_id);
        let mut item_widths = Vec::new();
        // A popup's first frame open is egui's invisible sizing pass; the
        // second is the one shown.
        for _ in 0..2 {
            item_widths.clear();
            let _ = run_ui(&ctx, egui::RawInput::default(), |ui| {
                let anchor = ui.allocate_response(egui::vec2(ANCHOR_W, 20.0), egui::Sense::click());
                super::popup_below(
                    &anchor,
                    popup_id,
                    None,
                    egui::PopupCloseBehavior::CloseOnClickOutside,
                    |ui| {
                        for label in ["A", "Longer item"] {
                            item_widths.push(ui.button(label).rect.width());
                        }
                    },
                );
            });
        }
        let margin = egui::Frame::popup(&ctx.global_style())
            .total_margin()
            .sum()
            .x;
        assert_eq!(item_widths.len(), 2, "the open popup drew its items");
        for w in item_widths {
            assert!(w >= ANCHOR_W - margin - 0.5, "item width {w}");
        }
    }
}
