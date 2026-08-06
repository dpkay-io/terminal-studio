use super::super::pane::PaneContent;
use super::super::settings::InfoBarPosition;
use super::super::App;
use crate::theme;
use crate::ui_kit::{icon_button, IconButtonStyle};

const BTN_SIZE: f32 = 16.0;
const TOGGLE_ICON_SIZE: f32 = 14.0;

pub(in crate::app) enum InfoBarAction {
    None,
    SetPosition(InfoBarPosition),
}

impl App {
    pub(in crate::app) fn render_info_bar(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        active_pane_id: Option<u32>,
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

        ui.painter().rect_filled(rect, 0.0, bg);

        let pad_x = theme::SP_4;
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + pad_x, rect.min.y),
            egui::pos2(
                rect.max.x - pad_x - BTN_SIZE * 2.0 - theme::SP_2,
                rect.max.y,
            ),
        );

        // Text segments: session · workspace  ⎇ branch
        let title_font = egui::FontId::proportional(theme::FONT_UI_SM);
        let branch_font = egui::FontId::proportional(theme::FONT_UI_XS);

        let mut segments: Vec<(std::sync::Arc<egui::Galley>, egui::Color32)> = Vec::new();

        if let Some(ref title) = session_title {
            segments.push((
                ui.fonts(|f| f.layout_no_wrap(title.clone(), title_font.clone(), fg)),
                fg,
            ));
        }
        if let Some(ref ws) = ws_name {
            if !segments.is_empty() {
                segments.push((
                    ui.fonts(|f| f.layout_no_wrap(" · ".into(), title_font.clone(), dim)),
                    dim,
                ));
            }
            segments.push((
                ui.fonts(|f| f.layout_no_wrap(ws.clone(), title_font.clone(), dim)),
                dim,
            ));
        }
        if let Some(ref branch) = git_branch {
            if !branch.is_empty() {
                segments.push((
                    ui.fonts(|f| {
                        f.layout_no_wrap(
                            format!("  \u{2387} {branch}"),
                            branch_font.clone(),
                            extra_dim,
                        )
                    }),
                    extra_dim,
                ));
            }
        }

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

        // Ellipsis if text overflows
        if total_w > avail_w {
            let ellipsis_rect = egui::Rect::from_min_size(
                egui::pos2(text_rect.max.x - 16.0, text_rect.min.y),
                egui::vec2(16.0, text_rect.height()),
            );
            ui.painter().rect_filled(ellipsis_rect, 0.0, bg);
            ui.painter().text(
                egui::pos2(text_rect.max.x - 2.0, text_rect.center().y),
                egui::Align2::RIGHT_CENTER,
                "…",
                egui::FontId::proportional(theme::FONT_UI_XS),
                dim,
            );
        }

        // Action buttons on the right
        let is_top = self.settings.info_bar_position == InfoBarPosition::Top;
        let toggle_icon = if is_top { "▼" } else { "▲" };
        let toggle_tooltip = if is_top {
            "Move to bottom"
        } else {
            "Move to top"
        };

        let btn_y = rect.center().y - BTN_SIZE / 2.0;
        let hide_rect = egui::Rect::from_min_size(
            egui::pos2(rect.max.x - pad_x - BTN_SIZE, btn_y),
            egui::vec2(BTN_SIZE, BTN_SIZE),
        );
        let toggle_rect = egui::Rect::from_min_size(
            egui::pos2(hide_rect.min.x - BTN_SIZE - theme::SP_1, btn_y),
            egui::vec2(BTN_SIZE, BTN_SIZE),
        );

        let toggle_resp = icon_button(
            ui,
            ui.id().with("info_bar_toggle"),
            toggle_rect,
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
            action = InfoBarAction::SetPosition(new_pos);
        }

        let hide_resp = icon_button(
            ui,
            ui.id().with("info_bar_hide"),
            hide_rect,
            "×",
            theme::ICON_SM,
            fg,
            IconButtonStyle::Default,
        )
        .on_hover_text("Hide info bar");
        if hide_resp.clicked() {
            action = InfoBarAction::SetPosition(InfoBarPosition::Hidden);
        }

        action
    }

    /// Small floating button to re-show the info bar when hidden.
    pub(in crate::app) fn render_info_bar_show_toggle(
        &self,
        ui: &mut egui::Ui,
        content_rect: egui::Rect,
    ) -> bool {
        let t = theme::active();
        let size = TOGGLE_ICON_SIZE;
        let margin = theme::SP_2;
        let btn_rect = egui::Rect::from_min_size(
            egui::pos2(
                content_rect.max.x - size - margin,
                content_rect.min.y + margin,
            ),
            egui::vec2(size, size),
        );

        icon_button(
            ui,
            ui.id().with("info_bar_show"),
            btn_rect,
            "ℹ",
            theme::ICON_SM,
            t.overlay0,
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

        let ws = pane
            .workspace_id
            .and_then(|wid| self.workspace_store.workspaces.iter().find(|w| w.id == wid));
        let ws_name = ws.map(|w| w.name.clone());
        let ws_color = ws.map(|w| w.color);

        let git_branch = pane
            .workspace_id
            .and_then(|wid| self.workers.workspace_git_worker.get(wid))
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
}
