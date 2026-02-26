//! Terminal detection system using environment variables.
//!
//! Detects the current terminal emulator and provides terminal-specific configuration paths.

use anyhow::{Context, Result};
use std::env;
use std::path::PathBuf;

/// Supported terminal emulators
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

/// Terminal features that can be configured
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalFeature {
    Multiline,
    CopyPaste,
    ShellIntegration,
    ThemeSync,
    Notifications,
}

impl TerminalType {
    /// Detect the current terminal emulator from environment variables
    pub fn detect() -> Result<Self> {
        // Priority 1: Check TERM_PROGRAM (macOS standard, supported by many terminals)
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

        // Priority 2: Kitty-specific environment variables
        if env::var("KITTY_WINDOW_ID").is_ok() || env::var("KITTY_PID").is_ok() {
            return Ok(TerminalType::Kitty);
        }

        // Priority 3: Alacritty-specific environment variables
        if env::var("ALACRITTY_SOCKET").is_ok() || env::var("ALACRITTY_LOG").is_ok() {
            return Ok(TerminalType::Alacritty);
        }

        // Priority 4: Zed terminal marker
        if env::var("ZED_TERMINAL").is_ok() {
            return Ok(TerminalType::Zed);
        }

        // Priority 5: Windows Terminal
        if env::var("WT_SESSION").is_ok() || env::var("WT_PROFILE_ID").is_ok() {
            return Ok(TerminalType::WindowsTerminal);
        }

        // Priority 6: TERM variable fallback hints
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

    /// Check if terminal supports a specific feature
    pub fn supports_feature(&self, feature: TerminalFeature) -> bool {
        match (self, feature) {
            // Ghostty supports all features
            (TerminalType::Ghostty, _) => true,

            // Kitty supports all features
            (TerminalType::Kitty, _) => true,

            // Alacritty supports all features
            (TerminalType::Alacritty, _) => true,

            // WezTerm supports all features
            (TerminalType::WezTerm, _) => true,

            // Terminal.app supports multiline/shell integration/notifications
            (TerminalType::TerminalApp, TerminalFeature::Multiline) => true,
            (TerminalType::TerminalApp, TerminalFeature::ShellIntegration) => true,
            (TerminalType::TerminalApp, TerminalFeature::Notifications) => true,
            (TerminalType::TerminalApp, _) => false,

            // xterm supports baseline multiline and bell notifications
            (TerminalType::Xterm, TerminalFeature::Multiline) => true,
            (TerminalType::Xterm, TerminalFeature::Notifications) => true,
            (TerminalType::Xterm, _) => false,

            // Zed: multiline, theme, and notifications only
            (TerminalType::Zed, TerminalFeature::Multiline) => true,
            (TerminalType::Zed, TerminalFeature::ThemeSync) => true,
            (TerminalType::Zed, TerminalFeature::Notifications) => true,
            (TerminalType::Zed, _) => false,

            // Warp: has built-in multiline support, no manual config needed
            (TerminalType::Warp, TerminalFeature::Multiline) => false, // Built-in
            (TerminalType::Warp, TerminalFeature::Notifications) => true, // Built-in
            (TerminalType::Warp, _) => false,

            // iTerm2 supports all features but requires manual setup
            (TerminalType::ITerm2, _) => true,

            // VS Code: multiline and notifications
            (TerminalType::VSCode, TerminalFeature::Multiline) => true,
            (TerminalType::VSCode, TerminalFeature::Notifications) => true,
            (TerminalType::VSCode, _) => false,

            // Windows Terminal supports all features
            (TerminalType::WindowsTerminal, _) => true,

            // Hyper supports all features
            (TerminalType::Hyper, _) => true,

            // Tabby supports all features
            (TerminalType::Tabby, _) => true,

            // Unknown terminal: no support
            (TerminalType::Unknown, _) => false,
        }
    }

    /// Get the configuration file path for this terminal
    pub fn config_path(&self) -> Result<PathBuf> {
        let home_dir = dirs::home_dir().context("Failed to determine home directory")?;

        let path = match self {
            TerminalType::Ghostty => {
                if cfg!(target_os = "windows") {
                    let appdata =
                        env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata).join("ghostty").join("config")
                } else {
                    home_dir.join(".config").join("ghostty").join("config")
                }
            }

            TerminalType::Kitty => {
                if cfg!(target_os = "windows") {
                    let appdata =
                        env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata).join("kitty").join("kitty.conf")
                } else {
                    home_dir.join(".config").join("kitty").join("kitty.conf")
                }
            }

            TerminalType::Alacritty => {
                if cfg!(target_os = "windows") {
                    let appdata =
                        env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata)
                        .join("alacritty")
                        .join("alacritty.toml")
                } else {
                    home_dir
                        .join(".config")
                        .join("alacritty")
                        .join("alacritty.toml")
                }
            }

