use anyhow::Result;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

const DESKTOP_INSTALL_COMMAND: &str = "vtcode desktop --install";
const DESKTOP_SUGGEST_OPT_OUT: &str = "[desktop] suggest-install = false";
const SECS_PER_DAY: u64 = 86_400;

/// Suggest installing the native macOS desktop app when it is missing.
///
/// Shown at most once per calendar day (UTC); the user opts out permanently
/// via `[desktop] suggest-install = false`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesktopSuggestNotice;

impl DesktopSuggestNotice {
    /// Pure display decision: installed or already shown today → `None`.
    /// Callers check the `suggest_install` preference before any I/O.
    pub(crate) fn from_snapshot(
        installed: bool,
        last_shown_unix_secs: Option<u64>,
        now_unix_secs: u64,
    ) -> Option<Self> {
        if installed {
            return None;
        }
        let shown_today = last_shown_unix_secs.is_some_and(|last| last / SECS_PER_DAY == now_unix_secs / SECS_PER_DAY);
        if shown_today {
            return None;
        }
        Some(Self)
    }

    pub(crate) fn render(&self, renderer: &mut AnsiRenderer) -> Result<()> {
        renderer.line(MessageStyle::Status, "VT Code Desktop isn't installed — meet the native macOS app.")?;
        for line in self.lines() {
            renderer.line(MessageStyle::Info, &line)?;
        }
        renderer.line(MessageStyle::Info, "")?;
        Ok(())
    }

    fn lines(&self) -> Vec<String> {
        vec![
            "The full engine behind a premium native interface, sharing this CLI's providers, config, and sessions."
                .to_string(),
            format!("Install it anytime with `{DESKTOP_INSTALL_COMMAND}`."),
            format!("Mute this reminder with `{DESKTOP_SUGGEST_OPT_OUT}` in vtcode.toml."),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::DesktopSuggestNotice;

    const DAY: u64 = super::SECS_PER_DAY;

    #[test]
    fn shows_when_missing_and_never_shown() {
        assert!(DesktopSuggestNotice::from_snapshot(false, None, 100 * DAY).is_some());
    }

    #[test]
    fn hides_when_installed() {
        assert!(DesktopSuggestNotice::from_snapshot(true, None, 100 * DAY).is_none());
    }

    #[test]
    fn hides_when_shown_today() {
        // Same UTC day: morning show suppresses an evening launch.
        assert!(DesktopSuggestNotice::from_snapshot(false, Some(100 * DAY), 100 * DAY + 12 * 3_600).is_none());
    }

    #[test]
    fn shows_again_after_midnight() {
        // Day boundary both sides: last second of day 100 vs first of day 101.
        assert!(DesktopSuggestNotice::from_snapshot(false, Some(101 * DAY - 1), 101 * DAY).is_some());
    }

    #[test]
    fn shows_when_last_shown_is_older_than_a_day() {
        assert!(DesktopSuggestNotice::from_snapshot(false, Some(90 * DAY), 100 * DAY).is_some());
    }

    #[test]
    fn lines_name_install_command_and_opt_out() {
        let lines = DesktopSuggestNotice.lines();

        assert!(lines[0].contains("native"), "{lines:?}");
        assert!(lines[1].contains("vtcode desktop --install"), "{lines:?}");
        assert!(lines[2].contains("[desktop] suggest-install = false"), "{lines:?}");
    }
}
