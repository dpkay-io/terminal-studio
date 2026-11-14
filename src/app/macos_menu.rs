//! The titlebar's show/hide actions in the macOS menu bar.
//!
//! Native Mac users look for these in the menu bar, so they appear where Mac
//! apps put them:
//!
//! - Terminal Studio → Settings…
//! - View → Hide/Show Sidebar, Hide/Show Explorer, Hide/Show Tab Bar,
//!   Switcher, Recently Closed Sessions
//! - Help → Keyboard Shortcuts
//!
//! Each item shows its current shortcut and runs the same
//! [`App::toggle_view`](super::App::toggle_view) as the titlebar button.
//!
//! The shortcuts are only shown, never claimed by the menu: the keys keep
//! going to the app's own handling, which knows when a key belongs to a text
//! field or the shortcut recorder instead. AppKit would otherwise take a key
//! that matches a menu item before the window sees it, so each item disables
//! itself while AppKit is matching a key press against its own chord, which
//! lets the key through. (A menu delegate answering "no key equivalents" does
//! not stop this for a menu whose items already exist: tried, AppKit still
//! matched them.) Everywhere else, such as a menu opened with the mouse, the
//! items are enabled.
//!
//! Menu clicks happen inside AppKit's event handling, outside egui's frame.
//! They are queued here and run by the focused window's next frame, which
//! also publishes what the menu should show.

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use super::view_toggles::VIEW_TOGGLES;
use super::App;
use crate::macos_objc::{self, Object, Selector};
use crate::shortcuts::{key_name, AppAction, Shortcut};

/// The top-level menu an item belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Menu {
    /// The application menu winit creates (About, Hide, Quit).
    App,
    View,
    Help,
}

