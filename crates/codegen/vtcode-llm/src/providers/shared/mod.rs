//! Shared provider wire helpers with stable facade imports.

mod prompt_cache;
mod responses_adapter;
mod responses_stream;
mod sse;
mod stream;
mod tag_sanitizer;
mod tool_output;
mod usage;
mod utf8;

pub use crate::providers::ReasoningBuffer;
pub(crate) use prompt_cache::{session_lineage_from_prompt_cache_key, split_dynamic_prompt_suffix};
pub(crate) use responses_stream::{
    RESPONSES_COMPLETION_TOKEN_KEYS, RESPONSES_PROMPT_TOKEN_KEYS, ResponsesStreamEventPolicy,
    response_stream_event_policy,
};
pub(crate) use responses_stream::{
    ResponsesNormalizedStreamOptions, ResponsesNormalizedStreamProcessor, create_responses_normalized_stream,
};
pub(crate) use sse::{drain_consumed_sse, extract_data_payload, find_sse_boundary_bytes, next_sse_event};
pub use stream::{
    NoopStreamTelemetry, OpenAiDeltaOrder, StreamAssemblyError, StreamDelta, StreamFragment, StreamTelemetry,
    ToolCallBuilder,
};
pub(crate) use stream::{
    StreamAggregator, collect_tool_references_from_tool_search_output, generate_tool_call_id,
    handle_openai_compatible_chunk, parse_openai_tool_calls, process_openai_stream,
};
pub use tag_sanitizer::TagStreamSanitizer;
pub(crate) use tool_output::{
    function_output_value_from_message_content, parse_compacted_output_messages,
    tool_result_content_from_message_content,
};
pub(crate) use usage::{
    parse_cache_write_tokens_from_usage, parse_cached_prompt_tokens_from_usage, usage_u32_from_keys,
};
pub use utf8::Utf8StreamDecoder;
