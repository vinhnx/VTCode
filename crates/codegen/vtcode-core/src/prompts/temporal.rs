use chrono::{DateTime, Local, Utc};

/// Generate a cache-friendly date-only context for the system prompt.
///
/// Prompt caching is a prefix match: an in-depth per-second timestamp in the
/// static prompt breaks the cache on every rebuild. The system prompt carries
/// only the calendar date (stable all day); precise clock time belongs in a
/// `<system-reminder>` message appended to the history when a task actually
/// needs it.
///
/// # Examples
/// ```
/// use vtcode_core::prompts::temporal::generate_temporal_date_context;
///
/// let context = generate_temporal_date_context(false);
/// assert!(context.contains("Current date:"));
/// assert!(!context.contains(" at "));
/// ```
pub fn generate_temporal_date_context(use_utc: bool) -> String {
    if use_utc {
        let now: DateTime<Utc> = Utc::now();
        format!("\n\nCurrent date (UTC): {}", now.format("%A, %B %d, %Y"))
    } else {
        let now: DateTime<Local> = Local::now();
        format!("\n\nCurrent date: {}", now.format("%A, %B %d, %Y %Z"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temporal_date_context_is_cache_friendly() {
        let utc = generate_temporal_date_context(true);
        assert!(utc.contains("Current date (UTC):"), "Should include UTC date label");
        assert!(!utc.contains(" at "), "Date-only context must not carry clock time");
        // Only the label colon may remain; HH:MM:SS clock time would add more.
        assert_eq!(utc.matches(':').count(), 1, "Date-only context must not carry clock time");
        let local = generate_temporal_date_context(false);
        assert!(local.contains("Current date:"), "Should include date label");
        assert!(!local.contains(" at "), "Date-only context must not carry clock time");
        assert!(!local.contains("Current date and time"), "Must not use the per-second label");
    }
}
