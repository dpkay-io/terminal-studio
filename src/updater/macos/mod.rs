//! macOS install detection and the install-specific update paths.

pub(super) mod app_bundle;
mod bundle_version;
mod disk_image;
pub(super) mod homebrew;
mod process;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::install::InstallKind;

/// Homebrew cask token, as in `Casks/terminal-studio.rb`.
pub(super) const CASK_TOKEN: &str = "terminal-studio";
/// Release asset a DMG install updates from (release.yml "Package macOS").
pub(super) const DMG_ASSET: &str = "terminal-studio-macos-arm.dmg";

/// `<Name>.app/Contents/MacOS/<executable>` layout of a macOS bundle.
const BUNDLE_EXTENSION: &str = "app";
const BUNDLE_CONTENTS_DIR: &str = "Contents";
const BUNDLE_EXECUTABLE_DIR: &str = "MacOS";

/// `brew` executables to try, in order: Apple Silicon prefix, then Intel.
/// GUI apps do not inherit the shell `PATH`, so these are absolute.
const BREW_CANDIDATES: [&str; 2] = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"];
const CASKROOM_DIR: &str = "Caskroom";
const HIDDEN_NAME_PREFIX: char = '.';

/// Distinguishes temp paths created concurrently within one process.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Separates the pid from the sequence number in temp names.
const TEMP_NAME_SEPARATOR: char = '-';

/// A temp path no other call in any process will produce:
/// `<tmp>/<prefix><pid>-<n>`.
pub(super) fn unique_temp_path(prefix: &str) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "{prefix}{}{TEMP_NAME_SEPARATOR}{sequence}",
        std::process::id()
    ))
}

/// The pid of the process that created `name` with `unique_temp_path(prefix)`.
pub(super) fn temp_path_owner(name: &str, prefix: &str) -> Option<u32> {
    let (pid, sequence) = name.strip_prefix(prefix)?.split_once(TEMP_NAME_SEPARATOR)?;
    sequence.parse::<u64>().ok()?;
    pid.parse().ok()
}

/// Settings for the `#[ignore]`d end-to-end tests, which act on a real
/// install and real release artifacts. See each test for how to run it.
#[cfg(test)]
pub(super) mod e2e {
    /// Path of the installed `Terminal Studio.app` to update.
    pub const BUNDLE_VAR: &str = "TS_E2E_BUNDLE";
    /// Version the update must leave the bundle at, e.g. `0.16.0`.
    pub const TARGET_VAR: &str = "TS_E2E_TARGET";
    /// Path of a release DMG (bundle path only).
    pub const DMG_VAR: &str = "TS_E2E_DMG";

    pub fn var(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| panic!("set {name} to run this test"))
    }
}

/// Detect how the running app was installed.
pub(super) fn detect() -> InstallKind {
    let exe = match std::env::current_exe() {
        Ok(exe) => std::fs::canonicalize(&exe).unwrap_or(exe),
        Err(e) => {
            log::warn!("updater: cannot locate executable: {e}");
            return InstallKind::Standalone;
        }
    };
    let kind = classify(&exe, &RealFs);
    log::info!("updater: install kind {kind:?}");
    kind
}

/// The `.app` directory containing `exe`, if `exe` is a bundle executable.
pub(super) fn bundle_root(exe: &Path) -> Option<PathBuf> {
    let executable_dir = exe.parent()?;
    let contents = executable_dir.parent()?;
    let bundle = contents.parent()?;
    let is_bundle = executable_dir.file_name()? == BUNDLE_EXECUTABLE_DIR
        && contents.file_name()? == BUNDLE_CONTENTS_DIR
        && bundle.extension()? == BUNDLE_EXTENSION;
    is_bundle.then(|| bundle.to_path_buf())
}

/// The file-system queries `classify` makes; faked in tests.
pub(super) trait FsProbe {
    fn exists(&self, path: &Path) -> bool;
    /// The entries of `dir` (none if it cannot be read).
    fn entries(&self, dir: &Path) -> Vec<PathBuf>;
    /// Where `path` resolves to, if it is a symlink.
    fn link_target(&self, path: &Path) -> Option<PathBuf>;
}

/// The real file system.
pub(super) struct RealFs;

impl FsProbe for RealFs {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn entries(&self, dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| entries.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default()
    }

    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        let is_link = std::fs::symlink_metadata(path)
            .ok()?
            .file_type()
            .is_symlink();
        is_link.then(|| std::fs::canonicalize(path).ok()).flatten()
    }
}

/// Classify the install `exe` belongs to (pure over `fs`, so unit-testable).
/// A bundle is Homebrew-managed only when a brew's Caskroom records this
/// very bundle as the cask's app, wherever its `appdir` is; any other copy
/// (e.g. one dragged to `~/Downloads`) is updated as a plain bundle.
pub(super) fn classify(exe: &Path, fs: &impl FsProbe) -> InstallKind {
    let Some(bundle) = bundle_root(exe) else {
        return InstallKind::Standalone;
    };
    let brew = BREW_CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|brew| fs.exists(brew) && cask_installed(brew, &bundle, fs));
    match brew {
        Some(brew) => InstallKind::Homebrew { brew, bundle },
        None => InstallKind::AppBundle { bundle },
    }
}

