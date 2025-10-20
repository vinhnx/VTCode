use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Error, Result};

use crate::{ErrorReporter, TelemetrySink, WorkspacePaths};

/// Reference implementation of [`WorkspacePaths`] backed by static [`PathBuf`]s.
///
/// This is useful for adopters who want to drive the extracted crates from an
/// existing application without wiring additional indirection layers. The
/// implementation is intentionally straightforward: callers provide the root
/// workspace directory and configuration path up front and can optionally
/// supply cache or telemetry directories.
#[derive(Debug, Clone)]
pub struct StaticWorkspacePaths {
    root: PathBuf,
    config: PathBuf,
    cache: Option<PathBuf>,
    telemetry: Option<PathBuf>,
}

impl StaticWorkspacePaths {
    /// Creates a new [`StaticWorkspacePaths`] with the required workspace and
    /// configuration directories.
    pub fn new(root: impl Into<PathBuf>, config: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            config: config.into(),
            cache: None,
            telemetry: None,
        }
    }

    /// Configures an optional cache directory used by the consumer.
    pub fn with_cache_dir(mut self, cache: impl Into<PathBuf>) -> Self {
        self.cache = Some(cache.into());
        self
    }

    /// Configures an optional telemetry directory used by the consumer.
    pub fn with_telemetry_dir(mut self, telemetry: impl Into<PathBuf>) -> Self {
        self.telemetry = Some(telemetry.into());
        self
    }
}

impl WorkspacePaths for StaticWorkspacePaths {
    fn workspace_root(&self) -> &Path {
        &self.root
    }

    fn config_dir(&self) -> PathBuf {
        self.config.clone()
    }

    fn cache_dir(&self) -> Option<PathBuf> {
        self.cache.clone()
    }

    fn telemetry_dir(&self) -> Option<PathBuf> {
        self.telemetry.clone()
    }
}

/// In-memory telemetry sink that records cloned events for later inspection.
///
/// This helper is primarily intended for tests, examples, or prototypes that
/// want to assert on the events emitted by a component without integrating a
/// full telemetry backend. The recorded events can be retrieved via
/// [`MemoryTelemetry::take`].
#[derive(Debug, Default, Clone)]
pub struct MemoryTelemetry<Event> {
    events: Arc<Mutex<Vec<Event>>>,
}

impl<Event> MemoryTelemetry<Event> {
    /// Creates a new memory-backed telemetry sink.
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns the recorded events, draining the internal buffer.
    pub fn take(&self) -> Vec<Event> {
        let mut events = self.events.lock().expect("telemetry poisoned");
        std::mem::take(&mut *events)
    }
}

impl<Event> TelemetrySink<Event> for MemoryTelemetry<Event>
where
    Event: Clone + Send,
{
    fn record(&self, event: &Event) -> Result<()> {
        let mut events = self.events.lock().expect("telemetry poisoned");
        events.push(event.clone());
        Ok(())
    }
}

/// Simple [`ErrorReporter`] that stores error messages in memory.
///
/// This helper is designed for tests and examples that need to assert on the
/// errors emitted by a component without wiring an external monitoring system.
/// Callers can retrieve captured messages via [`MemoryErrorReporter::take`].
#[derive(Debug, Default, Clone)]
pub struct MemoryErrorReporter {
    messages: Arc<Mutex<Vec<String>>>,
}

impl MemoryErrorReporter {
    /// Creates a new memory-backed error reporter.
    pub fn new() -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns the captured error messages, draining the buffer.
    pub fn take(&self) -> Vec<String> {
        let mut messages = self.messages.lock().expect("reporter poisoned");
        std::mem::take(&mut *messages)
    }
}

impl ErrorReporter for MemoryErrorReporter {
    fn capture(&self, error: &Error) -> Result<()> {
        let mut messages = self.messages.lock().expect("reporter poisoned");
        messages.push(format!("{error:?}"));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn static_paths_exposes_optional_directories() {
        let paths = StaticWorkspacePaths::new("/tmp/work", "/tmp/work/config")
            .with_cache_dir("/tmp/work/cache")
            .with_telemetry_dir("/tmp/work/telemetry");

        assert_eq!(paths.workspace_root(), Path::new("/tmp/work"));
        assert_eq!(paths.config_dir(), PathBuf::from("/tmp/work/config"));
        assert_eq!(paths.cache_dir(), Some(PathBuf::from("/tmp/work/cache")));
        assert_eq!(
            paths.telemetry_dir(),
            Some(PathBuf::from("/tmp/work/telemetry"))
        );
    }

    #[test]
    fn memory_telemetry_records_events() {
        let telemetry = MemoryTelemetry::new();
        telemetry.record(&"event-1").unwrap();
        telemetry.record(&"event-2").unwrap();

        assert_eq!(telemetry.take(), vec!["event-1", "event-2"]);
        assert!(telemetry.take().is_empty());
    }

    #[test]
    fn memory_error_reporter_captures_messages() {
        let reporter = MemoryErrorReporter::new();
        reporter.capture(&Error::msg("error-1")).unwrap();
        reporter.capture(&Error::msg("error-2")).unwrap();

        let messages = reporter.take();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].contains("error-1"));
        assert!(messages[1].contains("error-2"));
        assert!(reporter.take().is_empty());
    }
}
