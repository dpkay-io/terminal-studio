//! Files and folders dragged in from the OS (Finder, Explorer, a file
//! manager).
//!
//! Where they are dropped decides what happens, as in an editor with a
//! terminal panel: on a terminal their paths are pasted there, shell-quoted
//! (what dropping on a terminal has always done); anywhere else they open —
//! folders through the Open Folder dialog (each in its own terminal, saved as
//! a workspace unless unticked), files in the editor, in the split they were
//! dropped on. While they hover, the place they would land is lit up and
//! says which it is.
//!
//! The OS gives no pointer position during the drag, so it is read from the
//! OS (`os_cursor`). Where it can't be (a platform without one), a drop does
//! what it did before: paste into the focused terminal, or open when the
//! focused tab isn't one.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::feedback::{FlashKind, FlashTarget};
use super::pane::PaneContent;
use super::pending_open::OpenRequest;
use super::App;
use crate::editor_group::GroupId;
use crate::theme;

/// Files larger than this are not opened in the editor (it holds the whole
/// file in memory and lays out every line).
pub(super) const MAX_OPEN_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// How much of a file is read to tell text from binary.
const TEXT_SNIFF_BYTES: usize = 8 * 1024;
/// How often the hover highlight follows the pointer: no mouse moves arrive
/// while the OS drags, so the frame has to look.
const HOVER_REFRESH: Duration = Duration::from_millis(16);
const SORT_THREAD: &str = "drop-sort";

const OPEN_TITLE: &str = "Drop to open";
const OPEN_DETAIL: &str = "Folders open in new terminals \u{00b7} files open in the editor";
const PASTE_DETAIL: &str = "Drop outside a terminal to open instead";

fn paste_title(count: usize) -> String {
    match count {
        1 => "Drop to paste the path".to_string(),
        n => format!("Drop to paste {n} paths"),
    }
}

/// What a drop at some point would do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum DropZone {
    /// Over a terminal: paste the paths there.
    Paste {
        pane_id: u32,
        session_id: u32,
        rect: egui::Rect,
    },
    /// Anywhere else: open them; files go to `group` (None = the focused
    /// split). `rect` is what lights up.
    Open {
        group: Option<GroupId>,
        rect: egui::Rect,
    },
}

impl DropZone {
    fn rect(&self) -> egui::Rect {
        match self {
            DropZone::Paste { rect, .. } | DropZone::Open { rect, .. } => *rect,
        }
    }
}

/// A pane drawn this frame, as zones see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DropPane {
    pub(super) pane_id: u32,
    pub(super) rect: egui::Rect,
    /// Set for a running terminal.
    pub(super) session_id: Option<u32>,
    pub(super) group: Option<GroupId>,
}

/// The zone at `pointer` (window points) among the panes drawn this frame;
/// `focused` is the focused tab's pane, used when the pointer is unknown.
pub(super) fn zone_at(
    pointer: Option<egui::Pos2>,
    panes: &[DropPane],
    window: egui::Rect,
    focused: Option<&DropPane>,
) -> DropZone {
    let pane = match pointer {
        Some(pos) => panes.iter().find(|p| p.rect.contains(pos)),
        None => focused,
    };
    match pane {
        Some(&DropPane {
            pane_id,
            rect,
            session_id: Some(session_id),
            ..
        }) => DropZone::Paste {
            pane_id,
            session_id,
            rect,
        },
        // A pointer over some other pane drops into its split; an unknown
        // pointer has no split to point at.
        Some(p) if pointer.is_some() => DropZone::Open {
            group: p.group,
            rect: p.rect,
        },
        _ => DropZone::Open {
            group: None,
            rect: window,
        },
    }
}

/// Dropped paths sorted by what opening them means.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SortedDrop {
    pub(super) folders: Vec<PathBuf>,
    pub(super) files: Vec<PathBuf>,
    /// Missing, unreadable, too big or not text.
    pub(super) skipped: Vec<PathBuf>,
}

