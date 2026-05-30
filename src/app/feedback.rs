use egui::Color32;
use std::time::Instant;

use crate::theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashTarget {
    Pane(u32),
    Tab(u32),
    Global,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashKind {
    Neutral,
    Success,
    Error,
}

struct FlashEntry {
    target: FlashTarget,
    kind: FlashKind,
    start: Instant,
}

pub struct FlashManager {
    flashes: Vec<FlashEntry>,
}

impl FlashManager {
    pub fn new() -> Self {
        Self {
            flashes: Vec::new(),
        }
    }

    pub fn trigger(&mut self, target: FlashTarget, kind: FlashKind) {
        self.flashes.retain(|f| f.target != target);
        self.flashes.push(FlashEntry {
            target,
            kind,
            start: Instant::now(),
        });
    }

    pub fn tick(&mut self) {
        let duration = std::time::Duration::from_millis(theme::FLASH_DURATION_MS);
        self.flashes.retain(|f| f.start.elapsed() < duration);
    }

    pub fn flash_alpha(&self, target: FlashTarget) -> Option<(FlashKind, u8)> {
        let duration_ms = theme::FLASH_DURATION_MS as f32;
        self.flashes.iter().find(|f| f.target == target).map(|f| {
            let elapsed = f.start.elapsed().as_millis() as f32;
            let progress = (elapsed / duration_ms).min(1.0);
            let alpha = (theme::ALPHA_FLASH as f32 * (1.0 - progress)) as u8;
            (f.kind, alpha)
        })
    }

    pub fn flash_color(kind: FlashKind, alpha: u8) -> Color32 {
        let t = theme::active();
        let base = match kind {
            FlashKind::Neutral => t.flash_bg,
            FlashKind::Success => t.flash_success_bg,
            FlashKind::Error => t.flash_error_bg,
        };
        Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), alpha)
    }

    pub fn has_active(&self) -> bool {
        !self.flashes.is_empty()
    }

    pub fn render_on_rect(&self, painter: &egui::Painter, rect: egui::Rect, target: FlashTarget) {
        if let Some((kind, alpha)) = self.flash_alpha(target) {
            let color = Self::flash_color(kind, alpha);
            painter.rect_filled(rect, theme::R_NONE, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_trigger_and_tick() {
        let mut fm = FlashManager::new();
        fm.trigger(FlashTarget::Pane(1), FlashKind::Neutral);
        assert!(fm.has_active());
        assert!(fm.flash_alpha(FlashTarget::Pane(1)).is_some());
        assert!(fm.flash_alpha(FlashTarget::Pane(2)).is_none());
    }

    #[test]
    fn test_flash_expires() {
        let mut fm = FlashManager::new();
        fm.trigger(FlashTarget::Pane(1), FlashKind::Success);
        sleep(Duration::from_millis(theme::FLASH_DURATION_MS + 10));
        fm.tick();
        assert!(!fm.has_active());
    }

    #[test]
    fn test_alpha_decreases_over_time() {
        let mut fm = FlashManager::new();
        fm.trigger(FlashTarget::Global, FlashKind::Error);
        let (_, alpha1) = fm.flash_alpha(FlashTarget::Global).unwrap();
        sleep(Duration::from_millis(50));
        let (_, alpha2) = fm.flash_alpha(FlashTarget::Global).unwrap();
        assert!(alpha2 <= alpha1);
    }

    #[test]
    fn test_duplicate_target_replaces() {
        let mut fm = FlashManager::new();
        fm.trigger(FlashTarget::Tab(1), FlashKind::Neutral);
        fm.trigger(FlashTarget::Tab(1), FlashKind::Error);
        assert_eq!(fm.flashes.len(), 1);
        let (kind, _) = fm.flash_alpha(FlashTarget::Tab(1)).unwrap();
        assert_eq!(kind, FlashKind::Error);
    }

    #[test]
    fn test_flash_color_returns_valid_color() {
        crate::theme::set_theme(crate::theme::ThemeId::CatppuccinMocha);
        let color = FlashManager::flash_color(FlashKind::Neutral, 60);
        assert_eq!(color.a(), 60);
    }

    #[test]
    fn test_multiple_targets_independent() {
        let mut fm = FlashManager::new();
        fm.trigger(FlashTarget::Pane(1), FlashKind::Neutral);
        fm.trigger(FlashTarget::Pane(2), FlashKind::Success);
        fm.trigger(FlashTarget::Tab(1), FlashKind::Error);
        assert_eq!(fm.flashes.len(), 3);
        assert!(fm.flash_alpha(FlashTarget::Pane(1)).is_some());
        assert!(fm.flash_alpha(FlashTarget::Pane(2)).is_some());
        assert!(fm.flash_alpha(FlashTarget::Tab(1)).is_some());
    }
}
