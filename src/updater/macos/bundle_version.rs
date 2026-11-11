//! Read the version an installed `.app` bundle reports (`Info.plist`).

use std::path::Path;
use std::time::Duration;

use super::process::{self, CommandSpec};

const PLUTIL: &str = "/usr/bin/plutil";
const SHORT_VERSION_KEY: &str = "CFBundleShortVersionString";
/// `plutil -extract <key> raw -o - <plist>`: print one value, unquoted.
const PLUTIL_EXTRACT_ARGS: [&str; 5] = ["-extract", SHORT_VERSION_KEY, "raw", "-o", "-"];
const PLUTIL_TIMEOUT: Duration = Duration::from_secs(10);
const CONTENTS_DIR: &str = "Contents";
const INFO_PLIST: &str = "Info.plist";

/// `CFBundleShortVersionString` of `bundle`, e.g. `0.16.0`.
pub(super) fn read(bundle: &Path) -> Result<String, String> {
    let plist = bundle.join(CONTENTS_DIR).join(INFO_PLIST);
    let spec = CommandSpec::new(PLUTIL, PLUTIL_TIMEOUT)
        .args(PLUTIL_EXTRACT_ARGS)
        .arg(plist);
    process::run(&spec).map(|out| out.trim().to_string())
}

/// Whether version `found` is `target` or newer (semver, so `0.10.0` is
/// newer than `0.9.0` and a pre-release is older than its release).
pub(super) fn is_at_least(found: &str, target: &str) -> Result<bool, String> {
    let parse = |v: &str| {
        semver::Version::parse(v).map_err(|e| format!("Invalid app version \"{v}\": {e}"))
    };
    Ok(parse(found)? >= parse(target)?)
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
    fn is_at_least_compares_as_semver() {
        assert_eq!(is_at_least("0.16.0", "0.16.0"), Ok(true));
        assert_eq!(is_at_least("0.16.1", "0.16.0"), Ok(true));
        // A string compare would call 0.10.0 older than 0.9.0.
        assert_eq!(is_at_least("0.10.0", "0.9.0"), Ok(true));
        assert_eq!(is_at_least("0.15.4", "0.16.0"), Ok(false));
        assert_eq!(is_at_least("0.16.0-rc.1", "0.16.0"), Ok(false));
    }

    #[test]
    fn is_at_least_rejects_unparseable_versions() {
        let err = is_at_least("garbage", "0.16.0").unwrap_err();
        assert!(err.contains("garbage"), "{err}");
        assert!(is_at_least("0.16.0", "").is_err());
    }

    #[test]
    fn read_fails_without_info_plist() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read(&dir.path().join("Missing.app")).is_err());
    }
}
