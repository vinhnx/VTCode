//! Shared rate-limit configuration parsing for tool execution paths.

use std::env;

const TOOL_CALLS_PER_MIN_ENV: &str = "VTCODE_TOOL_CALLS_PER_MIN";
const TOOL_RATE_LIMIT_PER_SECOND_ENV: &str = "VTCODE_TOOL_RATE_LIMIT_PER_SECOND";

pub(crate) fn tool_calls_per_second_from_env() -> Option<usize> {
    positive_rate_limit_from_env(TOOL_RATE_LIMIT_PER_SECOND_ENV)
}

pub(crate) fn tool_calls_per_minute_from_env() -> Option<usize> {
    positive_rate_limit_from_env(TOOL_CALLS_PER_MIN_ENV)
}

pub(crate) fn positive_rate_limit_from_env(name: &str) -> Option<usize> {
    env::var(name)
        .ok()
        .and_then(|raw| parse_positive_rate_limit(&raw))
}

pub(crate) fn parse_positive_rate_limit(raw: &str) -> Option<usize> {
    raw.trim().parse::<usize>().ok().filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::parse_positive_rate_limit;

    #[test]
    fn parse_positive_rate_limit_accepts_positive_values() {
        assert_eq!(parse_positive_rate_limit(" 60 "), Some(60));
    }

    #[test]
    fn parse_positive_rate_limit_rejects_zero_or_invalid_values() {
        assert_eq!(parse_positive_rate_limit("0"), None);
        assert_eq!(parse_positive_rate_limit("many"), None);
    }
}
