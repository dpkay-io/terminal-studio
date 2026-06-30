use crate::app::conflict_parser::{ConflictBlock, Resolution};
use crate::app::pane::ConflictResolverState;
use crate::theme;

pub(in crate::app) enum ConflictAction {
    Single {
        conflict_index: usize,
        resolution: Resolution,
    },
    AllOurs,
    AllTheirs,
}

/// Return value from `render_conflict_resolver` so callers can apply deferred state changes.
pub(in crate::app) struct ConflictResolverResult {
    /// A conflict block was resolved.
    pub action: Option<ConflictAction>,
    /// The user toggled the view mode; apply this new value to the pane state.
    pub view_toggle: Option<bool>,
}

pub(in crate::app) fn render_conflict_resolver(
    ui: &mut egui::Ui,
    state: &ConflictResolverState,
) -> ConflictResolverResult {
    let pane_rect = ui.max_rect();
    let t = theme::active();
    ui.painter().rect_filled(pane_rect, 0.0, t.bg_term);

    let mut action: Option<ConflictAction> = None;
    let mut view_toggle: Option<bool> = None;

    // ── Toolbar ──
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("\u{26A0} {}", state.path.display()))
                .strong()
                .size(theme::FONT_UI_LG)
                .color(t.warning),
        );

        // View mode toggle (inline vs side-by-side)
        ui.add_space(theme::SP_4);
        let modes = [(false, "Inline"), (true, "Side by Side")];
        for (is_sbs, label) in &modes {
            let active = state.side_by_side == *is_sbs;
            let (bg, fg) = if active {
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
            if btn.clicked() && !active {
                view_toggle = Some(*is_sbs);
            }
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(theme::SP_4);
            if state.resolved_count < state.content.total_conflicts {
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("All Theirs")
                                .size(theme::FONT_UI_SM)
                                .color(t.error),
                        )
                        .rounding(theme::R_SM),
                    )
                    .clicked()
                {
                    action = Some(ConflictAction::AllTheirs);
                }
                ui.add_space(theme::SP_2);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("All Ours")
                                .size(theme::FONT_UI_SM)
                                .color(t.success),
                        )
                        .rounding(theme::R_SM),
                    )
                    .clicked()
                {
                    action = Some(ConflictAction::AllOurs);
                }
                ui.add_space(theme::SP_4);
            }
            ui.label(
                egui::RichText::new(format!(
                    "{}/{} resolved",
                    state.resolved_count, state.content.total_conflicts
                ))
                .size(theme::FONT_UI_SM)
                .color(if state.resolved_count == state.content.total_conflicts {
                    t.success
                } else {
                    t.subtext0
                }),
            );
        });
    });
    ui.separator();

    let side_by_side = state.side_by_side;
    let total_conflicts = state.content.total_conflicts;

    // ── Scrollable content ──
    egui::ScrollArea::both()
        .id_source(("conflict_scroll", state.path.display().to_string()))
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let mut line_number: usize = 1;
            for block in &state.content.blocks {
                match block {
                    ConflictBlock::Context { lines } => {
                        for line in lines {
                            render_context_line(ui, line_number, line, t);
                            line_number += 1;
                        }
                    }
                    ConflictBlock::Conflict {
                        index,
                        ours_lines,
                        theirs_lines,
                        ours_label,
                        theirs_label,
                        resolved,
                    } => {
                        if let Some(resolution) = resolved {
                            if side_by_side {
                                render_resolved_block_sbs(
                                    ui,
                                    &mut line_number,
                                    ours_lines,
                                    theirs_lines,
                                    resolution,
                                    t,
                                );
                            } else {
                                render_resolved_block(
                                    ui,
                                    &mut line_number,
                                    ours_lines,
                                    theirs_lines,
                                    resolution,
                                    t,
                                );
                            }
                        } else if side_by_side {
                            let block_action = render_conflict_block_sbs(
                                ui,
                                *index,
                                ours_lines,
                                theirs_lines,
                                ours_label,
                                theirs_label,
                                total_conflicts,
                                t,
                            );
                            if action.is_none() {
                                action = block_action;
                            }
                        } else {
                            let block_action = render_conflict_block(
                                ui,
                                *index,
                                ours_lines,
                                theirs_lines,
                                ours_label,
                                theirs_label,
                                total_conflicts,
                                t,
                            );
                            if action.is_none() {
                                action = block_action;
                            }
                        }
                    }
                }
            }
        });

    ConflictResolverResult {
        action,
        view_toggle,
    }
}

