//! Routes macOS termination requests through the app's normal close flow.
//!
//! winit's default menu binds Cmd+Q to `-[NSApplication terminate:]`, which
//! ends the process without any window event, skipping the quit confirmation
//! and killing every running terminal. Giving the application delegate an
//! `applicationShouldTerminate:` lets the app cancel that and ask first.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use crate::macos_objc::{self, Object, Selector};

/// `NSTerminateCancel`: the delegate declines the immediate termination.
const NS_TERMINATE_CANCEL: isize = 0;
/// Objective-C signature: returns NSInteger, takes self, _cmd, and sender.
const SHOULD_TERMINATE_TYPES: &[u8] = b"q@:@\0";
const DELEGATE: &[u8] = b"delegate\0";
const SHOULD_TERMINATE: &[u8] = b"applicationShouldTerminate:\0";

static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);
static REPAINT_CONTEXT: OnceLock<egui::Context> = OnceLock::new();

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

    let Some(app) = macos_objc::shared_application() else {
        log::warn!("NSApplication unavailable; Cmd+Q will not ask to confirm");
        return;
    };
    // SAFETY: `delegate` returns an object or nil, and `should_terminate`
    // matches the `q@:@` encoding: NSInteger return, (self, _cmd, sender).
    let added = unsafe {
        let delegate = macos_objc::send(app, DELEGATE);
        macos_objc::add_method(
            delegate,
            SHOULD_TERMINATE,
            should_terminate as *const c_void,
            SHOULD_TERMINATE_TYPES,
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
        for name in [SHOULD_TERMINATE_TYPES, DELEGATE, SHOULD_TERMINATE] {
            assert_eq!(name.last(), Some(&0));
        }
    }
}
