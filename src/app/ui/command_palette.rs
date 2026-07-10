use crate::shortcuts::{AppAction, ShortcutRegistry};
use crate::theme;
use crate::ui_kit;

use super::super::pane::PaneContent;
use super::super::App;

impl App {
    pub(in crate::app) fn render_palette(&mut self, ctx: &egui::Context) {
        if !self.palette_open {
            return;
        }

        let screen_rect = ctx.screen_rect();
        let t = theme::active();

        // Dim background
        egui::Area::new(self.vp_id("cmd_palette_dim"))
            .fixed_pos(screen_rect.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let resp = ui.interact(
                    screen_rect,
                    self.vp_id("cmd_palette_dim_click"),
                    egui::Sense::click(),
                );
                ui.painter().rect_filled(
                    screen_rect,
                    0.0,
                    egui::Color32::from_black_alpha(theme::ALPHA_OVERLAY_DIM),
                );
                if resp.clicked() {
                    self.close_palette();
                }
            });

        let dialog_w = (screen_rect.width() * 0.45).clamp(320.0, 520.0);
        let dialog_h = (screen_rect.height() * 0.55).clamp(200.0, 480.0);
        let dialog_pos = egui::pos2(
            screen_rect.center().x - dialog_w / 2.0,
            screen_rect.min.y + theme::DIALOG_TOP_OFFSET,
        );

        let command_mode = self.palette_query.starts_with('>');

        let mut action_to_run: Option<AppAction> = None;
        let mut file_to_open: Option<std::path::PathBuf> = None;
        let mut file_to_open_external: Option<std::path::PathBuf> = None;

