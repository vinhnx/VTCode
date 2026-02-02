use super::super::{
    ReasoningBuffer, extract_reasoning_trace,
    shared::{
        StreamDelta, StreamTelemetry, ToolCallBuilder, append_text_with_reasoning,
        apply_tool_call_delta_from_content, update_tool_calls,
    },
};

#[cfg(debug_assertions)]
use tracing::debug;
