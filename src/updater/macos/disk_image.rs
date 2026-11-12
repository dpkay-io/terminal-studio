//! Attach and detach disk images (`diskutil image` where `hdiutil` is
//! deprecated, `hdiutil` before), and find images that are still attached
//! (e.g. after a timed-out attach or an app that quit).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::os_version;
use super::process::{self, CommandSpec};

const HDIUTIL: &str = "/usr/bin/hdiutil";
const DISKUTIL: &str = "/usr/sbin/diskutil";
const DISK_TOOL_TIMEOUT: Duration = Duration::from_secs(120);
/// Read-only, hidden from Finder, no auto-opened windows; the mount-point
/// flag comes last, followed by the mount point and the image.
const HDIUTIL_ATTACH_ARGS: [&str; 5] = [
    "attach",
    "-nobrowse",
    "-readonly",
    "-noautoopen",
    "-mountpoint",
];
/// diskutil's spelling of the same attach (it never auto-opens windows).
const DISKUTIL_ATTACH_ARGS: [&str; 5] = [
    "image",
    "attach",
    "--readOnly",
    "--nobrowse",
    "--mountPoint",
];
/// Followed by the whole-disk device.
const HDIUTIL_DETACH_ARGS: [&str; 2] = ["detach", "-force"];
const DISKUTIL_DETACH_ARGS: [&str; 2] = ["eject", "force"];
/// Lists attached images on every macOS (not deprecated; diskutil has no
/// equivalent listing).
const INFO_ARGS: [&str; 2] = ["info", "-plist"];
/// From macOS 27 `hdiutil attach`/`detach` print deprecation warnings (which
/// would leak into error messages) and point at `diskutil image`. Older
/// releases keep hdiutil, which works there without warnings.
const DISKUTIL_FROM_MACOS: u32 = 27;

/// The program that attaches and ejects disk images on this Mac.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImageTool {
    Hdiutil,
    Diskutil,
}

impl ImageTool {
    /// An unreadable version keeps hdiutil, which every supported macOS has.
    fn for_macos(major: Option<u32>) -> Self {
        match major {
            Some(major) if major >= DISKUTIL_FROM_MACOS => Self::Diskutil,
            _ => Self::Hdiutil,
        }
    }

    fn current() -> Self {
        Self::for_macos(os_version::major())
    }

    fn program(self) -> &'static str {
        match self {
            Self::Hdiutil => HDIUTIL,
            Self::Diskutil => DISKUTIL,
        }
    }

    fn attach_args(self) -> [&'static str; 5] {
        match self {
            Self::Hdiutil => HDIUTIL_ATTACH_ARGS,
            Self::Diskutil => DISKUTIL_ATTACH_ARGS,
        }
    }

    fn detach_args(self) -> [&'static str; 2] {
        match self {
            Self::Hdiutil => HDIUTIL_DETACH_ARGS,
            Self::Diskutil => DISKUTIL_DETACH_ARGS,
        }
    }
}

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
    /// exactly as it was attached (by either tool), so matching is reliable.
    image: PathBuf,
    /// Detach with the tool that attached it.
    tool: ImageTool,
}

/// How to run the attach; injectable so the retry is testable.
struct AttachPolicy<'a> {
    tool: ImageTool,
    /// The tool's program, or a stand-in in tests.
    program: &'a str,
    timeout: Duration,
    attempts: u32,
    retry_delay: Duration,
}

impl AttachPolicy<'static> {
    /// hdiutil refuses as busy while other disk-image work runs (seen with
    /// parallel image operations); a few seconds later it succeeds.
    const ATTEMPTS: u32 = 3;
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    fn for_tool(tool: ImageTool) -> Self {
        Self {
            tool,
            program: tool.program(),
            timeout: DISK_TOOL_TIMEOUT,
            attempts: Self::ATTEMPTS,
            retry_delay: Self::RETRY_DELAY,
        }
    }
}

/// The message for EAGAIN, the only failure worth retrying.
const BUSY_ERROR: &str = "Resource temporarily unavailable";
const ATTACH_FAILED: &str = "Failed to open the update image";

fn attach_spec(policy: &AttachPolicy, mount_point: &Path, image: &Path) -> CommandSpec {
    CommandSpec::new(policy.program, policy.timeout)
        .args(policy.tool.attach_args())
        .arg(mount_point)
        .arg(image)
}