        egui::Area::new(self.vp_id("cmd_palette_dialog"))
            .fixed_pos(dialog_pos)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(t.bg_term)
                    .rounding(egui::Rounding::same(theme::R_LG))
                    .stroke(egui::Stroke::new(theme::STROKE_THIN, t.surface2))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(0.0, 4.0),
                        blur: 16.0,
                        spread: 4.0,
                        color: t.shadow_md,
                    })
                    .inner_margin(egui::Margin::same(theme::SP_4))
                    .show(ui, |ui| {
                        let inner_w = (dialog_w - theme::SP_4 * 2.0).max(0.0);
                        ui.set_min_width(inner_w);
                        ui.set_max_width(inner_w);
                        ui.set_max_height(dialog_h);

                        // Escape to close
                        let esc = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                        });
                        if esc {
                            self.close_palette();
                            return;
                        }

                        // Navigation keys
                        let up = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                        });
                        let down = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                        });
                        let enter = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        });
                        let ctrl_enter = ctx.input_mut(|i| {
                            i.consume_key(
                                egui::Modifiers {
                                    alt: false,
                                    ctrl: true,
                                    shift: false,
                                    mac_cmd: false,
                                    command: false,
                                },
                                egui::Key::Enter,
                            )
                        });

                        // Search input
                        let placeholder = if command_mode {
                            "Type a command\u{2026}"
                        } else {
                            "Search files by name\u{2026}"
                        };
                        let search_id = self.vp_id("cmd_palette_search");
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.palette_query)
                                .id(search_id)
                                .desired_width(dialog_w - theme::SP_4 * 2.0 - theme::SP_6)
                                .hint_text(placeholder)
                                .font(egui::FontId::monospace(theme::FONT_UI_MD)),
                        );
                        if resp.changed() {
                            self.palette_selected = 0;
                        }
                        if resp.lost_focus()
                            && !esc
                            && !ui.input(|i| i.key_pressed(egui::Key::Escape))
                        {
                            resp.request_focus();
                        }
                        if !resp.has_focus() && !esc {
                            ui.memory_mut(|m| m.request_focus(search_id));
                        }

                        ui.add_space(theme::SP_2);
                        let sep_rect =
                            ui.allocate_space(egui::vec2(ui.available_width(), 1.0)).1;
                        ui.painter().rect_filled(sep_rect, 0.0, t.border_subtle);
                        ui.add_space(theme::SP_2);

                        if command_mode {
                            // ── Command mode ──
                            let raw_query = &self.palette_query[1..]; // strip '>'
                            let query = raw_query.trim().to_lowercase();
                            let all_actions = all_palette_actions(&self.shortcut_registry);
                            let filtered: Vec<&PaletteEntry> = if query.is_empty() {
                                all_actions.iter().collect()
                            } else {
                                all_actions
                                    .iter()
                                    .filter(|e| fuzzy_match(&e.label_lower, &query))
                                    .collect()
                            };

                            let count = filtered.len();
                            if count == 0 {
                                ui.add_space(theme::SP_4);
                                ui.label(
                                    egui::RichText::new("No matching commands")
                                        .size(theme::FONT_UI_SM)
                                        .color(t.overlay0),
                                );
                            } else {
                                if self.palette_selected >= count {
                                    self.palette_selected = count.saturating_sub(1);
                                }
                                if up && self.palette_selected > 0 {
                                    self.palette_selected -= 1;
                                }
                                if down && self.palette_selected + 1 < count {
                                    self.palette_selected += 1;
                                }
                                if enter {
                                    action_to_run =
                                        Some(filtered[self.palette_selected].action);
                                }

                                egui::ScrollArea::vertical()
                                    .id_source(self.vp_id("cmd_palette_scroll"))
                                    .auto_shrink([false; 2])
                                    .max_height(dialog_h - theme::DIALOG_TOP_OFFSET)
                                    .show(ui, |ui| {
                                        for (idx, entry) in filtered.iter().enumerate() {
                                            let is_selected = idx == self.palette_selected;
                                            let item_w = dialog_w - theme::SP_4 * 2.0;

                                            let resp = ui_kit::list_item(
                                                ui,
                                                egui::Id::new(("cmd_item", entry.action)),
                                                item_w,
                                                is_selected,
                                                |painter, row_rect| {
                                                    render_command_row(
                                                        painter,
                                                        row_rect,
                                                        entry,
                                                        is_selected,
                                                        &t,
                                                    );
                                                },
                                            );

                                            if resp.hovered() && !is_selected {
                                                self.palette_selected = idx;
                                            }
                                            if resp.clicked() {
                                                action_to_run = Some(entry.action);
                                            }
                                            if is_selected {
                                                resp.scroll_to_me(Some(egui::Align::Center));
                                            }
                                        }
                                    });
                            }
                        } else {
                            // ── File mode ──
                            let query = self.palette_query.trim();

                            if query.is_empty() {
                                // Show recent files
                                if self.recent_files.is_empty() {
                                    ui.add_space(theme::SP_4);
                                    ui.label(
                                        egui::RichText::new(
                                            "Type to search files, or > for commands",
                                        )
                                        .size(theme::FONT_UI_SM)
                                        .color(t.overlay0),
                                    );
                                } else {
                                    ui.add_space(theme::SP_1);
                                    ui.label(
                                        egui::RichText::new("RECENT FILES")
                                            .size(theme::FONT_UI_XS)
                                            .color(t.fg_muted),
                                    );
                                    ui.add_space(theme::SP_1);

                                    let count = self.recent_files.len();
                                    if self.palette_selected >= count {
                                        self.palette_selected = count.saturating_sub(1);
                                    }
                                    if up && self.palette_selected > 0 {
                                        self.palette_selected -= 1;
                                    }
                                    if down && self.palette_selected + 1 < count {
                                        self.palette_selected += 1;
                                    }

                                    // Clone paths to avoid borrow issues
                                    let recent_snapshot: Vec<std::path::PathBuf> =
                                        self.recent_files.clone();

                                    egui::ScrollArea::vertical()
                                        .id_source(self.vp_id("cmd_palette_scroll"))
                                        .auto_shrink([false; 2])
                                        .max_height(dialog_h - theme::DIALOG_TOP_OFFSET)
                                        .show(ui, |ui| {
                                            for (idx, path) in
                                                recent_snapshot.iter().enumerate()
                                            {
                                                let is_selected =
                                                    idx == self.palette_selected;
                                                let item_w = dialog_w - theme::SP_4 * 2.0;

                                                let resp = render_file_row(
                                                    ui,
                                                    path,
                                                    is_selected,
                                                    item_w,
                                                    &t,
                                                );

                                                if resp.hovered() && !is_selected {
                                                    self.palette_selected = idx;
                                                }
                                                if resp.clicked() || (enter && is_selected) {
                                                    file_to_open = Some(path.clone());
                                                }
                                                if ctrl_enter && is_selected {
                                                    file_to_open_external =
                                                        Some(path.clone());
                                                }
                                                if is_selected {
                                                    resp.scroll_to_me(Some(
                                                        egui::Align::Center,
                                                    ));
                                                }
                                            }
                                        });
                                }
                            } else {
                                // Debounced file search
                                self.palette_debouncer.update(query);
                                if self.palette_debouncer.ready() {
                                    let root = self.palette_search_root();
                                    self.workers
                                        .file_search_worker
                                        .search(query.to_string(), root);
                                }
                                if self.palette_debouncer.pending() {
                                    ctx.request_repaint_after(
                                        std::time::Duration::from_millis(16),
                                    );
                                }

                                let results = self.workers.file_search_worker.results();
                                let matches_snapshot: Vec<(std::path::PathBuf, bool)> =
                                    results
                                        .matches
                                        .iter()
                                        .map(|m| (m.path.clone(), m.is_dir))
                                        .collect();
                                let completed = results.completed;
                                drop(results);

                                let count = matches_snapshot.len();
                                if count == 0 {
                                    ui.add_space(theme::SP_4);
                                    let msg = if completed {
                                        "No matching files"
                                    } else {
                                        "Searching\u{2026}"
                                    };
                                    ui.label(
                                        egui::RichText::new(msg)
                                            .size(theme::FONT_UI_SM)
                                            .color(t.overlay0),
                                    );
                                } else {
                                    if self.palette_selected >= count {
                                        self.palette_selected = count.saturating_sub(1);
                                    }
                                    if up && self.palette_selected > 0 {
                                        self.palette_selected -= 1;
                                    }
                                    if down && self.palette_selected + 1 < count {
                                        self.palette_selected += 1;
                                    }

                                    egui::ScrollArea::vertical()
                                        .id_source(self.vp_id("cmd_palette_scroll"))
                                        .auto_shrink([false; 2])
                                        .max_height(dialog_h - theme::DIALOG_TOP_OFFSET)
                                        .show(ui, |ui| {
                                            for (idx, (path, _is_dir)) in
                                                matches_snapshot.iter().enumerate()
                                            {
                                                let is_selected =
                                                    idx == self.palette_selected;
                                                let item_w = dialog_w - theme::SP_4 * 2.0;

                                                let resp = render_file_row(
                                                    ui,
                                                    path,
                                                    is_selected,
                                                    item_w,
                                                    &t,
                                                );

                                                if resp.hovered() && !is_selected {
                                                    self.palette_selected = idx;
                                                }
                                                if resp.clicked() || (enter && is_selected) {
                                                    file_to_open = Some(path.clone());
                                                }
                                                if ctrl_enter && is_selected {
                                                    file_to_open_external =
                                                        Some(path.clone());
                                                }
                                                if is_selected {
                                                    resp.scroll_to_me(Some(
                                                        egui::Align::Center,
                                                    ));
                                                }
                                            }
                                        });
                                }
                            }
                        }

                        // Footer bar with keyboard hints
                        ui.add_space(theme::SP_2);
                        let sep_rect =
                            ui.allocate_space(egui::vec2(ui.available_width(), 1.0)).1;
                        ui.painter().rect_filled(sep_rect, 0.0, t.border_subtle);
                        ui.add_space(theme::SP_2);

                        ui.horizontal(|ui| {
                            let hint_color = t.fg_muted;
                            let hint_font = egui::FontId::proportional(theme::FONT_UI_XS);

                            let hints = if command_mode {
                                "\u{2191}\u{2193} navigate  \u{23CE} run  Esc close"
                            } else {
                                "\u{2191}\u{2193} navigate  \u{23CE} open  Ctrl+\u{23CE} external  Esc close  > commands"
                            };
                            ui.label(
                                egui::RichText::new(hints)
                                    .font(hint_font)
                                    .color(hint_color),
                            );
                        });
                    });
            });

        // Handle file opens after render (to avoid borrow issues)
        if let Some(path) = file_to_open {
            crate::app::persistence::push_recent_file(&mut self.recent_files, &path);
            crate::app::persistence::save_recent_files(&self.recent_files);
            self.close_palette();
            self.pending_palette_open_file = Some(path);
        } else if let Some(path) = file_to_open_external {
            crate::app::persistence::push_recent_file(&mut self.recent_files, &path);
            crate::app::persistence::save_recent_files(&self.recent_files);
            self.close_palette();
            let _ = open::that(&path);
        }

        if let Some(action) = action_to_run {
            self.close_palette();
            self.execute_palette_action(action, ctx);
        }
    }

    fn close_palette(&mut self) {
        self.palette_open = false;
        self.palette_query.clear();
        self.palette_selected = 0;
        self.palette_debouncer.reset();
        self.workers.file_search_worker.cancel();
    }

    fn palette_search_root(&self) -> std::path::PathBuf {
        if let Some(ws) = self.active_workspace() {
            if ws.path.is_dir() {
                return ws.path.clone();
            }
        }
        if let Some(cwd) = self.active_pane_cwd() {
            return cwd;
        }
        std::path::PathBuf::from(".")
    }

    fn execute_palette_action(&mut self, action: AppAction, ctx: &egui::Context) {
        match action {
            AppAction::ToggleLeftSidebar => self.show_left_panel = !self.show_left_panel,
            AppAction::ToggleRightSidebar => self.show_right_panel = !self.show_right_panel,
            AppAction::FocusTerminal => {
                ctx.memory_mut(|m| m.surrender_focus(egui::Id::NULL));
            }
            AppAction::NewTerminalTab => {
                self.deferred_spawn = Some(self.configured_shell());
            }
            AppAction::CloseCurrentPane => {
                self.deferred_close_pane = true;
            }
            AppAction::OpenSettings => {
                self.show_settings = !self.show_settings;
            }
            AppAction::ToggleShortcutHelp => self.show_shortcut_help = !self.show_shortcut_help,
            AppAction::OpenQuickSwitcher => {
                self.show_quick_switcher = !self.show_quick_switcher;
            }
            AppAction::SplitHorizontal => {
                self.deferred_split = Some(crate::pane_tree::SplitDir::Horizontal);
            }
            AppAction::SplitVertical => {
                self.deferred_split = Some(crate::pane_tree::SplitDir::Vertical);
            }
            AppAction::ZoomPane => {
                if self.zoomed_pane_id.is_some() {
                    self.zoomed_pane_id = None;
                } else {
                    self.zoomed_pane_id = self.pane_state.active_pane_id;
                }
            }
            AppAction::FocusSessionSearch => {
                self.show_left_panel = true;
                self.session_search_active = true;
            }
            AppAction::FocusFileSearch | AppAction::RightTabDirectory => {
                self.show_right_panel = true;
                self.right_tab = super::super::pane::RightTab::Directory;
                self.dir_search_active = true;
            }
            AppAction::RightTabGitDiff => {
                self.show_right_panel = true;
                self.right_tab = super::super::pane::RightTab::GitDiff;
            }
            AppAction::ToggleNotes => {
                self.notes_panel_collapsed = !self.notes_panel_collapsed;
            }
            AppAction::SearchTerminal => {
                let is_terminal = self
                    .pane_state
                    .active_pane_id
                    .and_then(|pid| self.pane_state.panes.iter().find(|p| p.id == pid))
                    .map(|p| matches!(p.content, PaneContent::Terminal(_)))
                    .unwrap_or(true);
                if is_terminal {
                    self.text_search.clear();
                    self.term_search.active = !self.term_search.active;
                    if !self.term_search.active {
                        self.term_search.query.clear();
                        self.term_search.matches.clear();
                        self.term_search.current_index = None;
                    }
                } else {
                    self.term_search.active = false;
                    self.term_search.query.clear();
                    self.term_search.matches.clear();
                    self.term_search.current_index = None;
                    self.text_search.active = !self.text_search.active;
                    if !self.text_search.active {
                        self.text_search.clear();
                    }
                }
            }
            AppAction::SearchAllSessions => {
                self.show_global_search = !self.show_global_search;
                if self.show_global_search {
                    self.show_left_panel = true;
                    self.session_search_active = false;
                    self.session_search_query.clear();
                }
            }
            AppAction::DuplicateSession => {
                self.deferred_duplicate = true;
            }
            AppAction::NextWorkspace if self.current_window_id.is_none() => {
                let ws_ids: Vec<u64> = self
                    .workspace_store
                    .workspaces
                    .iter()
                    .filter(|w| w.host_window_id.is_none())
                    .map(|w| w.id)
                    .collect();
                if !ws_ids.is_empty() {
                    let cur = self
                        .active_group
                        .and_then(|g| ws_ids.iter().position(|&id| id == g))
                        .unwrap_or(0);
                    let next = (cur + 1) % ws_ids.len();
                    self.deferred_open_workspace = Some(ws_ids[next]);
                }
            }
            AppAction::PrevWorkspace if self.current_window_id.is_none() => {
                let ws_ids: Vec<u64> = self
                    .workspace_store
                    .workspaces
                    .iter()
                    .filter(|w| w.host_window_id.is_none())
                    .map(|w| w.id)
                    .collect();
                if !ws_ids.is_empty() {
                    let cur = self
                        .active_group
                        .and_then(|g| ws_ids.iter().position(|&id| id == g))
                        .unwrap_or(0);
                    let prev = if cur == 0 { ws_ids.len() - 1 } else { cur - 1 };
                    self.deferred_open_workspace = Some(ws_ids[prev]);
                }
            }
            AppAction::FocusNextGroup
            | AppAction::FocusPrevGroup
            | AppAction::FocusGroupUp
            | AppAction::FocusGroupDown => {
                use crate::editor_group::NavigationDir;
                let nav = match action {
                    AppAction::FocusNextGroup => NavigationDir::Right,
                    AppAction::FocusPrevGroup => NavigationDir::Left,
                    AppAction::FocusGroupUp => NavigationDir::Up,
                    AppAction::FocusGroupDown => NavigationDir::Down,
                    _ => unreachable!(),
                };
                if let Some(target) = self
                    .pane_state
                    .group_layout
                    .spatial_neighbor(self.pane_state.focused_group_id, nav)
                {
                    self.pane_state.focused_group_id = target;
                    if let Some(g) = self.pane_state.groups.get(&target) {
                        self.pane_state.active_pane_id = g.active_pane_id;
                        if let Some(pid) = g.active_pane_id {
                            if let Some(pane) = self.pane_state.find(pid) {
                                if let PaneContent::Terminal(sid) = pane.content {
                                    self.session_state.active_id = Some(sid);
                                }
                            }
                        }
                    }
                    self.update_is_active_flags();
                    ctx.request_repaint();
                }
            }
            AppAction::MoveTabToNextGroup
            | AppAction::MoveTabToPrevGroup
            | AppAction::MoveTabToUpGroup
            | AppAction::MoveTabToDownGroup => {
                use crate::editor_group::NavigationDir;
                let nav = match action {
                    AppAction::MoveTabToNextGroup => NavigationDir::Right,
                    AppAction::MoveTabToPrevGroup => NavigationDir::Left,
                    AppAction::MoveTabToUpGroup => NavigationDir::Up,
                    AppAction::MoveTabToDownGroup => NavigationDir::Down,
                    _ => unreachable!(),
                };
                if let Some(pid) = self.pane_state.active_pane_id {
                    if let Some(target_gid) = self
                        .pane_state
                        .group_layout
                        .spatial_neighbor(self.pane_state.focused_group_id, nav)
                    {
                        self.pane_state.move_pane_to_group(pid, target_gid, None);
                        self.pane_state.focused_group_id = target_gid;
                        if let Some(g) = self.pane_state.groups.get(&target_gid) {
                            self.pane_state.active_pane_id = g.active_pane_id;
                        }
                        ctx.request_repaint();
                    }
                }
            }
            AppAction::RevealInExplorer => {
                if let Some(path) = self.active_pane_file_path() {
                    self.reveal_file_path = Some(path);
                    self.show_right_panel = true;
                    self.right_tab = super::super::pane::RightTab::Directory;
                }
            }
            AppAction::CommandPalette | AppAction::OpenFileFinder => {}
            _ => {}
        }
    }
}

