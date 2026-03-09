use semver::Version;

use super::InstallSource;

pub struct UpdateGuidance {
    pub source: InstallSource,
    pub command: String,
}

pub enum InstallOutcome {
    Updated(String),
    UpToDate(String),
}

pub struct UpdateInfo {
    pub version: Version,
    pub tag: String,
    pub download_url: String,
    pub release_notes: String,
}

pub struct VersionInfo {
    pub version: Version,
    pub tag: String,
    pub is_prerelease: bool,
    pub published_at: Option<String>,
}
