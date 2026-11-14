//! The row of tabs above each split, shown while `settings.show_tab_bar` is
//! on (the default; Settings, View → Show/Hide Tab Bar, the command palette).
//!
//! One tab per tab the split holds, in the split's own order — the order
//! ⇧⌘[ / ⇧⌘] step through. Click a tab to show it, double-click a terminal's
//! tab to rename it, middle-click or ✕ to close it, right-click for the same
//! menu as its session-list row, and drag it to reorder it, move it to
//! another split's tab bar or split it off at an edge.
//!
//! Every action goes through the session list's handling
//! ([`App::process_tab_actions`]), so a tab behaves exactly like its row.
//! The split's info bar below keeps the split, position and close-split
//! buttons; the tab bar holds only tabs.

use super::super::drag::{self, DragAction, DragPayload, DropTarget};
use super::super::pane::PaneContent;
use super::super::App;
use super::left_panel::{SessionListActions, BADGE_R};
use crate::editor_group::{EditorGroup, GroupId};
use crate::theme;
use crate::ui_kit;

/// Width of the marker drawn in the gap a dragged tab would drop into.
const DROP_MARKER_W: f32 = 3.0;
/// Accent line under the focused split's tab bar, when there are splits.
const FOCUS_LINE_H: f32 = 2.0;
const CLOSE_ICON: &str = "\u{00d7}";

/// One tab to draw: a pane the split holds that the session filters show.
struct Tab {
    pane_id: u32,
    pane_idx: usize,
    /// Place in the split's own order (`EditorGroup::pane_ids`).
    order: usize,
    label: String,
    ws_color: Option<[u8; 3]>,
    dormant: bool,
}

