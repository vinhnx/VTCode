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
use vtcode_commons::ui_protocol::AppIconPreference;

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
pub const PROFILE_ICON_FILENAME: &str = "vtcode-app-icon.png";

/// Retired artwork filename from earlier installs; removed opportunistically
/// on install so the data dir does not keep stale bytes forever.
const LEGACY_PROFILE_ICON_FILENAME: &str = "vtcode-profile-120.png";

/// Embedded 128px profile artwork so installed binaries work without a repo
/// checkout. Resolved through the `build.rs` embedded-assets pipeline so the
/// published crate compiles without reaching outside its package root.
const PROFILE_ICON_LIGHT_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/embedded_assets/resources/icons/vtcode-app-light-128.png"));
const PROFILE_ICON_DARK_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/embedded_assets/resources/icons/vtcode-app-dark-128.png"));
const PROFILE_ICON_CLEAR_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/embedded_assets/resources/icons/vtcode-app-clear-128.png"));
const PROFILE_ICON_TINTED_BYTES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/embedded_assets/resources/icons/vtcode-app-tinted-128.png"));

/// Artwork bytes for a concrete icon variant.
///
/// Callers pass [`resolve_artwork`] output: unresolved `Dynamic`/`Unknown`
/// input falls back to the light tile rather than failing the install.
pub fn artwork_bytes(preference: AppIconPreference) -> &'static [u8] {
    match preference {
        AppIconPreference::Dark => PROFILE_ICON_DARK_BYTES,
        AppIconPreference::Clear => PROFILE_ICON_CLEAR_BYTES,
        AppIconPreference::Tinted => PROFILE_ICON_TINTED_BYTES,
        AppIconPreference::Light | AppIconPreference::Dynamic | AppIconPreference::Unknown => PROFILE_ICON_LIGHT_BYTES,
    }
}

/// Reject empty artwork: published-crate, Nix, and docs.rs builds embed an
/// empty placeholder (see `build.rs`), which must never be installed as the
/// tab icon.
fn require_artwork_available(bytes: &'static [u8]) -> Result<&'static [u8]> {
    if bytes.is_empty() {
        anyhow::bail!("icon artwork is unavailable in this build; reinstall from a full release")
    } else {
        Ok(bytes)
    }
}

/// Resolve an icon preference to concrete artwork against a theme id.
/// `Dynamic` (and `Unknown`) follow the theme's own light/dark mode;
/// explicit variants pin their art.
pub fn resolve_artwork_with_theme(preference: AppIconPreference, theme_id: &str) -> AppIconPreference {
    preference.resolve(!crate::ui::theme::is_light_theme(theme_id))
}

/// Resolve an icon preference to concrete artwork against the active CLI theme.
pub fn resolve_artwork(preference: AppIconPreference) -> AppIconPreference {
    resolve_artwork_with_theme(preference, &crate::ui::theme::active_theme_id())
}

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
/// Pass [`resolve_artwork`] output: unresolved `Dynamic`/`Unknown` input
/// falls back to the light tile (see [`artwork_bytes`]). Writes only VT
/// Code-owned files: the artwork under the data root and `vtcode.json`
/// under iTerm2's DynamicProfiles directory. Existing profiles are never
/// modified; deleting `vtcode.json` uninstalls.
pub fn install_profile_icon(
    home: &Path,
    data_dir: &Path,
    preference: AppIconPreference,
) -> Result<ProfileIconInstallReport> {
    if !cfg!(target_os = "macos") {
        anyhow::bail!("iTerm2 profile icons are only available on macOS");
    }
    let icon_path = installed_icon_path(data_dir);
    if let Some(parent) = icon_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create icon directory {}", parent.display()))?;
    }
    std::fs::write(&icon_path, require_artwork_available(artwork_bytes(preference))?)
        .with_context(|| format!("failed to write icon {}", icon_path.display()))?;
    if let Some(parent) = icon_path.parent() {
        let _removed = std::fs::remove_file(parent.join(LEGACY_PROFILE_ICON_FILENAME));
    }
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
        "3. Switch artwork with `/terminal-setup icon light|dark|clear|tinted|dynamic` (persist via `ui.app_icon`)."
            .to_string(),
        "4. Or set it manually: Settings → Profiles → General → Icon → Custom, then pick a PNG from resources/icons/."
            .to_string(),
    ]
}

