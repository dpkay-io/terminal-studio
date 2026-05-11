#![allow(dead_code)]
use egui::Color32;

// ── Catppuccin Mocha palette — raw [u8;3] for tinting, WCAG, storage ─────────

pub const CRUST_RGB: [u8; 3] = [17, 17, 27];
pub const MANTLE_RGB: [u8; 3] = [24, 24, 37];
pub const BASE_RGB: [u8; 3] = [30, 30, 46];
pub const SURFACE0_RGB: [u8; 3] = [49, 50, 68];
pub const SURFACE1_RGB: [u8; 3] = [69, 71, 90];
pub const SURFACE2_RGB: [u8; 3] = [88, 91, 112];
pub const OVERLAY0_RGB: [u8; 3] = [108, 112, 134];
pub const OVERLAY1_RGB: [u8; 3] = [127, 132, 156];
pub const TEXT_RGB: [u8; 3] = [205, 214, 244];
pub const SUBTEXT1_RGB: [u8; 3] = [184, 192, 224];
pub const SUBTEXT0_RGB: [u8; 3] = [166, 173, 200];
pub const BLUE_RGB: [u8; 3] = [137, 180, 250];
pub const GREEN_RGB: [u8; 3] = [166, 227, 161];
pub const RED_RGB: [u8; 3] = [243, 139, 168];
pub const YELLOW_RGB: [u8; 3] = [249, 226, 175];
pub const MAUVE_RGB: [u8; 3] = [203, 166, 247];
pub const TEAL_RGB: [u8; 3] = [148, 226, 213];

// ── Catppuccin Mocha palette — egui Color32 constants ────────────────────────

pub const CRUST: Color32 = Color32::from_rgb(17, 17, 27);
pub const MANTLE: Color32 = Color32::from_rgb(24, 24, 37);
pub const BASE: Color32 = Color32::from_rgb(30, 30, 46);
pub const SURFACE0: Color32 = Color32::from_rgb(49, 50, 68);
pub const SURFACE1: Color32 = Color32::from_rgb(69, 71, 90);
pub const SURFACE2: Color32 = Color32::from_rgb(88, 91, 112);
pub const OVERLAY0: Color32 = Color32::from_rgb(108, 112, 134);
pub const OVERLAY1: Color32 = Color32::from_rgb(127, 132, 156);
pub const TEXT: Color32 = Color32::from_rgb(205, 214, 244);
pub const SUBTEXT1: Color32 = Color32::from_rgb(184, 192, 224);
pub const SUBTEXT0: Color32 = Color32::from_rgb(166, 173, 200);
pub const BLUE: Color32 = Color32::from_rgb(137, 180, 250);
pub const GREEN: Color32 = Color32::from_rgb(166, 227, 161);
pub const RED: Color32 = Color32::from_rgb(243, 139, 168);
pub const YELLOW: Color32 = Color32::from_rgb(249, 226, 175);
pub const MAUVE: Color32 = Color32::from_rgb(203, 166, 247);
pub const TEAL: Color32 = Color32::from_rgb(148, 226, 213);

// ── Semantic background aliases ───────────────────────────────────────────────

pub const BG_PANEL_FILL: Color32 = MANTLE;
pub const BG_WORKSPACE_FILL: Color32 = Color32::from_rgb(28, 30, 45);
pub const BG_TERM: Color32 = BASE;
pub const BG_ROW_ACTIVE: Color32 = SURFACE0;
pub const BG_ROW_HOVER: Color32 = Color32::from_rgb(40, 44, 58);

// ── Semantic foreground aliases ───────────────────────────────────────────────

pub const FG_PRIMARY: Color32 = TEXT;
pub const FG_SECONDARY: Color32 = SUBTEXT0;
pub const FG_MUTED: Color32 = OVERLAY0;
pub const FG_PATH: Color32 = BLUE;
pub const FG_DIR_ENTRY: Color32 = Color32::from_rgb(130, 170, 210);
pub const FG_MD_FILE: Color32 = GREEN;
pub const FG_OTHER_FILE: Color32 = SUBTEXT1;

// ── Interactive element colors ────────────────────────────────────────────────

