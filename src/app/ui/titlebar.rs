use super::super::pane::{PaneContent, RightTab};
use super::super::title::effective_title;
use super::super::App;
use crate::shortcuts::AppAction;
use crate::theme;
use crate::ui_kit;
use crate::updater::UpdateStatus;
use std::time::Duration;

mod sysmon;

use sysmon::sysmon_rect;

fn titlebar_fill(t: &theme::Theme, _ws_color: Option<egui::Color32>) -> egui::Color32 {
    t.bg_panel
}

fn title_content_rect(clip: egui::Rect, content_width: f32) -> egui::Rect {
    egui::Rect::from_center_size(
        clip.center(),
        egui::vec2(
            content_width.min(clip.width()).max(theme::SP_0),
            clip.height(),
        ),
    )
}

fn title_clip_rect(bar: egui::Rect, switcher_end_x: f32, clip_max_x: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(switcher_end_x + theme::TITLEBAR_ICON_GAP, bar.min.y),
        egui::pos2(clip_max_x, bar.max.y),
    )
}

#[cfg(any(not(target_os = "macos"), test))]
fn non_macos_title_clip_rect(
    bar: egui::Rect,
    switcher_end_x: f32,
    history_button_width: f32,
    clip_max_x: f32,
) -> egui::Rect {
    title_clip_rect(bar, switcher_end_x + history_button_width, clip_max_x)
}

#[cfg(any(target_os = "macos", test))]
fn macos_title_clip_rect(
    bar: egui::Rect,
    switcher_end_x: f32,
    sysmon_start_x: f32,
    update_button_start_x: Option<f32>,
) -> egui::Rect {
    let trailing_controls_start_x = update_button_start_x.unwrap_or(sysmon_start_x);
    title_clip_rect(
        bar,
        switcher_end_x,
        trailing_controls_start_x - theme::TITLEBAR_ICON_GAP,
    )
}

fn paint_titlebar_title(
    ui: &mut egui::Ui,
    clip_rect: egui::Rect,
    session_title: &Option<String>,
    ws_name: &Option<String>,
    git_branch: &Option<String>,
) {
    let t = theme::active();
    let font = egui::FontId::proportional(theme::FONT_MD);
    let mut segments = vec![(
        session_title.as_deref().unwrap_or("Terminal Studio"),
        t.fg_strong,
    )];
    if let Some(workspace) = ws_name.as_deref() {
        segments.push((workspace, t.fg));
    }
    if let Some(branch) = git_branch.as_deref().filter(|branch| !branch.is_empty()) {
        segments.push((branch, t.fg_muted));
    }
    let separator = " \u{00b7} ";
    let separator_width = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(separator.to_owned(), font.clone(), t.fg_muted)
            .size()
            .x
    });
    let separators_width = separator_width * segments.len().saturating_sub(1) as f32;
    let segment_width =
        ((clip_rect.width() - separators_width) / segments.len() as f32).max(theme::SP_0);
    let content_width = ui.fonts(|fonts| {
        segments
            .iter()
            .map(|(text, color)| {
                fonts
                    .layout_no_wrap((*text).to_owned(), font.clone(), *color)
                    .size()
                    .x
                    .min(segment_width)
            })
            .sum::<f32>()
            + separators_width
    });
    let content_rect = title_content_rect(clip_rect, content_width);

    ui.allocate_ui_at_rect(content_rect, |ui| {
        ui.set_clip_rect(clip_rect);
        ui.style_mut().interaction.selectable_labels = false;
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            for (index, (text, color)) in segments.into_iter().enumerate() {
                if index > 0 {
                    ui.label(
                        egui::RichText::new(separator)
                            .font(font.clone())
                            .color(t.fg_muted),
                    );
                }
                ui_kit::truncate_label(ui, text, font.clone(), color, segment_width);
            }
        });
    });
}

