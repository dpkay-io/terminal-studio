use egui::{FontData, FontDefinitions, FontFamily};

pub const UI_BOLD: &str = "ui_bold";

const INTER: &str = "Inter";
const INTER_SEMIBOLD: &str = "Inter-SemiBold";
const JETBRAINS_MONO: &str = "JetBrainsMono";
const JETBRAINS_MONO_BOLD: &str = "JetBrainsMono-Bold";
const LUCIDE: &str = "lucide";
pub const MONO_BOLD: &str = "mono_bold";
const ICONS: &str = "icons";
const SYMBOL_FALLBACK: &str = "symbol_fallback";

pub fn ui_bold() -> FontFamily {
    FontFamily::Name(UI_BOLD.into())
}

/// Build the complete font configuration without touching the filesystem.
///
/// Keeping this pure makes the fallback order testable and ensures font data
/// is assembled only once, when [`install_fonts`] is called at startup.
pub(crate) fn font_definitions(symbol: Option<Vec<u8>>) -> FontDefinitions {
    let mut definitions = FontDefinitions::default();
    let default_proportional = definitions
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let default_monospace = definitions
        .families
        .get(&FontFamily::Monospace)
        .cloned()
        .unwrap_or_default();

    definitions.font_data.insert(
        INTER.to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf")),
    );
    definitions.font_data.insert(
        INTER_SEMIBOLD.to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf")),
    );
    definitions.font_data.insert(
        JETBRAINS_MONO.to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")),
    );
    definitions.font_data.insert(
        JETBRAINS_MONO_BOLD.to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf")),
    );
    definitions.font_data.insert(
        LUCIDE.to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/lucide-subset.ttf")),
    );

    let symbol_name = symbol.map(|bytes| {
        definitions
            .font_data
            .insert(SYMBOL_FALLBACK.to_owned(), FontData::from_owned(bytes));
        SYMBOL_FALLBACK.to_owned()
    });

    let with_fallbacks =
        |primary: &str, symbol: Option<&String>, defaults: &[String]| -> Vec<String> {
            std::iter::once(primary.to_owned())
                .chain(symbol.cloned())
                .chain(defaults.iter().cloned())
                .collect()
        };

    let proportional = with_fallbacks(INTER, symbol_name.as_ref(), default_proportional.as_slice());
    let monospace = with_fallbacks(
        JETBRAINS_MONO,
        symbol_name.as_ref(),
        default_monospace.as_slice(),
    );
    let mono_bold = with_fallbacks(
        JETBRAINS_MONO_BOLD,
        symbol_name.as_ref(),
        default_monospace.as_slice(),
    );

    definitions
        .families
        .insert(FontFamily::Proportional, proportional.clone());
    definitions
        .families
        .insert(FontFamily::Monospace, monospace);
    definitions
        .families
        .insert(FontFamily::Name(UI_BOLD.into()), {
            let mut chain = vec![INTER_SEMIBOLD.to_owned()];
            chain.extend(proportional);
            chain
        });
    definitions
        .families
        .insert(FontFamily::Name(MONO_BOLD.into()), mono_bold);
    definitions.families.insert(
        FontFamily::Name(ICONS.into()),
        vec![LUCIDE.to_owned(), INTER.to_owned()],
    );

    definitions
}

pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions(load_symbol_font()));
}

fn load_symbol_font() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        let windows_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_owned());
        return std::fs::read(
            std::path::Path::new(&windows_root)
                .join("Fonts")
                .join("seguisym.ttf"),
        )
        .ok();
    }

    #[cfg(target_os = "linux")]
    {
        const CANDIDATES: &[&str] = &[
            "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
            "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
            "/usr/share/fonts/google-noto/NotoSansSymbols2-Regular.ttf",
            "/usr/share/fonts/TTF/NotoSansSymbols2-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        ];
        return CANDIDATES.iter().find_map(|path| std::fs::read(path).ok());
    }

    #[cfg(target_os = "macos")]
    {
        return std::fs::read("/System/Library/Fonts/Apple Symbols.ttf").ok();
    }

    #[allow(unreachable_code)]
    None
}

#[cfg(test)]
mod tests {
    use egui::FontFamily;

    use super::font_definitions;

    fn family(defs: &egui::FontDefinitions, family: FontFamily) -> Vec<String> {
        defs.families.get(&family).cloned().unwrap_or_default()
    }

