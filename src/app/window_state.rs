//! Platform-correct reads of, and commands for, the OS window state.
//!
//! egui-winit only refreshes `ViewportInfo::minimized` and `maximized` at
//! start-up on macOS (reading them later can deadlock), yet sending
//! `ViewportCommand::Minimized(true)` still writes `true` into the cached
//! flag. Nothing ever clears it, so on macOS that flag must not be trusted.
//! `inner_rect` is recomputed every frame from the live window, and egui-winit
//! leaves it empty exactly while the window is miniaturized.

use egui::{Context, ViewportCommand, ViewportInfo};

const IS_MAC: bool = cfg!(target_os = "macos");

pub(super) fn is_minimized(info: &ViewportInfo) -> bool {
    minimized_on(info, IS_MAC)
}

fn minimized_on(info: &ViewportInfo, is_mac: bool) -> bool {
    if is_mac {
        info.inner_rect.is_none()
    } else {
        info.minimized.unwrap_or(false)
    }
}

/// `fullscreen` is refreshed every frame on all platforms, so it is reliable.
pub(super) fn is_fullscreen(info: &ViewportInfo) -> bool {
    info.fullscreen.unwrap_or(false)
}

fn is_maximized(info: &ViewportInfo) -> bool {
    info.maximized.unwrap_or(false)
}

/// The app's own edge-resize handles, for a floating window. macOS resizes
/// the window natively (winit cannot start a resize drag there), so the app
/// stays out of the way.
pub(super) fn allows_edge_resize(info: &ViewportInfo) -> bool {
    allows_edge_resize_on(info, IS_MAC)
}

fn allows_edge_resize_on(info: &ViewportInfo, is_mac: bool) -> bool {
    !is_mac && !is_maximized(info) && !is_fullscreen(info)
}

fn current(ctx: &Context, read: impl Fn(&ViewportInfo) -> bool) -> bool {
    ctx.input(|i| read(i.viewport()))
}

/// On macOS, AppKit's own zoom, which knows the live state even after other
/// apps resize the window. Elsewhere egui's flag is refreshed every frame.
#[cfg(target_os = "macos")]
pub(super) fn toggle_maximized(_ctx: &Context) {
    crate::macos_objc::zoom_key_window();
}

#[cfg(not(target_os = "macos"))]
pub(super) fn toggle_maximized(ctx: &Context) {
    let maximized = current(ctx, is_maximized);
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
}

pub(super) fn toggle_fullscreen(ctx: &Context) {
    let fullscreen = current(ctx, is_fullscreen);
    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!fullscreen));
}

/// A fullscreen window cannot be minimized, so the request is ignored there.
pub(super) fn minimize(ctx: &Context) {
    if !current(ctx, is_fullscreen) {
        ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: bool = true;
    const OTHER: bool = false;

    fn on_screen_rect() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0))
    }

    fn info(minimized: Option<bool>, rect: Option<egui::Rect>) -> ViewportInfo {
        ViewportInfo {
            minimized,
            inner_rect: rect,
            ..Default::default()
        }
    }

    #[test]
    fn mac_ignores_stale_minimized_flag_after_restore() {
        let restored = info(Some(true), Some(on_screen_rect()));
        assert!(!minimized_on(&restored, MAC));
    }

    #[test]
    fn mac_reports_minimized_while_window_has_no_rect() {
        let miniaturized = info(Some(true), None);
        assert!(minimized_on(&miniaturized, MAC));
    }

    #[test]
    fn mac_detects_minimize_not_requested_by_the_app() {
        // Dock or Cmd+M minimize never touches the cached flag.
        let via_dock = info(Some(false), None);
        assert!(minimized_on(&via_dock, MAC));
    }

    #[test]
    fn other_platforms_trust_the_live_flag() {
        assert!(minimized_on(
            &info(Some(true), Some(on_screen_rect())),
            OTHER
        ));
        assert!(!minimized_on(&info(Some(false), None), OTHER));
        assert!(!minimized_on(&info(None, None), OTHER));
    }

    #[test]
    fn edge_resize_only_for_floating_windows() {
        let with = |maximized, fullscreen| ViewportInfo {
            maximized: Some(maximized),
            fullscreen: Some(fullscreen),
            ..Default::default()
        };
        assert!(allows_edge_resize_on(&with(false, false), OTHER));
        assert!(!allows_edge_resize_on(&with(true, false), OTHER));
        assert!(!allows_edge_resize_on(&with(false, true), OTHER));
    }

    #[test]
    fn mac_leaves_resizing_to_appkit() {
        assert!(!allows_edge_resize_on(&ViewportInfo::default(), MAC));
    }

    #[test]
    fn missing_state_defaults_to_floating_and_windowed() {
        let unknown = ViewportInfo::default();
        assert!(!is_fullscreen(&unknown));
        assert!(allows_edge_resize_on(&unknown, OTHER));
    }
}