/// Whether `brew`'s prefix installed the cask's app at `bundle`. brew moves
/// the app to its appdir and leaves a symlink to it at
/// `Caskroom/<token>/<version>/<app>` (`moved.rb`, `post_move`).
fn cask_installed(brew: &Path, bundle: &Path, fs: &impl FsProbe) -> bool {
    let Some(prefix) = brew.parent().and_then(Path::parent) else {
        return false;
    };
    let cask_dir = prefix.join(CASKROOM_DIR).join(CASK_TOKEN);
    fs.entries(&cask_dir)
        .iter()
        .filter(|version_dir| !is_hidden(version_dir))
        .flat_map(|version_dir| fs.entries(version_dir))
        .any(|entry| fs.link_target(&entry).as_deref() == Some(bundle))
}

/// `.metadata` and other dot-entries of the Caskroom hold no app links.
fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(HIDDEN_NAME_PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    const SYSTEM_EXE: &str = "/Applications/Terminal Studio.app/Contents/MacOS/terminal-studio";
    const SYSTEM_BUNDLE: &str = "/Applications/Terminal Studio.app";
    const USER_EXE: &str =
        "/Users/tester/Applications/Terminal Studio.app/Contents/MacOS/terminal-studio";
    const USER_BUNDLE: &str = "/Users/tester/Applications/Terminal Studio.app";
    const CUSTOM_EXE: &str = "/Volumes/Apps/Terminal Studio.app/Contents/MacOS/terminal-studio";
    const CUSTOM_BUNDLE: &str = "/Volumes/Apps/Terminal Studio.app";
    const DOWNLOADS_EXE: &str =
        "/Users/tester/Downloads/Terminal Studio.app/Contents/MacOS/terminal-studio";
    const DOWNLOADS_BUNDLE: &str = "/Users/tester/Downloads/Terminal Studio.app";
    const ARM_BREW: &str = "/opt/homebrew/bin/brew";
    const ARM_CASKROOM: &str = "/opt/homebrew/Caskroom/terminal-studio";
    const INTEL_BREW: &str = "/usr/local/bin/brew";
    const INTEL_CASKROOM: &str = "/usr/local/Caskroom/terminal-studio";
    const VERSION: &str = "0.16.0";
    const APP_NAME: &str = "Terminal Studio.app";

    /// An in-memory file system: existing paths, directory listings, symlinks.
    #[derive(Default)]
    struct FakeFs {
        dirs: HashMap<PathBuf, Vec<PathBuf>>,
        links: HashMap<PathBuf, PathBuf>,
        files: Vec<PathBuf>,
    }

    impl FakeFs {
        fn brew(mut self, brew: &str) -> Self {
            self.files.push(PathBuf::from(brew));
            self
        }

        /// What `brew install --cask` leaves: `<caskroom>/<version>/<app>`
        /// symlinked to where the app was moved (plus `.metadata`).
        fn cask(mut self, caskroom: &str, app: &str) -> Self {
            let caskroom = PathBuf::from(caskroom);
            let version_dir = caskroom.join(VERSION);
            let link = version_dir.join(APP_NAME);
            self.dirs.insert(
                caskroom.clone(),
                vec![caskroom.join(".metadata"), version_dir.clone()],
            );
            self.dirs.insert(version_dir, vec![link.clone()]);
            self.links.insert(link, PathBuf::from(app));
            self
        }
    }

    impl FsProbe for FakeFs {
        fn exists(&self, path: &Path) -> bool {
            self.files.iter().any(|f| f == path) || self.dirs.contains_key(path)
        }

        fn entries(&self, dir: &Path) -> Vec<PathBuf> {
            self.dirs.get(dir).cloned().unwrap_or_default()
        }

        fn link_target(&self, path: &Path) -> Option<PathBuf> {
            self.links.get(path).cloned()
        }
    }

    fn homebrew(brew: &str, bundle: &str) -> InstallKind {
        InstallKind::Homebrew {
            brew: PathBuf::from(brew),
            bundle: PathBuf::from(bundle),
        }
    }

    fn app_bundle(bundle: &str) -> InstallKind {
        InstallKind::AppBundle {
            bundle: PathBuf::from(bundle),
        }
    }

    #[test]
    fn temp_path_owner_reads_back_the_creating_pid() {
        const PREFIX: &str = "ts-owner-test-";
        let path = unique_temp_path(PREFIX);
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(temp_path_owner(name, PREFIX), Some(std::process::id()));
        assert_eq!(temp_path_owner("ts-owner-test-abc-1", PREFIX), None);
        assert_eq!(temp_path_owner("ts-owner-test-12", PREFIX), None);
        assert_eq!(temp_path_owner("ts-owner-test-12-x", PREFIX), None);
        assert_eq!(temp_path_owner("other-12-1", PREFIX), None);
    }

    #[test]
    fn bundle_root_from_app_exe() {
        assert_eq!(
            bundle_root(Path::new(SYSTEM_EXE)),
            Some(PathBuf::from(SYSTEM_BUNDLE))
        );
    }

    #[test]
    fn bundle_root_none_for_plain_binary() {
        assert_eq!(
            bundle_root(Path::new("/Users/tester/.local/bin/terminal-studio")),
            None
        );
        // Right shape, but the bundle directory is not a `.app`.
        assert_eq!(
            bundle_root(Path::new("/tmp/Foo/Contents/MacOS/terminal-studio")),
            None
        );
    }

    #[test]
    fn classify_homebrew_in_applications() {
        let fs = FakeFs::default()
            .brew(ARM_BREW)
            .cask(ARM_CASKROOM, SYSTEM_BUNDLE);
        assert_eq!(
            classify(Path::new(SYSTEM_EXE), &fs),
            homebrew(ARM_BREW, SYSTEM_BUNDLE)
        );
    }

    #[test]
    fn classify_homebrew_in_user_applications() {
        // `--appdir=~/Applications`: brew's symlink records where it went.
        let fs = FakeFs::default()
            .brew(ARM_BREW)
            .cask(ARM_CASKROOM, USER_BUNDLE);
        assert_eq!(
            classify(Path::new(USER_EXE), &fs),
            homebrew(ARM_BREW, USER_BUNDLE)
        );
    }

    #[test]
    fn classify_homebrew_with_a_custom_appdir() {
        let fs = FakeFs::default()
            .brew(ARM_BREW)
            .cask(ARM_CASKROOM, CUSTOM_BUNDLE);
        assert_eq!(
            classify(Path::new(CUSTOM_EXE), &fs),
            homebrew(ARM_BREW, CUSTOM_BUNDLE)
        );
    }

    #[test]
    fn classify_app_bundle_for_a_copy_the_cask_did_not_install() {
        // Cask in /Applications; this copy was dragged to ~/Applications.
        let fs = FakeFs::default()
            .brew(ARM_BREW)
            .cask(ARM_CASKROOM, SYSTEM_BUNDLE);
        assert_eq!(classify(Path::new(USER_EXE), &fs), app_bundle(USER_BUNDLE));
        assert_eq!(
            classify(Path::new(DOWNLOADS_EXE), &fs),
            app_bundle(DOWNLOADS_BUNDLE)
        );
    }

    #[test]
    fn classify_app_bundle_when_no_caskroom() {
        let fs = FakeFs::default().brew(ARM_BREW);
        assert_eq!(
            classify(Path::new(SYSTEM_EXE), &fs),
            app_bundle(SYSTEM_BUNDLE)
        );
    }

    #[test]
    fn classify_standalone_for_local_bin() {
        let fs = FakeFs::default()
            .brew(ARM_BREW)
            .cask(ARM_CASKROOM, SYSTEM_BUNDLE);
        assert_eq!(
            classify(Path::new("/Users/tester/.local/bin/terminal-studio"), &fs),
            InstallKind::Standalone
        );
    }

    #[test]
    fn classify_prefers_opt_homebrew() {
        let fs = FakeFs::default()
            .brew(ARM_BREW)
            .cask(ARM_CASKROOM, SYSTEM_BUNDLE)
            .brew(INTEL_BREW)
            .cask(INTEL_CASKROOM, SYSTEM_BUNDLE);
        assert_eq!(
            classify(Path::new(SYSTEM_EXE), &fs),
            homebrew(ARM_BREW, SYSTEM_BUNDLE)
        );
    }

    #[test]
    fn classify_uses_the_brew_that_installed_this_copy() {
        // Both brews exist, but only the Intel prefix installed the cask.
        let fs = FakeFs::default()
            .brew(ARM_BREW)
            .brew(INTEL_BREW)
            .cask(INTEL_CASKROOM, SYSTEM_BUNDLE);
        assert_eq!(
            classify(Path::new(SYSTEM_EXE), &fs),
            homebrew(INTEL_BREW, SYSTEM_BUNDLE)
        );
    }

    #[test]
    fn real_fs_resolves_cask_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join(APP_NAME);
        std::fs::create_dir(&app).unwrap();
        let link = dir.path().join("link.app");
        std::os::unix::fs::symlink(&app, &link).unwrap();

        let canonical = std::fs::canonicalize(&app).unwrap();
        assert_eq!(RealFs.link_target(&link), Some(canonical));
        assert_eq!(RealFs.link_target(&app), None);
        assert!(RealFs.entries(dir.path()).contains(&link));
    }
}