/// How an item reads, given whether its panel or overlay is showing.
#[derive(Clone, Copy, Debug)]
enum Kind {
    /// "Hide Sidebar" while shown, "Show Sidebar" while hidden, the way Mac
    /// apps word panel toggles.
    ShowHide {
        shown: &'static str,
        hidden: &'static str,
    },
    /// A checkmark while open; choosing it again closes it.
    Checked(&'static str),
    /// Opens it, and leaves it open when it already is (Settings…).
    Opens(&'static str),
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    action: AppAction,
    menu: Menu,
    kind: Kind,
    /// Starts a new group within its menu.
    separator_before: bool,
}

/// Every item, in menu order within each menu. An item's index here is its
/// `NSMenuItem` tag.
const ENTRIES: [Entry; VIEW_TOGGLES.len()] = [
    Entry {
        action: AppAction::OpenSettings,
        menu: Menu::App,
        kind: Kind::Opens("Settings…"),
        separator_before: false,
    },
    Entry {
        action: AppAction::ToggleLeftSidebar,
        menu: Menu::View,
        kind: Kind::ShowHide {
            shown: "Hide Sidebar",
            hidden: "Show Sidebar",
        },
        separator_before: false,
    },
    Entry {
        action: AppAction::ToggleRightSidebar,
        menu: Menu::View,
        kind: Kind::ShowHide {
            shown: "Hide Explorer",
            hidden: "Show Explorer",
        },
        separator_before: false,
    },
    Entry {
        action: AppAction::ToggleTabBar,
        menu: Menu::View,
        kind: Kind::ShowHide {
            shown: "Hide Tab Bar",
            hidden: "Show Tab Bar",
        },
        separator_before: false,
    },
    Entry {
        action: AppAction::OpenQuickSwitcher,
        menu: Menu::View,
        kind: Kind::Checked("Switcher"),
        separator_before: true,
    },
    Entry {
        action: AppAction::ReopenClosedSession,
        menu: Menu::View,
        kind: Kind::Checked("Recently Closed Sessions"),
        separator_before: false,
    },
    Entry {
        action: AppAction::ToggleShortcutHelp,
        menu: Menu::Help,
        kind: Kind::Checked("Keyboard Shortcuts"),
        separator_before: false,
    },
];

const VIEW_MENU_TITLE: &str = "View";
const HELP_MENU_TITLE: &str = "Help";
/// Where Settings… goes in the app menu: after "About" and its separator.
const SETTINGS_INDEX_IN_APP_MENU: isize = 2;

impl Entry {
    fn title(&self, open: bool) -> &'static str {
        match self.kind {
            Kind::ShowHide { shown, hidden } => {
                if open {
                    shown
                } else {
                    hidden
                }
            }
            Kind::Checked(title) | Kind::Opens(title) => title,
        }
    }

    fn checked(&self, open: bool) -> bool {
        matches!(self.kind, Kind::Checked(_)) && open
    }

    /// Whether choosing the item should toggle its view now.
    fn should_toggle(&self, open: bool) -> bool {
        !(matches!(self.kind, Kind::Opens(_)) && open)
    }
}

// ── Shortcuts as AppKit key equivalents ─────────────────────────────────────

/// `NSEventModifierFlags` bits.
const SHIFT_MASK: usize = 1 << 17;
const CONTROL_MASK: usize = 1 << 18;
const OPTION_MASK: usize = 1 << 19;
const COMMAND_MASK: usize = 1 << 20;
const MODIFIER_MASK: usize = SHIFT_MASK | CONTROL_MASK | OPTION_MASK | COMMAND_MASK;

/// AppKit's characters for keys that don't type one (`NSEvent.h`).
const UP_ARROW_KEY: char = '\u{F700}';
const DOWN_ARROW_KEY: char = '\u{F701}';
const LEFT_ARROW_KEY: char = '\u{F702}';
const RIGHT_ARROW_KEY: char = '\u{F703}';
const F1_KEY: u32 = 0xF704;
const INSERT_KEY: char = '\u{F727}';
const FORWARD_DELETE_KEY: char = '\u{F728}';
const HOME_KEY: char = '\u{F729}';
const END_KEY: char = '\u{F72B}';
const PAGE_UP_KEY: char = '\u{F72C}';
const PAGE_DOWN_KEY: char = '\u{F72D}';
const BACKSPACE_CHAR: char = '\u{8}';
const ESCAPE_CHAR: char = '\u{1B}';

const FUNCTION_KEYS: [egui::Key; 12] = [
    egui::Key::F1,
    egui::Key::F2,
    egui::Key::F3,
    egui::Key::F4,
    egui::Key::F5,
    egui::Key::F6,
    egui::Key::F7,
    egui::Key::F8,
    egui::Key::F9,
    egui::Key::F10,
    egui::Key::F11,
    egui::Key::F12,
];

/// A shortcut as an `NSMenuItem` shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct KeyEquivalent {
    key: char,
    modifiers: usize,
}

/// The character AppKit uses for `key`, or None for a key it can't show.
fn key_character(key: egui::Key) -> Option<char> {
    use egui::Key;
    if let Some(index) = FUNCTION_KEYS.iter().position(|&f| f == key) {
        return char::from_u32(F1_KEY + index as u32);
    }
    Some(match key {
        Key::Space => ' ',
        Key::Enter => '\r',
        Key::Tab => '\t',
        Key::Escape => ESCAPE_CHAR,
        Key::Backspace => BACKSPACE_CHAR,
        Key::Delete => FORWARD_DELETE_KEY,
        Key::Insert => INSERT_KEY,
        Key::Home => HOME_KEY,
        Key::End => END_KEY,
        Key::PageUp => PAGE_UP_KEY,
        Key::PageDown => PAGE_DOWN_KEY,
        Key::ArrowUp => UP_ARROW_KEY,
        Key::ArrowDown => DOWN_ARROW_KEY,
        Key::ArrowLeft => LEFT_ARROW_KEY,
        Key::ArrowRight => RIGHT_ARROW_KEY,
        // Letters, digits and punctuation: the one character the key is
        // named by. A letter is given lowercase; Shift is in the modifiers.
        other => {
            let mut name = key_name(other).chars();
            match (name.next(), name.next()) {
                (Some(c), None) if c != UNNAMED_KEY => c.to_ascii_lowercase(),
                _ => return None,
            }
        }
    })
}

