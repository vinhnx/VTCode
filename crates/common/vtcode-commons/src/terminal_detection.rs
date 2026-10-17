//! Terminal detection primitives shared across VT Code crates.

use anyhow::{Context, Result};
use std::env;
use std::path::{Path, PathBuf};

/// Name of the VT Code iTerm2 profile shipped for tab-icon support.
/// Distinctive so the one-shot profile switch never hijacks an unrelated
/// user profile with a generic name.
pub const ITERM2_PROFILE_NAME: &str = "VT Code";

/// Name of iTerm2's built-in default profile, used as the restore fallback
/// when the session's original profile cannot be determined.
pub const ITERM2_DEFAULT_PROFILE_NAME: &str = "Default";

/// Filename of the shipped iTerm2 dynamic profile.
pub const ITERM2_DYNAMIC_PROFILE_FILENAME: &str = "vtcode.json";

/// Profile `Icon` mode selecting a custom image.
///
/// Best-effort mapping from observed behavior: a default profile stores
/// `Icon = 1` and renders the built-in foreground-app glyph, while the
/// preferences offer custom / built-in / none. If a future iTerm2 maps the
/// custom mode elsewhere, only this constant changes.
pub const ITERM2_ICON_MODE_CUSTOM: u8 = 2;

/// iTerm2 DynamicProfiles directory under `home`.
///
/// iTerm2 loads JSON profiles from this directory live with no restart.
/// The directory may not exist yet; callers create it on install.
pub fn iterm2_dynamic_profiles_dir(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("iTerm2")
        .join("DynamicProfiles")
}

/// Installed dynamic-profile path under `home`.
pub fn installed_iterm2_profile_path(home: &Path) -> PathBuf {
    iterm2_dynamic_profiles_dir(home).join(ITERM2_DYNAMIC_PROFILE_FILENAME)
}

/// Named gate conditions for the one-shot iTerm2 profile switch.
///
/// Stable-Rust emulation of named arguments: call sites construct this
/// struct with named fields instead of passing three positional `bool`s,
/// so `iterm_session` cannot be confused with `tmux_session` at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ITerm2ProfileConditions {
    /// `ITERM_SESSION_ID` is present (real iTerm2 session).
    pub iterm_session: bool,
    /// `TMUX` is present (proprietary sequences do not pass through multiplexers).
    pub tmux_session: bool,
    /// Shipped dynamic profile is installed.
    pub profile_installed: bool,
}

impl ITerm2ProfileConditions {
    /// Pure gate evaluation.
    pub fn should_apply(&self) -> bool {
        self.iterm_session && !self.tmux_session && self.profile_installed
    }
}

/// Pure gate for the one-shot iTerm2 profile switch.
///
/// Switch only for a real iTerm2 session outside tmux (proprietary
/// sequences do not pass through multiplexers) with the shipped profile
/// installed, so removing the profile file disables the switch.
pub fn should_apply_iterm2_profile(iterm_session: bool, tmux_session: bool, profile_installed: bool) -> bool {
    ITerm2ProfileConditions { iterm_session, tmux_session, profile_installed }.should_apply()
}

/// Environment plus install gate evaluated against the live process.
///
/// Combines [`should_apply_iterm2_profile`] with `ITERM_SESSION_ID` /
/// `TMUX` detection and the installed-profile check, so all callers share
/// one gating decision.
pub fn should_apply_iterm2_profile_now() -> bool {
    let iterm_session = env::var("ITERM_SESSION_ID").is_ok();
    let tmux_session = env::var("TMUX").is_ok();
    let installed = dirs::home_dir()
        .map(|home| installed_iterm2_profile_path(&home))
        .map(|path| path.exists())
        .unwrap_or(false);
    ITerm2ProfileConditions {
        iterm_session,
        tmux_session,
        profile_installed: installed,
    }
    .should_apply()
}

/// The profile name to restore after VT Code's one-shot icon switch.
///
/// Reads iTerm2's `ITERM_PROFILE`, which is fixed at session creation and is
/// **not** updated by `OSC 1337;SetProfile=`, so it still names the session's
/// original profile after the switch. Returns `None` when the value is unset,
/// empty, or already [`ITERM2_PROFILE_NAME`]: in that case there is nothing to
/// restore (the session already uses the shipped profile, or we cannot tell
/// what to revert to without hijacking an unrelated profile).
pub fn original_iterm2_profile_name() -> Option<String> {
    let name = env::var("ITERM_PROFILE").ok()?;
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == ITERM2_PROFILE_NAME {
        return None;
    }
    Some(trimmed.to_string())
}