    #[test]
    fn proportional_starts_with_inter() {
        let defs = font_definitions(None);
        let names = family(&defs, FontFamily::Proportional);
        assert!(
            names[0].eq_ignore_ascii_case("inter")
                || names[0].to_ascii_lowercase().starts_with("inter"),
            "proportional chain starts with {names:?}"
        );
    }

    #[test]
    fn monospace_starts_with_jetbrains() {
        let defs = font_definitions(None);
        let names = family(&defs, FontFamily::Monospace);
        assert!(
            names[0].to_ascii_lowercase().starts_with("jetbrains"),
            "monospace chain starts with {names:?}"
        );
    }

    #[test]
    fn symbol_fallback_is_last_when_present() {
        // P5 order: primary face, then symbol_fallback, then egui defaults.
        // symbol_fallback is the last font this crate inserts; defaults follow it.
        let defs = font_definitions(Some(b"symbol-bytes".to_vec()));
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            let names = family(&defs, fam.clone());
            assert_eq!(
                names.get(1).map(String::as_str),
                Some("symbol_fallback"),
                "{fam:?} {names:?}"
            );
            assert!(
                names.len() > 2,
                "{fam:?} must keep egui defaults after symbol_fallback"
            );
        }
        let mono_bold = family(&defs, FontFamily::Name("mono_bold".into()));
        assert_eq!(
            mono_bold.get(1).map(String::as_str),
            Some("symbol_fallback")
        );
    }

    #[test]
    fn no_symbol_font_does_not_register_symbol_fallback() {
        let defs = font_definitions(None);
        assert!(!defs.font_data.contains_key("symbol_fallback"));
        for names in defs.families.values() {
            assert!(
                !names.iter().any(|n| n == "symbol_fallback"),
                "symbol_fallback leaked into {names:?}"
            );
        }
    }

    #[test]
    fn egui_default_fonts_kept_as_fallback() {
        let defs = font_definitions(None);
        let proportional = family(&defs, FontFamily::Proportional);
        let monospace = family(&defs, FontFamily::Monospace);
        for required in ["Ubuntu-Light", "NotoEmoji-Regular", "emoji-icon-font"] {
            assert!(
                proportional.iter().any(|n| n == required),
                "proportional missing {required}: {proportional:?}"
            );
        }
        for required in [
            "Hack",
            "Ubuntu-Light",
            "NotoEmoji-Regular",
            "emoji-icon-font",
        ] {
            assert!(
                monospace.iter().any(|n| n == required),
                "monospace missing {required}: {monospace:?}"
            );
        }
        assert!(
            proportional
                .iter()
                .position(|n| n == "Ubuntu-Light")
                .unwrap()
                > 0
        );
        assert!(monospace.iter().position(|n| n == "Hack").unwrap() > 0);
    }

    #[test]
    fn every_named_family_is_registered() {
        let defs = font_definitions(None);
        for name in ["ui_bold", "mono_bold", "icons"] {
            let names = family(&defs, FontFamily::Name(name.into()));
            assert!(!names.is_empty(), "{name} is not registered");
        }
        assert_eq!(
            family(&defs, FontFamily::Name("icons".into())),
            vec!["lucide".to_owned(), "Inter".to_owned()]
        );
        for family_name in [FontFamily::Proportional, FontFamily::Monospace] {
            assert!(
                !family(&defs, family_name.clone())
                    .iter()
                    .any(|name| name == "lucide"),
                "lucide leaked into {family_name:?}"
            );
        }
        assert!(
            family(&defs, FontFamily::Name("ui_bold".into()))[0]
                .to_ascii_lowercase()
                .contains("semibold")
                || family(&defs, FontFamily::Name("ui_bold".into()))[0]
                    .to_ascii_lowercase()
                    .contains("semi")
        );
        assert!(family(&defs, FontFamily::Name("mono_bold".into()))[0]
            .to_ascii_lowercase()
            .contains("bold"));
        assert_eq!(super::UI_BOLD, "ui_bold");
        assert_eq!(super::ui_bold(), FontFamily::Name("ui_bold".into()));
    }

    #[test]
    fn font_bytes_are_valid_ttf() {
        let defs = font_definitions(None);
        for key in [
            "Inter",
            "Inter-SemiBold",
            "JetBrainsMono",
            "JetBrainsMono-Bold",
            "lucide",
        ] {
            let data = defs
                .font_data
                .get(key)
                .unwrap_or_else(|| panic!("missing font data {key}"));
            ttf_parser::Face::parse(data.font.as_ref(), 0)
                .unwrap_or_else(|err| panic!("{key} is not a valid TTF: {err}"));
        }
    }
}
