use std::sync::Arc;

use super::super::feedback;
use super::super::file_browser;
use super::super::markdown::render_markdown;
use super::super::pane::{NoteEditorState, PaneContent, PaneEntry, SessionEntry, TermSelection};
use super::super::settings::CursorStyle;
use super::super::App;
use super::tab_bar::GroupTabBarResult;
use crate::editor_group::{GroupId, GroupNode};
use crate::pane_tree::{split_rect, SplitDir};
use crate::renderer::terminal_pass::TerminalGeometry;
use crate::syntax;
use crate::theme;

fn text_fingerprint(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

#[derive(Clone)]
struct SyntaxGalleyCache {
    text_hash: u64,
    theme_id: crate::theme::ThemeId,
    galley: std::sync::Arc<egui::Galley>,
}

/// Actions emitted by the 3-dot context menu on split panes.
#[allow(dead_code)]
pub(in crate::app) enum PaneContextAction {
    MoveToTab(u32),
    Close(u32),
    SplitHorizontal(u32),
    SplitVertical(u32),
    ConflictResolve {
        pane_id: u32,
        action: super::conflict_resolver::ConflictAction,
    },
}

/// Mutable context threaded through the recursive pane renderer.
///
/// This struct exists so we can pass references to output accumulators and
/// read-only state without capturing `&mut self`.
#[allow(dead_code)]
pub(in crate::app) struct RenderCtx<'a> {
    pub sessions: &'a [SessionEntry],
    pub panes: &'a [PaneEntry],
    pub editor_texts: &'a mut Vec<(u32, Option<String>)>,
    pub cursor_alpha: f32,
    pub focused_pane_id: Option<u32>,
    pub active_term_geo: &'a mut Option<TerminalGeometry>,
    pub active_term_ui_id: &'a mut Option<egui::Id>,
    pub clicked_pane_id: &'a mut Option<u32>,
    pub editor_saves: &'a mut Vec<u32>,
    pub editor_preview_toggles: &'a mut Vec<u32>,
    pub pane_widths_snap: &'a mut Vec<(u32, f32)>,
    pub split_ratio_changes: &'a mut Vec<(u32, f32)>,
    pub pane_context_actions: &'a mut Vec<PaneContextAction>,
    pub term_selection: &'a Option<TermSelection>,
    pub term_selection_sid: Option<u32>,
    pub workspace_dialog_open: bool,
    pub workspace_edit_dialog_open: bool,
    pub show_settings: bool,
    pub font_size: f32,
    pub cursor_style: CursorStyle,
    pub has_splits: bool,
    pub flash: &'a crate::app::feedback::FlashManager,
    pub text_search: &'a mut crate::search::TextSearchState,
    pub diff_mode_changes: &'a mut Vec<(u32, super::super::diff_parser::DiffViewMode)>,
    pub diff_hunk_navigations: &'a mut Vec<(u32, usize)>,
    pub drag_state: &'a mut crate::app::drag::DragState,
    pub scrollbar_clear_restore: &'a mut Vec<u32>,
    pub scrollbar_dragging: &'a mut bool,
    /// (pane_id, new side_by_side value) toggled this frame by the conflict resolver toolbar.
    pub conflict_view_toggles: &'a mut Vec<(u32, bool)>,
}

// ── Leaf renderers ──────────────────────────────────────────────────────────

fn render_terminal_leaf(
    ui: &mut egui::Ui,
    sid: u32,
    _pane_id: u32,
    is_focused: bool,
    rctx: &mut RenderCtx<'_>,
) {
    if let Some(idx) = rctx.sessions.iter().position(|e| e.id == sid) {
        let session = Arc::clone(&rctx.sessions[idx].session);
        let sel_range = if rctx.term_selection_sid == Some(sid) {
            rctx.term_selection
                .as_ref()
                .map(|s| crate::renderer::terminal_pass::SelectionRange {
                    start_col: s.start_col,
                    start_row: s.start_row,
                    end_col: s.end_col,
                    end_row: s.end_row,
                    display_offset: s.display_offset,
                })
        } else {
            None
        };
        let mut geo = crate::renderer::terminal_pass::TerminalView::new(Arc::clone(&session)).show(
            ui,
            is_focused,
            rctx.cursor_alpha,
            sel_range.as_ref(),
            rctx.font_size,
            rctx.cursor_style,
        );
        if let Some(target_offset) = geo.scrollbar_drag_offset {
            use alacritty_terminal::grid::Scroll;
            if let Some(mut s) = session.try_write() {
                let current = s.term.grid().display_offset();
                let delta = target_offset as i32 - current as i32;
                if delta != 0 {
                    s.term.scroll_display(Scroll::Delta(delta));
                }
            }
            rctx.scrollbar_clear_restore.push(sid);
        }
        if geo.scrollbar_drag_offset.is_some() {
            *rctx.scrollbar_dragging = true;
        }
        if geo.scrollbar_hovered || geo.scrollbar_drag_offset.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }
        let pointer_in_rect = ui.input(|i| {
            i.pointer
                .latest_pos()
                .map(|p| geo.rect.contains(p))
                .unwrap_or(false)
        });
        geo.session_id = Some(sid);
        if pointer_in_rect || (is_focused && rctx.active_term_geo.is_none()) {
            *rctx.active_term_geo = Some(geo);
        }
    }
    if is_focused {
        let this_id = ui.id();
        *rctx.active_term_ui_id = Some(this_id);
        let dialog_open =
            rctx.workspace_dialog_open || rctx.workspace_edit_dialog_open || rctx.show_settings;
        if !dialog_open {
            let clicked = ui.input(|i| i.pointer.any_pressed())
                && ui.input(|i| {
                    i.pointer
                        .latest_pos()
                        .map(|p| ui.max_rect().contains(p))
                        .unwrap_or(false)
                });
            if clicked {
                ui.memory_mut(|m| m.request_focus(this_id));
            } else {
                let other_focused =
                    ui.memory(|m| m.focused().map(|id| id != this_id).unwrap_or(false));
                if !other_focused {
                    ui.memory_mut(|m| m.request_focus(this_id));
                }
            }
        }
    }
}

