use egui::{Color32, FontFamily, RichText};

#[allow(dead_code)]
pub mod icon {
    pub const CHEVRON_RIGHT: char = '\u{e073}';
    pub const CHEVRON_DOWN: char = '\u{e071}';
    pub const CHEVRON_UP: char = '\u{e074}';
    pub const FOLDER: char = '\u{e0dc}';
    pub const FOLDER_OPEN: char = '\u{e246}';
    pub const FILE: char = '\u{e0c4}';
    pub const FILE_TEXT: char = '\u{e0d0}';
    pub const SEARCH: char = '\u{e154}';
    pub const GIT_BRANCH: char = '\u{e0e5}';
    pub const COLUMNS_2: char = '\u{e09c}';
    pub const ROWS_2: char = '\u{e43d}';
    pub const X: char = '\u{e1b1}';
    pub const POWER: char = '\u{e143}';
    pub const TERMINAL: char = '\u{e184}';
    pub const MENU: char = '\u{e118}';
    pub const ARROW_LEFT_RIGHT: char = '\u{e249}';
    pub const SETTINGS: char = '\u{e157}';
    pub const PLUS: char = '\u{e140}';
    pub const KEYBOARD: char = '\u{e283}';
    pub const HISTORY: char = '\u{e1f4}';
    pub const PANEL_LEFT: char = '\u{e12d}';
    pub const PANEL_RIGHT: char = '\u{e435}';
    pub const DOWNLOAD: char = '\u{e0b6}';
    pub const REFRESH_CW: char = '\u{e148}';
    pub const COPY: char = '\u{e0a2}';
    pub const TRASH: char = '\u{e18c}';
    pub const PENCIL: char = '\u{e1f8}';
    pub const MAXIMIZE: char = '\u{e115}';
    pub const MINIMIZE: char = '\u{e11d}';
    pub const SQUARE: char = '\u{e16a}';
}

#[allow(dead_code)]
pub const ALL: &[(&str, char)] = &[
    ("chevron-right", icon::CHEVRON_RIGHT),
    ("chevron-down", icon::CHEVRON_DOWN),
    ("chevron-up", icon::CHEVRON_UP),
    ("folder", icon::FOLDER),
    ("folder-open", icon::FOLDER_OPEN),
    ("file", icon::FILE),
    ("file-text", icon::FILE_TEXT),
    ("search", icon::SEARCH),
    ("git-branch", icon::GIT_BRANCH),
    ("columns-2", icon::COLUMNS_2),
    ("rows-2", icon::ROWS_2),
    ("x", icon::X),
    ("power", icon::POWER),
    ("terminal", icon::TERMINAL),
    ("menu", icon::MENU),
    ("arrow-left-right", icon::ARROW_LEFT_RIGHT),
    ("settings", icon::SETTINGS),
    ("plus", icon::PLUS),
    ("keyboard", icon::KEYBOARD),
    ("history", icon::HISTORY),
    ("panel-left", icon::PANEL_LEFT),
    ("panel-right", icon::PANEL_RIGHT),
    ("download", icon::DOWNLOAD),
    ("refresh-cw", icon::REFRESH_CW),
    ("copy", icon::COPY),
    ("trash", icon::TRASH),
    ("pencil", icon::PENCIL),
    ("maximize", icon::MAXIMIZE),
    ("minimize", icon::MINIMIZE),
    ("square", icon::SQUARE),
];

pub fn icon(ch: char, size: f32, color: Color32) -> RichText {
    RichText::new(ch.to_string())
        .font(egui::FontId::new(size, FontFamily::Name("icons".into())))
        .color(color)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use egui::{Align, Color32, FontFamily, FontSelection, Style};

    use super::{icon, ALL};

    #[test]
    fn every_icon_constant_exists_in_font() {
        let bytes = include_bytes!("../../assets/fonts/lucide-subset.ttf");
        let face = ttf_parser::Face::parse(bytes, 0).expect("Lucide subset must be a valid TTF");

        for &(name, codepoint) in ALL {
            assert!(
                face.glyph_index(codepoint).is_some(),
                "Lucide subset is missing {name} ({codepoint:?})"
            );
        }
    }

    #[test]
    fn icon_names_are_unique() {
        let mut names = HashSet::new();
        for &(name, _) in ALL {
            assert!(names.insert(name), "duplicate Lucide icon name: {name}");
        }
    }

    #[test]
    fn icon_uses_icons_family() {
        let mut job = egui::text::LayoutJob::default();
        icon(super::icon::SEARCH, 12.0, Color32::WHITE).append_to(
            &mut job,
            &Style::default(),
            FontSelection::Default,
            Align::Center,
        );

        assert_eq!(
            job.sections[0].format.font_id.family,
            FontFamily::Name("icons".into())
        );
    }
}
