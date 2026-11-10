use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) struct FileEntry {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) is_dir: bool,
}

/// Global counter for in-flight subdir load threads to prevent unbounded spawning.
static SUBDIR_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

pub(super) struct SubdirCache<'a> {
    pub(super) map: &'a mut HashMap<PathBuf, (Arc<Vec<FileEntry>>, Instant)>,
    pub(super) ttl: Duration,
    pub(super) loading: &'a mut HashSet<PathBuf>,
    pub(super) load_tx: &'a std::sync::mpsc::Sender<(PathBuf, Vec<FileEntry>)>,
    pub(super) ctx: egui::Context,
}

const SUBDIR_CACHE_MAX: usize = 256;
const MAX_CONCURRENT_LOADS: usize = 8;

impl<'a> SubdirCache<'a> {
    pub(super) fn get_or_read(&mut self, path: &Path) -> Arc<Vec<FileEntry>> {
        if let Some((entries, t)) = self.map.get_mut(path) {
            if t.elapsed() < self.ttl {
                return Arc::clone(entries);
            }
            // Stale: return old data, dispatch background refresh
            let stale = Arc::clone(entries);
            *t = Instant::now();
            self.dispatch_load(path);
            return stale;
        }
        // Cache miss: dispatch background load, return empty
        self.dispatch_load(path);
        if self.map.len() >= SUBDIR_CACHE_MAX {
            let oldest = self
                .map
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k.clone());
            if let Some(k) = oldest {
                self.map.remove(&k);
            }
        }
        Arc::new(Vec::new())
    }

    fn dispatch_load(&mut self, path: &Path) {
        if self.loading.contains(path) {
            return;
        }
        if SUBDIR_IN_FLIGHT.load(Ordering::Relaxed) >= MAX_CONCURRENT_LOADS {
            return; // will retry next frame
        }
        self.loading.insert(path.to_path_buf());
        SUBDIR_IN_FLIGHT.fetch_add(1, Ordering::Relaxed);
        let p = path.to_path_buf();
        let tx = self.load_tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let entries = list_dir_entries(&p);
            let _ = tx.send((p, entries));
            SUBDIR_IN_FLIGHT.fetch_sub(1, Ordering::Relaxed);
            ctx.request_repaint();
        });
    }
}

pub(super) struct DirData {
    pub(super) is_git: bool,
    pub(super) git_diff: String,
    pub(super) git_status: Arc<super::git_worker::GitStatusSnapshot>,
    pub(super) git_unpushed: Vec<(String, String)>,
    pub(super) git_refresh_at: Option<Instant>,
    pub(super) merge_operation: super::git_worker::MergeOperation,
    pub(super) md_files: HashMap<PathBuf, Arc<String>>,
    pub(super) dir_entries: Arc<Vec<FileEntry>>,
}

impl DirData {
    pub(super) fn new(path: &Path) -> Self {
        let is_git = crate::util::find_git_root(path).is_some();
        DirData {
            is_git,
            git_diff: String::new(),
            git_status: Arc::new(super::git_worker::GitStatusSnapshot::default()),
            git_unpushed: Vec::new(),
            git_refresh_at: if is_git { Some(Instant::now()) } else { None },
            merge_operation: super::git_worker::MergeOperation::None,
            md_files: HashMap::new(),
            dir_entries: Arc::new(list_dir_entries(path)),
        }
    }
}

pub(super) fn list_dir_entries(path: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    if let Ok(rd) = std::fs::read_dir(path) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with('.') {
                const ALLOWED_DOTFILES: &[&str] = &[
                    ".github",
                    ".vscode",
                    ".gitignore",
                    ".gitmodules",
                    ".editorconfig",
                    ".env.example",
                    ".dockerignore",
                    ".prettierrc",
                    ".eslintrc",
                ];
                if !ALLOWED_DOTFILES.iter().any(|&a| name == a) {
                    continue;
                }
            }
            // Follow symlinks so symlinked directories show as dirs (L4)
            let is_dir = std::fs::metadata(&p).map(|m| m.is_dir()).unwrap_or(false);
            entries.push(FileEntry {
                is_dir,
                name,
                path: p,
            });
        }
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    entries
}

const REVEAL_HIGHLIGHT_DURATION: std::time::Duration = std::time::Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq)]
struct TreeRowLayout {
    chevron_x: f32,
    icon_x: f32,
    name_x: f32,
}

