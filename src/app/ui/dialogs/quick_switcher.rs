use crate::app::pane::{PaneContent, PaneEntry, SessionEntry};
use crate::app::App;
use crate::shortcuts::AppAction;
use crate::{theme, ui_kit};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::borrow::Cow;
use std::sync::Arc;

fn switcher_row_title(entry: &SessionEntry) -> Cow<'_, str> {
    let Some(session) = entry.session.try_read() else {
        return Cow::Borrowed(entry.shell.display_name());
    };
    let title = session.title();
    let normalized = title.trim();
    let is_default = normalized.is_empty()
        || normalized.starts_with("Session ")
        || normalized.eq_ignore_ascii_case(entry.shell.name())
        || normalized.eq_ignore_ascii_case(entry.shell.display_name());
    if is_default {
        Cow::Borrowed(entry.shell.display_name())
    } else {
        Cow::Owned(normalized.to_owned())
    }
}

fn short_cwd(path: &str, max_w_chars: usize) -> String {
    if path.chars().count() <= max_w_chars {
        return path.to_owned();
    }

    let separator = if path.contains('\\') { '\\' } else { '/' };
    let components: Vec<_> = path
        .split(['/', '\\'])
        .filter(|component| !component.is_empty())
        .collect();
    let mut suffix = String::new();
    for component in components.iter().rev() {
        let candidate = if suffix.is_empty() {
            (*component).to_owned()
        } else {
            format!("{component}{separator}{suffix}")
        };
        if candidate.chars().count() + 2 > max_w_chars {
            break;
        }
        suffix = candidate;
    }

    if suffix.is_empty() {
        let tail: String = path
            .chars()
            .rev()
            .take(max_w_chars.saturating_sub(1))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("\u{2026}{tail}")
    } else {
        format!("\u{2026}{separator}{suffix}")
    }
}

fn row_hint(picked_ws: Option<usize>, idx: usize) -> Option<char> {
    picked_ws
        .filter(|_| idx < 26)
        .map(|_| (b'a' + idx as u8) as char)
}

#[derive(Clone)]
struct SwitcherEntry {
    pane_id: u32,
    title: String,
    cwd: String,
    is_active: bool,
}

#[derive(Clone)]
struct SwitcherGroup {
    workspace_id: Option<u64>,
    name: String,
    color: [u8; 3],
    entries: Vec<SwitcherEntry>,
    last_activated: u64,
}

#[derive(Clone)]
struct QuickSwitcherModel {
    query: String,
    groups: Vec<SwitcherGroup>,
}

impl App {
    fn switcher_collect_entry(&self, pane: &PaneEntry) -> Option<SwitcherEntry> {
        let (title, cwd) = match &pane.content {
            PaneContent::Terminal(session_id) => {
                let entry = self.session_state.find(*session_id)?;
                let title = switcher_row_title(entry).into_owned();
                let cwd = entry
                    .session
                    .try_read()
                    .map(|session| short_cwd(&session.cwd.to_string_lossy(), 48))
                    .unwrap_or_default();
                (title, cwd)
            }
            PaneContent::DeferredTerminal {
                cwd, saved_title, ..
            } => (
                saved_title
                    .as_deref()
                    .filter(|title| !title.is_empty())
                    .unwrap_or("Terminal")
                    .to_owned(),
                cwd.as_ref()
                    .map(|path| short_cwd(&path.to_string_lossy(), 48))
                    .unwrap_or_default(),
            ),
            PaneContent::FileEditor(editor) => (
                editor
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Editor")
                    .to_owned(),
                short_cwd(&editor.path.to_string_lossy(), 48),
            ),
            PaneContent::FileDiff(diff) => (
                format!(
                    "Diff: {}",
                    diff.path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("file")
                ),
                short_cwd(&diff.path.to_string_lossy(), 48),
            ),
            PaneContent::NoteEditor(_) => ("Notes".to_owned(), String::new()),
            PaneContent::ConflictResolver(resolver) => (
                format!(
                    "Conflicts: {}",
                    resolver
                        .path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("file")
                ),
                short_cwd(&resolver.path.to_string_lossy(), 48),
            ),
        };
        Some(SwitcherEntry {
            pane_id: pane.id,
            title,
            cwd,
            is_active: self.is_pane_active_in_any_window(pane.id),
        })
    }