/// Render a single file row in the palette (used for both recent files and search results).
fn render_file_row(
    ui: &mut egui::Ui,
    path: &std::path::Path,
    is_selected: bool,
    width: f32,
    t: &crate::theme::Theme,
) -> egui::Response {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();

    ui_kit::list_item(
        ui,
        egui::Id::new(("file_item", path.to_string_lossy().as_ref())),
        width,
        is_selected,
        |painter, row_rect| {
            // Extension badge (left)
            let badge_text = if ext.is_empty() {
                "\u{2022}".to_string() // bullet for no extension
            } else {
                ext.clone()
            };
            let badge_font = egui::FontId::monospace(theme::FONT_UI_XS);
            let badge_galley =
                painter.layout_no_wrap(badge_text, badge_font, t.accent_muted);
            let badge_w = badge_galley.size().x + theme::SP_2 * 2.0;
            let badge_h = badge_galley.size().y + theme::SP_1;
            let badge_rect = egui::Rect::from_min_size(
                egui::pos2(
                    row_rect.min.x + theme::SP_3,
                    row_rect.center().y - badge_h / 2.0,
                ),
                egui::vec2(badge_w, badge_h),
            );
            let badge_bg = if is_selected { t.surface2 } else { t.surface0 };
            painter.rect_filled(badge_rect, theme::R_SM, badge_bg);
            painter.galley(
                egui::pos2(
                    badge_rect.min.x + theme::SP_2,
                    badge_rect.center().y - badge_galley.size().y / 2.0,
                ),
                badge_galley,
                t.accent_muted,
            );

            // File name (center)
            let name_x = badge_rect.max.x + theme::SP_3;
            let name_color = if is_selected { t.text } else { t.subtext0 };
            painter.text(
                egui::pos2(name_x, row_rect.center().y - theme::FONT_UI_SM * 0.55),
                egui::Align2::LEFT_TOP,
                &file_name,
                egui::FontId::proportional(theme::FONT_UI_SM),
                name_color,
            );

            // Parent path (right-aligned)
            if !parent.is_empty() {
                let parent_font = egui::FontId::proportional(theme::FONT_UI_XS);
                let parent_galley =
                    painter.layout_no_wrap(parent.clone(), parent_font, t.fg_muted);
                // Truncate if it would overlap the file name
                let max_parent_w = (row_rect.max.x - name_x - theme::SP_6)
                    .max(0.0)
                    .min(parent_galley.size().x);
                if max_parent_w > 40.0 {
                    painter.galley(
                        egui::pos2(
                            row_rect.max.x - theme::SP_3 - max_parent_w,
                            row_rect.center().y - parent_galley.size().y / 2.0,
                        ),
                        parent_galley,
                        t.fg_muted,
                    );
                }
            }
        },
    )
}