impl App {
    /// Draw a split's tab bar in `rect`. What the user does is collected in
    /// `self.tab_bar_actions` and run after the splits are drawn.
    pub(in crate::app) fn render_tab_bar(
        &mut self,
        ui: &mut egui::Ui,
        group: &EditorGroup,
        group_id: GroupId,
        is_focused: bool,
        rect: egui::Rect,
    ) {
        let t = theme::active();
        let marks_focus = is_focused && self.pane_state.groups.len() > 1;
        let bar_bg = if marks_focus {
            theme::lerp_color(t.bg_toolbar, t.surface1, theme::BLEND_LIGHT)
        } else {
            t.bg_toolbar
        };
        ui.painter().rect_filled(rect, theme::R_NONE, bar_bg);
        ui.painter().hline(
            rect.x_range(),
            rect.max.y - theme::STROKE_THIN / 2.0,
            egui::Stroke::new(theme::STROKE_THIN, t.border_subtle),
        );

        // Labels take a session read lock each; built once per frame here,
        // never inside the drawing loop.
        let tabs: Vec<Tab> = group
            .pane_ids
            .iter()
            .enumerate()
            .filter_map(|(order, &pane_id)| {
                let pane_idx = self.pane_state.panes.iter().position(|p| p.id == pane_id)?;
                if !self.pane_passes_tab_filter(&self.pane_state.panes[pane_idx]) {
                    return None;
                }
                Some(Tab {
                    pane_id,
                    pane_idx,
                    order,
                    label: self.tab_display_text(pane_idx),
                    ws_color: self.ws_color_for_pane(pane_idx),
                    dormant: self.pane_is_dormant(pane_idx),
                })
            })
            .collect();

        // Bring the tab the split shows into view whenever that changes
        // (opened, clicked in the list, ⇧⌘], …), without fighting the user's
        // own scrolling the rest of the time.
        let shown_key = self.vp_id("tab_bar_shown").with(group_id);
        let newly_shown = group
            .active_pane_id
            .filter(|&active| ui.ctx().data(|d| d.get_temp::<u32>(shown_key)) != Some(active));
        if let Some(active) = newly_shown {
            ui.ctx().data_mut(|d| d.insert_temp(shown_key, active));
        }

        let mut actions = std::mem::take(&mut self.tab_bar_actions);
        let mut tab_rects: Vec<(usize, egui::Rect)> = Vec::with_capacity(tabs.len());

        let mut bar_ui = ui.child_ui(rect, egui::Layout::left_to_right(egui::Align::Center), None);
        bar_ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        bar_ui.spacing_mut().scroll.floating_allocated_width = 0.0;
        egui::ScrollArea::horizontal()
            .id_source(self.vp_id(&format!("tab_bar_scroll_{group_id}")))
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(&mut bar_ui, |ui| {
                ui.spacing_mut().item_spacing.x = theme::SP_0;
                ui.horizontal(|ui| {
                    for tab in &tabs {
                        let is_active = group.active_pane_id == Some(tab.pane_id);
                        let tab_rect =
                            self.render_tab(ui, tab, is_active, rect.height(), &mut actions);
                        if newly_shown == Some(tab.pane_id) {
                            ui.scroll_to_rect(tab_rect, None);
                        }
                        tab_rects.push((tab.order, tab_rect));
                    }
                });
            });

        if marks_focus {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(rect.min.x, rect.max.y - FOCUS_LINE_H),
                    rect.max,
                ),
                theme::R_NONE,
                t.accent.gamma_multiply(theme::BLEND_MEDIUM),
            );
        }

        self.hit_test_tab_bar_drop(ui, rect, group, group_id, &tab_rects);
        self.tab_bar_actions = actions;
    }

    /// Draw one tab and collect what is done to it. Returns its rect.
    fn render_tab(
        &mut self,
        ui: &mut egui::Ui,
        tab: &Tab,
        is_active: bool,
        tab_h: f32,
        actions: &mut SessionListActions,
    ) -> egui::Rect {
        let t = theme::active();
        let pane_id = tab.pane_id;
        let tab_w = if is_active {
            theme::TAB_W_ACTIVE
        } else {
            theme::TAB_W
        };
        let (tab_rect, _) = ui.allocate_exact_size(egui::vec2(tab_w, tab_h), egui::Sense::hover());
        let resp = ui.interact(
            tab_rect,
            self.vp_id("tab_bar_tab").with(pane_id),
            egui::Sense::click_and_drag(),
        );
        resp.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                is_active,
                &tab.label,
            )
        });
        let renaming = self.rename_pane_id == Some(pane_id) && self.renaming_in_tab_bar();
        if resp.hovered() && !renaming {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let hover_t = super::animation::animated_hover(
            ui.ctx(),
            self.vp_id("tab_bar_hover").with(pane_id),
            resp.hovered(),
        );

        let painter = ui.painter_at(tab_rect);
        let top_rounding = egui::Rounding {
            nw: theme::R_MD,
            ne: theme::R_MD,
            sw: theme::R_NONE,
            se: theme::R_NONE,
        };
        if is_active {
            painter.rect_filled(tab_rect, top_rounding, t.bg_tab_active);
            painter.rect_stroke(
                tab_rect,
                top_rounding,
                egui::Stroke::new(theme::STROKE_THIN, t.border_subtle),
            );
        } else if hover_t > 0.0 {
            painter.rect_filled(
                tab_rect,
                top_rounding,
                theme::lerp_color(egui::Color32::TRANSPARENT, t.bg_row_hover, hover_t),
            );
        }
        // Copy path, a new session landing here, … — the same flash as the row.
        self.flash.render_on_rect(
            &painter,
            tab_rect,
            crate::app::feedback::FlashTarget::Tab(pane_id),
        );
        if let Some(c) = tab.ws_color {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    tab_rect.min,
                    egui::vec2(theme::TAB_COLOR_STRIP_W, tab_h),
                ),
                theme::R_NONE,
                theme::from_rgb(c),
            );
        }
        painter.vline(
            tab_rect.max.x - theme::STROKE_THIN / 2.0,
            tab_rect.y_range(),
            egui::Stroke::new(theme::STROKE_THIN, t.border_subtle),
        );

        let strip_w = tab.ws_color.map_or(0.0, |_| theme::TAB_COLOR_STRIP_W);
        // The tab the split shows is marked by its fill and outline alone: a
        // dot would double the one many shells already put in the title.
        let text_x = tab_rect.min.x + theme::TAB_PAD_X + strip_w;

        // ✕: shown on the active tab and on hover, like the session row.
        let close_rect = egui::Rect::from_min_size(
            egui::pos2(tab_rect.max.x - theme::BTN_W, tab_rect.min.y),
            egui::vec2(theme::BTN_W, tab_h),
        );
        let close_t = super::animation::animated_hover(
            ui.ctx(),
            self.vp_id("tab_bar_close_anim").with(pane_id),
            is_active || resp.hovered(),
        );
        let close_resp = (close_t > 0.0).then(|| {
            ui_kit::icon_button(
                ui,
                self.vp_id("tab_bar_close").with(pane_id),
                close_rect,
                CLOSE_ICON,
                theme::FONT_TERM,
                t.danger_fg.gamma_multiply(close_t),
                ui_kit::IconButtonStyle::Danger,
            )
            .on_hover_text(
                self.shortcut_tooltip("Close tab", crate::shortcuts::AppAction::CloseCurrentPane),
            )
        });
        let close_hovered = close_resp.as_ref().is_some_and(|r| r.hovered());
        let close_clicked = close_resp.as_ref().is_some_and(|r| r.clicked());

        let text_color = if tab.dormant {
            t.overlay0
        } else if is_active {
            t.text
        } else {
            theme::lerp_color(t.subtext0, t.subtext1, hover_t)
        };
        let text_max_x = close_rect.min.x - theme::SP_1;
        if renaming {
            let edit_rect = egui::Rect::from_min_max(
                egui::pos2(text_x, tab_rect.min.y + theme::SP_1),
                egui::pos2(text_max_x.max(text_x), tab_rect.max.y - theme::SP_1),
            );
            self.render_rename_field(ui, edit_rect, text_color, pane_id, actions);
        } else {
            let galley = ui_kit::one_line_elided(
                ui,
                &tab.label,
                egui::FontId::proportional(theme::FONT_UI_MD),
                text_color,
                text_max_x - text_x,
            );
            painter.galley(
                egui::pos2(text_x, tab_rect.center().y - galley.size().y / 2.0),
                galley,
                text_color,
            );
        }

        // A command finished while the tab was in the background.
        if let PaneContent::Terminal(sid) = &self.pane_state.panes[tab.pane_idx].content {
            if self.completed_badges.contains(sid) {
                painter.circle_filled(
                    egui::pos2(
                        tab_rect.min.x + strip_w + theme::SP_1 + BADGE_R,
                        tab_rect.center().y,
                    ),
                    BADGE_R,
                    t.green,
                );
            }
        }

        // The full name (cut short on the tab), its folder and workspace.
        // Built only while hovered: it takes a session read lock.
        let resp = if resp.hovered() && !close_hovered && !renaming {
            ui_kit::full_hover_text(resp, &self.tab_tooltip_text(tab.pane_idx))
        } else {
            resp
        };

        if !renaming {
            if close_clicked || resp.clicked_by(egui::PointerButton::Middle) {
                actions.quit_pane_id = Some(pane_id);
            } else if resp.double_clicked() && self.can_rename_pane(tab.pane_idx) {
                actions.start_rename_pane_id = Some(pane_id);
                actions.rename_in_tab_bar = true;
            } else if resp.clicked() {
                actions.clicked_sidebar_pane_id = Some(pane_id);
            }
            if resp.drag_started() {
                let origin = resp.interact_pointer_pos().unwrap_or(tab_rect.center());
                self.drag_state
                    .set_payload(DragPayload::Tab(pane_id), origin, &tab.label);
            }
        }
        resp.context_menu(|ui| self.pane_menu(ui, tab.pane_idx, true, actions));
        tab_rect
    }

    /// While something is dragged over this tab bar, mark the gap it would
    /// drop into and make that the drop target. A drop that would change
    /// nothing (a tab next to itself) shows no marker.
    fn hit_test_tab_bar_drop(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        group: &EditorGroup,
        group_id: GroupId,
        tab_rects: &[(usize, egui::Rect)],
    ) {
        if !self.drag_state.is_active() {
            return;
        }
        let Some(pos) = ui.ctx().input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if !rect.contains(pos) {
            return;
        }
        let Some(payload) = self.drag_state.payload.clone() else {
            return;
        };
        let visible: Vec<(usize, egui::Rect)> = tab_rects
            .iter()
            .copied()
            .filter(|(_, r)| r.max.x > rect.min.x && r.min.x < rect.max.x)
            .collect();
        let (position, marker_x) = drop_gap(&visible, pos.x, group.pane_ids.len(), rect.min.x);
        let target = DropTarget::GroupTabBar { group_id, position };
        if matches!(
            drag::resolve_group(payload, target.clone(), &self.pane_state),
            DragAction::Noop
        ) {
            return;
        }
        let x = marker_x.clamp(rect.min.x, rect.max.x - DROP_MARKER_W);
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(x, rect.min.y),
                egui::vec2(DROP_MARKER_W, rect.height()),
            ),
            theme::R_NONE,
            theme::active().blue,
        );
        self.drag_state.drop_target = Some(target);
    }
}

