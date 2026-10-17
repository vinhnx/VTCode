//! Dual emitter bridging ThreadEvent and normalized streams.

use super::*;

/// Adapter that wraps a VT Code event sink and also emits Open Responses events.
pub struct DualEventEmitter<E: StreamEventEmitter> {
    open_responses_emitter: E,
    builder: ResponseBuilder,
}

impl<E: StreamEventEmitter> DualEventEmitter<E> {
    /// Creates a new dual emitter with the given Open Responses emitter and model.
    pub fn new(emitter: E, model: impl Into<String>) -> Self {
        Self {
            open_responses_emitter: emitter,
            builder: ResponseBuilder::new(model),
        }
    }

    /// Processes a VT Code event and emits corresponding Open Responses events.
    pub fn process(&mut self, event: &ThreadEvent) {
        self.builder.process_event(event, &mut self.open_responses_emitter);
    }

    /// Processes a normalized provider stream event and emits corresponding Open Responses events.
    pub fn process_normalized(&mut self, event: &NormalizedStreamEvent) {
        self.builder.process_normalized_event(event, &mut self.open_responses_emitter);
    }

    /// Returns a reference to the current response.
    pub fn response(&self) -> &Response {
        self.builder.response()
    }

    /// Returns the underlying Open Responses emitter.
    pub fn into_emitter(self) -> E {
        self.open_responses_emitter
    }

    /// Consumes the adapter and returns the final response.
    pub fn into_response(self) -> Response {
        self.builder.build()
    }
}
