//! Keeps the native title bar from sliding over the app's own title bar in
//! macOS full screen.
//!
//! The window has no native decorations, but AppKit only lets a window with
//! a title bar enter full screen, so winit gives it one for the switch. In
//! full screen AppKit moves that title bar (title and traffic lights) into a
//! strip of its own, a child window just below the menu bar, and slides it
//! down whenever the pointer reaches the top of the screen. It covers the
//! app's own title bar and its controls.
//!
//! The strip is left in place but made empty and click-through once the
//! window is in full screen: its contents are hidden, its background is
//! clear, and it ignores the mouse. AppKit can still show it; there is just
//! nothing to see or hit. The window's style is untouched, so entering and
//! leaving full screen work as before, and the menu bar is left to the
//! system. AppKit builds a new strip each time, so this runs on every entry.

use std::ffi::c_void;
use std::sync::OnceLock;

use crate::macos_objc::{self, Object, Selector};

/// `NSWindowStyleMaskFullScreen`.
const FULL_SCREEN: usize = 1 << 14;
/// `NSWindowCloseButton`: any of the title bar's buttons leads to the strip.
const CLOSE_BUTTON: usize = 0;

const OBSERVER_CLASS: &[u8] = b"TerminalStudioFullScreenObserver\0";
const NS_OBJECT: &[u8] = b"NSObject\0";
const NS_NOTIFICATION_CENTER: &[u8] = b"NSNotificationCenter\0";
const NS_COLOR: &[u8] = b"NSColor\0";
const DEFAULT_CENTER: &[u8] = b"defaultCenter\0";
const ADD_OBSERVER: &[u8] = b"addObserver:selector:name:object:\0";
const OBJECT: &[u8] = b"object\0";
const WINDOWS: &[u8] = b"windows\0";
const COUNT: &[u8] = b"count\0";
const OBJECT_AT_INDEX: &[u8] = b"objectAtIndex:\0";
const STYLE_MASK: &[u8] = b"styleMask\0";
const STANDARD_WINDOW_BUTTON: &[u8] = b"standardWindowButton:\0";
const WINDOW: &[u8] = b"window\0";
const CONTENT_VIEW: &[u8] = b"contentView\0";
const SET_HIDDEN: &[u8] = b"setHidden:\0";
const SET_IGNORES_MOUSE_EVENTS: &[u8] = b"setIgnoresMouseEvents:\0";
const SET_OPAQUE: &[u8] = b"setOpaque:\0";
const SET_HAS_SHADOW: &[u8] = b"setHasShadow:\0";
const SET_BACKGROUND_COLOR: &[u8] = b"setBackgroundColor:\0";
const CLEAR_COLOR: &[u8] = b"clearColor\0";

const DID_ENTER: &[u8] = b"terminalStudioDidEnterFullScreen:\0";
/// Returns void; takes self, _cmd, and the NSNotification.
const HANDLER_TYPES: &[u8] = b"v@:@\0";
/// AppKit's notification name, whose value is its own name.
const DID_ENTER_NAME: &str = "NSWindowDidEnterFullScreenNotification";

type GetUnsigned = unsafe extern "C" fn(Object, Selector) -> usize;
type ObjectAt = unsafe extern "C" fn(Object, Selector, usize) -> Object;
type SendBool = unsafe extern "C" fn(Object, Selector, bool);
type AddObserver = unsafe extern "C" fn(Object, Selector, Object, Selector, Object, Object);

/// The observer, kept for the life of the app (the notification center
/// doesn't retain its observers).
static OBSERVER: OnceLock<usize> = OnceLock::new();

/// Whether a window with this style is in full screen.
fn is_full_screen(style: usize) -> bool {
    style & FULL_SCREEN != 0
}