fn render_context_line(ui: &mut egui::Ui, line_number: usize, content: &str, t: &theme::Theme) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{:>4} ", line_number))
                .monospace()
                .size(theme::FONT_TERM)
                .color(t.overlay0),
        );
        ui.label(
            egui::RichText::new(content)
                .monospace()
                .size(theme::FONT_TERM)
                .color(t.text),
        );
    });
}

fn render_resolved_block(
    ui: &mut egui::Ui,
    line_number: &mut usize,
    ours_lines: &[String],
    theirs_lines: &[String],
    resolution: &Resolution,
    t: &theme::Theme,
) {
    let label = match resolution {
        Resolution::Ours => "Resolved: Ours",
        Resolution::Theirs => "Resolved: Theirs",
        Resolution::Both => "Resolved: Both",
    };
    let resolved_bg = theme::blend_colors(t.surface0, t.overlay0, 0.08);

    egui::Frame::none()
        .fill(resolved_bg)
        .inner_margin(egui::Margin::symmetric(0.0, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(theme::SP_2);
                ui.label(
                    egui::RichText::new(label)
                        .size(theme::FONT_UI_XS)
                        .color(t.overlay0)
                        .italics(),
                );
            });
            let primary_lines = match resolution {
                Resolution::Ours => ours_lines,
                Resolution::Theirs => theirs_lines,
                Resolution::Both => ours_lines,
            };
            for line in primary_lines {
                render_context_line(ui, *line_number, line, t);
                *line_number += 1;
            }
            if matches!(resolution, Resolution::Both) {
                for line in theirs_lines {
                    render_context_line(ui, *line_number, line, t);
                    *line_number += 1;
                }
            }
        });
}

fn render_resolved_block_sbs(
    ui: &mut egui::Ui,
    line_number: &mut usize,
    ours_lines: &[String],
    theirs_lines: &[String],
    resolution: &Resolution,
    t: &theme::Theme,
) {
    let label = match resolution {
        Resolution::Ours => "Resolved: Ours",
        Resolution::Theirs => "Resolved: Theirs",
        Resolution::Both => "Resolved: Both",
    };
    let resolved_bg = theme::blend_colors(t.surface0, t.overlay0, 0.08);

    egui::Frame::none()
        .fill(resolved_bg)
        .inner_margin(egui::Margin::symmetric(0.0, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(theme::SP_2);
                ui.label(
                    egui::RichText::new(label)
                        .size(theme::FONT_UI_XS)
                        .color(t.overlay0)
                        .italics(),
                );
            });

            let col_width = (ui.available_width() - 2.0) / 2.0;
            let max_lines = ours_lines.len().max(theirs_lines.len());

            // Advance line_number by lines produced by the chosen resolution
            let ours_count = match resolution {
                Resolution::Ours | Resolution::Both => ours_lines.len(),
                Resolution::Theirs => 0,
            };
            let theirs_count = match resolution {
                Resolution::Theirs | Resolution::Both => theirs_lines.len(),
                Resolution::Ours => 0,
            };

            ui.horizontal(|ui| {
                // Left column (ours)
                ui.vertical(|ui| {
                    ui.set_width(col_width);
                    let ours_alpha: f32 = match resolution {
                        Resolution::Ours | Resolution::Both => 1.0,
                        Resolution::Theirs => 0.3,
                    };
                    for i in 0..max_lines {
                        if let Some(line) = ours_lines.get(i) {
                            let color = t.success.gamma_multiply(ours_alpha);
                            ui.label(
                                egui::RichText::new(line)
                                    .monospace()
                                    .size(theme::FONT_TERM)
                                    .color(color),
                            );
                        } else {
                            ui.label(egui::RichText::new(" ").monospace().size(theme::FONT_TERM));
                        }
                    }
                });

                // 2px separator
                let sep_height = ui.available_height().max(1.0);
                let (sep_rect, _) =
                    ui.allocate_exact_size(egui::vec2(2.0, sep_height), egui::Sense::hover());
                ui.painter().rect_filled(sep_rect, 0.0, t.overlay0);

                // Right column (theirs)
                ui.vertical(|ui| {
                    ui.set_width(col_width);
                    let theirs_alpha: f32 = match resolution {
                        Resolution::Theirs | Resolution::Both => 1.0,
                        Resolution::Ours => 0.3,
                    };
                    for i in 0..max_lines {
                        if let Some(line) = theirs_lines.get(i) {
                            let color = t.error.gamma_multiply(theirs_alpha);
                            ui.label(
                                egui::RichText::new(line)
                                    .monospace()
                                    .size(theme::FONT_TERM)
                                    .color(color),
                            );
                        } else {
                            ui.label(egui::RichText::new(" ").monospace().size(theme::FONT_TERM));
                        }
                    }
                });
            });

            *line_number += ours_count + theirs_count;
        });
}