/// What `key_name` returns for a key it has no name for.
const UNNAMED_KEY: char = '?';

fn key_equivalent(shortcut: &Shortcut) -> Option<KeyEquivalent> {
    let mut modifiers = 0;
    for (held, mask) in [
        (shortcut.ctrl, COMMAND_MASK),
        (shortcut.control, CONTROL_MASK),
        (shortcut.alt, OPTION_MASK),
        (shortcut.shift, SHIFT_MASK),
    ] {
        if held {
            modifiers |= mask;
        }
    }
    Some(KeyEquivalent {
        key: key_character(shortcut.key)?,
        modifiers,
    })
}

/// Whether a key press with these characters (no modifiers applied) and
/// modifier flags is `equivalent`'s chord.
fn is_chord(equivalent: KeyEquivalent, characters: &str, flags: usize) -> bool {
    let mut chars = characters.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return false;
    };
    c.to_ascii_lowercase() == equivalent.key && flags & MODIFIER_MASK == equivalent.modifiers
}

// ── What the menu shows ─────────────────────────────────────────────────────

/// Each item's state, published by the focused window every frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MenuState {
    open: [bool; ENTRIES.len()],
    keys: [Option<KeyEquivalent>; ENTRIES.len()],
}

impl MenuState {
    fn new(
        open: impl Fn(AppAction) -> bool,
        shortcut: impl Fn(AppAction) -> Option<Shortcut>,
    ) -> Self {
        let mut state = MenuState::default();
        for (i, entry) in ENTRIES.iter().enumerate() {
            state.open[i] = open(entry.action);
            state.keys[i] = shortcut(entry.action).as_ref().and_then(key_equivalent);
        }
        state
    }
}

/// The installed menu: its items (by entry index) and what they show now.
struct Installed {
    items: [usize; ENTRIES.len()],
    shown: Option<MenuState>,
}

static INSTALLED: Mutex<Option<Installed>> = Mutex::new(None);
static PENDING: Mutex<Vec<AppAction>> = Mutex::new(Vec::new());
static REPAINT_CONTEXT: OnceLock<egui::Context> = OnceLock::new();

// ── Objective-C side ────────────────────────────────────────────────────────

const TARGET_CLASS: &[u8] = b"TerminalStudioMenuTarget\0";
const NS_OBJECT: &[u8] = b"NSObject\0";
const NS_MENU: &[u8] = b"NSMenu\0";
const NS_MENU_ITEM: &[u8] = b"NSMenuItem\0";
const NS_WINDOW: &[u8] = b"NSWindow\0";

const PERFORM: &[u8] = b"performTerminalStudioMenuItem:\0";
const PERFORM_TYPES: &[u8] = b"v@:@\0";
const VALIDATE: &[u8] = b"validateMenuItem:\0";
const VALIDATE_TYPES: &[u8] = b"B@:@\0";

const MAIN_MENU: &[u8] = b"mainMenu\0";
const ITEM_AT_INDEX: &[u8] = b"itemAtIndex:\0";
const NUMBER_OF_ITEMS: &[u8] = b"numberOfItems\0";
const SUBMENU: &[u8] = b"submenu\0";
const SET_SUBMENU: &[u8] = b"setSubmenu:\0";
const ADD_ITEM: &[u8] = b"addItem:\0";
const INSERT_ITEM_AT_INDEX: &[u8] = b"insertItem:atIndex:\0";
const INIT_WITH_TITLE: &[u8] = b"initWithTitle:\0";
const INIT_ITEM: &[u8] = b"initWithTitle:action:keyEquivalent:\0";
const SEPARATOR_ITEM: &[u8] = b"separatorItem\0";
const SET_TARGET: &[u8] = b"setTarget:\0";
const SET_TAG: &[u8] = b"setTag:\0";
const TAG: &[u8] = b"tag\0";
const SET_TITLE: &[u8] = b"setTitle:\0";
const SET_STATE: &[u8] = b"setState:\0";
const SET_KEY_EQUIVALENT: &[u8] = b"setKeyEquivalent:\0";
const SET_KEY_EQUIVALENT_MASK: &[u8] = b"setKeyEquivalentModifierMask:\0";
const SET_ALLOWS_TABBING: &[u8] = b"setAllowsAutomaticWindowTabbing:\0";
const CURRENT_EVENT: &[u8] = b"currentEvent\0";
const EVENT_TYPE: &[u8] = b"type\0";
const MODIFIER_FLAGS: &[u8] = b"modifierFlags\0";
/// The characters with no modifiers at all applied. (Unlike
/// `charactersIgnoringModifiers`, which still applies Shift: ⇧, reads "<".)
const CHARACTERS_WITHOUT_MODIFIERS: &[u8] = b"charactersByApplyingModifiers:\0";