/// Render the floating text-search bar for non-terminal panes.
/// Returns the 0-based line number of the current match (if any) so callers
/// can scroll to it.
fn render_text_search_bar(
    ui: &mut egui::Ui,
    pane_rect: egui::Rect,
    content: &str,
    search: &mut crate::search::TextSearchState,
) -> Option<usize> {
    if !search.active {
        return None;
    }
    let t = theme::active();
    let bar_w = 320.0_f32.min(pane_rect.width() - 24.0).max(120.0);
    if pane_rect.width() < 148.0 {
        return None;
    }
    let bar_h = 30.0_f32;
    let bar_rect = egui::Rect::from_min_size(
        egui::pos2(pane_rect.max.x - bar_w - 8.0, pane_rect.min.y + 8.0),
        egui::vec2(bar_w, bar_h),
    );
    ui.painter().rect_filled(bar_rect, theme::R_MD, t.surface0);
    ui.painter()
        .rect_stroke(bar_rect, theme::R_MD, egui::Stroke::new(1.0, t.overlay0));

    let input_rect = egui::Rect::from_min_max(
        egui::pos2(bar_rect.min.x + 6.0, bar_rect.min.y + 4.0),
        egui::pos2(bar_rect.max.x - 90.0, bar_rect.max.y - 4.0),
    );
    let resp = ui.put(
        input_rect,
        egui::TextEdit::singleline(&mut search.query)
            .desired_width(input_rect.width())
            .font(egui::FontId::monospace(theme::FONT_UI_MD))
            .hint_text("Search\u{2026}"),
    );
    if resp.changed() {
        search.search(content);
    }
    resp.request_focus();

    if ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.shift) {
        search.prev_match();
    } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        search.next_match();
    }

    let controls_x = bar_rect.max.x - 90.0;
    ui.painter().line_segment(
        [
            egui::pos2(controls_x, bar_rect.min.y + theme::SP_2),
            egui::pos2(controls_x, bar_rect.max.y - theme::SP_2),
        ],
        egui::Stroke::new(theme::STROKE_THIN, t.border_subtle),
    );

    let count_text = if search.matches.is_empty() {
        if search.query.is_empty() {
            String::new()
        } else {
            "0/0".to_string()
        }
    } else {
        format!(
            "{}/{}",
            search.current_index.unwrap_or(0) + 1,
            search.matches.len()
        )
    };
    ui.painter().text(
        egui::pos2(bar_rect.max.x - 48.0, bar_rect.center().y),
        egui::Align2::CENTER_CENTER,
        &count_text,
        egui::FontId::monospace(theme::FONT_UI_SM),
        t.subtext0,
    );

    search.current_match().map(|m| m.line)
}

