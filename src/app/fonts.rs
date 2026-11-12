//! The fonts the UI and terminal draw with: egui's bundled fonts, then the
//! system fonts that cover what those lack (symbols, dingbats such as ✓ and
//! ✕, CJK and Hangul), so such characters render instead of as boxes.

use std::path::PathBuf;
use std::sync::OnceLock;

use egui::{FontData, FontDefinitions, FontFamily};

/// One fallback font: its key in egui's font table and the places it may be
/// installed on this OS (the first that exists is used).
struct FallbackFont {
    name: &'static str,
    candidates: Vec<PathBuf>,
}

impl FallbackFont {
    fn new(name: &'static str, candidates: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            name,
            candidates: candidates.into_iter().collect(),
        }
    }
}

#[cfg(target_os = "macos")]
fn fallback_fonts() -> Vec<FallbackFont> {
    const FONTS: &str = "/System/Library/Fonts";
    let at = |file: &str| [PathBuf::from(FONTS).join(file)];
    vec![
        FallbackFont::new("symbol_fallback", at("Apple Symbols.ttf")),
        // Dingbats (✓ ✕ ✗ ✘) and technical symbols missing from the above.
        FallbackFont::new("menlo_fallback", at("Menlo.ttc")),
        // Broad coverage, including CJK ideographs, kana and Hangul.
        FallbackFont::new("unicode_fallback", at("Supplemental/Arial Unicode.ttf")),
    ]
}

#[cfg(target_os = "windows")]
fn fallback_fonts() -> Vec<FallbackFont> {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    let fonts = PathBuf::from(root).join("Fonts");
    let at = |file: &str| [fonts.join(file)];
    vec![
        FallbackFont::new("symbol_fallback", at("seguisym.ttf")),
        // CJK ideographs and kana (Microsoft YaHei).
        FallbackFont::new("cjk_fallback", at("msyh.ttc")),
        // Hangul (Malgun Gothic).
        FallbackFont::new("hangul_fallback", at("malgun.ttf")),
    ]
}

#[cfg(target_os = "linux")]
fn fallback_fonts() -> Vec<FallbackFont> {
    /// Where distributions install Noto fonts (Debian/Ubuntu, Fedora, Arch).
    const NOTO_DIRS: [&str; 7] = [
        "/usr/share/fonts/truetype/noto",
        "/usr/share/fonts/opentype/noto",
        "/usr/share/fonts/noto",
        "/usr/share/fonts/noto-cjk",
        "/usr/share/fonts/google-noto",
        "/usr/share/fonts/google-noto-cjk",
        "/usr/share/fonts/TTF",
    ];
    /// Where distributions install DejaVu.
    const DEJAVU_DIRS: [&str; 3] = [
        "/usr/share/fonts/truetype/dejavu",
        "/usr/share/fonts/dejavu",
        "/usr/share/fonts/TTF",
    ];
    let within = |dirs: &[&str], file: &str| {
        dirs.iter()
            .map(|dir| PathBuf::from(dir).join(file))
            .collect::<Vec<_>>()
    };
    vec![
        FallbackFont::new(
            "symbol_fallback",
            within(&NOTO_DIRS, "NotoSansSymbols2-Regular.ttf"),
        ),
        FallbackFont::new("dejavu_fallback", within(&DEJAVU_DIRS, "DejaVuSans.ttf")),
        // CJK ideographs, kana and Hangul.
        FallbackFont::new(
            "cjk_fallback",
            within(&NOTO_DIRS, "NotoSansCJK-Regular.ttc"),
        ),
    ]
}

/// The first readable candidate of each fallback font, memory-mapped once for
/// the life of the process: fonts like Arial Unicode are tens of MB, and a
/// mapping only pages in the glyph tables actually drawn.
fn loaded_fallbacks() -> &'static [(&'static str, &'static [u8])] {
    static LOADED: OnceLock<Vec<(&'static str, &'static [u8])>> = OnceLock::new();
    LOADED.get_or_init(|| {
        fallback_fonts()
            .into_iter()
            .filter_map(|font| {
                let bytes = font.candidates.iter().find_map(|path| map_font(path))?;
                Some((font.name, bytes))
            })
            .collect()
    })
}

/// Maps a font file read-only. The mapping is intentionally never unmapped:
/// egui borrows the bytes until the process exits, and `loaded_fallbacks`
/// maps each file at most once.
fn map_font(path: &std::path::Path) -> Option<&'static [u8]> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: system font files are not modified in place while mapped
    // (package managers and OS updates replace them by rename or snapshot).
    let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
    if map.is_empty() {
        return None;
    }
    let map: &'static memmap2::Mmap = Box::leak(Box::new(map));
    Some(&map[..])
}

