use std::path::Path;

use crate::theme;
use crate::ui_kit;

pub(in crate::app) struct StatusBarData {
    pub cwd: String,
    pub git_branch: String,
    pub git_diff_count: usize,
    pub shell_name: String,
    pub cols: u16,
    pub rows: u16,
    pub zoomed: bool,
    pub unsaved_folder: bool,
}

pub(in crate::app) struct StatusBarResult {
    pub save_workspace_clicked: bool,
}

pub(in crate::app) fn render_status_bar(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    data: &StatusBarData,
) -> StatusBarResult {
    let t = theme::active();
    let painter = ui.painter();

    painter.rect_filled(rect, 0.0, t.surface0);
    painter.line_segment(
        [rect.left_top(), rect.right_top()],
        egui::Stroke::new(theme::STROKE_THIN, t.surface1),
    );

    let font = egui::FontId::monospace(theme::FONT_UI_XS);
    let fg = t.subtext0;
    let accent = t.blue;
    let y_center = rect.center().y;
    let pad = theme::SP_3;
    let mut x = rect.min.x + pad;

    // CWD
    if !data.cwd.is_empty() {
        let short = theme::short_path(Path::new(&data.cwd));
        let galley = painter.layout_no_wrap(short, font.clone(), fg);
        let w = galley.size().x;
        painter.galley(egui::pos2(x, y_center - galley.size().y * 0.5), galley, fg);
        x += w + pad;
    }

    // Git branch
    if !data.git_branch.is_empty() {
        let sep = "\u{2502}";
        let sep_galley = painter.layout_no_wrap(sep.to_string(), font.clone(), t.overlay0);
        painter.galley(
            egui::pos2(x, y_center - sep_galley.size().y * 0.5),
            sep_galley,
            t.overlay0,
        );
        x += painter
            .layout_no_wrap(sep.to_string(), font.clone(), t.overlay0)
            .size()
            .x
            + theme::SP_2;

        let branch_icon = "\u{2387}";
        let branch_text = format!("{} {}", branch_icon, data.git_branch);
        let branch_galley = painter.layout_no_wrap(branch_text.clone(), font.clone(), accent);
        painter.galley(
            egui::pos2(x, y_center - branch_galley.size().y * 0.5),
            branch_galley,
            accent,
        );
        x += painter
            .layout_no_wrap(branch_text, font.clone(), accent)
            .size()
            .x;

        if data.git_diff_count > 0 {
            x += theme::SP_1;
            let diff_text = format!(" +{}", data.git_diff_count);
            let diff_galley = painter.layout_no_wrap(diff_text.clone(), font.clone(), t.yellow);
            painter.galley(
                egui::pos2(x, y_center - diff_galley.size().y * 0.5),
                diff_galley,
                t.yellow,
            );
        }
    }

    // Right side: shell name, dimensions, zoom indicator
    let mut right_parts: Vec<(String, egui::Color32)> = Vec::new();

    if data.zoomed {
        right_parts.push(("\u{26F6} ZOOM".to_string(), t.yellow));
    }

    right_parts.push((data.shell_name.clone(), fg));
    right_parts.push((format!("{}\u{00d7}{}", data.cols, data.rows), t.overlay0));

    let mut rx = rect.max.x - pad;
    for (text, color) in right_parts.iter().rev() {
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), *color);
        let w = galley.size().x;
        rx -= w;
        painter.galley(
            egui::pos2(rx, y_center - galley.size().y * 0.5),
            galley,
            *color,
        );
        rx -= pad;
    }

    // "Save as workspace" pill — clickable element on the right side
    let mut save_clicked = false;
    if data.unsaved_folder && !data.cwd.is_empty() {
        let label = "Save as workspace";
        let pill_font = egui::FontId::monospace(theme::FONT_UI_XS);
        let text_size = {
            let p = ui.painter();
            let g = p.layout_no_wrap(label.to_string(), pill_font.clone(), t.accent);
            g.size()
        };
        let pill_h = text_size.y + theme::SP_1 * 2.0;
        let pill_w = text_size.x + theme::SP_4 * 2.0;

        let pill_x = rx - pill_w - pad;
        let pill_rect = egui::Rect::from_min_size(
            egui::pos2(pill_x, y_center - pill_h * 0.5),
            egui::vec2(pill_w, pill_h),
        );

        let resp = ui_kit::pill_button(ui, label, pill_rect);

        if resp.clicked() {
            save_clicked = true;
        }
    }

    ui.allocate_rect(rect, egui::Sense::hover());

    StatusBarResult {
        save_workspace_clicked: save_clicked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_bar_data_default_fields() {
        let data = StatusBarData {
            cwd: "/home/user/project".to_string(),
            git_branch: "main".to_string(),
            git_diff_count: 3,
            shell_name: "bash".to_string(),
            cols: 120,
            rows: 40,
            zoomed: false,
            unsaved_folder: false,
        };
        assert_eq!(data.cols, 120);
        assert_eq!(data.rows, 40);
        assert_eq!(data.git_branch, "main");
        assert!(!data.zoomed);
        assert!(!data.unsaved_folder);
    }

    #[test]
    fn status_bar_data_empty_cwd() {
        let data = StatusBarData {
            cwd: String::new(),
            git_branch: String::new(),
            git_diff_count: 0,
            shell_name: "powershell".to_string(),
            cols: 80,
            rows: 24,
            zoomed: true,
            unsaved_folder: false,
        };
        assert!(data.cwd.is_empty());
        assert!(data.zoomed);
    }

    #[test]
    fn status_bar_data_unsaved_folder() {
        let data = StatusBarData {
            cwd: "/home/user/new-project".to_string(),
            git_branch: String::new(),
            git_diff_count: 0,
            shell_name: "bash".to_string(),
            cols: 80,
            rows: 24,
            zoomed: false,
            unsaved_folder: true,
        };
        assert!(data.unsaved_folder);
        assert!(!data.cwd.is_empty());
    }

    #[test]
    fn status_bar_data_unsaved_folder_empty_cwd() {
        let data = StatusBarData {
            cwd: String::new(),
            git_branch: String::new(),
            git_diff_count: 0,
            shell_name: "bash".to_string(),
            cols: 80,
            rows: 24,
            zoomed: false,
            unsaved_folder: true,
        };
        assert!(data.unsaved_folder);
        assert!(data.cwd.is_empty());
    }
}
