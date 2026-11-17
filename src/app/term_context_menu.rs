//! The terminal's right-click menu (Copy, Paste, Search, Select All), and the
//! copy / paste actions it shares with ⌘C / ⌘V.
//!
//! The menu is drawn on every frame it is open, apart from the terminal's
//! input handling. That handling stands down while any floating layer is
//! open, and the menu is one: drawn from inside it, the menu showed only on
//! every other frame, so egui restarted its fade-in each time and the menu
//! stayed nearly see-through, under the terminal's text and selection.

use alacritty_terminal::{
    grid::{Dimensions, Scroll},
    term::TermMode,
};

use super::{feedback, input, App, TermSelection};
use crate::shortcuts::AppAction;

/// Base of the menu's popup id (made per window by [`App::vp_id`]).
const POPUP_ID: &str = "term_context_menu";
/// Width of the point the menu opens below; the menu is at least this wide.
const ANCHOR_WIDTH: f32 = 160.0;
/// Height of that anchor: a line under the pointer.
const ANCHOR_HEIGHT: f32 = 1.0;
/// Narrowest the menu gets.
const MENU_MIN_WIDTH: f32 = 140.0;
/// Height of each menu item, so every row is an easy click target.
const ITEM_HEIGHT: f32 = 22.0;

/// What a click in the menu asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TermMenuAction {
    Copy,
    Paste,
    Search,
    SelectAll,
}

/// Draws the menu's items and returns the one clicked. Copy is disabled
/// when nothing is selected.
pub(super) fn menu_items(
    ui: &mut egui::Ui,
    has_selection: bool,
    search_label: &str,
) -> Option<TermMenuAction> {
    let item = |label: &str| egui::Button::new(label).min_size(egui::vec2(0.0, ITEM_HEIGHT));
    let mut action = None;
    ui.set_min_width(MENU_MIN_WIDTH);
    if ui.add_enabled(has_selection, item("Copy")).clicked() {
        action = Some(TermMenuAction::Copy);
    }
    if ui.add(item("Paste")).clicked() {
        action = Some(TermMenuAction::Paste);
    }
    ui.separator();
    if ui.add(item(search_label)).clicked() {
        action = Some(TermMenuAction::Search);
    }
    if ui.add(item("Select All")).clicked() {
        action = Some(TermMenuAction::SelectAll);
    }
    action
}

/// Shows the open menu below `pos` (a popup on egui's foreground layer)
/// and returns the item clicked. Call it on every frame the popup is open.
pub(super) fn show_menu_popup(
    ui: &mut egui::Ui,
    popup_id: egui::Id,
    pos: egui::Pos2,
    has_selection: bool,
    search_label: &str,
) -> Option<TermMenuAction> {
    let anchor = ui.interact(
        egui::Rect::from_min_size(pos, egui::vec2(ANCHOR_WIDTH, ANCHOR_HEIGHT)),
        popup_id.with("anchor"),
        egui::Sense::hover(),
    );
    egui::Popup::from_response(&anchor)
        .id(popup_id)
        // Open/closed stays in egui's memory: `open_terminal_context_menu`
        // opens it, a pick or a click outside closes it.
        .open_memory(None)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| menu_items(ui, has_selection, search_label))
        .and_then(|r| r.inner)
}

impl App {
    fn term_menu_popup_id(&self) -> egui::Id {
        self.vp_id(POPUP_ID)
    }

    /// Opens the menu at `pos` (a right-click on the terminal).
    pub(super) fn open_terminal_context_menu(&mut self, ctx: &egui::Context, pos: egui::Pos2) {
        self.context_menu_pos = Some(pos);
        let popup_id = self.term_menu_popup_id();
        egui::Popup::open_id(ctx, popup_id);
    }

