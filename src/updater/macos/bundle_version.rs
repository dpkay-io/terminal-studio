//! Read the version an installed `.app` bundle reports (`Info.plist`).

use std::path::Path;
use std::time::Duration;

use super::process::{self, CommandSpec};

const PLUTIL: &str = "/usr/bin/plutil";
const SHORT_VERSION_KEY: &str = "CFBundleShortVersionString";
const PLUTIL_TIMEOUT: Duration = Duration::from_secs(10);
const CONTENTS_DIR: &str = "Contents";
const INFO_PLIST: &str = "Info.plist";

/// `CFBundleShortVersionString` of `bundle`, e.g. `0.16.0`.
pub(super) fn read(bundle: &Path) -> Result<String, String> {
    let plist = bundle.join(CONTENTS_DIR).join(INFO_PLIST);
    let spec = CommandSpec::new(PLUTIL, PLUTIL_TIMEOUT)
        .arg("-extract")
        .arg(SHORT_VERSION_KEY)
        .arg("raw")
        .arg("-o")
        .arg("-")
        .arg(plist);
    process::run(&spec).map(|out| out.trim().to_string())
}

#[cfg(test)]
pub(super) mod test_support {
    use std::path::{Path, PathBuf};

    const INFO_PLIST_TEMPLATE: &str = include_str!("../../../packaging/macos/Info.plist");
    const VERSION_PLACEHOLDER: &str = "VERSION_PLACEHOLDER";

    /// A minimal `Terminal Studio.app` in `dir`, with the real release
    /// `Info.plist` template stamped with `version`.
    pub fn fake_bundle(dir: &Path, version: &str) -> PathBuf {
        let bundle = dir.join("Terminal Studio.app");
        let contents = bundle.join("Contents");
        std::fs::create_dir_all(contents.join("MacOS")).unwrap();
        std::fs::write(
            contents.join("Info.plist"),
            INFO_PLIST_TEMPLATE.replace(VERSION_PLACEHOLDER, version),
        )
        .unwrap();
        std::fs::write(contents.join("MacOS").join("terminal-studio"), version).unwrap();
        bundle
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::fake_bundle;
    use super::*;

    #[test]
    fn read_returns_short_version() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = fake_bundle(dir.path(), "0.15.4");
        assert_eq!(read(&bundle), Ok("0.15.4".to_string()));
    }

    #[test]
    fn read_fails_without_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&dir.path().join("Missing.app")).is_err());
    }
}
