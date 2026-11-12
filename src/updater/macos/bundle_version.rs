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

    static HDIUTIL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Serializes tests that use hdiutil: in parallel, `hdiutil create` can
    /// exit leaving its image attached, and attaches fail as busy. Take it
    /// first in every such test (never inside `fake_dmg`, which would
    /// deadlock).
    pub fn hdiutil_lock() -> std::sync::MutexGuard<'static, ()> {
        HDIUTIL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

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

    /// `hdiutil create` takes ~4s, so each image is built once and reused.
    const DMG_CACHE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/test-dmg-cache");

    /// A real UDZO disk image at `<dir>/<version>.dmg` containing
    /// `fake_bundle(version)`. Hold `hdiutil_lock` while calling it.
    pub fn fake_dmg(dir: &Path, version: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let dmg = dir.join(format!("{version}.dmg"));
        std::fs::copy(cached_dmg(version), &dmg).unwrap();
        dmg
    }

    /// The cached image for `version`, keyed on the plist template too so a
    /// template change rebuilds it.
    fn cached_dmg(version: &str) -> PathBuf {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (INFO_PLIST_TEMPLATE, version).hash(&mut hasher);
        let cache = Path::new(DMG_CACHE_DIR);
        let cached = cache.join(format!("{version}-{:016x}.dmg", hasher.finish()));
        if cached.exists() {
            return cached;
        }
        let build = tempfile::tempdir().unwrap();
        let src = build.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        fake_bundle(&src, version);
        let built = build.path().join("image.dmg");
        let status = std::process::Command::new("/usr/bin/hdiutil")
            .args(["create", "-quiet", "-format", "UDZO", "-srcfolder"])
            .arg(&src)
            .arg(&built)
            .status()
            .unwrap();
        assert!(status.success(), "hdiutil create failed");
        std::fs::create_dir_all(cache).unwrap();
        // Same volume as `cached`? Not guaranteed: copy, then rename atomically.
        let partial = cached.with_extension("partial");
        std::fs::copy(&built, &partial).unwrap();
        std::fs::rename(&partial, &cached).unwrap();
        cached
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
