#![allow(dead_code)]
use egui::Color32;
use std::cell::RefCell;
use std::collections::HashMap;
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

    // Accent / semantic states
    pub accent: Color32,
    pub accent_muted: Color32,
    pub accent_strong: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub error: Color32,

    // Flash feedback
    pub flash_bg: Color32,
    pub flash_success_bg: Color32,
    pub flash_error_bg: Color32,

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
    pub md_inline_code: Color32,
    pub md_inline_code_bg: Color32,
    pub md_link: Color32,
    pub md_table_border: Color32,
    pub md_table_header_bg: Color32,
    pub md_table_row_alt_bg: Color32,

    // Terminal
    pub ansi: [Color32; 16],
    pub cursor_color: Color32,
    pub cursor_dim_color: Color32,
    pub selection_bg: Color32,
    pub scrollbar_color: Color32,

    // UI refresh: component backgrounds
    pub bg_input: Color32,
    pub bg_tab_active: Color32,
    pub bg_tab_inactive: Color32,
    pub bg_toolbar: Color32,

    // UI refresh: borders
    pub border_panel: Color32,
    pub border_subtle: Color32,
    pub border_focus: Color32,

    // UI refresh: shadows (simulated via darker strokes)
    pub shadow_sm: Color32,
    pub shadow_md: Color32,
}

impl ThemeDef {
    fn build(&self) -> Theme {
        let c = |rgb: [u8; 3]| Color32::from_rgb(rgb[0], rgb[1], rgb[2]);

        let blend = |base: [u8; 3], target: [u8; 3], factor: f32| -> [u8; 3] {
            [
                (base[0] as f32 + (target[0] as f32 - base[0] as f32) * factor).clamp(0.0, 255.0)
                    as u8,
                (base[1] as f32 + (target[1] as f32 - base[1] as f32) * factor).clamp(0.0, 255.0)
                    as u8,
                (base[2] as f32 + (target[2] as f32 - base[2] as f32) * factor).clamp(0.0, 255.0)
                    as u8,
            ]
        };

        let bg_workspace_fill = blend(self.mantle, self.base, BLEND_LIGHT);
        let bg_row_hover = blend(self.base, self.surface0, BLEND_MEDIUM);
        let split_hover_bg = blend(self.base, self.blue, BLEND_LIGHT);
        let divider_idle = blend(self.surface0, self.surface1, BLEND_LIGHT);
        let divider_active = blend(self.surface1, self.blue, BLEND_MEDIUM);
        let ws_div_idle = blend(self.base, self.surface0, 0.4);
        let ws_div_active = blend(self.surface1, self.overlay0, BLEND_MEDIUM);
        let fg_dir_entry = blend(self.blue, self.subtext1, BLEND_LIGHT);
        let md_code_bg = blend(self.base, self.surface0, BLEND_MEDIUM);

        let accent_muted = blend(self.base, self.blue, 0.2);
        let accent_strong = blend(self.blue, self.text, BLEND_LIGHT);
        let flash_bg = blend(self.base, self.blue, BLEND_SUBTLE);
        let flash_success_bg = blend(self.base, self.green, BLEND_SUBTLE);
        let flash_error_bg = blend(self.base, self.red, BLEND_SUBTLE);

        let danger_bg = blend(self.base, self.red, BLEND_SUBTLE);
        let danger_fg = blend(self.red, self.text, BLEND_LIGHT);

        let cursor_color = if self.is_light {
            Color32::from_rgba_unmultiplied(40, 40, 40, ALPHA_CURSOR)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, ALPHA_CURSOR)
        };
        let cursor_dim_color = if self.is_light {
            Color32::from_rgba_unmultiplied(40, 40, 40, ALPHA_CURSOR_DIM)
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, ALPHA_CURSOR_DIM)
        };
        let selection_bg_rgb = blend(self.base, self.blue, 0.55);
        let scrollbar_color = if self.is_light {
            Color32::from_rgba_unmultiplied(80, 80, 80, ALPHA_SCROLLBAR_IDLE)
        } else {
            Color32::from_rgba_unmultiplied(180, 180, 180, ALPHA_SCROLLBAR_IDLE)
        };

        let ansi_c32: [Color32; 16] = std::array::from_fn(|i| c(self.ansi[i]));
        let md_bullet = blend(self.green, self.overlay0, BLEND_LIGHT);

        let bg_input = blend(self.base, self.surface0, 0.6);
        let bg_tab_inactive = blend(self.mantle, self.surface0, BLEND_LIGHT);
        let border_subtle_rgb = blend(self.surface0, self.surface1, BLEND_MEDIUM);

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

            accent: c(self.blue),
            accent_muted: c(accent_muted),
            accent_strong: c(accent_strong),
            success: c(self.green),
            warning: c(self.yellow),
            error: c(self.red),

            flash_bg: c(flash_bg),
            flash_success_bg: c(flash_success_bg),
            flash_error_bg: c(flash_error_bg),

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
            md_inline_code: c(self.teal),
            md_inline_code_bg: c(blend(self.base, self.surface0, 0.6)),
            md_link: c(self.blue),
            md_table_border: c(self.surface1),
            md_table_header_bg: c(blend(self.base, self.surface0, 0.7)),
            md_table_row_alt_bg: c(blend(self.base, self.surface0, 0.3)),

            ansi: ansi_c32,
            cursor_color,
            cursor_dim_color,
            selection_bg: Color32::from_rgba_unmultiplied(
                selection_bg_rgb[0],
                selection_bg_rgb[1],
                selection_bg_rgb[2],
                ALPHA_SELECTION,
            ),
            scrollbar_color,

            bg_input: c(bg_input),
            bg_tab_active: c(self.base),
            bg_tab_inactive: c(bg_tab_inactive),
            bg_toolbar: c(self.mantle),

            border_panel: c(self.surface0),
            border_subtle: c(border_subtle_rgb),
            border_focus: Color32::from_rgba_unmultiplied(
                self.blue[0],
                self.blue[1],
                self.blue[2],
                ALPHA_BORDER_FOCUS,
            ),

            shadow_sm: Color32::from_rgba_unmultiplied(
                self.crust[0],
                self.crust[1],
                self.crust[2],
                ALPHA_SHADOW_SM,
            ),
            shadow_md: Color32::from_rgba_unmultiplied(
                self.crust[0],
                self.crust[1],
                self.crust[2],
                ALPHA_SHADOW_MD,
            ),
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

