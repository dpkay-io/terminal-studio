//! Minimal Objective-C runtime bridge for the few AppKit calls the app needs.
//!
//! Every call here must run on the main thread, as AppKit requires; egui's
//! `update` and the app's construction both do.

use std::ffi::{c_char, c_void};

pub type Object = *mut c_void;
pub type Selector = *mut c_void;

type MessageNoArgs = unsafe extern "C" fn(Object, Selector) -> Object;
type MessageOneArg = unsafe extern "C" fn(Object, Selector, Object) -> Object;
type NewString = unsafe extern "C" fn(Object, Selector, *const u8, usize, usize) -> Object;
type GetUtf8 = unsafe extern "C" fn(Object, Selector) -> *const c_char;

const NS_APPLICATION: &[u8] = b"NSApplication\0";
const SHARED_APPLICATION: &[u8] = b"sharedApplication\0";
const KEY_WINDOW: &[u8] = b"keyWindow\0";
const ZOOM: &[u8] = b"zoom:\0";
const NS_STRING: &[u8] = b"NSString\0";
const ALLOC: &[u8] = b"alloc\0";
const INIT: &[u8] = b"init\0";
const RELEASE: &[u8] = b"release\0";
const INIT_WITH_BYTES: &[u8] = b"initWithBytes:length:encoding:\0";
const UTF8_STRING: &[u8] = b"UTF8String\0";
/// `NSUTF8StringEncoding`.
const UTF8_ENCODING: usize = 4;

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Object;
    fn sel_registerName(name: *const c_char) -> Selector;
    fn objc_msgSend();
    fn object_getClass(object: Object) -> Object;
    fn objc_allocateClassPair(
        superclass: Object,
        name: *const c_char,
        extra_bytes: usize,
    ) -> Object;
    fn objc_registerClassPair(class: Object);
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

/// The class called `name`, or nil when there is none.
pub fn class(name: &'static [u8]) -> Object {
    // SAFETY: `c_name` guarantees a NUL-terminated string with static lifetime.
    unsafe { objc_getClass(c_name(name)) }
}

/// `objc_msgSend` typed as `F`, for messages whose arguments or return value
/// are not plain objects (integers, strings, pointers).
///
/// # Safety
/// `F` must be an `unsafe extern "C" fn` whose parameters start with the
/// receiver and selector and match the method being sent, as must its return
/// type. Never send it to nil when the return type is not pointer- or
/// integer-sized.
pub unsafe fn message<F: Copy>() -> F {
    debug_assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<usize>());
    std::mem::transmute_copy(&(objc_msgSend as unsafe extern "C" fn()))
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
    message::<MessageNoArgs>()(receiver, selector(name))
}

/// Sends a one-argument message, returning nil for a nil receiver.
///
/// # Safety
/// As for [`send`], and `argument` must be what the method expects.
pub unsafe fn send_with(receiver: Object, name: &'static [u8], argument: Object) -> Object {
    if receiver.is_null() {
        return std::ptr::null_mut();
    }
    message::<MessageOneArg>()(receiver, selector(name), argument)
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

/// Defines `name` as a subclass of `superclass` with `methods` (selector,
/// implementation, type encoding), or returns the class already defined
/// under that name. None when `superclass` doesn't exist.
///
/// # Safety
/// Each implementation must be an `extern "C"` function matching its type
/// encoding.
pub unsafe fn define_class(
    name: &'static [u8],
    superclass: &'static [u8],
    methods: &[(&'static [u8], *const c_void, &'static [u8])],
) -> Option<Object> {
    let existing = class(name);
    if !existing.is_null() {
        return Some(existing);
    }
    let parent = class(superclass);
    if parent.is_null() {
        return None;
    }
    let new_class = objc_allocateClassPair(parent, c_name(name), 0);
    if new_class.is_null() {
        return None;
    }
    for &(method, implementation, types) in methods {
        class_addMethod(new_class, selector(method), implementation, c_name(types));
    }
    objc_registerClassPair(new_class);
    Some(new_class)
}

/// `[class alloc]`: uninitialised storage the caller must `init…` next.
///
/// # Safety
/// `class` must be nil or a class.
pub unsafe fn alloc(class: Object) -> Object {
    send(class, ALLOC)
}

/// `[[class alloc] init]`: a new instance the caller owns, or nil.
///
/// # Safety
/// `class` must be nil or a class whose plain `init` is valid.
pub unsafe fn new_object(class: Object) -> Object {
    send(alloc(class), INIT)
}

/// Gives up the caller's ownership of `object`. Does nothing for nil.
///
/// # Safety
/// The caller must own `object` (from `alloc`/`new_object`/`ns_string`).
pub unsafe fn release(object: Object) {
    send(object, RELEASE);
}

/// A new NSString holding `text`, which the caller owns and must `release`.
/// Nil when Foundation is unavailable.
pub fn ns_string(text: &str) -> Object {
    // SAFETY: `initWithBytes:length:encoding:` copies `text.len()` bytes
    // from a live slice; its signature matches `NewString`. A nil `alloc`
    // result is never messaged.
    unsafe {
        let allocated = alloc(class(NS_STRING));
        if allocated.is_null() {
            return allocated;
        }
        message::<NewString>()(
            allocated,
            selector(INIT_WITH_BYTES),
            text.as_ptr(),
            text.len(),
            UTF8_ENCODING,
        )
    }
}

/// The text of an NSString, or None for nil or non-UTF-8 text.
///
/// # Safety
/// `string` must be nil or a live NSString.
pub unsafe fn rust_string(string: Object) -> Option<String> {
    if string.is_null() {
        return None;
    }
    let utf8 = message::<GetUtf8>()(string, selector(UTF8_STRING));
    if utf8.is_null() {
        return None;
    }
    std::ffi::CStr::from_ptr(utf8)
        .to_str()
        .ok()
        .map(str::to_owned)
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
        for name in [
            NS_APPLICATION,
            SHARED_APPLICATION,
            KEY_WINDOW,
            ZOOM,
            NS_STRING,
            ALLOC,
            INIT,
            RELEASE,
            INIT_WITH_BYTES,
            UTF8_STRING,
        ] {
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

    #[test]
    fn strings_round_trip_through_nsstring() {
        for text in ["", "Settings…", "Hide Sidebar", "\u{F700}"] {
            let string = ns_string(text);
            assert!(!string.is_null());
            // SAFETY: `string` is a live NSString this test owns.
            unsafe {
                assert_eq!(rust_string(string).as_deref(), Some(text));
                release(string);
            }
        }
        // SAFETY: nil is allowed.
        assert_eq!(unsafe { rust_string(std::ptr::null_mut()) }, None);
    }

    #[test]
    fn define_class_returns_the_same_class_twice() {
        const NAME: &[u8] = b"TerminalStudioObjcTestClass\0";
        // SAFETY: no methods are added.
        let first = unsafe { define_class(NAME, b"NSObject\0", &[]) };
        let second = unsafe { define_class(NAME, b"NSObject\0", &[]) };
        assert!(first.is_some());
        assert_eq!(first, second);
        // SAFETY: plain NSObject subclass; the instance is released.
        unsafe {
            let object = new_object(first.unwrap());
            assert!(!object.is_null());
            release(object);
        }
        assert_eq!(
            unsafe { define_class(b"TsNoSuchClassA\0", b"TsNoSuchClassB\0", &[]) },
            None
        );
    }
}
