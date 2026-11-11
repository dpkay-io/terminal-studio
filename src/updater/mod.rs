use std::sync::{mpsc, Arc};

use parking_lot::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

mod download;
mod install;
#[cfg(target_os = "macos")]
mod macos;
mod standalone;

use install::InstallKind;
pub use standalone::{cleanup_old_binary, handle_apply_update_flag};

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/dpkay-io/terminal-studio/releases/latest";
const CHECK_INTERVAL_SECS: u64 = 86_400; // 24 hours
const APPLY_UPDATE_FLAG: &str = "--apply-update";
const RESTARTING_FLAG: &str = "--restarting";
const NO_PLATFORM_ASSET: &str = "No binary found for this platform";

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
        /// `None` when this install updates without downloading a release
        /// asset itself (Homebrew fetches its own DMG).
        download_url: Option<String>,
    },
    Downloading {
        progress_pct: f32,
    },
    /// Applying an update that has no measurable progress (e.g. running brew).
    /// Only the macOS bundle/Homebrew paths have such a phase.
    #[cfg(target_os = "macos")]
    Installing {
        message: &'static str,
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
    // Off the UI thread: detection stats a few paths.
    let install = InstallKind::detect();
    install.cleanup_previous_update();
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
        do_check(&shared, &ctx, &install);
    }

    loop {
        match cmd_rx.recv_timeout(std::time::Duration::from_secs(2)) {
            Ok(Command::Check) => {
                do_check(&shared, &ctx, &install);
            }
            Ok(Command::StartUpdate) => {
                do_update(&shared, &ctx, &install);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn do_check(shared: &Arc<Mutex<UpdateState>>, ctx: &egui::Context, install: &InstallKind) {
    set_status(shared, ctx, UpdateStatus::Checking);

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
        Ok(json) => parse_release(shared, ctx, &json, install),
        Err(e) => set_error(shared, ctx, &format!("Network error: {e}")),
    }
}

fn parse_release(
    shared: &Arc<Mutex<UpdateState>>,
    ctx: &egui::Context,
    json: &serde_json::Value,
    install: &InstallKind,
) {
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
        let download_url = match install.asset_name() {
            None => None,
            Some(asset_name) => match asset_url(json, asset_name) {
                Some(url) => Some(url),
                None => {
                    set_error(shared, ctx, NO_PLATFORM_ASSET);
                    return;
                }
            },
        };

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

/// `browser_download_url` of the release asset called `name`.
fn asset_url(json: &serde_json::Value, name: &str) -> Option<String> {
    json["assets"]
        .as_array()?
        .iter()
        .find(|a| a["name"].as_str() == Some(name))?["browser_download_url"]
        .as_str()
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

fn do_update(shared: &Arc<Mutex<UpdateState>>, ctx: &egui::Context, install: &InstallKind) {
    let (version, download_url) = match &shared.lock().status {
        UpdateStatus::UpdateAvailable {
            version,
            download_url,
        } => (version.clone(), download_url.clone()),
        _ => return,
    };
    log::info!("updater: installing v{version} ({install:?})");

    let result = match install {
        InstallKind::Standalone => update_binary(shared, ctx, download_url.as_deref()),
        #[cfg(target_os = "macos")]
        InstallKind::AppBundle { bundle } => {
            update_app_bundle(shared, ctx, bundle, download_url.as_deref(), &version)
        }
        #[cfg(target_os = "macos")]
        InstallKind::Homebrew { brew, bundle } => {
            set_status(
                shared,
                ctx,
                UpdateStatus::Installing {
                    message: macos::homebrew::STATUS_MESSAGE,
                },
            );
            macos::homebrew::upgrade(brew, bundle, &version)
        }
    };

    match result {
        Ok(()) => set_status(shared, ctx, UpdateStatus::RestartRequired),
        Err(msg) => set_error(shared, ctx, &msg),
    }
}

/// Download the platform binary and swap the running executable in place.
fn update_binary(
    shared: &Arc<Mutex<UpdateState>>,
    ctx: &egui::Context,
    download_url: Option<&str>,
) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|e| format!("Cannot locate binary: {e}"))?;
    let needs_elevation = standalone::preflight_checks(&current_exe).is_err();
    let bytes = fetch_asset(shared, ctx, download_url)?;

    if needs_elevation {
        standalone::apply_update_elevated(&current_exe, &bytes)
    } else {
        standalone::apply_update(&current_exe, &bytes)
    }
}

/// Download the release DMG and replace the whole `.app` bundle with it.
#[cfg(target_os = "macos")]
fn update_app_bundle(
    shared: &Arc<Mutex<UpdateState>>,
    ctx: &egui::Context,
    bundle: &std::path::Path,
    download_url: Option<&str>,
    version: &str,
) -> Result<(), String> {
    let dmg = fetch_asset(shared, ctx, download_url)?;
    set_status(
        shared,
        ctx,
        UpdateStatus::Installing {
            message: macos::app_bundle::STATUS_MESSAGE,
        },
    );
    macos::app_bundle::install_from_dmg(&dmg, bundle, version)
}

/// Download the release asset with progress shown as `Downloading`.
fn fetch_asset(
    shared: &Arc<Mutex<UpdateState>>,
    ctx: &egui::Context,
    download_url: Option<&str>,
) -> Result<Vec<u8>, String> {
    let download_url = download_url.ok_or_else(|| NO_PLATFORM_ASSET.to_string())?;
    set_status(shared, ctx, UpdateStatus::Downloading { progress_pct: 0.0 });
    download::download(shared, ctx, download_url).map_err(|e| format!("Download failed: {e}"))
}

fn set_status(shared: &Arc<Mutex<UpdateState>>, ctx: &egui::Context, status: UpdateStatus) {
    shared.lock().status = status;
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

    use super::install::InstallKind;
    use super::*;

    const NEWER_TAG: &str = "v999.0.0";
    const NEWER_VERSION: &str = "999.0.0";
    const ASSET_URL_BASE: &str = "https://example.invalid/";

    fn release_json(tag: &str, assets: &[&str]) -> serde_json::Value {
        let assets: Vec<_> = assets
            .iter()
            .map(|name| {
                serde_json::json!({
                    "name": name,
                    "browser_download_url": format!("{ASSET_URL_BASE}{name}"),
                })
            })
            .collect();
        serde_json::json!({ "tag_name": tag, "assets": assets })
    }

    fn status_after(json: &serde_json::Value, install: &InstallKind) -> UpdateStatus {
        let shared = Arc::new(Mutex::new(UpdateState {
            status: UpdateStatus::Idle,
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            last_check: None,
        }));
        parse_release(&shared, &egui::Context::default(), json, install);
        let status = shared.lock().status.clone();
        status
    }

    #[test]
    fn parse_release_standalone_requires_asset() {
        let status = status_after(&release_json(NEWER_TAG, &[]), &InstallKind::Standalone);
        assert_eq!(
            status,
            UpdateStatus::Error("No binary found for this platform".to_string())
        );
    }

    #[test]
    fn parse_release_standalone_picks_platform_asset() {
        let asset = standalone::platform_asset_name();
        let json = release_json(NEWER_TAG, &["unrelated.zip", asset]);
        assert_eq!(
            status_after(&json, &InstallKind::Standalone),
            UpdateStatus::UpdateAvailable {
                version: NEWER_VERSION.to_string(),
                download_url: Some(format!("{ASSET_URL_BASE}{asset}")),
            }
        );
    }

    #[test]
    fn parse_release_up_to_date_for_current_version() {
        let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
        let status = status_after(&release_json(&tag, &[]), &InstallKind::Standalone);
        assert_eq!(status, UpdateStatus::UpToDate);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn parse_release_homebrew_needs_no_asset() {
        let install = InstallKind::Homebrew {
            brew: "/opt/homebrew/bin/brew".into(),
            bundle: "/Applications/Terminal Studio.app".into(),
        };
        assert_eq!(
            status_after(&release_json(NEWER_TAG, &[]), &install),
            UpdateStatus::UpdateAvailable {
                version: NEWER_VERSION.to_string(),
                download_url: None,
            }
        );
    }
}
