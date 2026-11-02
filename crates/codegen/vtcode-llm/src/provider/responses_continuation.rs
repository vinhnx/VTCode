use super::Message;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct ResponsesContinuationState<M = Message> {
    pub response_id: String,
    /// Full request history at the time the chain was recorded. Shared via
    /// `Arc` so recording a chain after each turn is O(1), not O(history).
    pub messages: Arc<Vec<M>>,
}

/// Session-scoped store of provider continuation chains.
///
/// One owner for the chain key, response-id, and history semantics so the
/// interactive runloop and the headless runner cannot drift apart.
#[derive(Debug, Default, Clone)]
pub struct ResponsesContinuationStore {
    chains: HashMap<(String, String), ResponsesContinuationState>,
}

impl ResponsesContinuationStore {
    /// The recorded chain for a provider and model, if any.
    pub fn get(&self, provider: &str, model: &str) -> Option<&ResponsesContinuationState> {
        responses_continuation_key(provider, model).and_then(|key| self.chains.get(&key))
    }

    /// Record a chain, or drop the existing one when the response id is empty.
    pub fn set(&mut self, provider: &str, model: &str, response_id: Option<&str>, messages: Arc<Vec<Message>>) {
        let Some(key) = responses_continuation_key(provider, model) else {
            return;
        };
        let Some(response_id) = response_id.map(str::trim).filter(|value| !value.is_empty()) else {
            self.chains.remove(&key);
            return;
        };

        self.chains
            .insert(key, ResponsesContinuationState { response_id: response_id.to_string(), messages });
    }

    /// Drop the chain recorded for one provider and model.
    pub fn clear_for(&mut self, provider: &str, model: &str) {
        if let Some(key) = responses_continuation_key(provider, model) {
            self.chains.remove(&key);
        }
    }

    /// Drop every recorded chain.
    pub fn clear(&mut self) {
        self.chains.clear();
    }

    /// Whether no chain is recorded.
    pub fn is_empty(&self) -> bool {
        self.chains.is_empty()
    }

    /// Prepare request messages for a continuation-aware provider.
    ///
    /// A stale chain that the provider rejects is dropped here so the next
    /// request starts from the full history.
    pub fn prepare_request_messages<'a>(
        &mut self,
        provider_name: &str,
        provider_supports_responses_compaction: bool,
        model: &str,
        messages: &'a [Message],
    ) -> (Cow<'a, [Message]>, Option<String>) {
        let key = responses_continuation_key(provider_name, model);
        let continuation = key.as_ref().and_then(|key| self.chains.get(key));
        let prepared = prepare_responses_continuation_request(
            provider_name,
            provider_supports_responses_compaction,
            messages,
            continuation,
        );
        if prepared.clear_stale_chain
            && let Some(key) = key
        {
            self.chains.remove(&key);
        }

        (prepared.messages, prepared.previous_response_id)
    }
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

pub fn supports_responses_chaining(provider_name: &str, provider_supports_responses_compaction: bool) -> bool {
    provider_supports_responses_compaction
        || provider_name.eq_ignore_ascii_case("openai")
        || provider_name.eq_ignore_ascii_case("openresponses")
        || provider_name.eq_ignore_ascii_case("gemini")
}

pub fn records_responses_continuation_state(
    provider_name: &str,
    _provider_supports_responses_compaction: bool,
) -> bool {
    provider_name.eq_ignore_ascii_case("openresponses") || provider_name.eq_ignore_ascii_case("gemini")
}

