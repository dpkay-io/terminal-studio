//! Display text, tooltip, directory and colour for a pane.
//!
//! The per-content-kind logic lives in free functions that take the pieces
//! only `App` can look up (live session state, the workspace store) as
//! closures, so it is exercisable without an `App`. The `impl App` methods
//! below are the wiring.

use super::super::pane::{PaneContent, SessionEntry};
use super::super::title::{effective_title, session_title};
use super::super::App;
use crate::notes::NoteKey;
use std::path::{Path, PathBuf};

/// The group of panes outside every workspace, as the left nav names it.
pub(in crate::app) const OTHER_GROUP_NAME: &str = "Other";
const NOTES_TITLE: &str = "Notes";
/// Between a note's title and its workspace.
const NOTES_OWNER_SEPARATOR: &str = " \u{00b7} ";
/// Tooltip line of a note kept in a file the user linked.
const LINKED_NOTE_TIP: &str = "Linked to";
/// Tooltip line of a note kept in the app's own notes folder.
const OWN_NOTE_TIP: &str = "Kept in Terminal Studio's notes folder";

/// A note's name: "Notes · <workspace>" (`owner`), so the notes of
/// different workspaces read apart; just "Notes" when its workspace is gone.
fn note_title(owner: Option<&str>) -> String {
    match owner {
        Some(name) => format!("{NOTES_TITLE}{NOTES_OWNER_SEPARATOR}{name}"),
        None => NOTES_TITLE.to_string(),
    }
}

/// A note's tooltip: its name, then where it is kept.
fn note_tooltip(title: &str, link: Option<&Path>) -> String {
    let mut tip = title.to_string();
    match link {
        Some(path) => append_line(&mut tip, &format!("{LINKED_NOTE_TIP} {}", path.display())),
        None => append_line(&mut tip, OWN_NOTE_TIP),
    }
    tip
}

/// Display text for a pane.
///
/// `terminal_label` supplies the label for a live terminal, which depends on
/// session state; `ws_name` resolves a directory to its workspace name.
fn content_display_text(
    content: &PaneContent,
    terminal_label: impl FnOnce(u32) -> String,
    ws_name: impl FnOnce(&Path) -> Option<String>,
    note_label: impl FnOnce(NoteKey) -> String,
) -> String {
    match content {
        PaneContent::Terminal(sid) => terminal_label(*sid),
        PaneContent::DeferredTerminal {
            cwd, saved_title, ..
        } => {
            if let Some(t) = saved_title.as_deref().filter(|s| !s.is_empty()) {
                return t.to_string();
            }
            let cwd_path = cwd.clone().unwrap_or_default();
            let ws = if cwd_path.as_os_str().is_empty() {
                None
            } else {
                ws_name(&cwd_path)
            };
            effective_title("", &cwd_path, None, None, ws.as_deref())
        }
        PaneContent::FileEditor(ed) => {
            let fname = file_name(&ed.path);
            if ed.save_error {
                format!("! {fname}")
            } else if ed.dirty {
                format!("* {fname}")
            } else {
                fname
            }
        }
        PaneContent::FileDiff(d) => format!("\u{21c4} {}", file_name(&d.path)),
        PaneContent::NoteEditor(ne) => note_label(ne.workspace_id),
        PaneContent::ConflictResolver(cr) => cr
            .path
            .file_name()
            .map(|n| format!("\u{26a0} {}", n.to_string_lossy()))
            .unwrap_or_else(|| "Conflicts".to_string()),
    }
}