fn render_file_editor_leaf(
    ui: &mut egui::Ui,
    ed: &super::super::pane::FileEditorState,
    pane_id: u32,
    rctx: &mut RenderCtx<'_>,
) {
    let pane_rect = ui.max_rect();
    ui.painter()
        .rect_filled(pane_rect, 0.0, theme::active().bg_term);
    if ed.loading {
        render_loading_indicator(ui, &ed.path);
        return;
    }
    if !file_browser::is_supported_text_file(&ed.path, &ed.content) {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("Binary file — cannot display as text")
                    .size(theme::FONT_STATUS)
                    .color(theme::active().overlay0),
            );
        });
    } else {
        let is_md = ed.path.extension().and_then(|e| e.to_str()) == Some("md");
        let previewing = is_md && ed.show_preview;
        if is_md {
            ui.horizontal(|ui| {
                let t = theme::active();
                let raw_color = if !previewing { t.text } else { t.overlay0 };
                let preview_color = if previewing { t.text } else { t.overlay0 };
                let raw_bg = if !previewing {
                    t.surface2
                } else {
                    egui::Color32::TRANSPARENT
                };
                let preview_bg = if previewing {
                    t.surface2
                } else {
                    egui::Color32::TRANSPARENT
                };
                let raw_resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new("Raw")
                            .size(theme::FONT_UI_SM)
                            .color(raw_color),
                    )
                    .fill(raw_bg)
                    .rounding(egui::Rounding::same(theme::R_MD))
                    .min_size(egui::vec2(56.0, 20.0)),
                );
                if raw_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if raw_resp.clicked() && previewing {
                    rctx.editor_preview_toggles.push(pane_id);
                }
                ui.add_space(theme::SP_1);
                let preview_resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new("Preview")
                            .size(theme::FONT_UI_SM)
                            .color(preview_color),
                    )
                    .fill(preview_bg)
                    .rounding(egui::Rounding::same(theme::R_MD))
                    .min_size(egui::vec2(56.0, 20.0)),
                );
                if preview_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if preview_resp.clicked() && !previewing {
                    rctx.editor_preview_toggles.push(pane_id);
                }
            });
            ui.separator();
        }

        let is_focused = rctx.focused_pane_id == Some(pane_id);
        let scroll_target = if is_focused {
            rctx.text_search.current_match().map(|m| m.line)
        } else {
            None
        };

        if previewing {
            if let Some(et) = rctx.editor_texts.iter().find(|(id, _)| *id == pane_id) {
                if let Some(ref text) = et.1 {
                    egui::ScrollArea::both()
                        .id_source(("editor_preview_scroll", pane_id))
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            render_markdown(ui, text);
                        });
                }
            }
        } else if let Some(et) = rctx.editor_texts.iter_mut().find(|(id, _)| *id == pane_id) {
            if let Some(ref mut text) = et.1 {
                let maybe_syntax = syntax::find_syntax_for_file(&ed.path);

                egui::ScrollArea::both()
                    .id_source(("editor_scroll", pane_id))
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        let line_count = if text.ends_with('\n') {
                            text.lines().count() + 1
                        } else {
                            text.lines().count()
                        }
                        .max(1);
                        let digits = ((line_count as f64).log10().floor() as usize) + 1;
                        let char_w = 7.5_f32;
                        let gutter_w = (digits as f32 + 1.5) * char_w;
                        let line_h = ui.text_style_height(&egui::TextStyle::Monospace);

                        let gutter_total_h = theme::SP_1 + line_count as f32 * line_h;
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;

                            // Virtual line-number gutter: allocate full height,
                            // paint only visible numbers.
                            ui.vertical(|ui| {
                                ui.set_min_width(gutter_w);
                                let (gutter_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(gutter_w, gutter_total_h),
                                    egui::Sense::hover(),
                                );
                                let clip = ui.clip_rect();
                                let base_y = gutter_rect.min.y + theme::SP_1;
                                let first =
                                    ((clip.min.y - base_y) / line_h).floor().max(0.0) as usize;
                                let last = ((clip.max.y - base_y) / line_h)
                                    .ceil()
                                    .min(line_count as f32)
                                    as usize;
                                let gutter_font = egui::FontId::monospace(
                                    ui.style().text_styles[&egui::TextStyle::Monospace].size,
                                );
                                let gutter_color = theme::active().overlay0;
                                for n in (first + 1)..=(last.min(line_count)) {
                                    let y = base_y + (n - 1) as f32 * line_h + line_h / 2.0;
                                    ui.painter().text(
                                        egui::pos2(gutter_rect.max.x - 4.0, y),
                                        egui::Align2::RIGHT_CENTER,
                                        format!("{:>width$}", n, width = digits),
                                        gutter_font.clone(),
                                        gutter_color,
                                    );
                                }
                            });

                            let sep_rect = ui
                                .allocate_exact_size(
                                    egui::vec2(1.0, gutter_total_h),
                                    egui::Sense::hover(),
                                )
                                .0;
                            ui.painter()
                                .rect_filled(sep_rect, 0.0, theme::active().surface1);
                            ui.add_space(theme::SP_2);

                            if let Some(syn) = maybe_syntax {
                                let cache_id = egui::Id::new(("syn_galley_cache", pane_id));
                                let mut layouter = |ui: &egui::Ui, s: &str, wrap_width: f32| {
                                    let hash = text_fingerprint(s);
                                    let current_theme = theme::active().id;
                                    let cached: Option<SyntaxGalleyCache> =
                                        ui.ctx().data(|d| d.get_temp(cache_id));
                                    if let Some(c) = cached {
                                        if c.text_hash == hash && c.theme_id == current_theme {
                                            return c.galley;
                                        }
                                    }
                                    let job = syntax::highlight_layout_job(ui, s, syn, wrap_width);
                                    let galley = ui.fonts(|f| f.layout_job(job));
                                    ui.ctx().data_mut(|d| {
                                        d.insert_temp(
                                            cache_id,
                                            SyntaxGalleyCache {
                                                text_hash: hash,
                                                theme_id: current_theme,
                                                galley: galley.clone(),
                                            },
                                        )
                                    });
                                    galley
                                };
                                ui.add(
                                    egui::TextEdit::multiline(text)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY)
                                        .frame(false)
                                        .layouter(&mut layouter),
                                );
                            } else {
                                ui.add(
                                    egui::TextEdit::multiline(text)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY)
                                        .frame(false),
                                );
                            }

                            if let Some(target_line) = scroll_target {
                                let target_y = theme::SP_1 + target_line as f32 * line_h;
                                let scroll_rect = egui::Rect::from_min_size(
                                    egui::pos2(0.0, target_y),
                                    egui::vec2(1.0, line_h),
                                );
                                ui.scroll_to_rect(scroll_rect, Some(egui::Align::Center));
                            }
                        });
                    });
            }
        }

        if is_focused && rctx.text_search.active {
            let current_text = rctx
                .editor_texts
                .iter()
                .find(|(id, _)| *id == pane_id)
                .and_then(|(_, t)| t.as_ref())
                .unwrap_or(&ed.content);
            render_text_search_bar(ui, pane_rect, current_text, rctx.text_search);
        }

        if ui.input(|inp| inp.modifiers.ctrl && inp.key_pressed(egui::Key::S)) {
            rctx.editor_saves.push(pane_id);
        }
    }
}