/// Empties a full-screen window's title bar strip and lets clicks through
/// it. Does nothing for a window that isn't in full screen, or whose title
/// bar is still its own.
///
/// # Safety
/// `window` must be nil or a live NSWindow; main thread only.
unsafe fn empty_title_bar_strip(window: Object) {
    if window.is_null() {
        return;
    }
    let style = macos_objc::message::<GetUnsigned>()(window, macos_objc::selector(STYLE_MASK));
    if !is_full_screen(style) {
        return;
    }
    let button = macos_objc::message::<ObjectAt>()(
        window,
        macos_objc::selector(STANDARD_WINDOW_BUTTON),
        CLOSE_BUTTON,
    );
    let strip = macos_objc::send(button, WINDOW);
    if strip.is_null() || strip == window {
        return;
    }
    let set_bool = macos_objc::message::<SendBool>();
    set_bool(
        macos_objc::send(strip, CONTENT_VIEW),
        macos_objc::selector(SET_HIDDEN),
        true,
    );
    set_bool(strip, macos_objc::selector(SET_IGNORES_MOUSE_EVENTS), true);
    set_bool(strip, macos_objc::selector(SET_OPAQUE), false);
    set_bool(strip, macos_objc::selector(SET_HAS_SHADOW), false);
    macos_objc::send_with(
        strip,
        SET_BACKGROUND_COLOR,
        macos_objc::send(macos_objc::class(NS_COLOR), CLEAR_COLOR),
    );
    log::debug!("full screen: emptied the title bar strip");
}

extern "C" fn did_enter(_this: Object, _cmd: Selector, notification: Object) {
    // SAFETY: AppKit posts this notification on the main thread with the
    // window as its object.
    unsafe { empty_title_bar_strip(macos_objc::send(notification, OBJECT)) }
}

/// Starts watching for windows entering full screen, and handles any window
/// already in it (one restored to full screen at launch). Main thread only;
/// later calls do nothing.
pub(super) fn install() {
    if OBSERVER.get().is_some() {
        return;
    }
    // SAFETY: the handler matches `v@:@`; every message goes to a live object
    // of a class that implements it, on the main thread.
    unsafe {
        let Some(class) = macos_objc::define_class(
            OBSERVER_CLASS,
            NS_OBJECT,
            &[(DID_ENTER, did_enter as *const c_void, HANDLER_TYPES)],
        ) else {
            log::warn!("could not define the full-screen observer; the title bar may slide over the app's in full screen");
            return;
        };
        let observer = macos_objc::new_object(class);
        let center = macos_objc::send(macos_objc::class(NS_NOTIFICATION_CENTER), DEFAULT_CENTER);
        if observer.is_null() || center.is_null() {
            log::warn!(
                "no notification center; the title bar may slide over the app's in full screen"
            );
            return;
        }
        let name = macos_objc::ns_string(DID_ENTER_NAME);
        macos_objc::message::<AddObserver>()(
            center,
            macos_objc::selector(ADD_OBSERVER),
            observer,
            macos_objc::selector(DID_ENTER),
            name,
            std::ptr::null_mut(),
        );
        macos_objc::release(name);
        let _ = OBSERVER.set(observer as usize);

        if let Some(app) = macos_objc::shared_application() {
            let windows = macos_objc::send(app, WINDOWS);
            let count = if windows.is_null() {
                0
            } else {
                macos_objc::message::<GetUnsigned>()(windows, macos_objc::selector(COUNT))
            };
            for index in 0..count {
                let window = macos_objc::message::<ObjectAt>()(
                    windows,
                    macos_objc::selector(OBJECT_AT_INDEX),
                    index,
                );
                empty_title_bar_strip(window);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_full_screen_windows_are_touched() {
        // What winit gives a frameless window for full screen: titled,
        // closable, miniaturizable, resizable.
        let titled = 0b1111;
        assert!(is_full_screen(titled | FULL_SCREEN));
        assert!(!is_full_screen(titled));
    }

    #[test]
    fn objc_names_are_nul_terminated() {
        for name in [
            OBSERVER_CLASS,
            NS_OBJECT,
            NS_NOTIFICATION_CENTER,
            NS_COLOR,
            DEFAULT_CENTER,
            ADD_OBSERVER,
            OBJECT,
            WINDOWS,
            COUNT,
            OBJECT_AT_INDEX,
            STYLE_MASK,
            STANDARD_WINDOW_BUTTON,
            WINDOW,
            CONTENT_VIEW,
            SET_HIDDEN,
            SET_IGNORES_MOUSE_EVENTS,
            SET_OPAQUE,
            SET_HAS_SHADOW,
            SET_BACKGROUND_COLOR,
            CLEAR_COLOR,
            DID_ENTER,
            HANDLER_TYPES,
        ] {
            assert_eq!(name.last(), Some(&0));
        }
    }
}