/// Render label + shortcut badge for a command row.
fn render_command_row(
    painter: &egui::Painter,
    row_rect: egui::Rect,
    entry: &PaletteEntry,
    is_selected: bool,
    t: &crate::theme::Theme,
) {
    let label_pos = egui::pos2(
        row_rect.min.x + theme::SP_3,
        row_rect.center().y - theme::FONT_UI_SM * 0.55,
    );
    painter.text(
        label_pos,
        egui::Align2::LEFT_TOP,
        &entry.label,
        egui::FontId::proportional(theme::FONT_UI_SM),
        if is_selected { t.text } else { t.subtext0 },
    );

    if let Some(ref hint) = entry.shortcut_hint {
        let badge_font = egui::FontId::monospace(theme::FONT_SYS_SM);
        let badge_text_color = if is_selected { t.subtext1 } else { t.fg_muted };
        let badge_galley =
            painter.layout_no_wrap(hint.clone(), badge_font, badge_text_color);
        let badge_w = badge_galley.size().x + theme::SP_3 * 2.0;
        let badge_h = badge_galley.size().y + theme::SP_1 * 2.0;
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(
                row_rect.max.x - theme::SP_3 - badge_w,
                row_rect.center().y - badge_h / 2.0,
            ),
            egui::vec2(badge_w, badge_h),
        );
        let badge_bg = if is_selected { t.surface2 } else { t.surface0 };
        painter.rect_filled(badge_rect, theme::R_SM, badge_bg);
        painter.galley(
            egui::pos2(
                badge_rect.min.x + theme::SP_3,
                badge_rect.center().y - badge_galley.size().y / 2.0,
            ),
            badge_galley,
            badge_text_color,
        );
    }
}