fn paint_switcher_contents(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    icon_sz: f32,
    icon_color: egui::Color32,
    shortcut: Option<&crate::shortcuts::Shortcut>,
) {
    let t = theme::active();
    ui.allocate_ui_at_rect(rect.shrink2(egui::vec2(theme::SP_2, theme::SP_0)), |ui| {
        ui.set_clip_rect(rect);
        ui.style_mut().interaction.selectable_labels = false;
        ui.spacing_mut().item_spacing.x = theme::SP_2;
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Center),
            |ui| {
                ui.label(ui_kit::icon(
                    ui_kit::icon::ARROW_LEFT_RIGHT,
                    icon_sz,
                    icon_color,
                ));
                ui.label(
                    egui::RichText::new("Switch")
                        .size(theme::FONT_SM)
                        .color(t.fg),
                );
                if let Some(shortcut) = shortcut {
                    ui_kit::kbd_combo(ui, shortcut);
                }
            },
        );
    });
}

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

        // ── Update window title with active session + workspace ────────────
        let active_session_title: Option<String> = self
            .pane_state
            .active_pane_id
            .and_then(|pid| self.pane_state.find(pid))
            .and_then(|pane| match &pane.content {
                PaneContent::Terminal(sid) => {
                    let sid = *sid;
                    self.session_state
                        .sessions
                        .iter()
                        .find(|e| e.id == sid)
                        .map(|e| {
                            let s = e.session.read();
                            let title = s.title();
                            let cwd = s.cwd.clone();
                            drop(s);
                            let fg = self.workers.foreground_worker.get(e.id);
                            let ws_name = if cwd.as_os_str().is_empty() {
                                None
                            } else {
                                self.workspace_store
                                    .find_for_cwd(&cwd)
                                    .map(|w| w.name.clone())
                            };
                            effective_title(
                                &title,
                                &cwd,
                                fg.as_ref(),
                                Some(&e.shell),
                                ws_name.as_deref(),
                            )
                        })
                }
                PaneContent::FileEditor(ed) => ed
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned()),
                PaneContent::FileDiff(d) => d
                    .path
                    .file_name()
                    .map(|n| format!("diff: {}", n.to_string_lossy())),
                PaneContent::NoteEditor(_) => Some("Notes".to_string()),
                PaneContent::ConflictResolver(cr) => cr
                    .path
                    .file_name()
                    .map(|n| format!("conflict: {}", n.to_string_lossy())),
                PaneContent::DeferredTerminal { .. } => None,
            });
        let ws_name_for_title = self.active_workspace().map(|w| w.name.clone());
        let ws_title: String = match (&active_session_title, &ws_name_for_title) {
            (Some(session), Some(ws)) => format!("{session} — {ws}"),
            (Some(session), None) => session.clone(),
            (None, Some(ws)) => format!("Terminal Studio — {ws}"),
            (None, None) => "Terminal Studio".to_string(),
        };
        let active_ws_color = self
            .active_workspace()
            .map(|workspace| theme::from_rgb(workspace.color));
        // Only send the title command when it changes. Sending every frame
        // produces a SetWindowTextW syscall on Windows for no reason.
        if self.last_title_sent.as_deref() != Some(ws_title.as_str()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(ws_title.clone()));
            self.last_title_sent = Some(ws_title.clone());
        }

        // ── Custom titlebar ─────────────────────────────────────────────────
        let t = theme::active();
        let tb_bg = titlebar_fill(t, active_ws_color);
        let tb_fg = t.fg;

        egui::TopBottomPanel::top(self.vp_id("titlebar"))
            .exact_height(theme::TITLEBAR_H)
            .frame(egui::Frame::none().fill(tb_bg))
            .show(ctx, |ui| {
                let r = ui.max_rect();
                let painter = ui.painter().clone();

                // Bottom border
                painter.line_segment(
                    [egui::pos2(r.min.x, r.max.y), egui::pos2(r.max.x, r.max.y)],
                    egui::Stroke::new(theme::STROKE_THIN, t.border),
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
                    let mac_icon_sz = theme::FONT_MD;
                    let left_tbr = egui::Rect::from_min_size(
                        egui::pos2(r.min.x + 72.0, r.min.y),
                        egui::vec2(mac_btn_w, r.height()),
                    );
                    let left_resp = ui_kit::icon_button(
                        ui,
                        self.vp_id("tb_left_toggle"),
                        left_tbr,
                        ui_kit::icon::PANEL_LEFT,
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
                    let switcher_end_x;
                    {
                        let hint_text = self
                            .shortcut_registry
                            .label_for(AppAction::OpenQuickSwitcher)
                            .unwrap_or("");
                        let hint_font = egui::FontId::monospace(theme::FONT_SM);
                        let hint_galley =
                            painter.layout_no_wrap(hint_text.to_owned(), hint_font, tb_fg);
                        let switch_galley = painter.layout_no_wrap(
                            "Switch".to_owned(),
                            egui::FontId::proportional(theme::FONT_SM),
                            tb_fg,
                        );
                        let total_w = mac_btn_w
                            + switch_galley.size().x
                            + hint_galley.size().x
                            + theme::SP_6 * 2.0;
                        let sw_tbr = egui::Rect::from_min_size(
                            egui::pos2(r.min.x + 72.0 + mac_btn_w, r.min.y),
                            egui::vec2(total_w, r.height()),
                        );
                        switcher_end_x = sw_tbr.max.x;
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
                            t.bg_raised
                        } else {
                            theme::lerp_color(egui::Color32::TRANSPARENT, t.bg_raised, sw_hover_t)
                        };
                        painter.rect_filled(sw_tbr, theme::R_MD, sw_bg);
                        paint_switcher_contents(
                            ui,
                            sw_tbr,
                            mac_icon_sz,
                            tb_fg,
                            self.shortcut_registry
                                .find_shortcut(AppAction::OpenQuickSwitcher),
                        );
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
                            ui_kit::icon::HISTORY,
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
                        ui_kit::icon::SETTINGS,
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
                            ui_kit::icon::PANEL_RIGHT,
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
                        let hint_font = egui::FontId::proportional(theme::FONT_XS);
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
                            ui_kit::icon::KEYBOARD,
                            egui::FontId::new(mac_icon_sz, egui::FontFamily::Name("icons".into())),
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
                        self.paint_sys_monitor(&painter, sysmon_rect(sr, theme::SYSMON_W));
                    }

                    // Update button (macOS) — left of sys monitor when visible
                    let mut update_button_start_x = None;
                    if let Some(ref uc) = self.workers.update_checker {
                        let update_state = uc.state();
                        let show_update_btn = matches!(
                            update_state.status,
                            UpdateStatus::UpdateAvailable { .. } | UpdateStatus::RestartRequired
                        );
                        if show_update_btn {
                            let label = match &update_state.status {
                                UpdateStatus::UpdateAvailable { version, .. } => {
                                    format!("{} Update v{version}", ui_kit::icon::DOWNLOAD)
                                }
                                UpdateStatus::RestartRequired => {
                                    format!("{} Restart to update", ui_kit::icon::DOWNLOAD)
                                }
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
                                egui::FontId::new(
                                    theme::FONT_XS,
                                    egui::FontFamily::Name("icons".into()),
                                ),
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
                            update_button_start_x = Some(update_x);
                        }
                    }

                    // Title centered — clipped to avoid overlapping traffic lights and right buttons
                    let mac_clip_rect = macos_title_clip_rect(
                        r,
                        switcher_end_x,
                        sysmon_mac_x,
                        update_button_start_x,
                    );
                    let (ws_name, ws_id) = match self.active_workspace() {
                        Some(w) => (Some(w.name.clone()), Some(w.id)),
                        None => (None, None),
                    };
                    let git_branch = ws_id
                        .and_then(|id| self.workers.workspace_git_worker.get(id).map(|i| i.branch));
                    paint_titlebar_title(
                        ui,
                        mac_clip_rect,
                        &active_session_title,
                        &ws_name,
                        &git_branch,
                    );
                }

                // ── Windows / Linux: controls on the right ─────────────────
                #[cfg(not(target_os = "macos"))]
                {
                    let btn_w = theme::TITLEBAR_BTN_W;
                    let icon_sz = theme::FONT_MD;
                    // right-to-left: close(0), maximize(1), minimize(2)
                    let btns: &[(char, usize, bool)] = &[
                        (ui_kit::icon::X, 0, true),
                        (ui_kit::icon::MAXIMIZE, 1, false),
                        (ui_kit::icon::MINIMIZE, 2, false),
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
                            ui_kit::icon::PANEL_LEFT,
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
                        let hint_font = egui::FontId::monospace(theme::FONT_SM);
                        let hint_galley =
                            painter.layout_no_wrap(hint_text.to_owned(), hint_font, tb_fg);
                        let switch_galley = painter.layout_no_wrap(
                            "Switch".to_owned(),
                            egui::FontId::proportional(theme::FONT_SM),
                            tb_fg,
                        );
                        let total_w = btn_w
                            + switch_galley.size().x
                            + hint_galley.size().x
                            + theme::SP_6 * 2.0;
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
                            t.bg_raised
                        } else {
                            theme::lerp_color(egui::Color32::TRANSPARENT, t.bg_raised, sw_hover_t)
                        };
                        painter.rect_filled(br, theme::R_MD, bg);
                        paint_switcher_contents(
                            ui,
                            br,
                            icon_sz,
                            tb_fg,
                            self.shortcut_registry
                                .find_shortcut(AppAction::OpenQuickSwitcher),
                        );
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
                            ui_kit::icon::HISTORY,
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
                            ui_kit::icon::SETTINGS,
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
                            ui_kit::icon::PANEL_RIGHT,
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
                        let hint_font = egui::FontId::proportional(theme::FONT_XS);
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
                            ui_kit::icon::KEYBOARD,
                            egui::FontId::new(icon_sz, egui::FontFamily::Name("icons".into())),
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
                        self.paint_sys_monitor(&painter, sysmon_rect(sr, theme::SYSMON_W));
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
                                    format!("{} Update v{version}", ui_kit::icon::DOWNLOAD)
                                }
                                UpdateStatus::RestartRequired => {
                                    format!("{} Restart to update", ui_kit::icon::DOWNLOAD)
                                }
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
                                egui::FontId::new(
                                    theme::FONT_XS,
                                    egui::FontFamily::Name("icons".into()),
                                ),
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
                            theme::FONT_SM,
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
                    let clip_max_x = update_btn_end_x - theme::TITLEBAR_ICON_GAP;
                    let clip_rect = non_macos_title_clip_rect(r, switcher_end_x, btn_w, clip_max_x);
                    let (ws_name, ws_id) = match self.active_workspace() {
                        Some(w) => (Some(w.name.clone()), Some(w.id)),
                        None => (None, None),
                    };
                    let git_branch = ws_id
                        .and_then(|id| self.workers.workspace_git_worker.get(id).map(|i| i.branch));
                    paint_titlebar_title(
                        ui,
                        clip_rect,
                        &active_session_title,
                        &ws_name,
                        &git_branch,
                    );
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titlebar_fill_ignores_workspace_color() {
        let _guard = theme::test_support::lock_theme(theme::ThemeId::CatppuccinMocha);
        theme::set_theme(theme::ThemeId::CatppuccinMocha);
        let t = theme::active();

        assert_eq!(titlebar_fill(t, Some(egui::Color32::BLUE)), t.bg_panel);
        assert_eq!(titlebar_fill(t, None), t.bg_panel);
    }

    #[test]
    fn sysmon_rect_is_vertically_centered() {
        let bar = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 28.0));
        let rect = sysmon_rect(bar, theme::SYSMON_W);

        assert_eq!(rect.center().y, bar.center().y);
        assert!(bar.contains_rect(rect));
    }

    #[test]
    fn title_content_rect_is_centered() {
        let clip = egui::Rect::from_min_size(egui::pos2(20.0, 10.0), egui::vec2(300.0, 28.0));
        let rect = title_content_rect(clip, 80.0);

        assert_eq!(rect.center(), clip.center());
        assert!(clip.contains_rect(rect));
    }

    #[test]
    fn macos_title_clip_starts_after_switcher() {
        let bar = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 28.0));
        let switcher_end_x = 220.0;
        let clip = macos_title_clip_rect(bar, switcher_end_x, 600.0, None);

        assert_eq!(clip.min.x, switcher_end_x + theme::TITLEBAR_ICON_GAP);
    }

    #[test]
    fn windows_linux_title_clip_starts_after_history() {
        let bar = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 28.0));
        let switcher_end_x = 220.0;
        let clip = non_macos_title_clip_rect(bar, switcher_end_x, theme::TITLEBAR_BTN_W, 600.0);

        assert_eq!(
            clip.min.x,
            switcher_end_x + theme::TITLEBAR_BTN_W + theme::TITLEBAR_ICON_GAP
        );
    }

    #[test]
    fn macos_title_clip_ends_before_update() {
        let bar = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 28.0));
        let switcher_end_x = 220.0;
        let sysmon_start_x = 600.0;
        let update_start_x = 480.0;
        let clip = macos_title_clip_rect(bar, switcher_end_x, sysmon_start_x, Some(update_start_x));

        assert_eq!(clip.max.x, update_start_x - theme::TITLEBAR_ICON_GAP);
    }

    #[test]
    fn switcher_label_click_reaches_overlay() {
        let ctx = crate::ui_kit::test_harness::ctx_with_fonts();
        let overlay = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(180.0, 28.0));
        let shortcut = crate::shortcuts::Shortcut {
            ctrl: true,
            shift: false,
            alt: false,
            key: egui::Key::P,
        };
        let mut overlay_clicked = false;
        let _ = crate::ui_kit::test_harness::click_at(&ctx, overlay.center(), |ui| {
            let response = ui.interact(
                overlay,
                egui::Id::new("tb_switcher_test"),
                egui::Sense::click(),
            );
            paint_switcher_contents(ui, overlay, 12.0, egui::Color32::WHITE, Some(&shortcut));
            if response.clicked() {
                overlay_clicked = true;
            }
        });

        assert!(
            overlay_clicked,
            "Switch labels and shortcut chips must not steal the overlay click"
        );
    }

    #[test]
    fn title_labels_do_not_steal_bar_clicks() {
        let ctx = crate::ui_kit::test_harness::ctx_with_fonts();
        let bar = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 28.0));
        let clip = egui::Rect::from_min_size(egui::pos2(80.0, 0.0), egui::vec2(240.0, 28.0));
        let session = Some("main".to_string());
        let workspace = Some("workspace".to_string());
        let branch = Some("feat".to_string());
        let mut bar_clicked = false;
        let _ = crate::ui_kit::test_harness::click_at(&ctx, clip.center(), |ui| {
            let response = ui.interact(
                bar,
                egui::Id::new("tb_bar_test"),
                egui::Sense::click_and_drag(),
            );
            paint_titlebar_title(ui, clip, &session, &workspace, &branch);
            if response.clicked() {
                bar_clicked = true;
            }
        });

        assert!(
            bar_clicked,
            "title labels must not steal titlebar drag/click hits"
        );
    }
}