/// Fail closed unless the session runs under iTerm2. Returns false after
/// reporting, so callers just early-return. Reset-only: it emits a
/// live-session profile switch, which is meaningless outside iTerm2.
fn check_iterm2_terminal(renderer: &mut crate::utils::ansi::AnsiRenderer) -> Result<bool> {
    use crate::utils::ansi::MessageStyle;

    let terminal = TerminalType::detect()?;
    if matches!(terminal, TerminalType::ITerm2) {
        return Ok(true);
    }
    renderer.line(MessageStyle::Error, &format!("Tab icon commands need iTerm2 (detected {}).", terminal.name()))?;
    Ok(false)
}

/// Fail closed off macOS. Returns false after reporting, so callers just
/// early-return.
///
/// Install/show/switch only write VT Code-owned files (show only reads
/// theme state), so they accept any terminal on macOS: a stale iTerm2
/// profile can be repaired from Rex, Ghostty, or anywhere else without
/// launching iTerm2 first.
fn check_macos(renderer: &mut crate::utils::ansi::AnsiRenderer) -> Result<bool> {
    use crate::utils::ansi::MessageStyle;

    if cfg!(target_os = "macos") {
        return Ok(true);
    }
    renderer.line(MessageStyle::Error, "Tab icon commands need macOS (iTerm2 profiles are macOS-only).")?;
    Ok(false)
}

/// Parsed `/terminal-setup icon` subcommand.
#[derive(Debug, PartialEq, Eq)]
pub enum TerminalIconCommand {
    /// Report the Dynamic resolution without writing files.
    Show,
    /// Install one variant immediately.
    Switch(AppIconPreference),
}

/// Parse `icon [variant]` args (case- and whitespace-tolerant). Returns
/// `None` for anything that is not an icon command, including unknown
/// variants, so callers fall through to usage.
pub fn parse_terminal_icon_command(args: &str) -> Option<TerminalIconCommand> {
    let lowered = args.trim().to_ascii_lowercase();
    let parts: Vec<&str> = lowered.split_whitespace().collect();
    match parts.as_slice() {
        ["icon"] => Some(TerminalIconCommand::Show),
        ["icon", "light"] => Some(TerminalIconCommand::Switch(AppIconPreference::Light)),
        ["icon", "dark"] => Some(TerminalIconCommand::Switch(AppIconPreference::Dark)),
        ["icon", "clear"] => Some(TerminalIconCommand::Switch(AppIconPreference::Clear)),
        ["icon", "tinted"] => Some(TerminalIconCommand::Switch(AppIconPreference::Tinted)),
        ["icon", "dynamic"] => Some(TerminalIconCommand::Switch(AppIconPreference::Dynamic)),
        _ => None,
    }
}

/// Run the non-interactive profile-icon install with progress output.
///
/// Installs the `Dynamic` artwork (resolved against the active CLI theme).
/// Fails closed off macOS; works from any terminal since it only writes
/// VT Code-owned files, never existing profiles.
pub fn run_profile_icon_install(renderer: &mut crate::utils::ansi::AnsiRenderer) -> Result<()> {
    run_terminal_icon_switch(renderer, AppIconPreference::Dynamic)
}

/// Report the Dynamic artwork resolution without writing files.
///
/// Fails closed off macOS like the installer: the advice only applies
/// to the iTerm2 dynamic profile.
pub fn run_terminal_icon_show(renderer: &mut crate::utils::ansi::AnsiRenderer) -> Result<()> {
    use crate::utils::ansi::MessageStyle;

    if !check_macos(renderer)? {
        return Ok(());
    }
    let theme_id = crate::ui::theme::active_theme_id();
    let resolved = resolve_artwork(AppIconPreference::Dynamic);
    renderer.line(
        MessageStyle::Info,
        &format!(
            "With `ui.app_icon = \"dynamic\"` (default), the tab icon follows the active theme ('{theme_id}', {} artwork). Switch now with `/terminal-setup icon light|dark|clear|tinted|dynamic`, or pin via `ui.app_icon`.",
            resolved.id(),
        ),
    )?;
    Ok(())
}

