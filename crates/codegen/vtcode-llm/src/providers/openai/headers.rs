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
pub(crate) fn apply_turn_metadata(builder: RequestBuilder, metadata: Option<&Value>) -> RequestBuilder {
    if let Some(metadata) = metadata
        && let Ok(metadata_str) = serde_json::to_string(metadata)
    {
        return builder.header("X-Turn-Metadata", metadata_str);
    }
    builder
}

pub(crate) fn apply_client_request_id(builder: RequestBuilder, client_request_id: &str) -> RequestBuilder {
    if client_request_id.trim().is_empty() {
        return builder;
    }
    builder.header("X-Client-Request-Id", client_request_id)
}

#[cfg(test)]
mod tests {
    use super::apply_turn_metadata;
    use serde_json::{Value, json};

    #[test]
    fn borrowed_metadata_preserves_header_values_and_absence() {
        let client = reqwest::Client::new();
        let metadata = json!({"branch": "feature", "dirty": true});
        let null = Value::Null;
        for (metadata, expected) in [
            (None, None),
            (Some(&null), Some("null")),
            (Some(&metadata), Some(r#"{"branch":"feature","dirty":true}"#)),
        ] {
            let request = apply_turn_metadata(client.post("https://example.invalid/responses"), metadata)
                .build()
                .unwrap();
            assert_eq!(request.headers().get("X-Turn-Metadata").map(|value| value.to_str().unwrap()), expected);
        }
    }
}
