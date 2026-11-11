use std::sync::{mpsc, Arc};

use parking_lot::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

mod download;
mod standalone;

pub use standalone::{cleanup_old_binary, handle_apply_update_flag};

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
        let asset_name = standalone::platform_asset_name();
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

    let needs_elevation = standalone::preflight_checks(&current_exe).is_err();

    let download_result = download::download(shared, ctx, &download_url);

    let bytes = match download_result {
        Ok(b) => b,
        Err(e) => {
            set_error(shared, ctx, &format!("Download failed: {e}"));
            return;
        }
    };

    if needs_elevation {
        if let Err(msg) = standalone::apply_update_elevated(&current_exe, &bytes) {
            set_error(shared, ctx, &msg);
            return;
        }
    } else if let Err(msg) = standalone::apply_update(&current_exe, &bytes) {
        set_error(shared, ctx, &msg);
        return;
    }

    {
        let mut s = shared.lock();
        s.status = UpdateStatus::RestartRequired;
    }
    ctx.request_repaint();
}

fn set_error(shared: &Arc<Mutex<UpdateState>>, ctx: &egui::Context, msg: &str) {
    log::error!("Update error: {msg}");
    let mut s = shared.lock();
    s.status = UpdateStatus::Error(msg.to_string());
    ctx.request_repaint();
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

#[cfg(test)]
mod tests {
    /// The Homebrew cask shipped from this repo must always point at the
    /// version being built, otherwise `brew upgrade` would install a
    /// different release than the one tagged from this commit.
    const CASK_SOURCE: &str = include_str!("../../Casks/terminal-studio.rb");
    const CASK_VERSION_PREFIX: &str = "version \"";

    fn cask_version(source: &str) -> Option<&str> {
        source.lines().find_map(|line| {
            line.trim()
                .strip_prefix(CASK_VERSION_PREFIX)
                .and_then(|rest| rest.split('"').next())
        })
    }

    #[test]
    fn cask_version_matches_cargo_version() {
        assert_eq!(cask_version(CASK_SOURCE), Some(env!("CARGO_PKG_VERSION")));
    }
}
