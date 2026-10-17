//! iTerm2 configuration instruction generator.
//!
//! Most iTerm2 settings live in plist files, which are complex to modify
//! programmatically, so this module generates manual setup instructions for
//! them. The tab icon is the exception: iTerm2 loads Dynamic Profiles from
//! plain JSON files with no restart, so the icon profile below is fully
//! installable from code.

use std::path::{Path, PathBuf};

use crate::terminal_setup::detector::TerminalType;
use crate::terminal_setup::features::multiline;
use anyhow::{Context, Result};
use vtcode_commons::VtCodePaths;
use vtcode_commons::terminal_detection::{
    ITERM2_DYNAMIC_PROFILE_FILENAME, ITERM2_ICON_MODE_CUSTOM, ITERM2_PROFILE_NAME, installed_iterm2_profile_path,
    iterm2_dynamic_profiles_dir,
};

/// Generate iTerm2 setup instructions (manual configuration required)
pub fn generate_config(features: &[crate::terminal_setup::detector::TerminalFeature]) -> Result<String> {
    let mut instructions = vec![
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
        "  iTerm2 Manual Configuration Instructions".to_string(),
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
        String::new(),
        "iTerm2 requires manual configuration via the GUI.".to_string(),
        "Follow these steps to configure each feature:".to_string(),
        String::new(),
    ];

    for (i, feature) in features.iter().enumerate() {
        match feature {
            crate::terminal_setup::detector::TerminalFeature::Multiline => {
                instructions.push(format!("{}. MULTILINE INPUT (Shift+Enter)", i + 1));
                instructions.push(String::new());
                let multiline_instructions = multiline::generate_config(TerminalType::ITerm2)?;
                instructions.push(multiline_instructions);
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::CopyPaste => {
                instructions.push(format!("{}. COPY/PASTE INTEGRATION", i + 1));
                instructions.push(String::new());
                instructions.push("1. Open iTerm2 Preferences (Cmd+,)".to_string());
                instructions.push("2. Go to General → Selection".to_string());
                instructions.push("3. Enable 'Copy to pasteboard on selection'".to_string());
                instructions.push("4. Go to Pointer tab".to_string());
                instructions.push("5. Set middle-click action to 'Paste from Clipboard'".to_string());
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ShellIntegration => {
                instructions.push(format!("{}. SHELL INTEGRATION", i + 1));
                instructions.push(String::new());
                instructions.push("1. Open iTerm2 Preferences (Cmd+,)".to_string());
                instructions.push("2. Go to Profiles → General".to_string());
                instructions.push("3. Under 'Command', select your shell".to_string());
                instructions.push("4. iTerm2's shell integration will auto-install on first launch".to_string());
                instructions.push("5. Or manually install: curl -L https://iterm2.com/shell_integration/install_shell_integration.sh | bash".to_string());
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ThemeSync => {
                instructions.push(format!("{}. THEME SYNCHRONIZATION", i + 1));
                instructions.push(String::new());
                instructions.push("1. Open iTerm2 Preferences (Cmd+,)".to_string());
                instructions.push("2. Go to Profiles → Colors".to_string());
                instructions.push("3. Choose a color preset or customize manually".to_string());
                instructions.push("4. VT Code theme colors can be manually configured here".to_string());
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::Notifications => {
                instructions.push(format!("{}. SYSTEM NOTIFICATIONS", i + 1));
                instructions.push(String::new());
                instructions.push("1. Open iTerm2 Preferences (Cmd+,)".to_string());
                instructions.push("2. Navigate to Profiles → Terminal".to_string());
                instructions.push(
                    "3. Enable 'Silence bell' and Filter Alerts → 'Send escape sequence-generated alerts'".to_string(),
                );
                instructions.push("4. Set your preferred notification delay".to_string());
                instructions.push(
                    "5. For shell integration notifications, consider using tools like terminal-notifier".to_string(),
                );
                instructions.push(String::new());
            }
        }
    }

    instructions.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
    instructions.push("After configuration, restart iTerm2 for changes to take effect.".to_string());
    instructions.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());

    Ok(instructions.join("\n"))
}

/// Stable identity of the shipped iTerm2 profile so reinstalls overwrite
/// instead of duplicating it.
pub const PROFILE_GUID: &str = "1FC21C70-F2B1-4F0F-BB03-D1AE12EF900E";

/// Filename of the installed profile artwork under the data root.
pub const PROFILE_ICON_FILENAME: &str = "vtcode-profile-120.png";

/// Embedded 120px profile artwork so installed binaries work without a
/// repo checkout. Resolved through the `build.rs` embedded-assets pipeline so
/// the published crate compiles without reaching outside its package root.
const PROFILE_ICON_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/embedded_assets/resources/icons/vtcode-profile-120.png"));

/// Outcome of [`install_profile_icon`].
pub struct ProfileIconInstallReport {
    /// Dynamic-profile JSON that iTerm2 loads live.
    pub profile_path: PathBuf,
    /// Installed artwork referenced by the profile.
    pub icon_path: PathBuf,
}

/// Render the dynamic-profile JSON pointing at an absolute icon path.
///
/// Unspecified attributes inherit live from the default profile, so
/// switching to this profile changes only the tab icon. There is no
/// `Automatic Profile Switching` rule: VT Code switches to this profile
/// explicitly at TUI startup and reverts to the session's original profile
/// on exit, so automatic reversion (which requires iTerm2 Shell Integration)
/// is neither needed nor reliable.
pub fn dynamic_profile_json(icon_path: &Path) -> Result<String> {
    let document = serde_json::json!({
        "Profiles": [{
            "Name": ITERM2_PROFILE_NAME,
            "Guid": PROFILE_GUID,
            "Icon": ITERM2_ICON_MODE_CUSTOM,
            "Custom Icon Path": icon_path.to_string_lossy(),
        }],
    });
    serde_json::to_string_pretty(&document).context("failed to serialize iTerm2 dynamic profile")
}

/// Absolute destination of the installed icon under a data root.
pub fn installed_icon_path(data_dir: &Path) -> PathBuf {
    data_dir.join("icons").join(PROFILE_ICON_FILENAME)
}

/// Install the VT Code iTerm2 profile icon (macOS only, idempotent).
///
/// Writes only VT Code-owned files: the artwork under the data root and
/// `vtcode.json` under iTerm2's DynamicProfiles directory. Existing
/// profiles are never modified; deleting `vtcode.json` uninstalls.
pub fn install_profile_icon(home: &Path, data_dir: &Path) -> Result<ProfileIconInstallReport> {
    if !cfg!(target_os = "macos") {
        anyhow::bail!("iTerm2 profile icons are only available on macOS");
    }
    let icon_path = installed_icon_path(data_dir);
    if let Some(parent) = icon_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create icon directory {}", parent.display()))?;
    }
    std::fs::write(&icon_path, PROFILE_ICON_BYTES)
        .with_context(|| format!("failed to write icon {}", icon_path.display()))?;
    let profiles_dir = iterm2_dynamic_profiles_dir(home);
    std::fs::create_dir_all(&profiles_dir).with_context(|| format!("failed to create {}", profiles_dir.display()))?;
    let profile_path = profiles_dir.join(ITERM2_DYNAMIC_PROFILE_FILENAME);
    let json = dynamic_profile_json(&icon_path)?;
    std::fs::write(&profile_path, json).with_context(|| format!("failed to write {}", profile_path.display()))?;
    Ok(ProfileIconInstallReport { profile_path, icon_path })
}

/// Resolve default install locations: home directory and canonical data root.
pub fn default_install_paths() -> Result<(PathBuf, PathBuf)> {
    let home = dirs::home_dir().context("failed to determine home directory")?;
    let data_dir = VtCodePaths::resolve()?.data_dir().to_path_buf();
    Ok((home, data_dir))
}

/// Guidance lines pointing at the automatic installer and the manual fallback.
pub fn profile_icon_instructions() -> Vec<String> {
    vec![
        "TAB ICON (profile image):".to_string(),
        "1. Run `/terminal-setup install-iterm2-icon` to install the VT Code tab icon automatically.".to_string(),
        "2. If a tab is stuck showing the VT Code icon after exit, run `/terminal-setup reset-iterm2-icon`."
            .to_string(),
        "3. Or set it manually: Settings → Profiles → General → Icon → Custom, then pick a PNG from resources/icons/."
            .to_string(),
    ]
}

/// Run the non-interactive profile-icon install with progress output.
///
/// Fails closed outside iTerm2 or off macOS; never touches existing profiles.
pub fn run_profile_icon_install(renderer: &mut crate::utils::ansi::AnsiRenderer) -> Result<()> {
    use crate::terminal_setup::detector::TerminalType;
    use crate::utils::ansi::MessageStyle;

    let terminal = TerminalType::detect()?;
    if !matches!(terminal, TerminalType::ITerm2) {
        renderer.line(MessageStyle::Error, &format!("This installer needs iTerm2 (detected {}).", terminal.name()))?;
        return Ok(());
    }
    let (home, data_dir) = default_install_paths()?;
    renderer.line(MessageStyle::Info, "Installing VT Code iTerm2 tab icon...")?;
    let report = install_profile_icon(&home, &data_dir)?;
    renderer.line(MessageStyle::Status, &format!("✓ Profile written: {}", report.profile_path.display()))?;
    renderer.line(MessageStyle::Status, &format!("✓ Icon installed: {}", report.icon_path.display()))?;
    renderer.line(
        MessageStyle::Info,
        "Open a new iTerm2 tab so it picks up the profile; the logo replaces the generic tab glyph while VT Code runs.",
    )?;
    Ok(())
}

/// Reset the current session's iTerm2 profile to its original name.
///
/// Repair path for tabs left stuck on the `VT Code` icon profile by an older
/// build that switched profile without reverting. Emits `OSC 1337;SetProfile=`
/// with the session's original profile (from `ITERM_PROFILE`, falling back to
/// iTerm2's `Default`). Fails closed outside iTerm2 or off macOS.
pub fn run_profile_icon_reset(renderer: &mut crate::utils::ansi::AnsiRenderer) -> Result<()> {
    use crate::terminal_setup::detector::TerminalType;
    use crate::utils::ansi::MessageStyle;
    use std::io::Write as _;
    use vtcode_commons::ansi_codes::set_iterm2_profile;
    use vtcode_commons::terminal_detection::{ITERM2_DEFAULT_PROFILE_NAME, original_iterm2_profile_name};

    let terminal = TerminalType::detect()?;
    if !matches!(terminal, TerminalType::ITerm2) {
        renderer.line(MessageStyle::Error, &format!("This reset needs iTerm2 (detected {}).", terminal.name()))?;
        return Ok(());
    }
    let target = original_iterm2_profile_name().unwrap_or_else(|| ITERM2_DEFAULT_PROFILE_NAME.to_string());
    let mut stdout = std::io::stdout();
    stdout
        .write_all(set_iterm2_profile(&target).as_bytes())
        .with_context(|| "failed to write iTerm2 profile reset sequence")?;
    stdout
        .flush()
        .with_context(|| "failed to flush iTerm2 profile reset sequence")?;
    renderer.line(MessageStyle::Status, &format!("✓ Session profile reset to \"{target}\"."))?;
    Ok(())
}

/// Ensure the iTerm2 profile icon is installed, returning the install
/// report when this call wrote files.
///
/// Best-effort first-run path: skips silently off macOS, outside iTerm2,
/// or under tmux, and rewrites the profile when the installed JSON or
/// artwork drifts from the shipped copy so icon/profile updates propagate.
pub fn ensure_profile_icon() -> Result<Option<ProfileIconInstallReport>> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    let (home, data_dir) = default_install_paths()?;
    let iterm_session = std::env::var("ITERM_SESSION_ID").is_ok();
    let tmux_session = std::env::var("TMUX").is_ok();
    ensure_profile_icon_at(&home, &data_dir, iterm_session, tmux_session)
}

/// Testable core of [`ensure_profile_icon`] with explicit paths and
/// environment flags (no process-environment reads, no global roots).
pub fn ensure_profile_icon_at(
    home: &Path,
    data_dir: &Path,
    iterm_session: bool,
    tmux_session: bool,
) -> Result<Option<ProfileIconInstallReport>> {
    if !cfg!(target_os = "macos") || !iterm_session || tmux_session {
        return Ok(None);
    }
    let profile_path = installed_iterm2_profile_path(home);
    let icon_path = installed_icon_path(data_dir);
    let icon_fresh = std::fs::read(&icon_path)
        .map(|bytes| bytes == PROFILE_ICON_BYTES)
        .unwrap_or(false);
    let profile_current = std::fs::read_to_string(&profile_path)
        .ok()
        .zip(dynamic_profile_json(&icon_path).ok())
        .is_some_and(|(installed, expected)| installed == expected);
    if profile_current && icon_fresh {
        return Ok(None);
    }
    install_profile_icon(home, data_dir).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_setup::detector::TerminalFeature;

    #[test]
    fn test_generate_instructions() {
        let features = vec![TerminalFeature::Multiline, TerminalFeature::CopyPaste];
        let instructions = generate_config(&features).unwrap();
        assert!(instructions.contains("iTerm2"));
        assert!(instructions.contains("Preferences"));
        assert!(instructions.contains("MULTILINE"));
        assert!(instructions.contains("COPY/PASTE"));
    }

    #[test]
    fn dynamic_profile_json_points_at_given_icon() {
        let first = dynamic_profile_json(Path::new("/data/vtcode/icons/vtcode-profile-120.png")).unwrap();
        let second = dynamic_profile_json(Path::new("/other/icons/vtcode-profile-120.png")).unwrap();
        assert_ne!(first, second);

        let value: serde_json::Value = serde_json::from_str(&first).unwrap();
        let profile = &value["Profiles"][0];
        assert_eq!(profile["Name"], ITERM2_PROFILE_NAME);
        assert_eq!(profile["Guid"], PROFILE_GUID);
        assert_eq!(profile["Icon"], ITERM2_ICON_MODE_CUSTOM);
        assert_eq!(profile["Custom Icon Path"], "/data/vtcode/icons/vtcode-profile-120.png");
        assert!(
            profile.get("Automatic Profile Switching").is_none(),
            "reversion is explicit now; the dynamic profile must not carry an APS rule"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_profile_icon_writes_artwork_and_profile() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let first = install_profile_icon(home.path(), data.path()).unwrap();
        assert!(first.profile_path.exists());
        assert!(first.icon_path.exists());
        assert_eq!(std::fs::read(&first.icon_path).unwrap(), PROFILE_ICON_BYTES);

        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&first.profile_path).unwrap()).unwrap();
        assert_eq!(
            stored["Profiles"][0]["Custom Icon Path"].as_str().unwrap().to_string(),
            first.icon_path.to_string_lossy().into_owned()
        );

        let second = install_profile_icon(home.path(), data.path()).unwrap();
        assert_eq!(second.profile_path, first.profile_path);
        assert_eq!(second.icon_path, first.icon_path);
    }

    #[test]
    fn profile_icon_instructions_point_at_installer() {
        let lines = profile_icon_instructions();
        assert!(lines.iter().any(|line| line.contains("install-iterm2-icon")));
    }

    #[test]
    fn ensure_skips_without_iterm_session_and_writes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let report = ensure_profile_icon_at(home.path(), data.path(), false, false).unwrap();
        assert!(report.is_none());
        assert!(!iterm2_dynamic_profiles_dir(home.path()).exists());
    }

    #[test]
    fn ensure_skips_under_tmux_and_writes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let report = ensure_profile_icon_at(home.path(), data.path(), true, true).unwrap();
        assert!(report.is_none());
        assert!(!iterm2_dynamic_profiles_dir(home.path()).exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ensure_installs_once_then_refreshes_stale_artwork() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let first = ensure_profile_icon_at(home.path(), data.path(), true, false).unwrap();
        assert!(first.is_some());

        let second = ensure_profile_icon_at(home.path(), data.path(), true, false).unwrap();
        assert!(second.is_none());

        let icon_path = installed_icon_path(data.path());
        std::fs::write(&icon_path, b"stale-bytes").unwrap();
        let third = ensure_profile_icon_at(home.path(), data.path(), true, false).unwrap();
        let report = third.expect("stale artwork must trigger a refresh");
        assert_eq!(std::fs::read(&report.icon_path).unwrap(), PROFILE_ICON_BYTES);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ensure_refreshes_when_installed_profile_content_drifts() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let first = ensure_profile_icon_at(home.path(), data.path(), true, false).unwrap();
        let profile_path = first.expect("first run installs").profile_path;

        // Simulate an older install that still carries the removed APS rule.
        std::fs::write(&profile_path, r#"{"Profiles":[{"Name":"VT Code"}]}"#).unwrap();
        let refreshed = ensure_profile_icon_at(home.path(), data.path(), true, false).unwrap();
        let report = refreshed.expect("drifted profile content must trigger a rewrite");
        assert_eq!(report.profile_path, profile_path);

        let stored: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&profile_path).unwrap()).unwrap();
        assert!(stored["Profiles"][0].get("Automatic Profile Switching").is_none());
    }
}