/// Tooltip for a pane: full title, plus CWD and workspace where they apply.
fn content_tooltip_text(
    content: &PaneContent,
    terminal_tip: impl FnOnce(u32) -> String,
    note_tip: impl FnOnce(NoteKey) -> String,
) -> String {
    match content {
        PaneContent::Terminal(sid) => terminal_tip(*sid),
        PaneContent::DeferredTerminal {
            cwd, saved_title, ..
        } => {
            let mut tip = saved_title
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("Terminal")
                .to_string();
            if let Some(c) = cwd {
                append_line(&mut tip, &c.to_string_lossy());
            }
            tip
        }
        // The path is where Save As will start; it is no file yet.
        PaneContent::FileEditor(ed) if ed.untitled => {
            format!("{}\n{UNTITLED_TIP}", file_name(&ed.path))
        }
        PaneContent::FileEditor(ed) => ed.path.to_string_lossy().into_owned(),
        PaneContent::FileDiff(d) => format!("Diff: {}", d.path.to_string_lossy()),
        PaneContent::NoteEditor(ne) => note_tip(ne.workspace_id),
        PaneContent::ConflictResolver(cr) => {
            format!("Conflicts: {}", cr.path.to_string_lossy())
        }
    }
}

/// Directory for a pane: CWD for terminals, parent directory for file panes.
fn content_dir(
    content: &PaneContent,
    terminal_cwd: impl FnOnce(u32) -> Option<PathBuf>,
) -> Option<PathBuf> {
    match content {
        PaneContent::Terminal(sid) => terminal_cwd(*sid),
        PaneContent::DeferredTerminal { cwd, .. } => {
            cwd.clone().filter(|p| !p.as_os_str().is_empty())
        }
        PaneContent::FileEditor(ed) => crate::util::parent_dir(&ed.path),
        PaneContent::FileDiff(d) => crate::util::parent_dir(&d.path),
        PaneContent::ConflictResolver(cr) => crate::util::parent_dir(&cr.path),
        PaneContent::NoteEditor(_) => None,
    }
}

/// What a new, never-saved buffer is called before its number.
const UNTITLED_PREFIX: &str = "Untitled-";
/// Second line of an untitled buffer's tooltip.
const UNTITLED_TIP: &str = "Not saved yet";