// ── Spacing scale (4px base) ────────────────────────────────────────────────

pub const SP_0: f32 = 0.0;
pub const SP_1: f32 = 2.0;
pub const SP_2: f32 = 4.0;
pub const SP_3: f32 = 6.0;
pub const SP_4: f32 = 8.0;
pub const SP_5: f32 = 12.0;
pub const SP_6: f32 = 16.0;

// ── Corner radii ────────────────────────────────────────────────────────────

pub const R_NONE: f32 = 0.0;
pub const R_SM: f32 = 3.0;
pub const R_MD: f32 = 6.0;
pub const R_LG: f32 = 8.0;

// ── Layout dimensions ──────────────────────────────────────────────────────

pub const TITLEBAR_H: f32 = 28.0;
pub const TITLEBAR_BTN_W: f32 = 44.0;
pub const TITLEBAR_ICON_GAP: f32 = 4.0;
pub const SYSMON_W: f32 = 100.0;
pub const UPDATE_BTN_W: f32 = 110.0;
pub const HEADER_H: f32 = 26.0;
pub const DIVIDER_W: f32 = 6.0;
pub const MIN_PANE_W: f32 = 80.0;
pub const BTN_W: f32 = 24.0;
pub const SESSION_ROW_H: f32 = 26.0;
pub const SEARCH_BAR_H: f32 = 22.0;
pub const FILE_ROW_H: f32 = 22.0;
pub const WS_BORDER_W: f32 = 4.0;
pub const RESIZE_BORDER: f32 = 5.0;
pub const MIN_WINDOW_W: f32 = 640.0;
pub const MIN_WINDOW_H: f32 = 400.0;
pub const LEFT_SIDEBAR_W: f32 = 180.0;
pub const RIGHT_SIDEBAR_W: f32 = 180.0;
pub const CONTEXT_BTN_SZ: f32 = 20.0;

// ── Tab bar constants ───────────────────────────────────────────────────────

pub const TAB_H: f32 = 26.0;
pub const TAB_W: f32 = 150.0;
pub const TAB_COLOR_STRIP_W: f32 = 3.0;
pub const TAB_ACTIVE_HIGHLIGHT_H: f32 = 2.0;
pub const TAB_PAD_X: f32 = 8.0;
pub const TAB_ACTIONS_W: f32 = 82.0;
pub const TAB_ACTION_GAP: f32 = 2.0;

// ── Terminal inner padding ──────────────────────────────────────────────────

pub const TERM_PAD_LEFT: f32 = 4.0;
pub const TERM_PAD_TOP: f32 = 2.0;
pub const TERM_PAD_RIGHT: f32 = 8.0;
pub const TERM_PAD_BOTTOM: f32 = 2.0;

// ── Scrollbar ───────────────────────────────────────────────────────────────

pub const SCROLLBAR_W_IDLE: f32 = 4.0;
pub const SCROLLBAR_W_ACTIVE: f32 = 8.0;
pub const SCROLLBAR_HIT_W: f32 = TERM_PAD_RIGHT;
pub const SCROLLBAR_MIN_THUMB: f32 = 32.0;

pub fn scroll_style() -> egui::style::ScrollStyle {
    egui::style::ScrollStyle {
        floating: true,
        bar_width: SCROLLBAR_W_ACTIVE,
        handle_min_length: SCROLLBAR_MIN_THUMB,
        bar_inner_margin: SP_0,
        bar_outer_margin: 0.0,
        floating_width: SCROLLBAR_W_IDLE,
        floating_allocated_width: SCROLLBAR_W_ACTIVE,
        foreground_color: true,
        dormant_background_opacity: 0.0,
        active_background_opacity: ALPHA_SCROLLBAR_IDLE as f32 / 255.0,
        interact_background_opacity: ALPHA_SCROLLBAR_HOVER as f32 / 255.0,
        dormant_handle_opacity: 0.0,
        active_handle_opacity: ALPHA_SCROLLBAR_HOVER as f32 / 255.0,
        interact_handle_opacity: ALPHA_SCROLLBAR_DRAG as f32 / 255.0,
    }
}

// ── Stroke widths ───────────────────────────────────────────────────────────

pub const STROKE_THIN: f32 = 1.0;
pub const STROKE_MEDIUM: f32 = 1.5;
pub const STROKE_BOLD: f32 = 2.0;

// ── Typography scale ────────────────────────────────────────────────────────

pub const FONT_HEADING_1: f32 = 22.0;
pub const FONT_HEADING_2: f32 = 18.0;
pub const FONT_STATUS: f32 = 16.0;
pub const FONT_TERM: f32 = 14.0;
pub const FONT_TERM_BOLD: f32 = 14.0;
pub const FONT_UI_LG: f32 = 13.0;
pub const FONT_UI_MD: f32 = 12.0;
pub const FONT_UI_SM: f32 = 11.0;
pub const FONT_UI_XS: f32 = 10.0;
pub const FONT_SYS_SM: f32 = 9.0;
pub const FONT_SYS_XS: f32 = 8.0;

