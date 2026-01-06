//! OpenAI streaming telemetry and utilities.
//!
//! This module contains stream processing helpers for OpenAI API responses.

#[cfg(debug_assertions)]
use tracing::debug;

use super::super::shared::StreamTelemetry;

/// Telemetry implementation for OpenAI streaming responses.
#[derive(Default)]
pub struct OpenAIStreamTelemetry;

impl StreamTelemetry for OpenAIStreamTelemetry {
    #[cfg_attr(not(debug_assertions), allow(unused_variables))]
    fn on_content_delta(&self, delta: &str) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openai::stream",
            length = delta.len(),
            "content delta received"
        );
    }

    #[cfg_attr(not(debug_assertions), allow(unused_variables))]
    fn on_reasoning_delta(&self, delta: &str) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openai::stream",
            length = delta.len(),
            "reasoning delta received"
        );
    }

    fn on_tool_call_delta(&self) {
        #[cfg(debug_assertions)]
        debug!(
            target = "vtcode::llm::openai::stream",
            "tool call delta received"
        );
    }
}