/// Compatibility shim for older callers.
///
/// Responses continuation now preserves full request history. Use
/// [`prepare_responses_continuation_request`] to derive request state.
pub fn uses_incremental_responses_history(_provider_name: &str, _provider_supports_responses_compaction: bool) -> bool {
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

    if provider_name.eq_ignore_ascii_case("openresponses") || provider_name.eq_ignore_ascii_case("gemini") {
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
        ResponsesContinuationState, ResponsesContinuationStore, prepare_openai_responses_request,
        prepare_responses_continuation_request, records_responses_continuation_state, responses_continuation_key,
    };
    use crate::provider::Message;
    use std::borrow::Cow;
    use std::sync::Arc;

    #[test]
    fn continuation_store_records_reads_and_clears_one_chain_at_a_time() {
        let mut store = ResponsesContinuationStore::default();
        let openai_messages = Arc::new(vec![Message::user("openai hello".to_string())]);
        let gemini_messages = Arc::new(vec![Message::user("gemini hello".to_string())]);

        store.set("openai", "gpt-5.6-sol", Some("resp_openai"), Arc::clone(&openai_messages));
        store.set("gemini", "gemini-2.5-pro", Some("resp_gemini"), Arc::clone(&gemini_messages));

        assert_eq!(store.get("openai", "gpt-5.6-sol").map(|chain| chain.response_id.as_str()), Some("resp_openai"));
        assert_eq!(store.get("gemini", "gemini-2.5-pro").map(|chain| chain.response_id.as_str()), Some("resp_gemini"));
        assert!(store.get("openai", "gpt-5.6-terra").is_none());
        assert!(!store.is_empty());

        store.clear_for("openai", "gpt-5.6-sol");
        assert!(store.get("openai", "gpt-5.6-sol").is_none());
        assert_eq!(store.get("gemini", "gemini-2.5-pro").map(|chain| chain.response_id.as_str()), Some("resp_gemini"));

        store.clear();
        assert!(store.is_empty());
    }

    #[test]
    fn continuation_store_normalizes_the_provider_and_drops_an_empty_response_id() {
        let mut store = ResponsesContinuationStore::default();
        store.set("OpenAI", "gpt-5.6-sol", Some("resp_1"), Arc::new(vec![Message::user("hi".to_string())]));

        // The provider key is normalized, so a differently-cased lookup finds it.
        assert!(store.get("openai", "gpt-5.6-sol").is_some());
        assert!(store.get("OPENAI", "gpt-5.6-sol").is_some());

        // An empty response id drops the recorded chain instead of storing it.
        store.set("openai", "gpt-5.6-sol", Some("   "), Arc::new(vec![]));
        assert!(store.get("openai", "gpt-5.6-sol").is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn continuation_store_prepares_provider_specific_requests() {
        let mut store = ResponsesContinuationStore::default();
        let prior = vec![Message::user("hello".to_string())];
        let current = vec![
            Message::user("hello".to_string()),
            Message::user("continue".to_string()),
        ];

        store.set("gemini", "gemini-2.5-pro", Some("resp_gemini"), Arc::new(prior.clone()));
        let (messages, previous_response_id) =
            store.prepare_request_messages("gemini", false, "gemini-2.5-pro", &current);

        assert_eq!(previous_response_id.as_deref(), Some("resp_gemini"));
        assert!(matches!(messages, Cow::Borrowed(_)));
        assert_eq!(messages.as_ref(), current.as_slice());

        // OpenAI stays stateless: the recorded chain never becomes a previous id.
        store.set("openai", "gpt-5.6-sol", Some("resp_openai"), Arc::new(prior.clone()));
        let (messages, previous_response_id) = store.prepare_request_messages("openai", false, "gpt-5.6-sol", &current);
        assert_eq!(previous_response_id, None);
        assert_eq!(messages.as_ref(), current.as_slice());
    }

    #[test]
    fn continuation_key_requires_non_empty_provider_and_model() {
        assert_eq!(responses_continuation_key("", "gpt-5"), None);
        assert_eq!(responses_continuation_key("openai", ""), None);
        assert_eq!(responses_continuation_key("OpenAI", "gpt-5"), Some(("openai".to_string(), "gpt-5".to_string())));
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
                messages: vec![Message::user("hello".to_string())].into(),
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
                messages: vec![Message::user("hello".to_string())].into(),
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
                messages: vec![Message::user("hello".to_string())].into(),
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
                messages: vec![Message::user("hello".to_string())].into(),
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
                messages: vec![Message::user("hello".to_string())].into(),
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
                messages: messages.clone().into(),
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
                messages: vec![Message::user("hello".to_string())].into(),
            }),
        );

        assert_eq!(prepared.previous_response_id, None);
        assert!(matches!(prepared.messages, Cow::Borrowed(_)));
        assert_eq!(prepared.messages.as_ref(), messages.as_slice());
        assert!(!prepared.clear_stale_chain);
    }
}
