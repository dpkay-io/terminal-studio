//! Reading and writing note files. Only the note worker thread calls these:
//! a linked file may sit on a slow or network disk.

use std::fmt;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use super::target::NoteTarget;
use crate::util::MAX_TEXT_FILE_BYTES;

/// What was at a note's path when last read or written: a change to any
/// part means another app changed it. A folder (or other non-file) has a
/// stamp too, so its removal is seen like any other change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskStamp {
    modified: Option<SystemTime>,
    len: u64,
    is_file: bool,
}

impl DiskStamp {
    fn of(meta: &std::fs::Metadata) -> Self {
        Self {
            modified: meta.modified().ok(),
            len: meta.len(),
            is_file: meta.is_file(),
        }
    }
}

/// Why a note file could not be used.
#[derive(Debug)]
pub enum NoteFileError {
    /// Bigger than the app edits.
    TooLarge,
    /// Not UTF-8 text.
    NotText,
    /// A folder (or other non-file) is at the path.
    NotAFile,
    Io(io::Error),
}

impl fmt::Display for NoteFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(
                f,
                "it is larger than {} MB",
                MAX_TEXT_FILE_BYTES / (1024 * 1024)
            ),
            Self::NotText => write!(f, "it is not a text file"),
            Self::NotAFile => write!(f, "it is not a file"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl From<io::Error> for NoteFileError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// The stamp of what is at `path` (a file or not), `None` when nothing is.
pub fn stamp(path: &Path) -> Option<DiskStamp> {
    std::fs::metadata(path).ok().map(|m| DiskStamp::of(&m))
}

/// The file's text and stamp, `Ok(None)` when it does not exist. The stamp
/// is taken before reading, so a write racing the read is seen as a change
/// next time.
pub fn read_note(path: &Path) -> Result<Option<(String, DiskStamp)>, NoteFileError> {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() {
        return Err(NoteFileError::NotAFile);
    }
    if meta.len() > MAX_TEXT_FILE_BYTES {
        return Err(NoteFileError::TooLarge);
    }
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let text = String::from_utf8(bytes).map_err(|_| NoteFileError::NotText)?;
    Ok(Some((text, DiskStamp::of(&meta))))
}

/// Writes `text` to the note's file and returns its new stamp. An emptied
/// own note removes its file (`Ok(None)`); a linked file is never removed.
pub fn write_note(target: &NoteTarget, text: &str) -> Result<Option<DiskStamp>, NoteFileError> {
    if text.is_empty() && !target.linked {
        return match std::fs::remove_file(&target.path) {
            Ok(()) => Ok(None),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        };
    }
    if std::fs::metadata(&target.path).is_ok_and(|m| !m.is_file()) {
        return Err(NoteFileError::NotAFile);
    }
    crate::util::atomic_write(&target.path, text)?;
    Ok(stamp(&target.path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_reads_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_note(&dir.path().join("none.md")).unwrap().is_none());
        assert!(stamp(&dir.path().join("none.md")).is_none());
    }

    #[test]
    fn written_text_reads_back_with_the_same_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let target = NoteTarget::linked(dir.path().join("a.md"));
        let written = write_note(&target, "hello").unwrap();
        let (text, read) = read_note(&target.path).unwrap().unwrap();
        assert_eq!(text, "hello");
        assert_eq!(Some(read), written);
        assert_eq!(stamp(&target.path), written);
    }

    #[test]
    fn a_folder_has_a_stamp_unlike_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        std::fs::create_dir(&path).unwrap();
        let folder = stamp(&path).expect("a folder is something at the path");
        std::fs::remove_dir(&path).unwrap();
        assert_eq!(stamp(&path), None);
        std::fs::write(&path, "").unwrap();
        assert_ne!(stamp(&path), Some(folder));
    }

    #[test]
    fn a_change_in_length_changes_the_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        std::fs::write(&path, "one").unwrap();
        let before = stamp(&path);
        std::fs::write(&path, "one two").unwrap();
        assert_ne!(stamp(&path), before);
    }

    #[test]
    fn an_emptied_own_note_removes_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = NoteTarget::own(dir.path().join("a.md"));
        write_note(&target, "x").unwrap();
        assert_eq!(write_note(&target, "").unwrap(), None);
        assert!(!target.path.exists());
        // Already gone: still fine.
        assert_eq!(write_note(&target, "").unwrap(), None);
    }

    #[test]
    fn an_emptied_linked_note_keeps_its_file_empty() {
        let dir = tempfile::tempdir().unwrap();
        let target = NoteTarget::linked(dir.path().join("a.md"));
        write_note(&target, "x").unwrap();
        assert!(write_note(&target, "").unwrap().is_some());
        assert_eq!(std::fs::read_to_string(&target.path).unwrap(), "");
    }

    #[test]
    fn folders_binary_and_huge_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            read_note(dir.path()),
            Err(NoteFileError::NotAFile)
        ));
        assert!(matches!(
            write_note(&NoteTarget::linked(dir.path().to_path_buf()), "x"),
            Err(NoteFileError::NotAFile)
        ));
        let binary = dir.path().join("b.bin");
        std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x80]).unwrap();
        assert!(matches!(read_note(&binary), Err(NoteFileError::NotText)));
        let huge = dir.path().join("huge.md");
        std::fs::File::create(&huge)
            .unwrap()
            .set_len(MAX_TEXT_FILE_BYTES + 1)
            .unwrap();
        assert!(matches!(read_note(&huge), Err(NoteFileError::TooLarge)));
    }

    #[test]
    fn errors_read_as_reasons() {
        assert_eq!(NoteFileError::NotText.to_string(), "it is not a text file");
        assert_eq!(
            NoteFileError::TooLarge.to_string(),
            "it is larger than 16 MB"
        );
        assert_eq!(NoteFileError::NotAFile.to_string(), "it is not a file");
    }
}
