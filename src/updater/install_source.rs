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
const SCOOP_UPDATE_COMMAND: &str = "scoop update vtcode";
const SNAP_UPDATE_COMMAND: &str = "sudo snap refresh vtcode";
const FLATPAK_UPDATE_COMMAND: &str = "flatpak update com.vinhnx.Vtcode";
const NIX_UPDATE_COMMAND: &str = "nix profile upgrade vtcode";
const WINGET_UPDATE_COMMAND: &str = "winget upgrade vtcode";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallSource {
    Standalone,
    Homebrew,
    Cargo,
    Npm,
    Scoop,
    Snap,
    Flatpak,
    Nix,
    Winget,
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
            Self::Scoop => "scoop",
            Self::Snap => "snap",
            Self::Flatpak => "flatpak",
            Self::Nix => "nix",
            Self::Winget => "winget",
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
            Self::Scoop => UpdateAction {
                source_label: self.label(),
                display_command: SCOOP_UPDATE_COMMAND,
                execution: UpdateExecutionStrategy::PowerShell,
                prefer_path_relaunch: true,
            },
            Self::Snap => UpdateAction {
                source_label: self.label(),
                display_command: SNAP_UPDATE_COMMAND,
                execution: UpdateExecutionStrategy::Shell,
                prefer_path_relaunch: true,
            },
            Self::Flatpak => UpdateAction {
                source_label: self.label(),
                display_command: FLATPAK_UPDATE_COMMAND,
                execution: UpdateExecutionStrategy::Shell,
                prefer_path_relaunch: true,
            },
            Self::Nix => UpdateAction {
                source_label: self.label(),
                display_command: NIX_UPDATE_COMMAND,
                execution: UpdateExecutionStrategy::Shell,
                prefer_path_relaunch: true,
            },
            Self::Winget => UpdateAction {
                source_label: self.label(),
                display_command: WINGET_UPDATE_COMMAND,
                execution: UpdateExecutionStrategy::PowerShell,
                prefer_path_relaunch: true,
            },
        }
    }
}

pub(super) fn detect_install_source() -> InstallSource {
    let exe = match env::current_exe() {
        Ok(path) => path,
        Err(_) => return InstallSource::Standalone,
    };

    let canonical = std::fs::canonicalize(&exe).unwrap_or(exe);
    detect_install_source_from_path(&canonical)
}

pub(super) fn detect_install_source_from_path(path: &Path) -> InstallSource {
    let path_text = path.to_string_lossy().to_ascii_lowercase();

    // --- Unix package managers (path uses /) ---

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

    // Nix: after canonicalize(), ~/.nix-profile/bin/vtcode resolves to /nix/store/...
    // Non-canonicalized paths use /.nix-profile/ (with dot).
    if path_text.contains("/nix/store/") || path_text.contains("/.nix-profile/") {
        return InstallSource::Nix;
    }

    // Snap: executables exposed at /snap/bin/<name>, data under /snap/<name>/
    if path_text.contains("/snap/") {
        return InstallSource::Snap;
    }

    // Flatpak: system at /var/lib/flatpak/app/, user at ~/.local/share/flatpak/app/
    if path_text.contains("/flatpak/app/") || path_text.contains("/flatpak/exports/") {
        return InstallSource::Flatpak;
    }

    // --- Windows package managers (path uses \) ---

    // Scoop: per-user at ~\scoop\apps\, global at %ProgramData%\scoop\apps\,
    // shims at ~\scoop\shims\
    if path_text.contains("\\scoop\\apps\\") || path_text.contains("\\scoop\\shims\\") {
        return InstallSource::Scoop;
    }

    // Winget: portable installs at %LOCALAPPDATA%\Microsoft\WinGet\Packages\
    if path_text.contains("\\winget\\packages\\") {
        return InstallSource::Winget;
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
        ("windows", "aarch64") => Some("aarch64-pc-windows-msvc"),
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

    #[test]
    fn scoop_detected_on_windows_path() {
        assert_eq!(
            detect_install_source_from_path(Path::new(
                "C:\\Users\\dev\\scoop\\apps\\vtcode\\0.1.0\\vtcode.exe"
            )),
            InstallSource::Scoop
        );
    }

    #[test]
    fn scoop_shim_detected() {
        assert_eq!(
            detect_install_source_from_path(Path::new("C:\\Users\\dev\\scoop\\shims\\vtcode.exe")),
            InstallSource::Scoop
        );
    }

    #[test]
    fn snap_detected() {
        assert_eq!(
            detect_install_source_from_path(Path::new("/snap/vtcode/123/bin/vtcode")),
            InstallSource::Snap
        );
    }

    #[test]
    fn nix_store_detected() {
        assert_eq!(
            detect_install_source_from_path(Path::new("/nix/store/abc123-vtcode-0.1.0/bin/vtcode")),
            InstallSource::Nix
        );
    }

    #[test]
    fn nix_profile_detected() {
        assert_eq!(
            detect_install_source_from_path(Path::new("/home/dev/.nix-profile/bin/vtcode")),
            InstallSource::Nix
        );
    }

    #[test]
    fn winget_portable_detected() {
        assert_eq!(
            detect_install_source_from_path(Path::new(
                "C:\\Users\\dev\\AppData\\Local\\Microsoft\\WinGet\\Packages\\vtcode\\vtcode.exe"
            )),
            InstallSource::Winget
        );
    }

    #[test]
    fn flatpak_detected() {
        assert_eq!(
            detect_install_source_from_path(Path::new(
                "/var/lib/flatpak/app/com.vinhnx.Vtcode/current/active/bin/vtcode"
            )),
            InstallSource::Flatpak
        );
    }

    #[test]
    fn all_new_sources_are_managed() {
        for source in [
            InstallSource::Scoop,
            InstallSource::Snap,
            InstallSource::Flatpak,
            InstallSource::Nix,
            InstallSource::Winget,
        ] {
            assert!(source.is_managed(), "{} should be managed", source.label());
        }
    }

    #[test]
    fn scoop_uses_powershell() {
        let action = InstallSource::Scoop.update_action_for_os("windows");
        assert_eq!(action.display_command, SCOOP_UPDATE_COMMAND);
        assert_eq!(action.execution, UpdateExecutionStrategy::PowerShell);
    }

    #[test]
    fn nix_uses_shell() {
        let action = InstallSource::Nix.update_action_for_os("linux");
        assert_eq!(action.display_command, NIX_UPDATE_COMMAND);
        assert_eq!(action.execution, UpdateExecutionStrategy::Shell);
    }

    #[test]
    fn snap_uses_shell() {
        let action = InstallSource::Snap.update_action_for_os("linux");
        assert_eq!(action.display_command, SNAP_UPDATE_COMMAND);
        assert_eq!(action.execution, UpdateExecutionStrategy::Shell);
    }

    #[test]
    fn winget_uses_powershell() {
        let action = InstallSource::Winget.update_action_for_os("windows");
        assert_eq!(action.display_command, WINGET_UPDATE_COMMAND);
        assert_eq!(action.execution, UpdateExecutionStrategy::PowerShell);
    }
}