fn tree_row_layout(depth: usize, _is_dir: bool) -> TreeRowLayout {
    let chevron_x = depth as f32 * crate::theme::SP_6;
    let icon_x = chevron_x + crate::theme::TREE_CHEVRON_W;
    TreeRowLayout {
        chevron_x,
        icon_x,
        name_x: icon_x + crate::theme::SP_6,
    }
}

fn should_paint_tree_row(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.is_rect_visible(rect)
}

const CHEVRON_RIGHT_TEXT: &str = "\u{e073}";
const CHEVRON_DOWN_TEXT: &str = "\u{e071}";
const FOLDER_TEXT: &str = "\u{e0dc}";
const FOLDER_OPEN_TEXT: &str = "\u{e246}";
const FILE_TEXT: &str = "\u{e0c4}";
const FILE_DOCUMENT_TEXT: &str = "\u{e0d0}";

pub(super) struct RevealState {
    pub(super) path: Option<PathBuf>,
    pub(super) highlight: Option<(PathBuf, std::time::Instant)>,
}

pub(super) struct TreeRenderState<'a> {
    pub(super) open_editor: &'a mut Option<PathBuf>,
    pub(super) open_terminal_at: &'a mut Option<PathBuf>,
    pub(super) drag_state: &'a mut super::drag::DragState,
    pub(super) reveal: &'a mut RevealState,
}

pub(super) fn render_dir_tree(
    ui: &mut egui::Ui,
    entries: &[FileEntry],
    modified: &HashSet<PathBuf>,
    cache: &mut SubdirCache<'_>,
    state: &mut TreeRenderState<'_>,
) {
    render_dir_tree_at_depth(ui, entries, modified, cache, state, 0);
}