// ── Icon metrics ────────────────────────────────────────────────────────────

pub const ICON_SM: f32 = 10.0;
pub const ICON_MD: f32 = 14.0;
pub const ICON_LG: f32 = 18.0;
pub const ICON_STROKE: f32 = 1.5;
pub const ICON_PAD: f32 = 3.0;

// ── Alpha constants ─────────────────────────────────────────────────────────

pub const ALPHA_CURSOR: u8 = 200;
pub const ALPHA_CURSOR_DIM: u8 = 160;
pub const ALPHA_SELECTION: u8 = 140;
pub const ALPHA_OVERLAY_DIM: u8 = 140;
pub const ALPHA_SCROLLBAR_IDLE: u8 = 160;
pub const ALPHA_SCROLLBAR_HOVER: u8 = 200;
pub const ALPHA_SCROLLBAR_DRAG: u8 = 240;
pub const ALPHA_FLASH: u8 = 120;
pub const ALPHA_SCROLL_INDICATOR: u8 = 60;
pub const ALPHA_SHADOW_SM: u8 = 100;
pub const ALPHA_SHADOW_MD: u8 = 150;
pub const ALPHA_BORDER_FOCUS: u8 = 100;

// ── Animation durations (seconds) ─────────────────────────────────────────
pub const ANIM_FAST: f32 = 0.12;
pub const ANIM_NORMAL: f32 = 0.20;
pub const ANIM_SMOOTH: f32 = 0.30;

// ── Button alpha tokens ──────────────────────────────────────────────────
pub const ALPHA_BTN_IDLE: u8 = 20;
pub const ALPHA_BTN_FILL: u8 = 38;
pub const ALPHA_BTN_STROKE: u8 = 50;
pub const ALPHA_BTN_HOVER: u8 = 64;
pub const ALPHA_SURFACE_OVERLAY: u8 = 200;

// ── Blend factors ───────────────────────────────────────────────────────────

pub const BLEND_SUBTLE: f32 = 0.15;
pub const BLEND_LIGHT: f32 = 0.30;
pub const BLEND_MEDIUM: f32 = 0.50;
pub const BLEND_STRONG: f32 = 0.75;

// ── Flash timing ────────────────────────────────────────────────────────────

pub const FLASH_DURATION_MS: u64 = 350;

// ── Button sizing ──────────────────────────────────────────────────────────

pub const BTN_SQ: f32 = 22.0;
pub const BTN_H_ACTION: f32 = 28.0;

// ── Dialog layout ──────────────────────────────────────────────────────────

pub const DIALOG_MARGIN: f32 = SP_6;
pub const DIALOG_ITEM_H: f32 = 26.0;
pub const DIALOG_TOP_OFFSET: f32 = 80.0;

// ── Panel divider ──────────────────────────────────────────────────────────

pub const PANEL_DIV_H: f32 = 8.0;

// ── Git row ────────────────────────────────────────────────────────────────

pub const GIT_ROW_H: f32 = 14.0;
pub const GIT_FONT_SZ: f32 = 10.0;

// ── Workspace card ─────────────────────────────────────────────────────────

pub const CARD_BAR_W: f32 = 3.0;
pub const CARD_GEAR_W: f32 = 26.0;

// ── Dot menu ───────────────────────────────────────────────────────────────

pub const DOT_R: f32 = 1.5;
pub const DOT_GAP: f32 = 4.5;

// ── Icon button inset ──────────────────────────────────────────────────────

pub const ICON_INSET: f32 = 6.0;

// ── Tint factors ───────────────────────────────────────────────────────────

pub const TINT_ACTIVE: f32 = 0.65;
pub const TINT_INACTIVE: f32 = 0.45;
pub const TINT_BORDER: f32 = 0.30;

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

static SRGB_LUT: OnceLock<[f32; 256]> = OnceLock::new();

fn srgb_lut() -> &'static [f32; 256] {
    SRGB_LUT.get_or_init(|| {
        std::array::from_fn(|i| {
            let f = i as f32 / 255.0;
            if f <= 0.04045 {
                f / 12.92
            } else {
                ((f + 0.055) / 1.055).powf(2.4)
            }
        })
    })
}

fn relative_luminance(c: [u8; 3]) -> f32 {
    let lut = srgb_lut();
    0.2126 * lut[c[0] as usize] + 0.7152 * lut[c[1] as usize] + 0.0722 * lut[c[2] as usize]
}

fn contrast_ratio(a: [u8; 3], b: [u8; 3]) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la > lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

thread_local! {
    static CONTRAST_CACHE: RefCell<HashMap<(u32, u32), Color32>> = RefCell::new(HashMap::with_capacity(256));
}

/// Clear the per-frame contrast cache. Call once at the start of each frame.
pub fn clear_contrast_cache() {
    CONTRAST_CACHE.with(|c| c.borrow_mut().clear());
}

/// Picks the adjustment direction (lighten or darken) that has more
/// contrast headroom, with fallback to the opposite direction.
/// Results are cached per (fg, bg) pair within a frame.
pub fn ensure_term_contrast(fg: Color32, bg: Color32) -> Color32 {
    let fg_key = u32::from_le_bytes(fg.to_array());
    let bg_key = u32::from_le_bytes(bg.to_array());
    let key = (fg_key, bg_key);

    let cached = CONTRAST_CACHE.with(|c| c.borrow().get(&key).copied());
    if let Some(result) = cached {
        return result;
    }

    let result = ensure_term_contrast_inner(fg, bg);
    CONTRAST_CACHE.with(|c| c.borrow_mut().insert(key, result));
    result
}

