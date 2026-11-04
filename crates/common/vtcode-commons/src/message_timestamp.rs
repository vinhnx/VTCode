//! Wall-clock timestamps for the final agent response of a turn.
//!
//! Each completed turn carries one short local-time footer (e.g. `· 3:12 PM`)
//! after its final agent message, so the clock marks the turn's summary point
//! instead of repeating on every intermediate response. Formatting lives here
//! so the TUI transcript, plain CLI output, and future desktop/export surfaces
//! share one shape.

use chrono::{DateTime, Local};

/// Format a local timestamp the way agent footers display it (`3:12 PM`).
///
/// Twelve-hour clock without seconds or leading zeros, matching the chat-style
/// reference. Takes the instant as a parameter so tests stay deterministic.
pub fn format_message_time(now: DateTime<Local>) -> String {
    now.format("%-I:%M %p").to_string()
}

/// Build the full footer line for a turn's final agent response (`· 3:12 PM`).
///
/// The leading middot marks the line as chrome in no-color output where
/// dimming is unavailable.
pub fn message_timestamp_footer(now: DateTime<Local>) -> String {
    format!("· {}", format_message_time(now))
}

/// Build the footer for the current local time.
pub fn current_message_timestamp_footer() -> String {
    message_timestamp_footer(Local::now())
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
        assert_eq!(message_timestamp_footer(local_at(15, 12)), "· 3:12 PM");
    }

    #[test]
    fn current_footer_has_footer_shape() {
        let footer = current_message_timestamp_footer();
        let time = footer.strip_prefix("· ").expect("current footer must carry the middot");
        assert!(time.contains(':'), "current footer must show clock time: {footer:?}");
        assert!(time.ends_with("AM") || time.ends_with("PM"), "current footer must show meridiem: {footer:?}");
    }
}
