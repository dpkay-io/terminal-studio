use crate::git::parser::{parse_git_status, FileChangeKind};
use crate::theme;

pub(super) enum GitStageAction {
    Stage(String),
    Unstage(String),
    StageAll,
    UnstageAll,
}

pub(super) struct GitDiffResult {
    pub(super) stage_action: Option<GitStageAction>,
    pub(super) open_diff_file: Option<String>,
    pub(super) open_file: Option<String>,
}

fn kind_to_tag(kind: FileChangeKind) -> &'static str {
    match kind {
        FileChangeKind::Modified => "M",
        FileChangeKind::Added => "A",
        FileChangeKind::Deleted => "D",
        FileChangeKind::Renamed => "R",
        FileChangeKind::Untracked => "?",
    }
}

fn kind_to_color(kind: FileChangeKind) -> egui::Color32 {
    match kind {
        FileChangeKind::Modified => theme::active().git_modified,
        FileChangeKind::Added => theme::active().git_added,
        FileChangeKind::Deleted => theme::active().git_removed,
        FileChangeKind::Renamed => theme::active().git_renamed,
        FileChangeKind::Untracked => theme::active().git_untracked,
    }
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

        let parsed = parse_git_status(status);
        let mut staged: Vec<StatusEntry> = Vec::new();
        let mut unstaged: Vec<StatusEntry> = Vec::new();

        for fs in &parsed {
            let entry = StatusEntry {
                tag: kind_to_tag(fs.kind),
                path: fs.path.clone(),
                color: kind_to_color(fs.kind),
            };
            if fs.staged {
                staged.push(entry);
            } else {
                unstaged.push(entry);
            }
        }

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("Staged ({})", staged.len()))
                    .strong()
                    .size(theme::FONT_UI_MD)
                    .color(theme::active().git_added),
            );
            if !staged.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(theme::SP_4);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("\u{2212}")
                                    .monospace()
                                    .size(theme::FONT_TERM)
                                    .color(theme::active().git_removed),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Unstage All")
                        .clicked()
                    {
                        action = Some(GitStageAction::UnstageAll);
                    }
                });
            }
        });
        ui.add_space(theme::SP_2);
        if staged.is_empty() {
            ui.label(
                egui::RichText::new("Nothing staged — click + to stage a file")
                    .italics()
                    .size(theme::FONT_UI_SM)
                    .color(theme::active().overlay0),
            );
        } else {
            for entry in &staged {
                let resp = ui.horizontal(|ui| {
                    let (badge_rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
                    let badge_bg = entry.color.gamma_multiply(0.25);
                    ui.painter()
                        .rect_filled(badge_rect, theme::R_MD, badge_bg);
                    let badge_bg_rgb = [badge_bg.r(), badge_bg.g(), badge_bg.b()];
                    let badge_fg_rgb = [entry.color.r(), entry.color.g(), entry.color.b()];
                    let badge_fg = theme::ensure_readable(badge_fg_rgb, badge_bg_rgb);
                    ui.painter().text(
                        badge_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        entry.tag,
                        egui::FontId::monospace(10.0),
                        badge_fg,
                    );
                    let label_resp = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(&entry.path)
                                    .monospace()
                                    .size(theme::FONT_UI_MD),
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
                        ui.add_space(theme::SP_4);
                        let btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new("\u{2212}")
                                    .monospace()
                                    .size(theme::FONT_TERM)
                                    .color(theme::active().git_removed),
                            )
                            .frame(false),
                        );
                        if btn.on_hover_text("Unstage").clicked() {
                            return Some(entry.path.clone());
                        }
                        None
                    })
                    .inner
                });
                if let Some(path) = resp.inner {
                    action = Some(GitStageAction::Unstage(path));
                }
            }
        }
        ui.add_space(theme::SP_3);
        ui.separator();
        ui.add_space(theme::SP_2);

        if !unstaged.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("Changes ({})", unstaged.len()))
                        .strong()
                        .size(theme::FONT_UI_MD),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(theme::SP_4);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("+")
                                    .monospace()
                                    .size(theme::FONT_TERM)
                                    .color(theme::active().git_added),
                            )
                            .frame(false),
                        )
                        .on_hover_text("Stage All")
                        .clicked()
                    {
                        action = Some(GitStageAction::StageAll);
                    }
                });
            });
            ui.add_space(theme::SP_2);
            for entry in &unstaged {
                let resp = ui.horizontal(|ui| {
                    let (badge_rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
                    let badge_bg = entry.color.gamma_multiply(0.25);
                    ui.painter()
                        .rect_filled(badge_rect, theme::R_MD, badge_bg);
                    let badge_bg_rgb = [badge_bg.r(), badge_bg.g(), badge_bg.b()];
                    let badge_fg_rgb = [entry.color.r(), entry.color.g(), entry.color.b()];
                    let badge_fg = theme::ensure_readable(badge_fg_rgb, badge_bg_rgb);
                    ui.painter().text(
                        badge_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        entry.tag,
                        egui::FontId::monospace(10.0),
                        badge_fg,
                    );
                    let label_resp = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(&entry.path)
                                    .monospace()
                                    .size(theme::FONT_UI_MD),
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
                        ui.add_space(theme::SP_4);
                        let btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new("+")
                                    .monospace()
                                    .size(theme::FONT_TERM)
                                    .color(theme::active().git_added),
                            )
                            .frame(false),
                        );
                        if btn.on_hover_text("Stage").clicked() {
                            return Some(entry.path.clone());
                        }
                        None
                    })
                    .inner
                });
                if let Some(path) = resp.inner {
                    action = Some(GitStageAction::Stage(path));
                }
            }
            ui.add_space(theme::SP_3);
            ui.separator();
            ui.add_space(theme::SP_2);
        }

        if staged.is_empty() && unstaged.is_empty() {
            ui.add_space(theme::SP_2);
        }
    }

    let _ = diff;
    if status.is_empty() {
        ui.add_space(theme::SP_4);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No changes")
                    .italics()
                    .color(theme::active().overlay0)
                    .size(theme::FONT_UI_LG),
            );
            ui.label(
                egui::RichText::new("Working tree is clean")
                    .size(theme::FONT_UI_SM)
                    .color(theme::active().overlay0),
            );
        });
        return GitDiffResult {
            stage_action: action,
            open_diff_file,
            open_file,
        };
    }

    GitDiffResult {
        stage_action: action,
        open_diff_file,
        open_file,
    }
}

pub(super) fn render_inline_diff(ui: &mut egui::Ui, diff_content: &str) {
    for line in diff_content.lines() {
        if line.starts_with("@@") {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::FONT_UI_SM)
                    .color(theme::active().git_hunk),
            );
        } else if line.starts_with('+') {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::FONT_UI_SM)
                    .color(theme::active().git_added),
            );
        } else if line.starts_with('-') {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::FONT_UI_SM)
                    .color(theme::active().git_removed),
            );
        } else if line.starts_with("diff --git ") {
            ui.add_space(theme::SP_3);
            let fname = line
                .strip_prefix("diff --git ")
                .and_then(|s| s.split(" b/").last())
                .unwrap_or(line);
            ui.label(
                egui::RichText::new(fname)
                    .strong()
                    .color(theme::active().git_filename)
                    .size(theme::FONT_UI_LG),
            );
        } else if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ")
        {
            // skip meta
        } else {
            ui.label(
                egui::RichText::new(line)
                    .monospace()
                    .size(theme::FONT_UI_SM)
                    .color(theme::active().subtext0),
            );
        }
    }
}