struct PaletteEntry {
    action: AppAction,
    label: String,
    label_lower: String,
    shortcut_hint: Option<String>,
}

fn all_palette_actions(registry: &ShortcutRegistry) -> Vec<PaletteEntry> {
    use AppAction::*;
    let actions = [
        ToggleLeftSidebar,
        ToggleRightSidebar,
        FocusTerminal,
        NewTerminalTab,
        CloseCurrentPane,
        SplitHorizontal,
        SplitVertical,
        OpenSettings,
        NextWorkspace,
        PrevWorkspace,
        RightTabDirectory,
        RightTabGitDiff,
        ToggleNotes,
        DuplicateSession,
        CopySelection,
        FocusSessionSearch,
        FocusFileSearch,
        ToggleShortcutHelp,
        OpenQuickSwitcher,
        SearchTerminal,
        SearchAllSessions,
        ZoomPane,
        ReopenClosedSession,
        FocusNextGroup,
        FocusPrevGroup,
        FocusGroupUp,
        FocusGroupDown,
        MoveTabToNextGroup,
        MoveTabToPrevGroup,
        MoveTabToUpGroup,
        MoveTabToDownGroup,
        RevealInExplorer,
    ];

    actions
        .into_iter()
        .map(|action| {
            let label = action.description().to_string();
            let label_lower = label.to_lowercase();
            let shortcut_hint = registry.find_shortcut(action).map(|s| s.label());
            PaletteEntry {
                action,
                label,
                label_lower,
                shortcut_hint,
            }
        })
        .collect()
}

fn fuzzy_match(haystack: &str, query: &str) -> bool {
    let mut hay_chars = haystack.chars();
    for qc in query.chars() {
        loop {
            match hay_chars.next() {
                Some(hc) if hc == qc => break,
                Some(_) => continue,
                None => return false,
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_match_basic() {
        assert!(fuzzy_match("toggle left sidebar", "tls"));
        assert!(fuzzy_match("toggle left sidebar", "toggle"));
        assert!(fuzzy_match("split horizontal", "sph"));
        assert!(!fuzzy_match("split horizontal", "xyz"));
    }

    #[test]
    fn fuzzy_match_empty_query() {
        assert!(fuzzy_match("anything", ""));
    }

    #[test]
    fn all_palette_actions_non_empty() {
        let registry = ShortcutRegistry::new();
        let actions = all_palette_actions(&registry);
        assert!(!actions.is_empty());
        for entry in &actions {
            assert!(!entry.label.is_empty());
        }
    }

    #[test]
    fn palette_entry_labels_unique() {
        let registry = ShortcutRegistry::new();
        let actions = all_palette_actions(&registry);
        let mut labels: Vec<&str> = actions.iter().map(|e| e.label.as_str()).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), actions.len());
    }
}
