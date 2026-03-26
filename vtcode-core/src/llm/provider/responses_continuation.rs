use super::Message;

#[derive(Debug, Clone, PartialEq)]
pub struct ResponsesContinuationState {
    pub response_id: String,
    pub messages: Vec<Message>,
}

pub struct PreparedResponsesRequest {
    pub messages: Vec<Message>,
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

pub fn prepare_openai_responses_request(
    messages: Vec<Message>,
    continuation: Option<&ResponsesContinuationState>,
) -> PreparedResponsesRequest {
    let Some(continuation) = continuation else {
        return PreparedResponsesRequest {
            messages,
            previous_response_id: None,
            clear_stale_chain: false,
        };
    };

    let previous_len = continuation.messages.len();
    if previous_len >= messages.len() || !messages.starts_with(&continuation.messages) {
        return PreparedResponsesRequest {
            messages,
            previous_response_id: None,
            clear_stale_chain: true,
        };
    }

    PreparedResponsesRequest {
        messages: messages[previous_len..].to_vec(),
        previous_response_id: Some(continuation.response_id.clone()),
        clear_stale_chain: false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PreparedResponsesRequest, ResponsesContinuationState, prepare_openai_responses_request,
        responses_continuation_key,
    };
    use crate::llm::provider::Message;

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
    fn prepare_openai_request_uses_incremental_suffix_for_matching_prefix() {
        let prepared = prepare_openai_responses_request(
            vec![
                Message::user("hello".to_string()),
                Message::user("continue".to_string()),
            ],
            Some(&ResponsesContinuationState {
                response_id: "resp_123".to_string(),
                messages: vec![Message::user("hello".to_string())],
            }),
        );

        assert_eq!(prepared.previous_response_id.as_deref(), Some("resp_123"));
        assert_eq!(
            prepared.messages,
            vec![Message::user("continue".to_string())]
        );
        assert!(!prepared.clear_stale_chain);
    }

    #[test]
    fn prepare_openai_request_replays_full_history_for_stale_prefix() {
        let prepared = prepare_openai_responses_request(
            vec![Message::user("continue".to_string())],
            Some(&ResponsesContinuationState {
                response_id: "resp_123".to_string(),
                messages: vec![Message::user("hello".to_string())],
            }),
        );

        assert!(matches!(
            prepared,
            PreparedResponsesRequest {
                previous_response_id: None,
                clear_stale_chain: true,
                ..
            }
        ));
    }
}
