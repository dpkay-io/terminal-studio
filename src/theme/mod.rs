#![allow(dead_code)]
mod palettes;
mod roles;
#[cfg(test)]
pub(crate) mod test_support;
mod visuals;

pub use visuals::visuals_for;

use egui::Color32;
use palettes::{all_defs, ThemeDef};
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
    GitHubLight,
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
        ThemeId::GitHubLight,
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
            ThemeId::GitHubLight => "GitHub Light",
        }
    }
}

// ── Expanded theme (ready for UI consumption) ────────────────────────────────

pub struct Theme {
    pub id: ThemeId,
    pub is_light: bool,

    // Color roles. Legacy fields below stay as aliases of these.
    pub bg_base: Color32,
    pub bg_panel: Color32,
    pub bg_raised: Color32,
    pub border: Color32,
    pub fg: Color32,
    pub fg_strong: Color32,
    pub fg_muted_raised: Color32,
    pub accent_bg: Color32,
    pub danger: Color32,
    pub danger_text: Color32,

    // Raw RGB values
    pub mantle_rgb: [u8; 3],
    pub base_rgb: [u8; 3],
    pub blue_rgb: [u8; 3],

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
    pub bg_workspace_fill: Color32,
    pub bg_term: Color32,
    pub bg_row_active: Color32,
    pub bg_row_hover: Color32,

    // Semantic foregrounds
    pub fg_secondary: Color32,
    pub fg_muted: Color32,
    pub fg_path: Color32,
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
    pub danger_fg: Color32,

    // Git
    pub git_added: Color32,
    pub git_removed: Color32,
    pub git_modified: Color32,
    pub git_renamed: Color32,
    pub git_untracked: Color32,
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
    pub md_heading: Color32,
    pub md_heading_rule: Color32,
    pub md_blockquote_bg: Color32,

    // Terminal
    pub ansi: [Color32; 16],
    pub cursor_color: Color32,
    pub cursor_dim_color: Color32,
    pub selection_bg: Color32,
    pub scrollbar_color: Color32,

    // UI refresh: component backgrounds
    pub bg_input: Color32,

    // UI refresh: borders
    pub border_subtle: Color32,
    pub border_focus: Color32,

    // UI refresh: shadows (simulated via darker strokes)
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
        let md_code_bg = blend(self.base, self.surface0, BLEND_MEDIUM);

        let roles = roles::derive_roles(self);
        let accent_rgb = [roles.accent.r(), roles.accent.g(), roles.accent.b()];
        let accent_muted = blend(self.base, accent_rgb, 0.2);
        let accent_strong = blend(accent_rgb, self.text, BLEND_LIGHT);
        let flash_bg = blend(self.base, self.blue, BLEND_SUBTLE);
        let flash_success_bg = blend(self.base, self.green, BLEND_SUBTLE);
        let flash_error_bg = blend(self.base, self.red, BLEND_SUBTLE);

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
        let border_subtle_rgb = blend(self.surface0, self.surface1, BLEND_MEDIUM);

