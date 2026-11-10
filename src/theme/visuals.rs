//! egui visuals built from theme roles. This is the only path that feeds
//! `Context::set_visuals`.

use egui::{Rounding, Shadow, Stroke, Visuals};

use super::{Theme, R_LG, R_MD, STROKE_THIN};

pub fn visuals_for(t: &Theme) -> Visuals {
    let mut vis = if t.is_light {
        Visuals::light()
    } else {
        Visuals::dark()
    };
    vis.panel_fill = t.bg_panel;
    vis.window_fill = t.bg_raised;
    vis.window_rounding = Rounding::same(R_LG);
    vis.window_shadow = Shadow::NONE;
    vis.popup_shadow = Shadow::NONE;
    vis.widgets.noninteractive.bg_fill = t.surface0;
    vis.widgets.inactive.bg_fill = t.surface0;
    vis.widgets.hovered.bg_fill = t.bg_raised;
    vis.widgets.hovered.weak_bg_fill = t.bg_raised;
    vis.widgets.active.bg_fill = t.surface2;
    vis.widgets.inactive.fg_stroke = Stroke::new(STROKE_THIN, t.subtext0);
    vis.widgets.noninteractive.fg_stroke = Stroke::new(STROKE_THIN, t.overlay0);
    vis.selection.bg_fill = t.accent_bg;
    for state in [
        &mut vis.widgets.noninteractive,
        &mut vis.widgets.inactive,
        &mut vis.widgets.hovered,
        &mut vis.widgets.active,
        &mut vis.widgets.open,
    ] {
        state.rounding = Rounding::same(R_MD);
    }
    vis.override_text_color = Some(t.text);
    vis
}

#[cfg(test)]
mod tests {
    use super::super::test_support::theme_by_id;
    use super::super::ThemeId;
    use super::visuals_for;

    #[test]
    fn visuals_for_uses_roles() {
        let t = theme_by_id(ThemeId::Nord);
        let vis = visuals_for(t);
        assert_eq!(vis.panel_fill, t.bg_panel);
        assert_eq!(vis.window_fill, t.bg_raised);
        assert_eq!(vis.selection.bg_fill, t.accent_bg);
        assert_eq!(vis.widgets.hovered.weak_bg_fill, t.bg_raised);
    }

    #[test]
    fn visuals_for_preserves_light_theme_mode() {
        let t = theme_by_id(ThemeId::GitHubLight);
        assert!(!visuals_for(t).dark_mode);
    }
}