/// The name for a new untitled buffer: one past the highest number an open
/// untitled buffer uses, so names never repeat while any are open, including
/// ones restored from the last session. Only the name counts, not the folder
/// the buffer was made in.
fn next_untitled_name<'a>(open_paths: impl Iterator<Item = &'a Path>) -> String {
    let highest = open_paths
        .filter_map(|path| {
            path.file_name()?
                .to_str()?
                .strip_prefix(UNTITLED_PREFIX)?
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0);
    format!("{UNTITLED_PREFIX}{}", highest.saturating_add(1))
}

/// A file's name as the user sees it in tabs and dialogs.
pub(in crate::app) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Append `line` as a new line of `tip`, skipping empties so a tooltip never
/// ends in a blank row.
fn append_line(tip: &mut String, line: &str) {
    if line.is_empty() {
        return;
    }
    tip.push('\n');
    tip.push_str(line);
}

impl App {
    /// Compute the display text for a tab at the given pane index.
    pub(in crate::app) fn tab_display_text(&self, pane_index: usize) -> String {
        content_display_text(
            &self.pane_state.panes[pane_index].content,
            |sid| self.terminal_display_text(sid),
            |cwd| self.ws_name_for_cwd(cwd),
            |key| self.note_title(key),
        )
    }

    /// Compute the tooltip text for a tab: full title + CWD + workspace name.
    pub(in crate::app) fn tab_tooltip_text(&self, pane_index: usize) -> String {
        content_tooltip_text(
            &self.pane_state.panes[pane_index].content,
            |sid| self.terminal_tooltip_text(sid),
            |key| note_tooltip(&self.note_title(key), self.note_store.link(key)),
        )
    }

    /// A note's name, the same in its row, tab, split header, pane header,
    /// window title and dialogs: "Notes · <workspace>" ("Notes · Other" for
    /// the notes of panes outside every workspace).
    pub(in crate::app) fn note_title(&self, key: NoteKey) -> String {
        match key {
            None => note_title(Some(OTHER_GROUP_NAME)),
            Some(id) => note_title(self.workspace_store.find(id).map(|w| w.name.as_str())),
        }
    }

    /// Name for a new untitled buffer, after the ones already open.
    pub(in crate::app) fn next_untitled_name(&self) -> String {
        next_untitled_name(
            self.pane_state
                .panes
                .iter()
                .filter_map(|p| match &p.content {
                    PaneContent::FileEditor(ed) if ed.untitled => Some(ed.path.as_path()),
                    _ => None,
                }),
        )
    }

    /// Directory path for a pane: CWD for terminals, parent dir for file-based panes.
    pub(in crate::app) fn pane_cwd(&self, pane_index: usize) -> Option<PathBuf> {
        content_dir(&self.pane_state.panes[pane_index].content, |sid| {
            self.session_cwd(sid)
        })
    }

    /// Workspace colour for a pane (by index): the colour of the workspace
    /// it is grouped under (`App::pane_workspace`), so its row, tab, split
    /// header and the title bar never disagree.
    pub(in crate::app) fn ws_color_for_pane(&self, pane_index: usize) -> Option<[u8; 3]> {
        let pane = self.pane_state.panes.get(pane_index)?;
        Self::pane_workspace(&self.session_state.sessions, &self.workspace_store, pane)
            .map(|w| w.color)
    }

    /// A session's CWD, or `None` when the session is gone or has yet to
    /// report one.
    fn session_cwd(&self, sid: u32) -> Option<PathBuf> {
        self.session_state
            .find(sid)
            .map(|e| e.session.read().cwd.clone())
            .filter(|p| !p.as_os_str().is_empty())
    }

    fn ws_name_for_cwd(&self, cwd: &Path) -> Option<String> {
        self.workspace_store
            .find_for_cwd(cwd)
            .map(|w| w.name.clone())
    }

    /// A terminal's name, the same in its row, its split's header, the
    /// window title and the saved session (see [`session_title`]).
    pub(in crate::app) fn session_label(&self, e: &SessionEntry) -> String {
        let s = e.session.read();
        let (title, pinned, cwd) = (s.title(), s.is_title_pinned(), s.cwd.clone());
        drop(s);
        let fg = self.workers.foreground_worker.get(e.id);
        let ws_name = if cwd.as_os_str().is_empty() {
            None
        } else {
            self.ws_name_for_cwd(&cwd)
        };
        session_title(
            &title,
            pinned,
            &cwd,
            fg.as_ref(),
            Some(&e.shell),
            ws_name.as_deref(),
        )
    }

    fn terminal_display_text(&self, sid: u32) -> String {
        self.session_state
            .find(sid)
            .map(|e| self.session_label(e))
            .unwrap_or_else(|| format!("Terminal {sid}"))
    }

    fn terminal_tooltip_text(&self, sid: u32) -> String {
        let Some(e) = self.session_state.find(sid) else {
            return format!("Terminal {sid}");
        };
        let mut tip = self.session_label(e);
        let cwd = e.session.read().cwd.clone();
        append_line(&mut tip, &cwd.to_string_lossy());
        if !cwd.as_os_str().is_empty() {
            if let Some(name) = self.ws_name_for_cwd(&cwd) {
                append_line(&mut tip, &name);
            }
        }
        tip
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn untitled_names_continue_after_the_highest_open_one() {
        use std::path::Path;
        assert_eq!(super::next_untitled_name(std::iter::empty()), "Untitled-1");
        let open = [
            Path::new("Untitled-1"),
            Path::new("Untitled-4"),
            Path::new("notes.md"),
        ];
        assert_eq!(super::next_untitled_name(open.into_iter()), "Untitled-5");
        // A buffer whose name only looks similar does not count.
        let odd = [Path::new("Untitled-x"), Path::new("Untitled-")];
        assert_eq!(super::next_untitled_name(odd.into_iter()), "Untitled-1");
        // Buffers remember the folder they were made in; only the name counts.
        let in_dirs = [Path::new("/a/Untitled-2"), Path::new("/b/Untitled-7")];
        assert_eq!(super::next_untitled_name(in_dirs.into_iter()), "Untitled-8");
    }

    use super::*;
    use crate::app::conflict_parser::ConflictFile;
    use crate::app::pane::{
        ConflictResolverState, FileDiffState, FileEditorState, NoteEditorState,
    };

    fn conflict(path: &str) -> PaneContent {
        let path = PathBuf::from(path);
        PaneContent::ConflictResolver(ConflictResolverState {
            path: path.clone(),
            content: ConflictFile {
                path,
                blocks: Vec::new(),
                total_conflicts: 0,
            },
            resolved_count: 0,
            side_by_side: false,
        })
    }

    fn note(workspace_id: Option<u64>) -> PaneContent {
        PaneContent::NoteEditor(NoteEditorState {
            workspace_id,
            show_preview: false,
        })
    }

    /// Panics if called — asserts an arm never reaches the terminal lookup.
    fn no_terminal(_: u32) -> String {
        panic!("terminal lookup taken for a non-terminal pane");
    }

    fn no_cwd(_: u32) -> Option<PathBuf> {
        panic!("session lookup taken for a non-terminal pane");
    }

    fn no_ws(_: &Path) -> Option<String> {
        panic!("workspace lookup taken for a pane with no directory");
    }

    fn editor(path: &str, dirty: bool, save_error: bool) -> PaneContent {
        PaneContent::FileEditor(FileEditorState {
            path: PathBuf::from(path),
            content: String::new(),
            dirty,
            save_error,
            workspace_id: None,
            show_preview: false,
            stale: false,
            loading: false,
            untitled: false,
            saved_text_hash: None,
        })
    }

    fn diff(path: &str) -> PaneContent {
        PaneContent::FileDiff(FileDiffState {
            path: PathBuf::from(path),
            old_content: String::new(),
            new_content: String::new(),
            hunks: Vec::new(),
            diff_mode: crate::app::diff_parser::DiffViewMode::Inline,
            current_hunk: 0,
            old_highlights: None,
            new_highlights: None,
            highlight_theme: crate::theme::active().id,
            loading: false,
            workspace_id: None,
            scroll_to_first_hunk: false,
        })
    }

    fn deferred(cwd: Option<&str>, saved_title: Option<&str>) -> PaneContent {
        PaneContent::DeferredTerminal {
            cwd: cwd.map(PathBuf::from),
            pending_command: None,
            saved_title: saved_title.map(str::to_string),
            title_pinned: false,
            scrollback_file: None,
        }
    }

    fn display(content: &PaneContent) -> String {
        content_display_text(content, no_terminal, no_ws, no_note)
    }

    fn no_note(_: NoteKey) -> String {
        panic!("note lookup taken for a pane that is not a note");
    }

    // ── Display text ───────────────────────────────────────────────────

    #[test]
    fn terminal_display_defers_to_the_session_lookup() {
        let out = content_display_text(
            &PaneContent::Terminal(7),
            |sid| format!("sess {sid}"),
            no_ws,
            no_note,
        );
        assert_eq!(out, "sess 7");
    }

    #[test]
    fn clean_editor_shows_the_bare_file_name() {
        assert_eq!(display(&editor("/a/b/main.rs", false, false)), "main.rs");
    }

    #[test]
    fn dirty_editor_is_starred() {
        assert_eq!(display(&editor("/a/b/main.rs", true, false)), "* main.rs");
    }

    #[test]
    fn save_error_outranks_dirty() {
        // A failed save is the more urgent of the two states, so `!` wins even
        // though the buffer is also dirty.
        assert_eq!(display(&editor("/a/b/main.rs", true, true)), "! main.rs");
    }

    #[test]
    fn editor_without_a_file_name_degrades_to_empty() {
        assert_eq!(display(&editor("/", false, false)), "");
    }

    #[test]
    fn diff_pane_is_prefixed_with_the_swap_arrow() {
        assert_eq!(display(&diff("/repo/src/app.rs")), "\u{21c4} app.rs");
    }

    #[test]
    fn note_pane_is_named_after_its_workspace() {
        let out = content_display_text(&note(Some(3)), no_terminal, no_ws, |key| {
            assert_eq!(key, Some(3));
            note_title(Some("api"))
        });
        assert_eq!(out, "Notes \u{00b7} api");
        assert_eq!(note_title(Some(OTHER_GROUP_NAME)), "Notes \u{00b7} Other");
        // Its workspace was deleted: no name to add.
        assert_eq!(note_title(None), "Notes");
    }

    #[test]
    fn note_tooltip_says_where_the_note_is_kept() {
        let tip = content_tooltip_text(&note(None), no_terminal, |key| {
            assert_eq!(key, None);
            note_tooltip("Notes \u{00b7} Other", Some(Path::new("/vault/todo.md")))
        });
        assert_eq!(tip, "Notes \u{00b7} Other\nLinked to /vault/todo.md");
        assert_eq!(
            note_tooltip("Notes", None),
            format!("Notes\n{OWN_NOTE_TIP}")
        );
    }

    #[test]
    fn conflict_pane_is_prefixed_with_a_warning() {
        assert_eq!(display(&conflict("/repo/merge.txt")), "\u{26a0} merge.txt");
    }

    #[test]
    fn deferred_terminal_prefers_its_saved_title() {
        let out = content_display_text(
            &deferred(Some("/work/api"), Some("build")),
            no_terminal,
            |_| Some("api".to_string()),
            no_note,
        );
        assert_eq!(out, "build");
    }

    #[test]
    fn deferred_terminal_with_a_blank_saved_title_falls_back_to_the_path() {
        // An empty saved title is the same as none — it must not win and blank
        // the tab.
        let out = content_display_text(
            &deferred(Some("/work/api"), Some("")),
            no_terminal,
            |_| Some("api".to_string()),
            no_note,
        );
        assert_ne!(out, "");
    }

    #[test]
    fn deferred_terminal_without_a_cwd_skips_the_workspace_lookup() {
        // `no_ws` panics if called: an empty path must never be handed to the
        // workspace store, which would match on a prefix of everything.
        let out = content_display_text(&deferred(None, None), no_terminal, no_ws, no_note);
        assert_eq!(out, effective_title("", Path::new(""), None, None, None));
    }

    // ── Tooltips ───────────────────────────────────────────────────────

    #[test]
    fn terminal_tooltip_defers_to_the_session_lookup() {
        let tip = content_tooltip_text(
            &PaneContent::Terminal(2),
            |sid| format!("tip {sid}"),
            no_note,
        );
        assert_eq!(tip, "tip 2");
    }

    #[test]
    fn editor_tooltip_is_the_full_path() {
        let tip = content_tooltip_text(&editor("/a/b/main.rs", true, false), no_terminal, no_note);
        assert_eq!(tip, PathBuf::from("/a/b/main.rs").to_string_lossy());
    }

    #[test]
    fn diff_and_conflict_tooltips_are_labelled() {
        assert!(content_tooltip_text(&diff("/r/x.rs"), no_terminal, no_note).starts_with("Diff: "));
        assert!(
            content_tooltip_text(&conflict("/r/x.rs"), no_terminal, no_note)
                .starts_with("Conflicts: ")
        );
    }

    #[test]
    fn deferred_tooltip_appends_the_cwd_on_its_own_line() {
        let tip = content_tooltip_text(
            &deferred(Some("/work/api"), Some("build")),
            no_terminal,
            no_note,
        );
        assert_eq!(tip.lines().next(), Some("build"));
        assert_eq!(tip.lines().count(), 2);
    }

    #[test]
    fn deferred_tooltip_without_a_cwd_is_a_single_line() {
        let tip = content_tooltip_text(&deferred(None, None), no_terminal, no_note);
        assert_eq!(tip, "Terminal");
        assert_eq!(tip.lines().count(), 1);
    }

    #[test]
    fn append_line_ignores_empty_lines() {
        // Guards the tooltip against a trailing blank row when a pane has a
        // title but no directory.
        let mut tip = "Terminal".to_string();
        append_line(&mut tip, "");
        assert_eq!(tip, "Terminal");
        append_line(&mut tip, "/tmp");
        assert_eq!(tip, "Terminal\n/tmp");
    }

    // ── Directory ──────────────────────────────────────────────────────

    #[test]
    fn file_pane_dirs_are_the_parent_directory() {
        assert_eq!(
            content_dir(&editor("/a/b/main.rs", false, false), no_cwd),
            Some(PathBuf::from("/a/b"))
        );
        assert_eq!(
            content_dir(&diff("/a/b/main.rs"), no_cwd),
            Some(PathBuf::from("/a/b"))
        );
    }

    #[test]
    fn note_pane_has_no_directory() {
        assert_eq!(content_dir(&note(Some(1)), no_cwd), None);
    }

    #[test]
    fn deferred_terminal_with_an_empty_cwd_has_no_directory() {
        // A `Some("")` cwd is what a restored pane carries before its shell
        // reports one; treating it as a real path would resolve to the process
        // working directory.
        assert_eq!(content_dir(&deferred(Some(""), None), no_cwd), None);
        assert_eq!(
            content_dir(&deferred(Some("/work"), None), no_cwd),
            Some(PathBuf::from("/work"))
        );
    }

    #[test]
    fn terminal_dir_defers_to_the_session_lookup() {
        let dir = content_dir(&PaneContent::Terminal(1), |_| Some(PathBuf::from("/live")));
        assert_eq!(dir, Some(PathBuf::from("/live")));
    }

    // ── Workspace (and so its colour) ──────────────────────────────────
    //
    // Every colour place (session list row, tab, split header, title bar)
    // reads `App::pane_workspace`, which is `pane_group_by` with the live
    // terminal folder. These pin that one rule.

    use crate::app::pane::PaneEntry;
    use crate::workspace::{Workspace, WorkspaceStore};

    const PROJECT: u64 = 1;
    const NESTED: u64 = 2;
    const OPENED_IN: u64 = 3;
    const DELETED: u64 = 99;

    fn store() -> WorkspaceStore {
        let ws = |id: u64, path: &str| Workspace {
            id,
            name: format!("ws-{id}"),
            path: PathBuf::from(path),
            color: [id as u8, 0, 0],
            host_window_id: None,
            last_activated: 0,
        };
        WorkspaceStore {
            workspaces: vec![
                ws(PROJECT, "/ws/project"),
                ws(NESTED, "/ws/project/packages/app"),
                ws(OPENED_IN, "/ws/other"),
            ],
        }
    }

    fn pane(content: PaneContent, opened_in: Option<u64>) -> PaneEntry {
        PaneEntry {
            id: 1,
            content,
            manual_width: None,
            last_size: (0, 0),
            labels: vec![],
            last_active_at: 0,
            workspace_id: opened_in,
        }
    }

    fn group(content: PaneContent, opened_in: Option<u64>, live_cwd: Option<&str>) -> Option<u64> {
        App::pane_group_by(&store(), &pane(content, opened_in), |_| {
            live_cwd.map(PathBuf::from)
        })
    }

    fn with_ws(mut content: PaneContent, id: u64) -> PaneContent {
        match &mut content {
            PaneContent::FileEditor(ed) => ed.workspace_id = Some(id),
            PaneContent::FileDiff(d) => d.workspace_id = Some(id),
            _ => unreachable!(),
        }
        content
    }

    #[test]
    fn a_terminal_in_a_subfolder_takes_its_workspace() {
        let term = || PaneContent::Terminal(1);
        assert_eq!(
            group(term(), None, Some("/ws/project/src/deep")),
            Some(PROJECT)
        );
        // The most specific workspace wins.
        assert_eq!(
            group(term(), None, Some("/ws/project/packages/app/src")),
            Some(NESTED)
        );
    }

    /// The missing-colour bug: a terminal outside every workspace folder (a
    /// Claude conversation resumed where it began, a `cd` away) was grouped
    /// under the workspace it was opened in but drawn with no colour.
    #[test]
    fn a_terminal_outside_every_workspace_keeps_the_one_it_was_opened_in() {
        let term = PaneContent::Terminal(1);
        assert_eq!(
            group(term, Some(OPENED_IN), Some("/tmp/elsewhere")),
            Some(OPENED_IN)
        );
    }

    #[test]
    fn a_terminal_that_has_not_reported_a_folder_keeps_the_one_it_was_opened_in() {
        assert_eq!(
            group(PaneContent::Terminal(1), Some(OPENED_IN), None),
            Some(OPENED_IN)
        );
        assert_eq!(
            group(PaneContent::Terminal(1), Some(OPENED_IN), Some("")),
            Some(OPENED_IN)
        );
    }

    #[test]
    fn a_folder_wins_over_the_workspace_it_was_opened_in() {
        let term = PaneContent::Terminal(1);
        assert_eq!(
            group(term, Some(OPENED_IN), Some("/ws/project")),
            Some(PROJECT)
        );
    }

    #[test]
    fn a_restored_terminal_not_started_yet_uses_its_saved_folder() {
        assert_eq!(
            group(deferred(Some("/ws/project/a"), None), Some(OPENED_IN), None),
            Some(PROJECT)
        );
        assert_eq!(
            group(deferred(Some(""), None), Some(OPENED_IN), None),
            Some(OPENED_IN)
        );
        assert_eq!(
            group(deferred(None, None), Some(OPENED_IN), None),
            Some(OPENED_IN)
        );
    }

    #[test]
    fn files_go_by_their_folder_then_the_recorded_workspace() {
        let in_project = with_ws(editor("/ws/project/src/main.rs", false, false), OPENED_IN);
        assert_eq!(group(in_project, None, None), Some(PROJECT));
        let outside = with_ws(editor("/tmp/notes.txt", false, false), OPENED_IN);
        assert_eq!(group(outside, None, None), Some(OPENED_IN));
        let diff_in = with_ws(diff("/ws/project/packages/app/x.rs"), OPENED_IN);
        assert_eq!(group(diff_in, None, None), Some(NESTED));
        let diff_out = with_ws(diff("/tmp/x.rs"), OPENED_IN);
        assert_eq!(group(diff_out, None, None), Some(OPENED_IN));
        assert_eq!(
            group(conflict("/ws/project/a.rs"), None, None),
            Some(PROJECT)
        );
        assert_eq!(
            group(conflict("/tmp/a.rs"), Some(OPENED_IN), None),
            Some(OPENED_IN)
        );
    }

    #[test]
    fn notes_use_their_workspace() {
        assert_eq!(
            group(note(Some(PROJECT)), Some(OPENED_IN), None),
            Some(PROJECT)
        );
    }

    #[test]
    fn a_deleted_workspace_is_never_a_group() {
        let term = PaneContent::Terminal(1);
        assert_eq!(group(term, Some(DELETED), Some("/tmp")), None);
        assert_eq!(
            group(note(Some(DELETED)), Some(OPENED_IN), None),
            Some(OPENED_IN)
        );
        let ed = with_ws(editor("/tmp/a.txt", false, false), DELETED);
        assert_eq!(group(ed, None, None), None);
    }

    #[test]
    fn non_terminals_never_ask_for_a_terminal_folder() {
        let panics = |_: u32| -> Option<PathBuf> { panic!("terminal lookup for a non-terminal") };
        let s = store();
        for content in [
            editor("/ws/project/a.rs", false, false),
            diff("/ws/project/a.rs"),
            note(Some(PROJECT)),
            conflict("/ws/project/a.rs"),
            deferred(Some("/ws/project"), None),
        ] {
            App::pane_group_by(&s, &pane(content, None), panics);
        }
    }
}