        Theme {
            id: self.id,
            is_light: self.is_light,

            bg_base: roles.bg_base,
            bg_panel: roles.bg_panel,
            bg_raised: roles.bg_raised,
            border: roles.border,
            fg: roles.fg,
            fg_strong: roles.fg_strong,
            fg_muted_raised: roles.fg_muted_raised,
            accent_bg: roles.accent_bg,
            danger: roles.danger,
            danger_text: roles.danger_text,

            mantle_rgb: self.mantle,
            base_rgb: self.base,
            blue_rgb: self.blue,

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

            bg_workspace_fill: c(bg_workspace_fill),
            bg_term: c(self.base),
            bg_row_active: c(self.surface0),
            bg_row_hover: c(bg_row_hover),

            fg_secondary: c(self.subtext0),
            fg_muted: roles.fg_muted,
            fg_path: c(self.blue),
            fg_md_file: c(self.green),
            fg_other_file: c(self.subtext1),

            accent: roles.accent,
            accent_muted: c(accent_muted),
            accent_strong: c(accent_strong),
            success: c(self.green),
            warning: c(self.yellow),
            error: c(self.red),

            flash_bg: c(flash_bg),
            flash_success_bg: c(flash_success_bg),
            flash_error_bg: c(flash_error_bg),

            danger_fg: c(danger_fg),

            git_added: c(self.green),
            git_removed: c(self.red),
            git_modified: c(self.yellow),
            git_renamed: c(self.mauve),
            git_untracked: c(self.overlay0),
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
            md_heading: c(blend(self.text, self.blue, 0.25)),
            md_heading_rule: c(blend(self.surface0, self.surface1, 0.6)),
            md_blockquote_bg: c(blend(self.base, self.surface0, 0.35)),

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

            border_subtle: c(border_subtle_rgb),
            border_focus: Color32::from_rgba_unmultiplied(
                self.blue[0],
                self.blue[1],
                self.blue[2],
                ALPHA_BORDER_FOCUS,
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
pub const SYSMON_W: f32 = 204.0;
pub const SYSMON_BAR_W: f32 = 40.0;
pub const SYSMON_BAR_H: f32 = 4.0;
pub const SYSMON_CONTENT_H: f32 = 11.0;
pub const UPDATE_BTN_W: f32 = 110.0;
pub const HEADER_H: f32 = 26.0;
pub const INFO_BAR_H: f32 = 22.0;
pub const DIVIDER_W: f32 = 6.0;
pub const MIN_PANE_W: f32 = 80.0;
pub const MIN_PANE_H: f32 = 80.0;
pub const BTN_W: f32 = 24.0;
pub const SESSION_ROW_H: f32 = 26.0;
pub const SEARCH_BAR_H: f32 = 22.0;
pub const FILE_ROW_H: f32 = 22.0;
pub const TREE_CHEVRON_W: f32 = 16.0;
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
pub const TAB_W_ACTIVE: f32 = 200.0;
pub const TAB_COLOR_STRIP_W: f32 = 3.0;
pub const TAB_ACTIVE_HIGHLIGHT_H: f32 = 2.0;
pub const TAB_PAD_X: f32 = 8.0;
pub const TAB_ACTIONS_W: f32 = 82.0;
pub const TAB_ACTION_GAP: f32 = 2.0;
pub const TAB_SELECTED_BAR_H: f32 = 2.0;

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
// Chrome uses exactly four sizes (11 / 12 / 13 / 14). Nothing in chrome is
// below 11. `FONT_HEADING_1` and `FONT_HEADING_2` are markdown preview only.

pub const FONT_XS: f32 = 11.0;
pub const FONT_SM: f32 = 12.0;
pub const FONT_MD: f32 = 13.0;
pub const FONT_LG: f32 = 14.0;
pub const FONT_TERM: f32 = 14.0;
// Terminal line-height multiplier. Defined for the renderer; not applied here.
pub const TERM_LINE_HEIGHT: f32 = 1.3;

// Markdown preview only.
pub const FONT_HEADING_1: f32 = 22.0;
// Markdown preview only.
pub const FONT_HEADING_2: f32 = 18.0;

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
pub const TOGGLE_W: f32 = 36.0;
pub const TOGGLE_H: f32 = 20.0;

// ── Dialog layout ──────────────────────────────────────────────────────────

pub const DIALOG_MARGIN: f32 = SP_6;
pub const DIALOG_ITEM_H: f32 = 26.0;
pub const DIALOG_BTN_H: f32 = 34.0;
pub const DIALOG_TOP_OFFSET: f32 = 80.0;
pub const QUICK_SWITCHER_W: f32 = 640.0;
pub const QUICK_SWITCHER_MAX_H: f32 = 560.0;
pub const QUICK_SWITCHER_ROW_H: f32 = 44.0;
pub const QUIT_DIALOG_W: f32 = 460.0;
pub const QUIT_DIALOG_MAX_H: f32 = 360.0;
pub const QUIT_TAB_CHIP_MAX_W: f32 = 128.0;

// ── Panel divider ──────────────────────────────────────────────────────────

pub const PANEL_DIV_H: f32 = 8.0;

// ── Git row ────────────────────────────────────────────────────────────────

pub const GIT_ROW_H: f32 = 14.0;

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
    fn switcher_width_is_640() {
        assert_eq!(QUICK_SWITCHER_W, 640.0);
    }

    #[test]
    fn test_tinted_at_zero_returns_base() {
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
        set_theme(ThemeId::CatppuccinMocha);
        let result = tinted([255, 0, 0], 0.0);
        assert_eq!(result, active().base_rgb);
    }

    #[test]
    fn test_tinted_at_one_returns_color() {
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
        set_theme(ThemeId::CatppuccinMocha);
        let c = [200u8, 100, 50];
        let result = tinted(c, 1.0);
        assert_eq!(result, c);
    }

    #[test]
    fn test_tinted_midpoint() {
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
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
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
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
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
        set_theme(ThemeId::CatppuccinMocha);
        let bright_green = [100, 255, 100];
        let dark_bg = [24, 24, 37]; // mantle
        let color = ensure_readable(bright_green, dark_bg);
        assert_eq!(color, from_rgb(bright_green));
    }

    #[test]
    fn test_ensure_readable_lightens_dark_text() {
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
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
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
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
    fn test_theme_switching() {
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
        set_theme(ThemeId::CatppuccinMocha);
        let mocha_text = active().text;
        set_theme(ThemeId::Dracula);
        let dracula_text = active().text;
        assert_ne!(mocha_text, dracula_text);
        set_theme(ThemeId::CatppuccinMocha);
    }

    #[test]
    fn test_all_theme_ids_produce_valid_themes() {
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
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
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
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
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinMocha);
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
        let _guard = super::test_support::lock_theme(ThemeId::CatppuccinLatte);
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

    #[test]
    fn type_scale_is_four_sizes() {
        assert_eq!(FONT_XS, 11.0);
        assert_eq!(FONT_SM, 12.0);
        assert_eq!(FONT_MD, 13.0);
        assert_eq!(FONT_LG, 14.0);
        // `assert!` on a const is denied by clippy; a const assert keeps the floor.
        const _: () = {
            assert!(FONT_XS >= 11.0);
        };
        assert_eq!(FONT_TERM, 14.0);
        assert_eq!(TERM_LINE_HEIGHT, 1.3);
    }
}
