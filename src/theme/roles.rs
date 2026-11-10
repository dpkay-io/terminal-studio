//! Surface, text, and accent roles derived from each palette.
//!
//! Nord is pinned to the spec. Every other theme uses the same recipe.

use egui::Color32;

use super::contrast_ratio;
use super::palettes::ThemeDef;
use super::ThemeId;

/// 14% of 255, rounded. Selected-row fill uses the accent at this alpha.
const ACCENT_BG_ALPHA: u8 = 36;
const CONTRAST_AA: f32 = 4.5;
const STEP: f32 = 0.02;
const MAX_STEPS: u8 = 50;

pub(super) struct Roles {
    pub(super) bg_base: Color32,
    pub(super) bg_panel: Color32,
    pub(super) bg_raised: Color32,
    pub(super) border: Color32,
    pub(super) fg: Color32,
    pub(super) fg_muted: Color32,
    pub(super) fg_muted_raised: Color32,
    pub(super) fg_strong: Color32,
    pub(super) accent: Color32,
    pub(super) accent_bg: Color32,
    pub(super) danger: Color32,
    pub(super) danger_text: Color32,
}

pub(super) fn derive_roles(def: &ThemeDef) -> Roles {
    if def.id == ThemeId::Nord {
        return nord_roles();
    }
    recipe(def)
}

fn nord_roles() -> Roles {
    let accent = rgb(0x88, 0xC0, 0xD0);
    Roles {
        bg_base: rgb(0x2E, 0x34, 0x40),
        bg_panel: rgb(0x29, 0x2E, 0x39),
        bg_raised: rgb(0x3B, 0x42, 0x52),
        border: rgb(0x3B, 0x42, 0x52),
        fg: rgb(0xD8, 0xDE, 0xE9),
        fg_muted: rgb(0x95, 0x9E, 0xB1),
        fg_strong: rgb(0xEC, 0xEF, 0xF4),
        fg_muted_raised: rgb(0xA7, 0xAF, 0xBF),
        accent,
        accent_bg: accent_bg(accent),
        danger: rgb(0xA5, 0x4B, 0x54),
        danger_text: Color32::WHITE,
    }
}

fn recipe(def: &ThemeDef) -> Roles {
    let bg_base = def.base;
    let bg_panel = blend(def.base, def.mantle, 0.5);
    let bg_raised = def.surface0;
    let border = def.surface0;
    let fg = if contrast_ratio(def.subtext1, bg_raised) >= CONTRAST_AA {
        def.subtext1
    } else {
        def.text
    };
    let fg_strong = def.text;
    let fg_muted = step_toward(def.subtext0, def.text, |color| {
        contrast_ratio(color, bg_base) >= CONTRAST_AA
            && contrast_ratio(color, bg_panel) >= CONTRAST_AA
    });
    let fg_muted_raised = step_toward(def.subtext0, def.text, |color| {
        contrast_ratio(color, bg_raised) >= CONTRAST_AA
    });
    let (danger, danger_text) = danger_pair(def);
    let accent = opaque(def.accent);
    Roles {
        bg_base: opaque(bg_base),
        bg_panel: opaque(bg_panel),
        bg_raised: opaque(bg_raised),
        border: opaque(border),
        fg: opaque(fg),
        fg_muted: opaque(fg_muted),
        fg_muted_raised: opaque(fg_muted_raised),
        fg_strong: opaque(fg_strong),
        accent,
        accent_bg: accent_bg(accent),
        danger: opaque(danger),
        danger_text,
    }
}

fn danger_pair(def: &ThemeDef) -> ([u8; 3], Color32) {
    let text = if def.is_light {
        def.base
    } else {
        [255, 255, 255]
    };
    let danger = step_toward(def.red, [0, 0, 0], |color| {
        contrast_ratio(text, color) >= CONTRAST_AA
    });
    (danger, opaque(text))
}

/// Blend `start` toward `target` in 2% steps until `passes`. The target is
/// the last resort after 50 steps (100%).
fn step_toward(start: [u8; 3], target: [u8; 3], passes: impl Fn([u8; 3]) -> bool) -> [u8; 3] {
    for step in 0..=MAX_STEPS {
        let candidate = blend(start, target, f32::from(step) * STEP);
        if passes(candidate) {
            return candidate;
        }
    }
    target
}

