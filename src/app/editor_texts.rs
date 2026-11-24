//! The text the editor panes edit during one frame.
//!
//! A text box edits a `String` of its own, so a file or note editor's text
//! is copied out of its pane (file) or the note store (note) for the frame.
//! The copy is made when the pane's text box is drawn, so panes not on
//! screen, previews and hidden splits cost nothing, and only a text box that
//! reports an edit is written back: nothing is compared after the frame.

use super::pane::{PaneContent, PaneEntry};
use crate::notes::NoteStore;

/// One editor pane's text, copied out for its text box this frame.
pub(in crate::app) struct EditorText {
    pub pane_id: u32,
    pub text: String,
    /// Its text box changed the text this frame.
    pub edited: bool,
}

/// The editor texts of one frame, made fresh every frame.
#[derive(Default)]
pub(in crate::app) struct EditorTexts {
    texts: Vec<EditorText>,
}

impl EditorTexts {
    /// The pane's text for its text box to edit: copied from `load` the
    /// first time it is asked for this frame. `None` when `load` has none
    /// (the pane is not an editor).
    pub fn get_or_load<'s>(
        &mut self,
        pane_id: u32,
        load: impl FnOnce() -> Option<&'s str>,
    ) -> Option<&mut EditorText> {
        let index = match self.texts.iter().position(|t| t.pane_id == pane_id) {
            Some(index) => index,
            None => {
                self.texts.push(EditorText {
                    pane_id,
                    text: load()?.to_owned(),
                    edited: false,
                });
                self.texts.len() - 1
            }
        };
        self.texts.get_mut(index)
    }

    /// The pane's text as shown this frame, without copying it: the text
    /// box's copy when it has one (edits included), else `source`.
    pub fn shown<'s>(&'s self, pane_id: u32, source: Option<&'s str>) -> Option<&'s str> {
        self.texts
            .iter()
            .find(|t| t.pane_id == pane_id)
            .map(|t| t.text.as_str())
            .or(source)
    }

    /// The texts edited this frame, to write back to their panes.
    pub fn into_edited(self) -> impl Iterator<Item = (u32, String)> {
        self.texts
            .into_iter()
            .filter(|t| t.edited)
            .map(|t| (t.pane_id, t.text))
    }
}

/// The text an editor pane shows: a file editor's buffer or its note's text.
/// `None` for any other pane, or a pane that is gone.
pub(in crate::app) fn pane_text<'a>(
    panes: &'a [PaneEntry],
    notes: &'a NoteStore,
    pane_id: u32,
) -> Option<&'a str> {
    match &panes.iter().find(|p| p.id == pane_id)?.content {
        PaneContent::FileEditor(ed) => Some(ed.content.as_str()),
        PaneContent::NoteEditor(ne) => Some(notes.get(ne.workspace_id)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn a_text_is_copied_once_per_frame_when_first_asked_for() {
        let mut texts = EditorTexts::default();
        let loads = Cell::new(0);
        let load = || {
            loads.set(loads.get() + 1);
            Some("hello")
        };
        texts.get_or_load(1, load).unwrap().text.push('!');
        // Asked again (search bar, second draw): the frame's copy, edits kept.
        let again = texts.get_or_load(1, || Some("ignored")).unwrap();
        assert_eq!(again.text, "hello!");
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn panes_never_drawn_are_never_copied() {
        let mut texts = EditorTexts::default();
        assert!(texts.get_or_load(1, || None).is_none());
        assert_eq!(texts.shown(2, None), None);
        assert_eq!(texts.into_edited().count(), 0);
    }

    #[test]
    fn shown_reads_the_source_until_a_text_box_has_a_copy() {
        let mut texts = EditorTexts::default();
        assert_eq!(texts.shown(1, Some("on disk")), Some("on disk"));
        texts.get_or_load(1, || Some("on disk")).unwrap().text = "typed".into();
        assert_eq!(texts.shown(1, Some("on disk")), Some("typed"));
    }

    #[test]
    fn only_edited_texts_are_written_back() {
        let mut texts = EditorTexts::default();
        texts.get_or_load(1, || Some("drawn, not typed in"));
        let typed = texts.get_or_load(2, || Some("a")).unwrap();
        typed.text.push('b');
        typed.edited = true;
        let edited: Vec<(u32, String)> = texts.into_edited().collect();
        assert_eq!(edited, vec![(2, "ab".to_string())]);
    }
}
