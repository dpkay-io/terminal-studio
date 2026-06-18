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

    let dialog_pos = match config.anchor {
        DialogAnchor::Center => {
            let h_offset = (config.max_height / 2.0).min(screen_rect.height() / 2.0 - 10.0);
            screen_rect.center() - egui::vec2(dialog_w / 2.0, h_offset)
        }
    };

    egui::Area::new(id_base.with("_dialog"))
        .fixed_pos(dialog_pos)
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
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

pub fn dialog_footer(ui: &mut egui::Ui, add_buttons: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(theme::SP_4);
    ui.horizontal(add_buttons);
}
