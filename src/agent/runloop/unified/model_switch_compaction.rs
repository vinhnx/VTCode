//! Context compaction triggered by a mid-session switch of the main model or
//! provider.
//!
//! This module is deliberately isolated from the model-selection plumbing
//! (`model_selection.rs`) and from the inline/TUI event machinery. The "should
//! we compact?" decision and the compaction execution live here behind a small,
//! explicit interface so the logic can be unit-tested without constructing a
//! renderer, an inline loop, or a full session context.

use std::path::Path;

use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider::{LLMProvider, Message};

use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::turn::compaction::{
    CompactionContext, CompactionOutcome, CompactionState, compact_history_on_model_switch_in_place,
};

/// Mutable handles required to compact the conversation when the main model or
/// provider is switched mid-session. All production call sites must populate
/// this; `finalize_model_selection` only reads/writes through it when a real
/// model/provider switch is detected.
pub(crate) struct ModelSwitchCompactionTargets<'a> {
    pub history: &'a mut Vec<Message>,
    pub session_stats: &'a mut SessionStats,
    pub context_manager: &'a mut ContextManager,
    pub session_id: &'a str,
    pub thread_id: &'a str,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
}

/// Outcome of a model-switch compaction attempt. Returned to the caller for
/// rendering so this module stays free of `AnsiRenderer` and is unit-testable.
#[derive(Debug)]
pub(crate) enum ModelSwitchCompactionOutcome {
    /// Same model/provider reselected: nothing to do, history preserved.
    Unchanged,
    /// The feature is opted out via config: compaction is skipped entirely.
    Disabled,
    /// The selection changed but no usable provider client was installed (for
    /// example an unconfigured custom provider), so nothing can be summarized.
    SkippedNoClient,
    /// Model/provider changed but history was empty; only the previous-response
    /// lineage was cleared.
    LineageCleared,
    /// Compaction produced a shorter history.
    Compacted(CompactionOutcome),
    /// A switch occurred but the history was already compact (no change made).
    AlreadyCompact,
    /// Compaction failed; the switch still applies and history is kept intact.
    Failed(anyhow::Error),
}

/// Everything needed to decide and perform compaction after a model switch.
pub(crate) struct ModelSwitchCompactionRequest<'a> {
    pub prev_provider: String,
    pub prev_model: String,
    pub new_provider: String,
    pub new_model: String,
    /// Whether a real provider client for the new selection was installed. When
    /// false the stale client cannot safely summarize, so compaction is skipped.
    pub client_installed: bool,
    /// Whether the feature is enabled (config `agent.harness.compact_on_model_switch`).
    pub enabled: bool,
    pub provider: &'a dyn LLMProvider,
    pub workspace: &'a Path,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub targets: ModelSwitchCompactionTargets<'a>,
}

/// Pure decision helper: did the user actually change the model or provider?
///
/// Provider comparison is case-insensitive so a cosmetic case difference (e.g.
/// `OpenAI` vs `openai`) is not treated as a switch.
pub(crate) fn is_real_model_switch(prev_provider: &str, prev_model: &str, new_provider: &str, new_model: &str) -> bool {
    !prev_provider.eq_ignore_ascii_case(new_provider) || prev_model != new_model
}

