//! Which GPU renderers startup tries, and when trying the next one can help.

use eframe::Renderer;

/// Tried in order when `--renderer=` does not force one: wgpu, then glow
/// (OpenGL) for machines whose drivers wgpu cannot use.
const DEFAULT_RENDERERS: [Renderer; 2] = [Renderer::Wgpu, Renderer::Glow];

const RENDERER_FLAG: &str = "--renderer=";

/// The renderer `--renderer=<name>` forces, if the arguments name a known one.
pub(crate) fn forced_renderer(args: impl IntoIterator<Item = String>) -> Option<Renderer> {
    let arg = args
        .into_iter()
        .find_map(|arg| arg.strip_prefix(RENDERER_FLAG).map(str::to_ascii_lowercase))?;
    match arg.as_str() {
        "glow" | "gl" | "opengl" => Some(Renderer::Glow),
        "wgpu" => Some(Renderer::Wgpu),
        _ => None,
    }
}

/// The renderers to try, in order.
pub(crate) fn renderers_to_try(forced: Option<Renderer>) -> Vec<Renderer> {
    match forced {
        Some(renderer) => vec![renderer],
        None => DEFAULT_RENDERERS.to_vec(),
    }
}

/// Whether another renderer could start where this error stopped one.
///
/// Only a renderer's own failure qualifies. A window system that cannot
/// start (no display, for example) or an app that fails to build fails every
/// renderer alike, and winit refuses to create a second event loop after a
/// failed first, so retrying would only replace the real error with
/// "EventLoop can't be recreated".
pub(crate) fn another_renderer_may_help(err: &eframe::Error) -> bool {
    !matches!(
        err,
        eframe::Error::WinitEventLoop(_) | eframe::Error::AppCreation(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn wgpu_then_glow_by_default() {
        assert_eq!(renderers_to_try(None), vec![Renderer::Wgpu, Renderer::Glow]);
    }

    #[test]
    fn a_forced_renderer_is_the_only_one_tried() {
        assert_eq!(renderers_to_try(Some(Renderer::Glow)), vec![Renderer::Glow]);
    }

    #[test]
    fn renderer_flag_names_are_case_insensitive() {
        for name in ["glow", "GL", "OpenGL"] {
            let flag = format!("{RENDERER_FLAG}{name}");
            assert_eq!(
                forced_renderer(args(&["ts", &flag])),
                Some(Renderer::Glow),
                "{name}"
            );
        }
        assert_eq!(
            forced_renderer(args(&["ts", "--renderer=WGPU"])),
            Some(Renderer::Wgpu)
        );
    }

    #[test]
    fn unknown_or_absent_renderer_flag_forces_nothing() {
        assert_eq!(forced_renderer(args(&["ts", "--renderer=vulkan"])), None);
        assert_eq!(forced_renderer(args(&["ts", "--no-singleton"])), None);
    }

    #[test]
    fn a_renderer_failure_falls_back() {
        let err = eframe::Error::Wgpu(eframe::egui_wgpu::WgpuError::NoSuitableAdapterFound);
        assert!(another_renderer_may_help(&err));
    }

    #[test]
    fn a_window_system_that_cannot_start_does_not_fall_back() {
        let err = eframe::Error::WinitEventLoop(winit::error::EventLoopError::RecreationAttempt);
        assert!(!another_renderer_may_help(&err));
        let err = eframe::Error::WinitEventLoop(winit::error::EventLoopError::ExitFailure(1));
        assert!(!another_renderer_may_help(&err));
    }

    #[test]
    fn an_app_that_fails_to_build_does_not_fall_back() {
        let err = eframe::Error::AppCreation("settings unreadable".into());
        assert!(!another_renderer_may_help(&err));
    }
}
