//! Context-window formatting shared by palette and appearance surfaces.

pub(super) fn format_context_window(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format_compact_count(tokens, 1_000_000, "M")
    } else if tokens >= 1_000 {
        format_compact_count(tokens, 1_000, "K")
    } else {
        tokens.to_string()
    }
}

fn format_compact_count(value: u64, divisor: u64, suffix: &str) -> String {
    let whole = value / divisor;
    let hundredths = (value % divisor) * 100 / divisor;
    if hundredths == 0 {
        format!("{whole}{suffix}")
    } else if hundredths.is_multiple_of(10) {
        format!("{whole}.{}{suffix}", hundredths / 10)
    } else {
        format!("{whole}.{hundredths:02}{suffix}")
    }
}

/// Approximate context utilization after a turn: every token the provider
/// processed, as a share of the active model's context window. Returns
/// `None` while either side is unknown, so the status bar hides the field
/// rather than inventing a number.
pub(super) fn context_usage_percent(usage: &vtcode_exec_events::Usage, context_window: u64) -> Option<u8> {
    if context_window == 0 {
        return None;
    }
    let tokens = usage
        .input_tokens
        .saturating_add(usage.cached_input_tokens)
        .saturating_add(usage.cache_creation_tokens)
        .saturating_add(usage.output_tokens);
    let percent = (tokens.saturating_mul(100)).checked_div(context_window)?;
    u8::try_from(percent).ok().or(Some(100))
}
