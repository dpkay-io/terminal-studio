pub(super) fn render_markdown(ui: &mut egui::Ui, content: &str) {
    use crate::syntax;
    use crate::theme;

    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);

    let lines: Vec<&str> = content.lines().collect();
    let len = lines.len();
    let mut i = 0;
    let mut code_block_idx: usize = 0;

    while i < len {
        let line = lines[i];

        // Fenced code block
        if line.starts_with("```") {
            let lang = line.trim_start_matches('`').trim();
            i += 1;
            let mut code_buf: Vec<&str> = Vec::new();
            while i < len && !lines[i].starts_with("```") {
                code_buf.push(lines[i]);
                i += 1;
            }
            if i < len {
                i += 1;
            }
            ui.add_space(theme::SP_2);
            let maybe_syntax = if lang.is_empty() {
                None
            } else {
                syntax::find_syntax_for_language(lang)
            };
            let code_text = code_buf.join("\n");

            egui::Frame::none()
                .fill(theme::active().md_code_bg)
                .stroke(egui::Stroke::new(
                    theme::STROKE_THIN,
                    theme::active().md_code_border,
                ))
                .inner_margin(egui::Margin::symmetric(theme::SP_4, theme::SP_3))
                .rounding(egui::Rounding::same(theme::R_MD))
                .show(ui, |ui| {
                    let avail = ui.available_width();
                    ui.set_min_width(avail);
                    ui.spacing_mut().scroll.floating_allocated_width = 0.0;
                    egui::ScrollArea::horizontal()
                        .id_source(("md_code_scroll", code_block_idx))
                        .show(ui, |ui| {
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                            if let Some(syn) = maybe_syntax {
                                let highlighted = syntax::highlighted_lines(&code_text, syn);
                                for spans in &highlighted {
                                    let job = build_line_job(spans);
                                    ui.label(job);
                                }
                            } else {
                                let th = theme::active();
                                let code_fg = theme::ensure_readable(
                                    [th.md_code.r(), th.md_code.g(), th.md_code.b()],
                                    [th.md_code_bg.r(), th.md_code_bg.g(), th.md_code_bg.b()],
                                );
                                for code_line in &code_buf {
                                    ui.label(
                                        egui::RichText::new(*code_line)
                                            .monospace()
                                            .size(theme::FONT_UI_MD)
                                            .color(code_fg),
                                    );
                                }
                            }
                        });
                });
            ui.add_space(theme::SP_2);
            code_block_idx += 1;
            continue;
        }

        // Table: detect pipe-delimited lines
        if is_table_row(line) && i + 1 < len && is_separator_row(lines[i + 1]) {
            let mut table_rows: Vec<Vec<&str>> = Vec::new();
            table_rows.push(parse_table_cells(line));
            i += 2; // skip header + separator
            while i < len && is_table_row(lines[i]) {
                table_rows.push(parse_table_cells(lines[i]));
                i += 1;
            }
            render_table(ui, &table_rows);
            ui.add_space(theme::SP_2);
            continue;
        }

        // Headings
        if let Some(t) = line.strip_prefix("#### ") {
            ui.label(egui::RichText::new(t).size(theme::FONT_UI_LG).strong());
        } else if let Some(t) = line.strip_prefix("### ") {
            ui.label(egui::RichText::new(t).size(theme::FONT_UI_LG).strong());
        } else if let Some(t) = line.strip_prefix("## ") {
            ui.add_space(theme::SP_2);
            ui.label(egui::RichText::new(t).size(theme::FONT_HEADING_2).strong());
        } else if let Some(t) = line.strip_prefix("# ") {
            ui.add_space(theme::SP_2);
            ui.label(egui::RichText::new(t).size(theme::FONT_HEADING_1).strong());
            ui.add_space(theme::SP_1);
        }
        // Unordered list
        else if let Some(rest) = strip_list_prefix(line) {
            let indent = leading_spaces(line) / 2;
            ui.horizontal(|ui| {
                ui.add_space(indent as f32 * theme::SP_6);
                ui.label(egui::RichText::new("•").color(theme::active().md_bullet));
                theme::render_inline(ui, rest);
            });
        }
        // Ordered list: digits followed by . or )
        else if let Some((num, rest)) = strip_ordered_prefix(line) {
            let indent = leading_spaces(line) / 2;
            ui.horizontal(|ui| {
                ui.add_space(indent as f32 * theme::SP_6);
                ui.label(egui::RichText::new(format!("{}.", num)).color(theme::active().md_bullet));
                theme::render_inline(ui, rest);
            });
        }
        // Blockquote
        else if let Some(t) = line.strip_prefix("> ") {
            ui.horizontal(|ui| {
                let bar_h = ui.text_style_height(&egui::TextStyle::Body);
                let (bar_rect, _) = ui.allocate_exact_size(
                    egui::vec2(theme::TAB_COLOR_STRIP_W, bar_h),
                    egui::Sense::hover(),
                );
                ui.painter()
                    .rect_filled(bar_rect, 0.0, theme::active().overlay0);
                ui.add_space(theme::SP_3);
                ui.label(
                    egui::RichText::new(t)
                        .italics()
                        .color(theme::active().md_blockquote),
                );
            });
        }
        // Horizontal rule
        else if line.starts_with("---") && line.chars().all(|c| c == '-') {
            ui.separator();
        }
        // Empty line
        else if line.is_empty() {
            ui.add_space(theme::SP_2);
        }
        // Normal paragraph
        else {
            theme::render_inline(ui, line);
        }

        i += 1;
    }
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn strip_list_prefix(line: &str) -> Option<&str> {
    let trimmed = line.trim_start_matches(' ');
    trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
}

