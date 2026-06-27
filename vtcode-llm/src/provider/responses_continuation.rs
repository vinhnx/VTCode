use super::Message;
use std::borrow::Cow;

#[derive(Debug, Clone, PartialEq)]
pub struct ResponsesContinuationState<M = Message> {
    pub response_id: String,
    pub messages: Vec<M>,
}

pub struct PreparedResponsesRequest<'a, M: Clone = Message> {
    pub messages: Cow<'a, [M]>,
    pub previous_response_id: Option<String>,
    pub clear_stale_chain: bool,
}

pub fn responses_continuation_key(provider: &str, model: &str) -> Option<(String, String)> {
    let provider = provider.trim().to_ascii_lowercase();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return None;
    }

    Some((provider, model.to_string()))
}

pub fn supports_responses_chaining(
    provider_name: &str,
    provider_supports_responses_compaction: bool,
) -> bool {
    provider_supports_responses_compaction
        || provider_name.eq_ignore_ascii_case("openai")
        || provider_name.eq_ignore_ascii_case("openresponses")
        || provider_name.eq_ignore_ascii_case("gemini")
}

pub fn records_responses_continuation_state(
    provider_name: &str,
    _provider_supports_responses_compaction: bool,
) -> bool {
    provider_name.eq_ignore_ascii_case("openresponses")
        || provider_name.eq_ignore_ascii_case("gemini")
}

/// Compatibility shim for older callers.
///
/// Responses continuation now preserves full request history. Use
/// [`prepare_responses_continuation_request`] to derive request state.
pub fn uses_incremental_responses_history(
    _provider_name: &str,
    _provider_supports_responses_compaction: bool,
) -> bool {
    false
}

pub fn prepare_responses_continuation_request<'a, M>(
    provider_name: &str,
    provider_supports_responses_compaction: bool,
    messages: &'a [M],
    continuation: Option<&ResponsesContinuationState<M>>,
) -> PreparedResponsesRequest<'a, M>
where
    M: Clone + PartialEq,
{
    if provider_name.eq_ignore_ascii_case("openai") {
        return prepare_openai_responses_request(messages, continuation);
    }

    if !supports_responses_chaining(provider_name, provider_supports_responses_compaction) {
        return PreparedResponsesRequest {
            messages: Cow::Borrowed(messages),
            previous_response_id: None,
            clear_stale_chain: false,
        };
    }

    if provider_name.eq_ignore_ascii_case("openresponses")
        || provider_name.eq_ignore_ascii_case("gemini")
    {
        return PreparedResponsesRequest {
            messages: Cow::Borrowed(messages),
            previous_response_id: continuation.map(|chain| chain.response_id.clone()),
            clear_stale_chain: false,
        };
    }

    prepare_openai_responses_request(messages, continuation)
}

