//! Keeps the macOS menu bar from sliding down over the app's own title bar in
//! full screen.
//!
//! The window has no native decorations, so the app draws its header at the
//! very top of the screen. In native full screen AppKit's default options
//! auto-hide the menu bar: pointing at the top edge slides it down on top of
//! that header, covering its controls. AppKit asks the window delegate which
//! options to use as the window enters full screen
//! (`window:willUseFullScreenPresentationOptions:`), so the app answers there:
//! the menu bar and Dock stay hidden for the whole full-screen session, and
//! come back as usual once the window leaves it.
//!
//! winit's window delegate already implements that method, for its exclusive
//! full-screen mode. It is wrapped rather than replaced outright: winit's
//! answer is asked first and only then adjusted, so winit keeps working as
//! before.

use std::ffi::c_void;
use std::sync::OnceLock;

use crate::macos_objc::{self, Object, Selector};

/// `NSApplicationPresentationOptions` bits (AppKit's `NSApplication.h`).
const AUTO_HIDE_DOCK: usize = 1 << 0;
const HIDE_DOCK: usize = 1 << 1;
const AUTO_HIDE_MENU_BAR: usize = 1 << 2;
const HIDE_MENU_BAR: usize = 1 << 3;
const FULL_SCREEN: usize = 1 << 10;

/// winit 0.29's `NSWindowDelegate` class, shared by every window it creates.
const WINDOW_DELEGATE_CLASS: &[u8] = b"WinitWindowDelegate\0";
const WILL_USE_OPTIONS: &[u8] = b"window:willUseFullScreenPresentationOptions:\0";
/// Returns NSUInteger; takes self, _cmd, the window, and NSUInteger options.
const WILL_USE_OPTIONS_TYPES: &[u8] = b"Q@:@Q\0";

type WillUseOptions = unsafe extern "C" fn(Object, Selector, Object, usize) -> usize;

/// winit's implementation, stored as an address so the static is `Sync`.
static PREVIOUS: OnceLock<Option<usize>> = OnceLock::new();

/// The options to use for a full-screen session, given what would otherwise
/// be used.
///
/// AppKit only accepts some combinations: hiding the menu bar requires hiding
/// the Dock, and neither can be both hidden and auto-hidden. So the auto-hide
/// bits are swapped for the hide bits, and every other bit is kept.
fn full_screen_options(options: usize) -> usize {
    (options & !(AUTO_HIDE_DOCK | AUTO_HIDE_MENU_BAR)) | FULL_SCREEN | HIDE_DOCK | HIDE_MENU_BAR
}

extern "C" fn will_use_options(
    this: Object,
    cmd: Selector,
    window: Object,
    proposed: usize,
) -> usize {
    let options = match PREVIOUS.get().copied().flatten() {
        // SAFETY: the address is the IMP that `class_replaceMethod` swapped
        // out for this same selector, whose signature is `WillUseOptions`.
        Some(previous) => unsafe {
            let previous: WillUseOptions = std::mem::transmute(previous);
            previous(this, cmd, window, proposed)
        },
        None => proposed,
    };
    full_screen_options(options)
}

/// Installs the hook. Must run on the main thread, once; the app does both
/// from its constructor. Later calls do nothing.
pub(super) fn install() {
    if PREVIOUS.get().is_some() {
        return;
    }
    // SAFETY: `will_use_options` matches `Q@:@Q`, as does winit's method it
    // chains to. The previous IMP is recorded before AppKit can call ours,
    // because this runs on the main thread, which AppKit also calls it on.
    let result = unsafe {
        macos_objc::replace_method(
            WINDOW_DELEGATE_CLASS,
            WILL_USE_OPTIONS,
            will_use_options as *const c_void,
            WILL_USE_OPTIONS_TYPES,
        )
    };
    match result {
        Ok(previous) => {
            let _ = PREVIOUS.set(previous.map(|imp| imp as usize));
        }
        Err(()) => log::warn!(
            "winit's window delegate class was not found; the menu bar may cover the \
             title bar in full screen"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What AppKit proposes for a normal full-screen session.
    const DEFAULT_PROPOSAL: usize = FULL_SCREEN | AUTO_HIDE_DOCK | AUTO_HIDE_MENU_BAR;

    #[test]
    fn menu_bar_and_dock_are_hidden_not_auto_hidden() {
        let options = full_screen_options(DEFAULT_PROPOSAL);
        assert_eq!(options, FULL_SCREEN | HIDE_DOCK | HIDE_MENU_BAR);
    }

    #[test]
    fn result_is_always_a_combination_appkit_accepts() {
        for proposed in [
            0,
            FULL_SCREEN,
            DEFAULT_PROPOSAL,
            FULL_SCREEN | HIDE_DOCK | HIDE_MENU_BAR,
            FULL_SCREEN | AUTO_HIDE_DOCK,
        ] {
            let options = full_screen_options(proposed);
            assert_ne!(options & FULL_SCREEN, 0, "{proposed:#x}");
            assert_ne!(
                options & HIDE_DOCK,
                0,
                "hiding the menu bar needs the Dock hidden"
            );
            assert_eq!(options & (AUTO_HIDE_DOCK | AUTO_HIDE_MENU_BAR), 0);
        }
    }

    #[test]
    fn unrelated_bits_survive() {
        const AUTO_HIDE_TOOLBAR: usize = 1 << 11;
        let options = full_screen_options(DEFAULT_PROPOSAL | AUTO_HIDE_TOOLBAR);
        assert_ne!(options & AUTO_HIDE_TOOLBAR, 0);
    }

    #[test]
    fn without_a_previous_implementation_the_proposal_is_adjusted() {
        // No test installs the hook, so there is no winit method to chain to
        // and the proposal itself is adjusted.
        let options = will_use_options(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            DEFAULT_PROPOSAL,
        );
        assert_eq!(options, full_screen_options(DEFAULT_PROPOSAL));
    }

    #[test]
    fn objc_names_are_nul_terminated() {
        for name in [
            WINDOW_DELEGATE_CLASS,
            WILL_USE_OPTIONS,
            WILL_USE_OPTIONS_TYPES,
        ] {
            assert_eq!(name.last(), Some(&0));
        }
    }
}
