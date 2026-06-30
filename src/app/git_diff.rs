use super::diff_parser::{DiffLineKind, DiffViewMode};
use crate::git::parser::{parse_git_status, FileChangeKind};
use crate::theme;

pub(super) enum GitStageAction {
    Stage(String),
    StageAll,
    Unstage(String),
    UnstageAll,
}

pub(super) struct GitDiffResult {
    pub(super) stage_action: Option<GitStageAction>,
    pub(super) open_diff_file: Option<String>,
    pub(super) open_file: Option<String>,
    pub(super) open_conflict_file: Option<String>,
    pub(super) show_commit_dialog: bool,
    pub(super) show_push_dialog: bool,
    pub(super) show_stage_all_confirm: bool,
    pub(super) gitignore_pattern: Option<String>,
    pub(super) request_refresh: bool,
    pub(super) revert_file: Option<String>,
    pub(super) merge_abort: bool,
    pub(super) merge_continue: bool,
}

fn kind_to_tag(kind: FileChangeKind) -> &'static str {
    match kind {
        FileChangeKind::Modified => "M",
        FileChangeKind::Added => "A",
        FileChangeKind::Deleted => "D",
        FileChangeKind::Renamed => "R",
        FileChangeKind::Untracked => "?",
        FileChangeKind::Conflicted => "!",
    }
}

