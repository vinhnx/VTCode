use anyhow::Result;
use vtcode_config::constants::context::TOKEN_BUDGET_HIGH_THRESHOLD;
use vtcode_core::compaction::CompactionConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::llm::provider::{LLMProvider, Message};

use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::state::SessionStats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CompactionOutcome {
    pub original_len: usize,
    pub compacted_len: usize,
}

fn configured_compaction_threshold(
    vt_cfg: Option<&VTCodeConfig>,
    provider: &dyn LLMProvider,
    model: &str,
) -> Option<usize> {
    let context_size = provider.effective_context_size(model);
    let configured_threshold =
        vt_cfg.and_then(|cfg| cfg.agent.harness.auto_compaction_threshold_tokens);
    let configured_threshold = configured_threshold.filter(|threshold| *threshold > 0);
    let derived_threshold = if context_size > 0 {
        Some(((context_size as f64) * TOKEN_BUDGET_HIGH_THRESHOLD).round() as u64)
    } else {
        None
    };

    configured_threshold
        .or(derived_threshold)
        .map(|threshold| threshold.max(1))
        .map(|threshold| {
            if context_size > 0 {
                threshold.min(context_size as u64)
            } else {
                threshold
            }
        })
        .and_then(|threshold| usize::try_from(threshold).ok())
}

pub(crate) async fn compact_history_in_place(
    provider: &dyn LLMProvider,
    model: &str,
    vt_cfg: Option<&VTCodeConfig>,
    history: &mut Vec<Message>,
    session_stats: &mut SessionStats,
    context_manager: &mut ContextManager,
) -> Result<Option<CompactionOutcome>> {
    let original_len = history.len();
    let compacted = vtcode_core::compaction::compact_history(
        provider,
        model,
        history,
        &CompactionConfig::default(),
    )
    .await?;

    if compacted == *history {
        return Ok(None);
    }

    *history = compacted;
    session_stats.clear_previous_response_chain();
    context_manager
        .cap_token_usage_after_compaction(configured_compaction_threshold(vt_cfg, provider, model));

    Ok(Some(CompactionOutcome {
        original_len,
        compacted_len: history.len(),
    }))
}

pub(crate) async fn maybe_auto_compact_history(
    provider: &dyn LLMProvider,
    model: &str,
    vt_cfg: Option<&VTCodeConfig>,
    history: &mut Vec<Message>,
    session_stats: &mut SessionStats,
    context_manager: &mut ContextManager,
) -> Result<Option<CompactionOutcome>> {
    let Some(vt_cfg) = vt_cfg else {
        return Ok(None);
    };

    if !vt_cfg.agent.harness.auto_compaction_enabled
        || provider.supports_responses_compaction(model)
    {
        return Ok(None);
    }

    let Some(compact_threshold) = configured_compaction_threshold(Some(vt_cfg), provider, model)
    else {
        return Ok(None);
    };

    if context_manager.current_token_usage() < compact_threshold {
        return Ok(None);
    }

    compact_history_in_place(
        provider,
        model,
        Some(vt_cfg),
        history,
        session_stats,
        context_manager,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{compact_history_in_place, maybe_auto_compact_history};
    use crate::agent::runloop::unified::context_manager::ContextManager;
    use crate::agent::runloop::unified::state::SessionStats;
    use async_trait::async_trait;
    use hashbrown::HashMap;
    use tokio::sync::RwLock;
    use vtcode_commons::llm::Usage;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, Message};

    struct LocalCompactionProvider;

    #[async_trait]
    impl LLMProvider for LocalCompactionProvider {
        fn name(&self) -> &str {
            "stub"
        }

        async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
            Ok(LLMResponse::new("stub-model", "summary"))
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
        (0..12)
            .map(|index| Message::user(format!("message-{index}")))
            .collect()
    }

    fn test_context_manager() -> ContextManager {
        ContextManager::new(
            "You are VT Code.".to_string(),
            (),
            std::sync::Arc::new(RwLock::new(HashMap::new())),
            None,
        )
    }

    #[tokio::test]
    async fn manual_compaction_succeeds_without_server_side_support() {
        let provider = LocalCompactionProvider;
        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        session_stats.set_previous_response_chain("stub", "stub-model", Some("resp_123"));
        let mut context_manager = test_context_manager();
        context_manager.update_token_usage(&Some(Usage {
            prompt_tokens: 900,
            completion_tokens: 10,
            total_tokens: 910,
            ..Usage::default()
        }));

        let outcome = compact_history_in_place(
            &provider,
            "stub-model",
            None,
            &mut history,
            &mut session_stats,
            &mut context_manager,
        )
        .await
        .expect("manual compaction succeeds")
        .expect("history should compact");

        assert_eq!(outcome.original_len, 12);
        assert_eq!(outcome.compacted_len, 11);
        assert!(
            history[0]
                .content
                .as_text()
                .contains("Previous conversation summary")
        );
        assert_eq!(
            session_stats.previous_response_id_for("stub", "stub-model"),
            None
        );
        assert!(context_manager.current_token_usage() < 900);
    }

    #[tokio::test]
    async fn auto_compaction_replaces_history_and_clears_response_chain() {
        let provider = LocalCompactionProvider;
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.harness.auto_compaction_enabled = true;
        vt_cfg.agent.harness.auto_compaction_threshold_tokens = Some(700);

        let mut history = test_history();
        let mut session_stats = SessionStats::default();
        session_stats.set_previous_response_chain("stub", "stub-model", Some("resp_123"));
        let mut context_manager = test_context_manager();
        context_manager.update_token_usage(&Some(Usage {
            prompt_tokens: 900,
            completion_tokens: 10,
            total_tokens: 910,
            ..Usage::default()
        }));

        let outcome = maybe_auto_compact_history(
            &provider,
            "stub-model",
            Some(&vt_cfg),
            &mut history,
            &mut session_stats,
            &mut context_manager,
        )
        .await
        .expect("auto compaction succeeds")
        .expect("history should compact");

        assert_eq!(outcome.original_len, 12);
        assert_eq!(outcome.compacted_len, 11);
        assert!(
            history[0]
                .content
                .as_text()
                .contains("Previous conversation summary")
        );
        assert_eq!(
            session_stats.previous_response_id_for("stub", "stub-model"),
            None
        );
        assert!(context_manager.current_token_usage() < 700);
    }
}
