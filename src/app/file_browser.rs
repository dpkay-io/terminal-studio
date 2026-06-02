use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub(super) struct FileEntry {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) is_dir: bool,
}

pub(super) struct SubdirCache<'a> {
    pub(super) map: &'a mut HashMap<PathBuf, (Arc<Vec<FileEntry>>, Instant)>,
    pub(super) ttl: Duration,
}

const SUBDIR_CACHE_MAX: usize = 256;

impl<'a> SubdirCache<'a> {
    pub(super) fn get_or_read(&mut self, path: &Path) -> Arc<Vec<FileEntry>> {
        if let Some((entries, t)) = self.map.get(path) {
            if t.elapsed() < self.ttl {
                return Arc::clone(entries);
            }
        }
        // Evict oldest entries when cache exceeds limit (L2)
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
        let entries = Arc::new(list_dir_entries(path));
        self.map
            .insert(path.to_path_buf(), (Arc::clone(&entries), Instant::now()));
        entries
    }
}

pub(super) struct DirData {
    pub(super) is_git: bool,
    pub(super) git_diff: String,
    pub(super) git_status: String,
    pub(super) git_unpushed: Vec<(String, String)>,
    pub(super) git_refresh_at: Option<Instant>,
    pub(super) md_files: HashMap<PathBuf, Arc<String>>,
    pub(super) dir_entries: Arc<Vec<FileEntry>>,
}

impl DirData {
    pub(super) fn new(path: &Path) -> Self {
        let is_git = path.join(".git").exists();
        DirData {
            is_git,
            git_diff: String::new(),
            git_status: String::new(),
            git_unpushed: Vec::new(),
            git_refresh_at: if is_git { Some(Instant::now()) } else { None },
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

pub(super) fn run_git_info(dir: &Path) -> (String, String) {
    use std::process::Command;
    let git = |args: &[&str]| -> String {
        Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .unwrap_or_default()
    };
    let staged = git(&["diff", "--cached", "--no-color"]);
    let unstaged = git(&["diff", "--no-color"]);
    let status = git(&["status", "--porcelain", "-uall"]);
    let diff = match (staged.is_empty(), unstaged.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!("=== Staged ===\n{staged}"),
        (true, false) => format!("=== Unstaged ===\n{unstaged}"),
        (false, false) => format!("=== Staged ===\n{staged}\n=== Unstaged ===\n{unstaged}"),
    };
    (diff, status)
}

pub(super) fn render_dir_tree(
    ui: &mut egui::Ui,
    entries: &[FileEntry],
    open_editor: &mut Option<PathBuf>,
    open_terminal_at: &mut Option<PathBuf>,
    cache: &mut SubdirCache<'_>,
) {
    use crate::theme;
    if entries.is_empty() {
        ui.label(
            egui::RichText::new("(empty directory)")
                .italics()
                .color(theme::active().overlay0)
                .size(theme::FONT_UI_MD),
        );
        return;
    }

    ui.spacing_mut().item_spacing.y = theme::SP_1;

    for entry in entries {
        if entry.is_dir {
            let id = ui.make_persistent_id(&entry.path);
            let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                false,
            );
            let chevron = if state.is_open() { "▼" } else { "▶" };
            let resp = ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(format!("{} {}", chevron, &entry.name))
                            .color(theme::active().fg_dir_entry)
                            .size(theme::FONT_UI_MD),
                    )
                    .truncate()
                    .sense(egui::Sense::click()),
                )
                .on_hover_text("Double-click to open terminal here");
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.double_clicked() {
                *open_terminal_at = Some(entry.path.clone());
            } else if resp.clicked() {
                state.toggle(ui);
            }
            state.show_body_indented(&resp, ui, |ui| {
                let children = cache.get_or_read(&entry.path);
                render_dir_tree(ui, &children, open_editor, open_terminal_at, cache);
            });
        } else {
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
            let resp = ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(format!("{} {}", file_icon(ext), &entry.name))
                            .color(color)
                            .size(theme::FONT_UI_MD),
                    )
                    .truncate()
                    .sense(egui::Sense::click()),
                )
                .on_hover_text(&entry.name);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                *open_editor = Some(entry.path.clone());
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
) {
    use crate::theme;
    let _ = open_terminal_at;
    if entries.is_empty() {
        ui.label(
            egui::RichText::new("No matching files")
                .italics()
                .color(theme::active().overlay0)
                .size(theme::FONT_UI_MD),
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
            .add(
                egui::Label::new(
                    egui::RichText::new(format!("{} {}", file_icon(ext), &display))
                        .color(color)
                        .size(theme::FONT_UI_MD),
                )
                .truncate()
                .sense(egui::Sense::click()),
            )
            .on_hover_text(entry.path.display().to_string());
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            *open_editor = Some(entry.path.clone());
        }
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

pub(super) fn file_icon(ext: &str) -> &'static str {
    match ext {
        "rs" => "⚙",
        "md" => "📝",
        "toml" | "yaml" | "yml" => "≡",
        "json" => "≡",
        "txt" => "≡",
        "sh" | "bat" | "ps1" | "cmd" => "▸",
        "py" => "▸",
        "js" | "ts" | "jsx" | "tsx" => "▸",
        _ => "·",
    }
}
