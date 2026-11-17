//! The Open Folder dialog: one or more folders, each opened in its own
//! terminal and, unless unticked, saved as a workspace with a name and a
//! colour picked to stand apart from the rest. One folder gets the full form;
//! several get a compact row each (tick, colour, name, path).

use super::super::workspace_ui::{EntryCheck, FolderEntry, OpenFolderDialog};
use super::super::App;
use super::color_choice::{color_choices, ColorEdit};
use crate::theme;
use crate::ui_kit;
use crate::workspace::WorkspaceStore;

const TITLE_ONE: &str = "Open Folder";
const TITLE_WORKSPACE: &str = "Open Workspace Folder";
const SAVE_LABEL: &str = "Save as Workspace";
const SAVE_TOOLTIP: &str = "Save as a workspace; untick to only open a terminal there";
const NAME_HINT: &str = "e.g. My Project";
const NAME_MISSING: &str = "Give the workspace a name";
const NAME_TAKEN: &str = "A workspace with this name already exists";
const COLOR_TAKEN: &str = "This color is already used by another workspace";
const COLOR_BUTTON_TOOLTIP: &str = "Change color";
const MANY_HINT: &str = "Each folder opens in its own terminal. Colors are picked to \
                         stand apart; click one to change it.";
const MAX_LIST_HEIGHT: f32 = 320.0;
const DIALOG_MAX_HEIGHT: f32 = 560.0;

fn title(dialog: &OpenFolderDialog) -> String {
    match dialog.entries.as_slice() {
        [only] if only.existing_workspace_id.is_some() => TITLE_WORKSPACE.to_string(),
        [_] => TITLE_ONE.to_string(),
        many => format!("Open {} Folders", many.len()),
    }
}

fn inside_note(parent_name: &str) -> String {
    format!("This folder is inside workspace \"{parent_name}\"")
}

/// The first problem of `check`, worded for the user.
fn problem_text(check: &EntryCheck) -> Option<&'static str> {
    if check.name_missing {
        Some(NAME_MISSING)
    } else if check.name_taken {
        Some(NAME_TAKEN)
    } else if check.color_taken {
        Some(COLOR_TAKEN)
    } else {
        None
    }
}

fn problem_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme::FONT_UI_XS)
            .color(theme::active().error),
    );
}

fn path_label(ui: &mut egui::Ui, entry: &FolderEntry, max_width: f32) {
    let t = theme::active();
    let galley = ui_kit::one_line_elided(
        ui,
        &theme::short_path(&entry.path),
        egui::FontId::monospace(theme::FONT_UI_SM),
        t.fg_path,
        max_width,
    );
    let resp = ui.add(egui::Label::new(galley).selectable(false));
    ui_kit::full_hover_text(resp, &entry.path.display().to_string());
}

fn shell_chips(ui: &mut egui::Ui, dialog: &mut OpenFolderDialog) {
    ui.label(ui_kit::label_secondary("Terminal"));
    ui.add_space(theme::SP_1);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(theme::SP_3, theme::SP_3);
        for shell in &dialog.available_shells {
            let selected = dialog.selected_shell == *shell;
            if ui_kit::toggle_chip(ui, shell.display_name(), selected).clicked() {
                dialog.selected_shell = shell.clone();
            }
        }
    });
}

fn name_field(ui: &mut egui::Ui, entry: &mut FolderEntry, focus: bool) {
    let resp = ui.add(
        egui::TextEdit::singleline(&mut entry.name)
            .hint_text(NAME_HINT)
            .desired_width(f32::INFINITY),
    );
    if focus {
        resp.request_focus();
    }
}