/// Supported terminal emulators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalType {
    Ghostty,
    Kitty,
    Alacritty,
    WezTerm,
    TerminalApp,
    Xterm,
    Zed,
    Warp,
    ITerm2,
    VSCode,
    WindowsTerminal,
    Hyper,
    Tabby,
    Unknown,
}

/// Terminal features that can be configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFeature {
    Multiline,
    CopyPaste,
    ShellIntegration,
    ThemeSync,
    Notifications,
}

/// How VT Code should present `/terminal-setup` for a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalSetupAvailability {
    NativeSupport,
    Offered,
    GuidanceOnly,
}

impl TerminalType {
    /// Detect the current terminal emulator from environment variables.
    pub fn detect() -> Result<Self> {
        if let Ok(term_program) = env::var("TERM_PROGRAM") {
            let term_lower = term_program.to_lowercase();

            if term_lower.contains("ghostty") {
                return Ok(TerminalType::Ghostty);
            } else if term_lower.contains("wezterm") {
                return Ok(TerminalType::WezTerm);
            } else if term_lower.contains("apple_terminal") {
                return Ok(TerminalType::TerminalApp);
            } else if term_lower.contains("iterm") {
                return Ok(TerminalType::ITerm2);
            } else if term_lower.contains("vscode") {
                return Ok(TerminalType::VSCode);
            } else if term_lower.contains("warp") {
                return Ok(TerminalType::Warp);
            } else if term_lower.contains("hyper") {
                return Ok(TerminalType::Hyper);
            } else if term_lower.contains("tabby") {
                return Ok(TerminalType::Tabby);
            }
        }

        if env::var("KITTY_WINDOW_ID").is_ok() || env::var("KITTY_PID").is_ok() {
            return Ok(TerminalType::Kitty);
        }

        if env::var("ALACRITTY_SOCKET").is_ok() || env::var("ALACRITTY_LOG").is_ok() {
            return Ok(TerminalType::Alacritty);
        }

        if env::var("ZED_TERMINAL").is_ok() {
            return Ok(TerminalType::Zed);
        }

        if env::var("WT_SESSION").is_ok() || env::var("WT_PROFILE_ID").is_ok() {
            return Ok(TerminalType::WindowsTerminal);
        }

        if let Ok(term) = env::var("TERM") {
            let term_lower = term.to_lowercase();

            if term_lower.contains("kitty") {
                return Ok(TerminalType::Kitty);
            } else if term_lower.contains("alacritty") {
                return Ok(TerminalType::Alacritty);
            } else if term_lower.contains("xterm") {
                return Ok(TerminalType::Xterm);
            }
        }

        Ok(TerminalType::Unknown)
    }

    /// Check if terminal supports a specific feature.
    pub fn supports_feature(&self, feature: TerminalFeature) -> bool {
        match (self, feature) {
            (TerminalType::Ghostty, _) => true,
            (TerminalType::Kitty, _) => true,
            (TerminalType::Alacritty, _) => true,
            (TerminalType::WezTerm, _) => true,
            (TerminalType::TerminalApp, TerminalFeature::Multiline) => true,
            (TerminalType::TerminalApp, TerminalFeature::ShellIntegration) => true,
            (TerminalType::TerminalApp, TerminalFeature::Notifications) => true,
            (TerminalType::TerminalApp, _) => false,
            (TerminalType::Xterm, TerminalFeature::Multiline) => true,
            (TerminalType::Xterm, TerminalFeature::Notifications) => true,
            (TerminalType::Xterm, _) => false,
            (TerminalType::Zed, TerminalFeature::Multiline) => true,
            (TerminalType::Zed, TerminalFeature::ThemeSync) => true,
            (TerminalType::Zed, TerminalFeature::Notifications) => true,
            (TerminalType::Zed, _) => false,
            (TerminalType::Warp, TerminalFeature::Multiline) => true,
            (TerminalType::Warp, TerminalFeature::Notifications) => true,
            (TerminalType::Warp, _) => false,
            (TerminalType::ITerm2, _) => true,
            (TerminalType::VSCode, TerminalFeature::Multiline) => true,
            (TerminalType::VSCode, TerminalFeature::Notifications) => true,
            (TerminalType::VSCode, _) => false,
            (TerminalType::WindowsTerminal, _) => true,
            (TerminalType::Hyper, _) => true,
            (TerminalType::Tabby, _) => true,
            (TerminalType::Unknown, _) => false,
        }
    }

