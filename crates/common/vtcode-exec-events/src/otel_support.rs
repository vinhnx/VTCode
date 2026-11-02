use opentelemetry::KeyValue;
use opentelemetry::trace::{Span, Status, Tracer};

use super::{EventEmitter, ThreadEvent, ThreadItemDetails};

/// Emits [`ThreadEvent`]s as OpenTelemetry spans and span events.
///
/// Each `ThreadEvent` is recorded as an OTel span with attributes derived
/// from the event payload.  Harness events are attached as span events
/// with their own attributes (event kind, message, path, etc.).
///
/// # Usage
///
/// ```rust,ignore
/// // Requires concrete SDK type (e.g. opentelemetry_sdk::trace::SdkTracerProvider)
/// # use vtcode_exec_events::OtelEmitter;
/// # let tracer = opentelemetry_sdk::trace::SdkTracerProvider::default()
/// #     .tracer("vtcode");
/// # let mut emitter = OtelEmitter::new(tracer);
/// ```
pub struct OtelEmitter<T: Tracer> {
    tracer: T,
}

impl<T: Tracer> OtelEmitter<T> {
    pub fn new(tracer: T) -> Self {
        Self { tracer }
    }
}

impl<T: Tracer> EventEmitter for OtelEmitter<T> {
    fn emit(&mut self, event: &ThreadEvent) {
        let span_name = match event {
            ThreadEvent::ThreadStarted(_) => "thread.started",
            ThreadEvent::ThreadCompleted(_) => "thread.completed",
            ThreadEvent::ContextReset(_) => "context.reset",
            ThreadEvent::TurnStarted(_) => "turn.started",
            ThreadEvent::TurnCompleted(_) => "turn.completed",
            ThreadEvent::TurnFailed(_) => "turn.failed",
            ThreadEvent::ItemStarted(_) => "item.started",
            ThreadEvent::ItemUpdated(_) => "item.updated",
            ThreadEvent::ItemCompleted(_) => "item.completed",
            ThreadEvent::Error(_) => "error",
            _ => "event",
        };

        let mut span = self.tracer.start(span_name);

        match event {
            ThreadEvent::ThreadStarted(e) => {
                span.set_attribute(KeyValue::new("thread_id", e.thread_id.clone()));
            }
            ThreadEvent::ThreadCompleted(e) => {
                if let Some(ref cost) = e.total_cost_usd {
                    span.set_attribute(KeyValue::new("total_cost_usd", cost.as_f64().unwrap_or(0.0)));
                }
                span.set_attribute(KeyValue::new(
                    "input_tokens",
                    i64::try_from(e.usage.input_tokens).unwrap_or(i64::MAX),
                ));
                span.set_attribute(KeyValue::new(
                    "output_tokens",
                    i64::try_from(e.usage.output_tokens).unwrap_or(i64::MAX),
                ));
                span.set_attribute(KeyValue::new("completion_subtype", e.subtype.as_str().to_string()));
            }
            ThreadEvent::ContextReset(e) => {
                span.set_attribute(KeyValue::new("thread_id", e.thread_id.clone()));
                span.set_attribute(KeyValue::new("turn_id", e.turn_id.clone()));
                span.set_attribute(KeyValue::new("plan_preserved", e.plan_preserved));
                span.set_attribute(KeyValue::new(
                    "previous_context_usage_percent",
                    e.previous_context_usage_percent as i64,
                ));
                span.set_attribute(KeyValue::new("tool_budget_reset", e.tool_budget_reset));
            }
            ThreadEvent::TurnCompleted(e) => {
                span.set_attribute(KeyValue::new(
                    "turn_input_tokens",
                    i64::try_from(e.usage.input_tokens).unwrap_or(i64::MAX),
                ));
                span.set_attribute(KeyValue::new(
                    "turn_output_tokens",
                    i64::try_from(e.usage.output_tokens).unwrap_or(i64::MAX),
                ));
            }
            ThreadEvent::ItemCompleted(e) => {
                if let ThreadItemDetails::Harness(harness) = &e.item.details {
                    span.set_attribute(KeyValue::new("harness_event", format!("{:?}", harness.event)));
                    if let Some(ref msg) = harness.message {
                        span.set_attribute(KeyValue::new("harness_message", msg.clone()));
                    }
                    if let Some(ref path) = harness.path {
                        span.set_attribute(KeyValue::new("harness_path", path.clone()));
                    }
                    if let Some(dur) = harness.duration_ms {
                        span.set_attribute(KeyValue::new("duration_ms", i64::try_from(dur).unwrap_or(i64::MAX)));
                    }
                    let mut event_attrs = vec![KeyValue::new("event_kind", format!("{:?}", harness.event))];
                    if let Some(ref msg) = harness.message {
                        event_attrs.push(KeyValue::new("message", msg.clone()));
                    }
                    span.add_event("harness_event", event_attrs);
                }
            }
            ThreadEvent::Error(e) => {
                span.set_status(Status::Error { description: e.message.clone().into() });
                span.set_attribute(KeyValue::new("error_message", e.message.clone()));
            }
            _ => {}
        }

        span.end();
    }
}

pub use OtelEmitter as PublicOtelEmitter;
