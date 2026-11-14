use std::sync::Arc;

use super::super::feedback;
use super::super::file_browser;
use super::super::markdown::render_markdown;
use super::super::pane::{NoteEditorState, PaneContent, PaneEntry, SessionEntry, TermSelection};
use super::super::settings::{CursorStyle, InfoBarPosition};
use super::super::{App, CloseAllTarget};
use super::info_bar::InfoBarAction;
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
    pub pane_rects_snap: &'a mut Vec<(u32, egui::Rect)>,
    pub split_ratio_changes: &'a mut Vec<(u32, f32)>,
    pub pane_context_actions: &'a mut Vec<PaneContextAction>,
    pub term_selection: &'a Option<TermSelection>,
    pub term_selection_sid: Option<u32>,
    /// A dialog or overlay owns the keyboard (`App::any_modal_open`): no
    /// pane takes focus or keys meanwhile.
    pub modal_open: bool,
    pub font_size: f32,
    pub cursor_style: CursorStyle,
    pub has_splits: bool,
    pub flash: &'a crate::app::feedback::FlashManager,
    pub text_search: &'a mut crate::search::TextSearchState,
    pub diff_mode_changes: &'a mut Vec<(u32, super::super::diff_parser::DiffViewMode)>,
    pub diff_hunk_navigations: &'a mut Vec<(u32, usize)>,
    pub diff_clear_scroll_to_first: &'a mut Vec<u32>,
    pub scrollbar_clear_restore: &'a mut Vec<u32>,
    pub scrollbar_dragging: &'a mut bool,
    /// (pane_id, new side_by_side value) toggled this frame by the conflict resolver toolbar.
    pub conflict_view_toggles: &'a mut Vec<(u32, bool)>,
}

// ── Leaf renderers ──────────────────────────────────────────────────────────

/// Keys each file's text box by its pane. Without it every editor drawn in the
/// same spot shares one cursor and one undo history, so opening another file
/// put the caret where the last one left it and undo could bring back that
/// file's text.
const FILE_EDITOR_TEXT_ID: &str = "file_editor_text";

/// Keyboard rule for a pane's text box. The active pane's box takes the
/// keyboard whenever nothing else holds it, like a terminal, so a new or
/// just-opened file is ready for typing. Any other pane's box gives it up:
/// a box in a split the user has left (by the session list, a drag or a
/// shortcut) must not keep taking their keys.
fn route_text_focus(ui: &egui::Ui, text_box: &egui::Response, is_active: bool, modal_open: bool) {
    if !is_active {
        if text_box.has_focus() {
            text_box.surrender_focus();
        }
        return;
    }
    if !modal_open && ui.memory(|m| m.focused().is_none()) {
        text_box.request_focus();
    }
}

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
        if !rctx.modal_open {
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

fn paint_search_highlights(
    painter: &egui::Painter,
    text_origin: egui::Pos2,
    line_h: f32,
    char_w: f32,
    matches: &[(usize, usize, usize)],
    current_idx: Option<usize>,
    clip: egui::Rect,
) {
    for (i, &(line, col_start, col_end)) in matches.iter().enumerate() {
        let y = text_origin.y + line as f32 * line_h;
        if y + line_h < clip.min.y || y > clip.max.y {
            continue;
        }
        let x0 = text_origin.x + col_start as f32 * char_w;
        let x1 = text_origin.x + col_end as f32 * char_w;
        let color = if Some(i) == current_idx {
            egui::Color32::from_rgba_unmultiplied(255, 165, 0, 100)
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 255, 0, 50)
        };
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, y + line_h)),
            0.0,
            color,
        );
    }
}

#[allow(dead_code)]
struct TextSearchBarResult {
    navigated: bool,
    current_line: Option<usize>,
}

