//! The notes header, the same in the right panel's notes and in a notes
//! pane: the title, the Markdown preview toggle (👁) beside it, a warning
//! (⚠) only while something is wrong, and on the right the notes file
//! button (⚙) with its menu, before whatever buttons the caller adds.
//!
//! A note lives in Terminal Studio's own file unless the user picks a file
//! of their own (an Obsidian vault page, a text file), which is then edited
//! both here and in other apps.

use std::path::{Path, PathBuf};

use crate::notes::{NoteKey, NoteStatus};
use crate::theme;

/// The notes file button's icon.
const FILE_ICON: &str = "\u{2699}";
/// The Markdown preview toggle's icon.
const PREVIEW_ICON: &str = "👁";
/// Shown while saving or linking has a problem.
const PROBLEM_ICON: &str = "\u{26A0}";

const PREVIEW_TIP: &str = "Preview Markdown";
const EDIT_TIP: &str = "Back to editing";
const FILE_MENU_HEADING: &str = "Notes file";
const DEFAULT_FILE_LABEL: &str = "Default";
const DEFAULT_FILE_TIP: &str = "Keep these notes in Terminal Studio's notes folder. A linked \
                                file is left as it is.";
const PICK_EXISTING_LABEL: &str = "Choose a File\u{2026}";
const PICK_EXISTING_TIP: &str = "Keep these notes in a Markdown or text file of your own, such \
                                 as a page in an Obsidian vault. Its text and these notes are \
                                 merged; nothing is lost.";
const PICK_NEW_LABEL: &str = "Create a New File\u{2026}";
const PICK_NEW_TIP: &str = "Create a file for these notes wherever you like.";
const COPY_PATH_LABEL: &str = "Copy Path";
/// How wide the notes file menu is; a long file name is ellipsised to it.
const FILE_MENU_W: f32 = 240.0;

/// Something asked of a notes header's file menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum NoteLinkAction {
    /// Pick an existing file to link.
    PickExisting(NoteKey),
    /// Pick where to create a file to link.
    PickNew(NoteKey),
    /// Go back to the app's own file.
    Unlink(NoteKey),
    CopyPath(PathBuf),
    Reveal(PathBuf),
}

/// What a notes header shows for its note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct NotesHeaderInfo {
    pub(in crate::app) key: NoteKey,
    /// "Notes · <workspace>".
    pub(in crate::app) title: String,
    /// The user's file the note is linked to.
    pub(in crate::app) link: Option<PathBuf>,
    /// The file the note is kept in: `link`, else the app's own.
    pub(in crate::app) file: Option<PathBuf>,
    pub(in crate::app) status: NoteStatus,
}

impl NotesHeaderInfo {
    /// Whether the note is Markdown, so it can be previewed: the app's own
    /// files are; a linked file is when its name says so (not a `.txt`).
    pub(in crate::app) fn is_markdown(&self) -> bool {
        note_is_markdown(self.link.as_deref())
    }
}

/// Whether a note linked to `link` (`None`: kept in the app's own file) is
/// Markdown. Pure.
pub(in crate::app) fn note_is_markdown(link: Option<&std::path::Path>) -> bool {
    link.is_none_or(crate::util::is_markdown_path)
}

/// What hovering a preview toggle says, with its shortcut: one for each
/// state, so a toggle can say what clicking it does.
pub(in crate::app) struct PreviewTips {
    /// While the text shows.
    pub(in crate::app) preview: String,
    /// While the preview shows.
    pub(in crate::app) edit: String,
}

impl PreviewTips {
    /// The tips, each passed through `with_shortcut` (which adds the chord).
    pub(in crate::app) fn new(with_shortcut: impl Fn(&str) -> String) -> Self {
        Self {
            preview: with_shortcut(PREVIEW_TIP),
            edit: with_shortcut(EDIT_TIP),
        }
    }

    /// What clicking the toggle does now.
    pub(in crate::app) fn for_state(&self, previewing: bool) -> &str {
        if previewing {
            &self.edit
        } else {
            &self.preview
        }
    }
}

