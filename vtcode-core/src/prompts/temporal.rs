use chrono::{DateTime, Local, Utc};

/// Generate temporal context string for system prompt
///
/// Adds current date and time to help the LLM understand temporal context
/// for time-sensitive tasks like log analysis, scheduling, or date calculations.
///
/// # Arguments
/// * `use_utc` - If true, use UTC time; if false, use local system time
///
/// # Returns
/// Formatted string with current date/time
///
/// # Examples
/// ```
/// use vtcode_core::prompts::temporal::generate_temporal_context;
///
/// // Local time
/// let context = generate_temporal_context(false);
/// assert!(context.contains("Current date and time:"));
///
/// // UTC time
/// let context = generate_temporal_context(true);
/// assert!(context.contains("UTC"));
/// ```
pub fn generate_temporal_context(use_utc: bool) -> String {
    if use_utc {
        let now: DateTime<Utc> = Utc::now();
        format!(
            "\n\nCurrent date and time (UTC): {}",
            now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        )
    } else {
        let now: DateTime<Local> = Local::now();
        format!(
            "\n\nCurrent date and time: {}",
            now.format("%A, %B %d, %Y at %I:%M:%S %p %Z")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temporal_context_utc_format() {
        let context = generate_temporal_context(true);
        assert!(
            context.contains("Current date and time (UTC):"),
            "Should include UTC label"
        );
        assert!(context.contains("T"), "Should use RFC3339 format with T separator");
        assert!(context.contains("Z"), "Should include Z timezone indicator");
    }

    #[test]
    fn test_temporal_context_local_format() {
        let context = generate_temporal_context(false);
        assert!(
            context.contains("Current date and time:"),
            "Should include date/time label"
        );
        assert!(context.contains(" at "), "Should include 'at' separator");
        // Check for day of week (will be one of the weekdays)
        let has_weekday = context.contains("Monday")
            || context.contains("Tuesday")
            || context.contains("Wednesday")
            || context.contains("Thursday")
            || context.contains("Friday")
            || context.contains("Saturday")
            || context.contains("Sunday");
        assert!(has_weekday, "Should include day of week");
    }

    #[test]
    fn test_temporal_context_non_empty() {
        let utc = generate_temporal_context(true);
        let local = generate_temporal_context(false);

        assert!(!utc.is_empty(), "UTC context should not be empty");
        assert!(!local.is_empty(), "Local context should not be empty");
        assert!(utc.len() > 30, "UTC context should have reasonable length");
        assert!(local.len() > 40, "Local context should have reasonable length");
    }

    #[test]
    fn test_temporal_context_starts_with_newlines() {
        let utc = generate_temporal_context(true);
        let local = generate_temporal_context(false);

        assert!(
            utc.starts_with("\n\n"),
            "Should start with double newline for spacing"
        );
        assert!(
            local.starts_with("\n\n"),
            "Should start with double newline for spacing"
        );
    }
}
