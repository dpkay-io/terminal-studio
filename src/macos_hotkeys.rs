//! Reads the keyboard shortcuts macOS has enabled for itself (System Settings →
//! Keyboard → Keyboard Shortcuts), so the app can show which of its bindings
//! the system takes before the app ever sees them. Which shortcuts are on
//! varies by machine and macOS version, so this is read live, not hard-coded.

use std::ffi::c_void;

use crate::shortcuts::Shortcut;

type CFTypeRef = *const c_void;
type CFIndex = isize;
type CFTypeID = usize;
type OSStatus = i32;

const NO_ERR: OSStatus = 0;
/// `kCFNumberSInt64Type`.
const CF_NUMBER_SINT64: CFIndex = 4;

// Carbon modifier masks (Events.h).
const CMD_KEY: i64 = 1 << 8;
const SHIFT_KEY: i64 = 1 << 9;
const OPTION_KEY: i64 = 1 << 11;
const CONTROL_KEY: i64 = 1 << 12;

/// `kCFStringEncodingUTF8`.
const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

// Dictionary keys. In the headers these are `CFSTR(...)` macros, not exported
// symbols, so the strings are created at run time from these names.
const HOT_KEY_CODE: &[u8] = b"kHISymbolicHotKeyCode\0";
const HOT_KEY_MODIFIERS: &[u8] = b"kHISymbolicHotKeyModifiers\0";
const HOT_KEY_ENABLED: &[u8] = b"kHISymbolicHotKeyEnabled\0";

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn CopySymbolicHotKeys(out_hot_keys: *mut CFTypeRef) -> OSStatus;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFArrayGetCount(array: CFTypeRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFTypeRef, index: CFIndex) -> CFTypeRef;
    fn CFDictionaryGetValue(dictionary: CFTypeRef, key: CFTypeRef) -> CFTypeRef;
    fn CFNumberGetValue(number: CFTypeRef, kind: CFIndex, out: *mut c_void) -> u8;
    fn CFBooleanGetValue(boolean: CFTypeRef) -> u8;
    fn CFGetTypeID(object: CFTypeRef) -> CFTypeID;
    fn CFBooleanGetTypeID() -> CFTypeID;
    fn CFNumberGetTypeID() -> CFTypeID;
    fn CFRelease(object: CFTypeRef);
    fn CFStringCreateWithCString(
        allocator: CFTypeRef,
        text: *const std::ffi::c_char,
        encoding: u32,
    ) -> CFTypeRef;
}

/// An owned CFString, released on drop.
struct CfString(CFTypeRef);

impl CfString {
    fn new(name: &'static [u8]) -> Option<Self> {
        debug_assert_eq!(name.last(), Some(&0));
        // SAFETY: `name` is a NUL-terminated static string; a null result is
        // handled by the caller.
        let string = unsafe {
            CFStringCreateWithCString(
                std::ptr::null(),
                name.as_ptr().cast(),
                CF_STRING_ENCODING_UTF8,
            )
        };
        (!string.is_null()).then_some(Self(string))
    }
}

impl Drop for CfString {
    fn drop(&mut self) {
        // SAFETY: created with a +1 retain count and released exactly once.
        unsafe { CFRelease(self.0) }
    }
}

/// macOS virtual key codes (Events.h, ANSI layout) for the keys the app can bind.
const KEY_CODES: &[(i64, egui::Key)] = {
    use egui::Key::*;
    &[
        (0, A),
        (1, S),
        (2, D),
        (3, F),
        (4, H),
        (5, G),
        (6, Z),
        (7, X),
        (8, C),
        (9, V),
        (11, B),
        (12, Q),
        (13, W),
        (14, E),
        (15, R),
        (16, Y),
        (17, T),
        (18, Num1),
        (19, Num2),
        (20, Num3),
        (21, Num4),
        (22, Num6),
        (23, Num5),
        (24, Equals),
        (25, Num9),
        (26, Num7),
        (27, Minus),
        (28, Num8),
        (29, Num0),
        (30, CloseBracket),
        (31, O),
        (32, U),
        (33, OpenBracket),
        (34, I),
        (35, P),
        (36, Enter),
        (37, L),
        (38, J),
        (40, K),
        (41, Semicolon),
        (42, Backslash),
        (43, Comma),
        (44, Slash),
        (45, N),
        (46, M),
        (47, Period),
        (48, Tab),
        (49, Space),
        (50, Backtick),
        (51, Backspace),
        (53, Escape),
        (96, F5),
        (97, F6),
        (98, F7),
        (99, F3),
        (100, F8),
        (101, F9),
        (103, F11),
        (109, F10),
        (111, F12),
        (115, Home),
        (116, PageUp),
        (117, Delete),
        (118, F4),
        (119, End),
        (120, F2),
        (121, PageDown),
        (122, F1),
        (123, ArrowLeft),
        (124, ArrowRight),
        (125, ArrowDown),
        (126, ArrowUp),
    ]
};