/// The header's parts, as drawn this frame.
pub(in crate::app) struct HeaderResponse {
    pub(in crate::app) title: egui::Response,
    /// The preview toggle, when shown.
    pub(in crate::app) preview: Option<egui::Response>,
}

impl HeaderResponse {
    /// Whether the preview toggle was clicked.
    pub(in crate::app) fn preview_toggled(&self) -> bool {
        self.preview.as_ref().is_some_and(egui::Response::clicked)
    }
}

/// A notes header to draw.
pub(in crate::app) struct NotesHeader<'a> {
    pub(in crate::app) info: &'a NotesHeaderInfo,
    /// What the title says ("Notes", "Notes · api").
    pub(in crate::app) title: &'a str,
    pub(in crate::app) title_size: f32,
    /// Whether clicking the title does something (collapses the panel).
    pub(in crate::app) title_clickable: bool,
    /// Whether the preview is showing; `None` hides the toggle (not
    /// Markdown, or nothing shown under the header).
    pub(in crate::app) previewing: Option<bool>,
    pub(in crate::app) preview_tips: &'a PreviewTips,
}

impl NotesHeader<'_> {
    /// Draws the header one row high across the space given. `trailing`
    /// adds the caller's buttons at the far right, rightmost first; the
    /// notes file button goes just left of them. What the file menu asks
    /// is pushed to `actions`.
    pub(in crate::app) fn show(
        self,
        ui: &mut egui::Ui,
        actions: &mut Vec<NoteLinkAction>,
        trailing: impl FnOnce(&mut egui::Ui),
    ) -> HeaderResponse {
        let problem = problem_line(self.info.link.as_deref(), &self.info.status);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), theme::HEADER_H),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = ICON_GAP;
                trailing(ui);
                file_button(ui, self.info, problem.as_ref(), actions);
                let (title, preview) = ui
                    .with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add_space(TITLE_INSET);
                        let toggle_w = if self.previewing.is_some() {
                            TITLE_GAP + TOGGLE_W + ICON_GAP
                        } else {
                            0.0
                        };
                        let natural = title_galley(ui, &self, f32::INFINITY).size().x;
                        let fit =
                            fit_title(ui.available_width() - toggle_w, natural, problem.is_some());
                        let title = title_label(ui, &self, fit.title_w);
                        let preview = self.previewing.map(|on| {
                            ui.add_space(TITLE_GAP);
                            preview_toggle(ui, on, self.preview_tips.for_state(on))
                        });
                        if let (true, Some(problem)) = (fit.show_badge, &problem) {
                            problem_badge(ui, problem);
                        }
                        (title, preview)
                    })
                    .inner;
                HeaderResponse { title, preview }
            },
        )
        .inner
    }
}

/// Gap between the header's icons: they are framed squares, so a wide gap
/// would only take the title's room.
const ICON_GAP: f32 = theme::SP_1;
/// Space before the title, so it lines up with the pane's info bar text.
const TITLE_INSET: f32 = theme::SP_2;
/// Space between the title and the preview toggle, so the toggle's frame
/// never touches the text.
const TITLE_GAP: f32 = theme::SP_2;
/// The preview toggle's width.
const TOGGLE_W: f32 = theme::ICON_LG + 2.0 * theme::ICON_PAD;
/// The ⚠'s width.
const BADGE_W: f32 = theme::ICON_MD + 2.0 * theme::ICON_PAD;
/// The 👁 glyph's size: emoji glyphs are drawn smaller than symbols, so it
/// takes a larger size to match the ⚙ beside it.
const PREVIEW_ICON_SIZE: f32 = theme::FONT_STATUS;

/// How the title and the ⚠ share the room left of the preview toggle.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TitleFit {
    title_w: f32,
    show_badge: bool,
}

/// The title comes first: the ⚠ shows only when the whole title still fits
/// beside it. Without it the ⚙ still takes the problem's colour and says
/// what happened. Pure.
fn fit_title(room: f32, title_natural_w: f32, has_problem: bool) -> TitleFit {
    let room = room.max(0.0);
    let with_badge = room - (BADGE_W + ICON_GAP);
    if has_problem && with_badge >= title_natural_w {
        TitleFit {
            title_w: with_badge,
            show_badge: true,
        }
    } else {
        TitleFit {
            title_w: room,
            show_badge: false,
        }
    }
}

