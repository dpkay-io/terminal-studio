use std::path::PathBuf;
use std::sync::{mpsc, Arc};

use parking_lot::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/dpkay-io/terminal-studio/releases/latest";
const CHECK_INTERVAL_SECS: u64 = 86_400; // 24 hours

#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    UpToDate,
    UpdateAvailable {
        version: String,
        download_url: String,
    },
    Downloading {
        progress_pct: f32,
    },
    RestartRequired,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct UpdateState {
    pub status: UpdateStatus,
    #[allow(dead_code)]
    pub current_version: String,
    pub last_check: Option<u64>,
}

enum Command {
    Check,
    StartUpdate,
}

pub struct UpdateChecker {
    state: Arc<Mutex<UpdateState>>,
    cmd_tx: mpsc::Sender<Command>,
}

impl UpdateChecker {
    pub fn spawn(ctx: egui::Context, last_check: Option<u64>) -> Option<Self> {
        let state = Arc::new(Mutex::new(UpdateState {
            status: UpdateStatus::Idle,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            last_check,
        }));
        let shared = state.clone();
        let (cmd_tx, cmd_rx) = mpsc::channel();

        if let Err(e) = std::thread::Builder::new()
            .name("update-checker".into())
            .spawn(move || worker(shared, ctx, cmd_rx, last_check))
        {
            log::error!("failed to spawn update-checker thread: {e}");
            return None;
        }

        Some(Self { state, cmd_tx })
    }

    pub fn state(&self) -> UpdateState {
        self.state.lock().clone()
    }

    pub fn trigger_check(&self) {
        let _ = self.cmd_tx.send(Command::Check);
    }

    pub fn start_update(&self) {
        let _ = self.cmd_tx.send(Command::StartUpdate);
    }
}

fn worker(
    shared: Arc<Mutex<UpdateState>>,
    ctx: egui::Context,
    cmd_rx: mpsc::Receiver<Command>,
    last_check: Option<u64>,
) {
    std::thread::sleep(std::time::Duration::from_secs(5));

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let should_auto_check = match last_check {
        Some(ts) => now.saturating_sub(ts) >= CHECK_INTERVAL_SECS,
        None => true,
    };

    if should_auto_check {
        do_check(&shared, &ctx);
    }

    loop {
        match cmd_rx.recv() {
            Ok(Command::Check) => {
                do_check(&shared, &ctx);
            }
            Ok(Command::StartUpdate) => {
                do_update(&shared, &ctx);
            }
            Err(_) => break,
        }
    }
}

fn do_check(shared: &Arc<Mutex<UpdateState>>, ctx: &egui::Context) {
    {
        let mut s = shared.lock();
        s.status = UpdateStatus::Checking;
    }
    ctx.request_repaint();

    let result = (|| -> anyhow::Result<serde_json::Value> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("terminal-studio-updater")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let resp = client.get(GITHUB_RELEASES_URL).send()?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("GitHub API returned {}", resp.status()));
        }
        resp.json().map_err(Into::into)
    })();

    match result {
        Ok(json) => parse_release(shared, ctx, &json),
        Err(e) => set_error(shared, ctx, &format!("Network error: {e}")),
    }
}