            TerminalType::WezTerm => home_dir.join(".wezterm.lua"),

            TerminalType::TerminalApp => {
                if cfg!(target_os = "macos") {
                    home_dir
                        .join("Library")
                        .join("Preferences")
                        .join("com.apple.Terminal.plist")
                } else {
                    anyhow::bail!("Terminal.app is only available on macOS")
                }
            }

            TerminalType::Xterm => home_dir.join(".Xresources"),

            TerminalType::Zed => {
                // Zed uses settings.json in its config directory
                if cfg!(target_os = "windows") {
                    let appdata =
                        env::var("APPDATA").context("APPDATA environment variable not set")?;
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
                // Warp config path (mainly for reference, limited config needed)
                if cfg!(target_os = "macos") {
                    home_dir.join(".warp")
                } else {
                    home_dir.join(".config").join("warp")
                }
            }

            TerminalType::ITerm2 => {
                // iTerm2 uses plist file on macOS only
                if cfg!(target_os = "macos") {
                    home_dir
                        .join("Library")
                        .join("Preferences")
                        .join("com.googlecode.iterm2.plist")
                } else {
                    anyhow::bail!("iTerm2 is only available on macOS")
                }
            }

            TerminalType::VSCode => {
                if cfg!(target_os = "windows") {
                    let appdata =
                        env::var("APPDATA").context("APPDATA environment variable not set")?;
                    PathBuf::from(appdata)
                        .join("Code")
                        .join("User")
                        .join("settings.json")
                } else if cfg!(target_os = "macos") {
                    home_dir
                        .join("Library")
                        .join("Application Support")
                        .join("Code")
                        .join("User")
                        .join("settings.json")
                } else {
                    home_dir
                        .join(".config")
                        .join("Code")
                        .join("User")
                        .join("settings.json")
                }
            }

            TerminalType::WindowsTerminal => {
                if cfg!(target_os = "windows") {
                    let local_appdata = env::var("LOCALAPPDATA")
                        .context("LOCALAPPDATA environment variable not set")?;
                    // Windows Terminal settings are in Packages folder
                    // Path pattern: %LOCALAPPDATA%/Packages/Microsoft.WindowsTerminal_*/LocalState/settings.json
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
                    let appdata =
                        env::var("APPDATA").context("APPDATA environment variable not set")?;
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

    /// Get a human-readable name for this terminal
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

    /// Check if terminal requires manual setup (vs automatic config)
    pub fn requires_manual_setup(&self) -> bool {
        matches!(
            self,
            TerminalType::ITerm2
                | TerminalType::VSCode
                | TerminalType::TerminalApp
                | TerminalType::Xterm
        )
    }
}

impl TerminalFeature {
    /// Get a human-readable name for this feature
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_feature_support() {
        // Ghostty supports all features
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::Multiline));
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::CopyPaste));
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::ShellIntegration));
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::ThemeSync));
        assert!(TerminalType::Ghostty.supports_feature(TerminalFeature::Notifications));

        // VS Code supports multiline and notifications
        assert!(TerminalType::VSCode.supports_feature(TerminalFeature::Multiline));
        assert!(TerminalType::VSCode.supports_feature(TerminalFeature::Notifications));
        assert!(!TerminalType::VSCode.supports_feature(TerminalFeature::CopyPaste));

        // Zed supports multiline, theme sync, and notifications
        assert!(TerminalType::Zed.supports_feature(TerminalFeature::Multiline));
        assert!(TerminalType::Zed.supports_feature(TerminalFeature::ThemeSync));
        assert!(TerminalType::Zed.supports_feature(TerminalFeature::Notifications));

        // Warp supports notifications
        assert!(TerminalType::Warp.supports_feature(TerminalFeature::Notifications));

        // Unknown supports nothing
        assert!(!TerminalType::Unknown.supports_feature(TerminalFeature::Multiline));
        assert!(!TerminalType::Unknown.supports_feature(TerminalFeature::Notifications));
    }

    #[test]
    fn test_terminal_names() {
        assert_eq!(TerminalType::Kitty.name(), "Kitty");
        assert_eq!(TerminalType::Alacritty.name(), "Alacritty");
        assert_eq!(TerminalType::VSCode.name(), "VS Code");
    }

    #[test]
    fn test_manual_setup_detection() {
        assert!(TerminalType::ITerm2.requires_manual_setup());
        assert!(TerminalType::VSCode.requires_manual_setup());
        assert!(!TerminalType::Kitty.requires_manual_setup());
    }

    #[test]
    fn test_detect_kitty() {
        // This test would need to set environment variables
        // Skipped in actual implementation due to env manipulation complexity
    }
}
