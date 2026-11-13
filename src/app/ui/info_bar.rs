use super::super::pane::PaneContent;
use super::super::settings::InfoBarPosition;
use super::super::App;
use crate::editor_group::GroupId;
use crate::pane_tree::SplitDir;
use crate::shortcuts::AppAction;
use crate::theme;
use crate::ui_kit::{icon_button, IconButtonStyle};

const BTN_SIZE: f32 = 16.0;
const TOGGLE_ICON_SIZE: f32 = 14.0;
/// Width of the "…" patch painted over overflowing header text.
const ELLIPSIS_W: f32 = 16.0;
/// Hot zone, in reveal-button widths and heights, at the top-right of a pane
/// whose info bar is hidden. The reveal button only appears while the pointer
/// is inside it, so the button never sits on top of terminal text otherwise.
const REVEAL_ZONE_W_BUTTONS: f32 = 3.0;
const REVEAL_ZONE_H_BUTTONS: f32 = 2.0;
const HIDE_TOOLTIP: &str = "Hide info bar (point at a pane's top-right corner to show it again)";

/// Where everything in one info bar goes. The buttons are right-aligned in
/// this order from the right edge: hide, move, close-all, split vertical,
/// split horizontal. The text gets whatever is left of them, so it can never
/// run under a button however many there are.
#[derive(Debug, Clone, Copy, PartialEq)]
struct InfoBarLayout {
    text: egui::Rect,
    hide: egui::Rect,
    toggle: egui::Rect,
    close_all: egui::Rect,
    split_v: egui::Rect,
    split_h: egui::Rect,
}

fn info_bar_layout(rect: egui::Rect) -> InfoBarLayout {
    let pad_x = theme::SP_4;
    let btn_y = rect.center().y - BTN_SIZE / 2.0;
    let step = BTN_SIZE + theme::SP_1;
    let button = |slot: usize| {
        egui::Rect::from_min_size(
            egui::pos2(rect.max.x - pad_x - BTN_SIZE - step * slot as f32, btn_y),
            egui::vec2(BTN_SIZE, BTN_SIZE),
        )
    };
    let split_h = button(4);
    let text_min_x = rect.min.x + pad_x;
    // A narrow split can push the buttons past the left edge; clamping keeps
    // the text rect from inverting, which egui would not draw correctly.
    let text_max_x = (split_h.min.x - theme::SP_2).max(text_min_x);
    InfoBarLayout {
        text: egui::Rect::from_min_max(
            egui::pos2(text_min_x, rect.min.y),
            egui::pos2(text_max_x, rect.max.y),
        ),
        hide: button(0),
        toggle: button(1),
        close_all: button(2),
        split_v: button(3),
        split_h,
    }
}

/// The hot zone that reveals the "show info bar" button on a pane whose bar
/// is hidden, and the button inside it.
fn reveal_zone(content_rect: egui::Rect) -> (egui::Rect, egui::Rect) {
    let size = TOGGLE_ICON_SIZE;
    let margin = theme::SP_2;
    let zone = egui::Rect::from_min_max(
        egui::pos2(
            (content_rect.max.x - size * REVEAL_ZONE_W_BUTTONS - margin).max(content_rect.min.x),
            content_rect.min.y,
        ),
        egui::pos2(
            content_rect.max.x,
            (content_rect.min.y + size * REVEAL_ZONE_H_BUTTONS + margin).min(content_rect.max.y),
        ),
    );
    let button = egui::Rect::from_min_size(
        egui::pos2(
            content_rect.max.x - size - margin,
            content_rect.min.y + margin,
        ),
        egui::vec2(size, size),
    );
    (zone, button)
}

pub(in crate::app) enum InfoBarAction {
    None,
    /// Move or hide one split's info bar; the others keep theirs.
    SetPosition(GroupId, InfoBarPosition),
    Split(GroupId, SplitDir),
    CloseAllInGroup(GroupId),
}

impl App {
    /// Where a split's info bar sits: the split's own choice, else the
    /// app-wide default. The single source for layout and painting.
    pub(in crate::app) fn info_bar_position(&self, group_id: GroupId) -> InfoBarPosition {
        self.pane_state
            .info_bar_position(group_id, self.settings.info_bar_position)
    }

    /// Where a revealed info bar goes: where new splits open theirs, or the
    /// top when an older settings file made hidden the default.
    pub(in crate::app) fn revealed_info_bar_position(&self) -> InfoBarPosition {
        match self.settings.info_bar_position {
            InfoBarPosition::Hidden => InfoBarPosition::Top,
            shown => shown,
        }
    }