    fn build_quick_switcher_model(&self, query: &str) -> QuickSwitcherModel {
        let matcher = SkimMatcherV2::default();
        let normalized_query = query.trim().to_lowercase();
        let mut groups = Vec::new();

        for workspace in &self.workspace_store.workspaces {
            let entries: Vec<_> = self
                .pane_state
                .panes
                .iter()
                .filter(|pane| {
                    Self::pane_group(&self.session_state.sessions, &self.workspace_store, pane)
                        == Some(workspace.id)
                })
                .filter_map(|pane| self.switcher_collect_entry(pane))
                .filter(|entry| {
                    normalized_query.is_empty()
                        || matcher
                            .fuzzy_match(
                                &format!("{} {} {}", workspace.name, entry.title, entry.cwd),
                                &normalized_query,
                            )
                            .is_some()
                })
                .collect();
            if !entries.is_empty() {
                groups.push(SwitcherGroup {
                    workspace_id: Some(workspace.id),
                    name: workspace.name.clone(),
                    color: workspace.color,
                    entries,
                    last_activated: workspace.last_activated,
                });
            }
        }

        let entries: Vec<_> = self
            .pane_state
            .panes
            .iter()
            .filter(|pane| {
                Self::pane_group(&self.session_state.sessions, &self.workspace_store, pane)
                    .is_none()
            })
            .filter_map(|pane| self.switcher_collect_entry(pane))
            .filter(|entry| {
                normalized_query.is_empty()
                    || matcher
                        .fuzzy_match(
                            &format!("Other {} {}", entry.title, entry.cwd),
                            &normalized_query,
                        )
                        .is_some()
            })
            .collect();
        if !entries.is_empty() {
            let other_color = theme::active().fg_muted_raised;
            groups.push(SwitcherGroup {
                workspace_id: None,
                name: "Other".to_owned(),
                color: [other_color.r(), other_color.g(), other_color.b()],
                entries,
                last_activated: 0,
            });
        }

        groups.sort_by_key(|group| std::cmp::Reverse(group.last_activated));
        QuickSwitcherModel {
            query: normalized_query,
            groups,
        }
    }

    pub(in crate::app) fn render_quick_switcher(&mut self, ctx: &egui::Context) {
        let model_id = self.vp_id("quick_switcher_model");
        if !self.show_quick_switcher {
            ctx.data_mut(|data| data.remove::<Arc<QuickSwitcherModel>>(model_id));
            return;
        }

        let mut close_switcher = false;
        let mut switch_to_workspace = None;
        let mut switch_to_pane = None;
        let screen_rect = ctx.screen_rect();
        let t = theme::active();

        egui::Area::new(self.vp_id("quick_switcher_dim"))
            .fixed_pos(screen_rect.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let response = ui.interact(
                    screen_rect,
                    self.vp_id("quick_switcher_dim_click"),
                    egui::Sense::click(),
                );
                ui.painter().rect_filled(
                    screen_rect,
                    theme::R_NONE,
                    egui::Color32::from_black_alpha(theme::ALPHA_OVERLAY_DIM),
                );
                close_switcher = response.clicked();
            });

        let dialog_width =
            theme::QUICK_SWITCHER_W.min((screen_rect.width() - theme::SP_6 * 2.0).max(theme::SP_6));
        let content_width = (dialog_width - theme::SP_6 * 2.0).max(theme::SP_6);
        let dialog_height = (screen_rect.height() - theme::SP_6 * 2.0)
            .clamp(theme::DIALOG_ITEM_H, theme::QUICK_SWITCHER_MAX_H);

