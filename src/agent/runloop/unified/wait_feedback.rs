use std::time::Duration;

use vtcode_commons::stop_hints::STOP_HINT_INLINE;

pub(crate) const WAIT_KEEPALIVE_INITIAL: Duration = Duration::from_secs(5);
pub(crate) const WAIT_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

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

pub(crate) fn resolve_fractional_warning_delay(
    total_timeout: Duration,
    warning_fraction: f32,
    min_headroom: Duration,
) -> Option<Duration> {
    let fraction = warning_fraction.clamp(0.0, 0.99);
    resolve_warning_delay(total_timeout, total_timeout.mul_f32(fraction), min_headroom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keepalive_cadence_uses_five_then_ten_seconds() {
        assert_eq!(WAIT_KEEPALIVE_INITIAL, Duration::from_secs(5));
        assert_eq!(WAIT_KEEPALIVE_INTERVAL, Duration::from_secs(10));
    }

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

    #[test]
    fn fractional_warning_delay_targets_three_quarters_budget() {
        assert_eq!(
            resolve_fractional_warning_delay(
                Duration::from_secs(60),
                0.75,
                Duration::from_secs(15)
            ),
            Some(Duration::from_secs(45))
        );
    }
}
