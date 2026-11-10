use super::super::super::App;
use crate::theme;

pub(super) fn sysmon_rect(bar: egui::Rect, width: f32) -> egui::Rect {
    let size = egui::vec2(
        width.min(bar.width()).max(theme::SP_0),
        theme::SYSMON_CONTENT_H.min(bar.height()).max(theme::SP_0),
    );
    egui::Rect::from_center_size(bar.center(), size)
}

impl App {
    pub(super) fn paint_sys_monitor(&self, painter: &egui::Painter, rect: egui::Rect) {
        let stats = self
            .workers
            .sys_monitor
            .as_ref()
            .map(|monitor| monitor.stats())
            .unwrap_or_default();
        let t = theme::active();
        let font = egui::FontId::proportional(theme::FONT_XS);
        let metrics = [
            ("CPU", stats.cpu_percent, t.accent),
            ("MEM", stats.ram_percent, t.warning),
        ];
        let mut x = rect.min.x;

        for (index, (label, percent, color)) in metrics.into_iter().enumerate() {
            if index > 0 {
                x += theme::SP_4;
            }

            let label_galley = painter.layout_no_wrap(label.to_owned(), font.clone(), t.fg_muted);
            let label_pos = egui::pos2(x, rect.center().y - label_galley.size().y / 2.0);
            x += label_galley.size().x + theme::SP_1;
            painter.galley(label_pos, label_galley, t.fg_muted);

            let track = egui::Rect::from_center_size(
                egui::pos2(x + theme::SYSMON_BAR_W / 2.0, rect.center().y),
                egui::vec2(theme::SYSMON_BAR_W, theme::SYSMON_BAR_H),
            );
            painter.rect_filled(track, theme::R_SM, t.bg_raised);
            let fill_width = theme::SYSMON_BAR_W * (percent / 100.0).clamp(theme::SP_0, 1.0);
            if fill_width > theme::SP_0 {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        track.min,
                        egui::vec2(fill_width, theme::SYSMON_BAR_H),
                    ),
                    theme::R_SM,
                    color,
                );
            }
            x += theme::SYSMON_BAR_W + theme::SP_1;

            let percent_text = format!("{percent:>3.0}%");
            let percent_galley = painter.layout_no_wrap(percent_text, font.clone(), t.fg_muted);
            let percent_pos = egui::pos2(x, rect.center().y - percent_galley.size().y / 2.0);
            x += percent_galley.size().x;
            painter.galley(percent_pos, percent_galley, t.fg_muted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_kit::test_harness::{ctx_with_fonts, run_ui};

    #[test]
    fn sysmon_content_width_fits_slot_at_100_percent() {
        let ctx = ctx_with_fonts();
        let mut content_width = theme::SP_0;
        let _ = run_ui(&ctx, egui::RawInput::default(), |ui| {
            let painter = ui.painter();
            let font = egui::FontId::proportional(theme::FONT_XS);

            for (index, label) in ["CPU", "MEM"].into_iter().enumerate() {
                if index > 0 {
                    content_width += theme::SP_4;
                }
                content_width += painter
                    .layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::WHITE)
                    .size()
                    .x
                    + theme::SP_1
                    + theme::SYSMON_BAR_W
                    + theme::SP_1
                    + painter
                        .layout_no_wrap("100%".to_owned(), font.clone(), egui::Color32::WHITE)
                        .size()
                        .x;
            }
        });

        assert!(
            content_width <= theme::SYSMON_W,
            "100% system monitor content width {content_width} exceeds slot {}",
            theme::SYSMON_W
        );
    }
}