/// `NSControlStateValueOn` / `Off`.
const STATE_ON: isize = 1;
const STATE_OFF: isize = 0;
/// `NSEventTypeKeyDown`.
const KEY_DOWN_EVENT: usize = 10;

type SendInteger = unsafe extern "C" fn(Object, Selector, isize) -> Object;
type SendUnsigned = unsafe extern "C" fn(Object, Selector, usize) -> Object;
type SendBool = unsafe extern "C" fn(Object, Selector, bool) -> Object;
type GetInteger = unsafe extern "C" fn(Object, Selector) -> isize;
type GetUnsigned = unsafe extern "C" fn(Object, Selector) -> usize;
type InsertAt = unsafe extern "C" fn(Object, Selector, Object, isize) -> Object;
type NewItem = unsafe extern "C" fn(Object, Selector, Object, Selector, Object) -> Object;

/// The entry behind a menu item, from its tag.
///
/// # Safety
/// `item` must be nil or a live NSMenuItem.
unsafe fn entry_of(item: Object) -> Option<(usize, &'static Entry)> {
    if item.is_null() {
        return None;
    }
    let tag = macos_objc::message::<GetInteger>()(item, macos_objc::selector(TAG));
    let index = usize::try_from(tag).ok()?;
    ENTRIES.get(index).map(|entry| (index, entry))
}

extern "C" fn perform(_this: Object, _cmd: Selector, sender: Object) {
    // SAFETY: AppKit passes the chosen NSMenuItem as the sender.
    let Some((_, entry)) = (unsafe { entry_of(sender) }) else {
        return;
    };
    log::debug!("menu bar: {:?} chosen", entry.action);
    if let Ok(mut pending) = PENDING.lock() {
        pending.push(entry.action);
    }
    if let Some(ctx) = REPAINT_CONTEXT.get() {
        ctx.request_repaint();
    }
}

/// Disables an item only while AppKit is matching a key press against its
/// own chord, so the key reaches the window instead (see the module docs).
extern "C" fn validate(_this: Object, _cmd: Selector, item: Object) -> bool {
    // SAFETY: AppKit passes the NSMenuItem being validated.
    let Some((index, _)) = (unsafe { entry_of(item) }) else {
        return true;
    };
    // `try_lock`: never wait inside AppKit's callback; a menu that is being
    // updated this instant just isn't checked.
    let equivalent = INSTALLED
        .try_lock()
        .ok()
        .and_then(|installed| installed.as_ref()?.shown?.keys[index]);
    let Some(equivalent) = equivalent else {
        return true;
    };
    // SAFETY: `currentEvent` returns an NSEvent or nil; `type` and
    // `modifierFlags` return NSUInteger and are only sent to a live event.
    let pressed = unsafe {
        let Some(app) = macos_objc::shared_application() else {
            return true;
        };
        let event = macos_objc::send(app, CURRENT_EVENT);
        if event.is_null()
            || macos_objc::message::<GetUnsigned>()(event, macos_objc::selector(EVENT_TYPE))
                != KEY_DOWN_EVENT
        {
            return true;
        }
        let flags =
            macos_objc::message::<GetUnsigned>()(event, macos_objc::selector(MODIFIER_FLAGS));
        let characters = macos_objc::rust_string(macos_objc::message::<SendUnsigned>()(
            event,
            macos_objc::selector(CHARACTERS_WITHOUT_MODIFIERS),
            0,
        ));
        characters.is_some_and(|characters| is_chord(equivalent, &characters, flags))
    };
    !pressed
}