/// Install one icon variant immediately.
///
/// Explicit variants apply for this session (persist via `ui.app_icon`);
/// `Dynamic` is the default and needs no persistence note. Fails closed
/// off macOS; works from any terminal since it only writes VT Code-owned
/// files, never existing profiles.
pub fn run_terminal_icon_switch(
    renderer: &mut crate::utils::ansi::AnsiRenderer,
    preference: AppIconPreference,
) -> Result<()> {
    use crate::utils::ansi::MessageStyle;

    if !check_macos(renderer)? {
        return Ok(());
    }
    let (home, data_dir) = default_install_paths()?;
    let resolved = resolve_artwork(preference);
    renderer.line(MessageStyle::Info, &format!("Installing VT Code iTerm2 tab icon ({} artwork)...", resolved.id()))?;
    let report = install_profile_icon(&home, &data_dir, resolved)?;
    renderer.line(MessageStyle::Status, &format!("✓ Profile written: {}", report.profile_path.display()))?;
    renderer.line(MessageStyle::Status, &format!("✓ Icon installed: {}", report.icon_path.display()))?;
    renderer.line(
        MessageStyle::Info,
        "Open a new iTerm2 tab so it picks up the profile; the logo replaces the generic tab glyph while VT Code runs.",
    )?;
    if !matches!(preference, AppIconPreference::Dynamic | AppIconPreference::Unknown) {
        renderer.line(
            MessageStyle::Info,
            &format!("Applied for this session. Persist with `ui.app_icon = \"{}\"`.", preference.id()),
        )?;
    }
    Ok(())
}

