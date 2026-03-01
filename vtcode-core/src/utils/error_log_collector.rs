//! Global error log collector for session diagnostics.
//!
//! Captures ERROR-level tracing events into a thread-safe buffer so they can be
//! appended to the session archive JSON at finalization time.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::sync::Mutex;
use tracing::{Event, Level, Subscriber, field::Visit};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::config::constants::output_limits::ERROR_LOG_BUFFER_SIZE_LIMIT_BYTES;

/// A single captured error log entry persisted to the session archive.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorLogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

#[derive(Default)]
struct ErrorLogBuffer {
    entries: VecDeque<ErrorLogEntry>,
    total_estimated_bytes: usize,
}

/// Global buffer of captured error log entries.
static ERROR_LOG_BUFFER: Mutex<ErrorLogBuffer> = Mutex::new(ErrorLogBuffer {
    entries: VecDeque::new(),
    total_estimated_bytes: 0,
});

fn with_error_log_buffer<R>(f: impl FnOnce(&mut ErrorLogBuffer) -> R) -> R {
    match ERROR_LOG_BUFFER.lock() {
        Ok(mut buffer) => f(&mut buffer),
        Err(poisoned) => {
            let mut buffer = poisoned.into_inner();
            f(&mut buffer)
        }
    }
}

fn estimate_entry_bytes(entry: &ErrorLogEntry) -> usize {
    entry.level.len() + entry.target.len() + entry.message.len()
}

fn push_entry_with_limit(buffer: &mut ErrorLogBuffer, entry: ErrorLogEntry, limit_bytes: usize) {
    buffer.total_estimated_bytes = buffer
        .total_estimated_bytes
        .saturating_add(estimate_entry_bytes(&entry));
    buffer.entries.push_back(entry);
    if buffer.total_estimated_bytes <= limit_bytes {
        return;
    }

    while buffer.total_estimated_bytes > limit_bytes {
        let Some(removed) = buffer.entries.pop_front() else {
            buffer.total_estimated_bytes = 0;
            break;
        };
        buffer.total_estimated_bytes = buffer
            .total_estimated_bytes
            .saturating_sub(estimate_entry_bytes(&removed));
    }
}

fn take_buffer_entries(buffer: &mut ErrorLogBuffer) -> Vec<ErrorLogEntry> {
    let drained = buffer.entries.drain(..).collect();
    buffer.total_estimated_bytes = 0;
    drained
}

/// Drain all collected error log entries, clearing the buffer.
pub fn drain_error_logs() -> Vec<ErrorLogEntry> {
    with_error_log_buffer(take_buffer_entries)
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

        with_error_log_buffer(|buffer| {
            push_entry_with_limit(buffer, entry, ERROR_LOG_BUFFER_SIZE_LIMIT_BYTES);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(message: &str) -> ErrorLogEntry {
        ErrorLogEntry {
            timestamp: Utc::now().to_rfc3339(),
            level: "ERROR".to_string(),
            target: "vtcode_test".to_string(),
            message: message.to_string(),
        }
    }

    #[test]
    fn drain_returns_empty_by_default() {
        let entries = drain_error_logs();
        assert!(entries.is_empty());
    }

    #[test]
    fn prunes_oldest_entries_when_over_limit() {
        let mut buffer = ErrorLogBuffer::default();
        let first = make_entry("aaaaa");
        let second = make_entry("bbbbb");
        let third = make_entry("ccccc");
        let limit = estimate_entry_bytes(&first) + estimate_entry_bytes(&second);

        push_entry_with_limit(&mut buffer, first, limit);
        push_entry_with_limit(&mut buffer, second, limit);
        push_entry_with_limit(&mut buffer, third, limit);

        let retained: Vec<&str> = buffer
            .entries
            .iter()
            .map(|entry| entry.message.as_str())
            .collect();
        assert_eq!(retained, vec!["bbbbb", "ccccc"]);
        assert!(buffer.total_estimated_bytes <= limit);
    }

    #[test]
    fn drops_single_oversized_entry() {
        let mut buffer = ErrorLogBuffer::default();
        push_entry_with_limit(&mut buffer, make_entry("oversized entry"), 4);

        assert!(buffer.entries.is_empty());
        assert_eq!(buffer.total_estimated_bytes, 0);
    }
}
