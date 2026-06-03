use crate::app::closed_sessions::ClosedSessionManifest;
use crate::theme;

use super::super::App;

impl App {
    pub(in crate::app) fn render_closed_sessions_picker(&mut self, ctx: &egui::Context) {
        if !self.show_closed_sessions {
            return;
        }

        if self.closed_sessions_cache.is_none() {
            self.closed_sessions_cache = Some(ClosedSessionManifest::load());
        }

        let screen_rect = ctx.screen_rect();
        let t = theme::active();

        // Dim background
        egui::Area::new(self.vp_id("closed_sessions_dim"))
            .fixed_pos(screen_rect.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let resp = ui.interact(
                    screen_rect,
                    self.vp_id("closed_sessions_dim_click"),
                    egui::Sense::click(),
                );
                ui.painter().rect_filled(
                    screen_rect,
                    0.0,
                    egui::Color32::from_black_alpha(theme::ALPHA_OVERLAY_DIM),
                );
                if resp.clicked() {
                    self.close_closed_sessions_picker();
                }
            });

        let dialog_w = (screen_rect.width() * 0.5).clamp(360.0, 560.0);
        let dialog_h = (screen_rect.height() * 0.6).clamp(240.0, 520.0);
        let dialog_pos = egui::pos2(
            screen_rect.center().x - dialog_w / 2.0,
            screen_rect.min.y + theme::DIALOG_TOP_OFFSET,
        );

        let mut restore_id: Option<u64> = None;
        let mut delete_id: Option<u64> = None;

        egui::Area::new(self.vp_id("closed_sessions_dialog"))
            .fixed_pos(dialog_pos)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(t.bg_term)
                    .rounding(egui::Rounding::same(theme::R_LG))
                    .stroke(egui::Stroke::new(theme::STROKE_THIN, t.surface2))
                    .inner_margin(egui::Margin::same(theme::SP_4))
                    .show(ui, |ui| {
                        ui.set_min_width(dialog_w - theme::SP_4 * 2.0);
                        ui.set_max_width(dialog_w - theme::SP_4 * 2.0);
                        ui.set_max_height(dialog_h);

                        // Escape to close
                        let esc = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                        });
                        if esc {
                            self.close_closed_sessions_picker();
                            return;
                        }

