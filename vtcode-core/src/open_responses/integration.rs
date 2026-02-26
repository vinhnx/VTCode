//! Integration layer for Open Responses with VT Code agent infrastructure.
//!
//! This module provides the glue between VT Code's internal event system
//! and the Open Responses specification. It uses configuration from
//! `vtcode.toml` to control when and how Open Responses events are emitted.

use std::sync::{Arc, Mutex};

use vtcode_config::OpenResponsesConfig;
use vtcode_exec_events::ThreadEvent;

use super::{
    OpenUsage, OutputItem, Response, ResponseBuilder, ResponseStreamEvent, VecStreamEmitter,
};

/// Callback type for receiving Open Responses streaming events.
pub type OpenResponsesCallback = Arc<Mutex<Box<dyn FnMut(ResponseStreamEvent) + Send>>>;

/// Open Responses integration manager.
///
/// This struct manages the integration between VT Code's internal event system
/// and the Open Responses specification. It respects the configuration flags
/// to control event emission and item mapping.
pub struct OpenResponsesIntegration {
    config: OpenResponsesConfig,
    builder: Option<ResponseBuilder>,
    events: Vec<ResponseStreamEvent>,
    callback: Option<OpenResponsesCallback>,
}

impl std::fmt::Debug for OpenResponsesIntegration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenResponsesIntegration")
            .field("config", &self.config)
            .field("builder", &self.builder)
            .field("events_count", &self.events.len())
            .field("callback_set", &self.callback.is_some())
            .finish()
    }
}

impl OpenResponsesIntegration {
    /// Creates a new integration manager with the given configuration.
    pub fn new(config: OpenResponsesConfig) -> Self {
        Self {
            config,
            builder: None,
            events: Vec::new(),
            callback: None,
        }
    }

    /// Creates a new integration manager that is disabled.
    pub fn disabled() -> Self {
        Self::new(OpenResponsesConfig::default())
    }

    /// Returns true if Open Responses integration is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Sets a callback for receiving Open Responses events.
    pub fn set_callback(&mut self, callback: OpenResponsesCallback) {
        self.callback = Some(callback);
    }

    /// Starts a new response session with the given model.
    ///
    /// This should be called when a new agent turn begins.
    pub fn start_response(&mut self, model: &str) {
        if !self.config.enabled {
            return;
        }

        self.builder = Some(ResponseBuilder::new(model));
        self.events.clear();
    }

    /// Processes a VT Code ThreadEvent and emits corresponding Open Responses events.
    pub fn process_event(&mut self, event: &ThreadEvent) {
        if !self.config.enabled || !self.config.emit_events {
            return;
        }

        let Some(builder) = self.builder.as_mut() else {
            return;
        };

        // Use a collecting emitter first
        let mut collector = VecStreamEmitter::new();
        builder.process_event(event, &mut collector);

        // Process collected events
        for stream_event in collector.into_events() {
            // Apply filtering based on config
            if self.should_emit_event(&stream_event) {
                self.emit_event(stream_event);
            }
        }
    }

    /// Returns the current response, if any.
    pub fn current_response(&self) -> Option<&Response> {
        self.builder.as_ref().map(|b| b.response())
    }

    /// Finishes the current response and returns it.
    pub fn finish_response(&mut self) -> Option<Response> {
        self.builder.take().map(|b| b.build())
    }

    /// Returns all collected events.
    pub fn events(&self) -> &[ResponseStreamEvent] {
        &self.events
    }

    /// Takes all collected events, leaving the internal buffer empty.
    pub fn take_events(&mut self) -> Vec<ResponseStreamEvent> {
        std::mem::take(&mut self.events)
    }

    fn should_emit_event(&self, event: &ResponseStreamEvent) -> bool {
        match event {
            // Always emit response lifecycle events
            ResponseStreamEvent::ResponseCreated { .. }
            | ResponseStreamEvent::ResponseInProgress { .. }
            | ResponseStreamEvent::ResponseCompleted { .. }
            | ResponseStreamEvent::ResponseFailed { .. }
            | ResponseStreamEvent::ResponseIncomplete { .. } => true,

            // Filter output items based on config
            ResponseStreamEvent::OutputItemAdded { item, .. }
            | ResponseStreamEvent::OutputItemDone { item, .. } => self.should_include_item(item),

            // Reasoning events
            ResponseStreamEvent::ReasoningDelta { .. }
            | ResponseStreamEvent::ReasoningDone { .. } => self.config.include_reasoning,

            // Function call events
            ResponseStreamEvent::FunctionCallArgumentsDelta { .. }
            | ResponseStreamEvent::FunctionCallArgumentsDone { .. } => self.config.map_tool_calls,

            // Extension events
            ResponseStreamEvent::CustomEvent { .. } => self.config.include_extensions,

            // Text and content events are always emitted
            _ => true,
        }
    }