fn render_text_search_bar(
    ui: &mut egui::Ui,
    pane_rect: egui::Rect,
    content: &str,
    search: &mut crate::search::TextSearchState,
) -> TextSearchBarResult {
    let none = TextSearchBarResult {
        navigated: false,
        current_line: None,
    };
    if !search.active {
        return none;
    }
    let t = theme::active();
    let bar_w = 320.0_f32.min(pane_rect.width() - 24.0).max(120.0);
    if pane_rect.width() < 148.0 {
        return none;
    }
    let bar_h = 30.0_f32;
    let bar_rect = egui::Rect::from_min_size(
        egui::pos2(pane_rect.max.x - bar_w - 8.0, pane_rect.min.y + 8.0),
        egui::vec2(bar_w, bar_h),
    );
    ui.painter().rect_filled(bar_rect, theme::R_MD, t.surface0);
    ui.painter().rect_stroke(
        bar_rect,
        theme::R_MD,
        egui::Stroke::new(1.0_f32, t.overlay0),
    );

    let controls_w = 110.0_f32;
    let input_rect = egui::Rect::from_min_max(
        egui::pos2(bar_rect.min.x + 6.0, bar_rect.min.y + 4.0),
        egui::pos2(bar_rect.max.x - controls_w, bar_rect.max.y - 4.0),
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

    let controls_x = bar_rect.max.x - controls_w;
    ui.painter().line_segment(
        [
            egui::pos2(controls_x, bar_rect.min.y + theme::SP_2),
            egui::pos2(controls_x, bar_rect.max.y - theme::SP_2),
        ],
        egui::Stroke::new(theme::STROKE_THIN, t.border_subtle),
    );

    let btn_size = 18.0_f32;
    let btn_y = bar_rect.center().y - btn_size / 2.0;
    let prev_rect = egui::Rect::from_min_size(
        egui::pos2(controls_x + 4.0, btn_y),
        egui::vec2(btn_size, btn_size),
    );
    let next_rect = egui::Rect::from_min_size(
        egui::pos2(prev_rect.max.x + 2.0, btn_y),
        egui::vec2(btn_size, btn_size),
    );
    let bar_id = ui.id().with("text_search_bar");
    let prev_clicked = crate::ui_kit::icon_button(
        ui,
        bar_id.with("prev"),
        prev_rect,
        "\u{25B2}",
        theme::FONT_UI_XS,
        t.subtext0,
        crate::ui_kit::IconButtonStyle::Default,
    )
    .clicked();
    let next_clicked = crate::ui_kit::icon_button(
        ui,
        bar_id.with("next"),
        next_rect,
        "\u{25BC}",
        theme::FONT_UI_XS,
        t.subtext0,
        crate::ui_kit::IconButtonStyle::Default,
    )
    .clicked();

    let kb_prev = ui.input(|i| {
        (i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::F3)) && i.modifiers.shift
    });
    let kb_next = ui.input(|i| {
        (i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::F3)) && !i.modifiers.shift
    });

    let mut navigated = false;
    if prev_clicked || kb_prev {
        search.prev_match();
        navigated = true;
    } else if next_clicked || kb_next {
        search.next_match();
        navigated = true;
    }

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
    let count_center_x = next_rect.max.x + (bar_rect.max.x - next_rect.max.x) / 2.0;
    ui.painter().text(
        egui::pos2(count_center_x, bar_rect.center().y),
        egui::Align2::CENTER_CENTER,
        &count_text,
        egui::FontId::monospace(theme::FONT_UI_SM),
        t.subtext0,
    );

    TextSearchBarResult {
        navigated: navigated || resp.changed(),
        current_line: search.current_match().map(|m| m.line),
    }
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
        // While a dialog is up its keys are its own: an Enter that confirms
        // "Save changes?" must not also add a line to the file it saves.
        let modal_open = rctx.modal_open;
        let search_active = is_focused && rctx.text_search.active;
        let scroll_target = if is_focused {
            rctx.text_search.current_match().map(|m| m.line)
        } else {
            None
        };
        let search_highlights: Vec<(usize, usize, usize)> = if search_active {
            rctx.text_search
                .matches
                .iter()
                .map(|m| (m.line, m.col_start, m.col_end))
                .collect()
        } else {
            Vec::new()
        };
        let search_current_idx = rctx.text_search.current_index;

        if previewing {
            if let Some(et) = rctx.editor_texts.iter().find(|(id, _)| *id == pane_id) {
                if let Some(ref text) = et.1 {
                    egui::ScrollArea::vertical()
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

                            let te_resp = if let Some(syn) = maybe_syntax {
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
                                        .id_source((FILE_EDITOR_TEXT_ID, pane_id))
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY)
                                        .frame(false)
                                        .lock_focus(true)
                                        .interactive(!modal_open)
                                        .layouter(&mut layouter),
                                )
                            } else {
                                ui.add(
                                    egui::TextEdit::multiline(text)
                                        .id_source((FILE_EDITOR_TEXT_ID, pane_id))
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY)
                                        .frame(false)
                                        .lock_focus(true)
                                        .interactive(!modal_open),
                                )
                            };
                            route_text_focus(ui, &te_resp, is_focused, modal_open);

                            if !search_highlights.is_empty() {
                                let mono_id =
                                    ui.style().text_styles[&egui::TextStyle::Monospace].clone();
                                let cw = ui.fonts(|f| {
                                    f.layout_no_wrap("0".to_string(), mono_id, egui::Color32::WHITE)
                                        .rect
                                        .width()
                                });
                                paint_search_highlights(
                                    ui.painter(),
                                    te_resp.rect.min,
                                    line_h,
                                    cw,
                                    &search_highlights,
                                    search_current_idx,
                                    ui.clip_rect(),
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

        // Match on the key event's own modifiers: a quick Cmd+S can be released
        // before the frame samples the live modifier state. Only the active
        // editor saves: with files side by side in splits, whichever drew
        // first would otherwise take the key.
        if is_focused
            && !modal_open
            && ui.input_mut(|inp| inp.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
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
    let search_active = is_focused && rctx.text_search.active;
    let scroll_target = if is_focused {
        rctx.text_search.current_match().map(|m| m.line)
    } else {
        None
    };
    let search_highlights: Vec<(usize, usize, usize)> = if search_active {
        rctx.text_search
            .matches
            .iter()
            .map(|m| (m.line, m.col_start, m.col_end))
            .collect()
    } else {
        Vec::new()
    };
    let search_current_idx = rctx.text_search.current_index;

    if let Some(et) = rctx.editor_texts.iter_mut().find(|(id, _)| *id == pane_id) {
        if let Some(ref mut text) = et.1 {
            let content_snapshot = if search_active {
                Some(text.clone())
            } else {
                None
            };

            egui::ScrollArea::both()
                .id_source(("note_editor_scroll", pane_id))
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let line_h = ui.text_style_height(&egui::TextStyle::Monospace);
                    let te_resp = ui.add(
                        egui::TextEdit::multiline(text)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .hint_text("Notes for this workspace\u{2026}")
                            .frame(false)
                            .lock_focus(true)
                            .interactive(!rctx.modal_open),
                    );
                    route_text_focus(ui, &te_resp, is_focused, rctx.modal_open);
                    if !search_highlights.is_empty() {
                        let mono_id = ui.style().text_styles[&egui::TextStyle::Monospace].clone();
                        let cw = ui.fonts(|f| {
                            f.layout_no_wrap("0".to_string(), mono_id, egui::Color32::WHITE)
                                .rect
                                .width()
                        });
                        paint_search_highlights(
                            ui.painter(),
                            te_resp.rect.min,
                            line_h,
                            cw,
                            &search_highlights,
                            search_current_idx,
                            ui.clip_rect(),
                        );
                    }
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
    clear_scroll_to_first: bool,
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

    let scroll_target = if d.scroll_to_first_hunk && !d.hunks.is_empty() {
        Some(0_usize)
    } else {
        toolbar.scroll_to_hunk
    };

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
                        scroll_target,
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
                        scroll_target,
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
        clear_scroll_to_first: d.scroll_to_first_hunk,
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

    // Track the exact content rect so the PTY grid is sized to match the drawn
    // area (same paddings), never wider/taller — otherwise content is hidden
    // past the right edge or under the info bar.
    rctx.pane_rects_snap.push((pane_id, rect));

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
                if result.clear_scroll_to_first {
                    rctx.diff_clear_scroll_to_first.push(pane_id);
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
}

fn node_has_visible_panes(app: &App, node: &GroupNode) -> bool {
    if !app.has_active_tab_filter() {
        return true;
    }
    match node {
        GroupNode::Leaf { group_id } => {
            let Some(group) = app.pane_state.groups.get(group_id) else {
                return false;
            };
            group.pane_ids.iter().any(|&pid| {
                app.pane_state
                    .panes
                    .iter()
                    .find(|p| p.id == pid)
                    .map(|p| app.pane_passes_tab_filter(p))
                    .unwrap_or(false)
            })
        }
        GroupNode::Split { a, b, .. } => {
            node_has_visible_panes(app, a) || node_has_visible_panes(app, b)
        }
    }
}

/// The area a drop will land in: half of `rect` for a split-edge drop, all of
/// it for a plain join.
fn drop_preview_rect(rect: egui::Rect, edge: Option<(SplitDir, bool)>) -> egui::Rect {
    match edge {
        None => rect,
        Some((SplitDir::Horizontal, true)) => {
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() / 2.0, rect.height()))
        }
        Some((SplitDir::Horizontal, false)) => egui::Rect::from_min_size(
            egui::pos2(rect.center().x, rect.min.y),
            egui::vec2(rect.width() / 2.0, rect.height()),
        ),
        Some((SplitDir::Vertical, true)) => {
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.height() / 2.0))
        }
        Some((SplitDir::Vertical, false)) => egui::Rect::from_min_size(
            egui::pos2(rect.min.x, rect.center().y),
            egui::vec2(rect.width(), rect.height() / 2.0),
        ),
    }
}

/// Translucent highlight over the area a drop will land in.
fn paint_drop_preview(ui: &egui::Ui, rect: egui::Rect, edge: Option<(SplitDir, bool)>) {
    let area = drop_preview_rect(rect, edge);
    let t = theme::active();
    ui.painter()
        .rect_filled(area, theme::R_MD, t.accent.gamma_multiply(0.25));
    ui.painter().rect_stroke(
        area,
        theme::R_MD,
        egui::Stroke::new(theme::STROKE_THIN, t.accent),
    );
}

/// Hit-test the active drag against one leaf and paint what a drop would do.
///
/// `hit_rect` is the leaf's whole area — info bar included, so the hint never
/// blinks out over a pane header — while `split_rect` is the content area the
/// edge zones are measured against. A pointer on the header therefore joins
/// the split, matching the left-nav section headers.
///
/// Called on every leaf path, degenerate ones included: gating this on a
/// separate hit test elsewhere is what made the hint intermittent.
fn hit_test_drop(
    app: &mut App,
    ui: &egui::Ui,
    hit_rect: egui::Rect,
    split_rect: egui::Rect,
    group_id: GroupId,
) {
    use crate::app::drag::{DragAction, DragPayload, DropTarget};

    // A workspace drag only ever opens a window, so it must not preview a drop.
    if !app.drag_state.is_active()
        || matches!(app.drag_state.payload, Some(DragPayload::Workspace(_)))
    {
        return;
    }
    let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) else {
        return;
    };
    if !hit_rect.contains(pos) {
        return;
    }
    // Cloned only once the pointer is known to be here — this runs for every
    // leaf on every frame of a drag.
    let Some(payload) = app.drag_state.payload.clone() else {
        return;
    };

    let edge = crate::app::drag::edge_zone(split_rect, pos);
    let target = match edge {
        Some((dir, before)) => DropTarget::SplitEdge {
            group_id,
            dir,
            before,
        },
        None => DropTarget::GroupArea(group_id),
    };

    // Ask the resolver what this drop would actually do, so a highlight is
    // never shown for one that does nothing — dragging a pane onto its own
    // split, or splitting off a group's only pane.
    if matches!(
        crate::app::drag::resolve_group(payload, target.clone(), &app.pane_state),
        DragAction::Noop
    ) {
        return;
    }

    paint_drop_preview(ui, if edge.is_some() { split_rect } else { hit_rect }, edge);
    app.drag_state.drop_target = Some(target);
}