        egui::Area::new(self.vp_id("quick_switcher_dialog"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(t.bg_raised)
                    .rounding(theme::R_LG)
                    .stroke(egui::Stroke::new(theme::STROKE_THIN, t.border))
                    .shadow(egui::epaint::Shadow {
                        offset: egui::vec2(theme::SP_0, theme::SP_1),
                        blur: theme::SP_6,
                        spread: theme::SP_2,
                        color: t.shadow_md,
                    })
                    .inner_margin(egui::Margin::same(theme::SP_6))
                    .show(ui, |ui| {
                        ui.set_width(content_width);
                        ui.set_max_height(dialog_height);

                        let escape = ctx.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                        });
                        if escape {
                            if self.quick_switcher_search_active
                                && !self.quick_switcher_query.is_empty()
                            {
                                self.quick_switcher_query.clear();
                            } else {
                                close_switcher = true;
                            }
                        }

                        let mut number_pressed = None;
                        if !self.quick_switcher_search_active {
                            let number_keys = [
                                egui::Key::Num1,
                                egui::Key::Num2,
                                egui::Key::Num3,
                                egui::Key::Num4,
                                egui::Key::Num5,
                                egui::Key::Num6,
                                egui::Key::Num7,
                                egui::Key::Num8,
                                egui::Key::Num9,
                            ];
                            for (index, key) in number_keys.iter().enumerate() {
                                if ctx.input_mut(|input| {
                                    input.consume_key(egui::Modifiers::NONE, *key)
                                }) {
                                    number_pressed = Some(index);
                                    break;
                                }
                            }
                            if number_pressed.is_some() {
                                ctx.input_mut(|input| {
                                    input
                                        .events
                                        .retain(|event| !matches!(event, egui::Event::Text(_)));
                                });
                            }
                        }

                        let mut letter_pressed = None;
                        if !self.quick_switcher_search_active
                            && self.quick_switcher_selected_ws.is_some()
                        {
                            let letter_keys = [
                                egui::Key::A,
                                egui::Key::B,
                                egui::Key::C,
                                egui::Key::D,
                                egui::Key::E,
                                egui::Key::F,
                                egui::Key::G,
                                egui::Key::H,
                                egui::Key::I,
                                egui::Key::J,
                                egui::Key::K,
                                egui::Key::L,
                                egui::Key::M,
                                egui::Key::N,
                                egui::Key::O,
                                egui::Key::P,
                                egui::Key::Q,
                                egui::Key::R,
                                egui::Key::S,
                                egui::Key::T,
                                egui::Key::U,
                                egui::Key::V,
                                egui::Key::W,
                                egui::Key::X,
                                egui::Key::Y,
                                egui::Key::Z,
                            ];
                            for (index, key) in letter_keys.iter().enumerate() {
                                if ctx.input_mut(|input| {
                                    input.consume_key(egui::Modifiers::NONE, *key)
                                }) {
                                    letter_pressed = Some(index);
                                    break;
                                }
                            }
                            if letter_pressed.is_some() {
                                ctx.input_mut(|input| {
                                    input
                                        .events
                                        .retain(|event| !matches!(event, egui::Event::Text(_)));
                                });
                            }
                        }

                        if !self.quick_switcher_search_active
                            && ctx.input_mut(|input| {
                                input.consume_key(egui::Modifiers::NONE, egui::Key::Slash)
                            })
                        {
                            ctx.input_mut(|input| {
                                input
                                    .events
                                    .retain(|event| !matches!(event, egui::Event::Text(_)));
                            });
                            self.quick_switcher_search_active = true;
                            self.quick_switcher_selected_ws = None;
                        }
                        let enter = ctx.input_mut(|input| {
                            input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        });

                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("Quick Switcher")
                                    .font(egui::FontId::new(
                                        theme::FONT_LG,
                                        crate::fonts::ui_bold(),
                                    ))
                                    .color(t.fg_strong),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if let Some(shortcut) = self
                                        .shortcut_registry
                                        .find_shortcut(AppAction::OpenQuickSwitcher)
                                    {
                                        ui_kit::kbd_combo(ui, shortcut);
                                    }
                                },
                            );
                        });
                        ui.add_space(theme::SP_4);

                        let search_frame = egui::Frame::none()
                            .fill(t.bg_base)
                            .rounding(theme::R_MD)
                            .stroke(egui::Stroke::new(theme::STROKE_THIN, t.border))
                            .inner_margin(egui::Margin::symmetric(theme::SP_3, theme::SP_2));
                        let search_id = self.vp_id("quick_switcher_search");
                        let search_response = search_frame
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(ui_kit::icon(
                                        ui_kit::icon::SEARCH,
                                        theme::ICON_MD,
                                        t.fg_muted_raised,
                                    ));
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.quick_switcher_query)
                                            .id(search_id)
                                            .desired_width(f32::INFINITY)
                                            .hint_text("Search sessions and workspaces")
                                            .frame(false),
                                    )
                                })
                                .inner
                            })
                            .inner;
                        if search_response.clicked() {
                            self.quick_switcher_search_active = true;
                            self.quick_switcher_selected_ws = None;
                        }
                        if self.quick_switcher_search_active && !search_response.has_focus() {
                            search_response.request_focus();
                        }
                        ui.add_space(theme::SP_4);

                        let normalized_query = self.quick_switcher_query.trim().to_lowercase();
                        let needs_model = ctx.data(|data| {
                            data.get_temp::<Arc<QuickSwitcherModel>>(model_id)
                                .map_or(true, |model| model.query != normalized_query)
                        });
                        if needs_model {
                            let model = self.build_quick_switcher_model(&normalized_query);
                            ctx.data_mut(|data| data.insert_temp(model_id, Arc::new(model)));
                        }
                        let model = ctx
                            .data(|data| data.get_temp::<Arc<QuickSwitcherModel>>(model_id))
                            .unwrap_or_else(|| {
                                Arc::new(QuickSwitcherModel {
                                    query: normalized_query,
                                    groups: Vec::new(),
                                })
                            });

                        if let Some(index) = number_pressed {
                            if index < model.groups.len() {
                                self.quick_switcher_selected_ws = Some(index);
                            }
                        }
                        if let (Some(group_index), Some(entry_index)) =
                            (self.quick_switcher_selected_ws, letter_pressed)
                        {
                            switch_to_pane = model
                                .groups
                                .get(group_index)
                                .and_then(|group| group.entries.get(entry_index))
                                .map(|entry| entry.pane_id);
                        }
                        if enter {
                            switch_to_pane = model
                                .groups
                                .iter()
                                .flat_map(|group| &group.entries)
                                .next()
                                .map(|entry| entry.pane_id);
                        }

                        let scroll_height =
                            (dialog_height - theme::SP_6 * 10.0).max(theme::DIALOG_ITEM_H);
                        egui::ScrollArea::vertical()
                            .id_source("quick_switcher_scroll")
                            .max_height(scroll_height)
                            .show(ui, |ui| {
                                if model.groups.is_empty() {
                                    ui.centered_and_justified(|ui| {
                                        ui.label(
                                            egui::RichText::new("No matching sessions")
                                                .size(theme::FONT_SM)
                                                .color(t.fg_muted_raised),
                                        );
                                    });
                                    return;
                                }

                                for (group_index, group) in model.groups.iter().enumerate() {
                                    let selected =
                                        self.quick_switcher_selected_ws == Some(group_index);
                                    let header = ui.horizontal(|ui| {
                                        ui_kit::kbd_chip(ui, &(group_index + 1).to_string());
                                        let dot_color = theme::from_rgb(group.color);
                                        let (dot_rect, _) = ui.allocate_exact_size(
                                            egui::vec2(theme::SP_2, theme::SP_2),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            dot_rect.center(),
                                            theme::SP_1,
                                            dot_color,
                                        );
                                        ui.label(
                                            egui::RichText::new(group.name.to_uppercase())
                                                .font(egui::FontId::new(
                                                    theme::FONT_XS,
                                                    crate::fonts::ui_bold(),
                                                ))
                                                .color(if selected {
                                                    t.fg_strong
                                                } else {
                                                    t.fg_muted_raised
                                                }),
                                        );
                                    });
                                    if header.response.interact(egui::Sense::click()).clicked() {
                                        switch_to_workspace =
                                            Some(group.workspace_id.unwrap_or(u64::MAX));
                                    }
                                    ui.add_space(theme::SP_2);

                                    for (entry_index, entry) in group.entries.iter().enumerate() {
                                        let hint = if selected {
                                            row_hint(self.quick_switcher_selected_ws, entry_index)
                                        } else {
                                            None
                                        };
                                        if ui_kit::selectable_row(
                                            ui,
                                            entry.is_active,
                                            theme::QUICK_SWITCHER_ROW_H,
                                            |ui| {
                                                ui.horizontal(|ui| {
                                                    egui::Frame::none()
                                                        .fill(t.bg_panel)
                                                        .rounding(theme::R_MD)
                                                        .inner_margin(egui::Margin::same(
                                                            theme::SP_2,
                                                        ))
                                                        .show(ui, |ui| {
                                                            ui.label(ui_kit::icon(
                                                                ui_kit::icon::TERMINAL,
                                                                theme::ICON_MD,
                                                                t.accent,
                                                            ));
                                                        });
                                                    ui.add_space(theme::SP_2);
                                                    ui.vertical(|ui| {
                                                        ui.label(
                                                            egui::RichText::new(&entry.title)
                                                                .font(egui::FontId::new(
                                                                    theme::FONT_MD,
                                                                    crate::fonts::ui_bold(),
                                                                ))
                                                                .color(t.fg_strong),
                                                        );
                                                        if !entry.cwd.is_empty() {
                                                            ui.label(
                                                                egui::RichText::new(&entry.cwd)
                                                                    .size(theme::FONT_SM)
                                                                    .color(t.fg_muted_raised),
                                                            );
                                                        }
                                                    });
                                                    if let Some(hint) = hint {
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::Center,
                                                            ),
                                                            |ui| {
                                                                ui_kit::kbd_chip(
                                                                    ui,
                                                                    &hint.to_string(),
                                                                );
                                                            },
                                                        );
                                                    }
                                                });
                                            },
                                        )
                                        .clicked()
                                        {
                                            switch_to_pane = Some(entry.pane_id);
                                        }
                                    }
                                    ui.add_space(theme::SP_4);
                                }
                            });

                        ui.add_space(theme::SP_3);
                        ui.horizontal(|ui| {
                            ui_kit::kbd_chip(ui, "1-9");
                            ui.label(ui_kit::label_muted("workspace"));
                            ui.add_space(theme::SP_4);
                            ui_kit::kbd_chip(ui, "Enter");
                            ui.label(ui_kit::label_muted("open"));
                            ui.add_space(theme::SP_4);
                            ui_kit::kbd_chip(ui, "Esc");
                            ui.label(ui_kit::label_muted("close"));
                        });
                    });
            });

        if let Some(workspace_id) = switch_to_workspace {
            self.navigate_to_workspace(workspace_id);
            close_switcher = true;
        }
        if let Some(pane_id) = switch_to_pane {
            self.navigate_to_pane(pane_id);
            close_switcher = true;
        }
        if close_switcher {
            self.show_quick_switcher = false;
            self.quick_switcher_query.clear();
            self.quick_switcher_selected_ws = None;
            self.quick_switcher_search_active = false;
            ctx.data_mut(|data| data.remove::<Arc<QuickSwitcherModel>>(model_id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::ShellKind;
    use crate::terminal::Session;
    use parking_lot::RwLock;
    use portable_pty::{native_pty_system, PtySize};
    use std::sync::atomic::AtomicBool;
    use std::sync::{mpsc, Arc};

    fn session_entry(shell: ShellKind, title: Option<&str>) -> SessionEntry {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("test PTY");
        let session = Session::new_for_test(1, 80, 24);
        if let Some(title) = title {
            session.set_title(title.to_owned());
        }
        let (pty_tx, _pty_rx) = mpsc::sync_channel(1);
        SessionEntry {
            id: 1,
            session: Arc::new(RwLock::new(session)),
            pty_tx,
            master: pair.master,
            shell_pid: 0,
            alive: Arc::new(AtomicBool::new(true)),
            is_active: Arc::new(AtomicBool::new(false)),
            pending_command: None,
            shell,
            restore_scroll_lines: None,
            restore_scroll_ready: false,
            restore_title: None,
            claude_session_id: None,
        }
    }

    #[test]
    fn switcher_row_title_uses_display_name_when_not_renamed() {
        let entry = session_entry(ShellKind::Zsh, None);
        assert_eq!(switcher_row_title(&entry), "Zsh");
    }

    #[test]
    fn switcher_row_title_keeps_user_rename() {
        let entry = session_entry(ShellKind::Bash, Some("Production logs"));
        assert_eq!(switcher_row_title(&entry), "Production logs");
    }

    #[test]
    fn short_cwd_keeps_last_components() {
        assert_eq!(
            short_cwd("/home/sid/projects/terminal-studio", 27),
            "\u{2026}/projects/terminal-studio"
        );
    }

    #[test]
    fn switcher_row_letters_hidden_until_workspace_picked() {
        assert_eq!(row_hint(None, 0), None);
        assert_eq!(row_hint(Some(0), 0), Some('a'));
        assert_eq!(row_hint(Some(0), 25), Some('z'));
        assert_eq!(row_hint(Some(0), 26), None);
    }
}