/// Decide whether a model/provider switch requires context compaction and, if
/// so, perform it. Never fails the caller: compaction errors are surfaced via
/// [`ModelSwitchCompactionOutcome::Failed`] so the model switch itself always
/// applies.
pub(crate) async fn compact_on_model_switch(
    req: ModelSwitchCompactionRequest<'_>,
) -> Result<ModelSwitchCompactionOutcome> {
    if !req.enabled {
        return Ok(ModelSwitchCompactionOutcome::Disabled);
    }

    if !is_real_model_switch(&req.prev_provider, &req.prev_model, &req.new_provider, &req.new_model) {
        return Ok(ModelSwitchCompactionOutcome::Unchanged);
    }

    // The selection changed (for example an unconfigured custom provider) but no
    // usable client was installed, so we cannot summarize with the new
    // provider. Compact against the stale client would produce a provider-
    // mismatched summary, so we leave history untouched instead.
    if !req.client_installed {
        return Ok(ModelSwitchCompactionOutcome::SkippedNoClient);
    }

    req.targets.session_stats.clear_previous_response_chain();

    if req.targets.history.is_empty() {
        return Ok(ModelSwitchCompactionOutcome::LineageCleared);
    }

    match compact_history_on_model_switch_in_place(
        CompactionContext::new(
            req.provider,
            &req.new_model,
            req.targets.session_id,
            req.targets.thread_id,
            req.workspace,
            req.vt_cfg,
            req.targets.lifecycle_hooks,
            req.targets.harness_emitter,
        ),
        CompactionState::new(req.targets.history, req.targets.session_stats, req.targets.context_manager),
    )
    .await
    {
        Ok(Some(outcome)) => Ok(ModelSwitchCompactionOutcome::Compacted(outcome)),
        Ok(None) => Ok(ModelSwitchCompactionOutcome::AlreadyCompact),
        Err(err) => Ok(ModelSwitchCompactionOutcome::Failed(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use tempfile::tempdir;
    use vtcode_core::llm::provider::{LLMError, LLMRequest, LLMResponse};

    struct StubProvider;

    #[async_trait]
    impl LLMProvider for StubProvider {
        fn name(&self) -> &str {
            "stub"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
        }

        async fn compact_history(&self, _model: &str, history: &[Message]) -> Result<Vec<Message>, LLMError> {
            let mut compacted = vec![Message::system("Previous conversation summary".to_string())];
            compacted.extend(history.iter().rev().take(1).cloned());
            compacted.reverse();
            Ok(compacted)
        }

        async fn compact_history_with_options(
            &self,
            model: &str,
            history: &[Message],
            _options: &vtcode_core::llm::provider::ResponsesCompactionOptions,
        ) -> Result<Vec<Message>, LLMError> {
            self.compact_history(model, history).await
        }

        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supports_manual_openai_compaction(&self, _model: &str) -> bool {
            true
        }

        fn supported_models(&self) -> Vec<String> {
            vec!["stub-model".to_string()]
        }

        fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
            Ok(())
        }

        fn effective_context_size(&self, _model: &str) -> usize {
            1_000
        }
    }

    fn test_history() -> Vec<Message> {
        vec![
            Message::system("system".to_string()),
            Message::user("first".to_string()),
            Message::assistant("reply".to_string()),
            Message::user("second".to_string()),
            Message::assistant("reply two".to_string()),
        ]
    }

    #[test]
    fn real_switch_detects_model_and_provider_changes() {
        assert!(!is_real_model_switch("openai", "gpt-x", "openai", "gpt-x"));
        assert!(is_real_model_switch("openai", "gpt-x", "openai", "gpt-y"));
        // Provider comparison is case-insensitive: a cosmetic case difference
        // alone is not a switch.
        assert!(!is_real_model_switch("OpenAI", "gpt-x", "openai", "gpt-x"));
        assert!(is_real_model_switch("openai", "gpt-x", "anthropic", "gpt-x"));
        assert!(is_real_model_switch("anthropic", "gpt-x", "Anthropic", "gpt-y"));
    }

    #[tokio::test]
    async fn same_model_is_unchanged_and_preserves_history() {
        let temp = tempdir().unwrap();
        let provider = StubProvider;
        let mut history = test_history();
        let original_len = history.len();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();

        let outcome = compact_on_model_switch(ModelSwitchCompactionRequest {
            prev_provider: "openai".to_string(),
            prev_model: "gpt-x".to_string(),
            new_provider: "openai".to_string(),
            new_model: "gpt-x".to_string(),
            client_installed: true,
            enabled: true,
            provider: &provider,
            workspace: temp.path(),
            vt_cfg: None,
            targets: ModelSwitchCompactionTargets {
                history: &mut history,
                session_stats: &mut session_stats,
                context_manager: &mut context_manager,
                session_id: "s",
                thread_id: "t",
                lifecycle_hooks: None,
                harness_emitter: None,
            },
        })
        .await
        .unwrap();

        assert!(matches!(outcome, ModelSwitchCompactionOutcome::Unchanged));
        assert_eq!(history.len(), original_len);
    }

    #[tokio::test]
    async fn switch_with_client_compacts_history() {
        let temp = tempdir().unwrap();
        let provider = StubProvider;
        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();

        let outcome = compact_on_model_switch(ModelSwitchCompactionRequest {
            prev_provider: "openai".to_string(),
            prev_model: "gpt-x".to_string(),
            new_provider: "anthropic".to_string(),
            new_model: "claude-x".to_string(),
            client_installed: true,
            enabled: true,
            provider: &provider,
            workspace: temp.path(),
            vt_cfg: None,
            targets: ModelSwitchCompactionTargets {
                history: &mut history,
                session_stats: &mut session_stats,
                context_manager: &mut context_manager,
                session_id: "s",
                thread_id: "t",
                lifecycle_hooks: None,
                harness_emitter: None,
            },
        })
        .await
        .unwrap();

        match outcome {
            ModelSwitchCompactionOutcome::Compacted(o) => {
                assert!(o.compacted_len < o.original_len || o.compacted_len <= o.original_len);
                assert!(!history.is_empty());
            }
            other => panic!("expected Compacted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn switch_without_installed_client_is_unchanged() {
        let temp = tempdir().unwrap();
        let provider = StubProvider;
        let mut history = test_history();
        let original_len = history.len();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();

        let outcome = compact_on_model_switch(ModelSwitchCompactionRequest {
            prev_provider: "openai".to_string(),
            prev_model: "gpt-x".to_string(),
            new_provider: "custom".to_string(),
            new_model: "custom-model".to_string(),
            client_installed: false,
            enabled: true,
            provider: &provider,
            workspace: temp.path(),
            vt_cfg: None,
            targets: ModelSwitchCompactionTargets {
                history: &mut history,
                session_stats: &mut session_stats,
                context_manager: &mut context_manager,
                session_id: "s",
                thread_id: "t",
                lifecycle_hooks: None,
                harness_emitter: None,
            },
        })
        .await
        .unwrap();

        assert!(matches!(outcome, ModelSwitchCompactionOutcome::SkippedNoClient));
        assert_eq!(history.len(), original_len);
    }

    #[tokio::test]
    async fn disabled_skips_compaction_and_preserves_history() {
        let temp = tempdir().unwrap();
        let provider = StubProvider;
        let mut history = test_history();
        let original_len = history.len();
        let mut session_stats = SessionStats::default();
        let mut context_manager = ContextManager::default_for_test();

        let outcome = compact_on_model_switch(ModelSwitchCompactionRequest {
            prev_provider: "openai".to_string(),
            prev_model: "gpt-x".to_string(),
            new_provider: "anthropic".to_string(),
            new_model: "claude-x".to_string(),
            client_installed: true,
            enabled: false,
            provider: &provider,
            workspace: temp.path(),
            vt_cfg: None,
            targets: ModelSwitchCompactionTargets {
                history: &mut history,
                session_stats: &mut session_stats,
                context_manager: &mut context_manager,
                session_id: "s",
                thread_id: "t",
                lifecycle_hooks: None,
                harness_emitter: None,
            },
        })
        .await
        .unwrap();

        assert!(matches!(outcome, ModelSwitchCompactionOutcome::Disabled));
        assert_eq!(history.len(), original_len);
    }
}
