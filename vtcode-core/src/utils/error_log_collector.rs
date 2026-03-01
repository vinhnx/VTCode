//! Global error log collector for session diagnostics.
//!
//! Captures ERROR-level tracing events into a thread-safe buffer so they can be
//! appended to the session archive JSON at finalization time.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Mutex;
use tracing::{Event, Level, Subscriber, field::Visit};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

/// A single captured error log entry persisted to the session archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorLogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// Global buffer of captured error log entries.
static ERROR_LOG_BUFFER: Mutex<Vec<ErrorLogEntry>> = Mutex::new(Vec::new());

/// Drain all collected error log entries, clearing the buffer.
pub fn drain_error_logs() -> Vec<ErrorLogEntry> {
    match ERROR_LOG_BUFFER.lock() {
        Ok(mut buf) => std::mem::take(&mut *buf),
        Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
    }
}

/// A tracing layer that silently collects ERROR-level events into the global buffer.
pub struct ErrorLogCollectorLayer;

#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }
}

impl<S> Layer<S> for ErrorLogCollectorLayer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();
        if level != Level::ERROR {
            return;
        }

        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let entry = ErrorLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            level: level.to_string(),
            target: event.metadata().target().to_string(),
            message: visitor
                .message
                .unwrap_or_else(|| "(no message)".to_string()),
        };

        if let Ok(mut buf) = ERROR_LOG_BUFFER.lock() {
            buf.push(entry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_returns_empty_by_default() {
        let entries = drain_error_logs();
        assert!(entries.is_empty());
    }
}