/// The full form for one folder. Returns the parent workspace to open
/// instead, when the user asks for it.
fn single_folder_form(
    ui: &mut egui::Ui,
    dialog: &mut OpenFolderDialog,
    check: &EntryCheck,
) -> Option<u64> {
    let mut open_parent = None;
    let focus = !std::mem::replace(&mut dialog.focus_requested, true);
    let width = ui.available_width();
    path_label(ui, &dialog.entries[0], width);
    ui.add_space(theme::SP_4);

    if let Some((parent_id, parent_name)) = dialog.entries[0].parent_workspace.clone() {
        ui.label(
            egui::RichText::new(inside_note(&parent_name))
                .size(theme::FONT_UI_SM)
                .color(theme::active().warning),
        );
        let open_instead = egui::Button::new(
            egui::RichText::new(format!("Open \"{parent_name}\" instead")).size(theme::FONT_UI_SM),
        )
        .rounding(theme::R_SM);
        if ui.add(open_instead).clicked() {
            open_parent = Some(parent_id);
        }
        ui.add_space(theme::SP_4);
    }

    shell_chips(ui, dialog);
    ui.add_space(theme::SP_5);

    let entry = &mut dialog.entries[0];
    ui.checkbox(&mut entry.save_as_workspace, ui_kit::label(SAVE_LABEL));
    ui.add_space(theme::SP_3);
    if entry.save_as_workspace {
        ui.label(ui_kit::label_secondary("Name"));
        name_field(ui, entry, focus);
        if check.name_missing {
            problem_label(ui, NAME_MISSING);
        } else if check.name_taken {
            problem_label(ui, NAME_TAKEN);
        }
        ui.add_space(theme::SP_3);
        ui.label(ui_kit::label_secondary("Color"));
        color_choices(ui, &mut entry.color);
        if check.color_taken {
            problem_label(ui, COLOR_TAKEN);
        }
        ui.add_space(theme::SP_3);
    }
    open_parent
}

/// One compact row per folder: tick, colour button, name, then the path.
fn folder_rows(ui: &mut egui::Ui, dialog: &mut OpenFolderDialog, checks: &[EntryCheck]) {
    let focus_first = !std::mem::replace(&mut dialog.focus_requested, true);
    egui::ScrollArea::vertical()
        .max_height(MAX_LIST_HEIGHT)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, entry) in dialog.entries.iter_mut().enumerate() {
                ui.push_id(i, |ui| {
                    folder_row(ui, entry, &checks[i], focus_first && i == 0);
                });
                ui.add_space(theme::SP_3);
            }
        });
}

fn folder_row(ui: &mut egui::Ui, entry: &mut FolderEntry, check: &EntryCheck, focus: bool) {
    let t = theme::active();
    let colors_open_id = ui.id().with("colors_open");
    ui.horizontal(|ui| {
        ui.checkbox(&mut entry.save_as_workspace, "")
            .on_hover_text(SAVE_TOOLTIP);
        if entry.save_as_workspace {
            color_button(ui, entry, colors_open_id);
            name_field(ui, entry, focus);
        } else {
            let name = entry
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // Elided, with the whole name on hover.
            ui.add(egui::Label::new(egui::RichText::new(name).color(t.subtext0)).truncate());
        }
    });
    ui.indent("path", |ui| {
        let width = ui.available_width();
        path_label(ui, entry, width);
        if let Some((_, parent_name)) = &entry.parent_workspace {
            ui.label(
                egui::RichText::new(inside_note(parent_name))
                    .size(theme::FONT_UI_XS)
                    .color(t.warning),
            );
        }
        if let Some(text) = problem_text(check) {
            problem_label(ui, text);
        }
        let colors_open = entry.save_as_workspace
            && ui.data(|d| d.get_temp::<bool>(colors_open_id).unwrap_or(false));
        if colors_open && color_choices(ui, &mut entry.color) == Some(ColorEdit::Preset) {
            ui.data_mut(|d| d.insert_temp(colors_open_id, false));
        }
    });
}

/// A swatch in the folder's colour; clicking it shows the colour choices
/// under the row (see `color_choices` for why not a popup).
fn color_button(ui: &mut egui::Ui, entry: &FolderEntry, open_id: egui::Id) {
    let resp =
        ui_kit::color_swatch(ui, entry.color.color(), false).on_hover_text(COLOR_BUTTON_TOOLTIP);
    if resp.clicked() {
        ui.data_mut(|d| {
            let open = d.get_temp_mut_or_default::<bool>(open_id);
            *open = !*open;
        });
    }
}

impl App {
    pub(in crate::app) fn render_open_folder_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.open_folder_dialog.take() else {
            return;
        };
        let store: &WorkspaceStore = &self.workspace_store;
        let checks = dialog.checks(store);
        let can_open = checks.iter().all(EntryCheck::is_ok);
        let many = dialog.entries.len() > 1;

