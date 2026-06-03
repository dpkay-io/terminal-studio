use crate::shortcuts::{AppAction, ShortcutRegistry};
use crate::theme;
use crate::ui_kit;

use super::super::pane::PaneContent;
use super::super::App;

impl App {
    pub(in crate::app) fn render_command_palette(&mut self, ctx: &egui::Context) {
        if !self.show_command_palette {
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
                    self.close_command_palette();
                }
            });

        let dialog_w = (screen_rect.width() * 0.45).clamp(320.0, 520.0);
        let dialog_h = (screen_rect.height() * 0.55).clamp(200.0, 480.0);
        let dialog_pos = egui::pos2(
            screen_rect.center().x - dialog_w / 2.0,
            screen_rect.min.y + theme::DIALOG_TOP_OFFSET,
        );

        let mut action_to_run: Option<AppAction> = None;

        egui::Area::new(self.vp_id("cmd_palette_dialog"))
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
                        let esc = ctx
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                        if esc {
                            self.close_command_palette();
                            return;
                        }

                        // Up/down to navigate
                        let up = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                        });
                        let down = ctx.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                        });
                        let enter = ctx
                            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));

                        // Search input
                        let search_id = self.vp_id("cmd_palette_search");
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut self.command_palette_query)
                                .id(search_id)
                                .desired_width(dialog_w - theme::SP_4 * 2.0 - theme::SP_6)
                                .hint_text("Type a command\u{2026}")
                                .font(egui::FontId::monospace(theme::FONT_UI_MD)),
                        );
                        if resp.changed() {
                            self.command_palette_selected = 0;
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
                        ui.separator();
                        ui.add_space(theme::SP_1);

                        // Build filtered action list
                        let query = self.command_palette_query.trim().to_lowercase();
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
                            // Clamp selection
                            if self.command_palette_selected >= count {
                                self.command_palette_selected = count.saturating_sub(1);
                            }
                            if up && self.command_palette_selected > 0 {
                                self.command_palette_selected -= 1;
                            }
                            if down && self.command_palette_selected + 1 < count {
                                self.command_palette_selected += 1;
                            }
                            if enter {
                                action_to_run =
                                    Some(filtered[self.command_palette_selected].action);
                            }

                            egui::ScrollArea::vertical()
                                .id_source(self.vp_id("cmd_palette_scroll"))
                                .auto_shrink([false; 2])
                                .max_height(dialog_h - theme::DIALOG_TOP_OFFSET)
                                .show(ui, |ui| {
                                    for (idx, entry) in filtered.iter().enumerate() {
                                        let is_selected = idx == self.command_palette_selected;
                                        let item_w = dialog_w - theme::SP_4 * 2.0;

                                        let resp = ui_kit::list_item(
                                            ui,
                                            item_w,
                                            is_selected,
                                            |painter, row_rect| {
                                                // Action label
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

                                                // Keybinding hint (right-aligned)
                                                if let Some(ref hint) = entry.shortcut_hint {
                                                    let hint_pos = egui::pos2(
                                                        row_rect.max.x - theme::SP_3,
                                                        row_rect.center().y
                                                            - theme::FONT_UI_XS * 0.55,
                                                    );
                                                    painter.text(
                                                        hint_pos,
                                                        egui::Align2::RIGHT_TOP,
                                                        hint,
                                                        egui::FontId::monospace(theme::FONT_UI_XS),
                                                        t.overlay0,
                                                    );
                                                }
                                            },
                                        );

                                        if resp.hovered() && !is_selected {
                                            self.command_palette_selected = idx;
                                        }

                                        if resp.clicked() {
                                            action_to_run = Some(entry.action);
                                        }

                                        // Scroll selected into view
                                        if is_selected {
                                            resp.scroll_to_me(Some(egui::Align::Center));
                                        }
                                    }
                                });
                        }
                    });
            });

        if let Some(action) = action_to_run {
            self.close_command_palette();
            self.execute_palette_action(action, ctx);
        }
    }

    fn close_command_palette(&mut self) {
        self.show_command_palette = false;
        self.command_palette_query.clear();
        self.command_palette_selected = 0;
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
            AppAction::CommandPalette => {}
            _ => {}
        }
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