pub const DANGER_BG: Color32 = Color32::from_rgb(180, 60, 60);
pub const DANGER_FG: Color32 = Color32::from_rgb(210, 180, 180);
pub const SPLIT_HOVER_BG: Color32 = Color32::from_rgb(40, 60, 110);
pub const DIVIDER_IDLE: Color32 = Color32::from_rgb(55, 58, 75);
pub const DIVIDER_ACTIVE: Color32 = Color32::from_rgb(100, 120, 180);
pub const WS_DIV_IDLE: Color32 = Color32::from_rgb(40, 44, 58);
pub const WS_DIV_ACTIVE: Color32 = Color32::from_rgb(90, 95, 120);

// ── Git diff colors ───────────────────────────────────────────────────────────

pub const GIT_ADDED: Color32 = Color32::from_rgb(80, 210, 100);
pub const GIT_REMOVED: Color32 = Color32::from_rgb(220, 80, 80);
pub const GIT_MODIFIED: Color32 = YELLOW;
pub const GIT_RENAMED: Color32 = MAUVE;
pub const GIT_UNTRACKED: Color32 = OVERLAY0;
pub const GIT_HUNK: Color32 = Color32::from_rgb(80, 160, 200);
pub const GIT_HEADER: Color32 = Color32::from_rgb(210, 210, 80);
pub const GIT_FILENAME: Color32 = Color32::from_rgb(130, 180, 230);

// ── Markdown colors ───────────────────────────────────────────────────────────

pub const MD_CODE: Color32 = GREEN;
pub const MD_CODE_BG: Color32 = Color32::from_rgb(40, 42, 58);
pub const MD_CODE_BORDER: Color32 = SURFACE1;
pub const MD_BULLET: Color32 = Color32::from_rgb(150, 200, 150);
pub const MD_BLOCKQUOTE: Color32 = Color32::from_rgb(160, 180, 200);

// ── Layout size constants ─────────────────────────────────────────────────────

pub const TITLEBAR_H: f32 = 32.0;
pub const TITLEBAR_BTN_W: f32 = 44.0;
pub const HEADER_H: f32 = 28.0;
pub const DIVIDER_W: f32 = 4.0;
pub const MIN_PANE_W: f32 = 80.0;
pub const BTN_W: f32 = 24.0;
pub const QUIT_W: f32 = 24.0;
pub const SESSION_ROW_H: f32 = 28.0; // unified with HEADER_H
pub const WS_BORDER_W: f32 = 2.0;
pub const LEFT_SIDEBAR_W: f32 = 200.0;
pub const BAR_PAD_X: f32 = 6.0; // consistent horizontal padding inside all bars
pub const SESSION_FONT_SZ: f32 = 12.5; // unified bar font
pub const HEADER_FONT_SZ: f32 = 12.5; // unified bar font
pub const STATUS_FONT_SZ: f32 = 12.5; // unified
pub const DIFF_FONT_SZ: f32 = 11.0;
pub const CWD_FONT_SZ: f32 = 11.5;

// ── Helper functions ──────────────────────────────────────────────────────────

pub fn from_rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Base-blend tinting: interpolates from BASE colour toward `c` by `factor`.
/// factor=1.0 → pure c; factor=0.0 → BASE colour.
/// Better than pure multiplication which produces very dark results at low factors.
pub fn tinted(c: [u8; 3], factor: f32) -> [u8; 3] {
    let base = BASE_RGB;
    [
        (base[0] as f32 + (c[0] as f32 - base[0] as f32) * factor).clamp(0.0, 255.0) as u8,
        (base[1] as f32 + (c[1] as f32 - base[1] as f32) * factor).clamp(0.0, 255.0) as u8,
        (base[2] as f32 + (c[2] as f32 - base[2] as f32) * factor).clamp(0.0, 255.0) as u8,
    ]
}

/// WCAG luminance check — returns TEXT (off-white) for dark backgrounds, BLACK for light.
pub fn text_on(bg: [u8; 3]) -> Color32 {
    let [r, g, b] = bg;
    let to_lin = |c: u8| -> f32 {
        let f = c as f32 / 255.0;
        if f <= 0.04045 {
            f / 12.92
        } else {
            ((f + 0.055) / 1.055).powf(2.4)
        }
    };
    let lum = 0.2126 * to_lin(r) + 0.7152 * to_lin(g) + 0.0722 * to_lin(b);
    if lum < 0.179 {
        TEXT
    } else {
        Color32::BLACK
    }
}

