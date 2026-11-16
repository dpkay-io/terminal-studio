//! Where the mouse pointer is on screen, asked of the OS directly.
//!
//! Needed while files are dragged in from another app: the OS drag session
//! owns the pointer then, so the window gets no mouse moves and winit (0.29)
//! reports the hovered and dropped files without a position. Reading the
//! pointer from the OS tells which part of the window a drop is over.
//!
//! macOS reads it through CoreGraphics, Windows through `GetCursorPos`, X11
//! through `XQueryPointer` (libX11 loaded at runtime, as winit does). Other
//! platforms, and an X11 display that can't be opened, give None.

/// The unit a screen position is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenUnit {
    /// Logical points (macOS): physical pixels divided by the display scale.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Points,
    /// Physical pixels (Windows, X11).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    Pixels,
}

/// The pointer's position on the virtual screen (origin top left of the
/// primary display, y down), or None when the OS can't say.
pub fn screen_position() -> Option<(f64, f64, ScreenUnit)> {
    imp::screen_position()
}

/// `screen` (from [`screen_position`]) in egui points relative to a window
/// whose content starts at `inner_min` (egui's `ViewportInfo::inner_rect`,
/// in points). `native_ppp` is the display's scale and `ppp` egui's points
/// per pixel (the scale times the zoom).
pub fn to_window_points(
    screen: (f64, f64, ScreenUnit),
    inner_min: egui::Pos2,
    native_ppp: f32,
    ppp: f32,
) -> Option<egui::Pos2> {
    if !(ppp > 0.0 && native_ppp > 0.0) {
        return None;
    }
    let (x, y, unit) = screen;
    // egui's inner rect is the window's physical position over `ppp`; bring
    // the pointer to the same scale.
    let to_points = match unit {
        ScreenUnit::Pixels => 1.0 / ppp,
        ScreenUnit::Points => native_ppp / ppp,
    };
    let pos = egui::pos2(x as f32 * to_points, y as f32 * to_points);
    let local = pos - inner_min.to_vec2();
    (local.x.is_finite() && local.y.is_finite()).then_some(local)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::ScreenUnit;
    use std::ffi::c_void;

    #[repr(C)]
    struct CgPoint {
        x: f64,
        y: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *const c_void) -> CgPoint;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(object: *const c_void);
    }

    /// An event made with no source carries the pointer's current location,
    /// in global display coordinates (points, top-left origin) — the same
    /// space winit places windows in. Needs no permission.
    pub(super) fn screen_position() -> Option<(f64, f64, ScreenUnit)> {
        // SAFETY: CGEventCreate accepts a null source and returns an owned
        // event or null; the event is only read while alive and released
        // exactly once.
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return None;
            }
            let point = CGEventGetLocation(event);
            CFRelease(event);
            Some((point.x, point.y, ScreenUnit::Points))
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::ScreenUnit;
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    /// Physical pixels: winit makes the process per-monitor DPI aware.
    pub(super) fn screen_position() -> Option<(f64, f64, ScreenUnit)> {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: GetCursorPos writes one POINT through a valid pointer.
        let ok = unsafe { GetCursorPos(&mut point) };
        (ok != 0).then(|| (f64::from(point.x), f64::from(point.y), ScreenUnit::Pixels))
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::ScreenUnit;
    use x11_dl::xlib;

    /// One display connection for the UI thread, opened on first use.
    struct Connection {
        lib: xlib::Xlib,
        display: *mut xlib::Display,
    }

    impl Drop for Connection {
        fn drop(&mut self) {
            // SAFETY: `display` came from XOpenDisplay and is closed once.
            unsafe {
                (self.lib.XCloseDisplay)(self.display);
            }
        }
    }

    thread_local! {
        static CONNECTION: Option<Connection> = open();
    }

    fn open() -> Option<Connection> {
        let lib = xlib::Xlib::open().ok()?;
        // SAFETY: a null name opens the display named by $DISPLAY, or fails
        // with null.
        let display = unsafe { (lib.XOpenDisplay)(std::ptr::null()) };
        (!display.is_null()).then_some(Connection { lib, display })
    }

    /// Physical pixels on the X screen. Under Wayland this is XWayland's
    /// idea of the pointer, but winit reports no file drops there anyway.
    pub(super) fn screen_position() -> Option<(f64, f64, ScreenUnit)> {
        CONNECTION.with(|connection| {
            let c = connection.as_ref()?;
            let (mut root, mut child) = (0, 0);
            let (mut root_x, mut root_y, mut win_x, mut win_y) = (0, 0, 0, 0);
            let mut mask = 0;
            // SAFETY: a live display, its default root window, and valid
            // out-pointers for every result.
            let found = unsafe {
                let root_window = (c.lib.XDefaultRootWindow)(c.display);
                (c.lib.XQueryPointer)(
                    c.display,
                    root_window,
                    &mut root,
                    &mut child,
                    &mut root_x,
                    &mut root_y,
                    &mut win_x,
                    &mut win_y,
                    &mut mask,
                )
            };
            (found != 0).then(|| (f64::from(root_x), f64::from(root_y), ScreenUnit::Pixels))
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod imp {
    use super::ScreenUnit;

    pub(super) fn screen_position() -> Option<(f64, f64, ScreenUnit)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_are_scaled_to_points_and_made_window_relative() {
        // A 2x display, window content at (100, 50) points.
        let local = to_window_points(
            (300.0, 200.0, ScreenUnit::Pixels),
            egui::pos2(100.0, 50.0),
            2.0,
            2.0,
        );
        assert_eq!(local, Some(egui::pos2(50.0, 50.0)));
    }

    #[test]
    fn points_follow_the_zoom() {
        // macOS points on a 2x display with egui zoomed to 1.25x: the inner
        // rect is in zoomed points, so the pointer is too.
        let local = to_window_points(
            (250.0, 125.0, ScreenUnit::Points),
            egui::pos2(0.0, 0.0),
            2.0,
            2.5,
        );
        assert_eq!(local, Some(egui::pos2(200.0, 100.0)));
    }

    #[test]
    fn no_scale_no_position() {
        let screen = (1.0, 1.0, ScreenUnit::Pixels);
        assert_eq!(to_window_points(screen, egui::Pos2::ZERO, 0.0, 1.0), None);
        assert_eq!(to_window_points(screen, egui::Pos2::ZERO, 1.0, 0.0), None);
        assert_eq!(
            to_window_points(screen, egui::Pos2::ZERO, f32::NAN, 1.0),
            None
        );
    }

    #[test]
    fn the_os_reports_a_finite_pointer_or_nothing() {
        // Headless CI may have no pointer at all; when there is one it must
        // be a real number.
        if let Some((x, y, _)) = screen_position() {
            assert!(x.is_finite() && y.is_finite());
        }
    }
}
