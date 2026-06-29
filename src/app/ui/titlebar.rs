use super::super::pane::RightTab;
use super::super::App;
use crate::shortcuts::AppAction;
use crate::theme;
use crate::ui_kit;
use crate::updater::UpdateStatus;
use std::time::Duration;

impl App {
    fn shortcut_tooltip(&self, desc: &str, action: AppAction) -> String {
        match self.shortcut_registry.label_for(action) {
            Some(label) => format!("{desc} ({label})"),
            None => desc.to_string(),
        }
    }
    pub(in crate::app) fn render_titlebar(&mut self, ctx: &egui::Context) {
        // Request an extra repaint the frame after the window gains focus so that
        // any stale wgpu surface frames (visible as a distorted first frame after
        // restoring from the taskbar) are immediately replaced with a correct one.
        // Per-window: each viewport tracks its own focus state.
        {
            let focused = ctx.input(|i| i.focused);
            if focused && !self.was_focused {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
            self.was_focused = focused;
        }

        // ── Track last active pane per group ───────────────────────────────
        self.track_active_pane_group();

        // Validate current right_tab; fall back to Directory if stale
        {
            let keep = match &self.right_tab {
                RightTab::Directory => true,
                RightTab::GitDiff => self
                    .active_pane_cwd()
                    .and_then(|cwd| self.watch_state.as_ref()?.dir_data.get(&cwd))
                    .map(|d| d.is_git)
                    .unwrap_or(false),
                RightTab::Markdown(p) => self
                    .shown_md_tabs
                    .get(&self.active_group)
                    .is_some_and(|tabs| tabs.contains(p)),
            };
            if !keep {
                self.right_tab = RightTab::Directory;
            }
        }

        // ── Update window title with active workspace ───────────────────────
        let ws_title: String = self
            .active_workspace()
            .map(|w| format!("Terminal Studio — {}", w.name))
            .unwrap_or_else(|| "Terminal Studio".to_string());
        let active_ws_color: Option<[u8; 3]> = self.active_workspace().map(|w| w.color);
        // Only send the title command when it changes. Sending every frame
        // produces a SetWindowTextW syscall on Windows for no reason.
        if self.last_title_sent.as_deref() != Some(ws_title.as_str()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(ws_title.clone()));
            self.last_title_sent = Some(ws_title.clone());
        }

        // ── Custom titlebar ─────────────────────────────────────────────────
        let tb_bg = match active_ws_color {
            Some(c) => theme::from_rgb(c),
            None => theme::active().bg_panel_fill,
        };
        let tb_fg = active_ws_color
            .map(theme::text_on)
            .unwrap_or(theme::active().subtext1);

        egui::TopBottomPanel::top(self.vp_id("titlebar"))
            .exact_height(theme::TITLEBAR_H)
            .frame(egui::Frame::none().fill(tb_bg))
            .show(ctx, |ui| {
                let r = ui.max_rect();
                let painter = ui.painter().clone();

                // Bottom border
                painter.line_segment(
                    [egui::pos2(r.min.x, r.max.y), egui::pos2(r.max.x, r.max.y)],
                    egui::Stroke::new(theme::STROKE_THIN, theme::active().border_subtle),
                );

                // Drag the whole bar to move the window; double-click to maximize/restore
                let drag_resp =
                    ui.interact(r, self.vp_id("tb_drag"), egui::Sense::click_and_drag());
                if drag_resp.dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag_resp.double_clicked() {
                    let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
                }

                // ── macOS: traffic lights on the left ──────────────────────
                #[cfg(target_os = "macos")]
                {
                    let btn_y = r.center().y;
                    // hover_any: show colour only when any circle is hovered
                    let hover_pos = ctx.input(|i| i.pointer.hover_pos());
                    let hover_any = hover_pos
                        .map(|p| {
                            [18.0_f32, 38.0, 58.0].iter().any(|&ox| {
                                (p.x - (r.min.x + ox)).abs() < 8.0 && (p.y - btn_y).abs() < 8.0
                            })
                        })
                        .unwrap_or(false);

                    let circles: &[(f32, egui::Color32, usize)] = &[
                        (r.min.x + 18.0, egui::Color32::from_rgb(255, 96, 89), 0), // close
                        (r.min.x + 38.0, egui::Color32::from_rgb(255, 189, 68), 1), // minimize
                        (r.min.x + 58.0, egui::Color32::from_rgb(39, 201, 63), 2), // maximize
                    ];
                    for &(cx, color, idx) in circles {
                        let pos = egui::pos2(cx, btn_y);
                        let brect = egui::Rect::from_center_size(pos, egui::vec2(14.0, 14.0));
                        let resp = ui.interact(
                            brect,
                            self.vp_id("tb_mac").with(idx),
                            egui::Sense::click(),
                        );
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let fill = if hover_any {
                            color
                        } else {
                            theme::active().overlay0
                        };
                        painter.circle_filled(pos, 6.0, fill);
                        if resp.clicked() {
                            match idx {
                                0 => {
                                    if self.current_window_id.is_some() {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    } else if self.session_state.sessions.is_empty() {
                                        self.quit_confirmed = true;
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    } else {
                                        self.show_quit_confirm = true;
                                    }
                                }
                                1 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                                _ => {
                                    let is_max =
                                        ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                                        !is_max,
                                    ));
                                }
                            }
                        }
                    }
                    // Left panel toggle (after traffic lights)
                    let mac_btn_w = 28.0_f32;
                    let mac_icon_sz = theme::FONT_UI_LG;
                    let left_tbr = egui::Rect::from_min_size(
                        egui::pos2(r.min.x + 72.0, r.min.y),
                        egui::vec2(mac_btn_w, r.height()),
                    );
                    let left_resp = ui_kit::icon_button(
                        ui,
                        self.vp_id("tb_left_toggle"),
                        left_tbr,
                        "≡",
                        mac_icon_sz,
                        tb_fg,
                        ui_kit::IconButtonStyle::Toggle {
                            active: self.show_left_panel,
                        },
                    );
                    if left_resp.clicked() {
                        self.show_left_panel = !self.show_left_panel;
                    }
                    left_resp.on_hover_text(
                        self.shortcut_tooltip("Toggle sidebar", AppAction::ToggleLeftSidebar),
                    );

                    // Switcher button (after left toggle)
                    {
                        let hint_text = self
                            .shortcut_registry
                            .label_for(AppAction::OpenQuickSwitcher)
                            .unwrap_or("");
                        let hint_font = egui::FontId::proportional(theme::FONT_UI_XS);
                        let hint_galley =
                            painter.layout_no_wrap(hint_text.to_string(), hint_font.clone(), tb_fg);
                        let hint_w = if self.show_quick_switcher {
                            0.0
                        } else {
                            hint_galley.size().x + theme::SP_3
                        };
                        let total_w = mac_btn_w + hint_w;
                        let sw_tbr = egui::Rect::from_min_size(
                            egui::pos2(r.min.x + 72.0 + mac_btn_w, r.min.y),
                            egui::vec2(total_w, r.height()),
                        );
                        let sw_resp =
                            ui.interact(sw_tbr, self.vp_id("tb_switcher"), egui::Sense::click());
                        if sw_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let sw_hover_t = crate::app::ui::animation::animated_hover(
                            ui.ctx(),
                            self.vp_id("tb_switcher"),
                            sw_resp.hovered(),
                        );
                        let sw_bg = if self.show_quick_switcher {
                            theme::active().surface2
                        } else {
                            theme::lerp_color(
                                egui::Color32::TRANSPARENT,
                                theme::active().surface1,
                                sw_hover_t,
                            )
                        };
                        painter.rect_filled(sw_tbr, theme::R_MD, sw_bg);
                        let icon_center =
                            egui::pos2(sw_tbr.min.x + mac_btn_w * 0.5, sw_tbr.center().y);
                        let sw_icon_color =
                            theme::lerp_color(tb_fg, theme::active().fg_secondary, sw_hover_t);
                        painter.text(
                            icon_center,
                            egui::Align2::CENTER_CENTER,
                            "\u{21C6}",
                            egui::FontId::proportional(mac_icon_sz),
                            sw_icon_color,
                        );
                        if !self.show_quick_switcher {
                            let hint_x = sw_tbr.min.x + mac_btn_w + theme::SP_1;
                            painter.text(
                                egui::pos2(hint_x, sw_tbr.center().y),
                                egui::Align2::LEFT_CENTER,
                                hint_text,
                                hint_font,
                                theme::active().subtext0,
                            );
                        }
                        if sw_resp.clicked() {
                            self.show_quick_switcher = !self.show_quick_switcher;
                            if !self.show_quick_switcher {
                                self.quick_switcher_query.clear();
                                self.quick_switcher_selected_ws = None;
                                self.quick_switcher_search_active = false;
                            }
                        }
                        sw_resp.on_hover_text(
                            self.shortcut_tooltip("Switcher", AppAction::OpenQuickSwitcher),
                        );
                    }

                    // Recently Closed Sessions button (macOS, after settings from right)
                    {
                        let rc_mac_x = r.max.x - mac_btn_w * 3.0;
                        let rc_tbr = egui::Rect::from_min_size(
                            egui::pos2(rc_mac_x, r.min.y),
                            egui::vec2(mac_btn_w, r.height()),
                        );
                        let rc_resp = ui_kit::icon_button(
                            ui,
                            self.vp_id("tb_closed_sessions"),
                            rc_tbr,
                            "\u{23EA}",
                            mac_icon_sz,
                            tb_fg,
                            ui_kit::IconButtonStyle::Toggle {
                                active: self.show_closed_sessions,
                            },
                        );
                        if rc_resp.clicked() {
                            self.show_closed_sessions = !self.show_closed_sessions;
                            if !self.show_closed_sessions {
                                self.closed_sessions_query.clear();
                                self.closed_sessions_selected = 0;
                                self.closed_sessions_cache = None;
                            }
                        }
                        rc_resp.on_hover_text(self.shortcut_tooltip(
                            "Recently closed sessions",
                            AppAction::ReopenClosedSession,
                        ));
                    }

                    // Gear / Settings (rightmost on macOS)
                    let gear_mac_tbr = egui::Rect::from_min_size(
                        egui::pos2(r.max.x - mac_btn_w, r.min.y),
                        egui::vec2(mac_btn_w, r.height()),
                    );
                    let gear_mac_resp = ui_kit::icon_button(
                        ui,
                        self.vp_id("tb_settings"),
                        gear_mac_tbr,
                        "⚙",
                        mac_icon_sz,
                        tb_fg,
                        ui_kit::IconButtonStyle::Toggle {
                            active: self.show_settings,
                        },
                    );
                    if gear_mac_resp.clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    gear_mac_resp
                        .on_hover_text(self.shortcut_tooltip("Settings", AppAction::OpenSettings));

                    // Right panel toggle (macOS) — just before settings
                    let mac_right_toggle_x = r.max.x - mac_btn_w * 2.0;
                    {
                        let right_tbr = egui::Rect::from_min_size(
                            egui::pos2(mac_right_toggle_x, r.min.y),
                            egui::vec2(mac_btn_w, r.height()),
                        );
                        let right_resp = ui_kit::icon_button(
                            ui,
                            self.vp_id("tb_right_toggle"),
                            right_tbr,
                            "⊞",
                            mac_icon_sz,
                            tb_fg,
                            ui_kit::IconButtonStyle::Toggle {
                                active: self.show_right_panel,
                            },
                        );
                        if right_resp.clicked() {
                            self.show_right_panel = !self.show_right_panel;
                        }
                        right_resp.on_hover_text(
                            self.shortcut_tooltip("Toggle explorer", AppAction::ToggleRightSidebar),
                        );
                    }

                    // Keyboard shortcuts button (macOS) with hint label
                    let mac_kb_btn_x;
                    {
                        let hint_text = self
                            .shortcut_registry
                            .label_for(AppAction::ToggleShortcutHelp)
                            .unwrap_or("");
                        let hint_font = egui::FontId::proportional(theme::FONT_UI_XS);
                        let hint_galley =
                            painter.layout_no_wrap(hint_text.to_string(), hint_font.clone(), tb_fg);
                        let hint_w = if self.show_shortcut_help {
                            0.0
                        } else {
                            hint_galley.size().x + theme::SP_3
                        };
                        let total_w = mac_btn_w + hint_w;
                        let kb_x = mac_right_toggle_x - total_w;
                        mac_kb_btn_x = kb_x;
                        let kb_tbr = egui::Rect::from_min_size(
                            egui::pos2(kb_x, r.min.y),
                            egui::vec2(total_w, r.height()),
                        );
                        let kb_resp =
                            ui.interact(kb_tbr, self.vp_id("tb_shortcuts"), egui::Sense::click());
                        let kb_hover_t = crate::app::ui::animation::animated_hover(
                            ui.ctx(),
                            self.vp_id("tb_shortcuts"),
                            kb_resp.hovered(),
                        );
                        let bg = if self.show_shortcut_help {
                            theme::active().surface1
                        } else {
                            theme::lerp_color(
                                egui::Color32::TRANSPARENT,
                                theme::active().surface1,
                                kb_hover_t,
                            )
                        };
                        painter.rect_filled(kb_tbr, theme::R_MD, bg);
                        let icon_center =
                            egui::pos2(kb_tbr.max.x - mac_btn_w * 0.5, kb_tbr.center().y);
                        let kb_icon_color =
                            theme::lerp_color(tb_fg, theme::active().fg_secondary, kb_hover_t);
                        painter.text(
                            icon_center,
                            egui::Align2::CENTER_CENTER,
                            "⌨",
                            egui::FontId::proportional(mac_icon_sz),
                            kb_icon_color,
                        );
                        if !self.show_shortcut_help {
                            let hint_x = kb_tbr.min.x + theme::SP_2;
                            painter.text(
                                egui::pos2(hint_x, kb_tbr.center().y),
                                egui::Align2::LEFT_CENTER,
                                hint_text,
                                hint_font,
                                theme::active().subtext0,
                            );
                        }
                        if kb_resp.clicked() {
                            self.show_shortcut_help = !self.show_shortcut_help;
                        }
                        kb_resp.on_hover_text(
                            self.shortcut_tooltip(
                                "Keyboard shortcuts",
                                AppAction::ToggleShortcutHelp,
                            ),
                        );
                    }

                    // System monitor widget — before keyboard shortcuts
                    let sysmon_w = if self.settings.show_sys_monitor {
                        theme::SYSMON_W
                    } else {
                        0.0
                    };
                    let sysmon_mac_x = mac_kb_btn_x - sysmon_w;
                    if self.settings.show_sys_monitor {
                        let sr = egui::Rect::from_min_size(
                            egui::pos2(sysmon_mac_x, r.min.y),
                            egui::vec2(theme::SYSMON_W, r.height()),
                        );
                        self.paint_sys_monitor(&painter, sr, tb_fg);
                    }

                    // Update button (macOS) — left of sys monitor when visible
                    if let Some(ref uc) = self.workers.update_checker {
                        let update_state = uc.state();
                        let show_update_btn = matches!(
                            update_state.status,
                            UpdateStatus::UpdateAvailable { .. } | UpdateStatus::RestartRequired
                        );
                        if show_update_btn {
                            let label = match &update_state.status {
                                UpdateStatus::UpdateAvailable { version, .. } => {
                                    format!("\u{2B06} Update v{version}")
                                }
                                UpdateStatus::RestartRequired => "Restart to update".to_string(),
                                _ => String::new(),
                            };
                            let update_x =
                                sysmon_mac_x - theme::UPDATE_BTN_W - theme::TITLEBAR_ICON_GAP;
                            let br = egui::Rect::from_min_size(
                                egui::pos2(update_x, r.min.y + theme::SP_2),
                                egui::vec2(theme::UPDATE_BTN_W, r.height() - theme::SP_4),
                            );
                            let resp =
                                ui.interact(br, self.vp_id("tb_update_btn"), egui::Sense::click());
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            let t = theme::active();
                            let bg = if resp.hovered() { t.green } else { t.surface2 };
                            let update_btn_fg = theme::text_on([bg.r(), bg.g(), bg.b()]);
                            painter.rect_filled(br, theme::R_MD, bg);
                            painter.text(
                                br.center(),
                                egui::Align2::CENTER_CENTER,
                                &label,
                                egui::FontId::proportional(theme::FONT_UI_SM),
                                update_btn_fg,
                            );
                            if resp.clicked() {
                                match &update_state.status {
                                    UpdateStatus::UpdateAvailable { .. } => {
                                        self.show_settings = true;
                                    }
                                    UpdateStatus::RestartRequired => {
                                        self.save_session();
                                        crate::updater::restart_app();
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    // Title centered — clipped to avoid overlapping traffic lights and right buttons
                    let mac_clip_min_x =
                        r.min.x + 72.0 + mac_btn_w * 2.0 + theme::TITLEBAR_ICON_GAP;
                    let mac_clip_max_x = sysmon_mac_x - theme::TITLEBAR_ICON_GAP;
                    let mac_clip_rect = egui::Rect::from_min_max(
                        egui::pos2(mac_clip_min_x, r.min.y),
                        egui::pos2(mac_clip_max_x, r.max.y),
                    );
                    let mac_clipped = painter.with_clip_rect(mac_clip_rect);
                    let t = theme::active();
                    let title_color = t.surface2;
                    if let Some(ws_name) = self.active_workspace().map(|w| w.name.clone()) {
                        let title_font = egui::FontId::proportional(theme::FONT_UI_MD);
                        let app_galley = ui.fonts(|f| {
                            f.layout_no_wrap(
                                "Terminal Studio".to_string(),
                                title_font.clone(),
                                title_color,
                            )
                        });
                        let sep_galley = ui.fonts(|f| {
                            f.layout_no_wrap(
                                " \u{00b7} ".to_string(),
                                title_font.clone(),
                                title_color,
                            )
                        });
                        let name_galley =
                            ui.fonts(|f| f.layout_no_wrap(ws_name, title_font, t.fg_secondary));
                        let app_w = app_galley.size().x;
                        let sep_w = sep_galley.size().x;
                        let name_w = name_galley.size().x;
                        let total_w = app_w + sep_w + name_w;
                        let start_x = r.center().x - total_w / 2.0;
                        let text_y = r.center().y - app_galley.size().y / 2.0;
                        mac_clipped.galley(egui::pos2(start_x, text_y), app_galley, title_color);
                        mac_clipped.galley(
                            egui::pos2(start_x + app_w, text_y),
                            sep_galley,
                            title_color,
                        );
                        mac_clipped.galley(
                            egui::pos2(start_x + app_w + sep_w, text_y),
                            name_galley,
                            t.fg_secondary,
                        );
                    } else {
                        mac_clipped.text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            "Terminal Studio",
                            egui::FontId::proportional(theme::FONT_UI_MD),
                            title_color,
                        );
                    }
                }

                // ── Windows / Linux: controls on the right ─────────────────
                #[cfg(not(target_os = "macos"))]
                {
                    let btn_w = theme::TITLEBAR_BTN_W;
                    let icon_sz = theme::FONT_UI_LG;
                    // right-to-left: close(0), maximize(1), minimize(2)
                    let btns: &[(&str, usize, bool)] = &[
                        ("×", 0, true),  // close   — danger colour on hover
                        ("□", 1, false), // maximize
                        ("–", 2, false), // minimize
                    ];

                    // Left panel toggle — leftmost button
                    {
                        let br = egui::Rect::from_min_size(
                            egui::pos2(r.min.x, r.min.y),
                            egui::vec2(btn_w, r.height()),
                        );
                        let resp = ui_kit::icon_button(
                            ui,
                            self.vp_id("tb_left_toggle"),
                            br,
                            "≡",
                            icon_sz,
                            tb_fg,
                            ui_kit::IconButtonStyle::Toggle {
                                active: self.show_left_panel,
                            },
                        );
                        if resp.clicked() {
                            self.show_left_panel = !self.show_left_panel;
                        }
                        resp.on_hover_text(
                            self.shortcut_tooltip("Toggle sidebar", AppAction::ToggleLeftSidebar),
                        );
                    }

                    // Switcher button (after left toggle)
                    let switcher_end_x;
                    {
                        let hint_text = self
                            .shortcut_registry
                            .label_for(AppAction::OpenQuickSwitcher)
                            .unwrap_or("");
                        let hint_font = egui::FontId::proportional(theme::FONT_UI_XS);
                        let hint_galley =
                            painter.layout_no_wrap(hint_text.to_string(), hint_font.clone(), tb_fg);
                        let hint_w = if self.show_quick_switcher {
                            0.0
                        } else {
                            hint_galley.size().x + theme::SP_3
                        };
                        let total_w = btn_w + hint_w;
                        let sw_x = r.min.x + btn_w;
                        switcher_end_x = sw_x + total_w;
                        let br = egui::Rect::from_min_size(
                            egui::pos2(sw_x, r.min.y),
                            egui::vec2(total_w, r.height()),
                        );
                        let resp = ui.interact(br, self.vp_id("tb_switcher"), egui::Sense::click());
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let sw_hover_t = crate::app::ui::animation::animated_hover(
                            ui.ctx(),
                            self.vp_id("tb_switcher"),
                            resp.hovered(),
                        );
                        let bg = if self.show_quick_switcher {
                            theme::active().surface2
                        } else {
                            theme::lerp_color(
                                egui::Color32::TRANSPARENT,
                                theme::active().surface1,
                                sw_hover_t,
                            )
                        };
                        painter.rect_filled(br, theme::R_MD, bg);
                        let icon_center = egui::pos2(br.min.x + btn_w * 0.5, br.center().y);
                        let sw_icon_color =
                            theme::lerp_color(tb_fg, theme::active().fg_secondary, sw_hover_t);
                        painter.text(
                            icon_center,
                            egui::Align2::CENTER_CENTER,
                            "\u{21C6}",
                            egui::FontId::proportional(icon_sz),
                            sw_icon_color,
                        );
                        if !self.show_quick_switcher {
                            let hint_x = br.min.x + btn_w + theme::SP_1;
                            painter.text(
                                egui::pos2(hint_x, br.center().y),
                                egui::Align2::LEFT_CENTER,
                                hint_text,
                                hint_font,
                                theme::active().subtext0,
                            );
                        }
                        if resp.clicked() {
                            self.show_quick_switcher = !self.show_quick_switcher;
                            if !self.show_quick_switcher {
                                self.quick_switcher_query.clear();
                                self.quick_switcher_selected_ws = None;
                                self.quick_switcher_search_active = false;
                            }
                        }
                        resp.on_hover_text(
                            self.shortcut_tooltip("Switcher", AppAction::OpenQuickSwitcher),
                        );
                    }

                    // Recently Closed Sessions button (after switcher)
                    {
                        let rc_x = switcher_end_x;
                        let rc_br = egui::Rect::from_min_size(
                            egui::pos2(rc_x, r.min.y),
                            egui::vec2(btn_w, r.height()),
                        );
                        let rc_resp = ui_kit::icon_button(
                            ui,
                            self.vp_id("tb_closed_sessions"),
                            rc_br,
                            "\u{23EA}",
                            icon_sz,
                            tb_fg,
                            ui_kit::IconButtonStyle::Toggle {
                                active: self.show_closed_sessions,
                            },
                        );
                        if rc_resp.clicked() {
                            self.show_closed_sessions = !self.show_closed_sessions;
                            if !self.show_closed_sessions {
                                self.closed_sessions_query.clear();
                                self.closed_sessions_selected = 0;
                                self.closed_sessions_cache = None;
                            }
                        }
                        rc_resp.on_hover_text(self.shortcut_tooltip(
                            "Recently closed sessions",
                            AppAction::ReopenClosedSession,
                        ));
                    }

                    // Gear / Settings button — just before window controls
                    {
                        let gear_x = r.max.x - btn_w * (btns.len() as f32 + 1.0);
                        let br = egui::Rect::from_min_size(
                            egui::pos2(gear_x, r.min.y),
                            egui::vec2(btn_w, r.height()),
                        );
                        let resp = ui_kit::icon_button(
                            ui,
                            self.vp_id("tb_settings"),
                            br,
                            "⚙",
                            icon_sz,
                            tb_fg,
                            ui_kit::IconButtonStyle::Toggle {
                                active: self.show_settings,
                            },
                        );
                        if resp.clicked() {
                            self.show_settings = !self.show_settings;
                        }
                        resp.on_hover_text(
                            self.shortcut_tooltip("Settings", AppAction::OpenSettings),
                        );
                    }

                    // Right panel toggle — just before settings
                    let right_toggle_x = r.max.x - btn_w * (btns.len() as f32 + 2.0);
                    {
                        let br = egui::Rect::from_min_size(
                            egui::pos2(right_toggle_x, r.min.y),
                            egui::vec2(btn_w, r.height()),
                        );
                        let resp = ui_kit::icon_button(
                            ui,
                            self.vp_id("tb_right_toggle"),
                            br,
                            "⊞",
                            icon_sz,
                            tb_fg,
                            ui_kit::IconButtonStyle::Toggle {
                                active: self.show_right_panel,
                            },
                        );
                        if resp.clicked() {
                            self.show_right_panel = !self.show_right_panel;
                        }
                        resp.on_hover_text(
                            self.shortcut_tooltip("Toggle explorer", AppAction::ToggleRightSidebar),
                        );
                    }

                    // Keyboard shortcuts button with hint label
                    let kb_btn_x;
                    {
                        let hint_text = self
                            .shortcut_registry
                            .label_for(AppAction::ToggleShortcutHelp)
                            .unwrap_or("");
                        let hint_font = egui::FontId::proportional(theme::FONT_UI_XS);
                        let hint_galley =
                            painter.layout_no_wrap(hint_text.to_string(), hint_font.clone(), tb_fg);
                        let hint_w = if self.show_shortcut_help {
                            0.0
                        } else {
                            hint_galley.size().x + theme::SP_3
                        };
                        let total_w = btn_w + hint_w;
                        let kb_x = right_toggle_x - total_w;
                        kb_btn_x = kb_x;
                        let br = egui::Rect::from_min_size(
                            egui::pos2(kb_x, r.min.y),
                            egui::vec2(total_w, r.height()),
                        );
                        let resp =
                            ui.interact(br, self.vp_id("tb_shortcuts"), egui::Sense::click());
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let kb_hover_t = crate::app::ui::animation::animated_hover(
                            ui.ctx(),
                            self.vp_id("tb_shortcuts"),
                            resp.hovered(),
                        );
                        let bg = if self.show_shortcut_help {
                            theme::active().surface1
                        } else {
                            theme::lerp_color(
                                egui::Color32::TRANSPARENT,
                                theme::active().surface1,
                                kb_hover_t,
                            )
                        };
                        painter.rect_filled(br, theme::R_MD, bg);
                        let icon_center = egui::pos2(br.max.x - btn_w * 0.5, br.center().y);
                        let kb_icon_color =
                            theme::lerp_color(tb_fg, theme::active().fg_secondary, kb_hover_t);
                        painter.text(
                            icon_center,
                            egui::Align2::CENTER_CENTER,
                            "⌨",
                            egui::FontId::proportional(icon_sz),
                            kb_icon_color,
                        );
                        if !self.show_shortcut_help {
                            let hint_x = br.min.x + theme::SP_2;
                            painter.text(
                                egui::pos2(hint_x, br.center().y),
                                egui::Align2::LEFT_CENTER,
                                hint_text,
                                hint_font,
                                theme::active().subtext0,
                            );
                        }
                        if resp.clicked() {
                            self.show_shortcut_help = !self.show_shortcut_help;
                        }
                        resp.on_hover_text(
                            self.shortcut_tooltip(
                                "Keyboard shortcuts",
                                AppAction::ToggleShortcutHelp,
                            ),
                        );
                    }

                    // System monitor widget — before keyboard shortcuts
                    let sysmon_w = if self.settings.show_sys_monitor {
                        theme::SYSMON_W
                    } else {
                        0.0
                    };
                    let sysmon_x = kb_btn_x - sysmon_w;
                    if self.settings.show_sys_monitor {
                        let sr = egui::Rect::from_min_size(
                            egui::pos2(sysmon_x, r.min.y),
                            egui::vec2(theme::SYSMON_W, r.height()),
                        );
                        self.paint_sys_monitor(&painter, sr, tb_fg);
                    }

                    // Update button — visible only when update is available or restart required
                    let mut update_btn_end_x = sysmon_x;
                    if let Some(ref uc) = self.workers.update_checker {
                        let update_state = uc.state();
                        let show_update_btn = matches!(
                            update_state.status,
                            UpdateStatus::UpdateAvailable { .. } | UpdateStatus::RestartRequired
                        );
                        if show_update_btn {
                            let label = match &update_state.status {
                                UpdateStatus::UpdateAvailable { version, .. } => {
                                    format!("\u{2B06} Update v{version}")
                                }
                                UpdateStatus::RestartRequired => "Restart to update".to_string(),
                                _ => String::new(),
                            };
                            let update_x =
                                sysmon_x - theme::UPDATE_BTN_W - theme::TITLEBAR_ICON_GAP;
                            let br = egui::Rect::from_min_size(
                                egui::pos2(update_x, r.min.y + theme::SP_2),
                                egui::vec2(theme::UPDATE_BTN_W, r.height() - theme::SP_4),
                            );
                            let resp =
                                ui.interact(br, self.vp_id("tb_update_btn"), egui::Sense::click());
                            if resp.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            let t = theme::active();
                            let bg = if resp.hovered() { t.green } else { t.surface2 };
                            let update_btn_fg = theme::text_on([bg.r(), bg.g(), bg.b()]);
                            painter.rect_filled(br, theme::R_MD, bg);
                            painter.text(
                                br.center(),
                                egui::Align2::CENTER_CENTER,
                                &label,
                                egui::FontId::proportional(theme::FONT_UI_SM),
                                update_btn_fg,
                            );
                            if resp.clicked() {
                                match &update_state.status {
                                    UpdateStatus::UpdateAvailable { .. } => {
                                        self.show_settings = true;
                                    }
                                    UpdateStatus::RestartRequired => {
                                        self.save_session();
                                        crate::updater::restart_app();
                                    }
                                    _ => {}
                                }
                            }
                            update_btn_end_x = update_x;
                        }
                    }

                    for &(symbol, idx, is_danger) in btns {
                        let x = r.max.x - btn_w * (idx as f32 + 1.0);
                        let br = egui::Rect::from_min_size(
                            egui::pos2(x, r.min.y),
                            egui::vec2(btn_w, r.height()),
                        );
                        let style = if is_danger {
                            ui_kit::IconButtonStyle::Danger
                        } else {
                            ui_kit::IconButtonStyle::Default
                        };
                        let resp = ui_kit::icon_button(
                            ui,
                            self.vp_id("tb_btn").with(idx),
                            br,
                            symbol,
                            theme::FONT_UI_MD,
                            tb_fg,
                            style,
                        );
                        if resp.clicked() {
                            match idx {
                                0 => {
                                    if self.current_window_id.is_some() {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    } else if self.session_state.sessions.is_empty() {
                                        self.quit_confirmed = true;
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    } else {
                                        self.show_quit_confirm = true;
                                    }
                                }
                                1 => {
                                    let is_max =
                                        ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(
                                        !is_max,
                                    ));
                                }
                                2 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                                _ => {}
                            }
                        }
                    }
                    // Title between switcher button and update button (or sys monitor)
                    let clip_min_x = switcher_end_x + theme::TITLEBAR_ICON_GAP;
                    let clip_max_x = update_btn_end_x - theme::TITLEBAR_ICON_GAP;
                    let clip_rect = egui::Rect::from_min_max(
                        egui::pos2(clip_min_x, r.min.y),
                        egui::pos2(clip_max_x, r.max.y),
                    );
                    let clipped = painter.with_clip_rect(clip_rect);
                    // "Terminal Studio" dimmed; workspace name emphasized
                    let t = theme::active();
                    let title_color = t.surface2;
                    if let Some(ws_name) = self.active_workspace().map(|w| w.name.clone()) {
                        let title_font = egui::FontId::proportional(theme::FONT_UI_MD);
                        let app_galley = ui.fonts(|f| {
                            f.layout_no_wrap(
                                "Terminal Studio".to_string(),
                                title_font.clone(),
                                title_color,
                            )
                        });
                        let sep_galley = ui.fonts(|f| {
                            f.layout_no_wrap(
                                " \u{00b7} ".to_string(),
                                title_font.clone(),
                                title_color,
                            )
                        });
                        let name_galley =
                            ui.fonts(|f| f.layout_no_wrap(ws_name, title_font, t.fg_secondary));
                        let app_w = app_galley.size().x;
                        let sep_w = sep_galley.size().x;
                        let name_w = name_galley.size().x;
                        let total_w = app_w + sep_w + name_w;
                        let start_x = r.center().x - total_w / 2.0;
                        let text_y = r.center().y - app_galley.size().y / 2.0;
                        clipped.galley(egui::pos2(start_x, text_y), app_galley, title_color);
                        clipped.galley(
                            egui::pos2(start_x + app_w, text_y),
                            sep_galley,
                            title_color,
                        );
                        clipped.galley(
                            egui::pos2(start_x + app_w + sep_w, text_y),
                            name_galley,
                            t.fg_secondary,
                        );
                    } else {
                        clipped.text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            "Terminal Studio",
                            egui::FontId::proportional(theme::FONT_UI_MD),
                            title_color,
                        );
                    }
                }
            });
    }
}
