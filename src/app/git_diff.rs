use crate::theme;

pub(super) enum GitStageAction {
    Stage(String),
    Unstage(String),
}

pub(super) struct GitDiffResult {
    pub(super) stage_action: Option<GitStageAction>,
    pub(super) open_diff_file: Option<String>,
    pub(super) open_file: Option<String>,
}

pub(super) fn render_git_diff(ui: &mut egui::Ui, diff: &str, status: &str) -> GitDiffResult {
    let mut action: Option<GitStageAction> = None;
    let mut open_diff_file: Option<String> = None;
    let mut open_file: Option<String> = None;

    if !status.is_empty() {
        struct StatusEntry {
            tag: &'static str,
            path: String,
            color: egui::Color32,
        }
        let mut staged: Vec<StatusEntry> = Vec::new();
        let mut unstaged: Vec<StatusEntry> = Vec::new();

        for line in status.lines() {
            if line.len() < 3 {
                continue;
            }
            let x = line.as_bytes()[0];
            let y = line.as_bytes()[1];
            let path = line[3..].trim().to_string();

            match x {
                b'M' => staged.push(StatusEntry { tag: "M", path: path.clone(), color: theme::active().git_modified }),
                b'A' => staged.push(StatusEntry { tag: "A", path: path.clone(), color: theme::active().git_added }),
                b'D' => staged.push(StatusEntry { tag: "D", path: path.clone(), color: theme::active().git_removed }),
                b'R' => staged.push(StatusEntry { tag: "R", path: path.clone(), color: theme::active().git_renamed }),
                _ => {}
            }

            if x == b'?' && y == b'?' {
                unstaged.push(StatusEntry { tag: "?", path, color: theme::active().git_untracked });
            } else {
                match y {
                    b'M' => unstaged.push(StatusEntry { tag: "M", path, color: theme::active().git_modified }),
                    b'D' => unstaged.push(StatusEntry { tag: "D", path, color: theme::active().git_removed }),
                    _ => {}
                }
            }
        }

        ui.label(
            egui::RichText::new("Staged")
                .strong()
                .size(theme::STATUS_FONT_SZ)
                .color(theme::active().git_added),
        );
        ui.add_space(theme::SP_SM);
        if staged.is_empty() {
            ui.label(
                egui::RichText::new("Nothing staged — click + to stage a file")
                    .italics()
                    .size(11.0)
                    .color(theme::active().overlay0),
            );
        } else {
            for entry in &staged {
                let resp = ui.horizontal(|ui| {
                    let btn = ui.add(
                        egui::Button::new(
                            egui::RichText::new("\u{2212}")
                                .monospace()
                                .size(14.0)
                                .color(theme::active().git_removed),
                        )
                        .frame(false),
                    );
                    let unstage_clicked = btn.on_hover_text("Unstage").clicked();
                    let label_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new(&entry.path)
                                .monospace()
                                .size(theme::STATUS_FONT_SZ),
                        )
                        .truncate()
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Click: diff · Double-click: open file");
                    if label_resp.double_clicked() {
                        open_file = Some(entry.path.clone());
                    } else if label_resp.clicked() {
                        open_diff_file = Some(entry.path.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(theme::BAR_PAD_X);
                        let (badge_rect, _) =
                            ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
                        ui.painter().rect_filled(
                            badge_rect,
                            theme::ROUNDING,
                            entry.color.gamma_multiply(0.25),
                        );
                        ui.painter().text(
                            badge_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            entry.tag,
                            egui::FontId::monospace(10.0),
                            entry.color,
                        );
                    });
                    if unstage_clicked {
                        return Some(entry.path.clone());
                    }
                    None
                });
                if let Some(path) = resp.inner {
                    action = Some(GitStageAction::Unstage(path));
                }
            }
        }
        ui.add_space(theme::BAR_PAD_X);
        ui.separator();
        ui.add_space(theme::SP_SM);

        if !unstaged.is_empty() {
            ui.label(
                egui::RichText::new("Changes")
                    .strong()
                    .size(theme::STATUS_FONT_SZ),
            );
            ui.add_space(theme::SP_SM);
            for entry in &unstaged {
                let resp = ui.horizontal(|ui| {
                    let btn = ui.add(
                        egui::Button::new(
                            egui::RichText::new("+")
                                .monospace()
                                .size(14.0)
                                .color(theme::active().git_added),
                        )
                        .frame(false),
                    );
                    let stage_clicked = btn.on_hover_text("Stage").clicked();
                    let label_resp = ui.add(
                        egui::Label::new(
                            egui::RichText::new(&entry.path)
                                .monospace()
                                .size(theme::STATUS_FONT_SZ),
                        )
                        .truncate()
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Click: diff · Double-click: open file");
                    if label_resp.double_clicked() {
                        open_file = Some(entry.path.clone());
                    } else if label_resp.clicked() {
                        open_diff_file = Some(entry.path.clone());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(theme::BAR_PAD_X);
                        let (badge_rect, _) =
                            ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
                        ui.painter().rect_filled(
                            badge_rect,
                            theme::ROUNDING,
                            entry.color.gamma_multiply(0.25),
                        );
                        ui.painter().text(
                            badge_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            entry.tag,
                            egui::FontId::monospace(10.0),
                            entry.color,
                        );
                    });
                    if stage_clicked {
                        return Some(entry.path.clone());
                    }
                    None
                });
                if let Some(path) = resp.inner {
                    action = Some(GitStageAction::Stage(path));
                }
            }
            ui.add_space(theme::BAR_PAD_X);
            ui.separator();
            ui.add_space(theme::SP_SM);
        }

        if staged.is_empty() && unstaged.is_empty() {
            ui.add_space(theme::SP_SM);
        }
    }

    let _ = diff;
    if status.is_empty() {
        ui.add_space(theme::SP_MD);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No changes")
                    .italics()
                    .color(theme::active().overlay0)
                    .size(13.0),
            );
            ui.label(
                egui::RichText::new("Working tree is clean")
                    .size(11.0)
                    .color(theme::active().overlay0),
            );
        });
        return GitDiffResult { stage_action: action, open_diff_file, open_file };
    }

    GitDiffResult { stage_action: action, open_diff_file, open_file }
}

pub(super) fn render_inline_diff(ui: &mut egui::Ui, diff_content: &str) {
    for line in diff_content.lines() {
        if line.starts_with("@@") {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::DIFF_FONT_SZ)
                    .color(theme::active().git_hunk),
            );
        } else if line.starts_with('+') {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::DIFF_FONT_SZ)
                    .color(theme::active().git_added),
            );
        } else if line.starts_with('-') {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::DIFF_FONT_SZ)
                    .color(theme::active().git_removed),
            );
        } else if line.starts_with("diff --git ") {
            ui.add_space(theme::BAR_PAD_X);
            let fname = line
                .strip_prefix("diff --git ")
                .and_then(|s| s.split(" b/").last())
                .unwrap_or(line);
            ui.label(
                egui::RichText::new(fname)
                    .strong()
                    .color(theme::active().git_filename)
                    .size(13.0),
            );
        } else if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ") {
            // skip meta
        } else {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::DIFF_FONT_SZ)
                    .color(theme::active().subtext0),
            );
        }
    }
}