fn detach_spec(tool: ImageTool, device: &str) -> CommandSpec {
    CommandSpec::new(tool.program(), DISK_TOOL_TIMEOUT)
        .args(tool.detach_args())
        .arg(device)
}

impl AttachedImage {
    /// Attach `dmg` read-only at `mount_point`.
    pub fn attach(dmg: &Path, mount_point: &Path) -> Result<Self, String> {
        Self::attach_with(
            dmg,
            mount_point,
            &AttachPolicy::for_tool(ImageTool::current()),
        )
    }

    fn attach_with(dmg: &Path, mount_point: &Path, policy: &AttachPolicy) -> Result<Self, String> {
        let image = std::fs::canonicalize(dmg).map_err(|e| format!("{ATTACH_FAILED}: {e}"))?;
        let attach = attach_spec(policy, mount_point, &image);
        let mut attempt = 1;
        loop {
            match process::run(&attach) {
                Ok(_) => {
                    return Ok(Self {
                        image,
                        tool: policy.tool,
                    })
                }
                Err(e) if e.contains(BUSY_ERROR) && attempt < policy.attempts => {
                    log::info!(
                        "updater: {:?} busy (attempt {attempt}), retrying: {e}",
                        policy.tool
                    );
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
        if let Err(e) = detach_matching_with(self.tool, |path| path == self.image) {
            log::warn!("updater: failed to detach {}: {e}", self.image.display());
        }
    }
}

/// Detach every attached image whose (canonical) path matches. Returns how
/// many were detached.
pub(super) fn detach_matching(matches: impl Fn(&Path) -> bool) -> Result<usize, String> {
    detach_matching_with(ImageTool::current(), matches)
}

fn detach_matching_with(tool: ImageTool, matches: impl Fn(&Path) -> bool) -> Result<usize, String> {
    let mut detached = 0;
    for image in attached_images()? {
        if !matches(&image.image_path) {
            continue;
        }
        let Some(device) = image.device() else {
            continue;
        };
        process::run(&detach_spec(tool, device)).map_err(|e| format!("{device}: {e}"))?;
        detached += 1;
    }
    Ok(detached)
}

fn attached_images() -> Result<Vec<Image>, String> {
    let spec = CommandSpec::new(HDIUTIL, DISK_TOOL_TIMEOUT).args(INFO_ARGS);
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

    /// Short enough that hdiutil is killed mid-attach (it took >0.9s here);
    /// a faster diskutil may finish first, and then the guard detaches it.
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
            ..AttachPolicy::for_tool(ImageTool::current())
        }
    }

    /// The tools to exercise on this Mac: hdiutil (still present everywhere,
    /// and the fallback) plus diskutil where the app would choose it. Not
    /// every macOS that has `diskutil image` takes our flags: macOS 26 (the
    /// CI runner) rejects them with a usage error. Real-image tests run once
    /// per tool.
    fn tools_on_this_mac() -> Vec<ImageTool> {
        let mut tools = vec![ImageTool::Hdiutil];
        if ImageTool::current() == ImageTool::Diskutil {
            tools.push(ImageTool::Diskutil);
        }
        tools
    }

    fn args(spec: &CommandSpec) -> Vec<String> {
        spec.args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn diskutil_only_where_hdiutil_is_deprecated() {
        assert_eq!(ImageTool::for_macos(Some(11)), ImageTool::Hdiutil);
        assert_eq!(ImageTool::for_macos(Some(26)), ImageTool::Hdiutil);
        assert_eq!(ImageTool::for_macos(Some(27)), ImageTool::Diskutil);
        assert_eq!(ImageTool::for_macos(Some(28)), ImageTool::Diskutil);
    }

    #[test]
    fn unknown_macos_keeps_hdiutil() {
        assert_eq!(ImageTool::for_macos(None), ImageTool::Hdiutil);
    }

    #[test]
    fn attach_commands_mount_read_only_and_hidden() {
        let point = Path::new("/tmp/mnt");
        let image = Path::new("/tmp/update.dmg");
        let hdiutil = attach_spec(&AttachPolicy::for_tool(ImageTool::Hdiutil), point, image);
        assert_eq!(hdiutil.program, Path::new("/usr/bin/hdiutil"));
        assert_eq!(
            args(&hdiutil),
            [
                "attach",
                "-nobrowse",
                "-readonly",
                "-noautoopen",
                "-mountpoint",
                "/tmp/mnt",
                "/tmp/update.dmg"
            ]
        );
        let diskutil = attach_spec(&AttachPolicy::for_tool(ImageTool::Diskutil), point, image);
        assert_eq!(diskutil.program, Path::new("/usr/sbin/diskutil"));
        assert_eq!(
            args(&diskutil),
            [
                "image",
                "attach",
                "--readOnly",
                "--nobrowse",
                "--mountPoint",
                "/tmp/mnt",
                "/tmp/update.dmg"
            ]
        );
    }

    #[test]
    fn detach_commands_force_the_device_off() {
        let hdiutil = detach_spec(ImageTool::Hdiutil, "/dev/disk9");
        assert_eq!(hdiutil.program, Path::new("/usr/bin/hdiutil"));
        assert_eq!(args(&hdiutil), ["detach", "-force", "/dev/disk9"]);
        let diskutil = detach_spec(ImageTool::Diskutil, "/dev/disk9");
        assert_eq!(diskutil.program, Path::new("/usr/sbin/diskutil"));
        assert_eq!(args(&diskutil), ["eject", "force", "/dev/disk9"]);
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
        assert_eq!(
            call_count(&calls),
            AttachPolicy::for_tool(ImageTool::current()).attempts as usize
        );
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
    fn this_mac_uses_the_tool_its_version_calls_for() {
        assert_eq!(
            ImageTool::current(),
            ImageTool::for_macos(os_version::major())
        );
    }

    #[test]
    fn attach_mounts_and_drop_detaches() {
        let _hdiutil = hdiutil_lock();
        for tool in tools_on_this_mac() {
            let dir = tempfile::tempdir().unwrap();
            let dmg = fake_dmg(dir.path(), "0.16.0");
            let point = dir.path().join("mnt");

            let attached =
                AttachedImage::attach_with(&dmg, &point, &AttachPolicy::for_tool(tool)).unwrap();
            assert!(is_mount_point(&point), "{tool:?}");
            assert!(point.join("Terminal Studio.app").is_dir(), "{tool:?}");

            drop(attached);
            assert!(!is_attached(&dmg), "{tool:?}");
            assert!(!is_mount_point(&point), "{tool:?}");
        }
    }

    #[test]
    fn timed_out_attach_leaves_nothing_attached() {
        // From a shell, `kill -9 <hdiutil pid>` mid-attach left the image
        // attached and mounted (4/4): hdiutil's helper finishes on its own.
        // Through the runner nothing leaked at 50-900ms, whether it killed
        // the process group or only hdiutil, so this checks the outcome, not
        // a mechanism. Images that do leak are detached by the startup sweep.
        let _hdiutil = hdiutil_lock();
        for tool in tools_on_this_mac() {
            let dir = tempfile::tempdir().unwrap();
            let dmg = fake_dmg(dir.path(), "0.16.0");
            let point = dir.path().join("mnt");
            let policy = AttachPolicy {
                timeout: MID_ATTACH_TIMEOUT,
                ..AttachPolicy::for_tool(tool)
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

            assert!(
                !leaked,
                "{tool:?}: a timed-out attach left the image attached"
            );
        }
    }

    #[test]
    fn detach_matching_detaches_only_matching_images() {
        let _hdiutil = hdiutil_lock();
        for tool in tools_on_this_mac() {
            let dir = tempfile::tempdir().unwrap();
            let keep_dmg = fake_dmg(&dir.path().join("keep"), "0.16.0");
            let gone_dmg = fake_dmg(&dir.path().join("gone"), "0.16.0");
            let policy = AttachPolicy::for_tool(tool);
            let keep =
                AttachedImage::attach_with(&keep_dmg, &dir.path().join("m1"), &policy).unwrap();
            let gone =
                AttachedImage::attach_with(&gone_dmg, &dir.path().join("m2"), &policy).unwrap();
            let gone_dir = std::fs::canonicalize(dir.path().join("gone")).unwrap();

            let detached = detach_matching_with(tool, |p| p.starts_with(&gone_dir)).unwrap();

            assert_eq!(detached, 1, "{tool:?}");
            assert!(is_attached(&keep_dmg), "{tool:?}");
            assert!(!is_attached(&gone_dmg), "{tool:?}");
            drop(keep);
            drop(gone);
        }
    }

    #[test]
    fn is_mount_point_false_for_plain_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_mount_point(dir.path()));
        assert!(!is_mount_point(&dir.path().join("missing")));
    }
}
