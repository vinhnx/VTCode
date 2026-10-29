use log::Level;

use super::{EventEmitter, ThreadEvent, json};

/// Emits JSON serialized events to the `log` facade at the configured level.
#[derive(Debug, Clone)]
pub struct LogEmitter {
    level: Level,
}

impl LogEmitter {
    /// Creates a new [`LogEmitter`] that logs at the provided [`Level`].
    pub fn new(level: Level) -> Self {
        Self { level }
    }
}

impl Default for LogEmitter {
    fn default() -> Self {
        Self { level: Level::Info }
    }
}

impl EventEmitter for LogEmitter {
    fn emit(&mut self, event: &ThreadEvent) {
        if log::log_enabled!(self.level) {
            match json::to_string(event) {
                Ok(serialized) => log::log!(self.level, "{serialized}"),
                Err(err) => log::log!(self.level, "failed to serialize vtcode exec event for logging: {err}"),
            }
        }
    }
}

pub use LogEmitter as PublicLogEmitter;
