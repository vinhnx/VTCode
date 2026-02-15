pub(super) const ROLE_USER: &str = "user";
#[allow(dead_code)]
pub(super) const ROLE_MODEL: &str = "model";
pub(super) const MAX_STREAMING_FAILURES: u8 = 2;
pub(super) const LOOP_THROTTLE_BASE_MS: u64 = 75;
pub(super) const LOOP_THROTTLE_MAX_MS: u64 = 500;
pub(super) const STREAMING_COOLDOWN_SECS: u64 = 60;
pub(super) const IDLE_TURN_LIMIT: usize = 3;
