use std::sync::Arc;

use super::super::feedback;
use super::super::file_browser;
use super::super::markdown::render_markdown;
use super::super::pane::{NoteEditorState, PaneContent, PaneEntry, SessionEntry, TermSelection};
use super::super::settings::CursorStyle;
use super::super::App;
use crate::pane_tree::{split_rect, PaneNode, SplitDir};
use crate::renderer::terminal_pass::TerminalGeometry;
use crate::syntax;
use crate::theme;
use crate::ui_kit;

/// Actions emitted by the 3-dot context menu on split panes.
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

/// Mutable context threaded through the recursive pane-tree renderer.
///
/// This struct exists so we can pass references to output accumulators and
/// read-only state into `render_node()` without capturing `&mut self`.
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
    pub drag_state: &'a mut crate::app::drag::DragState,
}

/// Recursively render a pane tree node into the given rect.
pub(in crate::app) fn render_node(
    ui: &mut egui::Ui,
    node: &PaneNode,
    rect: egui::Rect,
    rctx: &mut RenderCtx<'_>,
) {
    match node {
        PaneNode::Leaf { pane_id, .. } => {
            let pane_id = *pane_id;
            let is_focused = rctx.focused_pane_id == Some(pane_id);
            let pane = rctx.panes.iter().find(|p| p.id == pane_id);
            let Some(pane) = pane else { return };

            // Track width for resize
            rctx.pane_widths_snap.push((pane_id, rect.width()));

            let content_rect = rect;

            ui.allocate_ui_at_rect(content_rect, |ui| match &pane.content {
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
                    if let Some(new_mode) = render_file_diff_leaf(ui, d, pane_id, rctx) {
                        rctx.diff_mode_changes.push((pane_id, new_mode));
                    }
                }
                PaneContent::NoteEditor(ne) => {
                    render_note_editor_leaf(ui, ne, pane_id, rctx);
                }
                PaneContent::ConflictResolver(ref state) => {
                    let conflict_action =
                        super::conflict_resolver::render_conflict_resolver(ui, state);
                    if let Some(ca) = conflict_action {
                        rctx.pane_context_actions
                            .push(PaneContextAction::ConflictResolve {
                                pane_id,
                                action: ca,
                            });
                    }
                }
            });

            // Focus border for split panes
            if is_focused && rctx.has_splits {
                let stroke = egui::Stroke::new(1.5, theme::active().accent);
                ui.painter().rect_stroke(rect, 0.0, stroke);
            }

            // Floating 3-dot context menu overlay for split panes
            if rctx.has_splits {
                render_pane_overlay_menu(ui, pane_id, rect, rctx);
            }

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
                    Some(crate::app::drag::DragPayload::Session(_))
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
                                egui::Stroke::new(
                                    2.0,
                                    crate::theme::active().blue.gamma_multiply(0.6),
                                ),
                            );
                            rctx.drag_state.drop_target =
                                Some(crate::app::drag::DropTarget::PaneArea);
                        }
                    }
                }
            }

            // Click to focus pane (ignore clicks during panel resize drags)
            let any_drag = ui
                .ctx()
                .input(|inp| inp.pointer.any_down() && inp.pointer.is_decidedly_dragging());
            if !any_drag
                && ui
                    .ctx()
                    .input(|inp| inp.pointer.button_clicked(egui::PointerButton::Primary))
            {
                if let Some(pos) = ui.ctx().input(|inp| inp.pointer.interact_pos()) {
                    if rect.contains(pos) {
                        *rctx.clicked_pane_id = Some(pane_id);
                        // Only surrender widget focus when clicking a terminal
                        // pane — editors need their TextEdit to keep focus.
                        if matches!(pane.content, PaneContent::Terminal(_)) {
                            if let Some(fid) = ui.ctx().memory(|m| m.focused()) {
                                ui.ctx().memory_mut(|m| m.surrender_focus(fid));
                            }
                        }
                    }
                }
            }
        }
        PaneNode::Split {
            split_id,
            dir,
            ratio,
            a,
            b,
        } => {
            let (rect_a, div_rect, rect_b) = split_rect(rect, *dir, *ratio);
            render_node(ui, a, rect_a, rctx);
            render_node(ui, b, rect_b, rctx);

            // Draw divider
            let div_id = egui::Id::new(("split_div", *split_id));
            let div_resp = ui.interact(div_rect, div_id, egui::Sense::drag());
            let is_active = div_resp.dragged() || div_resp.hovered();
            let anim_t = ui
                .ctx()
                .animate_bool_with_time(div_id.with("anim"), is_active, 0.15);
            let div_color = theme::lerp_color(
                theme::active().divider_idle,
                theme::active().divider_active,
                anim_t,
            );
            ui.painter()
                .rect_filled(div_rect, theme::STROKE_THIN, div_color);

            // Handle drag to resize
            if div_resp.dragged() {
                let delta = div_resp.drag_delta();
                let (extent, movement) = match dir {
                    SplitDir::Horizontal => (rect.width(), delta.x / rect.width()),
                    SplitDir::Vertical => (rect.height(), delta.y / rect.height()),
                };
                let min_pane = theme::MIN_PANE_W;
                let min_ratio = if extent > 0.0 {
                    (min_pane / extent).clamp(0.1, 0.4)
                } else {
                    0.1
                };
                let new_ratio = (*ratio + movement).clamp(min_ratio, 1.0 - min_ratio);
                rctx.split_ratio_changes.push((*split_id, new_ratio));
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

// ── Split-pane floating overlay menu ──────────────────────────────────────

fn render_pane_overlay_menu(
    ui: &mut egui::Ui,
    pane_id: u32,
    pane_rect: egui::Rect,
    rctx: &mut RenderCtx<'_>,
) {
    let popup_id = egui::Id::new(("pane_ctx_menu", pane_id));
    let popup_open = ui.memory(|m| m.is_popup_open(popup_id));

    let btn_size = egui::vec2(theme::BTN_SQ, theme::BTN_SQ);
    let btn_pos = egui::pos2(
        pane_rect.max.x - btn_size.x - theme::SP_3,
        pane_rect.min.y + theme::SP_2,
    );
    let btn_rect = egui::Rect::from_min_size(btn_pos, btn_size);

    let pane_hovered = ui.ctx().input(|i| {
        i.pointer
            .latest_pos()
            .map(|p| pane_rect.contains(p))
            .unwrap_or(false)
    });

    let show_btn = pane_hovered || popup_open;
    if !show_btn {
        return;
    }

    let t = theme::active();
    let bg_pill =
        egui::Color32::from_rgba_unmultiplied(t.surface0.r(), t.surface0.g(), t.surface0.b(), 200);
    ui.painter()
        .rect_filled(btn_rect.expand(theme::SP_1), theme::R_MD, bg_pill);

    let btn_id = egui::Id::new(("pane_hdr_menu_btn", pane_id));
    let btn_resp = ui_kit::dot_menu_button(ui, btn_id, btn_rect, popup_open);
    if btn_resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    egui::containers::popup::popup_below_widget(
        ui,
        popup_id,
        &btn_resp,
        egui::containers::popup::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(160.0);
            if ui.button("Move to tab").clicked() {
                rctx.pane_context_actions
                    .push(PaneContextAction::MoveToTab(pane_id));
                ui.memory_mut(|m| m.close_popup());
            }
            ui.separator();
            if ui.button("Split horizontal").clicked() {
                rctx.pane_context_actions
                    .push(PaneContextAction::SplitHorizontal(pane_id));
                ui.memory_mut(|m| m.close_popup());
            }
            if ui.button("Split vertical").clicked() {
                rctx.pane_context_actions
                    .push(PaneContextAction::SplitVertical(pane_id));
                ui.memory_mut(|m| m.close_popup());
            }
            ui.separator();
            if ui.button("Close pane").clicked() {
                rctx.pane_context_actions
                    .push(PaneContextAction::Close(pane_id));
                ui.memory_mut(|m| m.close_popup());
            }
        },
    );
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
    let bar_w = 320.0_f32;
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

                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.vertical(|ui| {
                                ui.set_min_width(gutter_w);
                                ui.add_space(theme::SP_1);
                                for n in 1..=line_count {
                                    let num_str = format!("{:>width$}", n, width = digits);
                                    ui.add_sized(
                                        egui::vec2(gutter_w, line_h),
                                        egui::Label::new(
                                            egui::RichText::new(num_str)
                                                .monospace()
                                                .color(theme::active().overlay0),
                                        ),
                                    );
                                }
                            });
                            let sep_rect = ui
                                .allocate_exact_size(
                                    egui::vec2(1.0, line_h * line_count as f32 + 4.0),
                                    egui::Sense::hover(),
                                )
                                .0;
                            ui.painter()
                                .rect_filled(sep_rect, 0.0, theme::active().surface1);
                            ui.add_space(theme::SP_2);

                            if let Some(syn) = maybe_syntax {
                                let mut layouter = |ui: &egui::Ui, s: &str, wrap_width: f32| {
                                    let job = syntax::highlight_layout_job(ui, s, syn, wrap_width);
                                    ui.fonts(|f| f.layout_job(job))
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

fn render_file_diff_leaf(
    ui: &mut egui::Ui,
    d: &super::super::pane::FileDiffState,
    pane_id: u32,
    rctx: &mut RenderCtx<'_>,
) -> Option<super::super::diff_parser::DiffViewMode> {
    let pane_rect = ui.max_rect();
    ui.painter()
        .rect_filled(pane_rect, 0.0, theme::active().bg_term);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("\u{21c4} {}", d.path.display()))
                .strong()
                .size(theme::FONT_UI_LG)
                .color(theme::active().git_filename),
        );
    });
    ui.separator();

    let mode_change = super::super::git_diff::render_diff_toolbar(ui, d.diff_mode);
    ui.separator();

    let is_focused = rctx.focused_pane_id == Some(pane_id);
    let scroll_target = if is_focused {
        rctx.text_search.current_match().map(|m| m.line)
    } else {
        None
    };

    egui::ScrollArea::both()
        .id_source(("diff_scroll", pane_id))
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let line_h = ui.text_style_height(&egui::TextStyle::Monospace);
            match d.diff_mode {
                super::super::diff_parser::DiffViewMode::Inline => {
                    super::super::git_diff::render_inline_diff_full(
                        ui,
                        &d.old_content,
                        &d.new_content,
                        &d.hunks,
                    );
                }
                super::super::diff_parser::DiffViewMode::SideBySide => {
                    super::super::git_diff::render_side_by_side_diff(
                        ui,
                        &d.old_content,
                        &d.new_content,
                        &d.hunks,
                    );
                }
            }
            if let Some(target_line) = scroll_target {
                let target_y = target_line as f32 * line_h;
                let scroll_rect =
                    egui::Rect::from_min_size(egui::pos2(0.0, target_y), egui::vec2(1.0, line_h));
                ui.scroll_to_rect(scroll_rect, Some(egui::Align::Center));
            }
        });

    if is_focused && rctx.text_search.active {
        render_text_search_bar(ui, pane_rect, &d.new_content, rctx.text_search);
    }

    mode_change
}