    /// Whether multiline input works without VT Code modifying terminal config.
    pub fn has_native_multiline_support(&self) -> bool {
        matches!(
            self,
            TerminalType::Ghostty
                | TerminalType::Kitty
                | TerminalType::WezTerm
                | TerminalType::ITerm2
                | TerminalType::Warp
        )
    }

    /// How VT Code should present `/terminal-setup` for this terminal.
    pub fn terminal_setup_availability(&self) -> TerminalSetupAvailability {
        match self {
            TerminalType::Ghostty
            | TerminalType::Kitty
            | TerminalType::WezTerm
            | TerminalType::ITerm2
            | TerminalType::Warp => TerminalSetupAvailability::NativeSupport,
            TerminalType::Alacritty | TerminalType::Zed | TerminalType::VSCode => TerminalSetupAvailability::Offered,
            TerminalType::TerminalApp
            | TerminalType::Xterm
            | TerminalType::WindowsTerminal
            | TerminalType::Hyper
            | TerminalType::Tabby
            | TerminalType::Unknown => TerminalSetupAvailability::GuidanceOnly,
        }
    }

    /// Whether `/terminal-setup` should appear in slash discovery surfaces.
    pub fn should_offer_terminal_setup(&self) -> bool {
        matches!(self.terminal_setup_availability(), TerminalSetupAvailability::Offered)
    }

    /// Get the configuration file path for this terminal.
    pub fn config_path(&self) -> Result<PathBuf> {
        let home_dir = dirs::home_dir().context("Failed to determine home directory")?;

        let path = match self {
            TerminalType::Ghostty => {
                if cfg!(target_os = "windows") {
                    let appdata = env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata).join("ghostty").join("config")
                } else {
                    home_dir.join(".config").join("ghostty").join("config")
                }
            }
            TerminalType::Kitty => {
                if cfg!(target_os = "windows") {
                    let appdata = env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata).join("kitty").join("kitty.conf")
                } else {
                    home_dir.join(".config").join("kitty").join("kitty.conf")
                }
            }
            TerminalType::Alacritty => {
                if cfg!(target_os = "windows") {
                    let appdata = env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata).join("alacritty").join("alacritty.toml")
                } else {
                    home_dir.join(".config").join("alacritty").join("alacritty.toml")
                }
            }
            TerminalType::WezTerm => home_dir.join(".wezterm.lua"),
            TerminalType::TerminalApp => {
                if cfg!(target_os = "macos") {
                    home_dir.join("Library").join("Preferences").join("com.apple.Terminal.plist")
                } else {
                    anyhow::bail!("Terminal.app is only available on macOS")
                }
            }
            TerminalType::Xterm => home_dir.join(".Xresources"),
            TerminalType::Zed => {
                if cfg!(target_os = "windows") {
                    let appdata = env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata).join("Zed").join("settings.json")
                } else if cfg!(target_os = "macos") {
                    home_dir
                        .join("Library")
                        .join("Application Support")
                        .join("Zed")
                        .join("settings.json")
                } else {
                    home_dir.join(".config").join("zed").join("settings.json")
                }
            }
            TerminalType::Warp => {
                if cfg!(target_os = "macos") {
                    home_dir.join(".warp")
                } else {
                    home_dir.join(".config").join("warp")
                }
            }
            TerminalType::ITerm2 => {
                if cfg!(target_os = "macos") {
                    home_dir.join("Library").join("Preferences").join("com.googlecode.iterm2.plist")
                } else {
                    anyhow::bail!("iTerm2 is only available on macOS")
                }
            }
            TerminalType::VSCode => {
                if cfg!(target_os = "windows") {
                    let appdata = env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata).join("Code").join("User").join("settings.json")
                } else if cfg!(target_os = "macos") {
                    home_dir
                        .join("Library")
                        .join("Application Support")
                        .join("Code")
                        .join("User")
                        .join("settings.json")
                } else {
                    home_dir.join(".config").join("Code").join("User").join("settings.json")
                }
            }
            TerminalType::WindowsTerminal => {
                if cfg!(target_os = "windows") {
                    let local_appdata =
                        env::var("LOCALAPPDATA").context("LOCALAPPDATA environment variable not set")?;
                    PathBuf::from(local_appdata)
                        .join("Packages")
                        .join("Microsoft.WindowsTerminal_8wekyb3d8bbwe")
                        .join("LocalState")
                        .join("settings.json")
                } else {
                    anyhow::bail!("Windows Terminal is only available on Windows")
                }
            }
            TerminalType::Hyper => home_dir.join(".hyper.js"),
            TerminalType::Tabby => {
                if cfg!(target_os = "windows") {
                    let appdata = env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata).join("tabby").join("config.yaml")
                } else if cfg!(target_os = "macos") {
                    home_dir
                        .join("Library")
                        .join("Application Support")
                        .join("tabby")
                        .join("config.yaml")
                } else {
                    home_dir.join(".config").join("tabby").join("config.yaml")
                }
            }
            TerminalType::Unknown => {
                anyhow::bail!("Cannot determine config path for unknown terminal")
            }
        };

        Ok(path)
    }

    /// Get a human-readable name for this terminal.
    pub fn name(&self) -> &'static str {
        match self {
            TerminalType::Ghostty => "Ghostty",
            TerminalType::Kitty => "Kitty",
            TerminalType::Alacritty => "Alacritty",
            TerminalType::WezTerm => "WezTerm",
            TerminalType::TerminalApp => "Terminal.app",
            TerminalType::Xterm => "xterm",
            TerminalType::Zed => "Zed",
            TerminalType::Warp => "Warp",
            TerminalType::ITerm2 => "iTerm2",
            TerminalType::VSCode => "VS Code",
            TerminalType::WindowsTerminal => "Windows Terminal",
            TerminalType::Hyper => "Hyper",
            TerminalType::Tabby => "Tabby",
            TerminalType::Unknown => "Unknown",
        }
    }

    /// Check if terminal requires manual setup (vs automatic config).
    fn requires_manual_setup(&self) -> bool {
        self.should_offer_terminal_setup()
    }
}