/// Apply a deferred action produced by a split's info bar header.
fn apply_info_bar_action(app: &mut App, action: InfoBarAction, ctx: &egui::Context) {
    match action {
        InfoBarAction::None => {}
        InfoBarAction::SetPosition(gid, pos) => {
            // No save_session() here: with scrollback saving on it writes
            // every terminal's history. The next natural save (pane changes,
            // quit) records the split's position; the default for new splits
            // is in settings and saved at once.
            app.choose_info_bar_position(gid, pos);
        }
        InfoBarAction::Split(gid, dir) => {
            // The split is performed against the focused group, so hand focus
            // over before requesting it — otherwise clicking the button on a
            // non-focused split would split the wrong one.
            app.pane_state.focused_group_id = gid;
            if let Some(pid) = app
                .pane_state
                .groups
                .get(&gid)
                .and_then(|g| g.active_pane_id)
            {
                // Go through activate_pane so the session active flags, the
                // recency stamp and the group's own active pane all agree —
                // setting active_pane_id alone leaves them stale.
                app.activate_pane(pid);
            }
            app.deferred_split = Some(dir);
            // deferred_split is consumed at the top of the *next* update(), so
            // guarantee that frame happens instead of relying on the cursor
            // blink or a stray mouse move to schedule it.
            ctx.request_repaint();
        }
        InfoBarAction::CloseTab(pane_id) => {
            app.request_close_tab(pane_id, crate::app::close_guard::TabClose::Pane)
        }
        InfoBarAction::CloseAllInGroup(gid) => {
            app.request_close_all(CloseAllTarget::EditorGroup(gid));
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
    pane_rects_snap: &mut Vec<(u32, egui::Rect)>,
    split_ratio_changes: &mut Vec<(u32, f32)>,
    pane_context_actions: &mut Vec<PaneContextAction>,
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
                // No drop target: this leaf names a group that is not in the
                // map, so a move would detach the pane and have nowhere to put
                // it. `sync_groups_with_panes` prunes the leaf shortly after.
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
                hit_test_drop(app, &leaf_ui, rect, rect, group_id);
                return;
            }

            // The tab bar, when on, runs along the top of the split; the info
            // bar and the content share what is below it.
            let (tab_bar_rect, body) = if app.settings.show_tab_bar {
                let bar = egui::Rect::from_min_size(
                    rect.min,
                    egui::vec2(rect.width(), theme::HEADER_H.min(rect.height())),
                );
                (
                    Some(bar),
                    egui::Rect::from_min_max(bar.left_bottom(), rect.max),
                )
            } else {
                (None, rect)
            };
            if let Some(bar) = tab_bar_rect {
                app.render_tab_bar(&mut leaf_ui, &group, group_id, is_focused, bar);
            }

            // Split the rest into optional info bar and content
            let info_bar_pos = app.info_bar_position(group_id);
            let info_h = if info_bar_pos != InfoBarPosition::Hidden {
                theme::INFO_BAR_H
            } else {
                0.0
            };

            let (info_bar_rect, content_rect) = match info_bar_pos {
                InfoBarPosition::Top => {
                    let ib = egui::Rect::from_min_size(body.min, egui::vec2(body.width(), info_h));
                    let cr = egui::Rect::from_min_size(
                        egui::pos2(body.min.x, body.min.y + info_h),
                        egui::vec2(body.width(), (body.height() - info_h).max(0.0)),
                    );
                    (Some(ib), cr)
                }
                InfoBarPosition::Bottom => {
                    let cr = egui::Rect::from_min_size(
                        body.min,
                        egui::vec2(body.width(), (body.height() - info_h).max(0.0)),
                    );
                    let ib = egui::Rect::from_min_size(
                        egui::pos2(body.min.x, cr.max.y),
                        egui::vec2(body.width(), info_h),
                    );
                    (Some(ib), cr)
                }
                InfoBarPosition::Hidden => (None, body),
            };

            // Render info bar (or show toggle when hidden)
            let mut info_bar_action = InfoBarAction::None;
            if let Some(ib_rect) = info_bar_rect {
                info_bar_action = app.render_info_bar(
                    &mut leaf_ui,
                    ib_rect,
                    group.active_pane_id,
                    group_id,
                    is_focused,
                );
            }

            // If active pane is filtered out, show empty state
            let active_filtered_out = if let Some(pid) = group.active_pane_id {
                !app.pane_state
                    .panes
                    .iter()
                    .find(|p| p.id == pid)
                    .map(|p| app.pane_passes_tab_filter(p))
                    .unwrap_or(true)
            } else {
                false
            };

            if active_filtered_out {
                let ctx = leaf_ui.ctx().clone();
                apply_info_bar_action(app, info_bar_action, &ctx);
                let t = theme::active();
                leaf_ui.painter().rect_filled(content_rect, 0.0, t.bg_term);
                leaf_ui.allocate_ui_at_rect(content_rect, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new("No sessions match the current filter")
                                .color(t.overlay0)
                                .size(theme::FONT_UI_MD),
                        );
                    });
                });
                hit_test_drop(app, &leaf_ui, body, content_rect, group_id);
                return;
            }

            // Render the active pane's content
            let active_pid = group.active_pane_id;
            if let Some(pid) = active_pid {
                let has_splits = app.pane_state.groups.len() > 1;
                let mut diff_mode_changes = Vec::new();
                let mut diff_hunk_navigations: Vec<(u32, usize)> = Vec::new();
                let mut diff_clear_scroll_to_first: Vec<u32> = Vec::new();
                let mut scrollbar_clear_restore = Vec::new();
                let mut scrollbar_dragging = false;
                let mut conflict_view_toggles: Vec<(u32, bool)> = Vec::new();
                let focused_pane_id = if is_focused { Some(pid) } else { None };
                let modal_open = app.any_modal_open();

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
                        pane_rects_snap,
                        split_ratio_changes,
                        pane_context_actions,
                        term_selection: &app.term_selection,
                        term_selection_sid: app.term_selection_sid,
                        modal_open,
                        font_size: app.settings.font_size,
                        cursor_style: app.settings.cursor_style,
                        has_splits,
                        flash: &app.flash,
                        text_search: &mut app.text_search,
                        diff_mode_changes: &mut diff_mode_changes,
                        diff_hunk_navigations: &mut diff_hunk_navigations,
                        diff_clear_scroll_to_first: &mut diff_clear_scroll_to_first,
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
                for pane_id in diff_clear_scroll_to_first {
                    if let Some(pane) = app.pane_state.panes.iter_mut().find(|p| p.id == pane_id) {
                        if let PaneContent::FileDiff(ref mut d) = pane.content {
                            d.scroll_to_first_hunk = false;
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

            // Show toggle when info bar is hidden (rendered after content to overlay)
            if info_bar_pos == InfoBarPosition::Hidden
                && app.render_info_bar_show_toggle(&mut leaf_ui, content_rect)
            {
                info_bar_action =
                    InfoBarAction::SetPosition(group_id, app.revealed_info_bar_position());
            }

            let ctx = leaf_ui.ctx().clone();
            apply_info_bar_action(app, info_bar_action, &ctx);

            // Route this leaf's drop target: near a content edge the drop
            // splits the group, anywhere else in the leaf it joins the group.
            hit_test_drop(app, &leaf_ui, body, content_rect, group_id);

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
                    // Only a click on the split itself: one on a dialog, menu
                    // or popup drawn over it (Settings, a right-click menu)
                    // must not move focus to the split underneath.
                    let on_this_layer = leaf_ui.ctx().layer_id_at(pos) == Some(leaf_ui.layer_id());
                    if rect.contains(pos) && on_this_layer {
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
            // Collapse filtered-out sides
            let a_visible = node_has_visible_panes(app, a);
            let b_visible = node_has_visible_panes(app, b);

            if !a_visible && !b_visible {
                ui.painter().rect_filled(rect, 0.0, theme::active().bg_term);
                return;
            }

            if !a_visible {
                render_group_node(
                    app,
                    ui,
                    b,
                    rect,
                    editor_texts,
                    clicked_pane_id,
                    clicked_group_id,
                    editor_saves,
                    editor_preview_toggles,
                    pane_rects_snap,
                    split_ratio_changes,
                    pane_context_actions,
                );
                return;
            }

            if !b_visible {
                render_group_node(
                    app,
                    ui,
                    a,
                    rect,
                    editor_texts,
                    clicked_pane_id,
                    clicked_group_id,
                    editor_saves,
                    editor_preview_toggles,
                    pane_rects_snap,
                    split_ratio_changes,
                    pane_context_actions,
                );
                return;
            }

            // Both visible — normal split rendering
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
                pane_rects_snap,
                split_ratio_changes,
                pane_context_actions,
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
                pane_rects_snap,
                split_ratio_changes,
                pane_context_actions,
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
        pane_rects_snap: &mut Vec<(u32, egui::Rect)>,
        split_ratio_changes: &mut Vec<(u32, f32)>,
        pane_context_actions: &mut Vec<PaneContextAction>,
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
            pane_rects_snap,
            split_ratio_changes,
            pane_context_actions,
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
                egui::Stroke::new(2.0_f32, t.blue),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(200.0, 100.0))
    }

    #[test]
    fn join_preview_covers_whole_rect() {
        assert_eq!(drop_preview_rect(r(), None), r());
    }

    #[test]
    fn edge_previews_cover_the_matching_half() {
        let rect = r();
        for (edge, expect_min, expect_max) in [
            (
                (SplitDir::Horizontal, true),
                egui::pos2(10.0, 20.0),
                egui::pos2(110.0, 120.0),
            ),
            (
                (SplitDir::Horizontal, false),
                egui::pos2(110.0, 20.0),
                egui::pos2(210.0, 120.0),
            ),
            (
                (SplitDir::Vertical, true),
                egui::pos2(10.0, 20.0),
                egui::pos2(210.0, 70.0),
            ),
            (
                (SplitDir::Vertical, false),
                egui::pos2(10.0, 70.0),
                egui::pos2(210.0, 120.0),
            ),
        ] {
            let got = drop_preview_rect(rect, Some(edge));
            assert_eq!(got.min, expect_min, "min for {edge:?}");
            assert_eq!(got.max, expect_max, "max for {edge:?}");
        }
    }

    #[test]
    fn every_edge_preview_is_half_the_area() {
        let rect = r();
        let full = rect.width() * rect.height();
        for dir in [SplitDir::Horizontal, SplitDir::Vertical] {
            for before in [true, false] {
                let half = drop_preview_rect(rect, Some((dir, before)));
                assert!((half.width() * half.height() - full / 2.0).abs() < 0.01);
                assert!(rect.contains_rect(half));
            }
        }
    }
}
