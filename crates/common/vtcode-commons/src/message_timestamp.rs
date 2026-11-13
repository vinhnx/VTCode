//! Wall-clock timestamps and turn durations for the final agent response of a
//! turn.
//!
//! Each completed turn carries one short local-time footer (e.g. `· 3:12 PM`,
//! optionally `· 3:12 PM · 13m 24s`) after its final agent message, so the
//! clock marks the turn's summary point instead of repeating on every
//! intermediate response. Formatting lives here so the TUI transcript, plain
//! CLI output, and future desktop/export surfaces share one shape.

use std::time::Duration;

use chrono::{DateTime, Local};

/// Format a local timestamp the way agent footers display it (`3:12 PM`).
///
/// Twelve-hour clock without seconds or leading zeros, matching the chat-style
/// reference. Takes the instant as a parameter so tests stay deterministic.
pub fn format_message_time(now: DateTime<Local>) -> String {
    now.format("%-I:%M %p").to_string()
}

/// Format a turn duration the way footers and outcome lines display it.
///
/// Compact clock symbols with zero parts omitted: `42s`, `13m 24s`, `1h 5m`.
/// The desktop app renders the same tiers through its locale catalogs; this
/// plain-symbol shape serves the CLI surfaces that have no i18n layer.
pub fn format_turn_elapsed(duration: Duration) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 3600;
    let seconds = duration.as_secs();
    if seconds < MINUTE {
        return format!("{seconds}s");
    }
    if seconds < HOUR {
        let minutes = seconds / MINUTE;
        let rest = seconds % MINUTE;
        if rest == 0 {
            return format!("{minutes}m");
        }
        return format!("{minutes}m {rest}s");
    }
    let hours = seconds / HOUR;
    let minutes = (seconds % HOUR) / MINUTE;
    if minutes == 0 {
        return format!("{hours}h");
    }
    format!("{hours}h {minutes}m")
}

/// Build the full footer line for a turn's final agent response
/// (`· 3:12 PM`, plus `· 13m 24s` when the caller measured the turn).
///
/// The leading middot marks the line as chrome in no-color output where
/// dimming is unavailable; the elapsed middot keeps both halves one chrome
/// unit so reflow and tail-matching treat them together.
pub fn message_timestamp_footer(now: DateTime<Local>, elapsed: Option<Duration>) -> String {
    match elapsed {
        Some(duration) => format!("· {} · {}", format_message_time(now), format_turn_elapsed(duration)),
        None => format!("· {}", format_message_time(now)),
    }
}

/// Build the footer for the current local time, without an elapsed half.
pub fn current_message_timestamp_footer() -> String {
    message_timestamp_footer(Local::now(), None)
}

/// Build the footer for the current local time with the measured turn
/// duration appended when the caller has one.
pub fn current_message_timestamp_footer_with_elapsed(elapsed: Option<Duration>) -> String {
    message_timestamp_footer(Local::now(), elapsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn local_at(hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 11, 2, hour, minute, 0)
            .single()
            .expect("fixture timestamp must be unambiguous")
    }

    #[test]
    fn formats_afternoon_without_leading_zero() {
        assert_eq!(format_message_time(local_at(15, 12)), "3:12 PM");
    }

    #[test]
    fn formats_single_digit_morning_hour() {
        assert_eq!(format_message_time(local_at(9, 5)), "9:05 AM");
    }

    #[test]
    fn formats_double_digit_morning_hour() {
        assert_eq!(format_message_time(local_at(11, 55)), "11:55 AM");
    }

    #[test]
    fn midnight_is_twelve_am_not_zero() {
        assert_eq!(format_message_time(local_at(0, 5)), "12:05 AM");
    }

    #[test]
    fn noon_is_twelve_pm_not_zero() {
        assert_eq!(format_message_time(local_at(12, 0)), "12:00 PM");
    }

    #[test]
    fn footer_prefixes_time_with_middot() {
        assert_eq!(message_timestamp_footer(local_at(15, 12), None), "· 3:12 PM");
    }

    #[test]
    fn footer_appends_elapsed_after_the_clock() {
        assert_eq!(message_timestamp_footer(local_at(15, 12), Some(Duration::from_secs(804))), "· 3:12 PM · 13m 24s");
        assert_eq!(message_timestamp_footer(local_at(15, 12), Some(Duration::from_secs(3900))), "· 3:12 PM · 1h 5m");
    }

    #[test]
    fn current_footer_with_elapsed_keeps_one_chrome_unit() {
        let footer = current_message_timestamp_footer_with_elapsed(Some(Duration::from_secs(59)));
        let time = footer.strip_prefix("· ").expect("current footer must carry the middot");
        let mut halves = time.split(" · ");
        let clock = halves.next().expect("clock half must exist");
        assert!(clock.contains(':'), "clock half must show clock time: {footer:?}");
        assert!(clock.ends_with("AM") || clock.ends_with("PM"), "clock half must show meridiem: {footer:?}");
        assert_eq!(halves.next(), Some("59s"), "elapsed half must follow the clock");
        assert!(halves.next().is_none(), "footer must stay one clock plus one duration");
    }

    #[test]
    fn elapsed_symbols_omit_zero_parts() {
        assert_eq!(format_turn_elapsed(Duration::from_secs(0)), "0s");
        assert_eq!(format_turn_elapsed(Duration::from_secs(59)), "59s");
        assert_eq!(format_turn_elapsed(Duration::from_secs(60)), "1m");
        assert_eq!(format_turn_elapsed(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_turn_elapsed(Duration::from_secs(3600)), "1h");
        assert_eq!(format_turn_elapsed(Duration::from_secs(3900)), "1h 5m");
    }

    #[test]
    fn current_footer_has_footer_shape() {
        let footer = current_message_timestamp_footer();
        let time = footer.strip_prefix("· ").expect("current footer must carry the middot");
        assert!(time.contains(':'), "current footer must show clock time: {footer:?}");
        assert!(time.ends_with("AM") || time.ends_with("PM"), "current footer must show meridiem: {footer:?}");
    }
}
