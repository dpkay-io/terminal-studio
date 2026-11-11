//! Update a DMG-installed `.app` by replacing the whole bundle with the one
//! from the release DMG, so `Info.plist` (Finder / About version) and the
//! bundle's code signature stay consistent with the new executable.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::bundle_version;
use super::process::{self, CommandSpec};

/// Shown while the DMG is mounted and the bundle copied.
pub(in crate::updater) const STATUS_MESSAGE: &str = "Installing\u{2026}";

/// Bundle name inside the release DMG (release.yml "Package macOS").
const DMG_APP_NAME: &str = "Terminal Studio.app";
const HDIUTIL: &str = "/usr/bin/hdiutil";
const DITTO: &str = "/usr/bin/ditto";
const HDIUTIL_TIMEOUT: Duration = Duration::from_secs(120);
const DITTO_TIMEOUT: Duration = Duration::from_secs(120);

/// Staging/backup bundles live next to the installed one (same volume, so
/// the swap is two atomic renames) and are hidden from Finder.
const HIDDEN_PREFIX: &str = ".";
const STAGING_SUFFIX: &str = ".update";
const BACKUP_SUFFIX: &str = ".old";

/// Per-process scratch directory under the system temp dir.
const SCRATCH_DIR_PREFIX: &str = "terminal-studio-update-";
const DMG_FILE_NAME: &str = "update.dmg";
const MOUNT_DIR_NAME: &str = "mnt";

pub(super) fn staging_path(bundle: &Path) -> PathBuf {
    sibling(bundle, STAGING_SUFFIX)
}

pub(super) fn backup_path(bundle: &Path) -> PathBuf {
    sibling(bundle, BACKUP_SUFFIX)
}

fn sibling(bundle: &Path, suffix: &str) -> PathBuf {
    let name = bundle
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| DMG_APP_NAME.to_string());
    bundle.with_file_name(format!("{HIDDEN_PREFIX}{name}{suffix}"))
}

/// Remove staging/backup bundles left by an earlier update (or one that was
/// interrupted). Called from the update worker at startup.
pub(in crate::updater) fn cleanup_leftovers(bundle: &Path) {
    for leftover in [staging_path(bundle), backup_path(bundle)] {
        remove_dir_if_present(&leftover);
    }
}

fn remove_dir_if_present(path: &Path) {
    if path.exists() {
        if let Err(e) = std::fs::remove_dir_all(path) {
            log::warn!("updater: could not remove {}: {e}", path.display());
        }
    }
}

/// Move `bundle` aside to its backup path and `staged` into its place. If the
/// second rename fails the original bundle is moved back.
pub(super) fn swap(bundle: &Path, staged: &Path) -> Result<(), String> {
    let backup = backup_path(bundle);
    remove_dir_if_present(&backup);
    std::fs::rename(bundle, &backup)
        .map_err(|e| format!("Failed to move the current app aside: {e}"))?;
    if let Err(e) = std::fs::rename(staged, bundle) {
        if let Err(restore) = std::fs::rename(&backup, bundle) {
            log::error!(
                "updater: rollback failed, app left at {}: {restore}",
                backup.display()
            );
        }
        return Err(format!("Failed to install the new app (rolled back): {e}"));
    }
    Ok(())
}

/// Install the `.app` from the release DMG `dmg` over `bundle`, after
/// checking the image really carries version `target`.
pub(in crate::updater) fn install_from_dmg(
    dmg: &[u8],
    bundle: &Path,
    target: &str,
) -> Result<(), String> {
    let parent = bundle.parent().unwrap_or(bundle);
    super::super::standalone::preflight_checks(bundle).map_err(|_| {
        format!(
            "No write access to {} \u{2014} reinstall from the DMG",
            parent.display()
        )
    })?;

    let scratch = ScratchDir::create()?;
    let dmg_path = scratch.path.join(DMG_FILE_NAME);
    std::fs::write(&dmg_path, dmg).map_err(|e| format!("Failed to save the update: {e}"))?;
    let mount = Mount::attach(&dmg_path, &scratch.path.join(MOUNT_DIR_NAME))?;

    let source = mount.point.join(DMG_APP_NAME);
    if !source.is_dir() {
        return Err(format!("The update image has no {DMG_APP_NAME}"));
    }

    let staged = staging_path(bundle);
    remove_dir_if_present(&staged);
    let result = stage_and_swap(&source, &staged, bundle, target);
    if result.is_err() {
        remove_dir_if_present(&staged);
    }
    result
}

