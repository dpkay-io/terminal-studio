#![allow(dead_code)]
use egui::Color32;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

// ── Theme ID enum ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ThemeId {
    CatppuccinMocha,
    Dracula,
    Nord,
    SolarizedDark,
    GruvboxDark,
    TokyoNight,
    RosePine,
    OneDark,
    EverforestDark,
    Monokai,
    GitHubDark,
    AyuDark,
    MaterialDarker,
    SolarizedLight,
    CatppuccinLatte,
}

impl ThemeId {
    pub const ALL: &'static [ThemeId] = &[
        ThemeId::CatppuccinMocha,
        ThemeId::Dracula,
        ThemeId::Nord,
        ThemeId::SolarizedDark,
        ThemeId::GruvboxDark,
        ThemeId::TokyoNight,
        ThemeId::RosePine,
        ThemeId::OneDark,
        ThemeId::EverforestDark,
        ThemeId::Monokai,
        ThemeId::GitHubDark,
        ThemeId::AyuDark,
        ThemeId::MaterialDarker,
        ThemeId::SolarizedLight,
        ThemeId::CatppuccinLatte,
    ];

    pub fn index(self) -> usize {
        ThemeId::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::CatppuccinMocha => "Catppuccin Mocha",
            ThemeId::Dracula => "Dracula",
            ThemeId::Nord => "Nord",
            ThemeId::SolarizedDark => "Solarized Dark",
            ThemeId::GruvboxDark => "Gruvbox Dark",
            ThemeId::TokyoNight => "Tokyo Night",
            ThemeId::RosePine => "Rosé Pine",
            ThemeId::OneDark => "One Dark",
            ThemeId::EverforestDark => "Everforest Dark",
            ThemeId::Monokai => "Monokai",
            ThemeId::GitHubDark => "GitHub Dark",
            ThemeId::AyuDark => "Ayu Dark",
            ThemeId::MaterialDarker => "Material Darker",
            ThemeId::SolarizedLight => "Solarized Light",
            ThemeId::CatppuccinLatte => "Catppuccin Latte",
        }
    }
}

// ── Theme definition (minimal spec per theme) ────────────────────────────────

struct ThemeDef {
    id: ThemeId,
    is_light: bool,
    // Base palette (12 colors)
    crust: [u8; 3],
    mantle: [u8; 3],
    base: [u8; 3],
    surface0: [u8; 3],
    surface1: [u8; 3],
    surface2: [u8; 3],
    overlay0: [u8; 3],
    overlay1: [u8; 3],
    subtext0: [u8; 3],
    subtext1: [u8; 3],
    text: [u8; 3],
    // Accent colors (6)
    blue: [u8; 3],
    green: [u8; 3],
    red: [u8; 3],
    yellow: [u8; 3],
    mauve: [u8; 3],
    teal: [u8; 3],
    // ANSI 16 colors
    ansi: [[u8; 3]; 16],
}

// ── Expanded theme (ready for UI consumption) ────────────────────────────────

pub struct Theme {
    pub id: ThemeId,
    pub is_light: bool,

    // Raw RGB values
    pub crust_rgb: [u8; 3],
    pub mantle_rgb: [u8; 3],
    pub base_rgb: [u8; 3],
    pub surface0_rgb: [u8; 3],
    pub surface1_rgb: [u8; 3],
    pub surface2_rgb: [u8; 3],
    pub overlay0_rgb: [u8; 3],
    pub overlay1_rgb: [u8; 3],
    pub subtext0_rgb: [u8; 3],
    pub subtext1_rgb: [u8; 3],
    pub text_rgb: [u8; 3],
    pub blue_rgb: [u8; 3],
    pub green_rgb: [u8; 3],
    pub red_rgb: [u8; 3],
    pub yellow_rgb: [u8; 3],
    pub mauve_rgb: [u8; 3],
    pub teal_rgb: [u8; 3],

    // Color32 palette
    pub crust: Color32,
    pub mantle: Color32,
    pub base: Color32,
    pub surface0: Color32,
    pub surface1: Color32,
    pub surface2: Color32,
    pub overlay0: Color32,
    pub overlay1: Color32,
    pub subtext0: Color32,
    pub subtext1: Color32,
    pub text: Color32,
    pub blue: Color32,
    pub green: Color32,
    pub red: Color32,
    pub yellow: Color32,
    pub mauve: Color32,
    pub teal: Color32,

    // Semantic backgrounds
    pub bg_panel_fill: Color32,
    pub bg_workspace_fill: Color32,
    pub bg_term: Color32,
    pub bg_row_active: Color32,
    pub bg_row_hover: Color32,

    // Semantic foregrounds
    pub fg_primary: Color32,
    pub fg_secondary: Color32,
    pub fg_muted: Color32,
    pub fg_path: Color32,
    pub fg_dir_entry: Color32,
    pub fg_md_file: Color32,
    pub fg_other_file: Color32,

    // Interactive
    pub danger_bg: Color32,
    pub danger_fg: Color32,
    pub split_hover_bg: Color32,
    pub divider_idle: Color32,
    pub divider_active: Color32,
    pub ws_div_idle: Color32,
    pub ws_div_active: Color32,