    /// The user moved or hid one split's info bar. Moving it also makes that
    /// the default for new splits, saved so it holds after a restart.
    pub(in crate::app) fn choose_info_bar_position(
        &mut self,
        group_id: GroupId,
        position: InfoBarPosition,
    ) {
        let default = self.settings.info_bar_position;
        if let Some(new_default) = self
            .pane_state
            .choose_info_bar_position(group_id, position, default)
        {
            self.settings.info_bar_position = new_default;
            self.settings.save();
            log::debug!("info bar default: {new_default:?}");
        }
    }

    pub(in crate::app) fn render_info_bar(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        active_pane_id: Option<u32>,
        group_id: GroupId,
        is_focused: bool,
    ) -> InfoBarAction {
        let t = theme::active();
        let mut action = InfoBarAction::None;

        let (session_title, ws_name, ws_color, git_branch) = self.info_bar_data(active_pane_id);

        let bg = match ws_color {
            Some(c) => theme::from_rgb(c),
            None => t.surface0,
        };
        let fg = match ws_color {
            Some(c) => theme::text_on(c),
            None => t.subtext1,
        };
        let dim = fg.linear_multiply(0.6);
        let extra_dim = fg.linear_multiply(0.4);

        // The info bar is the only focus indicator now that the tab bar is gone.
        let has_multiple_groups = self.pane_state.groups.len() > 1;
        let bg = if is_focused && has_multiple_groups {
            theme::lerp_color(bg, t.surface1, theme::BLEND_SUBTLE)
        } else {
            bg
        };

        ui.painter().rect_filled(rect, 0.0, bg);

        if is_focused && has_multiple_groups {
            let line_h = 2.0;
            // Sit the marker on whichever edge faces the pane content, so it
            // reads as belonging to that split rather than to its neighbour.
            let line_y = if self.info_bar_position(group_id) == InfoBarPosition::Bottom {
                rect.min.y
            } else {
                rect.max.y - line_h
            };
            ui.painter().rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(rect.min.x, line_y),
                    egui::vec2(rect.width(), line_h),
                ),
                0.0,
                t.accent.gamma_multiply(0.5),
            );
        }

        let layout = info_bar_layout(rect);
        let text_rect = layout.text;

        // Text segments: session · workspace  ⎇ branch
        let title_font = egui::FontId::proportional(theme::FONT_UI_SM);
        let branch_font = egui::FontId::proportional(theme::FONT_UI_XS);

        // One list drives both what is painted and the hover text, so the
        // tooltip always says exactly what the bar would show uncut.
        let mut parts: Vec<(String, &egui::FontId, egui::Color32)> = Vec::new();
        if let Some(ref title) = session_title {
            parts.push((title.clone(), &title_font, fg));
        }
        if let Some(ref ws) = ws_name {
            if !parts.is_empty() {
                parts.push((" · ".into(), &title_font, dim));
            }
            parts.push((ws.clone(), &title_font, dim));
        }
        if let Some(branch) = git_branch.as_deref().filter(|b| !b.is_empty()) {
            parts.push((format!("  \u{2387} {branch}"), &branch_font, extra_dim));
        }
        let segments: Vec<(std::sync::Arc<egui::Galley>, egui::Color32)> = parts
            .iter()
            .map(|(text, font, color)| {
                (
                    ui.fonts(|f| f.layout_no_wrap(text.clone(), (*font).clone(), *color)),
                    *color,
                )
            })
            .collect();

        // Paint segments left-aligned, vertically centered
        let total_w: f32 = segments.iter().map(|(g, _)| g.size().x).sum();
        let avail_w = text_rect.width();
        let mut x = text_rect.min.x;
        let clipped = ui.painter().with_clip_rect(text_rect);
        for (galley, color) in &segments {
            if x - text_rect.min.x > avail_w {
                break;
            }
            let y = text_rect.center().y - galley.size().y / 2.0;
            clipped.galley(egui::pos2(x, y), galley.clone(), *color);
            x += galley.size().x;
        }
        drop(clipped);

        // Ellipsis if text overflows, with the whole text on hover.
        if total_w > avail_w {
            let full_text: String = parts.iter().map(|(text, _, _)| text.as_str()).collect();
            ui.interact(
                text_rect,
                ui.id().with(("info_bar_text", group_id)),
                egui::Sense::hover(),
            )
            .on_hover_text(full_text);
            let ellipsis_rect = egui::Rect::from_min_size(
                egui::pos2(text_rect.max.x - ELLIPSIS_W, text_rect.min.y),
                egui::vec2(ELLIPSIS_W, text_rect.height()),
            );
            ui.painter().rect_filled(ellipsis_rect, 0.0, bg);
            ui.painter().text(
                egui::pos2(text_rect.max.x - theme::SP_1, text_rect.center().y),
                egui::Align2::RIGHT_CENTER,
                "…",
                egui::FontId::proportional(theme::FONT_UI_XS),
                dim,
            );
        }

        // Action buttons on the right
        let is_top = self.info_bar_position(group_id) == InfoBarPosition::Top;
        let toggle_icon = if is_top { "▼" } else { "▲" };
        let toggle_tooltip = if is_top {
            "Move to bottom"
        } else {
            "Move to top"
        };

        // Widget ids carry `group_id`: sibling splits render two info bars per frame.
        if icon_button(
            ui,
            ui.id().with(("info_bar_split_h", group_id)),
            layout.split_h,
            "⊞",
            theme::ICON_SM,
            fg,
            IconButtonStyle::Default,
        )
        .on_hover_text(self.shortcut_tooltip("Split horizontal", AppAction::SplitHorizontal))
        .clicked()
        {
            action = InfoBarAction::Split(group_id, SplitDir::Horizontal);
        }

        if icon_button(
            ui,
            ui.id().with(("info_bar_split_v", group_id)),
            layout.split_v,
            "⊟",
            theme::ICON_SM,
            fg,
            IconButtonStyle::Default,
        )
        .on_hover_text(self.shortcut_tooltip("Split vertical", AppAction::SplitVertical))
        .clicked()
        {
            action = InfoBarAction::Split(group_id, SplitDir::Vertical);
        }

        if icon_button(
            ui,
            ui.id().with(("info_bar_close_all", group_id)),
            layout.close_all,
            "✖",
            theme::ICON_SM,
            fg,
            IconButtonStyle::Danger,
        )
        .on_hover_text("Close all tabs in this split")
        .clicked()
        {
            action = InfoBarAction::CloseAllInGroup(group_id);
        }

        let toggle_resp = icon_button(
            ui,
            ui.id().with("info_bar_toggle"),
            layout.toggle,
            toggle_icon,
            theme::ICON_SM,
            fg,
            IconButtonStyle::Default,
        )
        .on_hover_text(toggle_tooltip);
        if toggle_resp.clicked() {
            let new_pos = if is_top {
                InfoBarPosition::Bottom
            } else {
                InfoBarPosition::Top
            };
            action = InfoBarAction::SetPosition(group_id, new_pos);
        }

        let hide_resp = icon_button(
            ui,
            ui.id().with("info_bar_hide"),
            layout.hide,
            "−",
            theme::ICON_SM,
            fg,
            IconButtonStyle::Default,
        )
        .on_hover_text(HIDE_TOOLTIP);
        if hide_resp.clicked() {
            action = InfoBarAction::SetPosition(group_id, InfoBarPosition::Hidden);
        }

        action
    }

    /// Button that re-shows a hidden info bar. It only appears while the
    /// pointer is in the pane's top-right corner: drawn all the time, it sat
    /// on top of the last characters of the terminal's first rows.
    pub(in crate::app) fn render_info_bar_show_toggle(
        &self,
        ui: &mut egui::Ui,
        content_rect: egui::Rect,
    ) -> bool {
        let (zone, btn_rect) = reveal_zone(content_rect);
        if !ui.rect_contains_pointer(zone) {
            return false;
        }
        let t = theme::active();
        // Opaque backing so the icon reads over whatever text is under it.
        ui.painter()
            .rect_filled(btn_rect.expand(theme::ICON_PAD), theme::R_MD, t.surface0);
        icon_button(
            ui,
            ui.id().with("info_bar_show"),
            btn_rect,
            "ℹ",
            theme::ICON_SM,
            t.text,
            IconButtonStyle::Default,
        )
        .on_hover_text("Show info bar")
        .clicked()
    }

    fn info_bar_data(
        &self,
        active_pane_id: Option<u32>,
    ) -> (
        Option<String>,
        Option<String>,
        Option<[u8; 3]>,
        Option<String>,
    ) {
        let Some(pid) = active_pane_id else {
            return (None, None, None, None);
        };
        let Some(pane) = self.pane_state.panes.iter().find(|p| p.id == pid) else {
            return (None, None, None, None);
        };

        // The same workspace the tab is grouped under, the sidebar and the
        // titlebar show: by the pane's folder, not the workspace that was
        // active when it was opened.
        let ws = Self::pane_workspace(&self.session_state.sessions, &self.workspace_store, pane);
        let ws_name = ws.map(|w| w.name.clone());
        let ws_color = ws.map(|w| w.color);

        let git_branch = ws
            .and_then(|w| self.workers.workspace_git_worker.get(w.id))
            .map(|info| info.branch);

        let session_title = match &pane.content {
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
                        let fg_proc = self.workers.foreground_worker.get(e.id);
                        let ws_label = if cwd.as_os_str().is_empty() {
                            None
                        } else {
                            self.workspace_store
                                .find_for_cwd(&cwd)
                                .map(|w| w.name.clone())
                        };
                        super::super::title::effective_title(
                            &title,
                            &cwd,
                            fg_proc.as_ref(),
                            Some(&e.shell),
                            ws_label.as_deref(),
                        )
                    })
            }
            PaneContent::FileEditor(ed) => Some(
                ed.path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Editor".into()),
            ),
            PaneContent::FileDiff(d) => Some(format!(
                "Diff: {}",
                d.path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )),
            PaneContent::NoteEditor(_) => Some("Notes".into()),
            PaneContent::ConflictResolver(_) => Some("Conflict Resolver".into()),
            PaneContent::DeferredTerminal { .. } => None,
        };

        (session_title, ws_name, ws_color, git_branch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_bar_position_default_is_top() {
        assert_eq!(InfoBarPosition::default(), InfoBarPosition::Top);
    }

    #[test]
    fn info_bar_position_serde_roundtrip() {
        for pos in [
            InfoBarPosition::Top,
            InfoBarPosition::Bottom,
            InfoBarPosition::Hidden,
        ] {
            let json = serde_json::to_string(&pos).unwrap();
            let restored: InfoBarPosition = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, pos);
        }
    }

    fn bar(width: f32) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(100.0, 50.0),
            egui::vec2(width, theme::INFO_BAR_H),
        )
    }

    fn buttons(l: &InfoBarLayout) -> [egui::Rect; 5] {
        [l.hide, l.toggle, l.close_all, l.split_v, l.split_h]
    }

    #[test]
    fn layout_buttons_are_right_aligned_in_order_without_overlap() {
        let rect = bar(600.0);
        let l = info_bar_layout(rect);
        let b = buttons(&l);
        assert_eq!(l.hide.max.x, rect.max.x - theme::SP_4);
        for pair in b.windows(2) {
            // Each button sits left of the previous one, with a gap.
            assert!(pair[1].max.x < pair[0].min.x, "{pair:?}");
        }
        for r in b {
            assert_eq!(r.size(), egui::vec2(BTN_SIZE, BTN_SIZE));
            assert!((r.center().y - rect.center().y).abs() < f32::EPSILON);
            assert!(rect.contains_rect(r));
        }
    }

    #[test]
    fn layout_text_never_runs_under_a_button() {
        for width in [600.0, 200.0, 140.0] {
            let l = info_bar_layout(bar(width));
            for r in buttons(&l) {
                assert!(
                    l.text.max.x <= r.min.x,
                    "width {width}: text {:?} vs {r:?}",
                    l.text
                );
            }
            assert_eq!(l.text.min.x, bar(width).min.x + theme::SP_4);
        }
    }

    #[test]
    fn layout_text_rect_never_inverts_on_a_narrow_split() {
        let l = info_bar_layout(bar(40.0));
        assert!(l.text.width() >= 0.0);
        assert_eq!(l.text.width(), 0.0);
    }

    #[test]
    fn layout_text_spans_the_full_bar_height() {
        let rect = bar(600.0);
        let l = info_bar_layout(rect);
        assert_eq!(l.text.min.y, rect.min.y);
        assert_eq!(l.text.max.y, rect.max.y);
    }

    #[test]
    fn reveal_zone_holds_its_button_in_the_top_right_corner() {
        let content = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 400.0));
        let (zone, button) = reveal_zone(content);
        assert!(zone.contains_rect(button));
        assert!(content.contains_rect(zone));
        assert_eq!(zone.max.x, content.max.x);
        assert_eq!(zone.min.y, content.min.y);
        // Small enough that it only covers the corner, not the first row.
        assert!(zone.width() < content.width() / 4.0);
    }

    #[test]
    fn reveal_zone_stays_inside_a_tiny_pane() {
        let content = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(20.0, 10.0));
        let (zone, _) = reveal_zone(content);
        assert!(content.contains_rect(zone));
    }
}