impl App {
    /// Render the active tab's pane content (terminal, editor, diff) using the
    /// split-aware recursive pane tree renderer.
    ///
    /// Returns the deferred mutations (clicked pane, editor saves/previews,
    /// pane width snapshots, split ratio changes) via in/out parameters.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::app) fn render_pane_content(
        &mut self,
        ui: &mut egui::Ui,
        content_rect: egui::Rect,
        active_pane_id_snap: Option<u32>,
        editor_texts: &mut Vec<(u32, Option<String>)>,
        clicked_pane_id: &mut Option<u32>,
        editor_saves: &mut Vec<u32>,
        editor_preview_toggles: &mut Vec<u32>,
        pane_widths_snap: &mut Vec<(u32, f32)>,
        split_ratio_changes: &mut Vec<(u32, f32)>,
        pane_context_actions: &mut Vec<PaneContextAction>,
    ) {
        // Clear stale terminal geometry so non-terminal panes (e.g. file editor)
        // don't inherit geometry from a previously rendered terminal tab (H9).
        self.active_term_geo = None;

        let root_pane_id = active_pane_id_snap.and_then(|apid| {
            if self.pane_state.pane_trees.contains_key(&apid) {
                return Some(apid);
            }
            self.pane_state
                .pane_trees
                .iter()
                .find(|(_, tree)| tree.leaf_ids().contains(&apid))
                .map(|(&rpid, _)| rpid)
        });
        if let Some(root_pane_id) = root_pane_id {
            let (tree, has_splits) = if let Some(zpid) = self.zoomed_pane_id {
                let pane_exists = self.pane_state.panes.iter().any(|p| p.id == zpid);
                if pane_exists {
                    (
                        PaneNode::Leaf {
                            pane_id: zpid,
                            last_size: (80, 24),
                        },
                        false,
                    )
                } else {
                    self.zoomed_pane_id = None;
                    let t = self
                        .pane_state
                        .pane_trees
                        .get(&root_pane_id)
                        .cloned()
                        .unwrap_or(PaneNode::Leaf {
                            pane_id: root_pane_id,
                            last_size: (80, 24),
                        });
                    let s = matches!(&t, PaneNode::Split { .. });
                    (t, s)
                }
            } else {
                let t = self
                    .pane_state
                    .pane_trees
                    .get(&root_pane_id)
                    .cloned()
                    .unwrap_or(PaneNode::Leaf {
                        pane_id: root_pane_id,
                        last_size: (80, 24),
                    });
                let s = matches!(&t, PaneNode::Split { .. });
                (t, s)
            };
            let mut diff_mode_changes = Vec::new();
            let mut rctx = RenderCtx {
                sessions: &self.session_state.sessions,
                panes: &self.pane_state.panes,
                editor_texts,
                cursor_alpha: self.cursor_alpha,
                focused_pane_id: active_pane_id_snap,
                active_term_geo: &mut self.active_term_geo,
                active_term_ui_id: &mut self.active_term_ui_id,
                clicked_pane_id,
                editor_saves,
                editor_preview_toggles,
                pane_widths_snap,
                split_ratio_changes,
                pane_context_actions,
                term_selection: &self.term_selection,
                term_selection_sid: self.term_selection_sid,
                workspace_dialog_open: self.workspace_dialog.is_some(),
                workspace_edit_dialog_open: self.workspace_edit_dialog.is_some(),
                show_settings: self.show_settings,
                font_size: self.settings.font_size,
                cursor_style: self.settings.cursor_style,
                has_splits,
                flash: &self.flash,
                text_search: &mut self.text_search,
                diff_mode_changes: &mut diff_mode_changes,
                drag_state: &mut self.drag_state,
            };
            render_node(ui, &tree, content_rect, &mut rctx);

            // Global flash overlay (rare — PTY spawn errors)
            self.flash
                .render_on_rect(ui.painter(), content_rect, feedback::FlashTarget::Global);

            for (pane_id, new_mode) in diff_mode_changes {
                if let Some(pane) = self.pane_state.panes.iter_mut().find(|p| p.id == pane_id) {
                    if let PaneContent::FileDiff(ref mut d) = pane.content {
                        d.diff_mode = new_mode;
                    }
                }
                self.settings.diff_view_mode = new_mode;
                self.settings.save();
            }
        }

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