fn strip_ordered_prefix(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start_matches(' ');
    let dot = trimmed.find(". ").or_else(|| trimmed.find(") "));
    if let Some(pos) = dot {
        let prefix = &trimmed[..pos];
        if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
            let sep_char = trimmed.as_bytes()[pos];
            let rest = &trimmed[pos + 2..];
            if sep_char == b'.' || sep_char == b')' {
                return Some((prefix, rest));
            }
        }
    }
    None
}

fn is_table_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.ends_with('|') && t.len() > 2
}

fn is_separator_row(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with('|') || !t.ends_with('|') {
        return false;
    }
    t[1..t.len() - 1].split('|').all(|cell| {
        let c = cell.trim();
        !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
    })
}

fn parse_table_cells(line: &str) -> Vec<&str> {
    let t = line.trim();
    let inner = if t.starts_with('|') && t.ends_with('|') {
        &t[1..t.len() - 1]
    } else {
        t
    };
    inner.split('|').map(|s| s.trim()).collect()
}

fn build_line_job(spans: &[(egui::Color32, String)]) -> egui::text::LayoutJob {
    use crate::theme;
    let t = theme::active();
    let code_bg_rgb = t.md_code_bg.to_array();
    let bg_rgb = [code_bg_rgb[0], code_bg_rgb[1], code_bg_rgb[2]];
    let mut job = egui::text::LayoutJob::default();
    let font = egui::FontId::monospace(theme::FONT_UI_MD);
    for (color, text) in spans {
        let fg_rgb = [color.r(), color.g(), color.b()];
        let safe_color = theme::ensure_readable(fg_rgb, bg_rgb);
        job.append(
            text,
            0.0,
            egui::TextFormat::simple(font.clone(), safe_color),
        );
    }
    if job.text.is_empty() {
        let code_fg_rgb = [t.md_code.r(), t.md_code.g(), t.md_code.b()];
        let safe_code = theme::ensure_readable(code_fg_rgb, bg_rgb);
        job.append(" ", 0.0, egui::TextFormat::simple(font, safe_code));
    }
    job
}