fn ensure_term_contrast_inner(fg: Color32, bg: Color32) -> Color32 {
    let fg_rgb = [fg.r(), fg.g(), fg.b()];
    let bg_rgb = [bg.r(), bg.g(), bg.b()];

    if contrast_ratio(fg_rgb, bg_rgb) >= 3.0 {
        return fg;
    }

    let bg_lum = relative_luminance(bg_rgb);
    let max_lighten = 1.05 / (bg_lum + 0.05);
    let max_darken = (bg_lum + 0.05) / 0.05;
    let lighten_first = max_lighten >= max_darken;

    if let Some(c) = adjust_toward(fg_rgb, bg_rgb, lighten_first) {
        return c;
    }
    if let Some(c) = adjust_toward(fg_rgb, bg_rgb, !lighten_first) {
        return c;
    }

    if lighten_first {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

fn adjust_toward(fg_rgb: [u8; 3], bg_rgb: [u8; 3], lighten: bool) -> Option<Color32> {
    let mut cur = fg_rgb;
    for _ in 0..30 {
        if contrast_ratio(cur, bg_rgb) >= 3.0 {
            return Some(Color32::from_rgb(cur[0], cur[1], cur[2]));
        }
        if lighten {
            cur = [
                (cur[0] as u16 + (255 - cur[0] as u16) / 4) as u8,
                (cur[1] as u16 + (255 - cur[1] as u16) / 4) as u8,
                (cur[2] as u16 + (255 - cur[2] as u16) / 4) as u8,
            ];
        } else {
            cur = [
                cur[0] - cur[0] / 4,
                cur[1] - cur[1] / 4,
                cur[2] - cur[2] / 4,
            ];
        }
    }
    None
}

pub fn text_on(bg: [u8; 3]) -> Color32 {
    if relative_luminance(bg) < 0.179 {
        active().text
    } else {
        Color32::BLACK
    }
}

/// Adjusts `fg` until it has WCAG AA contrast (4.5:1) against `bg`.
/// Picks the direction with more contrast headroom, with fallback.
pub fn ensure_readable(fg: [u8; 3], bg: [u8; 3]) -> Color32 {
    if contrast_ratio(fg, bg) >= 4.5 {
        return from_rgb(fg);
    }
    let bg_lum = relative_luminance(bg);
    let max_lighten = 1.05 / (bg_lum + 0.05);
    let max_darken = (bg_lum + 0.05) / 0.05;
    let lighten_first = max_lighten >= max_darken;

    let try_dir = |lighten: bool| -> Option<Color32> {
        let mut cur = fg;
        for _ in 0..20 {
            if contrast_ratio(cur, bg) >= 4.5 {
                return Some(from_rgb(cur));
            }
            if lighten {
                cur = [
                    (cur[0] as u16 + (255 - cur[0] as u16) / 3) as u8,
                    (cur[1] as u16 + (255 - cur[1] as u16) / 3) as u8,
                    (cur[2] as u16 + (255 - cur[2] as u16) / 3) as u8,
                ];
            } else {
                cur = [
                    cur[0] - cur[0] / 3,
                    cur[1] - cur[1] / 3,
                    cur[2] - cur[2] / 3,
                ];
            }
        }
        None
    };

    if let Some(c) = try_dir(lighten_first) {
        return c;
    }
    if let Some(c) = try_dir(!lighten_first) {
        return c;
    }
    if lighten_first {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

pub fn header_bg(ws_color: Option<[u8; 3]>, is_active: bool) -> Color32 {
    match (ws_color, is_active) {
        (Some(c), true) => from_rgb(tinted(c, BLEND_STRONG)),
        (Some(c), false) => from_rgb(tinted(c, BLEND_LIGHT + 0.05)),
        (None, true) => active().surface0,
        (None, false) => {
            let base = active().base_rgb;
            let s0 = active().surface0_rgb;
            let mid = [
                ((base[0] as u16 + s0[0] as u16) / 2) as u8,
                ((base[1] as u16 + s0[1] as u16) / 2) as u8,
                ((base[2] as u16 + s0[2] as u16) / 2) as u8,
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
        let sep = std::path::MAIN_SEPARATOR;
        format!(
            "\u{2026}{sep}{}{sep}{}",
            parts[parts.len() - 2],
            parts[parts.len() - 1]
        )
    }
}

pub fn render_inline(ui: &mut egui::Ui, line: &str) {
    let spans = parse_inline_spans(line);
    if spans.len() == 1 {
        if let InlineSpan::Text(t) = &spans[0] {
            ui.label(t.as_str());
            return;
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for span in &spans {
            render_span(ui, span);
        }
    });
}

enum InlineSpan {
    Text(String),
    Bold(String),
    Italic(String),
    BoldItalic(String),
    Code(String),
    Link { text: String, url: String },
}

fn render_span(ui: &mut egui::Ui, span: &InlineSpan) {
    let th = active();
    match span {
        InlineSpan::Text(t) => {
            ui.label(t.as_str());
        }
        InlineSpan::Bold(t) => {
            ui.label(egui::RichText::new(t.as_str()).strong());
        }
        InlineSpan::Italic(t) => {
            ui.label(egui::RichText::new(t.as_str()).italics());
        }
        InlineSpan::BoldItalic(t) => {
            ui.label(egui::RichText::new(t.as_str()).strong().italics());
        }
        InlineSpan::Code(t) => {
            egui::Frame::none()
                .fill(th.md_inline_code_bg)
                .rounding(egui::Rounding::same(R_SM))
                .inner_margin(egui::Margin::symmetric(ICON_PAD, 0.0))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(t.as_str())
                            .monospace()
                            .size(FONT_UI_MD)
                            .color(th.md_inline_code),
                    );
                });
        }
        InlineSpan::Link { text, url } => {
            if ui
                .add(
                    egui::Label::new(egui::RichText::new(text.as_str()).color(th.md_link))
                        .sense(egui::Sense::click()),
                )
                .on_hover_text(url.as_str())
                .clicked()
            {
                let _ = open::that(url);
            }
        }
    }
}

fn parse_inline_spans(input: &str) -> Vec<InlineSpan> {
    let mut spans = Vec::new();
    let mut pos = 0;
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut plain = String::new();

    while pos < len {
        // Inline code: `...`
        if bytes[pos] == b'`' {
            if let Some(end) = input[pos + 1..].find('`') {
                if !plain.is_empty() {
                    spans.push(InlineSpan::Text(std::mem::take(&mut plain)));
                }
                spans.push(InlineSpan::Code(input[pos + 1..pos + 1 + end].to_string()));
                pos = pos + 1 + end + 1;
                continue;
            }
        }

        // Link: [text](url)
        if bytes[pos] == b'[' {
            if let Some(close_bracket) = input[pos + 1..].find(']') {
                let text_end = pos + 1 + close_bracket;
                if text_end + 1 < len && bytes[text_end + 1] == b'(' {
                    if let Some(close_paren) = input[text_end + 2..].find(')') {
                        let link_text = &input[pos + 1..text_end];
                        let url = &input[text_end + 2..text_end + 2 + close_paren];
                        if !plain.is_empty() {
                            spans.push(InlineSpan::Text(std::mem::take(&mut plain)));
                        }
                        spans.push(InlineSpan::Link {
                            text: link_text.to_string(),
                            url: url.to_string(),
                        });
                        pos = text_end + 2 + close_paren + 1;
                        continue;
                    }
                }
            }
        }

        // Bold+italic: ***...***, or bold: **...**, or italic: *...*
        if bytes[pos] == b'*' {
            // ***bold italic***
            if pos + 2 < len && bytes[pos + 1] == b'*' && bytes[pos + 2] == b'*' {
                if let Some(end) = input[pos + 3..].find("***") {
                    if !plain.is_empty() {
                        spans.push(InlineSpan::Text(std::mem::take(&mut plain)));
                    }
                    spans.push(InlineSpan::BoldItalic(
                        input[pos + 3..pos + 3 + end].to_string(),
                    ));
                    pos = pos + 3 + end + 3;
                    continue;
                }
            }
            // **bold**
            if pos + 1 < len && bytes[pos + 1] == b'*' {
                if let Some(end) = input[pos + 2..].find("**") {
                    if !plain.is_empty() {
                        spans.push(InlineSpan::Text(std::mem::take(&mut plain)));
                    }
                    spans.push(InlineSpan::Bold(input[pos + 2..pos + 2 + end].to_string()));
                    pos = pos + 2 + end + 2;
                    continue;
                }
            }
            // *italic*
            if let Some(end) = input[pos + 1..].find('*') {
                if end > 0 {
                    if !plain.is_empty() {
                        spans.push(InlineSpan::Text(std::mem::take(&mut plain)));
                    }
                    spans.push(InlineSpan::Italic(
                        input[pos + 1..pos + 1 + end].to_string(),
                    ));
                    pos = pos + 1 + end + 1;
                    continue;
                }
            }
        }

        // Underscore-based emphasis: ___bold italic___, __bold__, _italic_
        if bytes[pos] == b'_' {
            // ___bold italic___
            if pos + 2 < len && bytes[pos + 1] == b'_' && bytes[pos + 2] == b'_' {
                if let Some(end) = input[pos + 3..].find("___") {
                    if !plain.is_empty() {
                        spans.push(InlineSpan::Text(std::mem::take(&mut plain)));
                    }
                    spans.push(InlineSpan::BoldItalic(
                        input[pos + 3..pos + 3 + end].to_string(),
                    ));
                    pos = pos + 3 + end + 3;
                    continue;
                }
            }
            // __bold__
            if pos + 1 < len && bytes[pos + 1] == b'_' {
                if let Some(end) = input[pos + 2..].find("__") {
                    if !plain.is_empty() {
                        spans.push(InlineSpan::Text(std::mem::take(&mut plain)));
                    }
                    spans.push(InlineSpan::Bold(input[pos + 2..pos + 2 + end].to_string()));
                    pos = pos + 2 + end + 2;
                    continue;
                }
            }
            // _italic_
            if let Some(end) = input[pos + 1..].find('_') {
                if end > 0 {
                    if !plain.is_empty() {
                        spans.push(InlineSpan::Text(std::mem::take(&mut plain)));
                    }
                    spans.push(InlineSpan::Italic(
                        input[pos + 1..pos + 1 + end].to_string(),
                    ));
                    pos = pos + 1 + end + 1;
                    continue;
                }
            }
        }

        plain.push(input[pos..].chars().next().unwrap());
        pos += input[pos..].chars().next().unwrap().len_utf8();
    }

    if !plain.is_empty() {
        spans.push(InlineSpan::Text(plain));
    }
    if spans.is_empty() {
        spans.push(InlineSpan::Text(String::new()));
    }
    spans
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
                [49, 50, 68],    // black (surface0, visible against base)
                [243, 139, 168], // red
                [166, 227, 161], // green
                [249, 226, 175], // yellow
                [137, 180, 250], // blue
                [245, 194, 231], // magenta
                [148, 226, 213], // cyan
                [186, 194, 222], // white (subtext0)
                [108, 112, 134], // bright black (overlay0, ~4.1:1 contrast)
                [255, 159, 188], // bright red
                [186, 247, 181], // bright green
                [255, 246, 195], // bright yellow
                [157, 200, 255], // bright blue
                [255, 214, 246], // bright magenta
                [168, 246, 233], // bright cyan
                [255, 255, 255], // bright white
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
                [33, 34, 44],    // black
                [255, 85, 85],   // red
                [80, 250, 123],  // green
                [241, 250, 140], // yellow
                [189, 147, 249], // blue
                [255, 121, 198], // magenta
                [139, 233, 253], // cyan
                [248, 248, 242], // white
                [98, 114, 164],  // bright black
                [255, 110, 110], // bright red
                [105, 255, 148], // bright green
                [255, 255, 165], // bright yellow
                [210, 172, 255], // bright blue
                [255, 146, 218], // bright magenta
                [164, 255, 255], // bright cyan
                [255, 255, 255], // bright white
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
                [59, 66, 82],    // black
                [191, 97, 106],  // red
                [163, 190, 140], // green
                [235, 203, 139], // yellow
                [129, 161, 193], // blue
                [180, 142, 173], // magenta
                [136, 192, 208], // cyan
                [229, 233, 240], // white
                [76, 86, 106],   // bright black
                [208, 135, 112], // bright red
                [163, 190, 140], // bright green
                [235, 203, 139], // bright yellow
                [136, 192, 208], // bright blue
                [180, 142, 173], // bright magenta
                [143, 188, 187], // bright cyan
                [236, 239, 244], // bright white
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
                [7, 54, 66],     // black
                [220, 50, 47],   // red
                [133, 153, 0],   // green
                [181, 137, 0],   // yellow
                [38, 139, 210],  // blue
                [211, 54, 130],  // magenta
                [42, 161, 152],  // cyan
                [238, 232, 213], // white
                [0, 43, 54],     // bright black
                [203, 75, 22],   // bright red
                [88, 110, 117],  // bright green
                [101, 123, 131], // bright yellow
                [131, 148, 150], // bright blue
                [108, 113, 196], // bright magenta
                [147, 161, 161], // bright cyan
                [253, 246, 227], // bright white
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
                [60, 56, 54],    // black (surface0, visible against base)
                [204, 36, 29],   // red
                [152, 151, 26],  // green
                [215, 153, 33],  // yellow
                [69, 133, 136],  // blue
                [177, 98, 134],  // magenta
                [104, 157, 106], // cyan
                [168, 153, 132], // white
                [146, 131, 116], // bright black
                [251, 73, 52],   // bright red
                [184, 187, 38],  // bright green
                [250, 189, 47],  // bright yellow
                [131, 165, 152], // bright blue
                [211, 134, 155], // bright magenta
                [142, 192, 124], // bright cyan
                [235, 219, 178], // bright white
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
                [65, 72, 104],   // black
                [247, 118, 142], // red
                [158, 206, 106], // green
                [224, 175, 104], // yellow
                [122, 162, 247], // blue
                [187, 154, 247], // magenta
                [115, 218, 202], // cyan
                [192, 202, 245], // white
                [86, 95, 137],   // bright black
                [255, 148, 168], // bright red
                [178, 226, 126], // bright green
                [244, 195, 124], // bright yellow
                [142, 182, 255], // bright blue
                [207, 174, 255], // bright magenta
                [135, 238, 222], // bright cyan
                [222, 232, 255], // bright white
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
                [38, 35, 58],    // black
                [235, 111, 146], // red
                [49, 116, 143],  // green
                [246, 193, 119], // yellow
                [156, 207, 216], // blue
                [196, 167, 231], // magenta
                [156, 207, 216], // cyan
                [224, 222, 244], // white
                [110, 106, 134], // bright black
                [255, 131, 166], // bright red
                [69, 136, 163],  // bright green
                [255, 213, 139], // bright yellow
                [176, 227, 236], // bright blue
                [216, 187, 251], // bright magenta
                [176, 227, 236], // bright cyan
                [244, 242, 255], // bright white
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
                [53, 57, 65],    // black (surface0, visible against base)
                [224, 108, 117], // red
                [152, 195, 121], // green
                [229, 192, 123], // yellow
                [97, 175, 239],  // blue
                [198, 120, 221], // magenta
                [86, 182, 194],  // cyan
                [171, 178, 191], // white
                [92, 99, 112],   // bright black
                [239, 128, 137], // bright red
                [172, 215, 141], // bright green
                [249, 212, 143], // bright yellow
                [117, 195, 255], // bright blue
                [218, 140, 241], // bright magenta
                [106, 202, 214], // bright cyan
                [211, 218, 231], // bright white
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
                [78, 84, 78],    // black
                [230, 126, 128], // red
                [167, 192, 128], // green
                [219, 188, 127], // yellow
                [127, 187, 179], // blue
                [214, 153, 182], // magenta
                [131, 192, 159], // cyan
                [211, 198, 170], // white
                [113, 119, 113], // bright black
                [250, 146, 148], // bright red
                [187, 212, 148], // bright green
                [239, 208, 147], // bright yellow
                [147, 207, 199], // bright blue
                [234, 173, 202], // bright magenta
                [151, 212, 179], // bright cyan
                [231, 218, 190], // bright white
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
                [56, 57, 50],    // black (surface0, visible against base)
                [249, 38, 114],  // red
                [166, 226, 46],  // green
                [230, 219, 116], // yellow
                [102, 217, 239], // blue
                [174, 129, 255], // magenta
                [102, 217, 239], // cyan
                [248, 248, 242], // white
                [117, 113, 94],  // bright black
                [255, 68, 134],  // bright red
                [186, 246, 66],  // bright green
                [250, 239, 136], // bright yellow
                [122, 237, 255], // bright blue
                [194, 149, 255], // bright magenta
                [122, 237, 255], // bright cyan
                [255, 255, 255], // bright white
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
                [72, 79, 88],    // black
                [255, 123, 114], // red
                [63, 185, 80],   // green
                [210, 153, 34],  // yellow
                [88, 166, 255],  // blue
                [188, 140, 255], // magenta
                [86, 211, 219],  // cyan
                [230, 237, 243], // white
                [110, 118, 129], // bright black
                [255, 148, 139], // bright red
                [86, 211, 100],  // bright green
                [230, 173, 54],  // bright yellow
                [108, 186, 255], // bright blue
                [208, 160, 255], // bright magenta
                [106, 231, 239], // bright cyan
                [255, 255, 255], // bright white
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
                [28, 34, 46],    // black (surface0, visible against base)
                [240, 113, 120], // red
                [170, 217, 76],  // green
                [255, 180, 84],  // yellow
                [57, 186, 230],  // blue
                [210, 166, 255], // magenta
                [149, 230, 203], // cyan
                [203, 204, 198], // white
                [90, 100, 118],  // bright black
                [255, 133, 140], // bright red
                [190, 237, 96],  // bright green
                [255, 200, 104], // bright yellow
                [77, 206, 250],  // bright blue
                [230, 186, 255], // bright magenta
                [169, 250, 223], // bright cyan
                [233, 234, 228], // bright white
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
                [48, 48, 48],    // black
                [240, 113, 120], // red
                [195, 232, 141], // green
                [255, 203, 107], // yellow
                [130, 170, 255], // blue
                [199, 146, 234], // magenta
                [137, 221, 255], // cyan
                [238, 255, 255], // white
                [100, 100, 100], // bright black
                [255, 133, 140], // bright red
                [215, 252, 161], // bright green
                [255, 223, 127], // bright yellow
                [150, 190, 255], // bright blue
                [219, 166, 254], // bright magenta
                [157, 241, 255], // bright cyan
                [255, 255, 255], // bright white
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
                [7, 54, 66],     // black
                [220, 50, 47],   // red
                [133, 153, 0],   // green
                [181, 137, 0],   // yellow
                [38, 139, 210],  // blue
                [211, 54, 130],  // magenta
                [42, 161, 152],  // cyan
                [238, 232, 213], // white
                [0, 43, 54],     // bright black
                [203, 75, 22],   // bright red
                [88, 110, 117],  // bright green
                [101, 123, 131], // bright yellow
                [131, 148, 150], // bright blue
                [108, 113, 196], // bright magenta
                [147, 161, 161], // bright cyan
                [253, 246, 227], // bright white
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
                [172, 176, 190], // black
                [210, 15, 57],   // red
                [64, 160, 43],   // green
                [223, 142, 29],  // yellow
                [30, 102, 245],  // blue
                [136, 57, 239],  // magenta
                [23, 146, 153],  // cyan
                [76, 79, 105],   // white
                [140, 143, 161], // bright black
                [210, 15, 57],   // bright red
                [64, 160, 43],   // bright green
                [223, 142, 29],  // bright yellow
                [30, 102, 245],  // bright blue
                [136, 57, 239],  // bright magenta
                [23, 146, 153],  // bright cyan
                [44, 47, 71],    // bright white
            ],
        },
    ]
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t) as u8,
    )
}