pub fn prepare_openai_responses_request<'a, M>(
    messages: &'a [M],
    _continuation: Option<&ResponsesContinuationState<M>>,
) -> PreparedResponsesRequest<'a, M>
where
    M: Clone,
{
    PreparedResponsesRequest {
        messages: Cow::Borrowed(messages),
        previous_response_id: None,
        clear_stale_chain: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ResponsesContinuationState, prepare_openai_responses_request,
        prepare_responses_continuation_request, records_responses_continuation_state,
        responses_continuation_key,
    };
    use crate::provider::Message;
    use std::borrow::Cow;

    #[test]
    fn continuation_key_requires_non_empty_provider_and_model() {
        assert_eq!(responses_continuation_key("", "gpt-5"), None);
        assert_eq!(responses_continuation_key("openai", ""), None);
        assert_eq!(
            responses_continuation_key("OpenAI", "gpt-5"),
            Some(("openai".to_string(), "gpt-5".to_string()))
        );
    }

    #[test]
    fn openai_does_not_record_normal_responses_continuation_state() {
        assert!(!records_responses_continuation_state("openai", true));
        assert!(!records_responses_continuation_state("OpenAI", false));
    }

    #[test]
    fn provider_specific_responses_chaining_records_continuation_state() {
        assert!(!records_responses_continuation_state("mycorp", true));
        assert!(records_responses_continuation_state("gemini", false));
        assert!(records_responses_continuation_state("openresponses", false));
        assert!(!records_responses_continuation_state("anthropic", false));
    }

    #[test]
    fn prepare_openai_request_keeps_full_history_without_previous_response_id() {
        let messages = vec![
            Message::user("hello".to_string()),
            Message::user("continue".to_string()),
        ];
        let prepared = prepare_openai_responses_request(
            &messages,
            Some(&ResponsesContinuationState {
                response_id: "resp_123".to_string(),
                messages: vec![Message::user("hello".to_string())],
            }),
        );

        assert_eq!(prepared.previous_response_id, None);
        assert!(matches!(prepared.messages, Cow::Borrowed(_)));
        assert_eq!(prepared.messages.as_ref(), messages.as_slice());
        assert!(!prepared.clear_stale_chain);
    }

    #[test]
    fn prepare_openai_request_ignores_stale_chain_without_retry_recovery() {
        let messages = vec![Message::user("continue".to_string())];
        let prepared = prepare_openai_responses_request(
            &messages,
            Some(&ResponsesContinuationState {
                response_id: "resp_123".to_string(),
                messages: vec![Message::user("hello".to_string())],
            }),
        );

        assert_eq!(prepared.previous_response_id, None);
        assert!(matches!(prepared.messages, Cow::Borrowed(_)));
        assert_eq!(prepared.messages.as_ref(), messages.as_slice());
        assert!(!prepared.clear_stale_chain);
    }

    #[test]
    fn prepare_responses_continuation_request_keeps_openai_stateless() {
        let messages = vec![
            Message::user("hello".to_string()),
            Message::user("continue".to_string()),
        ];
        let prepared = prepare_responses_continuation_request(
            "openai",
            false,
            &messages,
            Some(&ResponsesContinuationState {
                response_id: "resp_123".to_string(),
                messages: vec![Message::user("hello".to_string())],
            }),
        );

        assert_eq!(prepared.previous_response_id, None);
        assert!(matches!(prepared.messages, Cow::Borrowed(_)));
        assert_eq!(prepared.messages.as_ref(), messages.as_slice());
        assert!(!prepared.clear_stale_chain);
    }

    #[test]
    fn prepare_responses_continuation_request_keeps_compatible_provider_stateless() {
        let messages = vec![
            Message::user("hello".to_string()),
            Message::user("continue".to_string()),
        ];
        let prepared = prepare_responses_continuation_request(
            "mycorp",
            true,
            &messages,
            Some(&ResponsesContinuationState {
                response_id: "resp_123".to_string(),
                messages: vec![Message::user("hello".to_string())],
            }),
        );

        assert_eq!(prepared.previous_response_id, None);
        assert!(matches!(prepared.messages, Cow::Borrowed(_)));
        assert_eq!(prepared.messages.as_ref(), messages.as_slice());
        assert!(!prepared.clear_stale_chain);
    }

    #[test]
    fn prepare_responses_continuation_request_keeps_full_history_for_gemini() {
        let messages = vec![
            Message::user("hello".to_string()),
            Message::user("continue".to_string()),
        ];
        let prepared = prepare_responses_continuation_request(
            "gemini",
            false,
            &messages,
            Some(&ResponsesContinuationState {
                response_id: "resp_123".to_string(),
                messages: vec![Message::user("hello".to_string())],
            }),
        );

        assert_eq!(prepared.previous_response_id.as_deref(), Some("resp_123"));
        assert!(matches!(prepared.messages, Cow::Borrowed(_)));
        assert_eq!(prepared.messages.as_ref(), messages.as_slice());
        assert!(!prepared.clear_stale_chain);
    }

    #[test]
    fn prepare_responses_continuation_request_ignores_chain_for_unsupported_provider() {
        let messages = vec![Message::user("hello".to_string())];
        let prepared = prepare_responses_continuation_request(
            "local",
            false,
            &messages,
            Some(&ResponsesContinuationState {
                response_id: "resp_123".to_string(),
                messages: messages.clone(),
            }),
        );

        assert_eq!(prepared.previous_response_id, None);
        assert!(matches!(prepared.messages, Cow::Borrowed(_)));
        assert_eq!(prepared.messages.as_ref(), messages.as_slice());
        assert!(!prepared.clear_stale_chain);
    }

    #[test]
    fn prepare_responses_continuation_request_ignores_openai_stale_chain_without_retry_recovery() {
        let messages = vec![Message::user("continue".to_string())];
        let prepared = prepare_responses_continuation_request(
            "openai",
            false,
            &messages,
            Some(&ResponsesContinuationState {
                response_id: "resp_123".to_string(),
                messages: vec![Message::user("hello".to_string())],
            }),
        );

        assert_eq!(prepared.previous_response_id, None);
        assert!(matches!(prepared.messages, Cow::Borrowed(_)));
        assert_eq!(prepared.messages.as_ref(), messages.as_slice());
        assert!(!prepared.clear_stale_chain);
    }
}
