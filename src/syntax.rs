use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;

use egui::text::LayoutJob;
use egui::{Color32, FontId, TextFormat, TextStyle};
use syntect::highlighting::{Color as SynColor, FontStyle, Style, Theme, ThemeItem, ThemeSettings};
use syntect::highlighting::{ScopeSelectors, StyleModifier};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use crate::theme;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn syn_color(c: Color32) -> SynColor {
    SynColor {
        r: c.r(),
        g: c.g(),
        b: c.b(),
        a: c.a(),
    }
}

fn to_egui(c: SynColor) -> Color32 {
    Color32::from_rgba_premultiplied(c.r, c.g, c.b, c.a)
}

fn build_theme(t: &theme::Theme) -> Theme {
    let fg = syn_color(t.text);
    let bg = syn_color(t.bg_term);

    let scope = |sel: &str, color: Color32| ThemeItem {
        scope: ScopeSelectors::from_str(sel).unwrap_or_default(),
        style: StyleModifier {
            foreground: Some(syn_color(color)),
            background: None,
            font_style: None,
        },
    };

    let scope_italic = |sel: &str, color: Color32| ThemeItem {
        scope: ScopeSelectors::from_str(sel).unwrap_or_default(),
        style: StyleModifier {
            foreground: Some(syn_color(color)),
            background: None,
            font_style: Some(FontStyle::ITALIC),
        },
    };

    Theme {
        name: Some(String::from("terminal-studio")),
        author: None,
        settings: ThemeSettings {
            foreground: Some(fg),
            background: Some(bg),
            ..Default::default()
        },
        scopes: vec![
            scope_italic("comment", t.overlay0),
            scope("keyword, storage.type, storage.modifier", t.mauve),
            scope(
                "keyword.control, keyword.operator.logical, keyword.operator.assignment",
                t.mauve,
            ),
            scope("constant.numeric, constant.language", t.teal),
            scope("string, string.quoted", t.green),
            scope("variable, variable.other", t.text),
            scope("variable.parameter, variable.other.readwrite", t.red),
            scope("entity.name.function, support.function", t.blue),
            scope("entity.name.type, support.type, support.class", t.yellow),
            scope("entity.name.tag", t.red),
            scope("entity.other.attribute-name", t.yellow),
            scope("meta.decorator, punctuation.decorator", t.teal),
            scope("punctuation, meta.brace", t.subtext0),
            scope("constant.character.escape", t.teal),
            scope("markup.heading", t.blue),
            scope("markup.bold", t.text),
            scope("markup.italic", t.text),
            scope("markup.raw, markup.inline.raw", t.green),
        ],
    }
}

pub fn find_syntax_for_file(path: &Path) -> Option<&'static SyntaxReference> {
    let ss = syntax_set();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let syn = ss.find_syntax_by_extension(ext);
        if syn.is_some() {
            return syn;
        }
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        return ss.find_syntax_by_extension(name);
    }
    None
}

pub fn find_syntax_for_language(lang: &str) -> Option<&'static SyntaxReference> {
    let ss = syntax_set();
    let lang_lower = lang.to_lowercase();
    ss.find_syntax_by_token(&lang_lower)
        .or_else(|| ss.find_syntax_by_extension(&lang_lower))
}

pub fn highlight_layout_job(
    ui: &egui::Ui,
    text: &str,
    syntax: &SyntaxReference,
    wrap_width: f32,
) -> LayoutJob {
    let t = theme::active();
    let syn_theme = build_theme(t);
    let ss = syntax_set();
    let h = syntect::highlighting::Highlighter::new(&syn_theme);
    let mut highlight_state =
        syntect::highlighting::HighlightState::new(&h, syntect::parsing::ScopeStack::new());
    let mut parse_state = syntect::parsing::ParseState::new(syntax);

    let font_id = FontId::monospace(ui.style().text_styles[&TextStyle::Monospace].size);
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;

    for line in LinesWithEndings::new(text) {
        let ops = parse_state.parse_line(line, ss).unwrap_or_default();
        let regions: Vec<(Style, &str)> =
            syntect::highlighting::HighlightIterator::new(&mut highlight_state, &ops, line, &h)
                .collect();

        for (style, segment) in regions {
            let raw_color = to_egui(style.foreground);
            let bg_rgb = [t.md_code_bg.r(), t.md_code_bg.g(), t.md_code_bg.b()];
            let color =
                theme::ensure_readable([raw_color.r(), raw_color.g(), raw_color.b()], bg_rgb);
            let mut format = TextFormat::simple(font_id.clone(), color);
            if style.font_style.contains(FontStyle::ITALIC) {
                format.italics = true;
            }
            if style.font_style.contains(FontStyle::UNDERLINE) {
                format.underline = egui::Stroke::new(1.0, color);
            }
            job.append(segment, 0.0, format);
        }
    }

    job
}

