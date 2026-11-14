//! Display text, tooltip, directory and colour for a pane.
//!
//! The per-content-kind logic lives in free functions that take the pieces
//! only `App` can look up (live session state, the workspace store) as
//! closures, so it is exercisable without an `App`. The `impl App` methods
//! below are the wiring.

use super::super::pane::{PaneContent, SessionEntry};
use super::super::title::{effective_title, session_title};
use super::super::App;
use std::path::{Path, PathBuf};

/// Display text for a pane.
///
/// `terminal_label` supplies the label for a live terminal, which depends on
/// session state; `ws_name` resolves a directory to its workspace name.
fn content_display_text(
    content: &PaneContent,
    terminal_label: impl FnOnce(u32) -> String,
    ws_name: impl FnOnce(&Path) -> Option<String>,
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
        PaneContent::NoteEditor(_) => "Notes".to_string(),
        PaneContent::ConflictResolver(cr) => cr
            .path
            .file_name()
            .map(|n| format!("\u{26a0} {}", n.to_string_lossy()))
            .unwrap_or_else(|| "Conflicts".to_string()),
    }
}

/// Tooltip for a pane: full title, plus CWD and workspace where they apply.
fn content_tooltip_text(content: &PaneContent, terminal_tip: impl FnOnce(u32) -> String) -> String {
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
        PaneContent::NoteEditor(_) => "Notes".to_string(),
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

/// Workspace colour for a pane, if it belongs to a workspace.
///
/// Terminal panes are matched by their live CWD, deferred ones by their saved
/// CWD, and the rest by the workspace id recorded on the pane.
fn content_ws_color(
    content: &PaneContent,
    terminal_cwd: impl FnOnce(u32) -> Option<PathBuf>,
    for_cwd: impl Fn(&Path) -> Option<[u8; 3]>,
    for_id: impl Fn(u64) -> Option<[u8; 3]>,
) -> Option<[u8; 3]> {
    match content {
        PaneContent::Terminal(sid) => terminal_cwd(*sid).as_deref().and_then(&for_cwd),
        PaneContent::DeferredTerminal { cwd, .. } => cwd.as_deref().and_then(&for_cwd),
        PaneContent::FileEditor(ed) => ed.workspace_id.and_then(&for_id),
        PaneContent::NoteEditor(ne) => ne.workspace_id.and_then(&for_id),
        PaneContent::FileDiff(d) => d.workspace_id.and_then(&for_id),
        PaneContent::ConflictResolver(_) => None,
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
        )
    }

    /// Compute the tooltip text for a tab: full title + CWD + workspace name.
    pub(in crate::app) fn tab_tooltip_text(&self, pane_index: usize) -> String {
        content_tooltip_text(&self.pane_state.panes[pane_index].content, |sid| {
            self.terminal_tooltip_text(sid)
        })
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

    /// Workspace color for a pane (by index), if the pane belongs to a workspace.
    pub(in crate::app) fn ws_color_for_pane(&self, pane_index: usize) -> Option<[u8; 3]> {
        content_ws_color(
            &self.pane_state.panes[pane_index].content,
            |sid| self.session_cwd(sid),
            |cwd| self.workspace_store.find_for_cwd(cwd).map(|w| w.color),
            |id| {
                self.workspace_store
                    .workspaces
                    .iter()
                    .find(|w| w.id == id)
                    .map(|w| w.color)
            },
        )
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
        PaneContent::NoteEditor(NoteEditorState { workspace_id })
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
        content_display_text(content, no_terminal, no_ws)
    }

    // ── Display text ───────────────────────────────────────────────────

    #[test]
    fn terminal_display_defers_to_the_session_lookup() {
        let out = content_display_text(
            &PaneContent::Terminal(7),
            |sid| format!("sess {sid}"),
            no_ws,
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
    fn note_pane_is_named_notes() {
        assert_eq!(display(&note(Some(3))), "Notes");
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
        );
        assert_eq!(out, "build");
    }

    #[test]
    fn deferred_terminal_with_a_blank_saved_title_falls_back_to_the_path() {
        // An empty saved title is the same as none — it must not win and blank
        // the tab.
        let out = content_display_text(&deferred(Some("/work/api"), Some("")), no_terminal, |_| {
            Some("api".to_string())
        });
        assert_ne!(out, "");
    }

    #[test]
    fn deferred_terminal_without_a_cwd_skips_the_workspace_lookup() {
        // `no_ws` panics if called: an empty path must never be handed to the
        // workspace store, which would match on a prefix of everything.
        let out = content_display_text(&deferred(None, None), no_terminal, no_ws);
        assert_eq!(out, effective_title("", Path::new(""), None, None, None));
    }

    // ── Tooltips ───────────────────────────────────────────────────────

    #[test]
    fn terminal_tooltip_defers_to_the_session_lookup() {
        let tip = content_tooltip_text(&PaneContent::Terminal(2), |sid| format!("tip {sid}"));
        assert_eq!(tip, "tip 2");
    }

    #[test]
    fn editor_tooltip_is_the_full_path() {
        let tip = content_tooltip_text(&editor("/a/b/main.rs", true, false), no_terminal);
        assert_eq!(tip, PathBuf::from("/a/b/main.rs").to_string_lossy());
    }

    #[test]
    fn diff_and_conflict_tooltips_are_labelled() {
        assert!(content_tooltip_text(&diff("/r/x.rs"), no_terminal).starts_with("Diff: "));
        assert!(content_tooltip_text(&conflict("/r/x.rs"), no_terminal).starts_with("Conflicts: "));
    }

    #[test]
    fn deferred_tooltip_appends_the_cwd_on_its_own_line() {
        let tip = content_tooltip_text(&deferred(Some("/work/api"), Some("build")), no_terminal);
        assert_eq!(tip.lines().next(), Some("build"));
        assert_eq!(tip.lines().count(), 2);
    }

    #[test]
    fn deferred_tooltip_without_a_cwd_is_a_single_line() {
        let tip = content_tooltip_text(&deferred(None, None), no_terminal);
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

    // ── Workspace colour ───────────────────────────────────────────────

    const RED: [u8; 3] = [255, 0, 0];
    const BLUE: [u8; 3] = [0, 0, 255];

    fn by_cwd(p: &Path) -> Option<[u8; 3]> {
        (p == Path::new("/work")).then_some(RED)
    }

    fn by_id(id: u64) -> Option<[u8; 3]> {
        (id == 42).then_some(BLUE)
    }

    #[test]
    fn terminal_colour_comes_from_its_live_cwd() {
        let c = content_ws_color(
            &PaneContent::Terminal(1),
            |_| Some(PathBuf::from("/work")),
            by_cwd,
            by_id,
        );
        assert_eq!(c, Some(RED));
    }

    #[test]
    fn terminal_without_a_cwd_has_no_colour() {
        let c = content_ws_color(&PaneContent::Terminal(1), |_| None, by_cwd, by_id);
        assert_eq!(c, None);
    }

    #[test]
    fn deferred_terminal_colour_comes_from_its_saved_cwd() {
        let c = content_ws_color(&deferred(Some("/work"), None), no_cwd, by_cwd, by_id);
        assert_eq!(c, Some(RED));
        let c = content_ws_color(&deferred(Some("/elsewhere"), None), no_cwd, by_cwd, by_id);
        assert_eq!(c, None);
    }

    #[test]
    fn file_panes_take_their_colour_from_the_recorded_workspace_id() {
        let mut ed = editor("/a/b/main.rs", false, false);
        if let PaneContent::FileEditor(state) = &mut ed {
            state.workspace_id = Some(42);
        }
        assert_eq!(content_ws_color(&ed, no_cwd, by_cwd, by_id), Some(BLUE));

        let mut d = diff("/a/b/main.rs");
        if let PaneContent::FileDiff(state) = &mut d {
            state.workspace_id = Some(42);
        }
        assert_eq!(content_ws_color(&d, no_cwd, by_cwd, by_id), Some(BLUE));
        assert_eq!(
            content_ws_color(&note(Some(42)), no_cwd, by_cwd, by_id),
            Some(BLUE)
        );
    }

    #[test]
    fn a_pane_in_no_workspace_has_no_colour() {
        assert_eq!(
            content_ws_color(&editor("/a/b/main.rs", false, false), no_cwd, by_cwd, by_id),
            None
        );
        assert_eq!(
            content_ws_color(&conflict("/x"), no_cwd, by_cwd, by_id),
            None
        );
    }

    #[test]
    fn an_unknown_workspace_id_has_no_colour() {
        let mut ed = editor("/a/b/main.rs", false, false);
        if let PaneContent::FileEditor(state) = &mut ed {
            state.workspace_id = Some(999);
        }
        assert_eq!(content_ws_color(&ed, no_cwd, by_cwd, by_id), None);
    }
}
