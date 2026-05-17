use std::path::PathBuf;
use std::sync::atomic::Ordering;
use crate::pane_tree::{PaneNode, RemoveResult};
use crate::pty::foreground::ForegroundProcess;
use crate::pty::{default_shell, ShellKind};
use crate::theme;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use super::super::App;
use super::super::pane::{PaneContent, PaneEntry};
use super::super::title::{effective_title, shell_escape_arg};
use super::super::workspace_ui::WorkspaceEditDialog;

impl App {
    pub(in crate::app) fn render_left_panel(&mut self, ctx: &egui::Context) {
        // ── Foreground process detection (background worker, 500 ms poll) ────
        // Update the worker's session list so it polls the right PIDs, then
        // read instantly from the shared cache — never blocks the UI thread.
        {
            let pids: Vec<(u32, u32)> = self
                .sessions
                .iter()
                .filter(|e| e.alive.load(Ordering::Relaxed))
                .map(|e| (e.id, e.shell_pid))
                .collect();
            self.foreground_worker.set_sessions(pids);
        }
        let active_fg: Option<ForegroundProcess> = self
            .active_id
            .and_then(|sid| self.foreground_worker.get(sid));

        // ── Left panel: sessions (top) + workspaces (bottom) ───────────────
        let mut spawn_new_session: Option<ShellKind> = self.deferred_spawn.take();
        let mut spawn_new_session_cwd: Option<(ShellKind, PathBuf)> = None;
        let mut duplicate_session = std::mem::replace(&mut self.deferred_duplicate, false);
        let shells = self.available_shells.clone();
        let mut open_workspace_id: Option<u64> = self.deferred_open_workspace.take();
        let mut edit_workspace_id: Option<u64> = None;
        let mut new_window_workspace_id: Option<u64> = None;
        let mut quit_pane_id: Option<u32> = None;
        let mut clicked_sidebar_pane_id: Option<u32> = None;

        if self.show_left_panel {
            egui::SidePanel::left("sessions")
                .default_width(theme::LEFT_SIDEBAR_W)
                .width_range(80.0..=400.0)
                .resizable(true)
                .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(6.0, 0.0)))
                .show(ctx, |ui| {
                    let panel_rect = ui.max_rect();
                    let panel_w = panel_rect.width();
                    let total_h = panel_rect.height();

                    const DIV_H: f32 = 4.0;
                    const COLLAPSED_H: f32 = theme::HEADER_H;

                    // ── Height allocation ──────────────────────────────────────
                    let (sess_h, ws_h) = if self.workspace_panel_collapsed {
                        (total_h - COLLAPSED_H - DIV_H, COLLAPSED_H)
                    } else {
                        let wh = (total_h * self.workspace_panel_ratio).max(60.0);
                        let sh = (total_h - wh - DIV_H).max(60.0);
                        (sh, wh)
                    };

                    // Claim the full panel rect so egui's layout system doesn't
                    // re-use this space for anything else.
                    ui.allocate_rect(panel_rect, egui::Sense::hover());

                    // ── Sessions section ───────────────────────────────────────
                    let sess_rect =
                        egui::Rect::from_min_size(panel_rect.min, egui::vec2(panel_w, sess_h));
                    ui.allocate_ui_at_rect(sess_rect, |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), theme::HEADER_H),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new("Sessions")
                                        .strong()
                                        .size(theme::HEADER_FONT_SZ),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.menu_button(
                                            egui::RichText::new("+ New ▾")
                                                .size(theme::HEADER_FONT_SZ),
                                            |ui| {
                                                for shell in &shells {
                                                    if ui.button(shell.display_name()).clicked() {
                                                        spawn_new_session = Some(shell.clone());
                                                        ui.close_menu();
                                                    }
                                                }
                                                ui.separator();
                                                if shells.len() == 1 {
                                                    if ui.button("Open Folder…").clicked() {
                                                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                                            spawn_new_session_cwd = Some((shells[0].clone(), path));
                                                        }
                                                        ui.close_menu();
                                                    }
                                                } else {
                                                    ui.menu_button("Open Folder…", |ui| {
                                                        for shell in &shells {
                                                            if ui.button(shell.display_name()).clicked() {
                                                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                                                    spawn_new_session_cwd = Some((shell.clone(), path));
                                                                }
                                                                ui.close_menu();
                                                            }
                                                        }
                                                    });
                                                }
                                            },
                                        ).response.on_hover_text("New terminal (Ctrl+Shift+T)");
                                        if let Some(ref fp) = active_fg {
                                            if ui
                                                .button(
                                                    egui::RichText::new("Duplicate")
                                                        .size(theme::HEADER_FONT_SZ),
                                                )
                                                .on_hover_text(format!("Duplicate: {} (Ctrl+Shift+K)", fp.name))
                                                .clicked()
                                            {
                                                duplicate_session = true;
                                            }
                                        }
                                    },
                                );
                            },
                        );
                        ui.separator();

                        // ── Session search bar ────────────────────────────
                        if self.session_search_active {
                            let search_id = egui::Id::new("session_search_input");
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("🔍").size(12.0));
                                let te = egui::TextEdit::singleline(&mut self.session_search_query)
                                    .desired_width(ui.available_width() - theme::BTN_W)
                                    .hint_text("Filter sessions…")
                                    .font(egui::FontId::proportional(theme::SESSION_FONT_SZ))
                                    .id(search_id);
                                let r = ui.add(te);
                                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    self.session_search_active = false;
                                    self.session_search_query.clear();
                                }
                                r.request_focus();
                            });
                            ui.add_space(theme::SP_XS);
                        }

                        let session_filter = self.session_search_query.clone();

                        egui::ScrollArea::vertical()
                            .id_source("sessions_scroll")
                            .show(ui, |ui| {
                                let matcher = SkimMatcherV2::default();
                                // Iterate panes (not sessions) so restored DeferredTerminal panes
                                // appear immediately at launch — without forcing eager PTY spawn.
                                for (pane_idx, pane) in self.panes.iter().enumerate() {
                                    let (label, ws_color, dimmed): (String, Option<[u8; 3]>, bool) =
                                        match &pane.content {
                                            PaneContent::Terminal(sid) => {
                                                if let Some(e) =
                                                    self.sessions.iter().find(|e| e.id == *sid)
                                                {
                                                    let (title, cwd) = {
                                                        let s = e.session.read();
                                                        (s.title(), s.cwd.clone())
                                                    };
                                                    let color = if cwd.as_os_str().is_empty() {
                                                        None
                                                    } else {
                                                        self.workspace_store
                                                            .find_for_cwd(&cwd)
                                                            .map(|w| w.color)
                                                    };
                                                    let fg = self.foreground_worker.get(e.id);
                                                    (effective_title(&title, &cwd, fg.as_ref(), Some(&e.shell)), color, false)
                                                } else {
                                                    ("(missing)".to_string(), None, true)
                                                }
                                            }
                                            PaneContent::DeferredTerminal { cwd, .. } => {
                                                let cwd_path = cwd.clone().unwrap_or_default();
                                                let mut text = effective_title("", &cwd_path, None, None);
                                                if text.is_empty() {
                                                    text = "(restored)".to_string();
                                                }
                                                let color = cwd
                                                    .as_ref()
                                                    .filter(|c| !c.as_os_str().is_empty())
                                                    .and_then(|c| {
                                                        self.workspace_store
                                                            .find_for_cwd(c)
                                                            .map(|w| w.color)
                                                    });
                                                (text, color, true)
                                            }
                                            PaneContent::FileEditor(ed) => {
                                                let text = ed
                                                    .path
                                                    .file_name()
                                                    .and_then(|n| n.to_str())
                                                    .map(|s| s.to_string())
                                                    .unwrap_or_else(|| {
                                                        ed.path.display().to_string()
                                                    });
                                                let color = ed.workspace_id.and_then(|id| {
                                                    self.workspace_store
                                                        .workspaces
                                                        .iter()
                                                        .find(|w| w.id == id)
                                                        .map(|w| w.color)
                                                });
                                                (text, color, false)
                                            }
                                            PaneContent::FileDiff(d) => {
                                                let name = d
                                                    .path
                                                    .file_name()
                                                    .and_then(|n| n.to_str())
                                                    .map(|s| format!("⇄ {}", s))
                                                    .unwrap_or_else(|| format!("⇄ {}", d.path.display()));
                                                (name, None, false)
                                            }
                                        };

                                    if !session_filter.is_empty() && matcher.fuzzy_match(&label, &session_filter).is_none() {
                                        continue;
                                    }

                                    let is_active = self.active_pane_id == Some(pane.id);

                                    let (resp, painter) = ui.allocate_painter(
                                        egui::vec2(ui.available_width(), theme::SESSION_ROW_H),
                                        egui::Sense::click(),
                                    );
                                    let row_rect = resp.rect;

                                    // Quit button — always reserved at right edge
                                    let quit_rect = egui::Rect::from_min_size(
                                        egui::pos2(row_rect.max.x - theme::BTN_W, row_rect.min.y),
                                        egui::vec2(theme::BTN_W, row_rect.height()),
                                    );
                                    let quit_resp = ui.interact(
                                        quit_rect,
                                        egui::Id::new(("pane_quit", pane.id)),
                                        egui::Sense::click(),
                                    );

                                    let bg = if is_active {
                                        theme::active().bg_row_active
                                    } else if resp.hovered() || quit_resp.hovered() {
                                        theme::active().bg_row_hover
                                    } else {
                                        egui::Color32::TRANSPARENT
                                    };
                                    painter.rect_filled(row_rect, 0.0, bg);

                                    if let Some(c) = ws_color {
                                        let border = egui::Rect::from_min_size(
                                            row_rect.min,
                                            egui::vec2(theme::WS_BORDER_W, row_rect.height()),
                                        );
                                        painter.rect_filled(border, 0.0, theme::from_rgb(c));
                                    }

                                    // Draw quit button
                                    if quit_resp.hovered() {
                                        painter.rect_filled(quit_rect, 0.0, theme::active().danger_bg);
                                    }
                                    painter.text(
                                        quit_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "×",
                                        egui::FontId::proportional(14.0),
                                        theme::active().danger_fg,
                                    );

                                    // Pane indicator badge (P1, P2...) between title and quit btn
                                    let pane_badge = format!("P{}", pane_idx + 1);
                                    let badge_w = theme::BADGE_W;
                                    painter.text(
                                        egui::pos2(
                                            quit_rect.min.x - badge_w / 2.0 - 2.0,
                                            row_rect.center().y,
                                        ),
                                        egui::Align2::CENTER_CENTER,
                                        &pane_badge,
                                        egui::FontId::proportional(10.0),
                                        theme::active().overlay0,
                                    );

                                    // Title text clipped to leave room for quit button + badge
                                    let text_x = row_rect.min.x
                                        + if ws_color.is_some() {
                                            theme::WS_BORDER_W + theme::BAR_PAD_X
                                        } else {
                                            theme::BAR_PAD_X
                                        };
                                    let clip_max = quit_rect.min.x - badge_w - 3.0;
                                    let text_color = if dimmed {
                                        theme::active().overlay0
                                    } else if is_active {
                                        theme::active().text
                                    } else {
                                        theme::active().subtext0
                                    };
                                    painter
                                        .with_clip_rect(egui::Rect::from_min_max(
                                            egui::pos2(text_x, row_rect.min.y),
                                            egui::pos2(clip_max, row_rect.max.y),
                                        ))
                                        .text(
                                            egui::pos2(text_x, row_rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            &label,
                                            egui::FontId::proportional(theme::SESSION_FONT_SZ),
                                            text_color,
                                        );

                                    let resp = resp.on_hover_text(&label);

                                    if quit_resp.clicked() {
                                        quit_pane_id = Some(pane.id);
                                    } else if resp.clicked() {
                                        clicked_sidebar_pane_id = Some(pane.id);
                                    }
                                }
                            });
                    });

                    // ── Draggable divider ──────────────────────────────────────
                    let div_top = panel_rect.min.y + sess_h;
                    let div_rect = egui::Rect::from_min_size(
                        egui::pos2(panel_rect.left(), div_top),
                        egui::vec2(panel_w, DIV_H),
                    );
                    let div_resp = ui.interact(
                        div_rect,
                        egui::Id::new("ws_panel_divider"),
                        egui::Sense::drag(),
                    );
                    if div_resp.hovered() || div_resp.dragged() {
                        ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    }
                    let div_color = if div_resp.hovered() || div_resp.dragged() {
                        theme::active().ws_div_active
                    } else {
                        theme::active().ws_div_idle
                    };
                    ui.painter().rect_filled(div_rect, theme::STROKE_THIN, div_color);
                    if !self.workspace_panel_collapsed && div_resp.dragged() {
                        let delta = div_resp.drag_delta().y;
                        // Drag down → workspace grows; drag up → workspace shrinks.
                        // ws_h and sess_h come from the ratio calculation above so we
                        // invert: moving the divider up means less workspace height.
                        let new_ws_h = (ws_h - delta).clamp(60.0, total_h - 60.0 - DIV_H);
                        self.workspace_panel_ratio = new_ws_h / total_h;
                    }

                    // ── Workspaces section ─────────────────────────────────────
                    let ws_top = div_top + DIV_H;
                    let ws_rect = egui::Rect::from_min_size(
                        egui::pos2(panel_rect.left(), ws_top),
                        egui::vec2(panel_w, ws_h),
                    );
                    ui.allocate_ui_at_rect(ws_rect, |ui| {
                        ui.painter()
                            .rect_filled(ws_rect, 0.0, theme::active().bg_workspace_fill);

                        let ws_count = self.workspace_store.workspaces.len();
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), theme::HEADER_H),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new(format!("Workspaces ({})", ws_count))
                                        .strong()
                                        .size(theme::HEADER_FONT_SZ),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let arrow = if self.workspace_panel_collapsed {
                                            "▶"
                                        } else {
                                            "▼"
                                        };
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    egui::RichText::new(arrow)
                                                        .size(theme::HEADER_FONT_SZ),
                                                )
                                                .min_size(egui::vec2(
                                                    theme::HEADER_H,
                                                    theme::HEADER_H,
                                                ))
                                                .frame(false),
                                            )
                                            .clicked()
                                        {
                                            self.workspace_panel_collapsed =
                                                !self.workspace_panel_collapsed;
                                        }
                                    },
                                );
                            },
                        );

                        if !self.workspace_panel_collapsed {
                            let active_group_snap = self.active_group;
                            // Filter workspaces by the current window's ownership:
                            //   * Main window: show workspaces with `host_window_id == None`
                            //     and dim those currently detached into an extra window
                            //     so the user knows where they live.
                            //   * Extra window: show only workspaces hosted in *this* window.
                            let cur_win = self.current_window_id.clone();
                            // Snapshot: (id, name, color, has_note, in_extra_window)
                            let workspaces: Vec<(u64, String, [u8; 3], bool, bool)> = self
                                .workspace_store
                                .workspaces
                                .iter()
                                .filter(|w| match (&cur_win, &w.host_window_id) {
                                    // Main window: include workspaces that live here
                                    // (host=None) and ones hosted elsewhere (so they
                                    // appear grayed out).
                                    (None, _) => true,
                                    // Extra window: include only workspaces hosted here.
                                    (Some(this), Some(host)) => this == host,
                                    (Some(_), None) => false,
                                })
                                .map(|w| {
                                    let in_extra = w.host_window_id.is_some()
                                        && self
                                            .extra_windows
                                            .iter()
                                            .any(|ew| ew.workspace_id == w.id)
                                        && cur_win.is_none();
                                    (
                                        w.id,
                                        w.name.clone(),
                                        w.color,
                                        !self.note_store.get(Some(w.id)).is_empty(),
                                        in_extra,
                                    )
                                })
                                .collect();

                            egui::ScrollArea::vertical()
                                .id_source("ws_panel_scroll")
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.y = theme::SP_SM;
                                    for (id, name, color, has_note, in_extra_window) in &workspaces
                                    {
                                        let active = active_group_snap == Some(*id);
                                        let tint_factor = if *in_extra_window {
                                            0.20 // grayed-out for workspaces in extra windows
                                        } else if active {
                                            0.65
                                        } else {
                                            0.45
                                        };
                                        let fill =
                                            theme::from_rgb(theme::tinted(*color, tint_factor));
                                        let fg = if *in_extra_window {
                                            theme::active().overlay0
                                        } else {
                                            theme::text_on(theme::tinted(*color, tint_factor))
                                        };

                                        {
                                            const GEAR_W: f32 = 26.0;
                                            let full_w = ui.available_width();
                                            let stroke_val = if active {
                                                egui::Stroke::new(theme::STROKE_BOLD, theme::active().text)
                                            } else {
                                                egui::Stroke::new(
                                                    1.0,
                                                    theme::from_rgb(theme::tinted(*color, 0.30)),
                                                )
                                            };
                                            let (full_rect, _) = ui.allocate_exact_size(
                                                egui::vec2(full_w, theme::HEADER_H),
                                                egui::Sense::hover(),
                                            );
                                            let gear_rect = egui::Rect::from_min_size(
                                                egui::pos2(
                                                    full_rect.max.x - GEAR_W,
                                                    full_rect.min.y,
                                                ),
                                                egui::vec2(GEAR_W, full_rect.height()),
                                            );
                                            let name_rect = egui::Rect::from_min_max(
                                                full_rect.min,
                                                egui::pos2(gear_rect.min.x, full_rect.max.y),
                                            );
                                            let name_resp = ui.interact(
                                                name_rect,
                                                egui::Id::new(("ws_name", *id)),
                                                egui::Sense::click_and_drag(),
                                            );
                                            let gear_resp = ui.interact(
                                                gear_rect,
                                                egui::Id::new(("ws_gear", *id)),
                                                egui::Sense::click(),
                                            );

                                            if ui.is_rect_visible(full_rect) {
                                                let rounding = egui::Rounding::same(theme::ROUNDING);
                                                ui.painter().rect_filled(full_rect, rounding, fill);
                                                ui.painter()
                                                    .rect_stroke(full_rect, rounding, stroke_val);

                                                let name_str = if *in_extra_window {
                                                    format!("→ {} (other window)", name)
                                                } else if active {
                                                    format!("▶ {}", name)
                                                } else {
                                                    name.clone()
                                                };
                                                let name_galley = ui.fonts(|f| {
                                                    f.layout_no_wrap(
                                                        name_str,
                                                        egui::FontId::proportional(
                                                            theme::SESSION_FONT_SZ,
                                                        ),
                                                        fg,
                                                    )
                                                });
                                                let text_y = full_rect.center().y
                                                    - name_galley.size().y / 2.0;
                                                ui.painter()
                                                    .with_clip_rect(name_rect)
                                                    .galley(
                                                        egui::pos2(
                                                            full_rect.left() + theme::BAR_PAD_X,
                                                            text_y,
                                                        ),
                                                        name_galley,
                                                        fg,
                                                    );

                                                if *has_note {
                                                    let note_galley = ui.fonts(|f| {
                                                        f.layout_no_wrap(
                                                            "📝".to_string(),
                                                            egui::FontId::proportional(12.0),
                                                            fg,
                                                        )
                                                    });
                                                    let note_x = gear_rect.left()
                                                        - 4.0
                                                        - note_galley.size().x;
                                                    ui.painter().galley(
                                                        egui::pos2(note_x, text_y),
                                                        note_galley,
                                                        fg,
                                                    );
                                                }

                                                let gear_fg = if gear_resp.hovered() {
                                                    theme::active().text
                                                } else {
                                                    theme::active().subtext0
                                                };
                                                ui.painter().text(
                                                    gear_rect.center(),
                                                    egui::Align2::CENTER_CENTER,
                                                    "⚙",
                                                    egui::FontId::proportional(12.0),
                                                    gear_fg,
                                                );
                                            }

                                            name_resp.clone().on_hover_text(name);
                                            if name_resp.clicked() && !*in_extra_window {
                                                open_workspace_id = Some(*id);
                                            }
                                            if gear_resp.clicked() {
                                                edit_workspace_id = Some(*id);
                                            }
                                            let in_main = cur_win.is_none();
                                            name_resp.context_menu(|ui| {
                                                let enabled = !*in_extra_window;
                                                if ui
                                                    .add_enabled(
                                                        enabled,
                                                        egui::Button::new("Open workspace"),
                                                    )
                                                    .clicked()
                                                {
                                                    open_workspace_id = Some(*id);
                                                    ui.close_menu();
                                                }
                                                // "Open in new window" is only available
                                                // from the main window for workspaces
                                                // that don't already live elsewhere.
                                                if in_main
                                                    && !*in_extra_window
                                                    && ui.button("Open in new window").clicked()
                                                {
                                                    new_window_workspace_id = Some(*id);
                                                    ui.close_menu();
                                                }
                                                ui.separator();
                                                if ui.button("Edit workspace…").clicked() {
                                                    edit_workspace_id = Some(*id);
                                                    ui.close_menu();
                                                }
                                            });
                                        }
                                    }

                                    // "Other" group — unaffiliated panes. Only meaningful
                                    // in the main window; extra windows host exactly one
                                    // workspace each and never display unaffiliated panes.
                                    let show_other = cur_win.is_none();
                                    let other_active = active_group_snap.is_none();
                                    let other_has_note = !self.note_store.get(None).is_empty();
                                    let other_fill = if other_active {
                                        theme::active().surface2
                                    } else {
                                        theme::active().surface0
                                    };
                                    let other_fg = if other_active {
                                        theme::active().text
                                    } else {
                                        theme::active().subtext0
                                    };
                                    let other_stroke = if other_active {
                                        egui::Stroke::new(theme::STROKE_BOLD, theme::active().text)
                                    } else {
                                        egui::Stroke::new(theme::STROKE_THIN, theme::active().overlay0)
                                    };
                                    let other_w = ui.available_width();
                                    let (other_rect, other_resp) = if show_other {
                                        ui.allocate_exact_size(
                                            egui::vec2(other_w, 28.0),
                                            egui::Sense::click(),
                                        )
                                    } else {
                                        // Allocate a zero-height placeholder; subsequent
                                        // code is guarded by `show_other` so the response
                                        // and rect are never used.
                                        (
                                            egui::Rect::NOTHING,
                                            ui.interact(
                                                egui::Rect::NOTHING,
                                                egui::Id::new("other_skip"),
                                                egui::Sense::hover(),
                                            ),
                                        )
                                    };
                                    if show_other && ui.is_rect_visible(other_rect) {
                                        let rounding = egui::Rounding::same(theme::ROUNDING);
                                        ui.painter().rect_filled(other_rect, rounding, other_fill);
                                        ui.painter().rect_stroke(
                                            other_rect,
                                            rounding,
                                            other_stroke,
                                        );

                                        let other_name = if other_active {
                                            "▶ Other".to_string()
                                        } else {
                                            "Other".to_string()
                                        };
                                        let other_galley = ui.fonts(|f| {
                                            f.layout_no_wrap(
                                                other_name,
                                                egui::FontId::proportional(13.0),
                                                other_fg,
                                            )
                                        });
                                        let text_y =
                                            other_rect.center().y - other_galley.size().y / 2.0;
                                        ui.painter().galley(
                                            egui::pos2(other_rect.left() + 8.0, text_y),
                                            other_galley,
                                            other_fg,
                                        );

                                        if other_has_note {
                                            let note_galley = ui.fonts(|f| {
                                                f.layout_no_wrap(
                                                    "📝".to_string(),
                                                    egui::FontId::proportional(12.0),
                                                    other_fg,
                                                )
                                            });
                                            let note_x =
                                                other_rect.right() - 8.0 - note_galley.size().x;
                                            ui.painter().galley(
                                                egui::pos2(note_x, text_y),
                                                note_galley,
                                                other_fg,
                                            );
                                        }
                                    }
                                    if show_other && other_resp.clicked() {
                                        open_workspace_id = Some(u64::MAX); // sentinel for "Other"
                                    }
                                });
                        }
                    });
                });
        } // end if self.show_left_panel

        if let Some(ws_id) = open_workspace_id {
            let (cols, rows) = self.panes.first().map(|p| p.last_size).unwrap_or((80, 24));
            // u64::MAX is the sentinel for the "Other" group
            let group = if ws_id == u64::MAX { None } else { Some(ws_id) };
            self.switch_group(group, cols, rows);
        }

        if let Some(ws_id) = edit_workspace_id {
            if let Some(ws) = self
                .workspace_store
                .workspaces
                .iter()
                .find(|w| w.id == ws_id)
            {
                self.workspace_edit_dialog =
                    Some(WorkspaceEditDialog::new(ws.id, ws.name.clone(), ws.color));
            }
        }

        if let Some(ws_id) = new_window_workspace_id {
            self.open_workspace_in_new_window(ctx, ws_id);
        }

        if let Some(qpid) = quit_pane_id {
            if let Some(pos) = self.panes.iter().position(|p| p.id == qpid) {
                // Capture the session id (if any) before removing the pane so we can
                // tear it down after the pane and split-tree cleanup.
                let killed_sid = match &self.panes[pos].content {
                    PaneContent::Terminal(sid) => Some(*sid),
                    _ => None,
                };
                self.panes.remove(pos);
                if self.active_pane_id == Some(qpid) {
                    self.active_pane_id = self.panes.last().map(|p| p.id);
                }
                // Clean up split tree: if this was a root, remove it; if a leaf in
                // someone else's tree, remove the leaf and collapse the parent split.
                if self.pane_trees.remove(&qpid).is_none() {
                    let root_pid_opt = self
                        .pane_trees
                        .iter()
                        .find(|(_, tree)| tree.leaf_ids().contains(&qpid))
                        .map(|(&rpid, _)| rpid);
                    if let Some(root_pid) = root_pid_opt {
                        let result = if let Some(tree) = self.pane_trees.get_mut(&root_pid) {
                            tree.remove_pane(qpid)
                        } else {
                            RemoveResult::NotFound
                        };
                        if let RemoveResult::CollapseToSibling(replacement) = result {
                            if let Some(tree) = self.pane_trees.get_mut(&root_pid) {
                                *tree = replacement;
                            }
                        }
                    }
                }
                if let Some(sid) = killed_sid {
                    self.uninit_sessions.remove(&sid);
                    self.sessions.retain(|e| e.id != sid);
                    if self.active_id == Some(sid) {
                        self.active_id = self.sessions.first().map(|e| e.id);
                        self.update_is_active_flags();
                    }
                }
                // Ensure active session is shown in a pane
                if self.panes.is_empty() {
                    if let Some(new_sid) = self.active_id {
                        let pane_id = self.next_pane_id;
                        self.next_pane_id += 1;
                        self.panes.push(PaneEntry {
                            id: pane_id,
                            content: PaneContent::Terminal(new_sid),
                            manual_width: None,
                            last_size: (0, 0),
                        });
                        self.pane_trees.insert(
                            pane_id,
                            PaneNode::Leaf {
                                pane_id,
                                last_size: (0, 0),
                            },
                        );
                        self.active_pane_id = Some(pane_id);
                    }
                }
                self.save_session();
            }
        }

        if let Some(qpid) = clicked_sidebar_pane_id {
            let group_opt = self
                .panes
                .iter()
                .find(|p| p.id == qpid)
                .map(|p| Self::pane_group(&self.sessions, &self.workspace_store, p));
            if let Some(group) = group_opt {
                self.active_group = group;
                self.activate_pane(qpid);
                self.last_pane_per_group.insert(group, qpid);
            }
        }

        let spawn_with_cwd = if let Some(ref new_shell) = spawn_new_session {
            let cwd = self.active_cwd().or_else(|| {
                self.active_group.and_then(|gid| {
                    self.workspace_store
                        .workspaces
                        .iter()
                        .find(|w| w.id == gid)
                        .map(|w| w.path.clone())
                })
            });
            Some((new_shell.clone(), cwd))
        } else {
            spawn_new_session_cwd.map(|(shell, path)| (shell, Some(path)))
        };

        if let Some((new_shell, cwd)) = spawn_with_cwd {
            let (cols, rows) = self
                .panes
                .iter()
                .find(|p| Some(p.id) == self.active_pane_id)
                .map(|p| p.last_size)
                .unwrap_or_else(|| self.panes.first().map(|p| p.last_size).unwrap_or((80, 24)));
            if let Some(new_id) = self.spawn_session(&new_shell, cols, rows, cwd) {
                self.active_id = Some(new_id);
                if !self
                    .panes
                    .iter()
                    .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == new_id))
                {
                    let pane_id = self.next_pane_id;
                    self.next_pane_id += 1;
                    self.panes.push(PaneEntry {
                        id: pane_id,
                        content: PaneContent::Terminal(new_id),
                        manual_width: None,
                        last_size: (cols, rows),
                    });
                    self.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (cols, rows),
                        },
                    );
                    self.activate_pane(pane_id);
                }
            }
        }

        if duplicate_session {
            let dup_shell = self
                .sessions
                .iter()
                .find(|e| Some(e.id) == self.active_id)
                .map(|e| e.shell.clone())
                .unwrap_or_else(default_shell);
            let cwd = self.active_cwd().or_else(|| {
                self.active_group.and_then(|gid| {
                    self.workspace_store
                        .workspaces
                        .iter()
                        .find(|w| w.id == gid)
                        .map(|w| w.path.clone())
                })
            });
            let (cols, rows) = self
                .panes
                .iter()
                .find(|p| Some(p.id) == self.active_pane_id)
                .map(|p| p.last_size)
                .unwrap_or_else(|| self.panes.first().map(|p| p.last_size).unwrap_or((80, 24)));
            // Build the command string to replay in the new session
            let cmd_to_run: Option<String> = active_fg.as_ref().map(|fp| {
                let parts: Vec<String> = fp.cmdline.iter().map(|a| shell_escape_arg(a)).collect();
                let joined = parts.join(" ");
                // PowerShell does not invoke a quoted path string as a command without
                // the call operator; & works for both bare names and quoted full paths.
                #[cfg(target_os = "windows")]
                {
                    format!("& {}", joined)
                }
                #[cfg(not(target_os = "windows"))]
                {
                    joined
                }
            });
            if let Some(new_id) = self.spawn_session(&dup_shell, cols, rows, cwd) {
                self.active_id = Some(new_id);
                if !self
                    .panes
                    .iter()
                    .any(|p| matches!(&p.content, PaneContent::Terminal(s) if *s == new_id))
                {
                    // Insert new pane immediately after the current active pane
                    let insert_at = self
                        .panes
                        .iter()
                        .position(|p| Some(p.id) == self.active_pane_id)
                        .map(|i| i + 1)
                        .unwrap_or(self.panes.len());
                    let pane_id = self.next_pane_id;
                    self.next_pane_id += 1;
                    self.panes.insert(
                        insert_at,
                        PaneEntry {
                            id: pane_id,
                            content: PaneContent::Terminal(new_id),
                            manual_width: None,
                            last_size: (cols, rows),
                        },
                    );
                    self.pane_trees.insert(
                        pane_id,
                        PaneNode::Leaf {
                            pane_id,
                            last_size: (cols, rows),
                        },
                    );
                    self.activate_pane(pane_id);
                }
                // Queue the command; it will be sent once the new shell emits OSC 7 (prompt ready).
                if let Some(cmd) = cmd_to_run {
                    if let Some(entry) = self.sessions.iter_mut().find(|e| e.id == new_id) {
                        entry.pending_command = Some(cmd);
                    }
                }
            }
        }

    }
}
