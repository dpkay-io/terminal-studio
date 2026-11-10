//! Test-only helpers for the process-wide active theme.
//!
//! `set_theme` mutates a process-global index and `cargo test` runs tests in
//! parallel. [`lock_theme`] serializes those tests and restores the default
//! theme when the guard drops.

use super::{all_themes, contrast_ratio, set_theme, Theme, ThemeId};
use egui::Color32;
use parking_lot::{Mutex, MutexGuard};

static THEME_LOCK: Mutex<()> = Mutex::new(());

/// Holds the process-wide theme lock and restores Catppuccin Mocha on drop.
pub struct ThemeGuard {
    _guard: MutexGuard<'static, ()>,
}

impl Drop for ThemeGuard {
    fn drop(&mut self) {
        set_theme(ThemeId::CatppuccinMocha);
    }
}

/// Lock the global theme for this test and select `id`.
pub fn lock_theme(id: ThemeId) -> ThemeGuard {
    let guard = THEME_LOCK.lock();
    set_theme(id);
    ThemeGuard { _guard: guard }
}

/// Theme for `id` without mutating the process-wide active theme.
pub fn theme_by_id(id: ThemeId) -> &'static Theme {
    &all_themes()[id.index()]
}

/// Opaque sRGB color from `0xRRGGBB`.
pub fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb(
        ((rgb >> 16) & 0xff) as u8,
        ((rgb >> 8) & 0xff) as u8,
        (rgb & 0xff) as u8,
    )
}

/// WCAG contrast ratio of two colors. Alpha is ignored.
pub fn ratio(a: Color32, b: Color32) -> f32 {
    contrast_ratio([a.r(), a.g(), a.b()], [b.r(), b.g(), b.b()])
}