pub fn highlighted_lines(text: &str, syntax: &SyntaxReference) -> Vec<Vec<(Color32, String)>> {
    let t = theme::active();
    let syn_theme = build_theme(t);
    let ss = syntax_set();
    let h = syntect::highlighting::Highlighter::new(&syn_theme);
    let mut highlight_state =
        syntect::highlighting::HighlightState::new(&h, syntect::parsing::ScopeStack::new());
    let mut parse_state = syntect::parsing::ParseState::new(syntax);

    let mut result = Vec::new();
    for line in LinesWithEndings::new(text) {
        let ops = parse_state.parse_line(line, ss).unwrap_or_default();
        let regions: Vec<(Style, &str)> =
            syntect::highlighting::HighlightIterator::new(&mut highlight_state, &ops, line, &h)
                .collect();
        let spans: Vec<(Color32, String)> = regions
            .into_iter()
            .map(|(style, seg)| {
                (
                    to_egui(style.foreground),
                    seg.trim_end_matches(['\r', '\n']).to_string(),
                )
            })
            .collect();
        result.push(spans);
    }
    result
}

struct LinesWithEndings<'a> {
    remaining: &'a str,
}

impl<'a> LinesWithEndings<'a> {
    fn new(text: &'a str) -> Self {
        Self { remaining: text }
    }
}

impl<'a> Iterator for LinesWithEndings<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        if self.remaining.is_empty() {
            return None;
        }
        let end = self
            .remaining
            .find('\n')
            .map(|i| i + 1)
            .unwrap_or(self.remaining.len());
        let line = &self.remaining[..end];
        self.remaining = &self.remaining[end..];
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn find_syntax_by_rs_extension() {
        let path = PathBuf::from("test.rs");
        let syn = find_syntax_for_file(&path);
        assert!(syn.is_some());
        assert!(syn.unwrap().name.contains("Rust"));
    }

    #[test]
    fn find_syntax_by_py_extension() {
        let path = PathBuf::from("main.py");
        let syn = find_syntax_for_file(&path);
        assert!(syn.is_some());
        assert!(syn.unwrap().name.contains("Python"));
    }

    #[test]
    fn find_syntax_by_language_name() {
        let syn = find_syntax_for_language("rust");
        assert!(syn.is_some());
        assert!(syn.unwrap().name.contains("Rust"));
    }

    #[test]
    fn find_syntax_by_language_name_case_insensitive() {
        let syn = find_syntax_for_language("JavaScript");
        assert!(syn.is_some());
    }

    #[test]
    fn find_syntax_unknown_extension() {
        let path = PathBuf::from("file.xyzxyz");
        let syn = find_syntax_for_file(&path);
        assert!(syn.is_none());
    }

    #[test]
    fn lines_with_endings_basic() {
        let lines: Vec<&str> = LinesWithEndings::new("a\nb\nc").collect();
        assert_eq!(lines, vec!["a\n", "b\n", "c"]);
    }

    #[test]
    fn lines_with_endings_trailing_newline() {
        let lines: Vec<&str> = LinesWithEndings::new("a\n").collect();
        assert_eq!(lines, vec!["a\n"]);
    }

    #[test]
    fn lines_with_endings_empty() {
        let lines: Vec<&str> = LinesWithEndings::new("").collect();
        assert!(lines.is_empty());
    }

    #[test]
    fn highlighted_lines_produces_output() {
        let syn = find_syntax_for_language("rust").unwrap();
        let lines = highlighted_lines("fn main() {}\n", syn);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].is_empty());
    }

    #[test]
    fn highlighted_lines_multi_line() {
        let syn = find_syntax_for_language("python").unwrap();
        let code = "def hello():\n    print(\"hi\")\n";
        let lines = highlighted_lines(code, syn);
        assert_eq!(lines.len(), 2);
    }
}