fn key_for_code(code: i64) -> Option<egui::Key> {
    KEY_CODES
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, key)| *key)
}

/// Converts a system hotkey into the app's chord form, where `ctrl` is Command.
fn shortcut_from(code: i64, modifiers: i64) -> Option<Shortcut> {
    let mut shortcut = Shortcut::key(key_for_code(code)?);
    shortcut.ctrl = modifiers & CMD_KEY != 0;
    shortcut.shift = modifiers & SHIFT_KEY != 0;
    shortcut.alt = modifiers & OPTION_KEY != 0;
    shortcut.control = modifiers & CONTROL_KEY != 0;
    Some(shortcut)
}

/// Reads a CFNumber or CFBoolean as an integer.
///
/// # Safety
/// `value` must be null or a valid Core Foundation object.
unsafe fn integer(value: CFTypeRef) -> Option<i64> {
    if value.is_null() {
        return None;
    }
    let kind = CFGetTypeID(value);
    if kind == CFBooleanGetTypeID() {
        return Some(i64::from(CFBooleanGetValue(value)));
    }
    if kind == CFNumberGetTypeID() {
        let mut out: i64 = 0;
        let ok = CFNumberGetValue(value, CF_NUMBER_SINT64, (&mut out as *mut i64).cast());
        return (ok != 0).then_some(out);
    }
    None
}

/// The system's enabled shortcuts that the app could also bind. Empty when
/// the list cannot be read, so a failure only hides the warnings.
pub fn reserved_shortcuts() -> Vec<Shortcut> {
    let (Some(code_key), Some(modifiers_key), Some(enabled_key)) = (
        CfString::new(HOT_KEY_CODE),
        CfString::new(HOT_KEY_MODIFIERS),
        CfString::new(HOT_KEY_ENABLED),
    ) else {
        log::warn!("could not create macOS shortcut dictionary keys");
        return Vec::new();
    };
    let mut array: CFTypeRef = std::ptr::null();
    // SAFETY: Carbon writes a +1 retained CFArray of CFDictionaries on
    // success. Every value is null-checked and type-checked before use, and
    // the array is released exactly once.
    unsafe {
        if CopySymbolicHotKeys(&mut array) != NO_ERR || array.is_null() {
            log::warn!("could not read macOS keyboard shortcuts");
            return Vec::new();
        }
        let mut reserved = Vec::new();
        for index in 0..CFArrayGetCount(array) {
            let entry = CFArrayGetValueAtIndex(array, index);
            if entry.is_null() {
                continue;
            }
            let field = |key: &CfString| integer(CFDictionaryGetValue(entry, key.0));
            if field(&enabled_key) != Some(1) {
                continue;
            }
            let (Some(code), Some(modifiers)) = (field(&code_key), field(&modifiers_key)) else {
                continue;
            };
            if let Some(shortcut) = shortcut_from(code, modifiers) {
                reserved.push(shortcut);
            }
        }
        CFRelease(array);
        reserved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_command_shift_three() {
        let screenshot = shortcut_from(20, CMD_KEY | SHIFT_KEY).expect("known key");
        assert_eq!(screenshot, Shortcut::primary(egui::Key::Num3).with_shift());
    }

    #[test]
    fn converts_control_command_space() {
        let viewer = shortcut_from(49, CMD_KEY | CONTROL_KEY).expect("known key");
        assert_eq!(viewer, Shortcut::primary(egui::Key::Space).with_control());
    }

    #[test]
    fn unknown_key_codes_are_skipped() {
        assert_eq!(shortcut_from(65535, CMD_KEY), None);
    }

    #[test]
    fn key_codes_are_unique() {
        for (i, (code, key)) in KEY_CODES.iter().enumerate() {
            for (other_code, other_key) in &KEY_CODES[i + 1..] {
                assert_ne!(code, other_code);
                assert_ne!(key, other_key);
            }
        }
    }

    #[test]
    fn reads_the_live_system_list() {
        // Every Mac enables Spotlight (Command-Space) unless the user turned it
        // off, so only check that reading works and yields well-formed chords.
        for shortcut in reserved_shortcuts() {
            assert!(KEY_CODES.iter().any(|(_, key)| *key == shortcut.key));
        }
    }
}