/// egui's defaults with every installed fallback font appended to each
/// family, after egui's own fonts: a glyph egui's fonts have keeps its look;
/// only characters they lack come from the system fonts.
pub(super) fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in loaded_fallbacks() {
        fonts
            .font_data
            .insert((*name).to_owned(), FontData::from_static(bytes));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push((*name).to_owned());
        }
    }
    fonts
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAMILIES: [FontFamily; 2] = [FontFamily::Proportional, FontFamily::Monospace];

    #[test]
    fn egui_fonts_stay_first_in_every_family() {
        let defaults = FontDefinitions::default();
        let ours = definitions();
        for family in FAMILIES {
            let theirs = &defaults.families[&family];
            assert_eq!(&ours.families[&family][..theirs.len()], &theirs[..]);
        }
    }

    #[test]
    fn every_fallback_font_is_registered_in_every_family() {
        let ours = definitions();
        for (name, _) in loaded_fallbacks() {
            assert!(ours.font_data.contains_key(*name), "{name}");
            for family in FAMILIES {
                assert!(ours.families[&family].iter().any(|n| n == name), "{name}");
            }
        }
    }

    #[test]
    fn fonts_are_mapped_once() {
        let first = loaded_fallbacks().as_ptr();
        let _ = definitions();
        assert_eq!(loaded_fallbacks().as_ptr(), first);
    }

    #[test]
    fn a_missing_or_empty_font_file_is_skipped() {
        let dir = std::env::temp_dir().join(format!("ts-font-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("empty.ttf");
        std::fs::write(&empty, b"").unwrap();
        assert!(map_font(&dir.join("absent.ttf")).is_none());
        assert!(map_font(&empty).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Glyph checks against the real system fonts. Required on macOS, whose
    /// system fonts are fixed by the OS; opt-in elsewhere (`cargo test --
    /// --ignored glyph_coverage`), where they depend on which font packages
    /// are installed.
    mod glyph_coverage {
        use super::*;

        const PROBE_SIZE: f32 = 14.0;

        /// A context whose font atlas is built from `definitions()`.
        fn context_with_app_fonts() -> egui::Context {
            let ctx = egui::Context::default();
            ctx.set_fonts(definitions());
            let _ = ctx.run(egui::RawInput::default(), |_| {});
            ctx
        }

        /// The characters of `text` (whitespace aside) no font in `family` has.
        fn missing_glyphs(ctx: &egui::Context, family: &FontFamily, text: &str) -> Vec<char> {
            let id = egui::FontId::new(PROBE_SIZE, family.clone());
            ctx.fonts(|f| {
                text.chars()
                    .filter(|c| !c.is_whitespace() && !f.has_glyph(&id, *c))
                    .collect()
            })
        }

        /// Symbols the UI draws and that terminal programs commonly print
        /// (test runners' ✓/✗, prompts' ⎇, key glyphs), plus CJK and Hangul.
        #[test]
        #[cfg_attr(
            not(target_os = "macos"),
            ignore = "depends on installed font packages"
        )]
        fn common_symbols_and_cjk_render() {
            const COMMON: &str =
                "✓✕✗✘✔✖⎇⌘⌥⇧⌃⏎…•·→←↑↓▶▼●○■□★⚠⚙ℹ═│┌┐└┘├┤┬┴┼█▌▐░▒▓⠿日本語中文한국어かなカナ";
            let ctx = context_with_app_fonts();
            for family in FAMILIES {
                assert_eq!(
                    missing_glyphs(&ctx, &family, COMMON),
                    Vec::<char>::new(),
                    "{family:?}"
                );
            }
        }

        /// Every non-ASCII character in the source's string literals (UI
        /// labels, icons, test text) has a glyph, so a newly added symbol
        /// that would draw as a box fails here.
        #[test]
        #[cfg_attr(
            not(target_os = "macos"),
            ignore = "depends on installed font packages"
        )]
        fn every_symbol_in_the_source_renders() {
            let mut used = String::new();
            let mut dirs = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
            while let Some(dir) = dirs.pop() {
                for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        dirs.push(path);
                    } else if path.extension().is_some_and(|e| e == "rs") {
                        used.extend(non_ascii_in_string_literals(
                            &std::fs::read_to_string(&path).unwrap(),
                        ));
                    }
                }
            }
            assert!(!used.is_empty());
            let ctx = context_with_app_fonts();
            for family in FAMILIES {
                assert_eq!(
                    missing_glyphs(&ctx, &family, &used),
                    Vec::<char>::new(),
                    "{family:?}"
                );
            }
        }

        /// Non-ASCII characters inside `"..."` literals, skipping `//`
        /// comment lines.
        fn non_ascii_in_string_literals(source: &str) -> Vec<char> {
            let mut found = Vec::new();
            for line in source.lines() {
                if line.trim_start().starts_with("//") {
                    continue;
                }
                let mut in_string = false;
                let mut escaped = false;
                for c in line.chars() {
                    match (in_string, escaped, c) {
                        (true, true, _) => escaped = false,
                        (true, false, '\\') => escaped = true,
                        (_, false, '"') => in_string = !in_string,
                        (true, false, c) if !c.is_ascii() => found.push(c),
                        _ => {}
                    }
                }
            }
            found
        }

        #[test]
        fn literal_scan_finds_only_characters_inside_strings() {
            let source = "let a = \"✓ ok\"; // ✗ trailing\n// ★ comment line\nlet b = \"\\\"✕\";";
            assert_eq!(non_ascii_in_string_literals(source), vec!['✓', '✕']);
        }
    }
}
