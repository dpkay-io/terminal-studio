use crate::theme;

/// Outcome returned by [`search_bar`] so the caller can react to user actions.
pub(crate) struct SearchBarResponse {
    /// The inner [`egui::Response`] of the text input.
    pub response: egui::Response,
    /// `true` when the user pressed Escape while the input was focused.
    pub escaped: bool,
}

pub(crate) fn search_bar(
    ui: &mut egui::Ui,
    query: &mut String,
    icon: &str,
    hint: &str,
    id: egui::Id,
) -> SearchBarResponse {
    search_bar_inner(ui, query, icon, hint, id, false)
}

pub(crate) fn search_bar_persistent(
    ui: &mut egui::Ui,
    query: &mut String,
    icon: &str,
    hint: &str,
    id: egui::Id,
    request_focus: bool,
) -> SearchBarResponse {
    search_bar_inner(ui, query, icon, hint, id, request_focus)
}

fn search_bar_inner(
    ui: &mut egui::Ui,
    query: &mut String,
    icon: &str,
    hint: &str,
    id: egui::Id,
    request_focus: bool,
) -> SearchBarResponse {
    let mut escaped = false;
    let mut inner_resp: Option<egui::Response> = None;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icon).size(theme::FONT_UI_MD));
        let te = egui::TextEdit::singleline(query)
            .desired_width(ui.available_width() - theme::BTN_W)
            .hint_text(hint)
            .font(egui::FontId::proportional(theme::FONT_UI_MD))
            .id(id);
        let r = ui.add(te);
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            escaped = true;
        }
        if request_focus {
            r.request_focus();
        }
        inner_resp = Some(r);
    });

    SearchBarResponse {
        response: inner_resp.unwrap(),
        escaped,
    }
}
