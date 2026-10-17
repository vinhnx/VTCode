//! Gemini explicit context caching (`cachedContents` lifecycle).
//!
//! When `GeminiPromptCacheMode::Explicit` is enabled, the first request of a
//! cache segment creates a `cachedContents` entry (model, system instruction,
//! tools, stable conversation prefix, TTL). Later `generateContent` calls send
//! `cachedContent: "<name>"` plus only the messages after the cached prefix.
//!
//! Segment identity is `(model, system hash, tool hash, prefix_len)`. Any
//! change creates a new cache and best-effort deletes the previous name.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::wire::models::{Content, SystemInstruction, Tool};

/// Identity of a Gemini explicit cache segment.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct CacheFingerprint {
    pub(super) model: String,
    pub(super) system_hash: u64,
    pub(super) tool_hash: u64,
    pub(super) prefix_len: usize,
}

impl CacheFingerprint {
    pub(super) fn new(
        model: &str,
        system: Option<&SystemInstruction>,
        tools: Option<&[Tool]>,
        prefix_len: usize,
    ) -> Self {
        Self {
            model: model.to_string(),
            system_hash: hash_json(&system.map(|s| json!(s))),
            tool_hash: hash_json(&tools.map(|t| json!(t))),
            prefix_len,
        }
    }
}

fn hash_json(value: &Option<Value>) -> u64 {
    let mut hasher = std::hash::DefaultHasher::new();
    match value {
        Some(v) => {
            let text = serde_json::to_string(v).unwrap_or_default();
            std::hash::Hash::hash(&text, &mut hasher);
        }
        None => std::hash::Hash::hash(&0u8, &mut hasher),
    }
    std::hash::Hasher::finish(&hasher)
}

/// Live cache slot for the provider instance.
#[derive(Debug, Default)]
pub(super) struct ExplicitCacheState {
    inner: Mutex<Option<ActiveCache>>,
}

#[derive(Debug, Clone)]
struct ActiveCache {
    name: String,
    fingerprint: CacheFingerprint,
}

impl ExplicitCacheState {
    pub(super) fn current(&self) -> Option<(String, CacheFingerprint)> {
        self.inner
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|c| (c.name.clone(), c.fingerprint.clone())))
    }

    pub(super) fn install(&self, name: String, fingerprint: CacheFingerprint) {
        if let Ok(mut guard) = self.inner.lock() {
            *guard = Some(ActiveCache { name, fingerprint });
        }
    }

    pub(super) fn clear(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|mut guard| guard.take().map(|c| c.name))
    }
}

/// Wire body for `POST /v1beta/cachedContents`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CreateCachedContentRequest {
    pub(super) model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) system_instruction: Option<SystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) tools: Option<Vec<Tool>>,
    pub(super) contents: Vec<Content>,
    pub(super) ttl: String,
}

/// Response from `cachedContents.create`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CachedContent {
    pub(super) name: String,
}

/// Build the create-cache body. `ttl_seconds` is rendered as Google's
/// duration string (e.g. `"900s"`).
pub(super) fn build_create_request(
    model: &str,
    system_instruction: Option<SystemInstruction>,
    tools: Option<Vec<Tool>>,
    contents: Vec<Content>,
    ttl_seconds: u64,
) -> CreateCachedContentRequest {
    CreateCachedContentRequest {
        model: format!("models/{model}"),
        display_name: Some("vtcode-prompt-cache".to_string()),
        system_instruction,
        tools,
        contents,
        ttl: format!("{ttl_seconds}s"),
    }
}

/// Split full conversation contents into the cached prefix and the tail that
/// must still be sent with `cachedContent`.
pub(super) fn split_contents_for_cache(contents: &[Content], prefix_len: usize) -> (&[Content], &[Content]) {
    let split = prefix_len.min(contents.len());
    contents.split_at(split)
}

/// True when a Gemini API error indicates the cached name is gone.
///
/// A 404 means the cached resource itself is gone. A 400 only counts when
/// the body names `cachedContent`: a generic "not found" on a 400 (for
/// example `Tool use with function 'x' not found`) is an unrelated
/// invalid-argument error and must neither discard the live cache slot nor
/// spend an uncached retry that fails identically.
pub(super) fn is_stale_cache_error(status: u16, body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    if lower.contains("cachedcontent") {
        return status == 400 || status == 404;
    }
    status == 404 && (lower.contains("not_found") || lower.contains("not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::gemini::wire::models::Part;

    fn content(role: &str, text: &str) -> Content {
        Content {
            role: role.to_string(),
            parts: vec![Part::Text { text: text.to_string(), thought_signature: None }],
        }
    }

    #[test]
    fn fingerprint_changes_when_system_or_prefix_changes() {
        let system = SystemInstruction::new("sys".to_string());
        let a = CacheFingerprint::new("gemini-3", Some(&system), None, 2);
        let b = CacheFingerprint::new("gemini-3", Some(&system), None, 2);
        let c = CacheFingerprint::new("gemini-3", Some(&system), None, 3);
        let other = SystemInstruction::new("other".to_string());
        let d = CacheFingerprint::new("gemini-3", Some(&other), None, 2);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn create_request_carries_ttl_and_model() {
        let body = build_create_request(
            "gemini-3-flash",
            Some(SystemInstruction::new("sys".to_string())),
            None,
            vec![content("user", "hello")],
            900,
        );
        assert_eq!(body.model, "models/gemini-3-flash");
        assert_eq!(body.ttl, "900s");
        assert_eq!(body.contents.len(), 1);
    }

    #[test]
    fn split_contents_returns_prefix_and_tail() {
        let contents = vec![
            content("user", "a"),
            content("model", "b"),
            content("user", "c"),
            content("model", "d"),
        ];
        let (cached, tail) = split_contents_for_cache(&contents, 2);
        assert_eq!(cached.len(), 2);
        assert_eq!(tail.len(), 2);
        let (_, tail2) = split_contents_for_cache(&contents, 10);
        assert!(tail2.is_empty());
    }

    #[test]
    fn stale_cache_error_matches_404_not_found() {
        assert!(is_stale_cache_error(404, "Cached content not found"));
        assert!(is_stale_cache_error(400, "cachedContent name is invalid"));
        assert!(is_stale_cache_error(404, r#"{"error":{"status":"NOT_FOUND"}}"#));
        assert!(is_stale_cache_error(
            404,
            r#"{"error":{"code":404,"message":"CachedContent not found","status":"NOT_FOUND"}}"#
        ));
        assert!(!is_stale_cache_error(500, "internal"));
    }

    #[test]
    fn stale_cache_error_400_requires_the_cache_resource_in_the_body() {
        // A generic "not found" on a 400 is an unrelated invalid-argument
        // error (e.g. unknown function name); it must not discard the live
        // cache slot or spend an uncached retry.
        assert!(!is_stale_cache_error(400, "Tool use with function 'get_weather' not found"));
        assert!(!is_stale_cache_error(400, "Invalid request: unsupported generationConfig field"));
        // Naming the cache resource still counts on a 400.
        assert!(is_stale_cache_error(400, "cachedContent TTL is invalid"));
    }

    #[test]
    fn explicit_cache_state_installs_and_clears() {
        let state = ExplicitCacheState::default();
        let fp = CacheFingerprint::new("m", None, None, 1);
        state.install("cachedContents/abc".to_string(), fp.clone());
        assert_eq!(state.current().map(|(n, _)| n).as_deref(), Some("cachedContents/abc"));
        assert_eq!(state.clear().as_deref(), Some("cachedContents/abc"));
        assert!(state.current().is_none());
    }
}