fn parse_release(shared: &Arc<Mutex<UpdateState>>, ctx: &egui::Context, json: &serde_json::Value) {
    let tag = json["tag_name"].as_str().unwrap_or("");
    let version_str = tag.strip_prefix('v').unwrap_or(tag);

    let remote = match semver::Version::parse(version_str) {
        Ok(v) => v,
        Err(_) => {
            set_error(shared, ctx, &format!("Invalid remote version: {tag}"));
            return;
        }
    };

    let current = match semver::Version::parse(env!("CARGO_PKG_VERSION")) {
        Ok(v) => v,
        Err(_) => {
            set_error(shared, ctx, "Invalid local version");
            return;
        }
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if remote > current {
        let asset_name = platform_asset_name();
        let download_url = json["assets"]
            .as_array()
            .and_then(|assets| {
                assets
                    .iter()
                    .find(|a| a["name"].as_str() == Some(asset_name))
            })
            .and_then(|a| a["browser_download_url"].as_str())
            .unwrap_or("")
            .to_string();

        if download_url.is_empty() {
            set_error(shared, ctx, "No binary found for this platform");
            return;
        }

        let mut s = shared.lock();
        s.status = UpdateStatus::UpdateAvailable {
            version: version_str.to_string(),
            download_url,
        };
        s.last_check = Some(now);
    } else {
        let mut s = shared.lock();
        s.status = UpdateStatus::UpToDate;
        s.last_check = Some(now);
    }
    ctx.request_repaint();
}

fn do_update(shared: &Arc<Mutex<UpdateState>>, ctx: &egui::Context) {
    let download_url = {
        let s = shared.lock();
        match &s.status {
            UpdateStatus::UpdateAvailable { download_url, .. } => download_url.clone(),
            _ => return,
        }
    };

    {
        let mut s = shared.lock();
        s.status = UpdateStatus::Downloading { progress_pct: 0.0 };
    }
    ctx.request_repaint();

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            set_error(shared, ctx, &format!("Cannot locate binary: {e}"));
            return;
        }
    };

    if let Err(msg) = preflight_checks(&current_exe) {
        set_error(shared, ctx, &msg);
        return;
    }

    let download_result = (|| -> anyhow::Result<Vec<u8>> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("terminal-studio-updater")
            .timeout(std::time::Duration::from_secs(300))
            .build()?;

        let resp = client.get(&download_url).send()?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Download failed: HTTP {}", resp.status()));
        }

        let bytes = resp.bytes()?.to_vec();

        {
            let mut s = shared.lock();
            s.status = UpdateStatus::Downloading {
                progress_pct: 100.0,
            };
        }
        ctx.request_repaint();

        Ok(bytes)
    })();

    let bytes = match download_result {
        Ok(b) => b,
        Err(e) => {
            set_error(shared, ctx, &format!("Download failed: {e}"));
            return;
        }
    };

    if let Err(msg) = apply_update(&current_exe, &bytes) {
        set_error(shared, ctx, &msg);
        return;
    }

    {
        let mut s = shared.lock();
        s.status = UpdateStatus::RestartRequired;
    }
    ctx.request_repaint();
}

fn preflight_checks(binary_path: &std::path::Path) -> Result<(), String> {
    let dir = binary_path
        .parent()
        .ok_or_else(|| "Cannot determine binary directory".to_string())?;

    let test_file = dir.join(".ts_update_preflight");
    std::fs::write(&test_file, b"test")
        .map_err(|e| format!("No write permission to install directory: {e}"))?;
    let _ = std::fs::remove_file(&test_file);

    Ok(())
}

fn apply_update(current_exe: &std::path::Path, new_bytes: &[u8]) -> Result<(), String> {
    let dir = current_exe
        .parent()
        .ok_or_else(|| "Cannot determine binary directory".to_string())?;

    let temp_path = dir.join("terminal-studio.update");
    std::fs::write(&temp_path, new_bytes)
        .map_err(|e| format!("Failed to write update file: {e}"))?;

    // Platform-specific binary replacement
    let backup_path = backup_path_for(current_exe);

    // Rename current → backup
    if let Err(e) = std::fs::rename(current_exe, &backup_path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to backup current binary: {e}"));
    }

    // Move new → current
    if let Err(e) = std::fs::rename(&temp_path, current_exe) {
        // Rollback
        let _ = std::fs::rename(&backup_path, current_exe);
        return Err(format!("Failed to install new binary (rolled back): {e}"));
    }

    // Unix: set executable permission
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(current_exe, std::fs::Permissions::from_mode(0o755));
    }

    Ok(())
}

fn backup_path_for(exe: &std::path::Path) -> PathBuf {
    if cfg!(target_os = "windows") {
        exe.with_extension("exe.old")
    } else {
        exe.with_extension("old")
    }
}

fn platform_asset_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "terminal-studio-windows.exe"
    } else if cfg!(target_os = "macos") {
        "terminal-studio-macos-arm"
    } else {
        "terminal-studio-linux"
    }
}

fn set_error(shared: &Arc<Mutex<UpdateState>>, ctx: &egui::Context, msg: &str) {
    log::error!("Update error: {msg}");
    let mut s = shared.lock();
    s.status = UpdateStatus::Error(msg.to_string());
    ctx.request_repaint();
}

pub fn cleanup_old_binary() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let ext = if cfg!(target_os = "windows") {
        "exe.old"
    } else {
        "old"
    };
    let old = exe.with_extension(ext);
    if old.exists() {
        let _ = std::fs::remove_file(&old);
    }
    // Also clean up leftover .update file from interrupted downloads
    if let Some(dir) = exe.parent() {
        let update_file = dir.join("terminal-studio.update");
        if update_file.exists() {
            let _ = std::fs::remove_file(&update_file);
        }
    }
}

pub fn restart_app() {
    if let Ok(exe) = std::env::current_exe() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let _ = std::process::Command::new(&exe).args(&args).spawn();
    }
    std::process::exit(0);
}