fn stage_and_swap(source: &Path, staged: &Path, bundle: &Path, target: &str) -> Result<(), String> {
    let copy = CommandSpec::new(DITTO, DITTO_TIMEOUT)
        .arg(source)
        .arg(staged);
    process::run(&copy).map_err(|e| format!("Failed to copy the new app: {e}"))?;
    let found = bundle_version::read(staged)?;
    if !bundle_version::is_at_least(&found, target)? {
        return Err(format!("The update image is v{found}, expected v{target}"));
    }
    swap(bundle, staged)
}

/// Temp directory removed on drop.
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn create() -> Result<Self, String> {
        let path = super::unique_temp_path(SCRATCH_DIR_PREFIX);
        remove_dir_if_present(&path);
        std::fs::create_dir_all(&path)
            .map_err(|e| format!("Failed to create a temp directory: {e}"))?;
        Ok(Self { path })
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        remove_dir_if_present(&self.path);
    }
}

/// A read-only, Finder-hidden DMG mount, detached on drop.
struct Mount {
    point: PathBuf,
}

impl Mount {
    fn attach(dmg: &Path, point: &Path) -> Result<Self, String> {
        let attach = CommandSpec::new(HDIUTIL, HDIUTIL_TIMEOUT)
            .arg("attach")
            .arg("-nobrowse")
            .arg("-readonly")
            .arg("-noautoopen")
            .arg("-mountpoint")
            .arg(point)
            .arg(dmg);
        process::run(&attach).map_err(|e| format!("Failed to open the update image: {e}"))?;
        Ok(Self {
            point: point.to_path_buf(),
        })
    }
}

impl Drop for Mount {
    fn drop(&mut self) {
        let detach = CommandSpec::new(HDIUTIL, HDIUTIL_TIMEOUT)
            .arg("detach")
            .arg("-force")
            .arg(&self.point);
        if let Err(e) = process::run(&detach) {
            log::warn!("updater: failed to detach {}: {e}", self.point.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::bundle_version::{self, test_support::fake_bundle};
    use super::*;
    use std::path::{Path, PathBuf};

    const OLD: &str = "0.15.4";
    const NEW: &str = "0.16.0";

    /// `<tmp>/Applications/Terminal Studio.app` at `version`.
    fn installed(root: &Path, version: &str) -> PathBuf {
        let apps = root.join("Applications");
        std::fs::create_dir_all(&apps).unwrap();
        fake_bundle(&apps, version)
    }

    /// The fake executable's content is its version (see `fake_bundle`).
    fn exe_version(bundle: &Path) -> String {
        std::fs::read_to_string(bundle.join("Contents/MacOS/terminal-studio")).unwrap()
    }

    /// A real UDZO disk image containing `Terminal Studio.app` at `version`.
    fn build_dmg(root: &Path, version: &str) -> Vec<u8> {
        let src = root.join(format!("dmg-src-{version}"));
        std::fs::create_dir_all(&src).unwrap();
        fake_bundle(&src, version);
        let dmg = root.join(format!("{version}.dmg"));
        let status = std::process::Command::new("/usr/bin/hdiutil")
            .args(["create", "-quiet", "-format", "UDZO", "-srcfolder"])
            .arg(&src)
            .arg(&dmg)
            .status()
            .unwrap();
        assert!(status.success(), "hdiutil create failed");
        std::fs::read(&dmg).unwrap()
    }

    /// Installs a real release DMG over a real (older) bundle:
    /// `TS_E2E_BUNDLE=<dir>/Terminal\ Studio.app TS_E2E_DMG=<release.dmg>
    ///  TS_E2E_TARGET=0.16.0 cargo test e2e_app_bundle_install -- --ignored`
    #[test]
    #[ignore = "acts on a real bundle and DMG; run manually"]
    fn e2e_app_bundle_install() {
        use super::super::e2e;
        let bundle = PathBuf::from(e2e::var(e2e::BUNDLE_VAR));
        let dmg = std::fs::read(e2e::var(e2e::DMG_VAR)).unwrap();
        let target = e2e::var(e2e::TARGET_VAR);

        install_from_dmg(&dmg, &bundle, &target).unwrap();
        assert_eq!(bundle_version::read(&bundle), Ok(target));
        assert!(backup_path(&bundle).exists());

        cleanup_leftovers(&bundle);
        assert!(!backup_path(&bundle).exists());
        assert!(!staging_path(&bundle).exists());
    }

    #[test]
    fn swap_replaces_bundle_and_keeps_backup() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = installed(dir.path(), OLD);
        let staged = staging_path(&bundle);
        std::fs::create_dir_all(staged.parent().unwrap()).unwrap();
        let built = fake_bundle(&dir.path().join("new"), NEW);
        std::fs::rename(built, &staged).unwrap();

        swap(&bundle, &staged).unwrap();

        assert_eq!(exe_version(&bundle), NEW);
        assert_eq!(exe_version(&backup_path(&bundle)), OLD);
        assert!(!staged.exists());
    }

    #[test]
    fn swap_rolls_back_when_staged_missing() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = installed(dir.path(), OLD);

        assert!(swap(&bundle, &staging_path(&bundle)).is_err());

        assert_eq!(exe_version(&bundle), OLD);
        assert!(!backup_path(&bundle).exists());
    }

    #[test]
    fn swap_succeeds_with_stale_backup_present() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = installed(dir.path(), OLD);
        let stale = backup_path(&bundle);
        std::fs::create_dir_all(stale.join("Contents")).unwrap();
        let staged = staging_path(&bundle);
        std::fs::rename(fake_bundle(&dir.path().join("new"), NEW), &staged).unwrap();

        swap(&bundle, &staged).unwrap();

        assert_eq!(exe_version(&bundle), NEW);
        assert_eq!(exe_version(&backup_path(&bundle)), OLD);
    }