fn render_note_editor_leaf(
    ui: &mut egui::Ui,
    ne: &NoteEditorState,
    pane_id: u32,
    rctx: &mut RenderCtx<'_>,
) {
    let pane_rect = ui.max_rect();
    let t = theme::active();
    ui.painter().rect_filled(pane_rect, 0.0, t.bg_term);

    let label = match ne.workspace_id {
        Some(_) => "Workspace Notes",
        None => "General Notes",
    };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .strong()
                .size(theme::FONT_UI_LG)
                .color(t.text),
        );
    });
    ui.separator();

    let is_focused = rctx.focused_pane_id == Some(pane_id);
    let scroll_target = if is_focused {
        rctx.text_search.current_match().map(|m| m.line)
    } else {
        None
    };

    if let Some(et) = rctx.editor_texts.iter_mut().find(|(id, _)| *id == pane_id) {
        if let Some(ref mut text) = et.1 {
            let content_snapshot = if is_focused && rctx.text_search.active {
                Some(text.clone())
            } else {
                None
            };

            egui::ScrollArea::both()
                .id_source(("note_editor_scroll", pane_id))
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let line_h = ui.text_style_height(&egui::TextStyle::Monospace);
                    ui.add(
                        egui::TextEdit::multiline(text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .hint_text("Notes for this workspace\u{2026}")
                            .frame(false),
                    );
                    if let Some(target_line) = scroll_target {
                        let target_y = target_line as f32 * line_h;
                        let scroll_rect = egui::Rect::from_min_size(
                            egui::pos2(0.0, target_y),
                            egui::vec2(1.0, line_h),
                        );
                        ui.scroll_to_rect(scroll_rect, Some(egui::Align::Center));
                    }
                });

            if let Some(content) = content_snapshot {
                render_text_search_bar(ui, pane_rect, &content, rctx.text_search);
            }
        }
    }
}

#[derive(Default)]
struct DiffLeafResult {
    mode_change: Option<super::super::diff_parser::DiffViewMode>,
    hunk_navigation: Option<usize>,
}

