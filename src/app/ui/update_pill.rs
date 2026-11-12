//! The titlebar's update pill: what it says and what clicking it does for
//! each update status. Painting lives in `titlebar.rs`.

use crate::updater::UpdateStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PillAction {
    /// Show Settings, where the update is started and its progress shown.
    OpenSettings,
    /// Relaunch into the installed update.
    Restart,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct UpdatePill {
    pub label: String,
    pub action: PillAction,
}

const UPDATE_ARROW: &str = "\u{2B06}";
const RESTART_LABEL: &str = "Restart to update";

/// The pill for `status`, or `None` when there is nothing to show. While an
/// update downloads or installs the pill stays, so the titlebar never goes
/// blank mid-update; clicking it opens Settings to the progress.
pub(super) fn pill_for(status: &UpdateStatus) -> Option<UpdatePill> {
    let (label, action) = match status {
        UpdateStatus::UpdateAvailable { version, .. } => (
            format!("{UPDATE_ARROW} Update v{version}"),
            PillAction::OpenSettings,
        ),
        UpdateStatus::Downloading { progress_pct } => (
            format!("Downloading {progress_pct:.0}%"),
            PillAction::OpenSettings,
        ),
        #[cfg(target_os = "macos")]
        UpdateStatus::Installing { message } => ((*message).to_string(), PillAction::OpenSettings),
        UpdateStatus::RestartRequired => (RESTART_LABEL.to_string(), PillAction::Restart),
        UpdateStatus::Idle
        | UpdateStatus::Checking
        | UpdateStatus::UpToDate
        | UpdateStatus::Error(_) => return None,
    };
    Some(UpdatePill { label, action })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pill(label: &str, action: PillAction) -> Option<UpdatePill> {
        Some(UpdatePill {
            label: label.to_string(),
            action,
        })
    }

    #[test]
    fn offers_an_available_update() {
        let status = UpdateStatus::UpdateAvailable {
            version: "0.16.0".into(),
            download_url: None,
        };
        assert_eq!(
            pill_for(&status),
            pill("\u{2B06} Update v0.16.0", PillAction::OpenSettings)
        );
    }

    #[test]
    fn shows_download_progress() {
        let status = UpdateStatus::Downloading { progress_pct: 42.4 };
        assert_eq!(
            pill_for(&status),
            pill("Downloading 42%", PillAction::OpenSettings)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn shows_the_install_step() {
        let status = UpdateStatus::Installing {
            message: "Updating via Homebrew\u{2026}",
        };
        assert_eq!(
            pill_for(&status),
            pill("Updating via Homebrew\u{2026}", PillAction::OpenSettings)
        );
    }

    #[test]
    fn offers_a_restart_once_installed() {
        assert_eq!(
            pill_for(&UpdateStatus::RestartRequired),
            pill("Restart to update", PillAction::Restart)
        );
    }

    #[test]
    fn hidden_when_there_is_nothing_to_act_on() {
        for status in [
            UpdateStatus::Idle,
            UpdateStatus::Checking,
            UpdateStatus::UpToDate,
            UpdateStatus::Error("boom".into()),
        ] {
            assert_eq!(pill_for(&status), None, "{status:?}");
        }
    }
}
