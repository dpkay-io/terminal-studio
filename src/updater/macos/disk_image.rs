//! Attach and detach disk images with `hdiutil`, and find images that are
//! still attached (e.g. after a timed-out attach or an app that quit).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::process::{self, CommandSpec};

const HDIUTIL: &str = "/usr/bin/hdiutil";
const HDIUTIL_TIMEOUT: Duration = Duration::from_secs(120);
/// Read-only, hidden from Finder, no auto-opened windows.
const ATTACH_ARGS: [&str; 4] = ["attach", "-nobrowse", "-readonly", "-noautoopen"];
const MOUNTPOINT_FLAG: &str = "-mountpoint";
const DETACH_ARGS: [&str; 2] = ["detach", "-force"];
const INFO_ARGS: [&str; 2] = ["info", "-plist"];

/// `hdiutil info -plist`: the parts this module uses.
#[derive(Deserialize)]
struct Info {
    #[serde(default)]
    images: Vec<Image>,
}

#[derive(Deserialize)]
struct Image {
    #[serde(rename = "image-path")]
    image_path: PathBuf,
    #[serde(rename = "system-entities", default)]
    entities: Vec<Entity>,
}

#[derive(Deserialize)]
struct Entity {
    #[serde(rename = "dev-entry")]
    dev_entry: String,
}

impl Image {
    /// The whole-disk device (`/dev/diskN`); detaching it detaches the image.
    fn device(&self) -> Option<&str> {
        self.entities.first().map(|e| e.dev_entry.as_str())
    }
}

/// A disk image attached by us, detached again on drop.
pub(super) struct AttachedImage {
    /// Canonical path. We attach by it, and `hdiutil info` reports the path
    /// exactly as it was attached, so matching on it is reliable.
    image: PathBuf,
}

/// How to run `hdiutil attach`; injectable so the retry is testable.
struct AttachPolicy<'a> {
    program: &'a str,
    timeout: Duration,
    attempts: u32,
    retry_delay: Duration,
}

impl AttachPolicy<'static> {
    /// hdiutil refuses as busy while other disk-image work runs (seen with
    /// parallel image operations); a few seconds later it succeeds.
    const DEFAULT: Self = Self {
        program: HDIUTIL,
        timeout: HDIUTIL_TIMEOUT,
        attempts: 3,
        retry_delay: Duration::from_secs(2),
    };
}

/// hdiutil's message for EAGAIN, the only failure worth retrying.
const BUSY_ERROR: &str = "Resource temporarily unavailable";
const ATTACH_FAILED: &str = "Failed to open the update image";

impl AttachedImage {
    /// Attach `dmg` read-only at `mount_point`.
    pub fn attach(dmg: &Path, mount_point: &Path) -> Result<Self, String> {
        Self::attach_with(dmg, mount_point, &AttachPolicy::DEFAULT)
    }

    fn attach_with(dmg: &Path, mount_point: &Path, policy: &AttachPolicy) -> Result<Self, String> {
        let image = std::fs::canonicalize(dmg).map_err(|e| format!("{ATTACH_FAILED}: {e}"))?;
        let attach = CommandSpec::new(policy.program, policy.timeout)
            .args(ATTACH_ARGS)
            .arg(MOUNTPOINT_FLAG)
            .arg(mount_point)
            .arg(&image);
        let mut attempt = 1;
        loop {
            match process::run(&attach) {
                Ok(_) => return Ok(Self { image }),
                Err(e) if e.contains(BUSY_ERROR) && attempt < policy.attempts => {
                    log::info!("updater: hdiutil busy (attempt {attempt}), retrying: {e}");
                    std::thread::sleep(policy.retry_delay);
                    attempt += 1;
                }
                Err(e) => return Err(format!("{ATTACH_FAILED}: {e}")),
            }
        }
    }
}