fn render_file_diff_leaf(
    ui: &mut egui::Ui,
    d: &super::super::pane::FileDiffState,
    pane_id: u32,
    rctx: &mut RenderCtx<'_>,
) -> DiffLeafResult {
    let pane_rect = ui.max_rect();
    ui.painter()
        .rect_filled(pane_rect, 0.0, theme::active().bg_term);
    if d.loading {
        render_loading_indicator(ui, &d.path);
        return DiffLeafResult::default();
    }
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("\u{21c4} {}", d.path.display()))
                .strong()
                .size(theme::FONT_UI_LG)
                .color(theme::active().git_filename),
        );
    });
    ui.separator();

    let toolbar =
        super::super::git_diff::render_diff_toolbar(ui, d.diff_mode, d.hunks.len(), d.current_hunk);
    ui.separator();

    let is_focused = rctx.focused_pane_id == Some(pane_id);

    egui::ScrollArea::both()
        .id_source(("diff_scroll", pane_id))
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            match d.diff_mode {
                super::super::diff_parser::DiffViewMode::Inline => {
                    super::super::git_diff::render_inline_diff_full(
                        ui,
                        &d.old_content,
                        &d.new_content,
                        &d.hunks,
                        d.old_highlights.as_deref(),
                        d.new_highlights.as_deref(),
                        toolbar.scroll_to_hunk,
                    );
                }
                super::super::diff_parser::DiffViewMode::SideBySide => {
                    super::super::git_diff::render_side_by_side_diff(
                        ui,
                        &d.old_content,
                        &d.new_content,
                        &d.hunks,
                        d.old_highlights.as_deref(),
                        d.new_highlights.as_deref(),
                        toolbar.scroll_to_hunk,
                    );
                }
            }
            if is_focused {
                if let Some(m) = rctx.text_search.current_match() {
                    let line_h = ui.text_style_height(&egui::TextStyle::Monospace);
                    let spacing = ui.spacing().item_spacing.y;
                    let target_y = m.line as f32 * (line_h + spacing);
                    let scroll_rect = egui::Rect::from_min_size(
                        egui::pos2(0.0, target_y),
                        egui::vec2(1.0, line_h),
                    );
                    ui.scroll_to_rect(scroll_rect, Some(egui::Align::Center));
                }
            }
        });

    if is_focused && rctx.text_search.active {
        render_text_search_bar(ui, pane_rect, &d.new_content, rctx.text_search);
    }

    DiffLeafResult {
        mode_change: toolbar.mode_change,
        hunk_navigation: toolbar.scroll_to_hunk,
    }
}

fn render_loading_indicator(ui: &mut egui::Ui, path: &std::path::Path) {
    let t = theme::active();
    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy())
        .unwrap_or_default();
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.35);
        let elapsed = ui.input(|i| i.time) as f32;
        let dots = match ((elapsed * 2.0) as usize) % 4 {
            0 => "",
            1 => ".",
            2 => "..",
            _ => "...",
        };
        ui.label(
            egui::RichText::new(format!("Loading {filename}{dots}"))
                .size(theme::FONT_STATUS)
                .color(t.overlay0),
        );
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    });
}

// ── Group-based rendering (new editor groups system) ──────────────────────

/// Render the content of a single pane leaf. Extracted so it can be reused
/// by both the old `render_node` Leaf case and the new group renderer.
fn render_leaf_content(
    ui: &mut egui::Ui,
    pane_id: u32,
    is_focused: bool,
    rect: egui::Rect,
    rctx: &mut RenderCtx<'_>,
) {
    let pane = rctx.panes.iter().find(|p| p.id == pane_id);
    let Some(pane) = pane else { return };

    // Track width for resize
    rctx.pane_widths_snap.push((pane_id, rect.width()));

    ui.allocate_ui_at_rect(rect, |ui| {
        ui.push_id(pane_id, |ui| match &pane.content {
            PaneContent::Terminal(sid) => {
                render_terminal_leaf(ui, *sid, pane_id, is_focused, rctx);
            }
            PaneContent::DeferredTerminal { .. } => {
                ui.painter()
                    .rect_filled(ui.max_rect(), 0.0, theme::active().bg_term);
            }
            PaneContent::FileEditor(ed) => {
                render_file_editor_leaf(ui, ed, pane_id, rctx);
            }
            PaneContent::FileDiff(d) => {
                let result = render_file_diff_leaf(ui, d, pane_id, rctx);
                if let Some(new_mode) = result.mode_change {
                    rctx.diff_mode_changes.push((pane_id, new_mode));
                }
                if let Some(hunk_idx) = result.hunk_navigation {
                    rctx.diff_hunk_navigations.push((pane_id, hunk_idx));
                }
            }
            PaneContent::NoteEditor(ne) => {
                render_note_editor_leaf(ui, ne, pane_id, rctx);
            }
            PaneContent::ConflictResolver(ref state) => {
                let result = super::conflict_resolver::render_conflict_resolver(ui, state);
                if let Some(ca) = result.action {
                    rctx.pane_context_actions
                        .push(PaneContextAction::ConflictResolve {
                            pane_id,
                            action: ca,
                        });
                }
                if let Some(sbs) = result.view_toggle {
                    rctx.conflict_view_toggles.push((pane_id, sbs));
                }
            }
        });
    });

    // Flash feedback overlay
    rctx.flash.render_on_rect(
        ui.painter(),
        rect,
        crate::app::feedback::FlashTarget::Pane(pane_id),
    );

    // Drop target highlight for drag-and-drop
    if rctx.drag_state.is_active() {
        let accepts = matches!(
            &rctx.drag_state.payload,
            Some(crate::app::drag::DragPayload::Tab(_))
                | Some(crate::app::drag::DragPayload::Session(_))
                | Some(crate::app::drag::DragPayload::File(_))
                | Some(crate::app::drag::DragPayload::Diff(_))
                | Some(crate::app::drag::DragPayload::Note(_))
        );
        if accepts {
            if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
                if rect.contains(pos) {
                    ui.painter().rect_stroke(
                        rect.shrink(2.0),
                        crate::theme::R_SM,
                        egui::Stroke::new(2.0, crate::theme::active().blue.gamma_multiply(0.6)),
                    );
                    rctx.drag_state.drop_target = Some(crate::app::drag::DropTarget::PaneArea);
                }
            }
        }
    }
}