/// Pane header background given optional workspace colour and active state.
pub fn header_bg(ws_color: Option<[u8; 3]>, is_active: bool) -> Color32 {
    match (ws_color, is_active) {
        (Some(c), true) => from_rgb(tinted(c, 0.75)),
        (Some(c), false) => from_rgb(tinted(c, 0.35)),
        (None, true) => SURFACE0,
        (None, false) => Color32::from_rgb(37, 38, 54),
    }
}

/// Shorten a path to show the last 2 components with a leading `…/`.
pub fn short_path(p: &std::path::Path) -> String {
    let parts: Vec<&str> = p
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    if parts.len() <= 2 {
        p.display().to_string()
    } else {
        format!("…/{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    }
}

/// Render a line of text with inline **bold** support.
/// Splits on `**` markers; odd-indexed segments are rendered bold.
pub fn render_inline(ui: &mut egui::Ui, line: &str) {
    let parts: Vec<&str> = line.split("**").collect();
    if parts.len() < 3 {
        ui.label(line);
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, part) in parts.iter().enumerate() {
            if part.is_empty() {
                continue;
            }
            if i % 2 == 1 {
                ui.label(egui::RichText::new(*part).strong());
            } else {
                ui.label(*part);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_tinted_at_zero_returns_base() {
        let result = tinted([255, 0, 0], 0.0);
        assert_eq!(result, BASE_RGB);
    }

    #[test]
    fn test_tinted_at_one_returns_color() {
        let c = [200u8, 100, 50];
        let result = tinted(c, 1.0);
        assert_eq!(result, c);
    }

    #[test]
    fn test_tinted_midpoint() {
        // At 0.5 each channel should be halfway between BASE_RGB and c
        let c = [100u8, 100, 100];
        let result = tinted(c, 0.5);
        let expected = [
            (BASE_RGB[0] as f32 + (100.0 - BASE_RGB[0] as f32) * 0.5) as u8,
            (BASE_RGB[1] as f32 + (100.0 - BASE_RGB[1] as f32) * 0.5) as u8,
            (BASE_RGB[2] as f32 + (100.0 - BASE_RGB[2] as f32) * 0.5) as u8,
        ];
        assert_eq!(result, expected);
    }

    #[test]
    fn test_text_on_dark_bg_returns_light_text() {
        // Very dark background → should return TEXT (off-white)
        let color = text_on([10, 10, 10]);
        assert_eq!(color, TEXT);
    }

    #[test]
    fn test_text_on_light_bg_returns_black() {
        // Very light background → should return BLACK
        let color = text_on([240, 240, 240]);
        assert_eq!(color, Color32::BLACK);
    }

    #[test]
    fn test_short_path_two_or_fewer_components() {
        let p = PathBuf::from("foo/bar");
        assert_eq!(short_path(&p), "foo/bar");
    }

    #[test]
    fn test_short_path_abbreviates_long_paths() {
        let p = PathBuf::from("/home/user/projects/myapp/src");
        let s = short_path(&p);
        assert!(s.starts_with('\u{2026}'), "should start with ellipsis: {s}");
        assert!(s.contains("myapp"), "should contain second-to-last: {s}");
        assert!(s.ends_with("src"), "should end with last component: {s}");
    }

    #[test]
    fn test_from_rgb_roundtrip() {
        let c = [137u8, 180, 250];
        let color = from_rgb(c);
        assert_eq!(color.r(), 137);
        assert_eq!(color.g(), 180);
        assert_eq!(color.b(), 250);
    }

    #[test]
    fn test_header_bg_active_with_color() {
        let c = Some([100u8, 140, 230]);
        let active = header_bg(c, true);
        let inactive = header_bg(c, false);
        // Active should be brighter (higher tint factor)
        // Just check they differ and don't panic
        assert_ne!(active, inactive);
    }

    #[test]
    fn test_header_bg_no_color() {
        let active = header_bg(None, true);
        let inactive = header_bg(None, false);
        assert_eq!(active, SURFACE0);
        assert_ne!(active, inactive);
    }
}