                        // Up/down/enter navigation
                        let up = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                        });
                        let down = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                        });
                        let enter = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        });

                        // Search input
                        let search_id = self.vp_id("closed_sessions_search");
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.closed_sessions_query)
                                .id(search_id)
                                .hint_text("Search closed sessions...")
                                .desired_width(f32::INFINITY)
                                .font(egui::FontId::proportional(theme::FONT_UI_MD))
                                .text_color(t.text),
                        );
                        if !resp.has_focus() {
                            resp.request_focus();
                        }

                        ui.add_space(theme::SP_3);

                        // Filter records
                        let records = self
                            .closed_sessions_cache
                            .as_ref()
                            .map(|m| &m.records[..])
                            .unwrap_or(&[]);

                        let query_lower = self.closed_sessions_query.to_lowercase();
                        let filtered: Vec<usize> = records
                            .iter()
                            .enumerate()
                            .filter(|(_, r)| {
                                if query_lower.is_empty() {
                                    return true;
                                }
                                r.title.to_lowercase().contains(&query_lower)
                                    || r.cwd
                                        .to_string_lossy()
                                        .to_lowercase()
                                        .contains(&query_lower)
                                    || r.workspace_name
                                        .as_deref()
                                        .unwrap_or("")
                                        .to_lowercase()
                                        .contains(&query_lower)
                                    || r.shell.to_lowercase().contains(&query_lower)
                            })
                            .map(|(i, _)| i)
                            .collect();

                        // Handle navigation
                        if filtered.is_empty() {
                            self.closed_sessions_selected = 0;
                        } else {
                            if up {
                                self.closed_sessions_selected =
                                    self.closed_sessions_selected.saturating_sub(1);
                            }
                            if down {
                                self.closed_sessions_selected = (self.closed_sessions_selected + 1)
                                    .min(filtered.len() - 1);
                            }
                            self.closed_sessions_selected =
                                self.closed_sessions_selected.min(filtered.len().saturating_sub(1));

                            if enter {
                                if let Some(&idx) =
                                    filtered.get(self.closed_sessions_selected)
                                {
                                    restore_id = Some(records[idx].id);
                                }
                            }
                        }

                        // Render list
                        if filtered.is_empty() {
                            ui.add_space(theme::SP_4);
                            ui.label(
                                egui::RichText::new("No recently closed sessions")
                                    .color(t.subtext0)
                                    .font(egui::FontId::proportional(theme::FONT_UI_MD)),
                            );
                        } else {
                            egui::ScrollArea::vertical()
                                .max_height(dialog_h - 60.0)
                                .show(ui, |ui| {
                                    for (list_idx, &record_idx) in filtered.iter().enumerate() {
                                        let record = &records[record_idx];
                                        let selected =
                                            list_idx == self.closed_sessions_selected;

                                        let row_rect = ui
                                            .horizontal(|ui| {
                                                let bg = if selected {
                                                    t.surface1
                                                } else {
                                                    egui::Color32::TRANSPARENT
                                                };

                                                let frame = egui::Frame::none()
                                                    .fill(bg)
                                                    .rounding(egui::Rounding::same(theme::R_SM))
                                                    .inner_margin(egui::Margin::symmetric(
                                                        theme::SP_3,
                                                        theme::SP_2,
                                                    ));

                                                frame.show(ui, |ui| {
                                                    ui.set_min_width(
                                                        dialog_w - theme::SP_4 * 4.0,
                                                    );
                                                    ui.vertical(|ui| {
                                                        // Title row
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                egui::RichText::new(&record.title)
                                                                    .color(t.text)
                                                                    .font(egui::FontId::proportional(
                                                                        theme::FONT_UI_MD,
                                                                    )),
                                                            );
                                                            ui.with_layout(
                                                                egui::Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    ui.label(
                                                                        egui::RichText::new(
                                                                            format_relative_time(
                                                                                record.closed_at,
                                                                            ),
                                                                        )
                                                                        .color(t.subtext0)
                                                                        .font(
                                                                            egui::FontId::proportional(
                                                                                theme::FONT_UI_XS,
                                                                            ),
                                                                        ),
                                                                    );
                                                                    if let Some(ws_name) =
                                                                        &record.workspace_name
                                                                    {
                                                                        ui.label(
                                                                            egui::RichText::new(
                                                                                ws_name,
                                                                            )
                                                                            .color(t.accent)
                                                                            .font(
                                                                                egui::FontId::proportional(
                                                                                    theme::FONT_UI_XS,
                                                                                ),
                                                                            ),
                                                                        );
                                                                    }
                                                                },
                                                            );
                                                        });
                                                        // CWD + shell row
                                                        ui.horizontal(|ui| {
                                                            let cwd_str =
                                                                record.cwd.to_string_lossy();
                                                            let display_cwd = if cwd_str.len() > 50
                                                            {
                                                                format!(
                                                                    "...{}",
                                                                    &cwd_str[cwd_str.len() - 47..]
                                                                )
                                                            } else {
                                                                cwd_str.to_string()
                                                            };
                                                            ui.label(
                                                                egui::RichText::new(display_cwd)
                                                                    .color(t.subtext0)
                                                                    .font(
                                                                        egui::FontId::proportional(
                                                                            theme::FONT_UI_XS,
                                                                        ),
                                                                    ),
                                                            );
                                                            ui.label(
                                                                egui::RichText::new(format!(
                                                                    "{}  {} lines",
                                                                    &record.shell,
                                                                    format_line_count(
                                                                        record.line_count
                                                                    ),
                                                                ))
                                                                .color(t.subtext0)
                                                                .font(
                                                                    egui::FontId::proportional(
                                                                        theme::FONT_UI_XS,
                                                                    ),
                                                                ),
                                                            );
                                                        });
                                                    });
                                                });
                                            })
                                            .response
                                            .rect;

                                        let row_resp = ui.interact(
                                            row_rect,
                                            egui::Id::new(("closed_sess_row", record.id)),
                                            egui::Sense::click(),
                                        );

                                        if row_resp.clicked() {
                                            restore_id = Some(record.id);
                                        }

                                        row_resp.context_menu(|ui| {
                                            if ui.button("Restore").clicked() {
                                                restore_id = Some(record.id);
                                                ui.close_menu();
                                            }
                                            if ui.button("Delete from history").clicked() {
                                                delete_id = Some(record.id);
                                                ui.close_menu();
                                            }
                                        });
                                    }
                                });
                        }
                    });
            });

        if let Some(id) = restore_id {
            self.close_closed_sessions_picker();
            self.restore_closed_session(id);
        }

        if let Some(id) = delete_id {
            if let Some(ref mut manifest) = self.closed_sessions_cache {
                manifest.remove(id);
                manifest.save();
            }
        }
    }

    fn close_closed_sessions_picker(&mut self) {
        self.show_closed_sessions = false;
        self.closed_sessions_query.clear();
        self.closed_sessions_selected = 0;
        self.closed_sessions_cache = None;
    }
}

fn format_relative_time(epoch_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.saturating_sub(epoch_secs);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

fn format_line_count(count: usize) -> String {
    if count >= 1000 {
        format!("{:.1}k", count as f64 / 1000.0)
    } else {
        count.to_string()
    }
}