fn kind_to_color(kind: FileChangeKind) -> egui::Color32 {
    match kind {
        FileChangeKind::Modified => theme::active().git_modified,
        FileChangeKind::Added => theme::active().git_added,
        FileChangeKind::Deleted => theme::active().git_removed,
        FileChangeKind::Renamed => theme::active().git_renamed,
        FileChangeKind::Untracked => theme::active().git_untracked,
        FileChangeKind::Conflicted => theme::active().warning,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn render_git_diff(
    ui: &mut egui::Ui,
    status: &str,
    unpushed: &[(String, String)],
    push_in_progress: bool,
    push_error: Option<&str>,
    git_refreshing: bool,
    root_dir: Option<&std::path::Path>,
    drag_state: &mut super::drag::DragState,
    merge_operation: &super::git_worker::MergeOperation,
) -> GitDiffResult {
    let mut action: Option<GitStageAction> = None;
    let mut open_diff_file: Option<String> = None;
    let mut open_file: Option<String> = None;
    let mut open_conflict_file: Option<String> = None;
    let mut show_commit_dialog = false;
    let mut show_push_dialog = false;
    let show_stage_all_confirm = false;
    let mut gitignore_pattern: Option<String> = None;
    let mut request_refresh = false;
    let mut revert_file: Option<String> = None;
    let mut merge_abort = false;
    let mut merge_continue = false;

    let panel_width = ui.available_width();
    ui.set_min_width(0.0);
    ui.set_max_width(panel_width);

    let t = theme::active();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::SP_2);
            if git_refreshing {
                ui.add(egui::Spinner::new().size(14.0));
                ui.label(
                    egui::RichText::new("Refreshing\u{2026}")
                        .size(theme::FONT_UI_SM)
                        .color(t.subtext0),
                );
            } else {
                let btn = ui.add(
                    egui::Button::new(
                        egui::RichText::new("\u{21BB}")
                            .size(theme::FONT_UI_MD)
                            .color(t.text),
                    )
                    .frame(false),
                );
                if btn.clicked() {
                    request_refresh = true;
                }
                btn.on_hover_text("Refresh git status");
            }
        });
    });

    let has_status = !status.is_empty();
    let parsed = if has_status {
        parse_git_status(status)
    } else {
        Vec::new()
    };
    let has_conflicts = parsed.iter().any(|f| f.kind == FileChangeKind::Conflicted);

    // ── Merge/rebase/cherry-pick status banner ─────────────────────────
    if *merge_operation != super::git_worker::MergeOperation::None {
        let t = theme::active();
        let banner_bg = theme::blend_colors(t.surface0, t.warning, theme::BLEND_SUBTLE);
        egui::Frame::none()
            .fill(banner_bg)
            .rounding(theme::R_MD)
            .inner_margin(egui::Margin::symmetric(theme::SP_3, theme::SP_2))
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.horizontal(|ui| {
                    // Left: icon + label
                    let (icon, label) = match merge_operation {
                        super::git_worker::MergeOperation::Merge { source_branch } => {
                            ("\u{1F500}", format!("Merging branch '{source_branch}'"))
                        }
                        super::git_worker::MergeOperation::Rebase {
                            onto,
                            current_step,
                            total_steps,
                        } => (
                            "\u{1F504}",
                            format!("Rebasing '{onto}' \u{2014} step {current_step}/{total_steps}"),
                        ),
                        super::git_worker::MergeOperation::CherryPick { commit } => {
                            let short = if commit.len() >= 7 {
                                &commit[..7]
                            } else {
                                commit.as_str()
                            };
                            ("\u{1F352}", format!("Cherry-picking {short}"))
                        }
                        super::git_worker::MergeOperation::None => unreachable!(),
                    };
                    ui.label(
                        egui::RichText::new(format!("{icon} {label}"))
                            .size(theme::FONT_UI_MD)
                            .color(t.warning),
                    );

                    // Right: Continue + Abort buttons (right-to-left)
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let abort_btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new("Abort")
                                    .size(theme::FONT_UI_SM)
                                    .color(t.text),
                            )
                            .fill(t.error)
                            .rounding(theme::R_SM),
                        );
                        if abort_btn.clicked() {
                            merge_abort = true;
                        }

                        let continue_btn = ui.add_enabled(
                            !has_conflicts,
                            egui::Button::new(
                                egui::RichText::new("Continue")
                                    .size(theme::FONT_UI_SM)
                                    .color(if has_conflicts { t.overlay0 } else { t.text }),
                            )
                            .fill(if has_conflicts { t.surface1 } else { t.success })
                            .rounding(theme::R_SM),
                        );
                        if has_conflicts {
                            continue_btn.on_hover_text("Resolve all conflicts first");
                        } else if continue_btn.clicked() {
                            merge_continue = true;
                        }
                    });
                });
            });
        ui.add_space(theme::SP_2);
    }

    let has_unpushed = !unpushed.is_empty();

    if has_status || has_unpushed {
        struct StatusEntry {
            tag: &'static str,
            path: String,
            color: egui::Color32,
            kind: FileChangeKind,
        }

        let mut staged: Vec<StatusEntry> = Vec::new();
        let mut unstaged: Vec<StatusEntry> = Vec::new();
        let mut conflicted: Vec<StatusEntry> = Vec::new();

        for fs in &parsed {
            if fs.kind == FileChangeKind::Conflicted {
                conflicted.push(StatusEntry {
                    tag: kind_to_tag(fs.kind),
                    path: fs.path.clone(),
                    color: kind_to_color(fs.kind),
                    kind: fs.kind,
                });
                continue;
            }
            let entry = StatusEntry {
                tag: kind_to_tag(fs.kind),
                path: fs.path.clone(),
                color: kind_to_color(fs.kind),
                kind: fs.kind,
            };
            if fs.staged {
                staged.push(entry);
            } else {
                unstaged.push(entry);
            }
        }

        // ── Conflicts section ──────────────────────────────────────
        if !conflicted.is_empty() {
            let t = theme::active();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("Conflicts ({})", conflicted.len()))
                        .strong()
                        .size(theme::FONT_UI_MD)
                        .color(t.warning),
                );
            });
            ui.add_space(theme::SP_2);
            for entry in &conflicted {
                ui.horizontal(|ui| {
                    ui.set_max_width(panel_width);
                    let (badge_rect, _) =
                        ui.allocate_exact_size(egui::vec2(16.0, 14.0), egui::Sense::hover());
                    let badge_bg = entry.color.gamma_multiply(0.25);
                    ui.painter().rect_filled(badge_rect, theme::R_MD, badge_bg);
                    let badge_bg_rgb = [badge_bg.r(), badge_bg.g(), badge_bg.b()];
                    let badge_fg_rgb = [entry.color.r(), entry.color.g(), entry.color.b()];
                    let badge_fg = theme::ensure_readable(badge_fg_rgb, badge_bg_rgb);
                    ui.painter().text(
                        badge_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        entry.tag,
                        egui::FontId::monospace(theme::GIT_FONT_SZ),
                        badge_fg,
                    );
                    let label_max = (ui.available_width()).max(20.0);
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
                        })
                        .inner;
                    if label_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if label_resp.clicked() {
                        open_conflict_file = Some(entry.path.clone());
                    }
                });
            }
            ui.add_space(theme::SP_3);
            ui.separator();
            ui.add_space(theme::SP_2);
        }

        // ── Committed (unpushed) section ────────────────────────────
        if !unpushed.is_empty() || push_in_progress {
            let t = theme::active();
            ui.horizontal(|ui| {
                let header_text = if push_in_progress && unpushed.is_empty() {
                    "Committed".to_string()
                } else {
                    format!("Committed ({})", unpushed.len())
                };
                ui.label(
                    egui::RichText::new(header_text)
                        .strong()
                        .size(theme::FONT_UI_MD)
                        .color(t.blue),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(theme::SP_4);
                    if push_in_progress {
                        ui.add(egui::Spinner::new().size(14.0));
                        ui.label(
                            egui::RichText::new("Pushing\u{2026}")
                                .size(theme::FONT_UI_SM)
                                .color(t.subtext0),
                        );
                    } else if ui
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

        // ── Push error display ─────────────────────────────────────
        if let Some(err) = push_error {
            let t = theme::active();
            egui::Frame::none()
                .fill(t.error.gamma_multiply(0.15))
                .rounding(theme::R_MD)
                .inner_margin(egui::Margin::symmetric(theme::SP_3, theme::SP_2))
                .show(ui, |ui| {
                    ui.set_max_width(panel_width);
                    ui.label(
                        egui::RichText::new("Push failed")
                            .strong()
                            .size(theme::FONT_UI_SM)
                            .color(t.error),
                    );
                    let lines: Vec<&str> = err.lines().take(5).collect();
                    let display = lines.join("\n");
                    ui.label(
                        egui::RichText::new(display)
                            .monospace()
                            .size(theme::FONT_UI_XS)
                            .color(t.text),
                    );
                });
            ui.add_space(theme::SP_3);
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
                    ui.painter().rect_filled(badge_rect, theme::R_MD, badge_bg);
                    let badge_bg_rgb = [badge_bg.r(), badge_bg.g(), badge_bg.b()];
                    let badge_fg_rgb = [entry.color.r(), entry.color.g(), entry.color.b()];
                    let badge_fg = theme::ensure_readable(badge_fg_rgb, badge_bg_rgb);
                    ui.painter().text(
                        badge_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        entry.tag,
                        egui::FontId::monospace(theme::GIT_FONT_SZ),
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
                                .sense(egui::Sense::click_and_drag()),
                            )
                        })
                        .inner;
                    if label_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if label_resp.double_clicked() {
                        open_file = Some(entry.path.clone());
                    } else if label_resp.clicked() {
                        open_diff_file = Some(entry.path.clone());
                    }
                    if label_resp.drag_started() {
                        let origin = label_resp.interact_pointer_pos().unwrap_or_default();
                        drag_state.set_payload(
                            super::drag::DragPayload::Diff(entry.path.clone()),
                            origin,
                            &entry.path,
                        );
                    }
                    label_resp.context_menu(|ui| {
                        file_context_menu(
                            ui,
                            &entry.path,
                            true,
                            entry.kind,
                            root_dir,
                            &mut ContextMenuOutputs {
                                action: &mut action,
                                open_diff_file: &mut open_diff_file,
                                open_file: &mut open_file,
                                gitignore_pattern: &mut gitignore_pattern,
                                revert_file: &mut revert_file,
                            },
                        );
                    });
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
                        action = Some(GitStageAction::StageAll);
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
                    ui.painter().rect_filled(badge_rect, theme::R_MD, badge_bg);
                    let badge_bg_rgb = [badge_bg.r(), badge_bg.g(), badge_bg.b()];
                    let badge_fg_rgb = [entry.color.r(), entry.color.g(), entry.color.b()];
                    let badge_fg = theme::ensure_readable(badge_fg_rgb, badge_bg_rgb);
                    ui.painter().text(
                        badge_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        entry.tag,
                        egui::FontId::monospace(theme::GIT_FONT_SZ),
                        badge_fg,
                    );
                    let can_revert = entry.kind != FileChangeKind::Untracked;
                    let btn_reserve = theme::SP_4 + if can_revert { 40.0 } else { 20.0 };
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
                                .sense(egui::Sense::click_and_drag()),
                            )
                        })
                        .inner;
                    if label_resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if label_resp.double_clicked() {
                        open_file = Some(entry.path.clone());
                    } else if label_resp.clicked() {
                        open_diff_file = Some(entry.path.clone());
                    }
                    if label_resp.drag_started() {
                        let origin = label_resp.interact_pointer_pos().unwrap_or_default();
                        drag_state.set_payload(
                            super::drag::DragPayload::Diff(entry.path.clone()),
                            origin,
                            &entry.path,
                        );
                    }
                    label_resp.context_menu(|ui| {
                        file_context_menu(
                            ui,
                            &entry.path,
                            false,
                            entry.kind,
                            root_dir,
                            &mut ContextMenuOutputs {
                                action: &mut action,
                                open_diff_file: &mut open_diff_file,
                                open_file: &mut open_file,
                                gitignore_pattern: &mut gitignore_pattern,
                                revert_file: &mut revert_file,
                            },
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(theme::SP_4);
                        let stage_btn = ui.add(
                            egui::Button::new(
                                egui::RichText::new("+")
                                    .monospace()
                                    .size(theme::FONT_TERM)
                                    .color(theme::active().git_added),
                            )
                            .frame(false),
                        );
                        if stage_btn.on_hover_text("Stage").clicked() {
                            return Some(entry.path.clone());
                        }
                        if can_revert {
                            let revert_btn = ui.add(
                                egui::Button::new(
                                    egui::RichText::new("\u{21BA}")
                                        .monospace()
                                        .size(theme::FONT_TERM)
                                        .color(theme::active().error),
                                )
                                .frame(false),
                            );
                            if revert_btn.on_hover_text("Revert changes").clicked() {
                                revert_file = Some(entry.path.clone());
                            }
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

    if status.is_empty() && unpushed.is_empty() && !push_in_progress && push_error.is_none() {
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
        open_conflict_file,
        show_commit_dialog,
        show_push_dialog,
        show_stage_all_confirm,
        gitignore_pattern,
        request_refresh,
        revert_file,
        merge_abort,
        merge_continue,
    }
}

struct ContextMenuOutputs<'a> {
    action: &'a mut Option<GitStageAction>,
    open_diff_file: &'a mut Option<String>,
    open_file: &'a mut Option<String>,
    gitignore_pattern: &'a mut Option<String>,
    revert_file: &'a mut Option<String>,
}

fn file_context_menu(
    ui: &mut egui::Ui,
    path: &str,
    is_staged: bool,
    kind: FileChangeKind,
    root_dir: Option<&std::path::Path>,
    out: &mut ContextMenuOutputs,
) {
    if kind != FileChangeKind::Untracked && ui.button("View diff").clicked() {
        *out.open_diff_file = Some(path.to_string());
        ui.close_menu();
    }
    if ui.button("Open file").clicked() {
        *out.open_file = Some(path.to_string());
        ui.close_menu();
    }
    if ui.button("Copy path").clicked() {
        let copy_text = match root_dir {
            Some(root) => root.join(path).display().to_string(),
            None => path.to_string(),
        };
        std::thread::spawn(move || {
            if let Ok(mut clip) = arboard::Clipboard::new() {
                let _ = clip.set_text(copy_text);
            }
        });
        ui.close_menu();
    }
    if ui.button("Reveal in file manager").clicked() {
        let full = match root_dir {
            Some(root) => root.join(path),
            None => std::path::PathBuf::from(path),
        };
        crate::util::reveal_in_file_manager(&full);
        ui.close_menu();
    }
    ui.separator();
    if is_staged {
        if ui.button("Unstage").clicked() {
            *out.action = Some(GitStageAction::Unstage(path.to_string()));
            ui.close_menu();
        }
    } else if ui.button("Stage").clicked() {
        *out.action = Some(GitStageAction::Stage(path.to_string()));
        ui.close_menu();
    }
    if kind != FileChangeKind::Untracked {
        ui.separator();
        let revert_label = egui::RichText::new("Revert changes").color(theme::active().error);
        if ui.button(revert_label).clicked() {
            *out.revert_file = Some(path.to_string());
            ui.close_menu();
        }
    }
    ui.separator();
    if ui.button("Add to .gitignore").clicked() {
        *out.gitignore_pattern = Some(path.to_string());
        ui.close_menu();
    }
}

pub(super) struct DiffToolbarResult {
    pub(super) mode_change: Option<DiffViewMode>,
    pub(super) scroll_to_hunk: Option<usize>,
}

pub(super) fn render_diff_toolbar(
    ui: &mut egui::Ui,
    current_mode: DiffViewMode,
    hunk_count: usize,
    current_hunk: usize,
) -> DiffToolbarResult {
    let mut mode_change = None;
    let mut scroll_to_hunk = None;
    let t = theme::active();

    ui.horizontal(|ui| {
        ui.add_space(theme::SP_2);

        let modes = [
            (DiffViewMode::Inline, "Inline"),
            (DiffViewMode::SideBySide, "Side by Side"),
        ];

        for (mode, label) in &modes {
            let is_active = current_mode == *mode;
            let (bg, fg) = if is_active {
                (t.accent, t.text)
            } else {
                (t.surface1, t.subtext0)
            };
            let btn = ui.add(
                egui::Button::new(
                    egui::RichText::new(*label)
                        .size(theme::FONT_UI_SM)
                        .color(fg),
                )
                .fill(bg)
                .rounding(theme::R_SM),
            );
            if btn.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if btn.clicked() && !is_active {
                mode_change = Some(*mode);
            }
        }

        if hunk_count > 0 {
            ui.add_space(theme::SP_4);
            ui.add(egui::Separator::default().vertical().spacing(theme::SP_2));
            ui.add_space(theme::SP_2);

            let has_prev = current_hunk > 0;
            let has_next = current_hunk + 1 < hunk_count;

            let prev_btn = ui.add_enabled(
                has_prev,
                egui::Button::new(
                    egui::RichText::new("\u{2191}")
                        .size(theme::FONT_UI_MD)
                        .color(if has_prev { t.text } else { t.overlay0 }),
                )
                .frame(false),
            );
            if prev_btn.hovered() && has_prev {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if prev_btn.on_hover_text("Previous change").clicked() && has_prev {
                scroll_to_hunk = Some(current_hunk - 1);
            }

            ui.label(
                egui::RichText::new(format!("{}/{}", current_hunk + 1, hunk_count))
                    .size(theme::FONT_UI_SM)
                    .color(t.subtext0),
            );

            let next_btn = ui.add_enabled(
                has_next,
                egui::Button::new(
                    egui::RichText::new("\u{2193}")
                        .size(theme::FONT_UI_MD)
                        .color(if has_next { t.text } else { t.overlay0 }),
                )
                .frame(false),
            );
            if next_btn.hovered() && has_next {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if next_btn.on_hover_text("Next change").clicked() && has_next {
                scroll_to_hunk = Some(current_hunk + 1);
            }
        }
    });

    DiffToolbarResult {
        mode_change,
        scroll_to_hunk,
    }
}

fn diff_bg_rgb(kind: DiffLineKind, t: &theme::Theme) -> [u8; 3] {
    match kind {
        DiffLineKind::Added => {
            let bg = t.git_added.gamma_multiply(0.12);
            [bg.r(), bg.g(), bg.b()]
        }
        DiffLineKind::Removed => {
            let bg = t.git_removed.gamma_multiply(0.12);
            [bg.r(), bg.g(), bg.b()]
        }
        DiffLineKind::Context => [t.bg_term.r(), t.bg_term.g(), t.bg_term.b()],
    }
}

fn paint_highlighted_line(
    ui: &egui::Ui,
    rect: egui::Rect,
    content: &str,
    segments: Option<&[(egui::Color32, String)]>,
    kind: DiffLineKind,
    t: &theme::Theme,
    font: &egui::FontId,
) {
    let bg_rgb = diff_bg_rgb(kind, t);

    if let Some(segs) = segments {
        let mut job = egui::text::LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        for (raw_color, text) in segs {
            let fg = theme::ensure_readable([raw_color.r(), raw_color.g(), raw_color.b()], bg_rgb);
            job.append(text, 0.0, egui::TextFormat::simple(font.clone(), fg));
        }
        let galley = ui.fonts(|f| f.layout_job(job));
        let y = rect.center().y - galley.size().y / 2.0;
        ui.painter()
            .galley(egui::pos2(rect.min.x + 4.0, y), galley, t.text);
    } else {
        let fg = match kind {
            DiffLineKind::Added => t.git_added,
            DiffLineKind::Removed => t.git_removed,
            DiffLineKind::Context => t.text,
        };
        let fg = theme::ensure_readable([fg.r(), fg.g(), fg.b()], bg_rgb);
        ui.painter().text(
            egui::pos2(rect.min.x + 4.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            content,
            font.clone(),
            fg,
        );
    }
}

fn compute_hunk_start_indices(
    lines: &[super::diff_parser::DiffLine],
    hunks: &[super::diff_parser::DiffHunk],
) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut hunk_idx = 0;
    for (i, line) in lines.iter().enumerate() {
        if hunk_idx >= hunks.len() {
            break;
        }
        let Some(first) = hunks[hunk_idx].lines.first() else {
            hunk_idx += 1;
            continue;
        };
        if line.kind == first.kind
            && line.old_lineno == first.old_lineno
            && line.new_lineno == first.new_lineno
        {
            starts.push(i);
            hunk_idx += 1;
        }
    }
    starts
}

pub(super) fn render_inline_diff_full(
    ui: &mut egui::Ui,
    old_content: &str,
    new_content: &str,
    hunks: &[super::diff_parser::DiffHunk],
    old_highlights: Option<&[Vec<(egui::Color32, String)>]>,
    new_highlights: Option<&[Vec<(egui::Color32, String)>]>,
    scroll_to_hunk: Option<usize>,
) {
    use super::diff_parser::build_full_diff_lines;

    let lines = build_full_diff_lines(old_content, new_content, hunks);
    let t = theme::active();
    let gutter_w = 36.0_f32;
    let indicator_w = 16.0_f32;
    let font_mono = egui::FontId::monospace(theme::FONT_UI_SM);
    let gutter_font = egui::FontId::monospace(theme::FONT_UI_XS);
    let line_h = ui.text_style_height(&egui::TextStyle::Monospace);

    let hunk_starts = compute_hunk_start_indices(&lines, hunks);
    let scroll_target_line = scroll_to_hunk.and_then(|idx| hunk_starts.get(idx).copied());

    let line_count = lines.len();
    let spacing = ui.spacing().item_spacing.y;
    let row_h = line_h + spacing;
    let content_top = ui.cursor().top();
    let clip = ui.clip_rect();
    let buffer = 5_usize;
    let first_visible = ((clip.min.y - content_top) / row_h).floor().max(0.0) as usize;
    let first_visible = first_visible.saturating_sub(buffer);
    let last_visible = ((clip.max.y - content_top) / row_h).ceil().max(0.0) as usize;
    let last_visible = (last_visible + buffer).min(line_count);

    if first_visible > 0 {
        ui.add_space(first_visible as f32 * row_h);
    }

    for line in lines.iter().take(last_visible).skip(first_visible) {
        let bg_color = match line.kind {
            DiffLineKind::Added => Some(t.git_added.gamma_multiply(0.12)),
            DiffLineKind::Removed => Some(t.git_removed.gamma_multiply(0.12)),
            DiffLineKind::Context => None,
        };

        ui.horizontal(|ui| {
            let old_text = line.old_lineno.map(|n| format!("{n}")).unwrap_or_default();
            let (old_rect, _) =
                ui.allocate_exact_size(egui::vec2(gutter_w, line_h), egui::Sense::hover());
            ui.painter().text(
                egui::pos2(old_rect.max.x - 4.0, old_rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &old_text,
                gutter_font.clone(),
                t.overlay0,
            );

            let new_text = line.new_lineno.map(|n| format!("{n}")).unwrap_or_default();
            let (new_rect, _) =
                ui.allocate_exact_size(egui::vec2(gutter_w, line_h), egui::Sense::hover());
            ui.painter().text(
                egui::pos2(new_rect.max.x - 4.0, new_rect.center().y),
                egui::Align2::RIGHT_CENTER,
                &new_text,
                gutter_font.clone(),
                t.overlay0,
            );

            let (ind_rect, _) =
                ui.allocate_exact_size(egui::vec2(indicator_w, line_h), egui::Sense::hover());
            let (ind_char, ind_color) = match line.kind {
                DiffLineKind::Added => ("+", t.git_added),
                DiffLineKind::Removed => ("\u{2212}", t.git_removed),
                DiffLineKind::Context => ("", t.overlay0),
            };
            if !ind_char.is_empty() {
                if let Some(bg) = bg_color {
                    ui.painter().rect_filled(ind_rect, 0.0, bg);
                }
                ui.painter().text(
                    ind_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    ind_char,
                    font_mono.clone(),
                    ind_color,
                );
            }

            let avail = ui.available_width();
            let (content_rect, _) =
                ui.allocate_exact_size(egui::vec2(avail.max(200.0), line_h), egui::Sense::hover());
            if let Some(bg) = bg_color {
                ui.painter().rect_filled(content_rect, 0.0, bg);
            }

            let segments = match line.kind {
                DiffLineKind::Removed => line
                    .old_lineno
                    .and_then(|n| old_highlights.and_then(|h| h.get(n - 1))),
                _ => line
                    .new_lineno
                    .and_then(|n| new_highlights.and_then(|h| h.get(n - 1))),
            };

            paint_highlighted_line(
                ui,
                content_rect,
                &line.content,
                segments.map(|v| v.as_slice()),
                line.kind,
                t,
                &font_mono,
            );
        });
    }

    let remaining = line_count.saturating_sub(last_visible);
    if remaining > 0 {
        ui.add_space(remaining as f32 * row_h);
    }

    if let Some(target_idx) = scroll_target_line {
        let target_y = content_top + target_idx as f32 * row_h;
        let target_rect =
            egui::Rect::from_min_size(egui::pos2(0.0, target_y), egui::vec2(1.0, row_h));
        ui.scroll_to_rect(target_rect, Some(egui::Align::Center));
    }
}

fn compute_sbs_hunk_start_indices(
    left: &[super::diff_parser::SideBySideLine],
    right: &[super::diff_parser::SideBySideLine],
    hunks: &[super::diff_parser::DiffHunk],
) -> Vec<usize> {
    let mut starts = Vec::new();
    for hunk in hunks {
        let Some(first) = hunk.lines.first() else {
            continue;
        };
        let pos = match first.kind {
            DiffLineKind::Removed => left
                .iter()
                .position(|l| l.lineno == first.old_lineno && l.kind == DiffLineKind::Removed),
            DiffLineKind::Added => right
                .iter()
                .position(|r| r.lineno == first.new_lineno && r.kind == DiffLineKind::Added),
            DiffLineKind::Context => left
                .iter()
                .position(|l| l.lineno == first.old_lineno && l.kind == DiffLineKind::Context),
        };
        if let Some(idx) = pos {
            starts.push(idx);
        }
    }
    starts
}

pub(super) fn render_side_by_side_diff(
    ui: &mut egui::Ui,
    old_content: &str,
    new_content: &str,
    hunks: &[super::diff_parser::DiffHunk],
    old_highlights: Option<&[Vec<(egui::Color32, String)>]>,
    new_highlights: Option<&[Vec<(egui::Color32, String)>]>,
    scroll_to_hunk: Option<usize>,
) {
    use super::diff_parser::{build_full_diff_lines, build_side_by_side_lines};

    let full = build_full_diff_lines(old_content, new_content, hunks);
    let (left_lines, right_lines) = build_side_by_side_lines(&full);

    let t = theme::active();
    let total_w = ui.available_width();
    let half_w = (total_w / 2.0 - 1.0).max(100.0);
    let gutter_w = 36.0_f32;
    let indicator_w = 16.0_f32;
    let font_mono = egui::FontId::monospace(theme::FONT_UI_SM);
    let gutter_font = egui::FontId::monospace(theme::FONT_UI_XS);
    let line_h = ui.text_style_height(&egui::TextStyle::Monospace);

    ui.horizontal(|ui| {
        let (left_header, _) =
            ui.allocate_exact_size(egui::vec2(half_w, line_h + 4.0), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(left_header.min.x + theme::SP_2, left_header.center().y),
            egui::Align2::LEFT_CENTER,
            "HEAD",
            egui::FontId::proportional(theme::FONT_UI_SM),
            t.subtext0,
        );
        let (sep_rect, _) =
            ui.allocate_exact_size(egui::vec2(2.0, line_h + 4.0), egui::Sense::hover());
        ui.painter().rect_filled(sep_rect, 0.0, t.surface2);
        let (right_header, _) =
            ui.allocate_exact_size(egui::vec2(half_w, line_h + 4.0), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(right_header.min.x + theme::SP_2, right_header.center().y),
            egui::Align2::LEFT_CENTER,
            "Working Tree",
            egui::FontId::proportional(theme::FONT_UI_SM),
            t.subtext0,
        );
    });
    ui.separator();

    let lay = SbsLayout {
        panel_w: half_w,
        gutter_w,
        indicator_w,
        line_h,
        font_mono: &font_mono,
        gutter_font: &gutter_font,
    };

    let sbs_hunk_starts = compute_sbs_hunk_start_indices(&left_lines, &right_lines, hunks);
    let scroll_target_line = scroll_to_hunk.and_then(|idx| sbs_hunk_starts.get(idx).copied());

    let line_count = left_lines.len();
    let spacing = ui.spacing().item_spacing.y;
    let row_h = line_h + spacing;
    let content_top = ui.cursor().top();
    let clip = ui.clip_rect();
    let buffer = 5_usize;
    let first_visible = ((clip.min.y - content_top) / row_h).floor().max(0.0) as usize;
    let first_visible = first_visible.saturating_sub(buffer);
    let last_visible = ((clip.max.y - content_top) / row_h).ceil().max(0.0) as usize;
    let last_visible = (last_visible + buffer).min(line_count);

    if first_visible > 0 {
        ui.add_space(first_visible as f32 * row_h);
    }

    for i in first_visible..last_visible {
        let left = &left_lines[i];
        let right = &right_lines[i];

        let left_segs = left
            .lineno
            .and_then(|n| old_highlights.and_then(|h| h.get(n - 1)));
        let right_segs = right
            .lineno
            .and_then(|n| new_highlights.and_then(|h| h.get(n - 1)));

        ui.horizontal(|ui| {
            render_sbs_cell(ui, left, &lay, t, left_segs.map(|v| v.as_slice()));

            let (sep_rect, _) =
                ui.allocate_exact_size(egui::vec2(2.0, line_h), egui::Sense::hover());
            ui.painter().rect_filled(sep_rect, 0.0, t.surface2);

            render_sbs_cell(ui, right, &lay, t, right_segs.map(|v| v.as_slice()));
        });
    }

    let remaining = line_count.saturating_sub(last_visible);
    if remaining > 0 {
        ui.add_space(remaining as f32 * row_h);
    }

    if let Some(target_idx) = scroll_target_line {
        let target_y = content_top + target_idx as f32 * row_h;
        let target_rect =
            egui::Rect::from_min_size(egui::pos2(0.0, target_y), egui::vec2(1.0, row_h));
        ui.scroll_to_rect(target_rect, Some(egui::Align::Center));
    }
}

struct SbsLayout<'a> {
    panel_w: f32,
    gutter_w: f32,
    indicator_w: f32,
    line_h: f32,
    font_mono: &'a egui::FontId,
    gutter_font: &'a egui::FontId,
}

fn render_sbs_cell(
    ui: &mut egui::Ui,
    line: &super::diff_parser::SideBySideLine,
    lay: &SbsLayout,
    t: &theme::Theme,
    segments: Option<&[(egui::Color32, String)]>,
) {
    let bg_color = match line.kind {
        DiffLineKind::Added => Some(t.git_added.gamma_multiply(0.12)),
        DiffLineKind::Removed => Some(t.git_removed.gamma_multiply(0.12)),
        DiffLineKind::Context => {
            if line.content.is_none() {
                Some(t.surface0)
            } else {
                None
            }
        }
    };

    let num_text = line.lineno.map(|n| format!("{n}")).unwrap_or_default();
    let (gutter_rect, _) =
        ui.allocate_exact_size(egui::vec2(lay.gutter_w, lay.line_h), egui::Sense::hover());
    ui.painter().text(
        egui::pos2(gutter_rect.max.x - 4.0, gutter_rect.center().y),
        egui::Align2::RIGHT_CENTER,
        &num_text,
        lay.gutter_font.clone(),
        t.overlay0,
    );

    let (ind_rect, _) = ui.allocate_exact_size(
        egui::vec2(lay.indicator_w, lay.line_h),
        egui::Sense::hover(),
    );
    let (ind_char, ind_color) = match line.kind {
        DiffLineKind::Added => ("+", t.git_added),
        DiffLineKind::Removed => ("\u{2212}", t.git_removed),
        DiffLineKind::Context => ("", t.overlay0),
    };
    if !ind_char.is_empty() {
        if let Some(bg) = bg_color {
            ui.painter().rect_filled(ind_rect, 0.0, bg);
        }
        ui.painter().text(
            ind_rect.center(),
            egui::Align2::CENTER_CENTER,
            ind_char,
            lay.font_mono.clone(),
            ind_color,
        );
    }

    let content_w = (lay.panel_w - lay.gutter_w - lay.indicator_w).max(50.0);
    let (content_rect, _) =
        ui.allocate_exact_size(egui::vec2(content_w, lay.line_h), egui::Sense::hover());
    if let Some(bg) = bg_color {
        ui.painter().rect_filled(content_rect, 0.0, bg);
    }
    let display = line.content.as_deref().unwrap_or("");

    if line.content.is_some() {
        paint_highlighted_line(
            ui,
            content_rect,
            display,
            segments,
            line.kind,
            t,
            lay.font_mono,
        );
    }
}