/// Applies `title` to `item` (an NSMenuItem or NSMenu).
///
/// # Safety
/// `item` must be nil or respond to `setTitle:`.
unsafe fn set_title(item: Object, title: &str) {
    let string = macos_objc::ns_string(title);
    macos_objc::send_with(item, SET_TITLE, string);
    macos_objc::release(string);
}

/// Makes `item` show `state` for the entry at `index`.
///
/// # Safety
/// `item` must be a live NSMenuItem, on the main thread.
unsafe fn show(item: Object, index: usize, state: &MenuState) {
    let entry = &ENTRIES[index];
    let open = state.open[index];
    set_title(item, entry.title(open));
    let checked = if entry.checked(open) {
        STATE_ON
    } else {
        STATE_OFF
    };
    macos_objc::message::<SendInteger>()(item, macos_objc::selector(SET_STATE), checked);
    let (key, modifiers) = match state.keys[index] {
        Some(equivalent) => {
            let mut buffer = [0; 4];
            (
                macos_objc::ns_string(equivalent.key.encode_utf8(&mut buffer)),
                equivalent.modifiers,
            )
        }
        None => (macos_objc::ns_string(""), 0),
    };
    macos_objc::send_with(item, SET_KEY_EQUIVALENT, key);
    macos_objc::release(key);
    macos_objc::message::<SendUnsigned>()(
        item,
        macos_objc::selector(SET_KEY_EQUIVALENT_MASK),
        modifiers,
    );
}

/// A new, empty menu titled `title`, owned by the caller.
///
/// # Safety
/// Main thread only.
unsafe fn new_menu(title: &str) -> Object {
    let string = macos_objc::ns_string(title);
    let menu = macos_objc::send_with(
        macos_objc::alloc(macos_objc::class(NS_MENU)),
        INIT_WITH_TITLE,
        string,
    );
    macos_objc::release(string);
    menu
}

/// A new menu item for the entry at `index`, owned by the caller.
///
/// # Safety
/// Main thread only; `target` must outlive the item.
unsafe fn new_item(index: usize, target: Object) -> Object {
    let title = macos_objc::ns_string(ENTRIES[index].title(false));
    let key = macos_objc::ns_string("");
    let allocated = macos_objc::alloc(macos_objc::class(NS_MENU_ITEM));
    let item = if allocated.is_null() {
        allocated
    } else {
        macos_objc::message::<NewItem>()(
            allocated,
            macos_objc::selector(INIT_ITEM),
            title,
            macos_objc::selector(PERFORM),
            key,
        )
    };
    macos_objc::release(title);
    macos_objc::release(key);
    macos_objc::send_with(item, SET_TARGET, target);
    macos_objc::message::<SendInteger>()(item, macos_objc::selector(SET_TAG), index as isize);
    item
}

/// `[NSMenuItem separatorItem]`, autoreleased.
unsafe fn separator() -> Object {
    macos_objc::send(macos_objc::class(NS_MENU_ITEM), SEPARATOR_ITEM)
}