    /// Draws the menu while it is open and runs what is clicked. Runs every
    /// frame, whatever has the keyboard (see the module notes).
    pub(super) fn show_terminal_context_menu(
        &mut self,
        ui: &mut egui::Ui,
        active_session_id: Option<u32>,
    ) {
        let popup_id = self.term_menu_popup_id();
        if !egui::Popup::is_id_open(ui.ctx(), popup_id) {
            self.context_menu_pos = None;
            return;
        }
        let Some(pos) = self.context_menu_pos else {
            return;
        };
        let has_selection = active_session_id.is_some_and(|sid| self.has_selection_in(sid));
        let search_label = self.shortcut_tooltip("Search", AppAction::SearchTerminal);
        let Some(action) = show_menu_popup(ui, popup_id, pos, has_selection, &search_label) else {
            return;
        };
        match (action, active_session_id) {
            (TermMenuAction::Copy, Some(sid)) => {
                self.copy_terminal_selection(ui.ctx(), sid);
            }
            (TermMenuAction::Paste, Some(sid)) => {
                if let Some(text) = read_clipboard_text() {
                    self.paste_into_terminal(sid, &text);
                }
            }
            (TermMenuAction::Search, _) => self.term_search.active = true,
            (TermMenuAction::SelectAll, Some(sid)) => self.select_all_visible(sid),
            (_, None) => {}
        }
        egui::Popup::close_id(ui.ctx(), popup_id);
        self.context_menu_pos = None;
    }

    /// Whether `sid` has a terminal selection.
    pub(super) fn has_selection_in(&self, sid: u32) -> bool {
        self.term_selection.is_some() && self.term_selection_sid == Some(sid)
    }

    /// Copies `sid`'s selection to the clipboard, flashes the pane and
    /// clears the selection. Returns false (and does nothing) when `sid` has
    /// no selection.
    pub(super) fn copy_terminal_selection(&mut self, ctx: &egui::Context, sid: u32) -> bool {
        if !self.has_selection_in(sid) {
            return false;
        }
        if let Some(idx) = self.session_state.sessions.iter().position(|e| e.id == sid) {
            let text = self.extract_selected_text(idx);
            if !text.is_empty() {
                ctx.copy_text(text);
                self.flash_active_pane();
            }
        }
        self.term_selection = None;
        self.term_selection_sid = None;
        true
    }

    /// Pastes `text` into `sid` (bracketed when the program asked for it),
    /// scrolled to the bottom, and flashes the pane.
    pub(super) fn paste_into_terminal(&mut self, sid: u32, text: &str) {
        self.scroll_accum.remove(&sid);
        let Some(entry) = self.session_state.find(sid) else {
            return;
        };
        let bracketed = {
            let mut session = entry.session.write();
            session.term.scroll_display(Scroll::Bottom);
            session.term.mode().contains(TermMode::BRACKETED_PASTE)
        };
        let _ = entry.pty_tx.try_send(input::paste_bytes(text, bracketed));
        self.flash_active_pane();
    }

    fn flash_active_pane(&mut self) {
        if let Some(pid) = self.pane_state.active_pane_id {
            self.flash.trigger(
                feedback::FlashTarget::Pane(pid),
                feedback::FlashKind::Neutral,
            );
        }
    }

    /// Selects every visible line of `sid`.
    fn select_all_visible(&mut self, sid: u32) {
        let Some(geo) = &self.active_term_geo else {
            return;
        };
        let Some(entry) = self.session_state.find(sid) else {
            return;
        };
        let visible_rows = (geo.rect.height() / geo.cell_h) as u16;
        let (cols, display_offset) = {
            let session = entry.session.read();
            (
                session.term.columns() as u16,
                session.term.grid().display_offset(),
            )
        };
        self.term_selection = Some(TermSelection {
            start_col: 0,
            start_row: 0,
            end_col: cols.saturating_sub(1),
            end_row: visible_rows.saturating_sub(1),
            display_offset,
        });
        self.term_selection_sid = Some(sid);
    }
}