fn render_dir_tree_at_depth(
    ui: &mut egui::Ui,
    entries: &[FileEntry],
    modified: &HashSet<PathBuf>,
    cache: &mut SubdirCache<'_>,
    state: &mut TreeRenderState<'_>,
    depth: usize,
) {
    use crate::theme;
    if entries.is_empty() {
        ui.label(
            egui::RichText::new("(empty directory)")
                .italics()
                .color(theme::active().overlay0)
                .size(theme::FONT_SM),
        );
        return;
    }

    // Expire stale highlight
    if let Some((_, t)) = state.reveal.highlight.as_ref() {
        if t.elapsed() >= REVEAL_HIGHLIGHT_DURATION {
            state.reveal.highlight = None;
        }
    }

    ui.spacing_mut().item_spacing.y = theme::SP_0;

    for entry in entries {
        let is_reveal_ancestor = state
            .reveal
            .path
            .as_ref()
            .map(|rp| rp.starts_with(&entry.path) && *rp != entry.path)
            .unwrap_or(false);
        let is_reveal_target = state
            .reveal
            .path
            .as_ref()
            .map(|rp| *rp == entry.path)
            .unwrap_or(false);
        let highlight_alpha = state
            .reveal
            .highlight
            .as_ref()
            .filter(|(p, _)| *p == entry.path)
            .map(|(_, t)| {
                let elapsed = t.elapsed().as_secs_f32();
                let total = REVEAL_HIGHLIGHT_DURATION.as_secs_f32();
                (1.0 - (elapsed / total)).clamp(0.0, 1.0)
            });
        let is_highlighted = is_reveal_target || highlight_alpha.is_some();

        let id = ui.make_persistent_id(&entry.path);
        let mut dir_state = entry.is_dir.then(|| {
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
        });
        if is_reveal_ancestor {
            if let Some(state) = dir_state.as_mut().filter(|state| !state.is_open()) {
                state.set_open(true);
            }
        }

        let is_open = dir_state.as_ref().is_some_and(|state| state.is_open());
        let row_layout = tree_row_layout(depth, entry.is_dir);
        let row_width = ui.available_width().max(theme::SP_0);
        let sense = if entry.is_dir {
            egui::Sense::click()
        } else {
            egui::Sense::click_and_drag()
        };
        let (row_rect, response) =
            ui.allocate_exact_size(egui::vec2(row_width, theme::FILE_ROW_H), sense);
        let response = response.on_hover_text(if entry.is_dir {
            "Double-click to open terminal here"
        } else {
            entry.name.as_str()
        });

        let is_modified = !modified.is_empty() && modified.contains(&entry.path);
        if should_paint_tree_row(ui, row_rect) {
            let t = theme::active();
            if is_highlighted {
                let alpha = highlight_alpha.unwrap_or(1.0);
                ui.painter().rect_filled(
                    row_rect,
                    theme::R_SM,
                    t.accent.linear_multiply(theme::BLEND_SUBTLE * alpha),
                );
            }

            let chevron = if entry.is_dir {
                if is_open {
                    CHEVRON_DOWN_TEXT
                } else {
                    CHEVRON_RIGHT_TEXT
                }
            } else {
                ""
            };
            let icon = if entry.is_dir {
                if is_open {
                    FOLDER_OPEN_TEXT
                } else {
                    FOLDER_TEXT
                }
            } else if matches!(
                entry.path.extension().and_then(|ext| ext.to_str()),
                Some("md" | "toml" | "yaml" | "yml" | "json" | "txt")
            ) {
                FILE_DOCUMENT_TEXT
            } else {
                FILE_TEXT
            };
            let icon_font =
                egui::FontId::new(theme::FONT_SM, egui::FontFamily::Name("icons".into()));
            let chevron_font =
                egui::FontId::new(theme::FONT_XS, egui::FontFamily::Name("icons".into()));
            let center_y = row_rect.center().y;
            if !chevron.is_empty() {
                ui.painter().text(
                    egui::pos2(row_rect.min.x + row_layout.chevron_x, center_y),
                    egui::Align2::LEFT_CENTER,
                    chevron,
                    chevron_font,
                    t.fg_muted,
                );
            }
            ui.painter().text(
                egui::pos2(row_rect.min.x + row_layout.icon_x, center_y),
                egui::Align2::LEFT_CENTER,
                icon,
                icon_font,
                t.fg_muted,
            );

            let badge_width = if is_modified {
                theme::TREE_CHEVRON_W
            } else {
                theme::SP_0
            };
            let name_width =
                (row_rect.width() - row_layout.name_x - badge_width - theme::SP_2).max(0.0);
            let name_color = if is_highlighted { t.accent } else { t.fg };
            let name_font = egui::FontId::proportional(theme::FONT_SM);
            let name_galley = crate::ui_kit::truncate_galley(
                ui.painter(),
                &entry.name,
                name_font,
                name_color,
                name_width,
            );
            ui.painter().galley(
                egui::pos2(
                    row_rect.min.x + row_layout.name_x,
                    center_y - name_galley.size().y * 0.5,
                ),
                name_galley,
                name_color,
            );

            if is_modified {
                ui.painter().text(
                    egui::pos2(row_rect.max.x - theme::SP_2, center_y),
                    egui::Align2::RIGHT_CENTER,
                    "M",
                    egui::FontId::new(theme::FONT_XS, crate::fonts::ui_bold()),
                    t.warning,
                );
            }
        }

        if is_reveal_target {
            response.scroll_to_me(Some(egui::Align::Center));
            state.reveal.highlight = Some((entry.path.clone(), std::time::Instant::now()));
            state.reveal.path = None;
            ui.ctx().request_repaint();
        }
        if highlight_alpha.is_some() {
            ui.ctx().request_repaint();
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        response.context_menu(|ui| {
            dir_entry_context_menu(ui, &entry.path);
        });

        if entry.is_dir {
            if response.double_clicked() {
                *state.open_terminal_at = Some(entry.path.clone());
            } else if response.clicked() {
                if let Some(state) = dir_state.as_mut() {
                    state.toggle(ui);
                }
            }
            if let Some(mut collapse_state) = dir_state {
                collapse_state.show_body_unindented(ui, |ui| {
                    let children = cache.get_or_read(&entry.path);
                    render_dir_tree_at_depth(ui, &children, modified, cache, state, depth + 1);
                });
            }
        } else {
            if response.clicked() {
                *state.open_editor = Some(entry.path.clone());
            }
            if response.drag_started() {
                let origin = response.interact_pointer_pos().unwrap_or_default();
                state.drag_state.set_payload(
                    super::drag::DragPayload::File(entry.path.clone()),
                    origin,
                    &entry.name,
                );
            }
        }
    }
}

pub(super) fn render_flat_file_list(
    ui: &mut egui::Ui,
    entries: &[FileEntry],
    root: &Path,
    open_editor: &mut Option<PathBuf>,
    open_terminal_at: &mut Option<PathBuf>,
    drag_state: &mut super::drag::DragState,
) {
    use crate::theme;
    let _ = open_terminal_at;
    if entries.is_empty() {
        ui.label(
            egui::RichText::new("No matching files")
                .italics()
                .color(theme::active().overlay0)
                .size(theme::FONT_SM),
        );
        return;
    }

    ui.spacing_mut().item_spacing.y = 2.0;
    for entry in entries {
        let ext = entry
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let is_md = ext == "md";
        let color = if is_md {
            theme::active().fg_md_file
        } else {
            theme::active().fg_other_file
        };
        let rel = entry.path.strip_prefix(root).unwrap_or(&entry.path);
        let display = rel.display().to_string();
        let resp = ui
            .horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::SP_2;
                ui.label(crate::ui_kit::icon(file_icon(ext), theme::FONT_SM, color));
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&display)
                            .family(egui::FontFamily::Proportional)
                            .color(color)
                            .size(theme::FONT_SM),
                    )
                    .truncate()
                    .sense(egui::Sense::click_and_drag()),
                )
            })
            .inner
            .on_hover_text(entry.path.display().to_string());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            *open_editor = Some(entry.path.clone());
        }
        if resp.drag_started() && !entry.is_dir {
            let origin = resp.interact_pointer_pos().unwrap_or_default();
            drag_state.set_payload(
                super::drag::DragPayload::File(entry.path.clone()),
                origin,
                &display,
            );
        }
        resp.context_menu(|ui| {
            dir_entry_context_menu(ui, &entry.path);
        });
    }
}

