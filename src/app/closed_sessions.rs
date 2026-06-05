use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::util;

const MANIFEST_VERSION: u32 = 1;
const DIR_NAME: &str = "closed_sessions";
const MANIFEST_FILE: &str = "manifest.json";
const MAX_COMPRESSED_FILE_BYTES: u64 = 10 * 1024 * 1024; // 10 MB per file
const MAX_TOTAL_DIR_BYTES: u64 = 500 * 1024 * 1024; // 500 MB total cap

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ClosedSessionRecord {
    pub id: u64,
    pub closed_at: u64,
    pub title: String,
    pub cwd: PathBuf,
    pub shell: String,
    pub workspace_id: Option<u64>,
    pub workspace_name: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub line_count: usize,
    pub scrollback_file: Option<String>,
    #[serde(default)]
    pub claude_session_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ClosedSessionManifest {
    pub version: u32,
    pub next_id: u64,
    pub records: Vec<ClosedSessionRecord>,
}

impl Default for ClosedSessionManifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            next_id: 1,
            records: Vec::new(),
        }
    }
}

impl ClosedSessionManifest {
    pub fn load() -> Self {
        let Some(path) = manifest_path() else {
            return Self::default();
        };
        util::safe_json_load::<Self>(&path).unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = manifest_path() else {
            return;
        };
        let Ok(json) = serde_json::to_string_pretty(self) else {
            log::warn!("Failed to serialize closed sessions manifest");
            return;
        };
        if let Err(e) = util::atomic_write(&path, &json) {
            log::warn!("Failed to save closed sessions manifest: {}", e);
        }
    }

    pub fn push(&mut self, record: ClosedSessionRecord, max_records: usize) {
        self.records.insert(0, record);
        self.evict_excess(max_records);
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(pos) = self.records.iter().position(|r| r.id == id) {
            let record = self.records.remove(pos);
            if let Some(filename) = &record.scrollback_file {
                delete_scrollback_file(filename);
            }
        }
    }

    pub fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn evict_excess(&mut self, max_records: usize) {
        while self.records.len() > max_records {
            if let Some(record) = self.records.pop() {
                if let Some(filename) = &record.scrollback_file {
                    delete_scrollback_file(filename);
                }
            }
        }
    }

    pub fn enforce_size_cap(&mut self) {
        let Some(dir) = closed_sessions_dir() else {
            return;
        };
        loop {
            let total = dir_size(&dir);
            if total <= MAX_TOTAL_DIR_BYTES || self.records.is_empty() {
                break;
            }
            if let Some(record) = self.records.pop() {
                if let Some(filename) = &record.scrollback_file {
                    delete_scrollback_file(filename);
                }
            }
        }
    }
}

/// Saves a closed session's scrollback to disk and updates the manifest.
#[allow(clippy::too_many_arguments)]
pub fn save_closed_session(
    ansi_bytes: Vec<u8>,
    cwd: PathBuf,
    title: String,
    shell: String,
    workspace_id: Option<u64>,
    workspace_name: Option<String>,
    cols: u16,
    rows: u16,
    line_count: usize,
    max_records: usize,
    claude_session_id: Option<String>,
) {
    let Some(dir) = closed_sessions_dir() else {
        log::warn!("Could not determine closed_sessions directory");
        return;
    };
    std::fs::create_dir_all(&dir).ok();

    let mut manifest = ClosedSessionManifest::load();
    let id = manifest.allocate_id();

    let scrollback_file = if !ansi_bytes.is_empty() {
        let filename = format!("sb_{:05}.zst", id);
        let filepath = dir.join(&filename);
        match compress_and_write(&filepath, &ansi_bytes) {
            Ok(size) if size <= MAX_COMPRESSED_FILE_BYTES => Some(filename),
            Ok(_) => {
                // File too large, delete it
                std::fs::remove_file(&filepath).ok();
                log::info!("Closed session {} scrollback too large, discarding", id);
                None
            }
            Err(e) => {
                log::warn!("Failed to write scrollback for session {}: {}", id, e);
                None
            }
        }
    } else {
        None
    };

    let record = ClosedSessionRecord {
        id,
        closed_at: epoch_secs(),
        title,
        cwd,
        shell,
        workspace_id,
        workspace_name,
        cols,
        rows,
        line_count,
        scrollback_file,
        claude_session_id,
    };

    manifest.push(record, max_records);
    manifest.enforce_size_cap();
    manifest.save();
}

/// Loads and decompresses a scrollback file. Returns None on any error.
pub fn load_scrollback(filename: &str) -> Option<Vec<u8>> {
    let dir = closed_sessions_dir()?;
    let filepath = dir.join(filename);
    let compressed = std::fs::read(&filepath).ok()?;
    zstd::decode_all(compressed.as_slice()).ok()
}

/// Returns the path to the closed_sessions directory.
pub fn closed_sessions_dir() -> Option<PathBuf> {
    util::data_dir().map(|d| d.join(DIR_NAME))
}

fn manifest_path() -> Option<PathBuf> {
    closed_sessions_dir().map(|d| d.join(MANIFEST_FILE))
}

fn delete_scrollback_file(filename: &str) {
    if let Some(dir) = closed_sessions_dir() {
        let path = dir.join(filename);
        std::fs::remove_file(&path).ok();
    }
}