/// The clipboard's text, if it holds any.
fn read_clipboard_text() -> Option<String> {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.get_text())
        .map_err(|e| log::debug!("clipboard read failed: {e}"))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const POS: egui::Pos2 = egui::pos2(40.0, 40.0);

    fn frame(ctx: &egui::Context, time: f64, run: impl FnMut(&mut egui::Ui)) {
        let mut run = run;
        let input = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let _ = crate::egui_test_frames::run_ui(ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| run(ui));
        });
    }

    fn popup_layer(id: egui::Id) -> egui::LayerId {
        egui::LayerId::new(egui::Order::Foreground, id)
    }

    /// Drawn on every frame, the menu's layer stays visible from one frame
    /// to the next, so egui's fade-in runs once and the menu ends opaque.
    #[test]
    fn menu_drawn_every_frame_stays_visible() {
        let ctx = egui::Context::default();
        let id = egui::Id::unique("menu_test");
        egui::Popup::open_id(&ctx, id);
        for i in 0..5 {
            frame(&ctx, i as f64 * 0.1, |ui| {
                show_menu_popup(ui, id, POS, true, "Search");
            });
            assert!(
                ctx.memory(|m| m.areas().visible_last_frame(&popup_layer(id))),
                "frame {i}: the menu's layer was not visible"
            );
        }
    }

    /// The bug this module fixes: skipping every other frame leaves the
    /// layer invisible on the frames after a skip, so the fade restarts.
    #[test]
    fn menu_skipped_on_alternate_frames_is_not_visible_after_a_skip() {
        let ctx = egui::Context::default();
        let id = egui::Id::unique("menu_test");
        egui::Popup::open_id(&ctx, id);
        frame(&ctx, 0.0, |ui| {
            show_menu_popup(ui, id, POS, true, "Search");
        });
        frame(&ctx, 0.1, |_| {});
        assert!(!ctx.memory(|m| m.areas().visible_last_frame(&popup_layer(id))));
    }

    #[test]
    fn nothing_clicked_gives_no_action() {
        let ctx = egui::Context::default();
        let id = egui::Id::unique("menu_test");
        egui::Popup::open_id(&ctx, id);
        let mut action = Some(TermMenuAction::Paste);
        frame(&ctx, 0.0, |ui| {
            action = show_menu_popup(ui, id, POS, false, "Search");
        });
        assert_eq!(action, None);
    }

    /// One frame of a menu drawn straight into a panel. Returns the action
    /// clicked and where the first item (Copy) sits.
    fn menu_frame(
        ctx: &egui::Context,
        has_selection: bool,
        events: Vec<egui::Event>,
        time: f64,
    ) -> (Option<TermMenuAction>, egui::Rect) {
        let mut action = None;
        let mut copy_rect = egui::Rect::NOTHING;
        let input = egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let _ = crate::egui_test_frames::run_ui(ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let top = ui.cursor().min;
                action = menu_items(ui, has_selection, "Search");
                copy_rect = egui::Rect::from_min_size(top, egui::vec2(MENU_MIN_WIDTH, ITEM_HEIGHT));
            });
        });
        (action, copy_rect)
    }

    /// Clicks the first item (Copy): press, then release, at its centre.
    fn click_copy(has_selection: bool) -> Option<TermMenuAction> {
        let ctx = egui::Context::default();
        let (_, copy_rect) = menu_frame(&ctx, has_selection, vec![], 0.0);
        // Inside the button: it is only as wide as its label.
        let at = copy_rect.left_center() + egui::vec2(ITEM_HEIGHT / 2.0, 0.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let (pressed, _) = menu_frame(
            &ctx,
            has_selection,
            vec![egui::Event::PointerMoved(at), button(true)],
            0.1,
        );
        let (released, _) = menu_frame(&ctx, has_selection, vec![button(false)], 0.2);
        pressed.or(released)
    }

    #[test]
    fn copy_click_with_a_selection_copies() {
        assert_eq!(click_copy(true), Some(TermMenuAction::Copy));
    }

    #[test]
    fn copy_is_disabled_without_a_selection() {
        assert_eq!(click_copy(false), None);
    }

    #[test]
    fn closed_menu_draws_nothing() {
        let ctx = egui::Context::default();
        let id = egui::Id::unique("menu_test");
        frame(&ctx, 0.0, |ui| {
            show_menu_popup(ui, id, POS, true, "Search");
        });
        assert!(!ctx.memory(|m| m.areas().visible_last_frame(&popup_layer(id))));
    }
}