    #[test]
    fn cleanup_leftovers_removes_staging_and_backup() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = installed(dir.path(), NEW);
        std::fs::create_dir_all(staging_path(&bundle).join("Contents")).unwrap();
        std::fs::create_dir_all(backup_path(&bundle).join("Contents")).unwrap();

        cleanup_leftovers(&bundle);

        assert!(!staging_path(&bundle).exists());
        assert!(!backup_path(&bundle).exists());
        assert_eq!(exe_version(&bundle), NEW);
    }

    #[test]
    fn staging_and_backup_are_hidden_siblings() {
        let bundle = Path::new("/Applications/Terminal Studio.app");
        assert_eq!(
            staging_path(bundle),
            Path::new("/Applications/.Terminal Studio.app.update")
        );
        assert_eq!(
            backup_path(bundle),
            Path::new("/Applications/.Terminal Studio.app.old")
        );
    }

    #[test]
    fn install_from_dmg_installs_bundle_from_image() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = installed(dir.path(), OLD);
        let dmg = build_dmg(dir.path(), NEW);

        install_from_dmg(&dmg, &bundle, NEW).unwrap();

        assert_eq!(bundle_version::read(&bundle), Ok(NEW.to_string()));
        assert_eq!(exe_version(&bundle), NEW);
        assert!(!staging_path(&bundle).exists());
    }

    #[test]
    fn install_from_dmg_rejects_wrong_version() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = installed(dir.path(), OLD);
        let dmg = build_dmg(dir.path(), "0.15.9");

        let err = install_from_dmg(&dmg, &bundle, NEW).unwrap_err();

        assert!(err.contains(NEW), "{err}");
        assert_eq!(exe_version(&bundle), OLD);
        assert!(!staging_path(&bundle).exists());
    }

    #[test]
    fn install_from_dmg_accepts_a_newer_image() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = installed(dir.path(), OLD);
        let dmg = build_dmg(dir.path(), "0.16.1");

        install_from_dmg(&dmg, &bundle, NEW).unwrap();

        assert_eq!(bundle_version::read(&bundle), Ok("0.16.1".to_string()));
    }

    #[test]
    fn install_from_dmg_rejects_garbage_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = installed(dir.path(), OLD);

        assert!(install_from_dmg(b"not a disk image", &bundle, NEW).is_err());

        assert_eq!(exe_version(&bundle), OLD);
        assert!(!staging_path(&bundle).exists());
    }

    #[test]
    fn install_from_dmg_rejects_unwritable_parent() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let bundle = installed(dir.path(), OLD);
        let parent = bundle.parent().unwrap().to_path_buf();
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = install_from_dmg(b"unused", &bundle, NEW);

        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o755)).unwrap();
        let err = result.unwrap_err();
        assert!(err.contains("No write access"), "{err}");
        assert_eq!(exe_version(&bundle), OLD);
    }
}