    // Git
    pub git_added: Color32,
    pub git_removed: Color32,
    pub git_modified: Color32,
    pub git_renamed: Color32,
    pub git_untracked: Color32,
    pub git_hunk: Color32,
    pub git_header: Color32,
    pub git_filename: Color32,

    // Markdown
    pub md_code: Color32,
    pub md_code_bg: Color32,
    pub md_code_border: Color32,
    pub md_bullet: Color32,
    pub md_blockquote: Color32,

    // Terminal
    pub ansi: [Color32; 16],
    pub cursor_color: Color32,
    pub cursor_dim_color: Color32,
    pub selection_bg: Color32,
    pub scrollbar_color: Color32,
}

impl ThemeDef {
    fn build(&self) -> Theme {
        let c = |rgb: [u8; 3]| Color32::from_rgb(rgb[0], rgb[1], rgb[2]);

        let blend = |base: [u8; 3], target: [u8; 3], factor: f32| -> [u8; 3] {
            [
                (base[0] as f32 + (target[0] as f32 - base[0] as f32) * factor).clamp(0.0, 255.0) as u8,
                (base[1] as f32 + (target[1] as f32 - base[1] as f32) * factor).clamp(0.0, 255.0) as u8,
                (base[2] as f32 + (target[2] as f32 - base[2] as f32) * factor).clamp(0.0, 255.0) as u8,
            ]
        };

        let bg_workspace_fill = blend(self.mantle, self.base, 0.3);
        let bg_row_hover = blend(self.base, self.surface0, 0.5);
        let split_hover_bg = blend(self.base, self.blue, 0.3);
        let divider_idle = blend(self.surface0, self.surface1, 0.3);
        let divider_active = blend(self.surface1, self.blue, 0.5);
        let ws_div_idle = blend(self.base, self.surface0, 0.4);
        let ws_div_active = blend(self.surface1, self.overlay0, 0.5);
        let fg_dir_entry = blend(self.blue, self.subtext1, 0.3);
        let md_code_bg = blend(self.base, self.surface0, 0.5);

        let danger_bg = if self.is_light {
            [200, 60, 60]
        } else {
            [180, 60, 60]
        };
        let danger_fg = if self.is_light {
            [255, 220, 220]
        } else {
            [210, 180, 180]
        };

        let cursor_color = if self.is_light {
            Color32::from_rgba_premultiplied(40, 40, 40, 220)
        } else {
            Color32::from_rgba_premultiplied(255, 255, 255, 200)
        };
        let cursor_dim_color = if self.is_light {
            Color32::from_rgba_premultiplied(40, 40, 40, 140)
        } else {
            Color32::from_rgba_premultiplied(255, 255, 255, 160)
        };
        let selection_bg_rgb = blend(self.base, self.blue, 0.55);
        let scrollbar_color = if self.is_light {
            Color32::from_rgba_unmultiplied(80, 80, 80, 150)
        } else {
            Color32::from_rgba_unmultiplied(180, 180, 180, 150)
        };

        let ansi_c32: [Color32; 16] = std::array::from_fn(|i| c(self.ansi[i]));
        let md_bullet = blend(self.green, self.overlay0, 0.3);

        Theme {
            id: self.id,
            is_light: self.is_light,

            crust_rgb: self.crust,
            mantle_rgb: self.mantle,
            base_rgb: self.base,
            surface0_rgb: self.surface0,
            surface1_rgb: self.surface1,
            surface2_rgb: self.surface2,
            overlay0_rgb: self.overlay0,
            overlay1_rgb: self.overlay1,
            subtext0_rgb: self.subtext0,
            subtext1_rgb: self.subtext1,
            text_rgb: self.text,
            blue_rgb: self.blue,
            green_rgb: self.green,
            red_rgb: self.red,
            yellow_rgb: self.yellow,
            mauve_rgb: self.mauve,
            teal_rgb: self.teal,

            crust: c(self.crust),
            mantle: c(self.mantle),
            base: c(self.base),
            surface0: c(self.surface0),
            surface1: c(self.surface1),
            surface2: c(self.surface2),
            overlay0: c(self.overlay0),
            overlay1: c(self.overlay1),
            subtext0: c(self.subtext0),
            subtext1: c(self.subtext1),
            text: c(self.text),
            blue: c(self.blue),
            green: c(self.green),
            red: c(self.red),
            yellow: c(self.yellow),
            mauve: c(self.mauve),
            teal: c(self.teal),

            bg_panel_fill: c(self.mantle),
            bg_workspace_fill: c(bg_workspace_fill),
            bg_term: c(self.base),
            bg_row_active: c(self.surface0),
            bg_row_hover: c(bg_row_hover),

            fg_primary: c(self.text),
            fg_secondary: c(self.subtext0),
            fg_muted: c(self.overlay0),
            fg_path: c(self.blue),
            fg_dir_entry: c(fg_dir_entry),
            fg_md_file: c(self.green),
            fg_other_file: c(self.subtext1),

            danger_bg: c(danger_bg),
            danger_fg: c(danger_fg),
            split_hover_bg: c(split_hover_bg),
            divider_idle: c(divider_idle),
            divider_active: c(divider_active),
            ws_div_idle: c(ws_div_idle),
            ws_div_active: c(ws_div_active),

            git_added: c(self.green),
            git_removed: c(self.red),
            git_modified: c(self.yellow),
            git_renamed: c(self.mauve),
            git_untracked: c(self.overlay0),
            git_hunk: c(blend(self.blue, self.teal, 0.4)),
            git_header: c(self.yellow),
            git_filename: c(blend(self.blue, self.subtext1, 0.2)),

            md_code: c(self.green),
            md_code_bg: c(md_code_bg),
            md_code_border: c(self.surface1),
            md_bullet: c(md_bullet),
            md_blockquote: c(blend(self.subtext0, self.blue, 0.3)),

            ansi: ansi_c32,
            cursor_color,
            cursor_dim_color,
            selection_bg: Color32::from_rgba_unmultiplied(selection_bg_rgb[0], selection_bg_rgb[1], selection_bg_rgb[2], 160),
            scrollbar_color,
        }
    }
}