pub(super) fn is_supported_text_file(_path: &Path, content: &str) -> bool {
    if content.is_empty() {
        return true;
    }
    if content.len() >= 2_000_000 {
        return false;
    }
    let bytes = content.as_bytes();
    // Check first 8KB for binary indicators
    let check_len = bytes.len().min(8192);
    let sample = &bytes[..check_len];
    // Null bytes are a definitive binary signal
    if sample.contains(&0) {
        return false;
    }
    // High ratio of non-text control chars indicates binary (PDF, images, etc.)
    let control_count = sample
        .iter()
        .filter(|&&b| b < 0x20 && b != b'\n' && b != b'\r' && b != b'\t' && b != 0x1b)
        .count();
    let threshold = check_len / 8;
    control_count <= threshold
}

fn dir_entry_context_menu(ui: &mut egui::Ui, path: &Path) {
    if ui.button("Copy path").clicked() {
        let text = path.display().to_string();
        std::thread::spawn(move || {
            if let Ok(mut clip) = arboard::Clipboard::new() {
                let _ = clip.set_text(text);
            }
        });
        ui.close_menu();
    }
    if ui.button("Reveal in file manager").clicked() {
        crate::util::reveal_in_file_manager(path);
        ui.close_menu();
    }
}

pub(super) fn file_icon(ext: &str) -> char {
    match ext {
        "md" | "toml" | "yaml" | "yml" | "json" | "txt" => crate::ui_kit::icon::FILE_TEXT,
        _ => crate::ui_kit::icon::FILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_icon_returns_lucide_glyph() {
        assert_eq!(file_icon("rs"), crate::ui_kit::icon::FILE);
        assert_eq!(file_icon("md"), crate::ui_kit::icon::FILE_TEXT);
        assert_ne!(file_icon("rs"), '·');
    }

    #[test]
    fn tree_row_layout_aligns_names() {
        let dir = tree_row_layout(2, true);
        let file = tree_row_layout(2, false);

        assert_eq!(dir.name_x, file.name_x);
        assert_eq!(dir.icon_x, file.icon_x);
        assert_eq!(dir.icon_x - dir.chevron_x, crate::theme::TREE_CHEVRON_W);
    }

    #[test]
    fn tree_lays_out_only_visible_rows() {
        let ctx = crate::ui_kit::test_harness::ctx_with_fonts();
        let mut visible_count = 0;
        let _ = crate::ui_kit::test_harness::run_ui(&ctx, egui::RawInput::default(), |ui| {
            ui.set_clip_rect(egui::Rect::from_min_max(
                egui::pos2(0.0, 10.0),
                egui::pos2(100.0, 30.0),
            ));
            let rows = [
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 8.0)),
                egui::Rect::from_min_size(egui::pos2(0.0, 12.0), egui::vec2(100.0, 8.0)),
                egui::Rect::from_min_size(egui::pos2(0.0, 32.0), egui::vec2(100.0, 8.0)),
            ];
            visible_count = rows
                .iter()
                .filter(|row| should_paint_tree_row(ui, **row))
                .count();
        });
        assert_eq!(visible_count, 1);
    }

    #[test]
    fn modified_paths_match_list_dir_entries_paths() {
        let root = std::env::temp_dir().join(format!(
            "terminal_studio_modified_paths_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("changed.rs"), "changed").unwrap();

        let snapshot =
            super::super::git_worker::GitStatusSnapshot::from_status(" M changed.rs\n", &root);
        let entries = list_dir_entries(&root);

        assert!(snapshot.modified.contains(&entries[0].path));
        let _ = std::fs::remove_dir_all(root);
    }
}
