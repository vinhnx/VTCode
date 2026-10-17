//! Prompt-cache observability: lineage, fingerprints, and billing advisories.
//!
//! Extracted from state.rs beside the pre-existing preflight_recovery,
//! request_envelope, ctrl_c, and verification submodules. No behavior
//! change: cache-turn health attribution, change-reason counters, and the
//! one-shot model-change and stream-timeout advisories are verbatim, and
//! PromptCacheDiagnostics is re-exported from the parent.

use super::SessionStats;

fn is_local_llm_provider(provider_name: &str) -> bool {
    matches!(
        provider_name.trim().to_ascii_lowercase().as_str(),
        "ollama" | "lmstudio" | "llamacpp" | "llama.cpp" | "local" | "local_server"
    )
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PromptCacheDiagnostics {
    pub observations: usize,
    pub model_changes: usize,
    pub unchanged: usize,
    pub stable_prefix_changes: usize,
    pub tool_catalog_changes: usize,
    pub combined_changes: usize,
    pub last_change_reason: Option<String>,
    pub last_stable_prefix_hash: Option<u64>,
    pub last_tool_catalog_hash: Option<u64>,
}

impl SessionStats {
    pub(crate) fn set_prompt_cache_lineage_id(&mut self, lineage_id: Option<String>) {
        self.prompt_cache_lineage_id = lineage_id;
    }

    pub(crate) fn prompt_cache_lineage_id(&self) -> Option<&str> {
        self.prompt_cache_lineage_id.as_deref()
    }

    pub(crate) fn prompt_cache_diagnostics(&self) -> PromptCacheDiagnostics {
        PromptCacheDiagnostics {
            observations: self.prompt_cache_observations,
            model_changes: self.prompt_cache_model_changes,
            unchanged: self.prompt_cache_unchanged,
            stable_prefix_changes: self.prompt_cache_stable_prefix_changes,
            tool_catalog_changes: self.prompt_cache_tool_catalog_changes,
            combined_changes: self.prompt_cache_combined_changes,
            last_change_reason: self.last_prompt_cache_change_reason.clone(),
            last_stable_prefix_hash: self.last_stable_prefix_hash,
            last_tool_catalog_hash: self.last_tool_catalog_hash,
        }
    }

    #[cfg(test)]
    pub(crate) fn record_prompt_cache_fingerprint(
        &mut self,
        model: &str,
        stable_prefix_hash: u64,
        tool_catalog_hash: Option<u64>,
    ) -> &'static str {
        self.record_prompt_cache_fingerprint_with_context(model, stable_prefix_hash, tool_catalog_hash, None, None)
    }

    /// Fingerprint with recovery/wire context so cache misses can be
    /// attributed to tool-catalog omission or `[Recovery Mode]` reason churn
    /// instead of a generic stable-prefix change.
    pub(crate) fn record_prompt_cache_fingerprint_with_context(
        &mut self,
        model: &str,
        stable_prefix_hash: u64,
        tool_catalog_hash: Option<u64>,
        wire_tool_count: Option<usize>,
        recovery_prompt_reason: Option<&str>,
    ) -> &'static str {
        let reason = if self.last_prompt_cache_model.as_deref() != Some(model) {
            "model"
        } else if matches!(
            (self.last_wire_tool_count, wire_tool_count),
            (Some(prev), Some(curr)) if prev > 0 && curr == 0
        ) {
            "tools_omitted"
        } else if self.last_recovery_prompt_reason.as_deref() != recovery_prompt_reason
            && recovery_prompt_reason.is_some()
            && self.last_recovery_prompt_reason.is_some()
        {
            "recovery_reason"
        } else {
            match (
                self.last_stable_prefix_hash == Some(stable_prefix_hash),
                self.last_tool_catalog_hash == tool_catalog_hash,
            ) {
                (true, true) => "unchanged",
                (false, true) => "stable_prefix",
                (true, false) => "tool_catalog",
                (false, false) => "stable_prefix+tool_catalog",
            }
        };

        self.prompt_cache_observations = self.prompt_cache_observations.saturating_add(1);
        *self.counter_for_reason(reason) += 1;

        self.last_prompt_cache_model = Some(model.to_string());
        self.last_stable_prefix_hash = Some(stable_prefix_hash);
        self.last_tool_catalog_hash = tool_catalog_hash;
        self.last_wire_tool_count = wire_tool_count;
        self.last_recovery_prompt_reason = recovery_prompt_reason.map(str::to_string);
        self.last_prompt_cache_change_reason = Some(reason.to_string());

        reason
    }

    /// Advisory for mid-session model switches, derived from the last
    /// recorded fingerprint. Prompt caches are unique per model, so a switch
    /// rebuilds the cache at full input cost even when the rest of the prefix
    /// is unchanged. Returns `Some` exactly on the first request carrying a
    /// new model (the session-start recording is excluded via the
    /// observations guard); callers should surface the message and keep
    /// going — no state is consumed.
    pub(crate) fn model_change_advisory(&self) -> Option<String> {
        (self.prompt_cache_observations > 1
            && self.last_prompt_cache_change_reason.as_deref() == Some("model"))
        .then(|| {
            "Model changed mid-session; provider prompt cache will be invalidated and the next request re-pays full \
             input cost. Prefer resolving the model up front; expect one full-price request before hits resume."
                .to_string()
        })
    }

    /// One-shot advisory when a remote non-streaming-capable provider falls
    /// back after stream first-token timeout: abandoned work may still be
    /// billed and the retry re-sends the full prompt. Local providers are
    /// excluded (no remote bill).
    pub(crate) fn stream_timeout_billing_advisory(&mut self, provider_name: &str) -> Option<String> {
        if self.stream_timeout_advisory_emitted || is_local_llm_provider(provider_name) {
            return None;
        }
        self.stream_timeout_advisory_emitted = true;
        let merge = provider_name.eq_ignore_ascii_case("merge-gateway");
        if merge {
            Some(
                "Merge Gateway stream timed out before first token; falling back to non-streaming. Abandoned provider \
                 streams may still be drained and billed, and the retry re-sends the full prompt."
                    .to_string(),
            )
        } else {
            Some(format!(
                "LLM stream timed out on {provider_name} before first token; falling back to non-streaming. \
                 Abandoned provider work may still be billed, and the retry re-sends the full prompt."
            ))
        }
    }

    fn counter_for_reason(&mut self, reason: &str) -> &mut usize {
        match reason {
            "model" => &mut self.prompt_cache_model_changes,
            "unchanged" => &mut self.prompt_cache_unchanged,
            "stable_prefix" => &mut self.prompt_cache_stable_prefix_changes,
            "tool_catalog" => &mut self.prompt_cache_tool_catalog_changes,
            "stable_prefix+tool_catalog" => &mut self.prompt_cache_combined_changes,
            // Attributed miss causes: tool definitions dropped from the wire,
            // or the frozen `[Recovery Mode]` reason rotated mid-activation.
            "tools_omitted" | "recovery_reason" => &mut self.prompt_cache_stable_prefix_changes,
            _ => &mut self.prompt_cache_unchanged,
        }
    }
}
