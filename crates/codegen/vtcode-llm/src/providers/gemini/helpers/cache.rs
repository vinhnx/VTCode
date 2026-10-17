//! Explicit cachedContents lifecycle for stable system+tools prefixes.

use super::*;

#[cfg(test)]
use crate::provider::ToolDefinition;

impl GeminiProvider {
    /// Whether explicit `cachedContents` mode is active for this provider.
    ///
    /// Some models reject explicit `cachedContents` entirely (Gemini 4 Argon
    /// supports implicit caching only — creation fails and body references
    /// error), so the shared lifecycle is additionally gated per model.
    pub(crate) fn explicit_cache_active(&self, model: &str) -> bool {
        if models::google::IMPLICIT_CACHING_ONLY_MODELS.contains(&model) {
            return false;
        }
        self.prompt_cache_enabled && matches!(self.prompt_cache_settings.mode, GeminiPromptCacheMode::Explicit)
    }

    /// Whether the built request carries a tool configuration that must travel
    /// in the body.
    ///
    /// Gemini rejects a `generateContent` request that combines `cachedContent`
    /// with `systemInstruction`, `tools`, or `toolConfig` ("CachedContent can
    /// not be used with GenerateContent request setting system_instruction,
    /// tools or tool_config"), and a cached entry's own `toolConfig` is
    /// immutable and shared for the whole segment. The API default when the
    /// field is omitted is plain `AUTO` without server-side flags, so only a
    /// request that narrows, disables, or validates tool use needs the body
    /// field.
    fn body_tool_config_is_non_default(gemini_request: &GenerateContentRequest) -> bool {
        gemini_request.tool_config.as_ref().is_some_and(|config| !config.is_default())
    }

    /// Ensure a `cachedContents` entry exists for the request's stable
    /// system+tools prefix. Conversation contents stay on the generateContent
    /// body so the cache is not rebuilt every turn as history grows.
    pub(crate) async fn ensure_explicit_cache(
        &self,
        request: &LLMRequest,
        gemini_request: &GenerateContentRequest,
    ) -> Result<Option<String>, LLMError> {
        if !self.explicit_cache_active(&request.model) {
            return Ok(None);
        }

        // A non-default tool configuration cannot travel next to
        // `cachedContent`, so such requests keep the implicit shape (with
        // `toolConfig` on the body) instead of silently losing the constraint.
        // The installed segment entry is left intact for later unconstrained
        // turns.
        if Self::body_tool_config_is_non_default(gemini_request) {
            return Ok(None);
        }

        let ttl = self.prompt_cache_settings.explicit_ttl_seconds.unwrap_or(900).max(60);
        let fingerprint = explicit_cache::CacheFingerprint::new(
            &request.model,
            gemini_request.system_instruction.as_ref(),
            gemini_request.tools.as_deref(),
            0,
        );
        if let Some((name, existing)) = self.explicit_cache.current()
            && existing == fingerprint
        {
            return Ok(Some(name));
        }

        if let Some(old) = self.explicit_cache.clear() {
            self.delete_cached_content(&old).await;
        }

        let body = explicit_cache::build_create_request(
            &request.model,
            gemini_request.system_instruction.clone(),
            gemini_request.tools.clone(),
            Vec::new(),
            ttl,
        );
        let url = format!("{}/cachedContents", self.base_url);
        let response = self
            .http_client
            .post(&url)
            .header("x-goog-api-key", self.api_key.as_ref())
            .json(&body)
            .send()
            .await
            .map_err(|e| format_network_error("Gemini", &e))?;
        if !response.status().is_success() {
            let status = response.status();
            let error_text = crate::providers::common::read_provider_error_body(response).await;
            // Explicit mode is best-effort: fall back to the implicit shape.
            tracing::warn!(status = %status, "Gemini cachedContents.create failed; falling back to implicit cache shape");
            let _ = error_text;
            return Ok(None);
        }
        let created: explicit_cache::CachedContent =
            response.json().await.map_err(|e| format_parse_error("Gemini", &e))?;
        self.explicit_cache.install(created.name.clone(), fingerprint);
        Ok(Some(created.name))
    }