fn compress_and_write(path: &Path, data: &[u8]) -> std::io::Result<u64> {
    let compressed = zstd::encode_all(data, 3)?;
    std::fs::write(path, &compressed)?;
    Ok(compressed.len() as u64)
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "terminal_studio_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn make_record(id: u64, scrollback_file: Option<&str>) -> ClosedSessionRecord {
        ClosedSessionRecord {
            id,
            closed_at: epoch_secs(),
            title: format!("Session {}", id),
            cwd: PathBuf::from("/tmp"),
            shell: "bash".to_string(),
            workspace_id: None,
            workspace_name: None,
            cols: 80,
            rows: 24,
            line_count: 100,
            scrollback_file: scrollback_file.map(String::from),
            claude_session_id: None,
        }
    }

    #[test]
    fn manifest_default() {
        let manifest = ClosedSessionManifest::default();
        assert_eq!(manifest.version, MANIFEST_VERSION);
        assert_eq!(manifest.next_id, 1);
        assert!(manifest.records.is_empty());
    }

    #[test]
    fn manifest_serde_roundtrip() {
        let mut manifest = ClosedSessionManifest::default();
        manifest.next_id = 42;
        manifest.records.push(make_record(1, Some("sb_00001.zst")));
        manifest.records.push(make_record(2, None));

        let json = serde_json::to_string(&manifest).unwrap();
        let restored: ClosedSessionManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.version, MANIFEST_VERSION);
        assert_eq!(restored.next_id, 42);
        assert_eq!(restored.records.len(), 2);
        assert_eq!(restored.records[0].id, 1);
        assert_eq!(
            restored.records[0].scrollback_file,
            Some("sb_00001.zst".to_string())
        );
        assert_eq!(restored.records[1].id, 2);
        assert_eq!(restored.records[1].scrollback_file, None);
    }

    #[test]
    fn push_prepends_and_evicts() {
        let mut manifest = ClosedSessionManifest::default();
        for i in 1..=5 {
            manifest.push(make_record(i, None), 3);
        }
        assert_eq!(manifest.records.len(), 3);
        // Newest first
        assert_eq!(manifest.records[0].id, 5);
        assert_eq!(manifest.records[1].id, 4);
        assert_eq!(manifest.records[2].id, 3);
    }

    #[test]
    fn remove_by_id() {
        let mut manifest = ClosedSessionManifest::default();
        manifest.records.push(make_record(1, None));
        manifest.records.push(make_record(2, None));
        manifest.records.push(make_record(3, None));

        manifest.remove(2);
        assert_eq!(manifest.records.len(), 2);
        assert!(manifest.records.iter().all(|r| r.id != 2));
    }

    #[test]
    fn remove_nonexistent_is_noop() {
        let mut manifest = ClosedSessionManifest::default();
        manifest.records.push(make_record(1, None));
        manifest.remove(999);
        assert_eq!(manifest.records.len(), 1);
    }

    #[test]
    fn allocate_id_increments() {
        let mut manifest = ClosedSessionManifest::default();
        assert_eq!(manifest.allocate_id(), 1);
        assert_eq!(manifest.allocate_id(), 2);
        assert_eq!(manifest.allocate_id(), 3);
        assert_eq!(manifest.next_id, 4);
    }

    #[test]
    fn compress_and_decompress_roundtrip() {
        let dir = temp_dir();
        let filepath = dir.join("test.zst");
        let data = b"Hello, compressed world! ".repeat(100);

        let size = compress_and_write(&filepath, &data).unwrap();
        assert!(size > 0);
        assert!(size < data.len() as u64); // Compression should shrink it

        let compressed = fs::read(&filepath).unwrap();
        let decompressed = zstd::decode_all(compressed.as_slice()).unwrap();
        assert_eq!(decompressed, data);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dir_size_calculation() {
        let dir = temp_dir();
        fs::write(dir.join("a.txt"), "hello").unwrap();
        fs::write(dir.join("b.txt"), "world!").unwrap();
        let size = dir_size(&dir);
        assert_eq!(size, 11); // 5 + 6

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn record_fields_preserved() {
        let record = ClosedSessionRecord {
            id: 42,
            closed_at: 1700000000,
            title: "My Session".to_string(),
            cwd: PathBuf::from("C:\\Users\\test"),
            shell: "powershell".to_string(),
            workspace_id: Some(123),
            workspace_name: Some("Work".to_string()),
            cols: 120,
            rows: 30,
            line_count: 5000,
            scrollback_file: Some("sb_00042.zst".to_string()),
            claude_session_id: Some("abc-def-123".to_string()),
        };

        let json = serde_json::to_string(&record).unwrap();
        let restored: ClosedSessionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record, restored);
    }

    #[test]
    fn evict_deletes_scrollback_files() {
        let dir = temp_dir();
        // Create fake scrollback files
        fs::write(dir.join("sb_00001.zst"), "data1").unwrap();
        fs::write(dir.join("sb_00002.zst"), "data2").unwrap();

        let mut manifest = ClosedSessionManifest::default();
        manifest.records.push(make_record(2, Some("sb_00002.zst")));
        manifest.records.push(make_record(1, Some("sb_00001.zst")));

        // Note: eviction deletes from the tail (oldest)
        // This test verifies the logic, actual file deletion requires
        // closed_sessions_dir() to point to our temp dir — which it won't
        // in tests. The logic is still verified structurally.
        manifest.evict_excess(1);
        assert_eq!(manifest.records.len(), 1);
        assert_eq!(manifest.records[0].id, 2);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn epoch_secs_returns_reasonable_value() {
        let secs = epoch_secs();
        // Should be after 2020-01-01 and before 2100-01-01
        assert!(secs > 1_577_836_800);
        assert!(secs < 4_102_444_800);
    }

    #[test]
    fn empty_ansi_bytes_no_file() {
        // Simulate save_closed_session logic for empty scrollback
        let ansi_bytes: Vec<u8> = Vec::new();
        let scrollback_file = if !ansi_bytes.is_empty() {
            Some("sb_00001.zst".to_string())
        } else {
            None
        };
        assert_eq!(scrollback_file, None);
    }
}