/// The gap a drop at `pointer_x` goes into, as a position in the split's
/// order, and where to draw its marker. Before the first tab whose middle is
/// right of the pointer; after the last one otherwise. `tabs` are the drawn
/// tabs, left to right, with their place in the split's order.
fn drop_gap(
    tabs: &[(usize, egui::Rect)],
    pointer_x: f32,
    tab_count: usize,
    bar_left: f32,
) -> (usize, f32) {
    match tabs.iter().find(|(_, r)| pointer_x < r.center().x) {
        Some(&(order, r)) => (order, r.min.x),
        None => tabs
            .last()
            .map_or((tab_count, bar_left), |&(order, r)| (order + 1, r.max.x)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(order: usize, x: f32) -> (usize, egui::Rect) {
        (
            order,
            egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(100.0, 20.0)),
        )
    }

    #[test]
    fn a_drop_goes_into_the_gap_nearest_the_pointer() {
        let tabs = [tab(0, 0.0), tab(1, 100.0), tab(2, 200.0)];
        assert_eq!(
            drop_gap(&tabs, 10.0, 3, 0.0),
            (0, 0.0),
            "left half of the first"
        );
        assert_eq!(
            drop_gap(&tabs, 60.0, 3, 0.0),
            (1, 100.0),
            "right half of the first"
        );
        assert_eq!(drop_gap(&tabs, 260.0, 3, 0.0), (3, 300.0), "past the last");
    }

    #[test]
    fn a_hidden_tab_keeps_its_place_in_the_order() {
        // Order 1 is filtered out of the bar; dropping after order 2 still
        // counts it.
        let tabs = [tab(0, 0.0), tab(2, 100.0)];
        assert_eq!(drop_gap(&tabs, 160.0, 3, 0.0), (3, 200.0));
        assert_eq!(drop_gap(&tabs, 110.0, 3, 0.0), (2, 100.0));
    }

    #[test]
    fn an_empty_bar_drops_at_the_end() {
        assert_eq!(drop_gap(&[], 50.0, 2, 10.0), (2, 10.0));
    }
}