// ── Global state ─────────────────────────────────────────────────────────────

static THEMES: OnceLock<Vec<Theme>> = OnceLock::new();
static ACTIVE_IDX: AtomicUsize = AtomicUsize::new(0);

fn themes() -> &'static Vec<Theme> {
    THEMES.get_or_init(|| all_defs().iter().map(|d| d.build()).collect())
}

pub fn active() -> &'static Theme {
    &themes()[ACTIVE_IDX.load(Ordering::Relaxed)]
}

pub fn set_theme(id: ThemeId) {
    ACTIVE_IDX.store(id.index(), Ordering::Relaxed);
}

pub fn current_id() -> ThemeId {
    themes()[ACTIVE_IDX.load(Ordering::Relaxed)].id
}

pub fn all_themes() -> &'static [Theme] {
    themes().as_slice()
}

// ── Spacing scale ───────────────────────────────────────────────────────────

pub const SP_XS: f32 = 2.0;
pub const SP_SM: f32 = 4.0;
pub const SP_MD: f32 = 8.0;
pub const SP_LG: f32 = 12.0;
pub const SP_XL: f32 = 16.0;

// ── Layout size constants ───────────────────────────────────────────────────

pub const TITLEBAR_H: f32 = 32.0;
pub const TITLEBAR_BTN_W: f32 = 44.0;
pub const TITLEBAR_ICON_GAP: f32 = 4.0;
pub const SYSMON_W: f32 = 100.0;
pub const UPDATE_BTN_W: f32 = 110.0;
pub const HEADER_H: f32 = 28.0;
pub const DIVIDER_W: f32 = 4.0;
pub const MIN_PANE_W: f32 = 80.0;
pub const BTN_W: f32 = 24.0;
pub const SESSION_ROW_H: f32 = 28.0;
pub const BADGE_W: f32 = 22.0;
pub const WS_BORDER_W: f32 = 2.0;
pub const LEFT_SIDEBAR_W: f32 = 200.0;
pub const BAR_PAD_X: f32 = 6.0;
pub const ROUNDING: f32 = 4.0;
pub const OVERLAY_DIM: u8 = 160;

// ── Tab bar constants ───────────────────────────────────────────────────────

pub const TAB_W: f32 = 160.0;
pub const TAB_COLOR_STRIP_W: f32 = 3.0;
pub const TAB_ACTIVE_HIGHLIGHT_H: f32 = 2.0;
pub const TAB_PAD_X: f32 = 6.0;

// ── Stroke widths ───────────────────────────────────────────────────────────

pub const STROKE_THIN: f32 = 1.0;
pub const STROKE_MEDIUM: f32 = 1.5;
pub const STROKE_BOLD: f32 = 2.0;

// ── Font sizes ──────────────────────────────────────────────────────────────

pub const SESSION_FONT_SZ: f32 = 12.5;
pub const HEADER_FONT_SZ: f32 = 12.5;
pub const STATUS_FONT_SZ: f32 = 12.5;
pub const DIFF_FONT_SZ: f32 = 11.0;
pub const CWD_FONT_SZ: f32 = 11.0;
pub const SHORTCUT_HINT_SZ: f32 = 10.0;
pub const TERM_FONT_SZ: f32 = 14.0;
pub const TERM_BOLD_FONT_SZ: f32 = 14.5;
pub const BROWSER_ROW_H: f32 = 13.0;
pub const DIALOG_TITLE_SZ: f32 = 15.0;
pub const DIALOG_CLOSE_SZ: f32 = 16.0;

// ── Helper functions ─────────────────────────────────────────────────────────

