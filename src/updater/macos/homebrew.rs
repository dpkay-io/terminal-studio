//! Update a Homebrew-cask install by running `brew`, so Homebrew's own
//! record of the installed version moves with the app.

use std::path::Path;
use std::time::Duration;

use super::process::{self, CommandSpec};
use super::{bundle_version, CASK_TOKEN};

/// Shown while brew runs (no measurable progress).
pub(in crate::updater) const STATUS_MESSAGE: &str = "Updating via Homebrew\u{2026}";

/// `brew update` refreshes every tap; allow for slow networks.
const BREW_UPDATE_TIMEOUT: Duration = Duration::from_secs(300);
/// `brew upgrade` downloads the DMG and swaps the bundle.
const BREW_UPGRADE_TIMEOUT: Duration = Duration::from_secs(600);
/// `brew update` already ran, hints are noise, and cleanup is unrelated work.
const BREW_ENV: [(&str, &str); 3] = [
    ("HOMEBREW_NO_AUTO_UPDATE", "1"),
    ("HOMEBREW_NO_ENV_HINTS", "1"),
    ("HOMEBREW_NO_INSTALL_CLEANUP", "1"),
];

/// `brew update`, then `brew upgrade --cask --no-quit terminal-studio`.
/// `--no-quit`: brew must never quit the app that is running it.
fn upgrade_steps(brew: &Path) -> [CommandSpec; 2] {
    let step = |timeout| {
        BREW_ENV
            .iter()
            .fold(CommandSpec::new(brew, timeout), |spec, (k, v)| {
                spec.env(k, v)
            })
    };
    [
        step(BREW_UPDATE_TIMEOUT).arg("update"),
        step(BREW_UPGRADE_TIMEOUT)
            .arg("upgrade")
            .arg("--cask")
            .arg("--no-quit")
            .arg(CASK_TOKEN),
    ]
}

/// Upgrade the cask, then confirm `bundle` really is at `target`.
pub(in crate::updater) fn upgrade(brew: &Path, bundle: &Path, target: &str) -> Result<(), String> {
    for step in upgrade_steps(brew) {
        process::run(&step).map_err(|e| format!("Homebrew: {e}"))?;
    }
    verify_upgraded(bundle, target)
}

/// brew exits 0 when the tap's cask is not newer yet, so success alone does
/// not mean the bundle changed.
fn verify_upgraded(bundle: &Path, target: &str) -> Result<(), String> {
    let found = bundle_version::read(bundle)?;
    if found == target {
        Ok(())
    } else {
        Err(format!(
            "Homebrew has no v{target} yet (installed: v{found}) \u{2014} try again later"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::bundle_version::test_support::fake_bundle;
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;

    const BREW: &str = "/opt/homebrew/bin/brew";

    fn args(spec: &CommandSpec) -> Vec<OsString> {
        spec.args.clone()
    }

    #[test]
    fn upgrade_steps_update_then_upgrade_cask() {
        let [update, upgrade] = upgrade_steps(Path::new(BREW));
        assert_eq!(update.program, Path::new(BREW));
        assert_eq!(args(&update), vec![OsString::from("update")]);
        assert_eq!(upgrade.program, Path::new(BREW));
        assert_eq!(
            args(&upgrade),
            ["upgrade", "--cask", "--no-quit", "terminal-studio"]
                .map(OsString::from)
                .to_vec()
        );
        for step in [&update, &upgrade] {
            for env in [
                "HOMEBREW_NO_AUTO_UPDATE",
                "HOMEBREW_NO_ENV_HINTS",
                "HOMEBREW_NO_INSTALL_CLEANUP",
            ] {
                assert!(
                    step.envs.contains(&(env, "1")),
                    "{env} missing from {step:?}"
                );
            }
        }
    }

    #[test]
    fn verify_upgraded_ok_when_versions_match() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = fake_bundle(dir.path(), "0.16.0");
        assert_eq!(verify_upgraded(&bundle, "0.16.0"), Ok(()));
    }

    #[test]
    fn verify_upgraded_err_when_bundle_still_old() {
        // brew exits 0 with "already installed" when the cask in the tap has
        // not been bumped yet; that must not look like a finished update.
        let dir = tempfile::tempdir().unwrap();
        let bundle = fake_bundle(dir.path(), "0.15.4");
        let err = verify_upgraded(&bundle, "0.16.0").unwrap_err();
        assert!(err.contains("0.16.0"), "{err}");
    }
}
