use std::sync::Arc;

use super::super::file_browser;
use super::super::markdown::render_markdown;
use super::super::pane::{PaneContent, PaneEntry, SessionEntry, TermSelection};
use super::super::settings::CursorStyle;
use super::super::App;
use crate::pane_tree::{split_rect, PaneNode, SplitDir};
use crate::renderer::terminal_pass::TerminalGeometry;
use crate::theme;

/// Mutable context threaded through the recursive pane-tree renderer.
///
/// This struct exists so we can pass references to output accumulators and
/// read-only state into `render_node()` without capturing `&mut self`.
pub(in crate::app) struct RenderCtx<'a> {
    pub sessions: &'a [SessionEntry],
    pub panes: &'a [PaneEntry],
    pub editor_texts: &'a mut Vec<(u32, Option<String>)>,
    pub cursor_blink_on: bool,
    pub focused_pane_id: Option<u32>,
    pub active_term_geo: &'a mut Option<TerminalGeometry>,
    pub active_term_ui_id: &'a mut Option<egui::Id>,
    pub clicked_pane_id: &'a mut Option<u32>,
    pub editor_saves: &'a mut Vec<u32>,
    pub editor_preview_toggles: &'a mut Vec<u32>,
    pub pane_widths_snap: &'a mut Vec<(u32, f32)>,
    pub split_ratio_changes: &'a mut Vec<(u32, f32)>,
    pub term_selection: &'a Option<TermSelection>,
    pub term_selection_sid: Option<u32>,
    pub workspace_dialog_open: bool,
    pub workspace_edit_dialog_open: bool,
    pub show_settings: bool,
    pub font_size: f32,
    pub cursor_style: CursorStyle,
    pub has_splits: bool,
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

            ui.allocate_ui_at_rect(rect, |ui| match &pane.content {
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
                    render_file_diff_leaf(ui, d, pane_id);
                }
            });

            // Focus border for split panes
            if is_focused && rctx.has_splits {
                let stroke = egui::Stroke::new(1.5, theme::active().blue);
                ui.painter().rect_stroke(rect, 0.0, stroke);
            }

            // Click to focus pane
            if ui
                .ctx()
                .input(|inp| inp.pointer.button_clicked(egui::PointerButton::Primary))
            {
                if let Some(pos) = ui.ctx().input(|inp| inp.pointer.interact_pos()) {
                    if rect.contains(pos) {
                        *rctx.clicked_pane_id = Some(pane_id);
                        // Release focus from any other widget (e.g. the
                        // notes TextEdit) so the terminal can take keyboard
                        // focus on the next frame.
                        if let Some(fid) = ui.ctx().memory(|m| m.focused()) {
                            ui.ctx().memory_mut(|m| m.surrender_focus(fid));
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
            let div_color = if div_resp.dragged() || div_resp.hovered() {
                theme::active().divider_active
            } else {
                theme::active().divider_idle
            };
            ui.painter()
                .rect_filled(div_rect, theme::STROKE_THIN, div_color);

            // Handle drag to resize
            if div_resp.dragged() {
                let delta = div_resp.drag_delta();
                let movement = match dir {
                    SplitDir::Horizontal => delta.x / rect.width(),
                    SplitDir::Vertical => delta.y / rect.height(),
                };
                let new_ratio = (*ratio + movement).clamp(0.1, 0.9);
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
                })
        } else {
            None
        };
        let geo = crate::renderer::terminal_pass::TerminalView::new(session).show(
            ui,
            is_focused,
            rctx.cursor_blink_on,
            sel_range.as_ref(),
            rctx.font_size,
            rctx.cursor_style,
        );
        if is_focused {
            *rctx.active_term_geo = Some(geo);
        }
    }
    if is_focused {
        let this_id = ui.id();
        *rctx.active_term_ui_id = Some(this_id);
        let dialog_open =
            rctx.workspace_dialog_open || rctx.workspace_edit_dialog_open || rctx.show_settings;
        // Re-assert focus only when no other widget owns it. This recovers
        // from transient focus steals (scroll areas, autocomplete) without
        // trampling intentional focus on widgets like the notes TextEdit or
        // the workspace search box.
        if !dialog_open {
            let other_focused = ui.memory(|m| m.focused().map(|id| id != this_id).unwrap_or(false));
            if !other_focused {
                ui.memory_mut(|m| m.request_focus(this_id));
            }
        }
    }
}

