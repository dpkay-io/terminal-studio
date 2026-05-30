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

impl<'a> SubdirCache<'a> {
    pub(super) fn get_or_read(&mut self, path: &Path) -> Arc<Vec<FileEntry>> {
        if let Some((entries, t)) = self.map.get(path) {
            if t.elapsed() < self.ttl {
                return Arc::clone(entries);
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
    pub(super) git_refresh_at: Option<Instant>,
    pub(super) md_files: HashMap<PathBuf, Arc<String>>,
    pub(super) dir_entries: Arc<Vec<FileEntry>>,
}

impl DirData {
    /// Non-blocking constructor. Checks for `.git` existence (file or dir)
    /// instead of spawning `git rev-parse` which can block for 50-200ms.
    pub(super) fn new(path: &Path) -> Self {
        let is_git = path.join(".git").exists();
        DirData {
            is_git,
            git_diff: String::new(),
            git_status: String::new(),
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
                continue;
            }
            let is_dir = e.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
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
    let status = git(&["status", "--porcelain"]);
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
            if resp.clicked() {
                *open_editor = Some(entry.path.clone());
            }
        }
    }
}

#[allow(dead_code)]
pub(super) fn collect_all_files(root: &Path, out: &mut Vec<FileEntry>, max_depth: usize) {
    if max_depth == 0 {
        return;
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with('.') {
                continue;
            }
            let is_dir = p.is_dir();
            if is_dir {
                collect_all_files(&p, out, max_depth - 1);
            } else {
                out.push(FileEntry {
                    name,
                    path: p,
                    is_dir: false,
                });
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
        if resp.clicked() {
            *open_editor = Some(entry.path.clone());
        }
    }
}

pub(super) fn is_supported_text_file(_path: &Path, content: &str) -> bool {
    if content.is_empty() {
        return true;
    }
    content.len() < 2_000_000 && !content.as_bytes().contains(&0)
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
