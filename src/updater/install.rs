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
}