pub fn from_rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn tinted(c: [u8; 3], factor: f32) -> [u8; 3] {
    let base = active().base_rgb;
    [
        (base[0] as f32 + (c[0] as f32 - base[0] as f32) * factor).clamp(0.0, 255.0) as u8,
        (base[1] as f32 + (c[1] as f32 - base[1] as f32) * factor).clamp(0.0, 255.0) as u8,
        (base[2] as f32 + (c[2] as f32 - base[2] as f32) * factor).clamp(0.0, 255.0) as u8,
    ]
}

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
        active().text
    } else {
        Color32::BLACK
    }
}

pub fn header_bg(ws_color: Option<[u8; 3]>, is_active: bool) -> Color32 {
    match (ws_color, is_active) {
        (Some(c), true) => from_rgb(tinted(c, 0.75)),
        (Some(c), false) => from_rgb(tinted(c, 0.35)),
        (None, true) => active().surface0,
        (None, false) => {
            let base = active().base_rgb;
            let s0 = active().surface0_rgb;
            let mid = [
                (base[0] as u16 + s0[0] as u16) as u8 / 2,
                (base[1] as u16 + s0[1] as u16) as u8 / 2,
                (base[2] as u16 + s0[2] as u16) as u8 / 2,
            ];
            from_rgb(mid)
        }
    }
}

