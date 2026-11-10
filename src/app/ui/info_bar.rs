use super::super::pane::PaneContent;
use super::super::settings::InfoBarPosition;
use super::super::App;
use crate::theme;
use crate::ui_kit::{icon, icon_button, IconButtonStyle};

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

        let (session_title, ws_name, _ws_color, git_branch) = self.info_bar_data(active_pane_id);

        ui.painter().rect_filled(rect, theme::R_NONE, t.bg_panel);
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(rect.min.x, rect.max.y - theme::STROKE_THIN),
                rect.max,
            ),
            theme::R_NONE,
            t.border,
        );

        let pad_x = theme::SP_4;
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + pad_x, rect.min.y),
            egui::pos2(
                rect.max.x - pad_x - BTN_SIZE * 2.0 - theme::SP_2,
                rect.max.y,
            ),
        );

        // Breadcrumb: workspace  [git-branch icon] branch. Fall back to the
        // pane title when a pane has no workspace.
        let label = ws_name.as_ref().or(session_title.as_ref());
        let branch = git_branch.as_deref().filter(|branch| !branch.is_empty());
        if label.is_some() || branch.is_some() {
            let text_font = egui::FontId::proportional(theme::FONT_SM);
            let icon_font =
                egui::FontId::new(theme::FONT_SM, egui::FontFamily::Name("icons".into()));
            let mut job = egui::text::LayoutJob::default();

            if let Some(label) = label {
                job.append(
                    label,
                    theme::SP_0,
                    egui::TextFormat {
                        font_id: text_font.clone(),
                        color: t.fg,
                        ..Default::default()
                    },
                );
            }
            if let Some(branch) = branch {
                if label.is_some() {
                    job.append(
                        "  ",
                        theme::SP_0,
                        egui::TextFormat {
                            font_id: text_font.clone(),
                            color: t.fg_muted,
                            ..Default::default()
                        },
                    );
                }
                job.append(
                    &icon::GIT_BRANCH.to_string(),
                    theme::SP_0,
                    egui::TextFormat {
                        font_id: icon_font,
                        color: t.fg_muted,
                        ..Default::default()
                    },
                );
                job.append(
                    &format!(" {branch}"),
                    theme::SP_0,
                    egui::TextFormat {
                        font_id: text_font,
                        color: t.fg_muted,
                        ..Default::default()
                    },
                );
            }

            job.wrap =
                egui::text::TextWrapping::truncate_at_width(text_rect.width().max(theme::SP_0));
            let galley = ui.fonts(|fonts| fonts.layout_job(job));
            ui.painter().with_clip_rect(text_rect).galley(
                egui::pos2(
                    text_rect.min.x,
                    text_rect.center().y - galley.size().y * 0.5,
                ),
                galley,
                egui::Color32::PLACEHOLDER,
            );
        }

        // Action buttons on the right
        let is_top = self.settings.info_bar_position == InfoBarPosition::Top;
        let toggle_icon = if is_top {
            icon::CHEVRON_DOWN
        } else {
            icon::CHEVRON_UP
        };
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
            theme::FONT_XS,
            t.fg_muted,
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
            icon::X,
            theme::FONT_XS,
            t.fg_muted,
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
            icon::ROWS_2,
            theme::FONT_XS,
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