fn blend(base: [u8; 3], target: [u8; 3], factor: f32) -> [u8; 3] {
    let mix = |from: u8, to: u8| -> u8 {
        (f32::from(from) + (f32::from(to) - f32::from(from)) * factor).clamp(0.0, 255.0) as u8
    };
    [
        mix(base[0], target[0]),
        mix(base[1], target[1]),
        mix(base[2], target[2]),
    ]
}

fn opaque(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn accent_bg(accent: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), ACCENT_BG_ALPHA)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{hex, ratio, theme_by_id};
    use super::super::ThemeId;
    use egui::Color32;

    #[test]
    fn nord_roles_match_spec() {
        let t = theme_by_id(ThemeId::Nord);
        assert_eq!(t.bg_base, hex(0x2E3440));
        assert_eq!(t.bg_panel, hex(0x292E39));
        assert_eq!(t.bg_raised, hex(0x3B4252));
        assert_eq!(t.border, hex(0x3B4252));
        assert_eq!(t.fg, hex(0xD8DEE9));
        assert_eq!(t.fg_muted, hex(0x959EB1));
        assert_eq!(t.fg_strong, hex(0xECEFF4));
        assert_eq!(t.accent, hex(0x88C0D0));
        assert_eq!(t.danger, hex(0xA54B54));
        assert_eq!(t.fg_muted_raised, hex(0xA7AFBF));
        assert_eq!(
            t.accent_bg,
            Color32::from_rgba_unmultiplied(0x88, 0xC0, 0xD0, 36)
        );
    }

    #[test]
    fn every_theme_meets_contrast() {
        assert_eq!(ThemeId::ALL.len(), 16);
        for &id in ThemeId::ALL {
            let t = theme_by_id(id);
            let checks = [
                ("fg_muted on bg_base", ratio(t.fg_muted, t.bg_base)),
                ("fg_muted on bg_panel", ratio(t.fg_muted, t.bg_panel)),
                ("fg on bg_raised", ratio(t.fg, t.bg_raised)),
                ("danger_text on danger", ratio(t.danger_text, t.danger)),
                (
                    "fg_muted_raised on bg_raised",
                    ratio(t.fg_muted_raised, t.bg_raised),
                ),
            ];
            for (name, value) in checks {
                assert!(
                    value >= 4.5,
                    "{id:?} {name} contrast {value:.2} is below 4.5"
                );
            }
        }
    }

    #[test]
    fn signature_accents_match_spec_table() {
        let cases = [
            (ThemeId::CatppuccinMocha, 0x89B4FA),
            (ThemeId::Dracula, 0xBD93F9),
            (ThemeId::Nord, 0x88C0D0),
            (ThemeId::SolarizedDark, 0x268BD2),
            (ThemeId::GruvboxDark, 0x83A598),
            (ThemeId::TokyoNight, 0x7AA2F7),
            (ThemeId::RosePine, 0xC4A7E7),
            (ThemeId::OneDark, 0x61AFEF),
            (ThemeId::EverforestDark, 0x7FBBB3),
            (ThemeId::Monokai, 0x66D9EF),
            (ThemeId::GitHubDark, 0x58A6FF),
            (ThemeId::AyuDark, 0x59C2FF),
            (ThemeId::MaterialDarker, 0x82AAFF),
            (ThemeId::SolarizedLight, 0x268BD2),
            (ThemeId::CatppuccinLatte, 0x1E66F5),
            (ThemeId::GitHubLight, 0x0969DA),
        ];
        assert_eq!(cases.len(), ThemeId::ALL.len());
        for (id, rgb) in cases {
            assert_eq!(theme_by_id(id).accent, hex(rgb), "{id:?}");
        }
    }

    #[test]
    fn light_theme_panel_differs_from_base() {
        let mut saw_light = false;
        for &id in ThemeId::ALL {
            let t = theme_by_id(id);
            if !t.is_light {
                continue;
            }
            saw_light = true;
            assert_ne!(t.bg_panel, t.bg_base, "{id:?} bg_panel");
            assert_ne!(t.bg_raised, t.bg_base, "{id:?} bg_raised");
        }
        assert!(saw_light, "expected at least one light theme");
    }

    #[test]
    fn github_light_is_registered() {
        assert_eq!(ThemeId::ALL.len(), 16);
        assert_eq!(ThemeId::GitHubLight.name(), "GitHub Light");
    }

    #[test]
    fn step_toward_falls_back_to_target_when_no_candidate_passes() {
        let target = [200, 150, 100];
        assert_eq!(super::step_toward([0, 0, 0], target, |_| false), target);
    }
}