/// Adds the View or Help menu, holding `menu`'s entries, to the menu bar.
///
/// # Safety
/// Main thread only; `main_menu` must be the live main menu.
unsafe fn add_menu(
    main_menu: Object,
    menu: Menu,
    title: &str,
    target: Object,
    items: &mut [usize; ENTRIES.len()],
) {
    let submenu = new_menu(title);
    if submenu.is_null() {
        return;
    }
    for (index, entry) in ENTRIES.iter().enumerate().filter(|(_, e)| e.menu == menu) {
        if entry.separator_before {
            macos_objc::send_with(submenu, ADD_ITEM, separator());
        }
        let item = new_item(index, target);
        macos_objc::send_with(submenu, ADD_ITEM, item);
        items[index] = item as usize;
        macos_objc::release(item);
    }
    let holder = macos_objc::new_object(macos_objc::class(NS_MENU_ITEM));
    set_title(holder, title);
    macos_objc::send_with(holder, SET_SUBMENU, submenu);
    macos_objc::send_with(main_menu, ADD_ITEM, holder);
    macos_objc::release(holder);
    macos_objc::release(submenu);
}

/// Adds the items to the menu bar. Must run on the main thread, once winit
/// has built its default menu, which it has by the time the app is created.
pub(super) fn install(ctx: &egui::Context) {
    let _ = REPAINT_CONTEXT.set(ctx.clone());
    let Ok(mut installed) = INSTALLED.lock() else {
        return;
    };
    if installed.is_some() {
        return;
    }
    let Some(app) = macos_objc::shared_application() else {
        log::warn!("NSApplication unavailable; the menu bar has no view items");
        return;
    };
    // SAFETY: every function matches its type encoding; every message below
    // goes to a live object of the class that implements it, on the main
    // thread. Objects this code creates are released once something else
    // holds them; the target is kept for the life of the app, since menu
    // items don't retain their target.
    unsafe {
        let Some(class) = macos_objc::define_class(
            TARGET_CLASS,
            NS_OBJECT,
            &[
                (PERFORM, perform as *const c_void, PERFORM_TYPES),
                (VALIDATE, validate as *const c_void, VALIDATE_TYPES),
            ],
        ) else {
            log::warn!("could not define the menu target; the menu bar has no view items");
            return;
        };
        let target = macos_objc::new_object(class);
        let main_menu = macos_objc::send(app, MAIN_MENU);
        if target.is_null() || main_menu.is_null() {
            log::warn!("no main menu; the menu bar has no view items");
            return;
        }

        // AppKit adds "Show Tab Bar" items to a View menu for windows that
        // can be tabbed; this app's frameless windows have no use for tabs.
        macos_objc::message::<SendBool>()(
            macos_objc::class(NS_WINDOW),
            macos_objc::selector(SET_ALLOWS_TABBING),
            false,
        );

        let mut items = [0; ENTRIES.len()];
        let app_menu_item =
            macos_objc::message::<SendInteger>()(main_menu, macos_objc::selector(ITEM_AT_INDEX), 0);
        let app_menu = macos_objc::send(app_menu_item, SUBMENU);
        if !app_menu.is_null() {
            let count = macos_objc::message::<GetInteger>()(
                app_menu,
                macos_objc::selector(NUMBER_OF_ITEMS),
            );
            let mut at = SETTINGS_INDEX_IN_APP_MENU.min(count);
            let insert = macos_objc::message::<InsertAt>();
            for (index, _) in ENTRIES
                .iter()
                .enumerate()
                .filter(|(_, e)| e.menu == Menu::App)
            {
                let item = new_item(index, target);
                insert(
                    app_menu,
                    macos_objc::selector(INSERT_ITEM_AT_INDEX),
                    item,
                    at,
                );
                items[index] = item as usize;
                macos_objc::release(item);
                at += 1;
            }
            insert(
                app_menu,
                macos_objc::selector(INSERT_ITEM_AT_INDEX),
                separator(),
                at,
            );
        }
        add_menu(main_menu, Menu::View, VIEW_MENU_TITLE, target, &mut items);
        add_menu(main_menu, Menu::Help, HELP_MENU_TITLE, target, &mut items);

        *installed = Some(Installed { items, shown: None });
    }
}