/// Sorts `paths` into folders and editable files. Reads the start of each
/// file, so it runs off the UI thread.
pub(super) fn sort_dropped(paths: Vec<PathBuf>) -> SortedDrop {
    let mut sorted = SortedDrop::default();
    for path in paths {
        match std::fs::metadata(&path) {
            Ok(meta) if meta.is_dir() => sorted.folders.push(path),
            Ok(meta) if meta.is_file() && opens_as_text(&path, meta.len()) => {
                sorted.files.push(path)
            }
            _ => sorted.skipped.push(path),
        }
    }
    sorted
}

/// Small enough for the editor, and no NUL byte in its first few KB (the
/// usual sign of a binary file).
fn opens_as_text(path: &Path, len: u64) -> bool {
    if len > MAX_OPEN_FILE_BYTES {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::with_capacity(TEXT_SNIFF_BYTES);
    if file
        .take(TEXT_SNIFF_BYTES as u64)
        .read_to_end(&mut head)
        .is_err()
    {
        return false;
    }
    !head.contains(&0)
}

/// The pointer in this window's points: from the OS while it owns the
/// pointer for a drag, else what egui last saw.
fn pointer_in_window(ctx: &egui::Context) -> Option<egui::Pos2> {
    let (inner, native_ppp, egui_pointer) = ctx.input(|i| {
        (
            i.viewport().inner_rect,
            i.viewport().native_pixels_per_point,
            i.pointer.hover_pos(),
        )
    });
    let from_os = crate::os_cursor::screen_position().and_then(|screen| {
        crate::os_cursor::to_window_points(
            screen,
            inner?.min,
            native_ppp.unwrap_or(1.0),
            ctx.pixels_per_point(),
        )
    });
    from_os.or(egui_pointer)
}

impl App {
    /// Lights up where files hovering over this window would land, and acts
    /// on files dropped on it. Runs once per window frame, after the panes
    /// are drawn (it reads their rects).
    pub(super) fn handle_os_drop(&mut self, ctx: &egui::Context) {
        let (hovered, dropped) = ctx.input(|i| {
            let dropped: Vec<PathBuf> = i
                .raw
                .dropped_files
                .iter()
                .map(|f| f.path())
                // Only the web gives a drop no path; a native one always has it.
                .filter(|p| !p.as_os_str().is_empty())
                .map(std::path::Path::to_path_buf)
                .collect();
            (i.raw.hovered_files.len(), dropped)
        });
        if hovered == 0 && dropped.is_empty() {
            return;
        }
        let zone = self.os_drop_zone(ctx);
        if hovered > 0 {
            paint_drop_zone(ctx, &zone, hovered);
            ctx.request_repaint_after(HOVER_REFRESH);
        }
        if dropped.is_empty() {
            return;
        }
        match zone {
            DropZone::Paste {
                pane_id,
                session_id,
                ..
            } => self.paste_paths(pane_id, session_id, &dropped),
            DropZone::Open { group, .. } => self.open_dropped(dropped, group, ctx),
        }
    }

    fn os_drop_zone(&self, ctx: &egui::Context) -> DropZone {
        let panes: Vec<DropPane> = self
            .drop_pane_rects
            .iter()
            .map(|&(pane_id, rect)| DropPane {
                pane_id,
                rect,
                session_id: self.pane_state.find(pane_id).and_then(|p| match p.content {
                    PaneContent::Terminal(sid) => Some(sid),
                    _ => None,
                }),
                group: self.pane_state.group_of(pane_id),
            })
            .collect();
        let focused = self
            .pane_state
            .active_pane_id
            .and_then(|id| panes.iter().find(|p| p.pane_id == id));
        zone_at(pointer_in_window(ctx), &panes, ctx.content_rect(), focused)
    }

    /// Pastes `paths`, shell-quoted and space-separated, into the terminal
    /// they were dropped on, and focuses it.
    fn paste_paths(&mut self, pane_id: u32, session_id: u32, paths: &[PathBuf]) {
        let text = paths
            .iter()
            .map(|p| super::input::shell_quote_path(p))
            .collect::<Vec<_>>()
            .join(" ");
        let Some(entry) = self.session_state.find(session_id) else {
            return;
        };
        let bracketed = {
            let mut session = entry.session.write();
            session
                .term
                .scroll_display(alacritty_terminal::grid::Scroll::Bottom);
            session
                .term
                .mode()
                .contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE)
        };
        let _ = entry
            .pty_tx
            .try_send(super::input::paste_bytes(&text, bracketed));
        if self.pane_state.active_pane_id != Some(pane_id) {
            self.activate_pane(pane_id);
        }
        self.flash
            .trigger(FlashTarget::Pane(pane_id), FlashKind::Neutral);
    }

    /// Sorts `paths` on a background thread and queues them to open in this
    /// window (`process_open_requests`).
    fn open_dropped(&self, paths: Vec<PathBuf>, group: Option<GroupId>, ctx: &egui::Context) {
        let ctx = ctx.clone();
        let queue = self.open_queue.clone();
        let window = self.current_window_id.clone();
        let spawned = std::thread::Builder::new()
            .name(SORT_THREAD.into())
            .spawn(move || {
                let sorted = sort_dropped(paths);
                let request = OpenRequest {
                    window,
                    folders: sorted.folders,
                    files: sorted.files,
                    target_group: group,
                    skipped: sorted.skipped,
                };
                queue.push(request, &ctx);
            });
        if let Err(e) = spawned {
            log::error!("could not sort dropped files: {e}");
        }
    }
}

