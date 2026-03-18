use std::env;
use std::path::Path;

use super::types::{UpdateAction, UpdateExecutionStrategy};

const CURL_INSTALL_COMMAND: &str =
    "curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash";
const WINDOWS_INSTALL_COMMAND: &str =
    "irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex";
const HOMEBREW_UPDATE_COMMAND: &str = "brew upgrade vtcode";
const CARGO_UPDATE_COMMAND: &str = "cargo install vtcode --force";
const NPM_UPDATE_COMMAND: &str =
    "npm install -g @vinhnx/vtcode@latest --registry=https://npm.pkg.github.com";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallSource {
    Standalone,
    Homebrew,
    Cargo,
    Npm,
}

impl InstallSource {
    pub(crate) fn is_managed(self) -> bool {
        !matches!(self, Self::Standalone)
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Homebrew => "homebrew",
            Self::Cargo => "cargo",
            Self::Npm => "npm",
        }
    }

    pub(crate) fn update_action(self) -> UpdateAction {
        self.update_action_for_os(env::consts::OS)
    }

    pub(crate) fn update_action_for_os(self, os: &str) -> UpdateAction {
        match self {
            Self::Standalone if os == "windows" => UpdateAction {
                source_label: self.label(),
                display_command: WINDOWS_INSTALL_COMMAND,
                execution: UpdateExecutionStrategy::PowerShell,
                prefer_path_relaunch: false,
            },
            Self::Standalone => UpdateAction {
                source_label: self.label(),
                display_command: CURL_INSTALL_COMMAND,
                execution: UpdateExecutionStrategy::Shell,
                prefer_path_relaunch: false,
            },
            Self::Homebrew => UpdateAction {
                source_label: self.label(),
                display_command: HOMEBREW_UPDATE_COMMAND,
                execution: UpdateExecutionStrategy::Shell,
                prefer_path_relaunch: true,
            },
            Self::Cargo => UpdateAction {
                source_label: self.label(),
                display_command: CARGO_UPDATE_COMMAND,
                execution: UpdateExecutionStrategy::Shell,
                prefer_path_relaunch: true,
            },
            Self::Npm => UpdateAction {
                source_label: self.label(),
                display_command: NPM_UPDATE_COMMAND,
                execution: UpdateExecutionStrategy::Shell,
                prefer_path_relaunch: true,
            },
        }
    }
}

pub(super) fn detect_install_source() -> InstallSource {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => return InstallSource::Standalone,
    };

    let canonical = std::fs::canonicalize(&exe).unwrap_or(exe);
    detect_install_source_from_path(&canonical)
}

pub(super) fn detect_install_source_from_path(path: &Path) -> InstallSource {
    let path_text = path.to_string_lossy().to_ascii_lowercase();

    if path_text.contains("/cellar/")
        || path_text.contains("/homebrew/")
        || path_text.contains("/linuxbrew/")
        || path_text.contains("/opt/homebrew/")
    {
        return InstallSource::Homebrew;
    }

    if path_text.contains("/.cargo/bin/") {
        return InstallSource::Cargo;
    }

    if path_text.contains("/node_modules/") || path_text.contains("/npm/") {
        return InstallSource::Npm;
    }

    InstallSource::Standalone
}

pub(super) fn get_target_triple() -> Option<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standalone_unix_uses_shell_installer() {
        let action = InstallSource::Standalone.update_action_for_os("linux");
        assert_eq!(action.display_command, CURL_INSTALL_COMMAND);
        assert_eq!(action.execution, UpdateExecutionStrategy::Shell);
    }

    #[test]
    fn standalone_windows_uses_powershell_installer() {
        let action = InstallSource::Standalone.update_action_for_os("windows");
        assert_eq!(action.display_command, WINDOWS_INSTALL_COMMAND);
        assert_eq!(action.execution, UpdateExecutionStrategy::PowerShell);
    }

    #[test]
    fn homebrew_uses_core_upgrade_command() {
        let action = InstallSource::Homebrew.update_action_for_os("linux");
        assert_eq!(action.display_command, "brew upgrade vtcode");
        assert_eq!(action.execution, UpdateExecutionStrategy::Shell);
        assert!(action.prefer_path_relaunch);
    }

    #[test]
    fn npm_uses_scoped_registry_command() {
        let action = InstallSource::Npm.update_action_for_os("linux");
        assert_eq!(
            action.display_command,
            "npm install -g @vinhnx/vtcode@latest --registry=https://npm.pkg.github.com"
        );
        assert!(action.prefer_path_relaunch);
    }
}