impl Drop for AttachedImage {
    fn drop(&mut self) {
        if let Err(e) = detach_matching(|path| path == self.image) {
            log::warn!("updater: failed to detach {}: {e}", self.image.display());
        }
    }
}

/// Detach every attached image whose (canonical) path matches. Returns how
/// many were detached.
pub(super) fn detach_matching(matches: impl Fn(&Path) -> bool) -> Result<usize, String> {
    let mut detached = 0;
    for image in attached_images()? {
        if !matches(&image.image_path) {
            continue;
        }
        let Some(device) = image.device() else {
            continue;
        };
        let detach = CommandSpec::new(HDIUTIL, HDIUTIL_TIMEOUT)
            .args(DETACH_ARGS)
            .arg(device);
        process::run(&detach).map_err(|e| format!("{device}: {e}"))?;
        detached += 1;
    }
    Ok(detached)
}

fn attached_images() -> Result<Vec<Image>, String> {
    let spec = CommandSpec::new(HDIUTIL, HDIUTIL_TIMEOUT).args(INFO_ARGS);
    let out = process::run(&spec)?;
    plist::from_bytes::<Info>(out.as_bytes())
        .map(|info| info.images)
        .map_err(|e| format!("Unreadable hdiutil info: {e}"))
}

/// Whether something is mounted on `path` (it sits on another device than
/// its parent directory).
pub(super) fn is_mount_point(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let device = |p: &Path| std::fs::metadata(p).map(|m| m.dev()).ok();
    match (device(path), path.parent().and_then(device)) {
        (Some(own), Some(parent)) => own != parent,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::bundle_version::test_support::{fake_dmg, hdiutil_lock};
    use super::*;

    /// Short enough that hdiutil is killed mid-attach (it took >0.9s here).
    const MID_ATTACH_TIMEOUT: Duration = Duration::from_millis(300);
    const HELPER_SETTLE: Duration = Duration::from_secs(2);
    const NO_DELAY: Duration = Duration::ZERO;

    fn is_attached(dmg: &Path) -> bool {
        let image = std::fs::canonicalize(dmg).unwrap();
        attached_images()
            .unwrap()
            .iter()
            .any(|i| i.image_path == image)
    }

    /// A stand-in `hdiutil` that logs each call and fails with `stderr` on
    /// the first `failures` calls, then succeeds. Returns it and its call log.
    fn fake_hdiutil(dir: &Path, failures: u32, stderr: &str) -> (PathBuf, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let calls = dir.join("calls");
        let program = dir.join("hdiutil");
        let script = format!(
            "#!/bin/sh\necho x >> '{log}'\n\
             [ \"$(wc -l < '{log}')\" -gt {failures} ] && exit 0\n\
             echo 'hdiutil: attach failed - {stderr}' >&2\nexit 1\n",
            log = calls.display()
        );
        std::fs::write(&program, script).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        (program, calls)
    }

    fn call_count(calls: &Path) -> usize {
        std::fs::read_to_string(calls).map_or(0, |log| log.lines().count())
    }

    fn quick_policy(program: &Path) -> AttachPolicy<'_> {
        AttachPolicy {
            program: program.to_str().unwrap(),
            retry_delay: NO_DELAY,
            ..AttachPolicy::DEFAULT
        }
    }

    #[test]
    fn attach_retries_while_hdiutil_is_busy() {
        let dir = tempfile::tempdir().unwrap();
        let (program, calls) = fake_hdiutil(dir.path(), 2, BUSY_ERROR);
        let dmg = dir.path().join("update.dmg");
        std::fs::write(&dmg, "").unwrap();

        let attached = AttachedImage::attach_with(&dmg, dir.path(), &quick_policy(&program));

        assert!(attached.is_ok(), "{:?}", attached.err());
        assert_eq!(call_count(&calls), 3);
    }

    #[test]
    fn attach_gives_up_after_the_last_busy_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let (program, calls) = fake_hdiutil(dir.path(), u32::MAX, BUSY_ERROR);
        let dmg = dir.path().join("update.dmg");
        std::fs::write(&dmg, "").unwrap();

        let err = AttachedImage::attach_with(&dmg, dir.path(), &quick_policy(&program))
            .err()
            .unwrap();

        assert!(err.contains(BUSY_ERROR), "{err}");
        assert_eq!(call_count(&calls), AttachPolicy::DEFAULT.attempts as usize);
    }

    #[test]
    fn attach_does_not_retry_other_failures() {
        let dir = tempfile::tempdir().unwrap();
        let (program, calls) = fake_hdiutil(dir.path(), u32::MAX, "image not recognized");
        let dmg = dir.path().join("update.dmg");
        std::fs::write(&dmg, "").unwrap();

        let result = AttachedImage::attach_with(&dmg, dir.path(), &quick_policy(&program));

        assert!(result.is_err());
        assert_eq!(call_count(&calls), 1);
    }

    #[test]
    fn attach_mounts_and_drop_detaches() {
        let _hdiutil = hdiutil_lock();
        let dir = tempfile::tempdir().unwrap();
        let dmg = fake_dmg(dir.path(), "0.16.0");
        let point = dir.path().join("mnt");

        let attached = AttachedImage::attach(&dmg, &point).unwrap();
        assert!(is_mount_point(&point));
        assert!(point.join("Terminal Studio.app").is_dir());

        drop(attached);
        assert!(!is_attached(&dmg));
        assert!(!is_mount_point(&point));
    }

    #[test]
    fn timed_out_attach_leaves_nothing_attached() {
        // From a shell, `kill -9 <hdiutil pid>` mid-attach left the image
        // attached and mounted (4/4): hdiutil's helper finishes on its own.
        // Through the runner nothing leaked at 50-900ms, whether it killed
        // the process group or only hdiutil, so this checks the outcome, not
        // a mechanism. Images that do leak are detached by the startup sweep.
        let _hdiutil = hdiutil_lock();
        let dir = tempfile::tempdir().unwrap();
        let dmg = fake_dmg(dir.path(), "0.16.0");
        let point = dir.path().join("mnt");
        let policy = AttachPolicy {
            timeout: MID_ATTACH_TIMEOUT,
            ..AttachPolicy::DEFAULT
        };

        // A fast machine may finish in time; then the guard detaches it.
        drop(AttachedImage::attach_with(&dmg, &point, &policy));
        // Give a surviving helper time to finish what it started.
        std::thread::sleep(HELPER_SETTLE);
        let leaked = is_attached(&dmg);
        if leaked {
            let image = std::fs::canonicalize(&dmg).unwrap();
            let _ = detach_matching(|p| p == image);
        }

        assert!(!leaked, "a timed-out attach left the image attached");
    }

    #[test]
    fn detach_matching_detaches_only_matching_images() {
        let _hdiutil = hdiutil_lock();
        let dir = tempfile::tempdir().unwrap();
        let keep_dmg = fake_dmg(&dir.path().join("keep"), "0.16.0");
        let gone_dmg = fake_dmg(&dir.path().join("gone"), "0.16.0");
        let keep = AttachedImage::attach(&keep_dmg, &dir.path().join("m1")).unwrap();
        let gone = AttachedImage::attach(&gone_dmg, &dir.path().join("m2")).unwrap();
        let gone_dir = std::fs::canonicalize(dir.path().join("gone")).unwrap();

        let detached = detach_matching(|p| p.starts_with(&gone_dir)).unwrap();

        assert_eq!(detached, 1);
        assert!(is_attached(&keep_dmg));
        assert!(!is_attached(&gone_dmg));
        drop(keep);
        drop(gone);
    }

    #[test]
    fn is_mount_point_false_for_plain_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_mount_point(dir.path()));
        assert!(!is_mount_point(&dir.path().join("missing")));
    }
}
