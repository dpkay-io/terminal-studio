//! Routes macOS termination requests through the app's normal close flow.
//!
//! winit's default menu binds Cmd+Q to `-[NSApplication terminate:]`, which
//! ends the process without any window event, skipping the quit confirmation
//! and killing every running terminal. Giving the application delegate an
//! `applicationShouldTerminate:` lets the app cancel that and ask first.

use std::ffi::{c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

type Object = *mut c_void;
type Selector = *mut c_void;
type MessageNoArgs = unsafe extern "C" fn(Object, Selector) -> Object;

/// `NSTerminateCancel`: the delegate declines the immediate termination.
const NS_TERMINATE_CANCEL: isize = 0;
/// Objective-C signature: returns NSInteger, takes self, _cmd, and sender.
const SHOULD_TERMINATE_TYPES: &[u8] = b"q@:@\0";
const NS_APPLICATION: &[u8] = b"NSApplication\0";
const SHARED_APPLICATION: &[u8] = b"sharedApplication\0";
const DELEGATE: &[u8] = b"delegate\0";
const SHOULD_TERMINATE: &[u8] = b"applicationShouldTerminate:\0";

static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static REPAINT_CONTEXT: OnceLock<egui::Context> = OnceLock::new();

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

fn name_ptr(name: &'static [u8]) -> *const c_char {
    name.as_ptr().cast()
}

extern "C" fn should_terminate(_this: Object, _cmd: Selector, _sender: Object) -> isize {
    QUIT_REQUESTED.store(true, Ordering::Release);
    if let Some(ctx) = REPAINT_CONTEXT.get() {
        ctx.request_repaint();
    }
    NS_TERMINATE_CANCEL
}

/// Installs the handler. Must run on the main thread once winit has set the
/// application delegate, which is guaranteed by the time the app is created.
pub(super) fn install(ctx: &egui::Context) {
    // Only the first context is kept; the app creates exactly one.
    let _ = REPAINT_CONTEXT.set(ctx.clone());

    // SAFETY: every name is NUL-terminated, each returned pointer is checked
    // before use, and `objc_msgSend` is cast to the exact signature of the two
    // argument-less messages sent. Called on the main thread, as AppKit needs.
    let added = unsafe {
        let send: MessageNoArgs = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        let app_class = objc_getClass(name_ptr(NS_APPLICATION));
        if app_class.is_null() {
            log::warn!("NSApplication class unavailable; Cmd+Q will not ask to confirm");
            return;
        }
        let app = send(app_class, sel_registerName(name_ptr(SHARED_APPLICATION)));
        let delegate = if app.is_null() {
            std::ptr::null_mut()
        } else {
            send(app, sel_registerName(name_ptr(DELEGATE)))
        };
        if delegate.is_null() {
            log::warn!("application delegate unavailable; Cmd+Q will not ask to confirm");
            return;
        }
        class_addMethod(
            object_getClass(delegate),
            sel_registerName(name_ptr(SHOULD_TERMINATE)),
            should_terminate as *const c_void,
            name_ptr(SHOULD_TERMINATE_TYPES),
        )
    };
    if !added {
        log::warn!("could not install termination handler; Cmd+Q will not ask to confirm");
    }
}

/// Returns true once for each termination request received since the last call.
pub(super) fn take_request() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::AcqRel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn termination_is_cancelled_and_reported_exactly_once() {
        let result = should_terminate(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        assert_eq!(result, NS_TERMINATE_CANCEL);
        assert!(take_request());
        assert!(!take_request());
    }

    #[test]
    fn objc_names_are_nul_terminated() {
        for name in [
            SHOULD_TERMINATE_TYPES,
            NS_APPLICATION,
            SHARED_APPLICATION,
            DELEGATE,
            SHOULD_TERMINATE,
        ] {
            assert_eq!(name.last(), Some(&0));
        }
    }
}