/// The title laid out on one line no wider than `max_width`.
fn title_galley(
    ui: &egui::Ui,
    header: &NotesHeader<'_>,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    crate::ui_kit::one_line_elided(
        ui,
        header.title,
        egui::FontId::proportional(header.title_size),
        theme::active().text,
        max_width,
    )
}

/// The title, ellipsised to `max_width` (whole on hover).
fn title_label(ui: &mut egui::Ui, header: &NotesHeader<'_>, max_width: f32) -> egui::Response {
    let galley = title_galley(ui, header, max_width);
    let elided = galley.elided;
    let sense = if header.title_clickable {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let mut resp = ui.add(egui::Label::new(galley).sense(sense));
    if header.title_clickable {
        resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    if elided {
        resp = crate::ui_kit::full_hover_text(resp, header.title);
    }
    resp
}

/// The 👁 toggle: lit while the preview shows (the text colour on the
/// selection fill, which an accent icon would barely show on).
fn preview_toggle(ui: &mut egui::Ui, on: bool, tip: &str) -> egui::Response {
    let t = theme::active();
    let color = if on { t.text } else { t.fg_muted };
    // Exactly this size: the title's room was worked out from it. Button
    // padding would make it wider; the square itself pads the glyph.
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = egui::Vec2::ZERO;
        ui.add_sized(
            egui::vec2(TOGGLE_W, theme::HEADER_H - 2.0 * ICON_GAP),
            egui::Button::selectable(
                on,
                egui::RichText::new(PREVIEW_ICON)
                    .size(PREVIEW_ICON_SIZE)
                    .color(color),
            ),
        )
    })
    .inner
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .on_hover_text(tip)
}

/// How a problem reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Warning,
    Error,
}

/// What is wrong with a note's file, for the ⚠ and its tooltip.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ProblemLine {
    /// The short form ("todo.md is missing"), the tooltip's first line.
    text: String,
    tone: Tone,
    /// What happened and what to do.
    tip: String,
}

/// A file's name as shown in a header.
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// What is wrong, or `None` while the note saves fine.
fn problem_line(link: Option<&Path>, status: &NoteStatus) -> Option<ProblemLine> {
    match status {
        NoteStatus::Synced => None,
        NoteStatus::LinkFailed(reason) => Some(ProblemLine {
            text: "Link failed".into(),
            tone: Tone::Error,
            tip: format!("Could not use that file: {reason}.\nThese notes stay where they were."),
        }),
        NoteStatus::Failed(reason) => Some(ProblemLine {
            text: "Not saved".into(),
            tone: Tone::Error,
            tip: format!(
                "Could not save to {}: {reason}.\nYour text is kept here and saving is \
                 tried again.",
                link.map(|p| p.display().to_string())
                    .unwrap_or_else(|| "the notes file".into())
            ),
        }),
        NoteStatus::Missing => {
            let path = link?;
            Some(ProblemLine {
                text: format!("{} is missing", file_label(path)),
                tone: Tone::Warning,
                tip: format!(
                    "{} was moved or deleted.\nYour text is kept here: typing writes the \
                     file again, or choose another file ({FILE_ICON}).",
                    path.display()
                ),
            })
        }
    }
}

/// The ⚠, coloured by how bad it is; the tooltip says what happened.
fn problem_badge(ui: &mut egui::Ui, problem: &ProblemLine) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(BADGE_W, theme::HEADER_H), egui::Sense::hover());
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        PROBLEM_ICON,
        egui::FontId::proportional(theme::FONT_UI_MD),
        tone_color(problem.tone),
    );
    crate::ui_kit::full_hover_text(resp, &format!("{}\n{}", problem.text, problem.tip));
}

fn tone_color(tone: Tone) -> egui::Color32 {
    let t = theme::active();
    match tone {
        Tone::Warning => t.warning,
        Tone::Error => t.error,
    }
}