/// Recursively render the group layout tree.
#[allow(clippy::too_many_arguments)]
fn render_group_node(
    app: &mut App,
    ui: &mut egui::Ui,
    node: &GroupNode,
    rect: egui::Rect,
    editor_texts: &mut Vec<(u32, Option<String>)>,
    clicked_pane_id: &mut Option<u32>,
    clicked_group_id: &mut Option<GroupId>,
    editor_saves: &mut Vec<u32>,
    editor_preview_toggles: &mut Vec<u32>,
    pane_widths_snap: &mut Vec<(u32, f32)>,
    split_ratio_changes: &mut Vec<(u32, f32)>,
    pane_context_actions: &mut Vec<PaneContextAction>,
    group_tab_results: &mut Vec<GroupTabBarResult>,
) {
    match node {
        GroupNode::Leaf { group_id } => {
            let group_id = *group_id;
            let is_focused = app.pane_state.focused_group_id == group_id;
            let group = app.pane_state.groups.get(&group_id).cloned();

            // Scope all leaf rendering under a group_id-keyed child UI so that
            // auto-generated widget IDs are unique across sibling groups in a
            // split.  Without this, two leaves share the parent `ui` and
            // `allocate_ui_at_rect` children get the same base `id`, leading to
            // egui "duplicate widget ID" warnings for interactive widgets.
            let mut leaf_ui = ui.child_ui_with_id_source(rect, *ui.layout(), group_id, None);

            let Some(group) = group else {
                leaf_ui
                    .painter()
                    .rect_filled(rect, 0.0, theme::active().bg_term);
                leaf_ui.allocate_ui_at_rect(rect, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new("Empty group")
                                .color(theme::active().overlay0)
                                .size(theme::FONT_TERM),
                        );
                    });
                });
                return;
            };

            let has_live_panes = !group.is_empty()
                && group
                    .pane_ids
                    .iter()
                    .any(|pid| app.pane_state.panes.iter().any(|p| p.id == *pid));
            if !has_live_panes {
                leaf_ui
                    .painter()
                    .rect_filled(rect, 0.0, theme::active().bg_term);
                return;
            }

            // Split rect into tab bar (top) and content (rest)
            let tab_h = theme::HEADER_H;
            let tab_bar_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), tab_h));
            let content_rect = egui::Rect::from_min_size(
                egui::pos2(rect.min.x, rect.min.y + tab_h),
                egui::vec2(rect.width(), (rect.height() - tab_h).max(0.0)),
            );

            // Render per-group tab bar
            let tab_result = app.render_group_tab_bar(
                &mut leaf_ui,
                &group,
                group_id,
                is_focused,
                tab_h,
                tab_bar_rect,
            );
            group_tab_results.push(tab_result);

            // Render the active pane's content
            let active_pid = group.active_pane_id;
            if let Some(pid) = active_pid {
                let has_splits = app.pane_state.groups.len() > 1;
                let mut diff_mode_changes = Vec::new();
                let mut diff_hunk_navigations: Vec<(u32, usize)> = Vec::new();
                let mut scrollbar_clear_restore = Vec::new();
                let mut scrollbar_dragging = false;
                let mut conflict_view_toggles: Vec<(u32, bool)> = Vec::new();
                let focused_pane_id = if is_focused { Some(pid) } else { None };

                {
                    let mut rctx = RenderCtx {
                        sessions: &app.session_state.sessions,
                        panes: &app.pane_state.panes,
                        editor_texts,
                        cursor_alpha: app.cursor_alpha,
                        focused_pane_id,
                        active_term_geo: &mut app.active_term_geo,
                        active_term_ui_id: &mut app.active_term_ui_id,
                        clicked_pane_id,
                        editor_saves,
                        editor_preview_toggles,
                        pane_widths_snap,
                        split_ratio_changes,
                        pane_context_actions,
                        term_selection: &app.term_selection,
                        term_selection_sid: app.term_selection_sid,
                        workspace_dialog_open: app.workspace_dialog.is_some(),
                        workspace_edit_dialog_open: app.workspace_edit_dialog.is_some(),
                        show_settings: app.show_settings,
                        font_size: app.settings.font_size,
                        cursor_style: app.settings.cursor_style,
                        has_splits,
                        flash: &app.flash,
                        text_search: &mut app.text_search,
                        diff_mode_changes: &mut diff_mode_changes,
                        diff_hunk_navigations: &mut diff_hunk_navigations,
                        drag_state: &mut app.drag_state,
                        scrollbar_clear_restore: &mut scrollbar_clear_restore,
                        scrollbar_dragging: &mut scrollbar_dragging,
                        conflict_view_toggles: &mut conflict_view_toggles,
                    };
                    render_leaf_content(&mut leaf_ui, pid, is_focused, content_rect, &mut rctx);
                }

                // Apply diff mode changes
                for (pane_id, new_mode) in diff_mode_changes {
                    if let Some(pane) = app.pane_state.panes.iter_mut().find(|p| p.id == pane_id) {
                        if let PaneContent::FileDiff(ref mut d) = pane.content {
                            d.diff_mode = new_mode;
                        }
                    }
                    app.settings.diff_view_mode = new_mode;
                    app.settings.save();
                }
                for (pane_id, hunk_idx) in diff_hunk_navigations {
                    if let Some(pane) = app.pane_state.panes.iter_mut().find(|p| p.id == pane_id) {
                        if let PaneContent::FileDiff(ref mut d) = pane.content {
                            d.current_hunk = hunk_idx;
                        }
                    }
                }
                for sid in scrollbar_clear_restore {
                    if let Some(entry) = app.session_state.sessions.iter_mut().find(|e| e.id == sid)
                    {
                        entry.restore_scroll_ready = false;
                        entry.restore_scroll_lines = None;
                    }
                }
                for (pane_id, sbs) in conflict_view_toggles {
                    if let Some(pane) = app.pane_state.panes.iter_mut().find(|p| p.id == pane_id) {
                        if let PaneContent::ConflictResolver(ref mut s) = pane.content {
                            s.side_by_side = sbs;
                        }
                    }
                }
            }

            // Convert PaneArea to GroupArea for group-aware drop routing
            if matches!(
                app.drag_state.drop_target,
                Some(crate::app::drag::DropTarget::PaneArea)
            ) {
                app.drag_state.drop_target =
                    Some(crate::app::drag::DropTarget::GroupArea(group_id));
            }

            // Click-to-focus: if clicked in this group's area, set focus
            let any_drag = leaf_ui
                .ctx()
                .input(|inp| inp.pointer.any_down() && inp.pointer.is_decidedly_dragging());
            if !any_drag
                && leaf_ui
                    .ctx()
                    .input(|inp| inp.pointer.button_clicked(egui::PointerButton::Primary))
            {
                if let Some(pos) = leaf_ui.ctx().input(|inp| inp.pointer.interact_pos()) {
                    if rect.contains(pos) {
                        *clicked_group_id = Some(group_id);
                    }
                }
            }
        }
        GroupNode::Split {
            split_id,
            dir,
            ratio,
            a,
            b,
        } => {
            let (rect_a, div_rect, rect_b) = split_rect(rect, *dir, *ratio);
            render_group_node(
                app,
                ui,
                a,
                rect_a,
                editor_texts,
                clicked_pane_id,
                clicked_group_id,
                editor_saves,
                editor_preview_toggles,
                pane_widths_snap,
                split_ratio_changes,
                pane_context_actions,
                group_tab_results,
            );
            render_group_node(
                app,
                ui,
                b,
                rect_b,
                editor_texts,
                clicked_pane_id,
                clicked_group_id,
                editor_saves,
                editor_preview_toggles,
                pane_widths_snap,
                split_ratio_changes,
                pane_context_actions,
                group_tab_results,
            );

            // Draw divider — reuse existing divider rendering
            let div_id = egui::Id::new(("group_split_div", *split_id));
            let div_resp = ui.interact(div_rect, div_id, egui::Sense::drag());
            let is_active = div_resp.dragged() || div_resp.hovered();
            let anim_t =
                ui.ctx()
                    .animate_bool_with_time(div_id.with("anim"), is_active, theme::ANIM_FAST);
            let t = theme::active();
            let drag_t = ui.ctx().animate_bool_with_time(
                div_id.with("drag"),
                div_resp.dragged(),
                theme::ANIM_FAST,
            );
            let line_width =
                theme::STROKE_THIN + (theme::STROKE_MEDIUM - theme::STROKE_THIN) * drag_t;
            let line_color = theme::lerp_color(t.border_subtle, t.border_focus, anim_t);
            match dir {
                SplitDir::Horizontal => {
                    let cx = div_rect.center().x;
                    ui.painter().line_segment(
                        [
                            egui::pos2(cx, div_rect.min.y),
                            egui::pos2(cx, div_rect.max.y),
                        ],
                        egui::Stroke::new(line_width, line_color),
                    );
                }
                SplitDir::Vertical => {
                    let cy = div_rect.center().y;
                    ui.painter().line_segment(
                        [
                            egui::pos2(div_rect.min.x, cy),
                            egui::pos2(div_rect.max.x, cy),
                        ],
                        egui::Stroke::new(line_width, line_color),
                    );
                }
            }
            // Grab handle dots
            if anim_t > 0.01 {
                let dot_color = theme::lerp_color(
                    egui::Color32::TRANSPARENT,
                    theme::lerp_color(t.overlay0, t.fg_secondary, anim_t),
                    anim_t,
                );
                let center = div_rect.center();
                match dir {
                    SplitDir::Horizontal => {
                        for i in [-1.0_f32, 0.0, 1.0] {
                            ui.painter().circle_filled(
                                egui::pos2(center.x, center.y + i * theme::DOT_GAP),
                                theme::DOT_R,
                                dot_color,
                            );
                        }
                    }
                    SplitDir::Vertical => {
                        for i in [-1.0_f32, 0.0, 1.0] {
                            ui.painter().circle_filled(
                                egui::pos2(center.x + i * theme::DOT_GAP, center.y),
                                theme::DOT_R,
                                dot_color,
                            );
                        }
                    }
                }
            }

            // Handle drag to resize
            if div_resp.dragged() {
                let delta = div_resp.drag_delta();
                let (extent, movement, min_pane) = match dir {
                    SplitDir::Horizontal => {
                        (rect.width(), delta.x / rect.width(), theme::MIN_PANE_W)
                    }
                    SplitDir::Vertical => {
                        (rect.height(), delta.y / rect.height(), theme::MIN_PANE_H)
                    }
                };
                let min_ratio = if extent > 0.0 {
                    (min_pane / extent).clamp(0.1, 0.4)
                } else {
                    0.1
                };
                let new_ratio = (*ratio + movement).clamp(min_ratio, 1.0 - min_ratio);
                split_ratio_changes.push((*split_id, new_ratio));
            }

            // Cursor feedback
            let cursor = match dir {
                SplitDir::Horizontal => egui::CursorIcon::ResizeHorizontal,
                SplitDir::Vertical => egui::CursorIcon::ResizeVertical,
            };
            if div_resp.hovered() || div_resp.dragged() {
                ui.ctx().set_cursor_icon(cursor);
            }
        }
    }
}

