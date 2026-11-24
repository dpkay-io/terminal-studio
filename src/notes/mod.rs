//! Per-workspace notes, each kept in a file: the app's own file in its
//! notes folder, or one the user linked (an Obsidian vault page, any text
//! file), edited both here and in other apps. All file work happens on the
//! note worker thread; see `store` for the UI side.

mod disk;
mod legacy;
mod links;
mod merge;
mod store;
mod target;
mod worker;

pub use store::{NoteStatus, NoteStore, FLUSH_TIMEOUT};
pub use target::NoteKey;
