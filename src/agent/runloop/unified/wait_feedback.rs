use std::time::Duration;

use vtcode_commons::stop_hints::STOP_HINT_INLINE;

pub(crate) const WAIT_KEEPALIVE_INITIAL: Duration = Duration::from_secs(10);
pub(crate) const WAIT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);

pub(crate) fn wait_keepalive_message(subject: &str, elapsed: Duration) -> String {
    format!(
        "{subject} is still running after {}s. {}.",
        elapsed.as_secs(),
        STOP_HINT_INLINE
    )
}

pub(crate) fn wait_timeout_warning_message(
    subject: &str,
    timeout: Duration,
    remaining: Duration,
) -> String {
    format!(
        "{subject} is nearing the {}s timeout ({}s remaining). {}.",
        timeout.as_secs(),
        remaining.as_secs(),
        STOP_HINT_INLINE
    )
}

pub(crate) fn resolve_warning_delay(
    total_timeout: Duration,
    desired_delay: Duration,
    min_headroom: Duration,
) -> Option<Duration> {
    if total_timeout.is_zero() {
        return None;
    }

    let warning_delay = desired_delay.min(total_timeout.saturating_sub(min_headroom));
    (!warning_delay.is_zero()).then_some(warning_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warning_delay_prefers_earlier_desired_delay() {
        assert_eq!(
            resolve_warning_delay(
                Duration::from_secs(60),
                Duration::from_secs(12),
                Duration::from_secs(5)
            ),
            Some(Duration::from_secs(12))
        );
    }

    #[test]
    fn warning_delay_caps_at_timeout_headroom() {
        assert_eq!(
            resolve_warning_delay(
                Duration::from_secs(60),
                Duration::from_secs(58),
                Duration::from_secs(10)
            ),
            Some(Duration::from_secs(50))
        );
    }

    #[test]
    fn warning_delay_skips_zero_delay() {
        assert_eq!(
            resolve_warning_delay(
                Duration::from_secs(5),
                Duration::from_secs(5),
                Duration::from_secs(5)
            ),
            None
        );
    }
}