    async fn delete_cached_content(&self, name: &str) {
        let url = format!("{}/{}", self.base_url, name.trim_start_matches('/'));
        let _ = self
            .http_client
            .delete(&url)
            .header("x-goog-api-key", self.api_key.as_ref())
            .send()
            .await;
    }

    /// Rewrite a generateContent body to use `cachedContent` for system+tools.
    /// Conversation contents remain on the request.
    ///
    /// The API rejects `systemInstruction`, `tools`, and `toolConfig` next to
    /// `cachedContent`, so all three are dropped here. A request whose built
    /// tool configuration is not the API default never reaches this function
    /// (see [`Self::body_tool_config_is_non_default`]).
    pub(crate) fn apply_explicit_cache_to_request(
        &self,
        mut gemini_request: GenerateContentRequest,
        cache_name: &str,
    ) -> GenerateContentRequest {
        gemini_request.system_instruction = None;
        gemini_request.tools = None;
        gemini_request.tool_config = None;
        gemini_request.cached_content = Some(cache_name.to_string());
        gemini_request
    }
}

#[cfg(test)]
mod explicit_cache_tool_choice_tests {
    use super::*;

    fn tool_definition() -> ToolDefinition {
        ToolDefinition::function(
            "search_workspace".to_string(),
            "Search project files".to_string(),
            json!({
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }),
        )
    }

    fn request(tool_choice: Option<ToolChoice>, with_tools: bool) -> LLMRequest {
        LLMRequest {
            model: models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
            messages: vec![Message::user("hello".to_string())].into(),
            tools: with_tools.then(|| Arc::new(vec![tool_definition()])),
            tool_choice,
            ..Default::default()
        }
    }

    #[test]
    fn only_non_default_tool_configs_require_the_body() {
        let provider = GeminiProvider::new("test-key".to_string());
        let built = |tool_choice, with_tools| {
            provider
                .convert_to_gemini_request(&request(tool_choice, with_tools))
                .expect("request builds")
        };

        // Unspecified and explicit AUTO both resolve to the API default, so the
        // cached body may omit the field and the cache stays usable.
        assert!(!GeminiProvider::body_tool_config_is_non_default(&built(None, true)));
        assert!(!GeminiProvider::body_tool_config_is_non_default(&built(Some(ToolChoice::Auto), true)));
        assert!(!GeminiProvider::body_tool_config_is_non_default(&built(None, false)));

        // Constrained choices must stay on the body; the API forbids them next
        // to `cachedContent`.
        for tool_choice in [
            ToolChoice::None,
            ToolChoice::Any,
            ToolChoice::function("search_workspace".to_string()),
        ] {
            assert!(
                GeminiProvider::body_tool_config_is_non_default(&built(Some(tool_choice), true)),
                "constrained tool choice must keep its body config"
            );
        }
    }

    #[test]
    fn implicit_caching_only_models_skip_explicit_cache() {
        let mut provider = GeminiProvider::new("test-key".to_string());
        provider.prompt_cache_enabled = true;
        provider.prompt_cache_settings.mode = GeminiPromptCacheMode::Explicit;

        // Explicit mode is honored for models that accept `cachedContents`.
        assert!(
            provider.explicit_cache_active(models::google::GEMINI_3_FLASH_PREVIEW),
            "explicit mode should stay active for models supporting cachedContents"
        );

        // Gemini 4 Argon supports implicit caching only; the explicit
        // lifecycle must stay off even when explicitly configured, since
        // `cachedContents.create` fails and body references error.
        for model in models::google::IMPLICIT_CACHING_ONLY_MODELS {
            assert!(!provider.explicit_cache_active(model), "{model} must skip the explicit cachedContents lifecycle");
        }
    }
}