/// Reset the current session's iTerm2 profile to its original name.
///
/// Repair path for tabs left stuck on the `VT Code` icon profile by an older
/// build that switched profile without reverting. Emits `OSC 1337;SetProfile=`
/// with the session's original profile (from `ITERM_PROFILE`, falling back to
/// iTerm2's `Default`). Fails closed outside iTerm2 or off macOS.
pub fn run_profile_icon_reset(renderer: &mut crate::utils::ansi::AnsiRenderer) -> Result<()> {
    use crate::utils::ansi::MessageStyle;
    use std::io::Write as _;
    use vtcode_commons::ansi_codes::set_iterm2_profile;
    use vtcode_commons::terminal_detection::{ITERM2_DEFAULT_PROFILE_NAME, original_iterm2_profile_name};

    if !check_iterm2_terminal(renderer)? {
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
/// `Dynamic` resolves against the active CLI theme, so a theme-mode change
/// across launches refreshes the tab icon on the next startup.
pub fn ensure_profile_icon(preference: AppIconPreference) -> Result<Option<ProfileIconInstallReport>> {
    if !cfg!(target_os = "macos") {
        return Ok(None);
    }
    let (home, data_dir) = default_install_paths()?;
    let iterm_session = std::env::var("ITERM_SESSION_ID").is_ok();
    let tmux_session = std::env::var("TMUX").is_ok();
    let resolved = resolve_artwork(preference);
    ensure_profile_icon_at(&home, &data_dir, iterm_session, tmux_session, resolved)
}

/// Testable core of [`ensure_profile_icon`] with explicit paths, environment
/// flags, and resolved artwork preference (no process-environment reads, no
/// global roots, no theme-global reads).
pub fn ensure_profile_icon_at(
    home: &Path,
    data_dir: &Path,
    iterm_session: bool,
    tmux_session: bool,
    preference: AppIconPreference,
) -> Result<Option<ProfileIconInstallReport>> {
    if !cfg!(target_os = "macos") || !iterm_session || tmux_session {
        return Ok(None);
    }
    let profile_path = installed_iterm2_profile_path(home);
    let icon_path = installed_icon_path(data_dir);
    let expected_artwork = artwork_bytes(preference);
    let icon_fresh = std::fs::read(&icon_path)
        .map(|bytes| bytes == expected_artwork)
        .unwrap_or(false);
    let profile_current = std::fs::read_to_string(&profile_path)
        .ok()
        .zip(dynamic_profile_json(&icon_path).ok())
        .is_some_and(|(installed, expected)| installed == expected);
    if profile_current && icon_fresh {
        return Ok(None);
    }
    install_profile_icon(home, data_dir, preference).map(Some)
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
        let first = dynamic_profile_json(Path::new("/data/vtcode/icons/vtcode-app-icon.png")).unwrap();
        let second = dynamic_profile_json(Path::new("/other/icons/vtcode-app-icon.png")).unwrap();
        assert_ne!(first, second);

        let value: serde_json::Value = serde_json::from_str(&first).unwrap();
        let profile = &value["Profiles"][0];
        assert_eq!(profile["Name"], ITERM2_PROFILE_NAME);
        assert_eq!(profile["Guid"], PROFILE_GUID);
        assert_eq!(profile["Icon"], ITERM2_ICON_MODE_CUSTOM);
        assert_eq!(profile["Custom Icon Path"], "/data/vtcode/icons/vtcode-app-icon.png");
        assert!(
            profile.get("Automatic Profile Switching").is_none(),
            "reversion is explicit now; the dynamic profile must not carry an APS rule"
        );
    }

    #[test]
    fn artwork_bytes_differ_per_variant_and_are_non_empty() {
        let variants = [
            AppIconPreference::Light,
            AppIconPreference::Dark,
            AppIconPreference::Clear,
            AppIconPreference::Tinted,
        ];
        for preference in variants {
            let bytes = artwork_bytes(preference);
            assert!(!bytes.is_empty(), "{preference:?} artwork must be embedded");
            assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "{preference:?} artwork must be a PNG");
            // IHDR is always the first chunk: width/height sit at bytes 16..24.
            // Fails when a wrong-size master (e.g. a 512px file) is embedded.
            assert_eq!(&bytes[12..16], b"IHDR", "{preference:?} artwork must start with IHDR");
            let width = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
            let height = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
            assert_eq!((width, height), (128, 128), "{preference:?} artwork must be 128x128");
        }
        for (index, first) in variants.iter().enumerate() {
            for second in &variants[index + 1..] {
                assert_ne!(artwork_bytes(*first), artwork_bytes(*second), "{first:?} vs {second:?}");
            }
        }
        assert_eq!(artwork_bytes(AppIconPreference::Dynamic), artwork_bytes(AppIconPreference::Light));
        assert_eq!(artwork_bytes(AppIconPreference::Unknown), artwork_bytes(AppIconPreference::Light));
    }

    #[test]
    fn resolve_artwork_pins_explicit_variants_and_follows_theme_for_dynamic() {
        use vtcode_commons::ui_protocol::AppIconPreference;

        for theme_id in ["mono", "vitesse-light", "unknown-theme"] {
            for pinned in [
                AppIconPreference::Light,
                AppIconPreference::Dark,
                AppIconPreference::Clear,
                AppIconPreference::Tinted,
            ] {
                assert_eq!(resolve_artwork_with_theme(pinned, theme_id), pinned, "{pinned:?} on {theme_id}");
            }
        }
        assert_eq!(resolve_artwork_with_theme(AppIconPreference::Dynamic, "mono"), AppIconPreference::Dark);
        assert_eq!(resolve_artwork_with_theme(AppIconPreference::Dynamic, "vitesse-light"), AppIconPreference::Light);
        assert_eq!(
            resolve_artwork_with_theme(AppIconPreference::Unknown, "vitesse-light"),
            resolve_artwork_with_theme(AppIconPreference::Dynamic, "vitesse-light")
        );
    }

    #[test]
    fn parse_terminal_icon_command_accepts_show_and_each_variant() {
        use vtcode_commons::ui_protocol::AppIconPreference;

        assert_eq!(parse_terminal_icon_command("icon"), Some(TerminalIconCommand::Show));
        for (raw, expected) in [
            ("light", AppIconPreference::Light),
            ("dark", AppIconPreference::Dark),
            ("clear", AppIconPreference::Clear),
            ("tinted", AppIconPreference::Tinted),
            ("dynamic", AppIconPreference::Dynamic),
        ] {
            assert_eq!(
                parse_terminal_icon_command(&format!("icon {raw}")),
                Some(TerminalIconCommand::Switch(expected)),
                "{raw}"
            );
        }
    }

    #[test]
    fn parse_terminal_icon_command_is_case_and_whitespace_tolerant() {
        use vtcode_commons::ui_protocol::AppIconPreference;

        for raw in ["  ICON  ", "Icon", "iCoN"] {
            assert_eq!(parse_terminal_icon_command(raw), Some(TerminalIconCommand::Show), "{raw:?}");
        }
        for raw in ["icon  light", "ICON\tDARK\n", "  icon dynamic  "] {
            let parsed = parse_terminal_icon_command(raw).expect("variant parses");
            assert!(matches!(parsed, TerminalIconCommand::Switch(_)), "{raw:?}");
        }
        assert_eq!(
            parse_terminal_icon_command("icon  light"),
            Some(TerminalIconCommand::Switch(AppIconPreference::Light))
        );
        assert_eq!(
            parse_terminal_icon_command("ICON\tDARK\n"),
            Some(TerminalIconCommand::Switch(AppIconPreference::Dark))
        );
    }

    #[test]
    fn parse_terminal_icon_command_rejects_unknown_and_extra_args() {
        for raw in [
            "",
            "icons",
            "icon holographic",
            "icon light dark",
            "icon-light",
            "install-iterm2-icon",
        ] {
            assert_eq!(parse_terminal_icon_command(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn require_artwork_available_rejects_empty_placeholders() {
        assert!(require_artwork_available(b"").is_err());
        assert_eq!(require_artwork_available(b"x").unwrap(), b"x");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_profile_icon_writes_artwork_and_profile() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let first = install_profile_icon(home.path(), data.path(), AppIconPreference::Dark).unwrap();
        assert!(first.profile_path.exists());
        assert!(first.icon_path.exists());
        assert_eq!(std::fs::read(&first.icon_path).unwrap(), artwork_bytes(AppIconPreference::Dark));

        let stored: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&first.profile_path).unwrap()).unwrap();
        assert_eq!(
            stored["Profiles"][0]["Custom Icon Path"].as_str().unwrap().to_string(),
            first.icon_path.to_string_lossy().into_owned()
        );

        let second = install_profile_icon(home.path(), data.path(), AppIconPreference::Dark).unwrap();
        assert_eq!(second.profile_path, first.profile_path);
        assert_eq!(second.icon_path, first.icon_path);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_profile_icon_removes_retired_legacy_artwork() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let legacy = data.path().join("icons").join(LEGACY_PROFILE_ICON_FILENAME);
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, b"stale-legacy-bytes").unwrap();

        let report = install_profile_icon(home.path(), data.path(), AppIconPreference::Dark).unwrap();
        assert!(report.icon_path.exists());
        assert!(!legacy.exists(), "retired artwork must be removed on install");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn install_profile_icon_switches_artwork_between_variants() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        // Install every pinned variant in turn: each write must land that
        // variant's own bytes at the same stable path (fails on crossed
        // artwork selection, where two variants would install identical art).
        let mut installed: Vec<Vec<u8>> = Vec::new();
        for preference in [
            AppIconPreference::Dark,
            AppIconPreference::Light,
            AppIconPreference::Clear,
            AppIconPreference::Tinted,
        ] {
            let report = install_profile_icon(home.path(), data.path(), preference).unwrap();
            assert_eq!(report.icon_path, installed_icon_path(data.path()));
            let bytes = std::fs::read(&report.icon_path).unwrap();
            assert_eq!(bytes, artwork_bytes(preference), "{preference:?} must install its own art");
            assert!(!installed.contains(&bytes), "{preference:?} art must differ from earlier installs");
            installed.push(bytes);
        }
    }

    #[test]
    fn profile_icon_instructions_point_at_installer() {
        let lines = profile_icon_instructions();
        assert!(lines.iter().any(|line| line.contains("install-iterm2-icon")));
        // The switch guidance must name every selectable variant, so a future
        // variant cannot land in the parser without surfacing in setup help.
        let joined = lines.join("\n");
        for variant in AppIconPreference::ALL {
            assert!(joined.contains(variant.id()), "instructions lack {}", variant.id());
        }
    }

    #[test]
    fn ensure_skips_without_iterm_session_and_writes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let report = ensure_profile_icon_at(home.path(), data.path(), false, false, AppIconPreference::Dark).unwrap();
        assert!(report.is_none());
        assert!(!iterm2_dynamic_profiles_dir(home.path()).exists());
    }

    #[test]
    fn ensure_skips_under_tmux_and_writes_nothing() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let report = ensure_profile_icon_at(home.path(), data.path(), true, true, AppIconPreference::Dark).unwrap();
        assert!(report.is_none());
        assert!(!iterm2_dynamic_profiles_dir(home.path()).exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ensure_installs_once_then_refreshes_stale_artwork() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let first = ensure_profile_icon_at(home.path(), data.path(), true, false, AppIconPreference::Dark).unwrap();
        assert!(first.is_some());

        let second = ensure_profile_icon_at(home.path(), data.path(), true, false, AppIconPreference::Dark).unwrap();
        assert!(second.is_none());

        let icon_path = installed_icon_path(data.path());
        std::fs::write(&icon_path, b"stale-bytes").unwrap();
        let third = ensure_profile_icon_at(home.path(), data.path(), true, false, AppIconPreference::Dark).unwrap();
        let report = third.expect("stale artwork must trigger a refresh");
        assert_eq!(std::fs::read(&report.icon_path).unwrap(), artwork_bytes(AppIconPreference::Dark));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ensure_refreshes_when_resolved_artwork_changes() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let first = ensure_profile_icon_at(home.path(), data.path(), true, false, AppIconPreference::Dark).unwrap();
        assert!(first.is_some());
        assert_eq!(std::fs::read(installed_icon_path(data.path())).unwrap(), artwork_bytes(AppIconPreference::Dark));

        let second = ensure_profile_icon_at(home.path(), data.path(), true, false, AppIconPreference::Clear).unwrap();
        let report = second.expect("artwork change must refresh the installed artwork");
        assert_eq!(std::fs::read(&report.icon_path).unwrap(), artwork_bytes(AppIconPreference::Clear));

        let third = ensure_profile_icon_at(home.path(), data.path(), true, false, AppIconPreference::Clear).unwrap();
        assert!(third.is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ensure_refreshes_when_installed_profile_content_drifts() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        let first = ensure_profile_icon_at(home.path(), data.path(), true, false, AppIconPreference::Dark).unwrap();
        let profile_path = first.expect("first run installs").profile_path;

        // Simulate an older install that still carries the removed APS rule.
        std::fs::write(&profile_path, r#"{"Profiles":[{"Name":"VT Code"}]}"#).unwrap();
        let refreshed = ensure_profile_icon_at(home.path(), data.path(), true, false, AppIconPreference::Dark).unwrap();
        let report = refreshed.expect("drifted profile content must trigger a rewrite");
        assert_eq!(report.profile_path, profile_path);

        let stored: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&profile_path).unwrap()).unwrap();
        assert!(stored["Profiles"][0].get("Automatic Profile Switching").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn ensure_repairs_profile_pointing_at_retired_legacy_artwork() {
        let home = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();

        // Arrange: an older install whose profile still points at the
        // retired `vtcode-profile-120.png` filename (observed in the wild).
        let legacy = installed_icon_path(data.path())
            .parent()
            .unwrap()
            .join(LEGACY_PROFILE_ICON_FILENAME);
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, b"stale-legacy-bytes").unwrap();
        let profiles_dir = iterm2_dynamic_profiles_dir(home.path());
        std::fs::create_dir_all(&profiles_dir).unwrap();
        let profile_path = installed_iterm2_profile_path(home.path());
        let stale = serde_json::json!({
            "Profiles": [{
                "Name": ITERM2_PROFILE_NAME,
                "Guid": PROFILE_GUID,
                "Icon": ITERM2_ICON_MODE_CUSTOM,
                "Custom Icon Path": legacy.to_string_lossy(),
            }],
        });
        std::fs::write(&profile_path, serde_json::to_string_pretty(&stale).unwrap()).unwrap();

        // Act.
        let refreshed = ensure_profile_icon_at(home.path(), data.path(), true, false, AppIconPreference::Dark).unwrap();
        let report = refreshed.expect("legacy-path profile must trigger a reinstall");

        // Assert: profile repoints at the current artwork and the retired
        // file is gone, so the tab can no longer load stale bytes.
        assert_eq!(report.profile_path, profile_path);
        assert_eq!(report.icon_path, installed_icon_path(data.path()));
        assert_eq!(std::fs::read(&report.icon_path).unwrap(), artwork_bytes(AppIconPreference::Dark));
        assert!(!legacy.exists(), "retired artwork must be removed on repair");
        let stored: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&profile_path).unwrap()).unwrap();
        assert_eq!(stored["Profiles"][0]["Custom Icon Path"].as_str().unwrap(), report.icon_path.to_string_lossy());
    }
}
