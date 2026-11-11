//! macOS install detection and the install-specific update paths.

pub(super) mod app_bundle;
mod bundle_version;
pub(super) mod homebrew;
mod process;

use std::path::{Path, PathBuf};

use super::install::InstallKind;

/// Homebrew cask token, as in `Casks/terminal-studio.rb`.
pub(super) const CASK_TOKEN: &str = "terminal-studio";
/// Release asset a DMG install updates from (release.yml "Package macOS").
pub(super) const DMG_ASSET: &str = "terminal-studio-macos-arm.dmg";

/// `<Name>.app/Contents/MacOS/<executable>` layout of a macOS bundle.
const BUNDLE_EXTENSION: &str = "app";
const BUNDLE_CONTENTS_DIR: &str = "Contents";
const BUNDLE_EXECUTABLE_DIR: &str = "MacOS";

/// Where the cask installs apps (Homebrew's default `appdir`, or the
/// per-user variant).
const SYSTEM_APPLICATIONS_DIR: &str = "/Applications";
const USER_APPLICATIONS_DIR: &str = "Applications";

/// `brew` executables to try, in order: Apple Silicon prefix, then Intel.
/// GUI apps do not inherit the shell `PATH`, so these are absolute.
const BREW_CANDIDATES: [&str; 2] = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"];
const CASKROOM_DIR: &str = "Caskroom";

/// Detect how the running app was installed.
pub(super) fn detect() -> InstallKind {
    let exe = match std::env::current_exe() {
        Ok(exe) => std::fs::canonicalize(&exe).unwrap_or(exe),
        Err(e) => {
            log::warn!("updater: cannot locate executable: {e}");
            return InstallKind::Standalone;
        }
    };
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let kind = classify(&exe, &home, Path::exists);
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

/// Pure classification over an injectable `exists` so it is unit-testable.
/// A bundle counts as Homebrew-managed only when it sits where the cask puts
/// it *and* a brew prefix has the cask installed; any other copy (e.g. one
/// dragged to `~/Downloads`) is updated as a plain bundle.
pub(super) fn classify(exe: &Path, home: &Path, exists: impl Fn(&Path) -> bool) -> InstallKind {
    let Some(bundle) = bundle_root(exe) else {
        return InstallKind::Standalone;
    };
    let in_cask_appdir = bundle.parent().is_some_and(|parent| {
        parent == Path::new(SYSTEM_APPLICATIONS_DIR) || parent == home.join(USER_APPLICATIONS_DIR)
    });
    match in_cask_appdir.then(|| brew_owning_cask(&exists)).flatten() {
        Some(brew) => InstallKind::Homebrew { brew, bundle },
        None => InstallKind::AppBundle { bundle },
    }
}

/// The first existing `brew` whose prefix has this cask in its Caskroom.
fn brew_owning_cask(exists: &impl Fn(&Path) -> bool) -> Option<PathBuf> {
    BREW_CANDIDATES.iter().map(PathBuf::from).find(|brew| {
        let caskroom_entry = brew
            .parent()
            .and_then(Path::parent)
            .map(|prefix| prefix.join(CASKROOM_DIR).join(CASK_TOKEN));
        exists(brew) && caskroom_entry.is_some_and(|entry| exists(&entry))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const HOME: &str = "/Users/tester";
    const SYSTEM_EXE: &str = "/Applications/Terminal Studio.app/Contents/MacOS/terminal-studio";
    const SYSTEM_BUNDLE: &str = "/Applications/Terminal Studio.app";
    const USER_EXE: &str =
        "/Users/tester/Applications/Terminal Studio.app/Contents/MacOS/terminal-studio";
    const USER_BUNDLE: &str = "/Users/tester/Applications/Terminal Studio.app";
    const DOWNLOADS_EXE: &str =
        "/Users/tester/Downloads/Terminal Studio.app/Contents/MacOS/terminal-studio";
    const DOWNLOADS_BUNDLE: &str = "/Users/tester/Downloads/Terminal Studio.app";
    const ARM_BREW: &str = "/opt/homebrew/bin/brew";
    const ARM_CASKROOM: &str = "/opt/homebrew/Caskroom/terminal-studio";
    const INTEL_BREW: &str = "/usr/local/bin/brew";
    const INTEL_CASKROOM: &str = "/usr/local/Caskroom/terminal-studio";

    fn fs(paths: &[&str]) -> impl Fn(&Path) -> bool {
        let set: HashSet<PathBuf> = paths.iter().map(PathBuf::from).collect();
        move |p: &Path| set.contains(p)
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
        let kind = classify(
            Path::new(SYSTEM_EXE),
            Path::new(HOME),
            fs(&[ARM_BREW, ARM_CASKROOM]),
        );
        assert_eq!(
            kind,
            InstallKind::Homebrew {
                brew: PathBuf::from(ARM_BREW),
                bundle: PathBuf::from(SYSTEM_BUNDLE),
            }
        );
    }

    #[test]
    fn classify_homebrew_in_user_applications() {
        let kind = classify(
            Path::new(USER_EXE),
            Path::new(HOME),
            fs(&[ARM_BREW, ARM_CASKROOM]),
        );
        assert_eq!(
            kind,
            InstallKind::Homebrew {
                brew: PathBuf::from(ARM_BREW),
                bundle: PathBuf::from(USER_BUNDLE),
            }
        );
    }

    #[test]
    fn classify_app_bundle_when_no_caskroom() {
        let kind = classify(Path::new(SYSTEM_EXE), Path::new(HOME), fs(&[ARM_BREW]));
        assert_eq!(
            kind,
            InstallKind::AppBundle {
                bundle: PathBuf::from(SYSTEM_BUNDLE),
            }
        );
    }

    #[test]
    fn classify_app_bundle_when_running_outside_applications_even_if_caskroom_exists() {
        let kind = classify(
            Path::new(DOWNLOADS_EXE),
            Path::new(HOME),
            fs(&[ARM_BREW, ARM_CASKROOM]),
        );
        assert_eq!(
            kind,
            InstallKind::AppBundle {
                bundle: PathBuf::from(DOWNLOADS_BUNDLE),
            }
        );
    }

    #[test]
    fn classify_standalone_for_local_bin() {
        let kind = classify(
            Path::new("/Users/tester/.local/bin/terminal-studio"),
            Path::new(HOME),
            fs(&[ARM_BREW, ARM_CASKROOM]),
        );
        assert_eq!(kind, InstallKind::Standalone);
    }

    #[test]
    fn classify_prefers_opt_homebrew() {
        let kind = classify(
            Path::new(SYSTEM_EXE),
            Path::new(HOME),
            fs(&[ARM_BREW, ARM_CASKROOM, INTEL_BREW, INTEL_CASKROOM]),
        );
        assert_eq!(
            kind,
            InstallKind::Homebrew {
                brew: PathBuf::from(ARM_BREW),
                bundle: PathBuf::from(SYSTEM_BUNDLE),
            }
        );
    }

    #[test]
    fn classify_uses_the_brew_that_owns_the_cask() {
        // Both brews exist, but only the Intel prefix installed the cask.
        let kind = classify(
            Path::new(SYSTEM_EXE),
            Path::new(HOME),
            fs(&[ARM_BREW, INTEL_BREW, INTEL_CASKROOM]),
        );
        assert_eq!(
            kind,
            InstallKind::Homebrew {
                brew: PathBuf::from(INTEL_BREW),
                bundle: PathBuf::from(SYSTEM_BUNDLE),
            }
        );
    }
}