#[allow(clippy::too_many_arguments)]
fn render_conflict_block_sbs(
    ui: &mut egui::Ui,
    conflict_index: usize,
    ours_lines: &[String],
    theirs_lines: &[String],
    ours_label: &str,
    theirs_label: &str,
    total_conflicts: usize,
    t: &theme::Theme,
) -> Option<ConflictAction> {
    let mut action: Option<ConflictAction> = None;
    let ours_bg = theme::blend_colors(t.surface0, t.success, theme::BLEND_SUBTLE);
    let theirs_bg = theme::blend_colors(t.surface0, t.error, theme::BLEND_SUBTLE);

    // Action bar
    egui::Frame::none()
        .fill(t.surface1)
        .rounding(egui::Rounding {
            nw: theme::R_SM,
            ne: theme::R_SM,
            sw: 0.0,
            se: 0.0,
        })
        .inner_margin(egui::Margin::symmetric(theme::SP_3, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "CONFLICT {}/{}",
                        conflict_index + 1,
                        total_conflicts
                    ))
                    .strong()
                    .size(theme::FONT_UI_XS)
                    .color(t.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Both")
                                    .size(theme::FONT_UI_XS)
                                    .color(t.base),
                            )
                            .fill(t.accent)
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        action = Some(ConflictAction::Single {
                            conflict_index,
                            resolution: Resolution::Both,
                        });
                    }
                    ui.add_space(theme::SP_1);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Theirs")
                                    .size(theme::FONT_UI_XS)
                                    .color(t.base),
                            )
                            .fill(t.error)
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        action = Some(ConflictAction::Single {
                            conflict_index,
                            resolution: Resolution::Theirs,
                        });
                    }
                    ui.add_space(theme::SP_1);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Ours")
                                    .size(theme::FONT_UI_XS)
                                    .color(t.base),
                            )
                            .fill(t.success)
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        action = Some(ConflictAction::Single {
                            conflict_index,
                            resolution: Resolution::Ours,
                        });
                    }
                });
            });
        });

    // Side-by-side columns
    let col_width = (ui.available_width() - 2.0) / 2.0;
    let max_lines = ours_lines.len().max(theirs_lines.len());

    ui.horizontal(|ui| {
        // Left column — OURS
        egui::Frame::none()
            .fill(ours_bg)
            .inner_margin(egui::Margin::symmetric(0.0, theme::SP_1))
            .show(ui, |ui| {
                ui.set_width(col_width);
                ui.horizontal(|ui| {
                    let (border_rect, _) = ui.allocate_exact_size(
                        egui::vec2(3.0, ui.spacing().interact_size.y),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(border_rect, 0.0, t.success);
                    ui.add_space(theme::SP_1);
                    ui.label(
                        egui::RichText::new(format!("\u{25C0} OURS ({})", ours_label))
                            .size(theme::FONT_UI_XS)
                            .strong()
                            .color(t.success),
                    );
                });
                for i in 0..max_lines {
                    ui.horizontal(|ui| {
                        let (border_rect, _) = ui.allocate_exact_size(
                            egui::vec2(3.0, ui.spacing().interact_size.y),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(border_rect, 0.0, t.success);
                        ui.add_space(theme::SP_1);
                        if let Some(line) = ours_lines.get(i) {
                            ui.label(
                                egui::RichText::new(line)
                                    .monospace()
                                    .size(theme::FONT_TERM)
                                    .color(t.success),
                            );
                        } else {
                            ui.label(egui::RichText::new(" ").monospace().size(theme::FONT_TERM));
                        }
                    });
                }
            });

        // 2px separator
        let sep_height = ui.available_height().max(1.0);
        let (sep_rect, _) =
            ui.allocate_exact_size(egui::vec2(2.0, sep_height), egui::Sense::hover());
        ui.painter().rect_filled(sep_rect, 0.0, t.overlay0);

        // Right column — THEIRS
        egui::Frame::none()
            .fill(theirs_bg)
            .inner_margin(egui::Margin::symmetric(0.0, theme::SP_1))
            .show(ui, |ui| {
                ui.set_width(col_width);
                ui.horizontal(|ui| {
                    let (border_rect, _) = ui.allocate_exact_size(
                        egui::vec2(3.0, ui.spacing().interact_size.y),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(border_rect, 0.0, t.error);
                    ui.add_space(theme::SP_1);
                    ui.label(
                        egui::RichText::new(format!("\u{25B6} THEIRS ({})", theirs_label))
                            .size(theme::FONT_UI_XS)
                            .strong()
                            .color(t.error),
                    );
                });
                for i in 0..max_lines {
                    ui.horizontal(|ui| {
                        let (border_rect, _) = ui.allocate_exact_size(
                            egui::vec2(3.0, ui.spacing().interact_size.y),
                            egui::Sense::hover(),
                        );
                        ui.painter().rect_filled(border_rect, 0.0, t.error);
                        ui.add_space(theme::SP_1);
                        if let Some(line) = theirs_lines.get(i) {
                            ui.label(
                                egui::RichText::new(line)
                                    .monospace()
                                    .size(theme::FONT_TERM)
                                    .color(t.error),
                            );
                        } else {
                            ui.label(egui::RichText::new(" ").monospace().size(theme::FONT_TERM));
                        }
                    });
                }
            });
    });

    action
}

#[allow(clippy::too_many_arguments)]
fn render_conflict_block(
    ui: &mut egui::Ui,
    conflict_index: usize,
    ours_lines: &[String],
    theirs_lines: &[String],
    ours_label: &str,
    theirs_label: &str,
    total_conflicts: usize,
    t: &theme::Theme,
) -> Option<ConflictAction> {
    let mut action: Option<ConflictAction> = None;
    let ours_bg = theme::blend_colors(t.surface0, t.success, theme::BLEND_SUBTLE);
    let theirs_bg = theme::blend_colors(t.surface0, t.error, theme::BLEND_SUBTLE);

    // Floating action bar
    egui::Frame::none()
        .fill(t.surface1)
        .rounding(egui::Rounding {
            nw: theme::R_SM,
            ne: theme::R_SM,
            sw: 0.0,
            se: 0.0,
        })
        .inner_margin(egui::Margin::symmetric(theme::SP_3, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "CONFLICT {}/{}",
                        conflict_index + 1,
                        total_conflicts
                    ))
                    .strong()
                    .size(theme::FONT_UI_XS)
                    .color(t.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Both")
                                    .size(theme::FONT_UI_XS)
                                    .color(t.base),
                            )
                            .fill(t.accent)
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        action = Some(ConflictAction::Single {
                            conflict_index,
                            resolution: Resolution::Both,
                        });
                    }
                    ui.add_space(theme::SP_1);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Theirs")
                                    .size(theme::FONT_UI_XS)
                                    .color(t.base),
                            )
                            .fill(t.error)
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        action = Some(ConflictAction::Single {
                            conflict_index,
                            resolution: Resolution::Theirs,
                        });
                    }
                    ui.add_space(theme::SP_1);
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new("Ours")
                                    .size(theme::FONT_UI_XS)
                                    .color(t.base),
                            )
                            .fill(t.success)
                            .rounding(theme::R_SM),
                        )
                        .clicked()
                    {
                        action = Some(ConflictAction::Single {
                            conflict_index,
                            resolution: Resolution::Ours,
                        });
                    }
                });
            });
        });

    // Ours section
    egui::Frame::none()
        .fill(ours_bg)
        .inner_margin(egui::Margin::symmetric(0.0, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(theme::SP_2);
                ui.label(
                    egui::RichText::new(format!("\u{25C0} OURS ({})", ours_label))
                        .size(theme::FONT_UI_XS)
                        .strong()
                        .color(t.success),
                );
            });
            for line in ours_lines {
                ui.horizontal(|ui| {
                    let (border_rect, _) = ui.allocate_exact_size(
                        egui::vec2(3.0, ui.spacing().interact_size.y),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(border_rect, 0.0, t.success);
                    ui.add_space(theme::SP_1);
                    ui.label(
                        egui::RichText::new(line)
                            .monospace()
                            .size(theme::FONT_TERM)
                            .color(t.success),
                    );
                });
            }
        });

    // Separator
    ui.horizontal(|ui| {
        ui.add_space(theme::SP_2);
        ui.label(
            egui::RichText::new("\u{2550}".repeat(30))
                .monospace()
                .size(theme::FONT_UI_XS)
                .color(t.overlay0),
        );
    });

    // Theirs section
    egui::Frame::none()
        .fill(theirs_bg)
        .inner_margin(egui::Margin::symmetric(0.0, theme::SP_1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(theme::SP_2);
                ui.label(
                    egui::RichText::new(format!("\u{25B6} THEIRS ({})", theirs_label))
                        .size(theme::FONT_UI_XS)
                        .strong()
                        .color(t.error),
                );
            });
            for line in theirs_lines {
                ui.horizontal(|ui| {
                    let (border_rect, _) = ui.allocate_exact_size(
                        egui::vec2(3.0, ui.spacing().interact_size.y),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(border_rect, 0.0, t.error);
                    ui.add_space(theme::SP_1);
                    ui.label(
                        egui::RichText::new(line)
                            .monospace()
                            .size(theme::FONT_TERM)
                            .color(t.error),
                    );
                });
            }
        });

    action
}
