use std::sync::{mpsc, Arc};

use parking_lot::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/dpkay-io/terminal-studio/releases/latest";
const CHECK_INTERVAL_SECS: u64 = 86_400; // 24 hours
const APPLY_UPDATE_FLAG: &str = "--apply-update";
const RESTARTING_FLAG: &str = "--restarting";

pub fn is_restarting() -> bool {
    std::env::args().any(|a| a == RESTARTING_FLAG)
}

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
        match cmd_rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(Command::Check) => {
                do_check(&shared, &ctx);
            }
            Ok(Command::StartUpdate) => {
                do_update(&shared, &ctx);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
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
    let (download_url, _version) = {
        let s = shared.lock();
        match &s.status {
            UpdateStatus::UpdateAvailable {
                download_url,
                version,
            } => (download_url.clone(), version.clone()),
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

    let needs_elevation = preflight_checks(&current_exe).is_err();

    let download_result = download_binary(shared, ctx, &download_url);

    let bytes = match download_result {
        Ok(b) => b,
        Err(e) => {
            set_error(shared, ctx, &format!("Download failed: {e}"));
            return;
        }
    };

    if needs_elevation {
        if let Err(msg) = apply_update_elevated(&current_exe, &bytes) {
            set_error(shared, ctx, &msg);
            return;
        }
    } else if let Err(msg) = apply_update(&current_exe, &bytes) {
        set_error(shared, ctx, &msg);
        return;
    }

    {
        let mut s = shared.lock();
        s.status = UpdateStatus::RestartRequired;
    }
    ctx.request_repaint();
}

fn download_binary(
    shared: &Arc<Mutex<UpdateState>>,
    ctx: &egui::Context,
    download_url: &str,
) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("terminal-studio-updater")
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let mut resp = client.get(download_url).send()?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("HTTP {}", resp.status()));
    }

    let content_length = resp.content_length();
    let total = content_length.unwrap_or(0);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 32768];
    loop {
        let n = std::io::Read::read(&mut resp, &mut buf)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        downloaded += n as u64;
        if total > 0 {
            let pct = (downloaded as f32 / total as f32) * 100.0;
            let mut s = shared.lock();
            s.status = UpdateStatus::Downloading { progress_pct: pct };
            ctx.request_repaint();
        }
    }

    if bytes.is_empty() {
        return Err(anyhow::anyhow!("Downloaded update is empty"));
    }
    if let Some(expected) = content_length {
        if bytes.len() as u64 != expected {
            return Err(anyhow::anyhow!(
                "Truncated: got {} of {} bytes",
                bytes.len(),
                expected
            ));
        }
    }

    {
        let mut s = shared.lock();
        s.status = UpdateStatus::Downloading {
            progress_pct: 100.0,
        };
    }
    ctx.request_repaint();

    Ok(bytes)
}

fn apply_update_elevated(target_exe: &std::path::Path, new_bytes: &[u8]) -> Result<(), String> {
    let temp_dir = std::env::temp_dir();
    let temp_name = if cfg!(target_os = "windows") {
        "terminal-studio-update.exe"
    } else {
        "terminal-studio-update"
    };
    let temp_path = temp_dir.join(temp_name);
    std::fs::write(&temp_path, new_bytes)
        .map_err(|e| format!("Failed to write update to temp: {e}"))?;

    let current_exe =
        std::env::current_exe().map_err(|e| format!("Cannot locate current exe: {e}"))?;

    let args = [
        APPLY_UPDATE_FLAG.to_string(),
        temp_path.display().to_string(),
        target_exe.display().to_string(),
    ];

    let result = run_elevated(&current_exe, &args);
    let _ = std::fs::remove_file(&temp_path);
    result
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

fn apply_update(_current_exe: &std::path::Path, new_bytes: &[u8]) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let dir = _current_exe
            .parent()
            .ok_or_else(|| "Cannot determine binary directory".to_string())?;
        let temp_path = dir.join("terminal-studio.update");
        std::fs::write(&temp_path, new_bytes)
            .map_err(|e| format!("Failed to write update file: {e}"))?;
        if let Err(e) = self_replace::self_replace(&temp_path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!("Failed to replace binary: {e}"));
        }
        let _ = std::fs::remove_file(&temp_path);
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let dir = _current_exe
            .parent()
            .ok_or_else(|| "Cannot determine binary directory".to_string())?;

        let temp_path = dir.join("terminal-studio.update");
        std::fs::write(&temp_path, new_bytes)
            .map_err(|e| format!("Failed to write update file: {e}"))?;

        let backup_path = backup_path_for(_current_exe);

        if let Err(e) = std::fs::rename(_current_exe, &backup_path) {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!("Failed to backup current binary: {e}"));
        }

        if let Err(e) = std::fs::rename(&temp_path, _current_exe) {
            let _ = std::fs::rename(&backup_path, _current_exe);
            return Err(format!("Failed to install new binary (rolled back): {e}"));
        }

        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(_current_exe, std::fs::Permissions::from_mode(0o755));

        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn backup_path_for(exe: &std::path::Path) -> std::path::PathBuf {
    exe.with_extension("old")
}

fn platform_asset_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "terminal-studio-windows.exe"
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "terminal-studio-macos-arm"
        } else {
            "terminal-studio-macos-intel"
        }
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

/// Spawn a new copy of ourselves and exit. Returns `true` if the spawn
/// succeeded (the process is about to exit) or `false` on failure. Callers
/// must stop retrying after a failure to avoid spawning processes every frame.
pub fn restart_app() -> bool {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log::error!("Failed to locate current executable for restart: {}", e);
            return false;
        }
    };
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != APPLY_UPDATE_FLAG && a != RESTARTING_FLAG)
        .collect();
    let mut cmd = std::process::Command::new(&exe);
    cmd.args(&args);
    cmd.arg(RESTARTING_FLAG);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    match cmd.spawn() {
        Ok(_) => std::process::exit(0),
        Err(e) => {
            log::error!("Failed to restart: {}", e);
            false
        }
    }
}

pub fn handle_apply_update_flag() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let flag_pos = args.iter().position(|a| a == APPLY_UPDATE_FLAG);
    let Some(pos) = flag_pos else {
        return false;
    };

    if args.len() < pos + 3 {
        eprintln!("Usage: --apply-update <source> <target>");
        return true;
    }

    let source = std::path::Path::new(&args[pos + 1]);
    let target = std::path::Path::new(&args[pos + 2]);

    if !source.exists() {
        eprintln!("Source file not found: {}", source.display());
        return true;
    }

    let bytes = match std::fs::read(source) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to read source: {e}");
            return true;
        }
    };

    if let Err(e) = apply_update(target, &bytes) {
        eprintln!("Apply update failed: {e}");
    }

    true
}

#[cfg(target_os = "windows")]
fn run_elevated(exe: &std::path::Path, args: &[String]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let params = args
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let file: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let parameters: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();

    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_HIDE;

    let success = unsafe { ShellExecuteExW(&mut info) };
    if success == 0 {
        return Err("UAC elevation was denied or failed".to_string());
    }

    if info.hProcess != 0 {
        unsafe {
            WaitForSingleObject(info.hProcess, INFINITE);
            CloseHandle(info.hProcess);
        }
    }

    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn run_elevated(exe: &std::path::Path, args: &[String]) -> Result<(), String> {
    let status = std::process::Command::new("sudo")
        .arg(exe)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to run sudo: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err("Elevated update failed".to_string())
    }
}