/// Dims and outlines the zone files would land in, with what a drop there
/// does.
fn paint_drop_zone(ctx: &egui::Context, zone: &DropZone, count: usize) {
    let t = theme::active();
    let rect = zone.rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("os_drop_zone"),
    ));
    painter.rect_filled(
        rect,
        theme::R_MD,
        crate::theme::with_alpha(t.surface0, theme::ALPHA_SURFACE_OVERLAY),
    );
    painter.rect_stroke(
        rect.shrink(theme::SP_1),
        theme::R_MD,
        egui::Stroke::new(theme::STROKE_BOLD, t.accent),
        egui::StrokeKind::Middle,
    );
    let (title, detail) = match zone {
        DropZone::Paste { .. } => (paste_title(count), PASTE_DETAIL),
        DropZone::Open { .. } => (OPEN_TITLE.to_string(), OPEN_DETAIL),
    };
    let max_width = (rect.width() - 2.0 * theme::SP_6).max(0.0);
    let title_galley = painter.layout(
        title,
        egui::FontId::proportional(theme::FONT_STATUS),
        t.text,
        max_width,
    );
    let detail_galley = painter.layout(
        detail.to_string(),
        egui::FontId::proportional(theme::FONT_UI_MD),
        t.subtext1,
        max_width,
    );
    let gap = theme::SP_2;
    let total_h = title_galley.size().y + gap + detail_galley.size().y;
    let top = rect.center().y - total_h / 2.0;
    let title_pos = egui::pos2(rect.center().x - title_galley.size().x / 2.0, top);
    let detail_pos = egui::pos2(
        rect.center().x - detail_galley.size().x / 2.0,
        top + title_galley.size().y + gap,
    );
    painter.galley(title_pos, title_galley, t.text);
    painter.galley(detail_pos, detail_galley, t.subtext1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, w: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(w, 100.0))
    }

    const WINDOW: egui::Rect = egui::Rect {
        min: egui::pos2(0.0, 0.0),
        max: egui::pos2(400.0, 100.0),
    };

    /// A terminal (pane 1, session 10, split 1) beside an editor (pane 2,
    /// split 2), right of a 100-point sidebar.
    fn panes() -> Vec<DropPane> {
        vec![
            DropPane {
                pane_id: 1,
                rect: rect(100.0, 150.0),
                session_id: Some(10),
                group: Some(1),
            },
            DropPane {
                pane_id: 2,
                rect: rect(250.0, 150.0),
                session_id: None,
                group: Some(2),
            },
        ]
    }

    #[test]
    fn over_a_terminal_pastes_into_it() {
        let zone = zone_at(Some(egui::pos2(120.0, 50.0)), &panes(), WINDOW, None);
        assert_eq!(
            zone,
            DropZone::Paste {
                pane_id: 1,
                session_id: 10,
                rect: rect(100.0, 150.0)
            }
        );
    }

    #[test]
    fn over_another_pane_opens_in_its_split() {
        let zone = zone_at(Some(egui::pos2(300.0, 50.0)), &panes(), WINDOW, None);
        assert_eq!(
            zone,
            DropZone::Open {
                group: Some(2),
                rect: rect(250.0, 150.0)
            }
        );
    }

    #[test]
    fn outside_every_pane_opens_in_the_focused_split() {
        let zone = zone_at(Some(egui::pos2(20.0, 50.0)), &panes(), WINDOW, None);
        assert_eq!(
            zone,
            DropZone::Open {
                group: None,
                rect: WINDOW
            }
        );
    }

    #[test]
    fn an_unknown_pointer_pastes_into_a_focused_terminal_or_opens() {
        let all = panes();
        let zone = zone_at(None, &all, WINDOW, Some(&all[0]));
        assert!(matches!(zone, DropZone::Paste { session_id: 10, .. }));
        let zone = zone_at(None, &all, WINDOW, Some(&all[1]));
        assert_eq!(
            zone,
            DropZone::Open {
                group: None,
                rect: WINDOW
            }
        );
        let zone = zone_at(None, &all, WINDOW, None);
        assert!(matches!(zone, DropZone::Open { group: None, .. }));
    }

    #[test]
    fn dropped_paths_are_sorted_into_folders_files_and_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("project");
        std::fs::create_dir(&folder).unwrap();
        let text = dir.path().join("notes.md");
        std::fs::write(&text, "# hi\n").unwrap();
        let empty = dir.path().join("empty.txt");
        std::fs::write(&empty, "").unwrap();
        let binary = dir.path().join("image.png");
        std::fs::write(&binary, [0x89, b'P', b'N', b'G', 0, 0, 1]).unwrap();
        let missing = dir.path().join("gone.txt");

        let sorted = sort_dropped(vec![
            folder.clone(),
            text.clone(),
            binary.clone(),
            missing.clone(),
            empty.clone(),
        ]);
        assert_eq!(sorted.folders, vec![folder]);
        assert_eq!(sorted.files, vec![text, empty]);
        assert_eq!(sorted.skipped, vec![binary, missing]);
    }

    #[test]
    fn files_over_the_limit_are_skipped_without_reading_them() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.log");
        let file = std::fs::File::create(&big).unwrap();
        // Sparse: the size is set without writing the bytes.
        file.set_len(MAX_OPEN_FILE_BYTES + 1).unwrap();
        let sorted = sort_dropped(vec![big.clone()]);
        assert_eq!(sorted.skipped, vec![big]);
    }

    #[test]
    fn every_zone_paints_even_when_tiny() {
        let ctx = egui::Context::default();
        let zones = [
            DropZone::Paste {
                pane_id: 1,
                session_id: 1,
                rect: rect(0.0, 300.0),
            },
            DropZone::Open {
                group: None,
                rect: WINDOW,
            },
            DropZone::Open {
                group: Some(1),
                rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::ZERO),
            },
        ];
        for zone in zones {
            let output =
                crate::egui_test_frames::run_pass(&ctx, egui::RawInput::default(), |ctx| {
                    paint_drop_zone(ctx, &zone, 2);
                });
            assert!(!output.shapes.is_empty(), "{zone:?} drew nothing");
        }
    }

    #[test]
    fn paste_title_counts_the_paths() {
        assert_eq!(paste_title(1), "Drop to paste the path");
        assert_eq!(paste_title(3), "Drop to paste 3 paths");
    }
}