impl TerminalFeature {
    /// Get a human-readable name for this feature.
    pub fn name(&self) -> &'static str {
        match self {
            TerminalFeature::Multiline => "Shift+Enter Multiline Input",
            TerminalFeature::CopyPaste => "Enhanced Copy/Paste",
            TerminalFeature::ShellIntegration => "Shell Integration",
            TerminalFeature::ThemeSync => "Theme Synchronization",
            TerminalFeature::Notifications => "System Notifications",
        }
    }
}

/// Returns whether the terminal identifiers point to Ghostty.
pub fn is_ghostty_terminal(term_program: Option<&str>, term: Option<&str>) -> bool {
    terminal_name_contains(term_program, "ghostty") || terminal_name_contains(term, "ghostty")
}

fn terminal_name_contains(value: Option<&str>, needle: &str) -> bool {
    value.is_some_and(|value| crate::formatting::contains_ignore_ascii_case(value, needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_feature_support_matches_expectations() {
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::Multiline));
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::CopyPaste));
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::ShellIntegration));
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::ThemeSync));
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::Notifications));

        assert!(TerminalType::VSCode.supports_feature(TerminalFeature::Multiline));
        assert!(TerminalType::VSCode.supports_feature(TerminalFeature::Notifications));
        assert!(!TerminalType::VSCode.supports_feature(TerminalFeature::CopyPaste));

        assert!(TerminalType::Zed.supports_feature(TerminalFeature::Multiline));
        assert!(TerminalType::Zed.supports_feature(TerminalFeature::ThemeSync));
        assert!(TerminalType::Zed.supports_feature(TerminalFeature::Notifications));

        assert!(TerminalType::Warp.supports_feature(TerminalFeature::Notifications));

        assert!(!TerminalType::Unknown.supports_feature(TerminalFeature::Multiline));
        assert!(!TerminalType::Unknown.supports_feature(TerminalFeature::Notifications));
    }

    #[test]
    fn terminal_names_match_current_labels() {
        assert_eq!(TerminalType::Kitty.name(), "Kitty");
        assert_eq!(TerminalType::Alacritty.name(), "Alacritty");
        assert_eq!(TerminalType::VSCode.name(), "VS Code");
    }

    #[test]
    fn manual_setup_detection_matches_offer_state() {
        assert!(TerminalType::VSCode.requires_manual_setup());
        assert!(!TerminalType::ITerm2.requires_manual_setup());
        assert!(!TerminalType::Kitty.requires_manual_setup());
    }

    #[test]
    fn native_multiline_terminals_are_not_offered_setup() {
        assert!(TerminalType::WezTerm.has_native_multiline_support());
        assert!(!TerminalType::WezTerm.should_offer_terminal_setup());
        assert!(TerminalType::ITerm2.has_native_multiline_support());
        assert!(!TerminalType::ITerm2.should_offer_terminal_setup());
        assert!(TerminalType::Warp.has_native_multiline_support());
        assert!(!TerminalType::Warp.should_offer_terminal_setup());
    }

    #[test]
    fn supported_setup_terminals_are_offered_setup() {
        assert!(TerminalType::VSCode.should_offer_terminal_setup());
        assert!(TerminalType::Alacritty.should_offer_terminal_setup());
        assert!(TerminalType::Zed.should_offer_terminal_setup());
        assert!(!TerminalType::WindowsTerminal.should_offer_terminal_setup());
        assert!(!TerminalType::Hyper.should_offer_terminal_setup());
        assert!(!TerminalType::Tabby.should_offer_terminal_setup());
    }

    #[test]
    fn ghostty_helper_matches_term_program_or_term() {
        assert!(is_ghostty_terminal(Some("Ghostty"), None));
        assert!(is_ghostty_terminal(None, Some("xterm-ghostty")));
        assert!(!is_ghostty_terminal(Some("WezTerm"), Some("xterm-256color")));
    }

    #[test]
    fn iterm2_profile_paths_stay_under_dynamic_profiles() {
        let first = installed_iterm2_profile_path(Path::new("/Users/demo"));
        let second = installed_iterm2_profile_path(Path::new("/home/other"));

        assert_eq!(first, PathBuf::from("/Users/demo/Library/Application Support/iTerm2/DynamicProfiles/vtcode.json"));
        assert_eq!(second, PathBuf::from("/home/other/Library/Application Support/iTerm2/DynamicProfiles/vtcode.json"));
        assert!(!ITERM2_PROFILE_NAME.is_empty());
        assert!(ITERM2_DYNAMIC_PROFILE_FILENAME.ends_with(".json"));
    }

    #[test]
    fn iterm2_profile_switch_requires_session_without_tmux_and_installed_profile() {
        assert!(should_apply_iterm2_profile(true, false, true));
        assert!(!should_apply_iterm2_profile(false, false, true));
        assert!(!should_apply_iterm2_profile(true, true, true));
        assert!(!should_apply_iterm2_profile(true, false, false));
        assert!(!should_apply_iterm2_profile(false, false, false));
    }

    #[test]
    fn iterm2_profile_conditions_match_legacy_function() {
        let named = ITerm2ProfileConditions {
            iterm_session: true,
            tmux_session: false,
            profile_installed: true,
        };
        assert!(named.should_apply());
        assert_eq!(named.should_apply(), should_apply_iterm2_profile(true, false, true));
        let defaulted = ITerm2ProfileConditions { iterm_session: true, ..Default::default() };
        assert!(!defaulted.should_apply());
    }

    #[test]
    fn original_iterm2_profile_name_returns_session_profile() {
        let env = crate::env_lock::lock();
        let previous = env::var_os("ITERM_PROFILE");
        env.set_var("ITERM_PROFILE", "  Solarized Dark  ");
        assert_eq!(original_iterm2_profile_name().as_deref(), Some("Solarized Dark"));
        env.restore_var("ITERM_PROFILE", previous);
    }

    #[test]
    fn original_iterm2_profile_name_skips_shipped_profile() {
        let env = crate::env_lock::lock();
        let previous = env::var_os("ITERM_PROFILE");
        env.set_var("ITERM_PROFILE", ITERM2_PROFILE_NAME);
        assert_eq!(original_iterm2_profile_name(), None);

        env.set_var("ITERM_PROFILE", "   ");
        assert_eq!(original_iterm2_profile_name(), None);

        env.remove_var("ITERM_PROFILE");
        assert_eq!(original_iterm2_profile_name(), None);
        env.restore_var("ITERM_PROFILE", previous);
    }
}
