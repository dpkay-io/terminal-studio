//! The Markdown preview shortcut (Toggle Markdown preview: ⇧⌘M, Ctrl+Shift+M
//! elsewhere). It does what the 👁 / Preview buttons do for the note or file
//! in use, and works while typing in it.

use super::pane::PaneContent;
use super::ui::notes_header::note_is_markdown;
use super::ui::preview_header::PreviewTips;
use super::App;
use crate::shortcuts::AppAction;

/// What the shortcut toggles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewTarget {
    /// The notes in the right panel.
    NotesPanel,
    /// A file editor or notes pane.
    Pane(u32),
}

/// The note or file in use: the right panel's notes while the keyboard is in
/// them (their text or their preview), else the active pane when it can
/// preview, else the right panel's notes when they show. `previewable_pane`
/// is the active pane only when it can preview. Pure.
fn preview_target(
    panel_has_keyboard: bool,
    panel_can_preview: bool,
    previewable_pane: Option<u32>,
) -> Option<PreviewTarget> {
    if panel_has_keyboard {
        // The notes being typed in, or nothing: never a pane elsewhere.
        return panel_can_preview.then_some(PreviewTarget::NotesPanel);
    }
    previewable_pane
        .map(PreviewTarget::Pane)
        .or(panel_can_preview.then_some(PreviewTarget::NotesPanel))
}

impl App {
    /// The preview toggles' hover text, with the shortcut.
    pub(super) fn preview_tips(&self) -> PreviewTips {
        PreviewTips::new(|tip| self.shortcut_tooltip(tip, AppAction::ToggleMarkdownPreview))
    }

    /// The right panel's notes preview, which takes the keyboard when
    /// clicked (as their text box does), so the shortcut finds them there.
    pub(super) fn notes_preview_id(&self) -> egui::Id {
        self.vp_id("notes_preview")
    }

    /// Lets the right panel's preview hold the keyboard, as its text box
    /// would, so the shortcut finds these notes after a click in them. Call
    /// before drawing the preview: the area lies under it and senses no
    /// clicks, so the preview's links still take theirs.
    pub(super) fn notes_preview_takes_keyboard(&self, ui: &mut egui::Ui) {
        let area = ui.interact(
            ui.available_rect_before_wrap(),
            self.notes_preview_id(),
            egui::Sense::focusable_noninteractive(),
        );
        // egui gives a widget's focus up on a click over any other widget
        // (a link here) as it draws it, so this comes after.
        let clicked_inside =
            ui.input(|i| i.pointer.any_click()) && ui.rect_contains_pointer(area.rect);
        if clicked_inside {
            area.request_focus();
        }
    }

    /// Runs the shortcut, unless a dialog, a menu or another text field
    /// (a rename, a filter) has the keyboard.
    pub(super) fn run_preview_shortcut(&mut self, ctx: &egui::Context) {
        if self.any_modal_open() || !self.preview_keys_apply(ctx) {
            return;
        }
        while ctx
            .input_mut(|i| {
                self.shortcut_registry
                    .consume_where(i, |a| a == AppAction::ToggleMarkdownPreview)
            })
            .is_some()
        {
            self.toggle_markdown_preview(ctx);
        }
    }

    /// Flips a file editor's or notes pane's preview, as its button does.
    pub(super) fn toggle_pane_preview(&mut self, pane_id: u32) {
        let Some(pane) = self.pane_state.panes.iter_mut().find(|p| p.id == pane_id) else {
            return;
        };
        match pane.content {
            PaneContent::FileEditor(ref mut ed) => {
                ed.show_preview = !ed.show_preview;
                self.md_prefer_preview = ed.show_preview;
            }
            PaneContent::NoteEditor(ref mut ne) => ne.show_preview = !ne.show_preview,
            _ => {}
        }
    }

    /// The keyboard is nowhere, in the active terminal, in the active
    /// pane's text box or in the right panel's notes.
    fn preview_keys_apply(&self, ctx: &egui::Context) -> bool {
        let focus = self.keyboard_focus(ctx);
        self.text_box_layout_keys_apply(ctx)
            || (focus.is_some()
                && focus == self.active_term_ui_id
                && !super::floating_layer_open(ctx))
    }

    /// Toggles the preview of the note or file in use (the shortcut, the
    /// command palette).
    pub(super) fn toggle_markdown_preview(&mut self, ctx: &egui::Context) {
        let panel_has_keyboard = self.notes_has_focus(ctx);
        let target = preview_target(
            panel_has_keyboard,
            self.notes_panel_can_preview(),
            self.previewable_active_pane(),
        );
        match target {
            Some(PreviewTarget::NotesPanel) => {
                self.notes_panel_preview = !self.notes_panel_preview;
                if panel_has_keyboard {
                    // The keyboard goes to what shows now: back in the text
                    // the user goes on typing, and the next press finds
                    // these notes again either way.
                    let shown = if self.notes_panel_preview {
                        self.notes_preview_id()
                    } else {
                        self.notes_text_id()
                    };
                    ctx.memory_mut(|m| m.request_focus(shown));
                }
            }
            // Back in the text, the pane's text box takes the keyboard
            // itself (it does whenever its pane is focused).
            Some(PreviewTarget::Pane(pane_id)) => self.toggle_pane_preview(pane_id),
            None => return,
        }
        ctx.request_repaint();
    }

    /// The right panel's notes show and are Markdown.
    fn notes_panel_can_preview(&self) -> bool {
        self.show_right_panel
            && !self.notes_panel_collapsed
            && note_is_markdown(self.note_store.link(self.active_group))
    }

    /// The active pane, when it is Markdown: a file editor or a note.
    fn previewable_active_pane(&self) -> Option<u32> {
        let pane = self.pane_state.find(self.pane_state.active_pane_id?)?;
        let markdown = match &pane.content {
            PaneContent::FileEditor(ed) => ed.is_markdown(),
            PaneContent::NoteEditor(ne) => note_is_markdown(self.note_store.link(ne.workspace_id)),
            _ => false,
        };
        markdown.then_some(pane.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANE: Option<u32> = Some(7);

    #[test]
    fn the_notes_being_typed_in_come_first() {
        assert_eq!(
            preview_target(true, true, PANE),
            Some(PreviewTarget::NotesPanel)
        );
    }

    #[test]
    fn notes_that_cannot_preview_never_toggle_a_pane_elsewhere() {
        assert_eq!(preview_target(true, false, PANE), None);
    }

    #[test]
    fn otherwise_the_active_pane_then_the_shown_notes() {
        assert_eq!(
            preview_target(false, true, PANE),
            Some(PreviewTarget::Pane(7))
        );
        assert_eq!(
            preview_target(false, true, None),
            Some(PreviewTarget::NotesPanel)
        );
        assert_eq!(preview_target(false, false, None), None);
    }
}