impl App {
    /// Publishes this window's state to the menu bar and runs the items
    /// chosen since the last frame. Only the focused window does either, so
    /// the menu always shows, and acts on, the window in front; the main
    /// window stands in while none of the app's windows has focus.
    pub(super) fn sync_macos_menu(&mut self, ctx: &egui::Context) {
        // Read before `input`: egui's context is locked inside its closure,
        // and asking it for anything there would deadlock.
        let is_main_window = ctx.viewport_id() == egui::ViewportId::ROOT;
        let in_front = ctx.input(|i| {
            i.focused
                || (is_main_window && !i.raw.viewports.values().any(|v| v.focused == Some(true)))
        });
        if !in_front {
            return;
        }

        let chosen = PENDING
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default();
        for action in chosen {
            let Some(entry) = ENTRIES.iter().find(|e| e.action == action) else {
                continue;
            };
            if entry.should_toggle(self.view_is_open(action).unwrap_or(false)) {
                self.toggle_view(action);
            }
        }

        let state = MenuState::new(
            |action| self.view_is_open(action).unwrap_or(false),
            |action| {
                if self.shortcut_registry.is_os_reserved(action) {
                    None
                } else {
                    self.shortcut_registry.find_shortcut(action).cloned()
                }
            },
        );
        // Mark the state shown, then update the items without holding the
        // lock, since AppKit may validate items (and so lock) meanwhile.
        let items = {
            let Ok(mut installed) = INSTALLED.lock() else {
                return;
            };
            let Some(installed) = installed.as_mut() else {
                return;
            };
            if installed.shown == Some(state) {
                return;
            }
            installed.shown = Some(state);
            installed.items
        };
        for (index, &item) in items.iter().enumerate() {
            if item != 0 {
                // SAFETY: the menus hold these items for the life of the
                // app, and this runs on the main thread.
                unsafe { show(item as Object, index, &state) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_titlebar_toggle_is_in_the_menu_once() {
        for action in VIEW_TOGGLES {
            let count = ENTRIES.iter().filter(|e| e.action == action).count();
            assert_eq!(count, 1, "{action:?}");
        }
    }

    #[test]
    fn items_follow_mac_conventions() {
        let settings = ENTRIES
            .iter()
            .find(|e| e.action == AppAction::OpenSettings)
            .unwrap();
        assert_eq!(settings.menu, Menu::App);
        assert_eq!(settings.title(false), "Settings…");
        let shortcuts = ENTRIES
            .iter()
            .find(|e| e.action == AppAction::ToggleShortcutHelp)
            .unwrap();
        assert_eq!(shortcuts.menu, Menu::Help);
    }

    #[test]
    fn panel_items_say_what_choosing_them_does() {
        let sidebar = ENTRIES
            .iter()
            .find(|e| e.action == AppAction::ToggleLeftSidebar)
            .unwrap();
        assert_eq!(sidebar.title(true), "Hide Sidebar");
        assert_eq!(sidebar.title(false), "Show Sidebar");
        assert!(
            !sidebar.checked(true),
            "a Show/Hide title replaces the checkmark"
        );
    }

    #[test]
    fn overlays_are_checked_while_open() {
        let switcher = ENTRIES
            .iter()
            .find(|e| e.action == AppAction::OpenQuickSwitcher)
            .unwrap();
        assert!(switcher.checked(true));
        assert!(!switcher.checked(false));
        assert!(switcher.should_toggle(true), "choosing it again closes it");
    }

    #[test]
    fn settings_stays_open_when_chosen_again() {
        let settings = ENTRIES
            .iter()
            .find(|e| e.action == AppAction::OpenSettings)
            .unwrap();
        assert!(settings.should_toggle(false));
        assert!(!settings.should_toggle(true));
    }

    #[test]
    fn shortcuts_become_key_equivalents() {
        use egui::Key;
        let cmd_comma = Shortcut::primary(Key::Comma);
        assert_eq!(
            key_equivalent(&cmd_comma),
            Some(KeyEquivalent {
                key: ',',
                modifiers: COMMAND_MASK
            })
        );
        let cmd_shift_b = Shortcut::primary(Key::B).with_shift();
        assert_eq!(
            key_equivalent(&cmd_shift_b),
            Some(KeyEquivalent {
                key: 'b',
                modifiers: COMMAND_MASK | SHIFT_MASK
            })
        );
        let ctrl_opt_space = Shortcut::key(Key::Space).with_control().with_alt();
        assert_eq!(
            key_equivalent(&ctrl_opt_space),
            Some(KeyEquivalent {
                key: ' ',
                modifiers: CONTROL_MASK | OPTION_MASK
            })
        );
        assert_eq!(key_character(Key::F1), Some('\u{F704}'));
        assert_eq!(key_character(Key::F12), Some('\u{F70F}'));
        assert_eq!(key_character(Key::ArrowLeft), Some(LEFT_ARROW_KEY));
        assert_eq!(key_character(Key::Num7), Some('7'));
        assert_eq!(key_character(Key::Slash), Some('/'));
    }

    #[test]
    fn keys_without_a_name_have_no_key_equivalent() {
        assert_eq!(key_character(egui::Key::Copy), None);
    }

    #[test]
    fn chords_match_only_the_exact_press() {
        let equivalent = KeyEquivalent {
            key: 'b',
            modifiers: COMMAND_MASK | SHIFT_MASK,
        };
        // A capital is still the letter's chord.
        assert!(is_chord(equivalent, "B", COMMAND_MASK | SHIFT_MASK));
        // Device-dependent and lock bits outside the modifier mask are ignored.
        assert!(is_chord(
            equivalent,
            "b",
            COMMAND_MASK | SHIFT_MASK | 1 << 16 | 0x100
        ));
        assert!(!is_chord(equivalent, "b", COMMAND_MASK));
        assert!(!is_chord(equivalent, "c", COMMAND_MASK | SHIFT_MASK));
        assert!(!is_chord(equivalent, "", COMMAND_MASK | SHIFT_MASK));
        assert!(!is_chord(equivalent, "bb", COMMAND_MASK | SHIFT_MASK));
        let comma = KeyEquivalent {
            key: ',',
            modifiers: COMMAND_MASK | SHIFT_MASK,
        };
        assert!(is_chord(comma, ",", COMMAND_MASK | SHIFT_MASK));
        assert!(!is_chord(comma, "<", COMMAND_MASK | SHIFT_MASK));
    }

    #[test]
    fn menu_state_reads_each_entry() {
        let state = MenuState::new(
            |action| action == AppAction::ToggleRightSidebar,
            |action| {
                (action == AppAction::OpenSettings).then(|| Shortcut::primary(egui::Key::Comma))
            },
        );
        for (i, entry) in ENTRIES.iter().enumerate() {
            assert_eq!(state.open[i], entry.action == AppAction::ToggleRightSidebar);
            assert_eq!(
                state.keys[i].is_some(),
                entry.action == AppAction::OpenSettings
            );
        }
    }

    #[test]
    fn objc_names_are_nul_terminated() {
        for name in [
            TARGET_CLASS,
            NS_OBJECT,
            NS_MENU,
            NS_MENU_ITEM,
            NS_WINDOW,
            PERFORM,
            PERFORM_TYPES,
            VALIDATE,
            VALIDATE_TYPES,
            MAIN_MENU,
            ITEM_AT_INDEX,
            NUMBER_OF_ITEMS,
            SUBMENU,
            SET_SUBMENU,
            ADD_ITEM,
            INSERT_ITEM_AT_INDEX,
            INIT_WITH_TITLE,
            INIT_ITEM,
            SEPARATOR_ITEM,
            SET_TARGET,
            SET_TAG,
            TAG,
            SET_TITLE,
            SET_STATE,
            SET_KEY_EQUIVALENT,
            SET_KEY_EQUIVALENT_MASK,
            SET_ALLOWS_TABBING,
            CURRENT_EVENT,
            EVENT_TYPE,
            MODIFIER_FLAGS,
            CHARACTERS_WITHOUT_MODIFIERS,
        ] {
            assert_eq!(name.last(), Some(&0));
        }
    }
}
