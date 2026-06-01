use crate::theme;

// ── Icon button styles ─────────────────────────────────────────────────────

pub enum IconButtonStyle {
    Default,
    Toggle { active: bool },
    Danger,
}

pub fn icon_button(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: egui::Rect,
    icon: &str,
    font_size: f32,
    fg: egui::Color32,
    style: IconButtonStyle,
) -> egui::Response {
    let resp = ui.interact(rect, id, egui::Sense::click());
    let t = theme::active();

    let bg = match style {
        IconButtonStyle::Default => {
            if resp.hovered() {
                t.surface1
            } else {
                egui::Color32::TRANSPARENT
            }
        }
        IconButtonStyle::Toggle { active } => {
            if active {
                t.surface2
            } else if resp.hovered() {
                t.surface1
            } else {
                egui::Color32::TRANSPARENT
            }
        }
        IconButtonStyle::Danger => {
            if resp.hovered() {
                t.danger_bg
            } else {
                egui::Color32::TRANSPARENT
            }
        }
    };

    let text_color = match style {
        IconButtonStyle::Danger => {
            if resp.hovered() {
                t.danger_fg
            } else {
                fg
            }
        }
        _ => fg,
    };

    ui.painter().rect_filled(rect, theme::R_NONE, bg);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(font_size),
        text_color,
    );

    resp
}

// ── Action button styles ───────────────────────────────────────────────────

pub enum ActionButtonStyle {
    Primary,
    Danger,
    Cancel,
}

pub fn action_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    style: ActionButtonStyle,
) -> egui::Response {
    let t = theme::active();
    match style {
        ActionButtonStyle::Primary => ui.add_enabled(
            enabled,
            egui::Button::new(egui::RichText::new(label).color(t.accent_strong)),
        ),
        ActionButtonStyle::Danger => ui.add_enabled(
            enabled,
            egui::Button::new(egui::RichText::new(label).color(t.danger_fg)).fill(t.danger_bg),
        ),
        ActionButtonStyle::Cancel => ui.add_enabled(enabled, egui::Button::new(label)),
    }
}

// ── Toggle chip ────────────────────────────────────────────────────────────

pub fn toggle_chip(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let t = theme::active();
    let btn = egui::Button::new(
        egui::RichText::new(label)
            .size(theme::FONT_UI_MD)
            .color(if selected { t.base } else { t.text }),
    )
    .fill(if selected { t.accent } else { t.surface1 })
    .rounding(theme::R_MD)
    .min_size(egui::vec2(0.0, theme::BTN_H_ACTION));
    ui.add(btn)
}

// ── Pill button ────────────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn pill_button(ui: &mut egui::Ui, label: &str, rect: egui::Rect) -> egui::Response {
    let t = theme::active();
    let resp = ui.allocate_rect(rect, egui::Sense::click());
    let hovered = resp.hovered();

    let tint = if hovered {
        theme::tinted(t.blue_rgb, theme::BLEND_LIGHT)
    } else {
        theme::tinted(t.blue_rgb, theme::BLEND_SUBTLE)
    };
    let bg = egui::Color32::from_rgb(tint[0], tint[1], tint[2]);
    let p = ui.painter();
    p.rect_filled(rect, theme::R_SM, bg);

    let pill_font = egui::FontId::monospace(theme::FONT_UI_XS);
    let text_galley = p.layout_no_wrap(label.to_string(), pill_font, t.accent);
    p.galley(
        egui::pos2(
            rect.min.x + theme::SP_4,
            rect.center().y - text_galley.size().y * 0.5,
        ),
        text_galley,
        t.accent,
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

// ── Dot menu button ────────────────────────────────────────────────────────

pub fn dot_menu_button(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: egui::Rect,
    open: bool,
) -> egui::Response {
    let t = theme::active();
    let resp = ui.interact(rect, id, egui::Sense::click());

    let bg = if open || resp.hovered() {
        t.surface2
    } else {
        t.surface1
    };
    ui.painter().rect_filled(rect, theme::R_MD, bg);

    let center = rect.center();
    for i in [-1.0_f32, 0.0, 1.0] {
        ui.painter().circle_filled(
            egui::pos2(center.x, center.y + i * theme::DOT_GAP),
            theme::DOT_R,
            t.text,
        );
    }

    resp
}

// ── Color swatch ───────────────────────────────────────────────────────────

pub fn color_swatch(ui: &mut egui::Ui, color: [u8; 3], selected: bool) -> egui::Response {
    let t = theme::active();
    let swatch = egui::Button::new("")
        .fill(theme::from_rgb(color))
        .stroke(if selected {
            egui::Stroke::new(theme::STROKE_BOLD, t.text)
        } else {
            egui::Stroke::new(theme::STROKE_THIN, t.overlay0)
        })
        .min_size(egui::vec2(theme::BTN_W, theme::BTN_W))
        .rounding(theme::R_MD);
    ui.add(swatch)
}