    fn should_include_item(&self, item: &OutputItem) -> bool {
        match item {
            OutputItem::Reasoning(_) => self.config.include_reasoning,
            OutputItem::FunctionCall(_) | OutputItem::FunctionCallOutput(_) => {
                self.config.map_tool_calls
            }
            OutputItem::Custom(_) => self.config.include_extensions,
            OutputItem::Message(_) => true,
        }
    }

    fn emit_event(&mut self, event: ResponseStreamEvent) {
        // Store in local buffer
        self.events.push(event.clone());

        // Send to callback if registered
        if let Some(callback) = &self.callback
            && let Ok(mut cb) = callback.lock()
        {
            cb(event);
        }
    }
}

impl Default for OpenResponsesIntegration {
    fn default() -> Self {
        Self::disabled()
    }
}

/// Trait for types that can provide Open Responses integration.
pub trait OpenResponsesProvider {
    /// Returns a reference to the Open Responses integration, if enabled.
    fn open_responses(&self) -> Option<&OpenResponsesIntegration>;

    /// Returns a mutable reference to the Open Responses integration, if enabled.
    fn open_responses_mut(&mut self) -> Option<&mut OpenResponsesIntegration>;
}

/// Extension trait for converting VT Code LLM responses to Open Responses format.
pub trait ToOpenResponse {
    /// Converts to an Open Responses Response object.
    fn to_open_response(&self, response_id: &str, model: &str) -> Response;
}

impl ToOpenResponse for crate::llm::provider::LLMResponse {
    fn to_open_response(&self, response_id: &str, model: &str) -> Response {
        let mut response = Response::new(response_id, model);

        // Add usage if available
        if let Some(usage) = &self.usage {
            response.usage = Some(OpenUsage::from_llm_usage(usage));
        }

        // Add content as message item if present
        if let Some(content) = &self.content
            && !content.is_empty()
        {
            let item = OutputItem::completed_message(
                super::response::generate_item_id(),
                super::items::MessageRole::Assistant,
                vec![super::ContentPart::output_text(content)],
            );
            response.add_output(item);
        }

        // Add reasoning if present
        if let Some(reasoning) = &self.reasoning
            && !reasoning.is_empty()
        {
            let item = OutputItem::Reasoning(super::items::ReasoningItem {
                id: super::response::generate_item_id(),
                status: super::ItemStatus::Completed,
                summary: None,
                content: Some(reasoning.clone()),
                encrypted_content: None,
            });
            response.add_output(item);
        }

        // Add tool calls as function call items
        if let Some(tool_calls) = &self.tool_calls {
            for tc in tool_calls {
                // Extract name and arguments from the function field
                let (name, arguments) = if let Some(ref func) = tc.function {
                    (
                        func.name.clone(),
                        serde_json::from_str(&func.arguments).unwrap_or(serde_json::json!({})),
                    )
                } else {
                    (tc.call_type.clone(), serde_json::json!({}))
                };

                let item = OutputItem::FunctionCall(super::items::FunctionCallItem {
                    id: tc.id.clone(),
                    status: super::ItemStatus::Completed,
                    name,
                    arguments,
                    call_id: Some(tc.id.clone()),
                });
                response.add_output(item);
            }
        }

        response.complete();
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integration_disabled_by_default() {
        let integration = OpenResponsesIntegration::disabled();
        assert!(!integration.is_enabled());
    }

    #[test]
    fn test_integration_enabled() {
        let config = OpenResponsesConfig {
            enabled: true,
            ..Default::default()
        };
        let integration = OpenResponsesIntegration::new(config);
        assert!(integration.is_enabled());
    }

    #[test]
    fn test_start_response() {
        let config = OpenResponsesConfig {
            enabled: true,
            ..Default::default()
        };
        let mut integration = OpenResponsesIntegration::new(config);
        integration.start_response("gpt-5");
        assert!(integration.current_response().is_some());
    }

    #[test]
    fn test_disabled_skips_events() {
        let mut integration = OpenResponsesIntegration::disabled();
        integration.start_response("gpt-5");
        // Should not create a builder when disabled
        assert!(integration.current_response().is_none());
    }
}
