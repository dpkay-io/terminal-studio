//! How this copy of the app was installed, which decides how it updates.

#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InstallKind {
    /// A bare executable (Windows, Linux, or `install.sh` on macOS):
    /// updated by swapping the binary in place.
    Standalone,
    /// A `.app` bundle installed by the Homebrew cask: updated by running
    /// `brew` so Homebrew's record of the installed version stays correct.
    #[cfg(target_os = "macos")]
    Homebrew { brew: PathBuf, bundle: PathBuf },
    /// A `.app` bundle installed from the DMG: updated by replacing the
    /// whole bundle so `Info.plist` and the code signature stay consistent.
    #[cfg(target_os = "macos")]
    AppBundle { bundle: PathBuf },
}

impl InstallKind {
    /// Inspect the running executable. Touches the file system, so call it
    /// from the update worker thread, never the UI thread.
    pub(super) fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            super::macos::detect()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::Standalone
        }
    }

    /// The release asset this install needs, or `None` when it updates
    /// without downloading one itself (Homebrew fetches its own DMG).
    pub(super) fn asset_name(&self) -> Option<&'static str> {
        match self {
            Self::Standalone => Some(super::standalone::platform_asset_name()),
            #[cfg(target_os = "macos")]
            Self::Homebrew { .. } => None,
            #[cfg(target_os = "macos")]
            Self::AppBundle { .. } => Some(super::macos::DMG_ASSET),
        }
    }

    /// Remove what an earlier (or interrupted) bundle update left behind.
    /// File-system work: call from the update worker thread.
    pub(super) fn cleanup_previous_update(&self) {
        match self {
            // Standalone leftovers are handled by `cleanup_old_binary` at startup.
            Self::Standalone => {}
            #[cfg(target_os = "macos")]
            Self::Homebrew { bundle, .. } | Self::AppBundle { bundle } => {
                super::macos::app_bundle::cleanup_leftovers(bundle);
                super::macos::app_bundle::sweep_stale_scratch();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_needs_the_platform_binary() {
        assert_eq!(
            InstallKind::Standalone.asset_name(),
            Some(super::super::standalone::platform_asset_name())
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn homebrew_needs_no_asset() {
        let kind = InstallKind::Homebrew {
            brew: PathBuf::from("/opt/homebrew/bin/brew"),
            bundle: PathBuf::from("/Applications/Terminal Studio.app"),
        };
        assert_eq!(kind.asset_name(), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn app_bundle_needs_the_release_dmg() {
        let kind = InstallKind::AppBundle {
            bundle: PathBuf::from("/Applications/Terminal Studio.app"),
        };
        assert_eq!(kind.asset_name(), Some("terminal-studio-macos-arm.dmg"));
    }
}
