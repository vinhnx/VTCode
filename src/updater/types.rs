use semver::Version;

use super::InstallSource;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpdateExecutionStrategy {
    Shell,
    PowerShell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UpdateAction {
    pub(crate) source_label: &'static str,
    pub(crate) display_command: &'static str,
    pub(crate) execution: UpdateExecutionStrategy,
    pub(crate) prefer_path_relaunch: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdateGuidance {
    pub(crate) source: InstallSource,
    pub(crate) action: UpdateAction,
}

impl UpdateGuidance {
    pub(crate) fn command(&self) -> &'static str {
        self.action.display_command
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StartupUpdateNotice {
    pub(crate) current_version: Version,
    pub(crate) latest_version: Version,
    pub(crate) guidance: UpdateGuidance,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct StartupUpdateCheck {
    pub(crate) cached_notice: Option<StartupUpdateNotice>,
    pub(crate) should_refresh: bool,
}

pub(crate) enum InstallOutcome {
    Updated(String),
    UpToDate(String),
}

pub(crate) struct UpdateInfo {
    pub(crate) version: Version,
    pub(crate) release_notes: String,
}

pub(crate) struct VersionInfo {
    pub(crate) version: Version,
    pub(crate) tag: String,
    pub(crate) is_prerelease: bool,
    pub(crate) published_at: Option<String>,
}
