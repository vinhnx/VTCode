use semver::Version;

use super::InstallSource;

pub(crate) struct UpdateGuidance {
    pub(crate) source: InstallSource,
    pub(crate) command: String,
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
