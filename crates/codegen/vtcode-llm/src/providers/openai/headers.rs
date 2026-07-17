use reqwest::RequestBuilder;
use serde_json::Value;

pub(crate) fn apply_json_content_type(builder: RequestBuilder) -> RequestBuilder {
    builder.header("Content-Type", "application/json")
}

pub(crate) fn apply_responses_beta(builder: RequestBuilder) -> RequestBuilder {
    builder.header("OpenAI-Beta", "responses=v1")
}

/// Apply turn metadata header if metadata is present in the request.
/// This header provides git context (remote URLs, commit hash) to the provider.
pub(crate) fn apply_turn_metadata(
    builder: RequestBuilder,
    metadata: &Option<Value>,
) -> RequestBuilder {
    if let Some(metadata) = metadata
        && let Ok(metadata_str) = serde_json::to_string(metadata)
    {
        return builder.header("X-Turn-Metadata", metadata_str);
    }
    builder
}

pub(crate) fn apply_client_request_id(
    builder: RequestBuilder,
    client_request_id: &str,
) -> RequestBuilder {
    if client_request_id.trim().is_empty() {
        return builder;
    }
    builder.header("X-Client-Request-Id", client_request_id)
}