pub fn short_path(p: &std::path::Path) -> String {
    let parts: Vec<&str> = p
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    if parts.len() <= 2 {
        p.display().to_string()
    } else {
        format!("\u{2026}/{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    }
}

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

// ── Theme definitions ────────────────────────────────────────────────────────

fn all_defs() -> Vec<ThemeDef> {
    vec![
        // 0: Catppuccin Mocha (default)
        ThemeDef {
            id: ThemeId::CatppuccinMocha,
            is_light: false,
            crust: [17, 17, 27],
            mantle: [24, 24, 37],
            base: [30, 30, 46],
            surface0: [49, 50, 68],
            surface1: [69, 71, 90],
            surface2: [88, 91, 112],
            overlay0: [108, 112, 134],
            overlay1: [127, 132, 156],
            subtext0: [166, 173, 200],
            subtext1: [184, 192, 224],
            text: [205, 214, 244],
            blue: [137, 180, 250],
            green: [166, 227, 161],
            red: [243, 139, 168],
            yellow: [249, 226, 175],
            mauve: [203, 166, 247],
            teal: [148, 226, 213],
            ansi: [
                [30, 30, 46],     // black
                [243, 139, 168],  // red
                [166, 227, 161],  // green
                [249, 226, 175],  // yellow
                [137, 180, 250],  // blue
                [245, 194, 231],  // magenta
                [148, 226, 213],  // cyan
                [205, 214, 244],  // white
                [88, 91, 112],    // bright black
                [243, 139, 168],  // bright red
                [166, 227, 161],  // bright green
                [249, 226, 175],  // bright yellow
                [137, 180, 250],  // bright blue
                [245, 194, 231],  // bright magenta
                [148, 226, 213],  // bright cyan
                [255, 255, 255],  // bright white
            ],
        },
        // 1: Dracula
        ThemeDef {
            id: ThemeId::Dracula,
            is_light: false,
            crust: [30, 31, 41],
            mantle: [37, 38, 50],
            base: [40, 42, 54],
            surface0: [55, 57, 72],
            surface1: [68, 71, 90],
            surface2: [83, 85, 108],
            overlay0: [98, 114, 164],
            overlay1: [127, 132, 156],
            subtext0: [174, 178, 194],
            subtext1: [200, 204, 218],
            text: [248, 248, 242],
            blue: [139, 233, 253],
            green: [80, 250, 123],
            red: [255, 85, 85],
            yellow: [241, 250, 140],
            mauve: [189, 147, 249],
            teal: [139, 233, 253],
            ansi: [
                [33, 34, 44],     // black
                [255, 85, 85],    // red
                [80, 250, 123],   // green
                [241, 250, 140],  // yellow
                [189, 147, 249],  // blue
                [255, 121, 198],  // magenta
                [139, 233, 253],  // cyan
                [248, 248, 242],  // white
                [98, 114, 164],   // bright black
                [255, 110, 110],  // bright red
                [105, 255, 148],  // bright green
                [255, 255, 165],  // bright yellow
                [210, 172, 255],  // bright blue
                [255, 146, 218],  // bright magenta
                [164, 255, 255],  // bright cyan
                [255, 255, 255],  // bright white
            ],
        },
        // 2: Nord
        ThemeDef {
            id: ThemeId::Nord,
            is_light: false,
            crust: [36, 40, 50],
            mantle: [40, 44, 55],
            base: [46, 52, 64],
            surface0: [59, 66, 82],
            surface1: [67, 76, 94],
            surface2: [76, 86, 106],
            overlay0: [100, 110, 130],
            overlay1: [130, 140, 158],
            subtext0: [160, 170, 185],
            subtext1: [192, 200, 212],
            text: [216, 222, 233],
            blue: [136, 192, 208],
            green: [163, 190, 140],
            red: [191, 97, 106],
            yellow: [235, 203, 139],
            mauve: [180, 142, 173],
            teal: [143, 188, 187],
            ansi: [
                [59, 66, 82],     // black
                [191, 97, 106],   // red
                [163, 190, 140],  // green
                [235, 203, 139],  // yellow
                [129, 161, 193],  // blue
                [180, 142, 173],  // magenta
                [136, 192, 208],  // cyan
                [229, 233, 240],  // white
                [76, 86, 106],    // bright black
                [208, 135, 112],  // bright red
                [163, 190, 140],  // bright green
                [235, 203, 139],  // bright yellow
                [136, 192, 208],  // bright blue
                [180, 142, 173],  // bright magenta
                [143, 188, 187],  // bright cyan
                [236, 239, 244],  // bright white
            ],
        },
        // 3: Solarized Dark
        ThemeDef {
            id: ThemeId::SolarizedDark,
            is_light: false,
            crust: [0, 34, 43],
            mantle: [0, 38, 48],
            base: [0, 43, 54],
            surface0: [7, 54, 66],
            surface1: [24, 68, 78],
            surface2: [42, 84, 94],
            overlay0: [88, 110, 117],
            overlay1: [101, 123, 131],
            subtext0: [131, 148, 150],
            subtext1: [147, 161, 161],
            text: [238, 232, 213],
            blue: [38, 139, 210],
            green: [133, 153, 0],
            red: [220, 50, 47],
            yellow: [181, 137, 0],
            mauve: [108, 113, 196],
            teal: [42, 161, 152],
            ansi: [
                [7, 54, 66],      // black
                [220, 50, 47],    // red
                [133, 153, 0],    // green
                [181, 137, 0],    // yellow
                [38, 139, 210],   // blue
                [211, 54, 130],   // magenta
                [42, 161, 152],   // cyan
                [238, 232, 213],  // white
                [0, 43, 54],      // bright black
                [203, 75, 22],    // bright red
                [88, 110, 117],   // bright green
                [101, 123, 131],  // bright yellow
                [131, 148, 150],  // bright blue
                [108, 113, 196],  // bright magenta
                [147, 161, 161],  // bright cyan
                [253, 246, 227],  // bright white
            ],
        },
        // 4: Gruvbox Dark
        ThemeDef {
            id: ThemeId::GruvboxDark,
            is_light: false,
            crust: [24, 24, 24],
            mantle: [29, 32, 33],
            base: [40, 40, 40],
            surface0: [60, 56, 54],
            surface1: [80, 73, 69],
            surface2: [102, 92, 84],
            overlay0: [124, 111, 100],
            overlay1: [146, 131, 116],
            subtext0: [168, 153, 132],
            subtext1: [189, 174, 147],
            text: [235, 219, 178],
            blue: [131, 165, 152],
            green: [184, 187, 38],
            red: [251, 73, 52],
            yellow: [250, 189, 47],
            mauve: [211, 134, 155],
            teal: [142, 192, 124],
            ansi: [
                [40, 40, 40],     // black
                [204, 36, 29],    // red
                [152, 151, 26],   // green
                [215, 153, 33],   // yellow
                [69, 133, 136],   // blue
                [177, 98, 134],   // magenta
                [104, 157, 106],  // cyan
                [168, 153, 132],  // white
                [146, 131, 116],  // bright black
                [251, 73, 52],    // bright red
                [184, 187, 38],   // bright green
                [250, 189, 47],   // bright yellow
                [131, 165, 152],  // bright blue
                [211, 134, 155],  // bright magenta
                [142, 192, 124],  // bright cyan
                [235, 219, 178],  // bright white
            ],
        },
        // 5: Tokyo Night
        ThemeDef {
            id: ThemeId::TokyoNight,
            is_light: false,
            crust: [22, 22, 30],
            mantle: [26, 27, 38],
            base: [36, 40, 59],
            surface0: [52, 56, 78],
            surface1: [65, 72, 104],
            surface2: [86, 95, 137],
            overlay0: [110, 120, 155],
            overlay1: [140, 150, 180],
            subtext0: [160, 170, 195],
            subtext1: [180, 190, 210],
            text: [192, 202, 245],
            blue: [122, 162, 247],
            green: [158, 206, 106],
            red: [247, 118, 142],
            yellow: [224, 175, 104],
            mauve: [187, 154, 247],
            teal: [115, 218, 202],
            ansi: [
                [65, 72, 104],    // black
                [247, 118, 142],  // red
                [158, 206, 106],  // green
                [224, 175, 104],  // yellow
                [122, 162, 247],  // blue
                [187, 154, 247],  // magenta
                [115, 218, 202],  // cyan
                [192, 202, 245],  // white
                [86, 95, 137],    // bright black
                [255, 148, 168],  // bright red
                [178, 226, 126],  // bright green
                [244, 195, 124],  // bright yellow
                [142, 182, 255],  // bright blue
                [207, 174, 255],  // bright magenta
                [135, 238, 222],  // bright cyan
                [222, 232, 255],  // bright white
            ],
        },
        // 6: Rosé Pine
        ThemeDef {
            id: ThemeId::RosePine,
            is_light: false,
            crust: [22, 20, 28],
            mantle: [25, 23, 36],
            base: [35, 33, 54],
            surface0: [42, 39, 63],
            surface1: [57, 53, 82],
            surface2: [72, 68, 100],
            overlay0: [110, 106, 134],
            overlay1: [144, 140, 170],
            subtext0: [165, 160, 190],
            subtext1: [190, 186, 210],
            text: [224, 222, 244],
            blue: [156, 207, 216],
            green: [49, 116, 143],
            red: [235, 111, 146],
            yellow: [246, 193, 119],
            mauve: [196, 167, 231],
            teal: [156, 207, 216],
            ansi: [
                [38, 35, 58],     // black
                [235, 111, 146],  // red
                [49, 116, 143],   // green
                [246, 193, 119],  // yellow
                [156, 207, 216],  // blue
                [196, 167, 231],  // magenta
                [156, 207, 216],  // cyan
                [224, 222, 244],  // white
                [110, 106, 134],  // bright black
                [255, 131, 166],  // bright red
                [69, 136, 163],   // bright green
                [255, 213, 139],  // bright yellow
                [176, 227, 236],  // bright blue
                [216, 187, 251],  // bright magenta
                [176, 227, 236],  // bright cyan
                [244, 242, 255],  // bright white
            ],
        },
        // 7: One Dark
        ThemeDef {
            id: ThemeId::OneDark,
            is_light: false,
            crust: [27, 29, 34],
            mantle: [33, 37, 43],
            base: [40, 44, 52],
            surface0: [53, 57, 65],
            surface1: [62, 68, 81],
            surface2: [78, 84, 98],
            overlay0: [92, 99, 112],
            overlay1: [115, 120, 133],
            subtext0: [139, 145, 157],
            subtext1: [163, 169, 180],
            text: [171, 178, 191],
            blue: [97, 175, 239],
            green: [152, 195, 121],
            red: [224, 108, 117],
            yellow: [229, 192, 123],
            mauve: [198, 120, 221],
            teal: [86, 182, 194],
            ansi: [
                [40, 44, 52],     // black
                [224, 108, 117],  // red
                [152, 195, 121],  // green
                [229, 192, 123],  // yellow
                [97, 175, 239],   // blue
                [198, 120, 221],  // magenta
                [86, 182, 194],   // cyan
                [171, 178, 191],  // white
                [92, 99, 112],    // bright black
                [239, 128, 137],  // bright red
                [172, 215, 141],  // bright green
                [249, 212, 143],  // bright yellow
                [117, 195, 255],  // bright blue
                [218, 140, 241],  // bright magenta
                [106, 202, 214],  // bright cyan
                [211, 218, 231],  // bright white
            ],
        },
        // 8: Everforest Dark
        ThemeDef {
            id: ThemeId::EverforestDark,
            is_light: false,
            crust: [37, 43, 38],
            mantle: [42, 48, 43],
            base: [47, 53, 47],
            surface0: [62, 68, 62],
            surface1: [78, 84, 78],
            surface2: [93, 100, 93],
            overlay0: [113, 119, 113],
            overlay1: [135, 140, 135],
            subtext0: [157, 163, 157],
            subtext1: [179, 184, 179],
            text: [211, 198, 170],
            blue: [127, 187, 179],
            green: [167, 192, 128],
            red: [230, 126, 128],
            yellow: [219, 188, 127],
            mauve: [214, 153, 182],
            teal: [131, 192, 159],
            ansi: [
                [78, 84, 78],     // black
                [230, 126, 128],  // red
                [167, 192, 128],  // green
                [219, 188, 127],  // yellow
                [127, 187, 179],  // blue
                [214, 153, 182],  // magenta
                [131, 192, 159],  // cyan
                [211, 198, 170],  // white
                [113, 119, 113],  // bright black
                [250, 146, 148],  // bright red
                [187, 212, 148],  // bright green
                [239, 208, 147],  // bright yellow
                [147, 207, 199],  // bright blue
                [234, 173, 202],  // bright magenta
                [151, 212, 179],  // bright cyan
                [231, 218, 190],  // bright white
            ],
        },
        // 9: Monokai
        ThemeDef {
            id: ThemeId::Monokai,
            is_light: false,
            crust: [25, 25, 22],
            mantle: [32, 32, 28],
            base: [39, 40, 34],
            surface0: [56, 57, 50],
            surface1: [73, 74, 66],
            surface2: [90, 91, 82],
            overlay0: [117, 113, 94],
            overlay1: [144, 140, 120],
            subtext0: [168, 164, 144],
            subtext1: [192, 188, 168],
            text: [248, 248, 242],
            blue: [102, 217, 239],
            green: [166, 226, 46],
            red: [249, 38, 114],
            yellow: [230, 219, 116],
            mauve: [174, 129, 255],
            teal: [102, 217, 239],
            ansi: [
                [39, 40, 34],     // black
                [249, 38, 114],   // red
                [166, 226, 46],   // green
                [230, 219, 116],  // yellow
                [102, 217, 239],  // blue
                [174, 129, 255],  // magenta
                [102, 217, 239],  // cyan
                [248, 248, 242],  // white
                [117, 113, 94],   // bright black
                [255, 68, 134],   // bright red
                [186, 246, 66],   // bright green
                [250, 239, 136],  // bright yellow
                [122, 237, 255],  // bright blue
                [194, 149, 255],  // bright magenta
                [122, 237, 255],  // bright cyan
                [255, 255, 255],  // bright white
            ],
        },
        // 10: GitHub Dark
        ThemeDef {
            id: ThemeId::GitHubDark,
            is_light: false,
            crust: [13, 17, 23],
            mantle: [22, 27, 34],
            base: [36, 41, 47],
            surface0: [48, 54, 61],
            surface1: [56, 62, 71],
            surface2: [72, 79, 88],
            overlay0: [110, 118, 129],
            overlay1: [139, 148, 158],
            subtext0: [163, 172, 182],
            subtext1: [186, 194, 203],
            text: [230, 237, 243],
            blue: [88, 166, 255],
            green: [63, 185, 80],
            red: [248, 81, 73],
            yellow: [210, 153, 34],
            mauve: [188, 140, 255],
            teal: [63, 185, 80],
            ansi: [
                [72, 79, 88],     // black
                [255, 123, 114],  // red
                [63, 185, 80],    // green
                [210, 153, 34],   // yellow
                [88, 166, 255],   // blue
                [188, 140, 255],  // magenta
                [86, 211, 219],   // cyan
                [230, 237, 243],  // white
                [110, 118, 129],  // bright black
                [255, 148, 139],  // bright red
                [86, 211, 100],   // bright green
                [230, 173, 54],   // bright yellow
                [108, 186, 255],  // bright blue
                [208, 160, 255],  // bright magenta
                [106, 231, 239],  // bright cyan
                [255, 255, 255],  // bright white
            ],
        },
        // 11: Ayu Dark
        ThemeDef {
            id: ThemeId::AyuDark,
            is_light: false,
            crust: [10, 14, 20],
            mantle: [13, 18, 26],
            base: [15, 20, 30],
            surface0: [28, 34, 46],
            surface1: [40, 48, 64],
            surface2: [55, 65, 82],
            overlay0: [90, 100, 118],
            overlay1: [115, 125, 140],
            subtext0: [140, 150, 165],
            subtext1: [170, 180, 195],
            text: [203, 204, 198],
            blue: [57, 186, 230],
            green: [170, 217, 76],
            red: [240, 113, 120],
            yellow: [255, 180, 84],
            mauve: [210, 166, 255],
            teal: [149, 230, 203],
            ansi: [
                [15, 20, 30],     // black
                [240, 113, 120],  // red
                [170, 217, 76],   // green
                [255, 180, 84],   // yellow
                [57, 186, 230],   // blue
                [210, 166, 255],  // magenta
                [149, 230, 203],  // cyan
                [203, 204, 198],  // white
                [90, 100, 118],   // bright black
                [255, 133, 140],  // bright red
                [190, 237, 96],   // bright green
                [255, 200, 104],  // bright yellow
                [77, 206, 250],   // bright blue
                [230, 186, 255],  // bright magenta
                [169, 250, 223],  // bright cyan
                [233, 234, 228],  // bright white
            ],
        },
        // 12: Material Darker
        ThemeDef {
            id: ThemeId::MaterialDarker,
            is_light: false,
            crust: [25, 25, 25],
            mantle: [30, 30, 30],
            base: [33, 33, 33],
            surface0: [48, 48, 48],
            surface1: [60, 60, 60],
            surface2: [74, 74, 74],
            overlay0: [100, 100, 100],
            overlay1: [130, 130, 130],
            subtext0: [158, 158, 158],
            subtext1: [180, 180, 180],
            text: [238, 255, 255],
            blue: [130, 170, 255],
            green: [195, 232, 141],
            red: [240, 113, 120],
            yellow: [255, 203, 107],
            mauve: [199, 146, 234],
            teal: [137, 221, 255],
            ansi: [
                [48, 48, 48],     // black
                [240, 113, 120],  // red
                [195, 232, 141],  // green
                [255, 203, 107],  // yellow
                [130, 170, 255],  // blue
                [199, 146, 234],  // magenta
                [137, 221, 255],  // cyan
                [238, 255, 255],  // white
                [100, 100, 100],  // bright black
                [255, 133, 140],  // bright red
                [215, 252, 161],  // bright green
                [255, 223, 127],  // bright yellow
                [150, 190, 255],  // bright blue
                [219, 166, 254],  // bright magenta
                [157, 241, 255],  // bright cyan
                [255, 255, 255],  // bright white
            ],
        },
        // 13: Solarized Light
        ThemeDef {
            id: ThemeId::SolarizedLight,
            is_light: true,
            crust: [238, 232, 213],
            mantle: [243, 237, 218],
            base: [253, 246, 227],
            surface0: [238, 232, 213],
            surface1: [220, 215, 200],
            surface2: [200, 195, 180],
            overlay0: [147, 161, 161],
            overlay1: [131, 148, 150],
            subtext0: [101, 123, 131],
            subtext1: [88, 110, 117],
            text: [7, 54, 66],
            blue: [38, 139, 210],
            green: [133, 153, 0],
            red: [220, 50, 47],
            yellow: [181, 137, 0],
            mauve: [108, 113, 196],
            teal: [42, 161, 152],
            ansi: [
                [7, 54, 66],      // black
                [220, 50, 47],    // red
                [133, 153, 0],    // green
                [181, 137, 0],    // yellow
                [38, 139, 210],   // blue
                [211, 54, 130],   // magenta
                [42, 161, 152],   // cyan
                [238, 232, 213],  // white
                [0, 43, 54],      // bright black
                [203, 75, 22],    // bright red
                [88, 110, 117],   // bright green
                [101, 123, 131],  // bright yellow
                [131, 148, 150],  // bright blue
                [108, 113, 196],  // bright magenta
                [147, 161, 161],  // bright cyan
                [253, 246, 227],  // bright white
            ],
        },
        // 14: Catppuccin Latte
        ThemeDef {
            id: ThemeId::CatppuccinLatte,
            is_light: true,
            crust: [220, 224, 232],
            mantle: [230, 233, 239],
            base: [239, 241, 245],
            surface0: [204, 208, 218],
            surface1: [188, 192, 204],
            surface2: [172, 176, 190],
            overlay0: [140, 143, 161],
            overlay1: [124, 127, 147],
            subtext0: [108, 111, 133],
            subtext1: [92, 95, 119],
            text: [76, 79, 105],
            blue: [30, 102, 245],
            green: [64, 160, 43],
            red: [210, 15, 57],
            yellow: [223, 142, 29],
            mauve: [136, 57, 239],
            teal: [23, 146, 153],
            ansi: [
                [172, 176, 190],  // black
                [210, 15, 57],    // red
                [64, 160, 43],    // green
                [223, 142, 29],   // yellow
                [30, 102, 245],   // blue
                [136, 57, 239],   // magenta
                [23, 146, 153],   // cyan
                [76, 79, 105],    // white
                [140, 143, 161],  // bright black
                [210, 15, 57],    // bright red
                [64, 160, 43],    // bright green
                [223, 142, 29],   // bright yellow
                [30, 102, 245],   // bright blue
                [136, 57, 239],   // bright magenta
                [23, 146, 153],   // bright cyan
                [44, 47, 71],     // bright white
            ],
        },
    ]
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_tinted_at_zero_returns_base() {
        set_theme(ThemeId::CatppuccinMocha);
        let result = tinted([255, 0, 0], 0.0);
        assert_eq!(result, active().base_rgb);
    }

    #[test]
    fn test_tinted_at_one_returns_color() {
        set_theme(ThemeId::CatppuccinMocha);
        let c = [200u8, 100, 50];
        let result = tinted(c, 1.0);
        assert_eq!(result, c);
    }

    #[test]
    fn test_tinted_midpoint() {
        set_theme(ThemeId::CatppuccinMocha);
        let c = [100u8, 100, 100];
        let result = tinted(c, 0.5);
        let base = active().base_rgb;
        let expected = [
            (base[0] as f32 + (100.0 - base[0] as f32) * 0.5) as u8,
            (base[1] as f32 + (100.0 - base[1] as f32) * 0.5) as u8,
            (base[2] as f32 + (100.0 - base[2] as f32) * 0.5) as u8,
        ];
        assert_eq!(result, expected);
    }

    #[test]
    fn test_text_on_dark_bg_returns_light_text() {
        set_theme(ThemeId::CatppuccinMocha);
        let color = text_on([10, 10, 10]);
        assert_eq!(color, active().text);
    }

    #[test]
    fn test_text_on_light_bg_returns_black() {
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
        set_theme(ThemeId::CatppuccinMocha);
        let c = Some([100u8, 140, 230]);
        let active_bg = header_bg(c, true);
        let inactive = header_bg(c, false);
        assert_ne!(active_bg, inactive);
    }

    #[test]
    fn test_header_bg_no_color() {
        set_theme(ThemeId::CatppuccinMocha);
        let active_bg = header_bg(None, true);
        let inactive = header_bg(None, false);
        assert_eq!(active_bg, active().surface0);
        assert_ne!(active_bg, inactive);
    }

    #[test]
    fn test_theme_switching() {
        set_theme(ThemeId::CatppuccinMocha);
        let mocha_text = active().text;
        set_theme(ThemeId::Dracula);
        let dracula_text = active().text;
        assert_ne!(mocha_text, dracula_text);
        set_theme(ThemeId::CatppuccinMocha);
    }

    #[test]
    fn test_all_theme_ids_produce_valid_themes() {
        for &id in ThemeId::ALL {
            set_theme(id);
            let t = active();
            assert_eq!(t.id, id);
            assert_ne!(t.text, Color32::TRANSPARENT);
            assert_ne!(t.base, Color32::TRANSPARENT);
        }
        set_theme(ThemeId::CatppuccinMocha);
    }
}