impl App {
    /// New entry point: renders the entire group layout.
    ///
    /// Walks the GroupNode tree, rendering per-group tab bars and content.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::app) fn render_group_content(
        &mut self,
        ui: &mut egui::Ui,
        content_rect: egui::Rect,
        editor_texts: &mut Vec<(u32, Option<String>)>,
        clicked_pane_id: &mut Option<u32>,
        clicked_group_id: &mut Option<GroupId>,
        editor_saves: &mut Vec<u32>,
        editor_preview_toggles: &mut Vec<u32>,
        pane_widths_snap: &mut Vec<(u32, f32)>,
        split_ratio_changes: &mut Vec<(u32, f32)>,
        pane_context_actions: &mut Vec<PaneContextAction>,
        group_tab_results: &mut Vec<GroupTabBarResult>,
    ) {
        // Clear stale terminal geometry
        self.active_term_geo = None;

        let layout = self.pane_state.group_layout.clone();
        render_group_node(
            self,
            ui,
            &layout,
            content_rect,
            editor_texts,
            clicked_pane_id,
            clicked_group_id,
            editor_saves,
            editor_preview_toggles,
            pane_widths_snap,
            split_ratio_changes,
            pane_context_actions,
            group_tab_results,
        );

        // Global flash overlay (rare — PTY spawn errors)
        self.flash
            .render_on_rect(ui.painter(), content_rect, feedback::FlashTarget::Global);

        // ── File drag hover overlay ────────────────────────────────────────
        let hovering_files = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        if hovering_files {
            let t = theme::active();
            let painter = ui.painter();
            painter.rect_filled(
                content_rect,
                theme::R_MD,
                egui::Color32::from_rgba_unmultiplied(
                    t.surface0.r(),
                    t.surface0.g(),
                    t.surface0.b(),
                    180,
                ),
            );
            painter.rect_stroke(
                content_rect.shrink(theme::SP_1),
                4.0,
                egui::Stroke::new(2.0, t.blue),
            );
            painter.text(
                content_rect.center(),
                egui::Align2::CENTER_CENTER,
                "Drop file(s) to paste path",
                egui::FontId::proportional(theme::FONT_STATUS),
                t.text,
            );
        }
    }
}