fn render_file_editor_leaf(
    ui: &mut egui::Ui,
    ed: &super::super::pane::FileEditorState,
    pane_id: u32,
    rctx: &mut RenderCtx<'_>,
) {
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, theme::active().bg_term);
    if !file_browser::is_supported_text_file(&ed.path, &ed.content) {
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new("File type not supported for preview")
                    .size(16.0)
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
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new("Raw").size(11.0).color(raw_color))
                            .fill(raw_bg)
                            .rounding(egui::Rounding::same(theme::ROUNDING))
                            .min_size(egui::vec2(56.0, 20.0)),
                    )
                    .clicked()
                    && previewing
                {
                    rctx.editor_preview_toggles.push(pane_id);
                }
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new("Preview")
                                .size(11.0)
                                .color(preview_color),
                        )
                        .fill(preview_bg)
                        .rounding(egui::Rounding::same(theme::ROUNDING))
                        .min_size(egui::vec2(56.0, 20.0)),
                    )
                    .clicked()
                    && !previewing
                {
                    rctx.editor_preview_toggles.push(pane_id);
                }
            });
            ui.separator();
        }
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
                egui::ScrollArea::both()
                    .id_source(("editor_scroll", pane_id))
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        let line_count = text.lines().count().max(1);
                        let digits = ((line_count as f64).log10().floor() as usize) + 1;
                        let char_w = 7.5_f32; // approx monospace char width at default size
                        let gutter_w = (digits as f32 + 1.5) * char_w;
                        let line_h = ui.text_style_height(&egui::TextStyle::Monospace);

                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            // Line number gutter
                            ui.vertical(|ui| {
                                ui.set_min_width(gutter_w);
                                // Pad top to match TextEdit internal padding
                                ui.add_space(2.0);
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
                            // Separator line
                            let sep_rect = ui
                                .allocate_exact_size(
                                    egui::vec2(1.0, line_h * line_count as f32 + 4.0),
                                    egui::Sense::hover(),
                                )
                                .0;
                            ui.painter()
                                .rect_filled(sep_rect, 0.0, theme::active().surface1);
                            ui.add_space(theme::SP_SM);
                            // Editor
                            ui.add(
                                egui::TextEdit::multiline(text)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .frame(false),
                            );
                        });
                    });
            }
        }
        if ui.input(|inp| inp.modifiers.ctrl && inp.key_pressed(egui::Key::S)) {
            rctx.editor_saves.push(pane_id);
        }
    }
}

fn render_file_diff_leaf(ui: &mut egui::Ui, d: &super::super::pane::FileDiffState, pane_id: u32) {
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, theme::active().bg_term);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("\u{21c4} {}", d.path.display()))
                .strong()
                .size(13.0)
                .color(theme::active().git_filename),
        );
    });
    ui.separator();
    egui::ScrollArea::both()
        .id_source(("diff_scroll", pane_id))
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            super::super::git_diff::render_inline_diff(ui, &d.diff_content);
        });
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
    ) {
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
            let tree = self
                .pane_state
                .pane_trees
                .get(&root_pane_id)
                .cloned()
                .unwrap_or(PaneNode::Leaf {
                    pane_id: root_pane_id,
                    last_size: (80, 24),
                });

            let has_splits = matches!(&tree, PaneNode::Split { .. });
            let mut rctx = RenderCtx {
                sessions: &self.session_state.sessions,
                panes: &self.pane_state.panes,
                editor_texts,
                cursor_blink_on: self.cursor_blink_on,
                focused_pane_id: active_pane_id_snap,
                active_term_geo: &mut self.active_term_geo,
                active_term_ui_id: &mut self.active_term_ui_id,
                clicked_pane_id,
                editor_saves,
                editor_preview_toggles,
                pane_widths_snap,
                split_ratio_changes,
                term_selection: &self.term_selection,
                term_selection_sid: self.term_selection_sid,
                workspace_dialog_open: self.workspace_dialog.is_some(),
                workspace_edit_dialog_open: self.workspace_edit_dialog.is_some(),
                show_settings: self.show_settings,
                font_size: self.settings.font_size,
                cursor_style: self.settings.cursor_style,
                has_splits,
            };
            render_node(ui, &tree, content_rect, &mut rctx);
        }
    }
}
