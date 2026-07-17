use std::time::Duration;

use vtcode_commons::llm::LLMErrorMetadata;

pub(crate) fn parse_retry_after_header(raw: &str) -> Option<Duration> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    if let Ok(seconds) = raw.parse::<f64>() {
        return Duration::try_from_secs_f64(seconds).ok();
    }
    None
}

pub(crate) fn retry_after_from_llm_metadata(metadata: &LLMErrorMetadata) -> Option<Duration> {
    parse_retry_after_header(metadata.retry_after.as_deref()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_header_accepts_integer_seconds() {
        assert_eq!(
            parse_retry_after_header(" 7 "),
            Some(Duration::from_secs(7))
        );
    }

    #[test]
    fn retry_after_header_accepts_fractional_seconds() {
        assert_eq!(
            parse_retry_after_header("0.5"),
            Some(Duration::from_millis(500))
        );
    }

    #[test]
    fn retry_after_header_rejects_empty_or_invalid_values() {
        assert_eq!(parse_retry_after_header(""), None);
        assert_eq!(parse_retry_after_header("soon"), None);
        assert_eq!(parse_retry_after_header("-1"), None);
        assert_eq!(parse_retry_after_header("inf"), None);
        assert_eq!(parse_retry_after_header("1e20"), None);
    }
}
