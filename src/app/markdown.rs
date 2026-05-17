pub(super) fn render_markdown(ui: &mut egui::Ui, content: &str) {
    use crate::theme;
    let mut in_code = false;
    let mut code_buf: Vec<&str> = Vec::new();

    for line in content.lines() {
        if line.starts_with("```") {
            if in_code {
                in_code = false;
                egui::Frame::none()
                    .fill(theme::active().md_code_bg)
                    .stroke(egui::Stroke::new(theme::STROKE_THIN, theme::active().md_code_border))
                    .inner_margin(egui::Margin::symmetric(theme::SP_MD, theme::BAR_PAD_X))
                    .rounding(egui::Rounding::same(theme::ROUNDING))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        for code_line in &code_buf {
                            ui.label(
                                egui::RichText::new(*code_line)
                                    .monospace()
                                    .size(12.0)
                                    .color(theme::active().md_code),
                            );
                        }
                    });
                code_buf.clear();
                ui.add_space(theme::SP_SM);
            } else {
                in_code = true;
                ui.add_space(theme::SP_SM);
            }
            continue;
        }
        if in_code {
            code_buf.push(line);
            continue;
        }

        if let Some(t) = line.strip_prefix("# ") {
            ui.add_space(theme::SP_SM);
            ui.label(egui::RichText::new(t).size(22.0).strong());
            ui.add_space(theme::SP_XS);
        } else if let Some(t) = line.strip_prefix("## ") {
            ui.add_space(theme::SP_SM);
            ui.label(egui::RichText::new(t).size(18.0).strong());
        } else if let Some(t) = line.strip_prefix("### ") {
            ui.label(egui::RichText::new(t).size(theme::DIALOG_TITLE_SZ).strong());
        } else if let Some(t) = line.strip_prefix("#### ") {
            ui.label(egui::RichText::new(t).size(13.0).strong());
        } else if let Some(t) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("•").color(theme::active().md_bullet));
                theme::render_inline(ui, t);
            });
        } else if let Some(t) = line.strip_prefix("> ") {
            ui.horizontal(|ui| {
                let bar_h = ui.text_style_height(&egui::TextStyle::Body);
                let (bar_rect, _) =
                    ui.allocate_exact_size(egui::vec2(theme::TAB_COLOR_STRIP_W, bar_h), egui::Sense::hover());
                ui.painter().rect_filled(bar_rect, 0.0, theme::active().overlay0);
                ui.add_space(theme::BAR_PAD_X);
                ui.label(egui::RichText::new(t).italics().color(theme::active().md_blockquote));
            });
        } else if line.starts_with("---") && line.chars().all(|c| c == '-') {
            ui.separator();
        } else if line.is_empty() {
            ui.add_space(theme::SP_SM);
        } else {
            theme::render_inline(ui, line);
        }
    }
}