/// The ⚙ tooltip: what is wrong, if anything, then which file the notes
/// are kept in.
fn file_button_tip(info: &NotesHeaderInfo, problem: Option<&ProblemLine>) -> String {
    let file = file_tip(info);
    match problem {
        Some(p) => format!("{}\n{}\n\n{file}", p.text, p.tip),
        None => file,
    }
}

/// Which file the notes are kept in.
fn file_tip(info: &NotesHeaderInfo) -> String {
    match &info.link {
        Some(path) => format!(
            "{FILE_MENU_HEADING}: {}\n{}\nEdits here and in other apps (Obsidian, \
             Notepad++\u{2026}) update each other.",
            file_label(path),
            path.display()
        ),
        None => format!(
            "{FILE_MENU_HEADING}: {DEFAULT_FILE_LABEL}, saved as you type.\nChoose a \
             Markdown or text file to edit these notes in other apps too."
        ),
    }
}

/// The ⚙ button and its menu. It takes the problem's colour while there
/// is one, else the accent while a file of the user's is linked.
fn file_button(
    ui: &mut egui::Ui,
    info: &NotesHeaderInfo,
    problem: Option<&ProblemLine>,
    actions: &mut Vec<NoteLinkAction>,
) {
    let t = theme::active();
    let color = match (problem, &info.link) {
        (Some(p), _) => tone_color(p.tone),
        (None, Some(_)) => t.accent,
        (None, None) => t.fg_muted,
    };
    let resp = ui
        .add(
            egui::Button::new(
                egui::RichText::new(FILE_ICON)
                    .size(theme::FONT_UI_MD)
                    .color(color),
            )
            .min_size(egui::vec2(theme::HEADER_H, theme::HEADER_H))
            .frame(false),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let resp = crate::ui_kit::full_hover_text(resp, &file_button_tip(info, problem));
    egui::Popup::menu(&resp).show(|ui| file_menu(ui, &menu_entries(info), actions));
}

/// Which file a radio choice in the menu stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    /// The app's own file.
    Default,
    /// The user's linked file.
    Linked(PathBuf),
}

/// One row of the notes file menu.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuEntry {
    Heading(&'static str),
    /// A file the notes can be kept in; `action` is what picking it does
    /// (`None` when it is the one in use).
    Choice {
        choice: Choice,
        selected: bool,
        action: Option<NoteLinkAction>,
    },
    Item {
        label: &'static str,
        tip: Option<&'static str>,
        action: NoteLinkAction,
    },
    Separator,
}

/// The notes file menu: which file (default, or the linked one), choose or
/// create one, and copy / reveal the file in use. Pure.
fn menu_entries(info: &NotesHeaderInfo) -> Vec<MenuEntry> {
    let linked = info.link.is_some();
    let mut entries = vec![
        MenuEntry::Heading(FILE_MENU_HEADING),
        MenuEntry::Choice {
            choice: Choice::Default,
            selected: !linked,
            action: linked.then_some(NoteLinkAction::Unlink(info.key)),
        },
    ];
    if let Some(path) = &info.link {
        entries.push(MenuEntry::Choice {
            choice: Choice::Linked(path.clone()),
            selected: true,
            action: None,
        });
    }
    entries.extend([
        MenuEntry::Separator,
        MenuEntry::Item {
            label: PICK_EXISTING_LABEL,
            tip: Some(PICK_EXISTING_TIP),
            action: NoteLinkAction::PickExisting(info.key),
        },
        MenuEntry::Item {
            label: PICK_NEW_LABEL,
            tip: Some(PICK_NEW_TIP),
            action: NoteLinkAction::PickNew(info.key),
        },
    ]);
    if let Some(path) = &info.file {
        entries.extend([
            MenuEntry::Separator,
            MenuEntry::Item {
                label: COPY_PATH_LABEL,
                tip: None,
                action: NoteLinkAction::CopyPath(path.clone()),
            },
            MenuEntry::Item {
                label: crate::util::REVEAL_IN_FILE_MANAGER_LABEL,
                tip: None,
                action: NoteLinkAction::Reveal(path.clone()),
            },
        ]);
    }
    entries
}

