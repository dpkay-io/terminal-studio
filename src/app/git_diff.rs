use crate::git::parser::{parse_git_status, FileChangeKind};
use crate::theme;

pub(super) enum GitStageAction {
    Stage(String),
    Unstage(String),
    UnstageAll,
}

pub(super) struct GitDiffResult {
    pub(super) stage_action: Option<GitStageAction>,
    pub(super) open_diff_file: Option<String>,
    pub(super) open_file: Option<String>,
    pub(super) show_commit_dialog: bool,
    pub(super) show_push_dialog: bool,
    pub(super) show_stage_all_confirm: bool,
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

pub(super) fn render_git_diff(
    ui: &mut egui::Ui,
    diff: &str,
    status: &str,
    unpushed: &[(String, String)],
) -> GitDiffResult {
    let mut action: Option<GitStageAction> = None;
    let mut open_diff_file: Option<String> = None;
    let mut open_file: Option<String> = None;
    let mut show_commit_dialog = false;
    let mut show_push_dialog = false;
    let mut show_stage_all_confirm = false;

    let panel_width = ui.available_width();
    ui.set_max_width(panel_width);

    let has_status = !status.is_empty();
    let has_unpushed = !unpushed.is_empty();

    if has_status || has_unpushed {
        struct StatusEntry {
            tag: &'static str,
            path: String,
            color: egui::Color32,
        }

        let parsed = if has_status { parse_git_status(status) } else { Vec::new() };
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

        // ── Committed (unpushed) section ────────────────────────────
        if !unpushed.is_empty() {
            let t = theme::active();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("Committed ({})", unpushed.len()))
                        .strong()
                        .size(theme::FONT_UI_MD)
                        .color(t.blue),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(theme::SP_4);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Push")
                                    .size(theme::FONT_UI_SM)
                                    .color(t.accent_strong),
                            )
                            .rounding(theme::R_SM),
                        )
                        .on_hover_text("Push to remote")
                        .clicked()
                    {
                        show_push_dialog = true;
                    }
                });
            });
            ui.add_space(theme::SP_2);
            for (hash, msg) in unpushed {
                ui.horizontal(|ui| {
                    ui.set_max_width(panel_width);
                    ui.label(
                        egui::RichText::new(hash)
                            .monospace()
                            .size(theme::FONT_UI_SM)
                            .color(t.yellow),
                    );
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(msg)
                                .size(theme::FONT_UI_SM)
                                .color(t.text),
                        )
                        .truncate(),
                    );
                });
            }
            ui.add_space(theme::SP_3);
            ui.separator();
            ui.add_space(theme::SP_2);
        }

        // ── Staged section ──────────────────────────────────────────
        if !staged.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("Staged ({})", staged.len()))
                        .strong()
                        .size(theme::FONT_UI_MD)
                        .color(theme::active().git_added),
                );
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
                    let t = theme::active();
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Commit")
                                    .size(theme::FONT_UI_SM)
                                    .color(t.accent_strong),
                            )
                            .rounding(theme::R_SM),
                        )
                        .on_hover_text("Commit staged changes")
                        .clicked()
                    {
                        show_commit_dialog = true;
                    }
                });
            });
            ui.add_space(theme::SP_2);
            for entry in &staged {
                let resp = ui.horizontal(|ui| {
                    ui.set_max_width(panel_width);
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
                    let btn_reserve = theme::SP_4 + 20.0;
                    let label_max = (ui.available_width() - btn_reserve).max(20.0);
                    let label_resp = ui
                        .allocate_ui(egui::vec2(label_max, 14.0), |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&entry.path)
                                        .monospace()
                                        .size(theme::FONT_UI_MD),
                                )
                                .truncate()
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text("Click: diff \u{00b7} Double-click: open file")
                        })
                        .inner;
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
            ui.add_space(theme::SP_3);
            ui.separator();
            ui.add_space(theme::SP_2);
        }

        // ── Changes (unstaged) section ──────────────────────────────
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
                        show_stage_all_confirm = true;
                    }
                });
            });
            ui.add_space(theme::SP_2);
            for entry in &unstaged {
                let resp = ui.horizontal(|ui| {
                    ui.set_max_width(panel_width);
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
                    let btn_reserve = theme::SP_4 + 20.0;
                    let label_max = (ui.available_width() - btn_reserve).max(20.0);
                    let label_resp = ui
                        .allocate_ui(egui::vec2(label_max, 14.0), |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&entry.path)
                                        .monospace()
                                        .size(theme::FONT_UI_MD),
                                )
                                .truncate()
                                .sense(egui::Sense::click()),
                            )
                            .on_hover_text("Click: diff \u{00b7} Double-click: open file")
                        })
                        .inner;
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
        }
    }

    let _ = diff;
    if status.is_empty() && unpushed.is_empty() {
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
    }

    GitDiffResult {
        stage_action: action,
        open_diff_file,
        open_file,
        show_commit_dialog,
        show_push_dialog,
        show_stage_all_confirm,
    }
}

pub(super) fn render_inline_diff(ui: &mut egui::Ui, diff_content: &str) {
    let max_w = ui.available_width();
    ui.set_max_width(max_w);
    for line in diff_content.lines() {
        if line.starts_with("@@") {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(line)
                        .monospace()
                        .size(theme::FONT_UI_SM)
                        .color(theme::active().git_hunk),
                )
                .truncate(),
            );
        } else if line.starts_with('+') {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(line)
                        .monospace()
                        .size(theme::FONT_UI_SM)
                        .color(theme::active().git_added),
                )
                .truncate(),
            );
        } else if line.starts_with('-') {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(line)
                        .monospace()
                        .size(theme::FONT_UI_SM)
                        .color(theme::active().git_removed),
                )
                .truncate(),
            );
        } else if line.starts_with("diff --git ") {
            ui.add_space(theme::SP_3);
            let fname = line
                .strip_prefix("diff --git ")
                .and_then(|s| s.split(" b/").last())
                .unwrap_or(line);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(fname)
                        .strong()
                        .color(theme::active().git_filename)
                        .size(theme::FONT_UI_LG),
                )
                .truncate(),
            );
        } else if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ")
        {
            // skip meta
        } else {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(line)
                        .monospace()
                        .size(theme::FONT_UI_SM)
                        .color(theme::active().subtext0),
                )
                .truncate(),
            );
        }
    }
}
