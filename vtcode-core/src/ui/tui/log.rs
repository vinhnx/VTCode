use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use humantime::format_rfc3339_seconds;
use once_cell::sync::Lazy;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{Event, Subscriber, field::Visit};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub formatted: Arc<str>,
}

#[derive(Default)]
struct LogForwarder {
    sender: Mutex<Option<UnboundedSender<LogEntry>>>,
}

impl LogForwarder {
    fn set_sender(&self, sender: UnboundedSender<LogEntry>) {
        *self.sender.lock().unwrap() = Some(sender);
    }

    fn clear_sender(&self) {
        *self.sender.lock().unwrap() = None;
    }

    fn send(&self, entry: LogEntry) {
        if let Some(sender) = self.sender.lock().unwrap().as_ref() {
            let _ = sender.send(entry);
        }
    }
}

static LOG_FORWARDER: Lazy<Arc<LogForwarder>> = Lazy::new(|| Arc::new(LogForwarder::default()));

struct FieldVisitor {
    message: Option<String>,
    extras: Vec<(String, String)>,
}

impl Default for FieldVisitor {
    fn default() -> Self {
        Self {
            message: None,
            extras: Vec::new(),
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        let rendered = format!("{:?}", value);
        if field.name() == "message" {
            self.message = Some(rendered);
        } else {
            self.extras.push((field.name().to_string(), rendered));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.extras
                .push((field.name().to_string(), value.to_string()));
        }
    }
}

pub struct TuiLogLayer {
    forwarder: Arc<LogForwarder>,
}

impl TuiLogLayer {
    pub fn new() -> Self {
        Self {
            forwarder: LOG_FORWARDER.clone(),
        }
    }

    pub fn set_sender(sender: UnboundedSender<LogEntry>) {
        LOG_FORWARDER.set_sender(sender);
    }

    pub fn clear_sender() {
        LOG_FORWARDER.clear_sender();
    }
}

impl<S> Layer<S> for TuiLogLayer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() < tracing::Level::INFO {
            return;
        }

        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);

        let message = visitor
            .message
            .unwrap_or_else(|| "(no message)".to_string());
        let extras = if visitor.extras.is_empty() {
            String::new()
        } else {
            let rendered = visitor
                .extras
                .into_iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(" ");
            format!(" {rendered}")
        };

        let timestamp = format_rfc3339_seconds(SystemTime::now());
        let line = format!(
            "{} {:<5} {} {}{}",
            timestamp,
            event.metadata().level(),
            event.metadata().target(),
            message,
            extras
        );

        self.forwarder.send(LogEntry {
            formatted: Arc::from(line.into_boxed_str()),
        });
    }
}

pub fn make_tui_log_layer() -> TuiLogLayer {
    TuiLogLayer::new()
}

pub fn register_tui_log_sender(sender: UnboundedSender<LogEntry>) {
    TuiLogLayer::set_sender(sender);
}

pub fn clear_tui_log_sender() {
    TuiLogLayer::clear_sender();
}
