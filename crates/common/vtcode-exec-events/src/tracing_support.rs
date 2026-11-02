use tracing::Level;

use super::{EVENT_SCHEMA_VERSION, EventEmitter, ThreadEvent, VersionedThreadEvent};

/// Emits structured events as `tracing` events at the specified level.
#[derive(Debug, Clone)]
pub struct TracingEmitter {
    level: Level,
}

impl TracingEmitter {
    /// Creates a new [`TracingEmitter`] with the provided [`Level`].
    pub fn new(level: Level) -> Self {
        Self { level }
    }
}

impl Default for TracingEmitter {
    fn default() -> Self {
        Self { level: Level::INFO }
    }
}

impl EventEmitter for TracingEmitter {
    fn emit(&mut self, event: &ThreadEvent) {
        match self.level {
            Level::TRACE => tracing::event!(
                target: "vtcode_exec_events",
                Level::TRACE,
                schema_version = EVENT_SCHEMA_VERSION,
                event = ?VersionedThreadEvent::new(event.clone()),
                "vtcode_exec_event"
            ),
            Level::DEBUG => tracing::event!(
                target: "vtcode_exec_events",
                Level::DEBUG,
                schema_version = EVENT_SCHEMA_VERSION,
                event = ?VersionedThreadEvent::new(event.clone()),
                "vtcode_exec_event"
            ),
            Level::INFO => tracing::event!(
                target: "vtcode_exec_events",
                Level::INFO,
                schema_version = EVENT_SCHEMA_VERSION,
                event = ?VersionedThreadEvent::new(event.clone()),
                "vtcode_exec_event"
            ),
            Level::WARN => tracing::event!(
                target: "vtcode_exec_events",
                Level::WARN,
                schema_version = EVENT_SCHEMA_VERSION,
                event = ?VersionedThreadEvent::new(event.clone()),
                "vtcode_exec_event"
            ),
            Level::ERROR => tracing::event!(
                target: "vtcode_exec_events",
                Level::ERROR,
                schema_version = EVENT_SCHEMA_VERSION,
                event = ?VersionedThreadEvent::new(event.clone()),
                "vtcode_exec_event"
            ),
        }
    }
}

pub use TracingEmitter as PublicTracingEmitter;