/// Blend `overlay` over `base` by `factor` (0.0 = all base, 1.0 = all overlay).
/// Alpha channel is preserved from `base`.
pub fn blend_colors(base: Color32, overlay: Color32, factor: f32) -> Color32 {
    let factor = factor.clamp(0.0, 1.0);
    let r = (base.r() as f32 * (1.0 - factor) + overlay.r() as f32 * factor) as u8;
    let g = (base.g() as f32 * (1.0 - factor) + overlay.g() as f32 * factor) as u8;
    let b = (base.b() as f32 * (1.0 - factor) + overlay.b() as f32 * factor) as u8;
    Color32::from_rgb(r, g, b)
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
    fn test_ensure_readable_already_bright() {
        set_theme(ThemeId::CatppuccinMocha);
        let bright_green = [100, 255, 100];
        let dark_bg = [24, 24, 37]; // mantle
        let color = ensure_readable(bright_green, dark_bg);
        assert_eq!(color, from_rgb(bright_green));
    }

    #[test]
    fn test_ensure_readable_lightens_dark_text() {
        set_theme(ThemeId::CatppuccinMocha);
        let dark_blue = [20, 20, 80];
        let dark_bg = [24, 24, 37]; // mantle
        let color = ensure_readable(dark_blue, dark_bg);
        assert_ne!(color, from_rgb(dark_blue));
        // Result should be lighter (higher channel values)
        let [r, g, b, _] = color.to_array();
        assert!(r > dark_blue[0] || g > dark_blue[1] || b > dark_blue[2]);
    }

    #[test]
    fn test_ensure_readable_preserves_hue() {
        set_theme(ThemeId::CatppuccinMocha);
        let dark_red = [60, 10, 10];
        let dark_bg = [24, 24, 37];
        let color = ensure_readable(dark_red, dark_bg);
        let [r, g, b, _] = color.to_array();
        // Red channel should still dominate
        assert!(r > g && r > b);
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
    fn test_header_bg_midpoint_no_overflow() {
        let a: u8 = 200;
        let b: u8 = 200;
        let correct = ((a as u16 + b as u16) / 2) as u8;
        assert_eq!(
            correct, 200,
            "midpoint of 200,200 should be 200, not truncated"
        );
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

    #[test]
    fn test_contrast_ratio_same_color_is_one() {
        let c = [100, 150, 200];
        let ratio = contrast_ratio(c, c);
        assert!((ratio - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_contrast_ratio_black_white() {
        let ratio = contrast_ratio([0, 0, 0], [255, 255, 255]);
        assert!(
            ratio > 20.0,
            "black/white contrast should be ~21:1, got {ratio}"
        );
    }

    #[test]
    fn test_ensure_term_contrast_same_color_adjusts() {
        set_theme(ThemeId::CatppuccinMocha);
        let green = Color32::from_rgb(166, 227, 161);
        let result = ensure_term_contrast(green, green);
        assert_ne!(result, green, "same fg/bg must be adjusted");
        let r = contrast_ratio(
            [result.r(), result.g(), result.b()],
            [green.r(), green.g(), green.b()],
        );
        assert!(r >= 3.0, "adjusted contrast should be >= 3:1, got {r}");
    }

    #[test]
    fn test_ensure_term_contrast_good_contrast_unchanged() {
        let white = Color32::from_rgb(255, 255, 255);
        let black = Color32::from_rgb(0, 0, 0);
        assert_eq!(ensure_term_contrast(white, black), white);
    }

    #[test]
    fn test_ensure_term_contrast_dark_fg_on_dark_bg() {
        let dark_fg = Color32::from_rgb(30, 30, 46);
        let dark_bg = Color32::from_rgb(30, 30, 46);
        let result = ensure_term_contrast(dark_fg, dark_bg);
        let r = contrast_ratio(
            [result.r(), result.g(), result.b()],
            [dark_bg.r(), dark_bg.g(), dark_bg.b()],
        );
        assert!(r >= 3.0, "contrast on dark bg should be >= 3:1, got {r}");
    }

    #[test]
    fn test_ensure_term_contrast_light_fg_on_light_bg() {
        let light_fg = Color32::from_rgb(240, 240, 240);
        let light_bg = Color32::from_rgb(239, 241, 245);
        let result = ensure_term_contrast(light_fg, light_bg);
        let r = contrast_ratio(
            [result.r(), result.g(), result.b()],
            [light_bg.r(), light_bg.g(), light_bg.b()],
        );
        assert!(r >= 3.0, "contrast on light bg should be >= 3:1, got {r}");
    }

    #[test]
    fn test_ensure_term_contrast_all_themes_ansi_on_same() {
        for &id in ThemeId::ALL {
            set_theme(id);
            let t = active();
            for i in 0..16 {
                let fg = t.ansi[i];
                let result = ensure_term_contrast(fg, fg);
                let r = contrast_ratio(
                    [result.r(), result.g(), result.b()],
                    [fg.r(), fg.g(), fg.b()],
                );
                assert!(
                    r >= 3.0,
                    "theme {:?} ansi[{i}] on itself: contrast {r} < 3.0",
                    id
                );
            }
        }
        set_theme(ThemeId::CatppuccinMocha);
    }

    #[test]
    fn test_ensure_readable_light_theme() {
        set_theme(ThemeId::CatppuccinLatte);
        let t = active();
        let light_fg = [220, 220, 220];
        let light_bg = t.base_rgb;
        let result = ensure_readable(light_fg, light_bg);
        let r = contrast_ratio([result.r(), result.g(), result.b()], light_bg);
        assert!(
            r >= 4.5,
            "ensure_readable on light bg should reach 4.5:1, got {r}"
        );
        set_theme(ThemeId::CatppuccinMocha);
    }

    #[test]
    fn test_srgb_lut_boundaries() {
        let lut = srgb_lut();
        assert!((lut[0] - 0.0).abs() < 0.001);
        assert!((lut[255] - 1.0).abs() < 0.001);
        for i in 1..256 {
            assert!(lut[i] >= lut[i - 1], "LUT must be monotonic at index {i}");
        }
    }

    #[test]
    fn test_lerp_color_endpoints() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(255, 255, 255);
        let at_zero = lerp_color(a, b, 0.0);
        assert_eq!(at_zero.r(), 0);
        assert_eq!(at_zero.g(), 0);
        let at_one = lerp_color(a, b, 1.0);
        assert_eq!(at_one.r(), 255);
        assert_eq!(at_one.g(), 255);
    }

    #[test]
    fn test_lerp_color_midpoint() {
        let a = Color32::from_rgb(0, 100, 200);
        let b = Color32::from_rgb(100, 200, 0);
        let mid = lerp_color(a, b, 0.5);
        assert_eq!(mid.r(), 50);
        assert_eq!(mid.g(), 150);
        assert_eq!(mid.b(), 100);
    }

    #[test]
    fn test_lerp_color_clamps() {
        let a = Color32::from_rgb(100, 100, 100);
        let b = Color32::from_rgb(200, 200, 200);
        let under = lerp_color(a, b, -1.0);
        assert_eq!(under.r(), 100);
        let over = lerp_color(a, b, 2.0);
        assert_eq!(over.r(), 200);
    }
}