        let mut open_it = false;
        let mut cancel_clicked = false;
        let mut open_parent = None;
        let config = ui_kit::DialogConfig {
            width: ui_kit::DialogWidth::Responsive {
                pct: 0.45,
                min: 360.0,
                max: 560.0,
            },
            max_height: DIALOG_MAX_HEIGHT,
            ..Default::default()
        };
        let resp = ui_kit::dialog(ctx, self.vp_id("open_folder"), config, |ui| {
            ui_kit::dialog_header(ui, &title(&dialog));
            if many {
                ui.label(
                    egui::RichText::new(MANY_HINT)
                        .size(theme::FONT_UI_SM)
                        .color(theme::active().subtext0),
                );
                ui.add_space(theme::SP_4);
                shell_chips(ui, &mut dialog);
                ui.add_space(theme::SP_5);
                folder_rows(ui, &mut dialog, &checks);
            } else {
                open_parent = single_folder_form(ui, &mut dialog, &checks[0]);
            }
            ui_kit::dialog_footer(ui, |ui| {
                if ui_kit::action_button(ui, "Open", can_open, ui_kit::ActionButtonStyle::Primary)
                    .clicked()
                {
                    open_it = true;
                }
                if ui_kit::action_button(ui, "Cancel", true, ui_kit::ActionButtonStyle::Cancel)
                    .clicked()
                {
                    cancel_clicked = true;
                }
            });
        });

        // Enter opens, checked against this frame's edits.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            && dialog.can_open(&self.workspace_store)
        {
            open_it = true;
        }

        if let Some(parent_id) = open_parent {
            self.navigate_to_workspace(parent_id);
        } else if open_it && dialog.can_open(&self.workspace_store) {
            self.open_folders(dialog);
        } else if !(resp.dismissed || cancel_clicked) {
            self.open_folder_dialog = Some(dialog);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::ShellKind;
    use std::path::PathBuf;

    fn dialog(paths: &[&str]) -> OpenFolderDialog {
        OpenFolderDialog::new(
            paths.iter().map(PathBuf::from).collect(),
            ShellKind::Bash,
            Vec::new(),
            &WorkspaceStore::default(),
        )
        .unwrap()
    }

    /// Draws the dialog's body for `frames` frames, feeding `events` to the
    /// last one, as egui would in the app.
    fn render(dialog: &mut OpenFolderDialog, frames: usize, events: Vec<egui::Event>) {
        let ctx = egui::Context::default();
        let store = WorkspaceStore::default();
        for frame in 0..frames {
            let input = egui::RawInput {
                events: if frame + 1 == frames {
                    events.clone()
                } else {
                    Vec::new()
                },
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let checks = dialog.checks(&store);
                    if dialog.entries.len() > 1 {
                        folder_rows(ui, dialog, &checks);
                    } else {
                        single_folder_form(ui, dialog, &checks[0]);
                    }
                });
            });
        }
    }

    #[test]
    fn the_name_field_takes_the_keyboard_when_the_dialog_opens() {
        for paths in [&["/a/app"][..], &["/a/app", "/b/api", "/c/web"][..]] {
            let mut dlg = dialog(paths);
            render(&mut dlg, 3, vec![egui::Event::Text("X".into())]);
            assert!(dlg.focus_requested);
            assert!(
                dlg.entries[0].name.ends_with('X'),
                "typing goes to the first name: {:?}",
                dlg.entries[0].name
            );
            for other in &dlg.entries[1..] {
                assert!(!other.name.contains('X'));
            }
        }
    }

    #[test]
    fn rows_draw_for_saved_unsaved_and_nested_folders() {
        let store = crate::workspace::WorkspaceStore {
            workspaces: vec![crate::workspace::Workspace {
                id: 1,
                name: "proj".into(),
                path: PathBuf::from("/work/proj"),
                color: [1, 2, 3],
                host_window_id: None,
                last_activated: 0,
            }],
        };
        let mut dlg = OpenFolderDialog::new(
            vec![
                PathBuf::from("/work/proj/src"),
                PathBuf::from("/work/proj"),
                PathBuf::from("/x"),
            ],
            ShellKind::Bash,
            vec![ShellKind::Bash, ShellKind::Zsh],
            &store,
        )
        .unwrap();
        dlg.entries[2].save_as_workspace = false;
        render(&mut dlg, 2, Vec::new());
        assert_eq!(dlg.entries.len(), 3, "drawing changes nothing");
    }

    #[test]
    fn title_counts_the_folders() {
        assert_eq!(title(&dialog(&["/a"])), TITLE_ONE);
        assert_eq!(title(&dialog(&["/a", "/b", "/c"])), "Open 3 Folders");
    }

    #[test]
    fn problems_are_named_in_order_of_what_to_fix_first() {
        let all = EntryCheck {
            name_missing: true,
            name_taken: true,
            color_taken: true,
        };
        assert_eq!(problem_text(&all), Some(NAME_MISSING));
        let colour_only = EntryCheck {
            color_taken: true,
            ..Default::default()
        };
        assert_eq!(problem_text(&colour_only), Some(COLOR_TAKEN));
        assert_eq!(problem_text(&EntryCheck::default()), None);
    }
}