/// Draws `entries`; what was picked goes to `actions` and closes the menu.
fn file_menu(ui: &mut egui::Ui, entries: &[MenuEntry], actions: &mut Vec<NoteLinkAction>) {
    ui.set_width(FILE_MENU_W);
    let t = theme::active();
    for entry in entries {
        let (resp, action) = match entry {
            MenuEntry::Heading(text) => {
                ui.label(
                    egui::RichText::new(*text)
                        .size(theme::FONT_UI_XS)
                        .color(t.fg_muted),
                );
                continue;
            }
            MenuEntry::Separator => {
                ui.separator();
                continue;
            }
            MenuEntry::Choice {
                choice,
                selected,
                action,
            } => {
                let resp = match choice {
                    Choice::Default => ui.radio(*selected, DEFAULT_FILE_LABEL),
                    Choice::Linked(path) => {
                        let room =
                            FILE_MENU_W - ui.spacing().icon_width - ui.spacing().icon_spacing;
                        let name = crate::ui_kit::one_line_elided(
                            ui,
                            &file_label(path),
                            egui::FontId::proportional(theme::FONT_UI_MD),
                            t.text,
                            room,
                        );
                        ui.radio(*selected, name)
                    }
                };
                let tip = match choice {
                    Choice::Default => DEFAULT_FILE_TIP.to_string(),
                    Choice::Linked(path) => path.display().to_string(),
                };
                let mut resp = crate::ui_kit::full_hover_text(resp, &tip);
                if action.is_some() {
                    resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                }
                (resp, action.clone())
            }
            MenuEntry::Item { label, tip, action } => {
                let mut resp = ui
                    .button(*label)
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if let Some(tip) = tip {
                    resp = resp.on_hover_text(*tip);
                }
                (resp, Some(action.clone()))
            }
        };
        if let (true, Some(action)) = (resp.clicked(), action) {
            actions.push(action);
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::egui_test_frames::run_ui;

    const OWN: &str = "/cfg/notes/api.md";
    const VAULT: &str = "/vault/todo.md";

    fn info(link: Option<&str>, status: NoteStatus) -> NotesHeaderInfo {
        NotesHeaderInfo {
            key: Some(1),
            title: "Notes \u{00b7} api".into(),
            link: link.map(PathBuf::from),
            file: Some(PathBuf::from(link.unwrap_or(OWN))),
            status,
        }
    }

    #[test]
    fn a_note_that_saves_fine_shows_no_problem() {
        assert_eq!(problem_line(None, &NoteStatus::Synced), None);
        assert_eq!(
            problem_line(Some(Path::new(VAULT)), &NoteStatus::Synced),
            None
        );
    }

    #[test]
    fn problems_say_what_happened() {
        let path = Some(Path::new(VAULT));
        let missing = problem_line(path, &NoteStatus::Missing).unwrap();
        assert_eq!(missing.text, "todo.md is missing");
        assert_eq!(missing.tone, Tone::Warning);

        let failed = problem_line(path, &NoteStatus::Failed("disk full".into())).unwrap();
        assert_eq!(failed.text, "Not saved");
        assert_eq!(failed.tone, Tone::Error);
        assert!(failed.tip.contains("/vault/todo.md: disk full"));

        let own_failed = problem_line(None, &NoteStatus::Failed("denied".into())).unwrap();
        assert!(own_failed.tip.contains("the notes file: denied"));

        let link = problem_line(
            None,
            &NoteStatus::LinkFailed("it is not a text file".into()),
        )
        .unwrap();
        assert_eq!(link.text, "Link failed");
        assert!(link.tip.contains("it is not a text file"));
    }

    #[test]
    fn the_file_button_says_which_file_is_used() {
        let own = file_button_tip(&info(None, NoteStatus::Synced), None);
        assert!(own.starts_with("Notes file: Default"));
        let linked = file_button_tip(&info(Some(VAULT), NoteStatus::Synced), None);
        assert!(linked.starts_with("Notes file: todo.md\n/vault/todo.md\n"));
    }

    #[test]
    fn the_file_button_leads_with_a_problem() {
        let missing = info(Some(VAULT), NoteStatus::Missing);
        let problem = problem_line(missing.link.as_deref(), &missing.status);
        let tip = file_button_tip(&missing, problem.as_ref());
        assert!(tip.starts_with("todo.md is missing\n/vault/todo.md was moved"));
        assert!(tip.ends_with(&file_tip(&missing)));
    }

    #[test]
    fn the_title_keeps_its_room_before_the_warning_shows() {
        let room = 100.0;
        let badge = BADGE_W + ICON_GAP;
        // Fits with the warning beside it.
        let fit = fit_title(room, room - badge, true);
        assert!(fit.show_badge);
        assert_eq!(fit.title_w, room - badge);
        // Would be cut short: the warning gives way (the gear says it).
        let fit = fit_title(room, room - badge + 1.0, true);
        assert!(!fit.show_badge);
        assert_eq!(fit.title_w, room);
        // No problem, no warning; no room never goes negative.
        assert!(!fit_title(room, 10.0, false).show_badge);
        assert_eq!(fit_title(-5.0, 10.0, true).title_w, 0.0);
    }

    #[test]
    fn only_markdown_notes_can_be_previewed() {
        assert!(info(None, NoteStatus::Synced).is_markdown());
        assert!(info(Some(VAULT), NoteStatus::Synced).is_markdown());
        assert!(!info(Some("/vault/todo.txt"), NoteStatus::Synced).is_markdown());
        assert!(note_is_markdown(Some(Path::new("/vault/Todo.MARKDOWN"))));
    }

    #[test]
    fn the_toggle_says_what_a_click_does_with_its_shortcut() {
        let tips = PreviewTips::new(|tip| format!("{tip} (\u{21E7}\u{2318}M)"));
        assert_eq!(
            tips.for_state(false),
            "Preview Markdown (\u{21E7}\u{2318}M)"
        );
        assert_eq!(tips.for_state(true), "Back to editing (\u{21E7}\u{2318}M)");
    }

    /// The actions a menu's rows give, in order.
    fn actions_of(entries: &[MenuEntry]) -> Vec<Option<NoteLinkAction>> {
        entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Choice { action, .. } => Some(action.clone()),
                MenuEntry::Item { action, .. } => Some(Some(action.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_default_file_is_chosen_until_a_file_is_linked() {
        let own = menu_entries(&info(None, NoteStatus::Synced));
        assert_eq!(
            own[1],
            MenuEntry::Choice {
                choice: Choice::Default,
                selected: true,
                action: None
            }
        );
        assert_eq!(
            actions_of(&own),
            vec![
                None,
                Some(NoteLinkAction::PickExisting(Some(1))),
                Some(NoteLinkAction::PickNew(Some(1))),
                Some(NoteLinkAction::CopyPath(OWN.into())),
                Some(NoteLinkAction::Reveal(OWN.into())),
            ]
        );
    }

    #[test]
    fn a_linked_file_is_chosen_and_default_unlinks() {
        let linked = menu_entries(&info(Some(VAULT), NoteStatus::Synced));
        assert_eq!(
            linked[1..3],
            [
                MenuEntry::Choice {
                    choice: Choice::Default,
                    selected: false,
                    action: Some(NoteLinkAction::Unlink(Some(1)))
                },
                MenuEntry::Choice {
                    choice: Choice::Linked(VAULT.into()),
                    selected: true,
                    action: None
                },
            ]
        );
        assert!(actions_of(&linked).contains(&Some(NoteLinkAction::CopyPath(VAULT.into()))));
    }

    #[test]
    fn without_a_notes_folder_there_is_no_path_to_copy() {
        let mut none = info(None, NoteStatus::Synced);
        none.file = None;
        let actions = actions_of(&menu_entries(&none));
        assert!(!actions
            .iter()
            .any(|a| matches!(a, Some(NoteLinkAction::CopyPath(_)))));
    }

    fn click(pos: egui::Pos2) -> egui::RawInput {
        let press = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        egui::RawInput {
            events: vec![egui::Event::PointerMoved(pos), press(true), press(false)],
            ..Default::default()
        }
    }

    /// One frame of `draw`; what it returns.
    fn frame<T>(
        ctx: &egui::Context,
        input: egui::RawInput,
        mut draw: impl FnMut(&mut egui::Ui) -> T,
    ) -> T {
        let mut out = None;
        let _ = run_ui(ctx, input, |ui| out = Some(draw(ui)));
        out.expect("the frame ran")
    }

    #[test]
    fn clicking_a_menu_row_asks_for_its_action() {
        let ctx = egui::Context::default();
        let entries = menu_entries(&info(None, NoteStatus::Synced));
        let mut actions = Vec::new();
        let mut menu = |ui: &mut egui::Ui| {
            ui.vertical(|ui| file_menu(ui, &entries, &mut actions))
                .response
                .rect
        };
        let area = frame(&ctx, egui::RawInput::default(), &mut menu);
        // The last row: Show in Finder / File Explorer.
        frame(
            &ctx,
            click(egui::pos2(area.left() + 10.0, area.bottom() - 4.0)),
            &mut menu,
        );
        assert_eq!(actions, vec![NoteLinkAction::Reveal(OWN.into())]);
    }

    /// One frame of a header with a preview toggle across the screen; the
    /// rects of its title, toggle and file button (the rightmost square, no
    /// buttons being added after it), and whether its title / toggle was
    /// clicked.
    fn header_frame(
        ctx: &egui::Context,
        info: &NotesHeaderInfo,
        input: egui::RawInput,
    ) -> ([egui::Rect; 3], bool, bool) {
        let mut actions = Vec::new();
        let tips = PreviewTips::new(str::to_string);
        let out = frame(ctx, input, |ui| {
            let r = NotesHeader {
                info,
                title: &info.title,
                title_size: theme::FONT_UI_MD,
                title_clickable: true,
                previewing: Some(false),
                preview_tips: &tips,
            }
            .show(ui, &mut actions, |_| {});
            let toggle = r.preview.as_ref().expect("a toggle");
            let right = ui.max_rect().right();
            let gear = egui::Rect::from_min_max(
                egui::pos2(right - theme::HEADER_H, r.title.rect.top()),
                egui::pos2(right, r.title.rect.bottom()),
            );
            (
                [r.title.rect, toggle.rect, gear],
                r.title.clicked(),
                r.preview_toggled(),
            )
        });
        assert!(actions.is_empty(), "the header itself picks nothing");
        out
    }

    #[test]
    fn the_toggle_sits_by_the_title_and_the_gear_opens_the_menu() {
        let ctx = egui::Context::default();
        let own = info(None, NoteStatus::Synced);
        let ([title, toggle, gear], ..) = header_frame(&ctx, &own, egui::RawInput::default());
        assert!(
            title.right() <= toggle.left(),
            "the toggle is beside the title"
        );
        assert!(toggle.right() < gear.left(), "the gear is on the right");

        let (_, title_clicked, toggled) = header_frame(&ctx, &own, click(title.center()));
        assert!(title_clicked && !toggled);
        let (_, title_clicked, toggled) = header_frame(&ctx, &own, click(toggle.center()));
        assert!(toggled && !title_clicked);

        assert!(!egui::Popup::is_any_open(&ctx));
        header_frame(&ctx, &own, click(gear.center()));
        assert!(egui::Popup::is_any_open(&ctx), "a click opens the menu");
    }

    #[test]
    fn a_long_title_never_pushes_the_icons_out() {
        const WIDTH: f32 = 200.0;
        let ctx = egui::Context::default();
        let mut long = info(None, NoteStatus::Failed("denied".into()));
        long.title = "Notes \u{00b7} a workspace with a very long name indeed ".repeat(4);
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH, 100.0),
            )),
            ..Default::default()
        };
        // The second frame lays out at the screen size the first one took.
        header_frame(&ctx, &long, input());
        let ([title, toggle, gear], ..) = header_frame(&ctx, &long, input());
        assert!(title.right() <= toggle.left());
        assert!(toggle.right() < gear.left());
        assert!(gear.right() <= WIDTH, "the gear stays on screen");
    }
}
