use vtcode_core::core::agent::events::{EventSink, event_sink};
use vtcode_core::core::agent::runtime::StreamingLifecycleBridge as CoreStreamingLifecycleBridge;
use vtcode_core::exec::events::ThreadEvent;

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::ui_interaction::StreamProgressEvent;
use std::time::Instant;
use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase};

/// Monotonic, content-free observations. Hidden reasoning is provider activity,
/// while OutputDelta only arrives after sanitization and visible rendering.
pub(super) struct ResponseLatency {
    operation: Option<ProgressOperation>,
    dispatched_at: Instant,
    step: usize,
    attempt: usize,
    first_event: bool,
    first_visible: bool,
}

impl ResponseLatency {
    pub(super) fn new(
        operation: Option<ProgressOperation>,
        dispatched_at: Instant,
        step: usize,
        attempt: usize,
    ) -> Self {
        Self {
            operation,
            dispatched_at,
            step,
            attempt,
            first_event: false,
            first_visible: false,
        }
    }

    pub(super) fn observe(&mut self, event: &StreamProgressEvent) -> ProgressPhase {
        if !self.first_event {
            self.first_event = true;
            tracing::debug!(target: "vtcode.response_latency", operation_id = self.operation.map(|op| op.id()),
                step = self.step, attempt = self.attempt,
                dispatch_to_first_event_ms = self.dispatched_at.elapsed().as_secs_f64() * 1000.0,
                "first provider event");
        }
        let visible = matches!(event, StreamProgressEvent::OutputDelta(delta) if !delta.trim().is_empty());
        if visible && !self.first_visible {
            self.first_visible = true;
            tracing::debug!(target: "vtcode.response_latency", operation_id = self.operation.map(|op| op.id()),
                step = self.step, attempt = self.attempt,
                dispatch_to_visible_output_ms = self.dispatched_at.elapsed().as_secs_f64() * 1000.0,
                accepted_to_visible_output_ms = self.operation.map(|op| op.started_at().elapsed().as_secs_f64() * 1000.0),
                "first visible model output");
        }
        // Runtime requests can change the displayed phase between model events.
        // The UI deduplicates against its current state, not this observer's history.
        if self.first_visible {
            ProgressPhase::ReceivingResponse
        } else {
            ProgressPhase::Processing
        }
    }
}

pub(super) struct HarnessStreamingBridge {
    inner: CoreStreamingLifecycleBridge,
    assistant_output_observed: bool,
}

impl HarnessStreamingBridge {
    pub(super) fn new(emitter: Option<&HarnessEventEmitter>, turn_id: &str, step: usize, attempt: usize) -> Self {
        let event_sink = emitter.cloned().map(harness_event_sink);
        Self {
            inner: CoreStreamingLifecycleBridge::new(event_sink, turn_id, step, attempt),
            assistant_output_observed: false,
        }
    }

    pub(super) fn on_progress(&mut self, event: StreamProgressEvent) {
        if matches!(&event, StreamProgressEvent::OutputDelta(delta) if !delta.trim().is_empty()) {
            self.assistant_output_observed = true;
        }
        self.inner.on_progress(event);
    }

    pub(super) fn abort(&mut self) {
        self.inner.abort();
    }

    pub(super) fn complete_open_items(&mut self) {
        self.inner.complete_open_items();
    }

    pub(super) fn take_streamed_tool_call_items(&mut self) -> hashbrown::HashMap<String, (String, String)> {
        self.inner.take_streamed_tool_call_items()
    }

    pub(super) fn assistant_output_observed(&self) -> bool {
        self.assistant_output_observed
    }
}

fn harness_event_sink(emitter: HarnessEventEmitter) -> EventSink {
    event_sink(move |event: &ThreadEvent| {
        let _ = emitter.emit(event.clone());
    })
}

#[cfg(test)]
mod tests;