fn render_table(ui: &mut egui::Ui, rows: &[Vec<&str>]) {
    use crate::theme;

    if rows.is_empty() {
        return;
    }

    let th = theme::active();
    let col_count = rows[0].len();
    if col_count == 0 {
        return;
    }

    let cell_h_padding = theme::SP_4;
    let border_width = theme::STROKE_THIN;

    ui.add_space(theme::SP_2);
    egui::Frame::none()
        .stroke(egui::Stroke::new(border_width, th.md_table_border))
        .rounding(egui::Rounding::same(theme::R_MD))
        .show(ui, |ui| {
            let total_width = ui.available_width();
            ui.set_min_width(total_width);

            let borders_width = border_width * (col_count as f32 - 1.0);
            let padding_width = cell_h_padding * 2.0 * col_count as f32;
            let content_width =
                (total_width - borders_width - padding_width).max(col_count as f32 * 20.0);
            let col_content_width = content_width / col_count as f32;

            egui::Grid::new(ui.next_auto_id())
                .num_columns(col_count)
                .min_col_width(0.0)
                .spacing(egui::vec2(0.0, 0.0))
                .show(ui, |ui| {
                    for (row_idx, row) in rows.iter().enumerate() {
                        let is_header = row_idx == 0;
                        for (col_idx, cell) in row.iter().enumerate() {
                            let bg = if is_header {
                                th.md_table_header_bg
                            } else if row_idx % 2 == 0 {
                                th.md_table_row_alt_bg
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let frame =
                                egui::Frame::none()
                                    .fill(bg)
                                    .inner_margin(egui::Margin::symmetric(
                                        cell_h_padding,
                                        theme::SP_1 + 1.0,
                                    ));

                            let cell_resp = frame.show(ui, |ui| {
                                ui.set_min_width(col_content_width);
                                if is_header {
                                    ui.label(
                                        egui::RichText::new(*cell).strong().size(theme::FONT_UI_MD),
                                    );
                                } else {
                                    theme::render_inline(ui, cell);
                                }
                            });

                            if col_idx < col_count.saturating_sub(1) {
                                let r = cell_resp.response.rect;
                                ui.painter().line_segment(
                                    [egui::pos2(r.max.x, r.min.y), egui::pos2(r.max.x, r.max.y)],
                                    egui::Stroke::new(
                                        border_width,
                                        th.md_table_border.linear_multiply(0.5),
                                    ),
                                );
                            }
                        }
                        ui.end_row();
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_table_row_valid() {
        assert!(is_table_row("| a | b | c |"));
        assert!(is_table_row("|a|b|"));
        assert!(!is_table_row("no pipes here"));
        assert!(is_table_row("| |"));
        assert!(!is_table_row("||")); // len == 2
    }

    #[test]
    fn is_separator_row_valid() {
        assert!(is_separator_row("|---|---|"));
        assert!(is_separator_row("| --- | :---: | ---: |"));
        assert!(!is_separator_row("| abc | def |"));
        assert!(!is_separator_row("not a separator"));
    }

    #[test]
    fn parse_table_cells_basic() {
        let cells = parse_table_cells("| foo | bar | baz |");
        assert_eq!(cells, vec!["foo", "bar", "baz"]);
    }

    #[test]
    fn parse_table_cells_trimmed() {
        let cells = parse_table_cells("|  a  |b|  c  |");
        assert_eq!(cells, vec!["a", "b", "c"]);
    }

    #[test]
    fn strip_list_prefix_basic() {
        assert_eq!(strip_list_prefix("- item"), Some("item"));
        assert_eq!(strip_list_prefix("* item"), Some("item"));
        assert_eq!(strip_list_prefix("  - nested"), Some("nested"));
        assert_eq!(strip_list_prefix("no list"), None);
    }

    #[test]
    fn strip_ordered_prefix_basic() {
        assert_eq!(strip_ordered_prefix("1. first"), Some(("1", "first")));
        assert_eq!(strip_ordered_prefix("12. twelfth"), Some(("12", "twelfth")));
        assert_eq!(strip_ordered_prefix("1) paren"), Some(("1", "paren")));
        assert_eq!(strip_ordered_prefix("not a list"), None);
        assert_eq!(strip_ordered_prefix("abc. nope"), None);
    }

    #[test]
    fn leading_spaces_count() {
        assert_eq!(leading_spaces("hello"), 0);
        assert_eq!(leading_spaces("  hello"), 2);
        assert_eq!(leading_spaces("    hello"), 4);
    }
}
