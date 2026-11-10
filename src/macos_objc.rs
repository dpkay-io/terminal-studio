//! Minimal Objective-C runtime bridge for the few AppKit calls the app needs.
//!
//! Every call here must run on the main thread, as AppKit requires; egui's
//! `update` and the app's construction both do.

use std::ffi::{c_char, c_void};

pub type Object = *mut c_void;
pub type Selector = *mut c_void;

type MessageNoArgs = unsafe extern "C" fn(Object, Selector) -> Object;
type MessageOneArg = unsafe extern "C" fn(Object, Selector, Object) -> Object;

const NS_APPLICATION: &[u8] = b"NSApplication\0";
const SHARED_APPLICATION: &[u8] = b"sharedApplication\0";
const KEY_WINDOW: &[u8] = b"keyWindow\0";
const ZOOM: &[u8] = b"zoom:\0";

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Object;
    fn sel_registerName(name: *const c_char) -> Selector;
    fn objc_msgSend();
    fn object_getClass(object: Object) -> Object;
    fn class_addMethod(
        class: Object,
        name: Selector,
        implementation: *const c_void,
        types: *const c_char,
    ) -> bool;
}

/// Pointer to a NUL-terminated name. Every caller passes a `b"...\0"` literal.
pub fn c_name(name: &'static [u8]) -> *const c_char {
    debug_assert_eq!(
        name.last(),
        Some(&0),
        "Objective-C names must be NUL-terminated"
    );
    name.as_ptr().cast()
}

pub fn selector(name: &'static [u8]) -> Selector {
    // SAFETY: `c_name` guarantees a NUL-terminated string with static lifetime.
    unsafe { sel_registerName(c_name(name)) }
}

/// Sends an argument-less message, returning nil for a nil receiver.
///
/// # Safety
/// `receiver` must be nil or a live Objective-C object that responds to
/// `name` with an object (or void) return value.
pub unsafe fn send(receiver: Object, name: &'static [u8]) -> Object {
    if receiver.is_null() {
        return std::ptr::null_mut();
    }
    let message: MessageNoArgs = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    message(receiver, selector(name))
}

/// Sends a one-argument message, returning nil for a nil receiver.
///
/// # Safety
/// As for [`send`], and `argument` must be what the method expects.
pub unsafe fn send_with(receiver: Object, name: &'static [u8], argument: Object) -> Object {
    if receiver.is_null() {
        return std::ptr::null_mut();
    }
    let message: MessageOneArg = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    message(receiver, selector(name), argument)
}

/// `NSApp`, or None when AppKit is unavailable.
pub fn shared_application() -> Option<Object> {
    // SAFETY: a class lookup by static name; `send` tolerates a nil class.
    let app = unsafe { send(objc_getClass(c_name(NS_APPLICATION)), SHARED_APPLICATION) };
    (!app.is_null()).then_some(app)
}

/// Adds a method to the class of `object`. Returns false if it could not be added.
///
/// # Safety
/// `implementation` must be an `extern "C"` function whose signature matches
/// the Objective-C type encoding `types`.
pub unsafe fn add_method(
    object: Object,
    name: &'static [u8],
    implementation: *const c_void,
    types: &'static [u8],
) -> bool {
    if object.is_null() {
        return false;
    }
    class_addMethod(
        object_getClass(object),
        selector(name),
        implementation,
        c_name(types),
    )
}

/// Toggles AppKit's own zoom on the key window. AppKit tracks whether the
/// window is zoomed, so this stays right even after another app (such as a
/// window manager) resizes the window.
pub fn zoom_key_window() {
    let Some(app) = shared_application() else {
        return;
    };
    // SAFETY: `keyWindow` returns an NSWindow or nil, and `zoom:` takes a
    // sender object, for which nil is allowed. `send_with` skips nil windows.
    unsafe {
        let window = send(app, KEY_WINDOW);
        send_with(window, ZOOM, std::ptr::null_mut());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_nul_terminated() {
        for name in [NS_APPLICATION, SHARED_APPLICATION, KEY_WINDOW, ZOOM] {
            assert_eq!(name.last(), Some(&0));
        }
    }

    #[test]
    fn messages_to_nil_are_ignored() {
        // SAFETY: nil receivers return before any message is sent.
        unsafe {
            assert!(send(std::ptr::null_mut(), KEY_WINDOW).is_null());
            assert!(send_with(std::ptr::null_mut(), ZOOM, std::ptr::null_mut()).is_null());
            assert!(!add_method(
                std::ptr::null_mut(),
                ZOOM,
                std::ptr::null(),
                b"v@:@\0"
            ));
        }
    }
}
