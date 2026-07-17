use anyhow::Result;

/// A lightweight sink used to record telemetry events emitted by extracted
/// components. The `Event` type is intentionally generic so downstream
/// consumers can supply their own event schema without depending on
/// `vtcode-core` internals.
pub trait TelemetrySink<Event>: Send + Sync {
    /// Record an event produced by the component.
    fn record(&self, event: &Event) -> Result<()>;

    /// Flush any buffered telemetry data to its destination.
    fn flush(&self) -> Result<()> {
        Ok(())
    }
}

/// A telemetry sink that ignores all events. Useful for tests or for consumers
/// who do not need telemetry integration yet.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopTelemetry;

impl<Event> TelemetrySink<Event> for NoopTelemetry {
    fn record(&self, _event: &Event) -> Result<()> {
        Ok(())
    }
}
