use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::fmt::Write;
use vtcode_config::constants::context::TOKEN_BUDGET_HIGH_THRESHOLD;

use crate::llm::provider::{LLMProvider, LLMRequest, Message, MessageRole};
use crate::llm::utils::truncate_to_token_limit;

pub mod summarizer;

const DEFAULT_COMPACTION_TARGET_THRESHOLD: f64 = 0.50;
const DEFAULT_COMPACTION_KEEP_LAST_MESSAGES: usize = 10;
const DEFAULT_RETAINED_USER_MESSAGE_TOKENS: usize = 20_000;
const DEFAULT_RETAINED_USER_MESSAGES: usize = 4;
const SUMMARY_PREFIX: &str = "Previous conversation summary:\n";

/// Compaction configuration for context window management.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Threshold (0.0-1.0) at which to trigger compaction.
    pub trigger_threshold: f64,
    /// Target usage ratio (0.0-1.0) after compaction.
    pub target_threshold: f64,
    /// Prompt for summarization.
    pub summary_prompt: String,
    /// Legacy short-circuit used to skip local compaction for tiny histories.
    pub keep_last_messages: usize,
    /// Total token budget reserved for retaining real user messages verbatim.
    pub retained_user_message_tokens: usize,
    /// Maximum number of recent user messages to retain verbatim.
    pub retained_user_messages: usize,
    /// Force local summarization even for short histories and providers with native compaction.
    pub always_summarize: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            trigger_threshold: TOKEN_BUDGET_HIGH_THRESHOLD,
            target_threshold: DEFAULT_COMPACTION_TARGET_THRESHOLD,
            summary_prompt: "Summarize the conversation so far using this exact structure:\n\n## Goal\n[What the user is trying to accomplish]\n\n## Constraints & Preferences\n- [Requirements, preferences, or constraints from the user]\n\n## Progress\n### Done\n- [Completed work]\n\n### In Progress\n- [Current work]\n\n### Blocked\n- [Blocking issues, if any]\n\n## Key Decisions\n- **[Decision]**: [Reason]\n\n## Next Steps\n1. [Most important next step]\n\n## Critical Context\n- [Facts needed to continue]\n\nKeep it concise and actionable. Always preserve the current task objective and acceptance criteria, file paths that were read or modified, test results and error messages, and decisions with their reasoning."
                .to_string(),
            keep_last_messages: DEFAULT_COMPACTION_KEEP_LAST_MESSAGES,
            retained_user_message_tokens: DEFAULT_RETAINED_USER_MESSAGE_TOKENS,
            retained_user_messages: DEFAULT_RETAINED_USER_MESSAGES,
            always_summarize: false,
        }
    }
}

/// Compact conversation history using the configured summarizer.
pub async fn compact_history(
    provider: &dyn LLMProvider,
    model: &str,
    history: &[Message],
    config: &CompactionConfig,
) -> Result<Vec<Message>> {
    if history.is_empty() {
        return Ok(Vec::new());
    }

    if !config.always_summarize && history.len() <= config.keep_last_messages {
        return Ok(history.to_vec());
    }

    if !config.always_summarize && provider.supports_responses_compaction(model) {
        return provider
            .compact_history(model, history)
            .await
            .context("Failed to compact history via Responses compact endpoint");
    }

    let summary_prompt = build_summary_prompt(history, &config.summary_prompt);
    let request = LLMRequest {
        messages: vec![Message::user(summary_prompt)],
        model: model.to_string(),
        ..Default::default()
    };

    let response = provider
        .generate(request)
        .await
        .context("Failed to generate compaction summary")?;

    let summary = response.content.unwrap_or_default().trim().to_string();
    Ok(build_local_compacted_history(
        history,
        &summary,
        config.retained_user_message_tokens,
        config.retained_user_messages,
    ))
}

fn build_summary_prompt(history: &[Message], instructions: &str) -> String {
    let mut formatted = String::new();
    let now: DateTime<Utc> = Utc::now();
    let _ = writeln!(
        &mut formatted,
        "Summary requested at {}.\n{}",
        now.to_rfc3339(),
        instructions
    );

    for message in history {
        let role = match message.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        let content = message.content.as_text();
        if content.trim().is_empty() {
            continue;
        }
        let _ = writeln!(&mut formatted, "\n[{}]\n{}", role, content.trim());
    }

    formatted
}

fn build_local_compacted_history(
    history: &[Message],
    summary: &str,
    retained_user_message_tokens: usize,
    retained_user_messages: usize,
) -> Vec<Message> {
    let retained_users = collect_retained_user_messages(
        history,
        retained_user_message_tokens,
        retained_user_messages,
    );
    let mut new_history = Vec::with_capacity(retained_users.len().saturating_add(1));
    new_history.push(Message::system(format!(
        "{SUMMARY_PREFIX}{}",
        summary.trim()
    )));
    new_history.extend(retained_users);
    new_history
}

fn collect_retained_user_messages(
    history: &[Message],
    token_budget: usize,
    max_messages: usize,
) -> Vec<Message> {
    if token_budget == 0 || max_messages == 0 {
        return Vec::new();
    }

    let mut kept = Vec::new();
    let mut remaining = token_budget;

    for message in history.iter().rev() {
        if kept.len() >= max_messages {
            break;
        }
        if !is_real_user_message(message) {
            continue;
        }

        let estimated = message.estimate_tokens();
        if estimated <= remaining {
            kept.push(message.clone());
            remaining = remaining.saturating_sub(estimated);
            continue;
        }

        if let Some(truncated) = truncate_user_message(message, remaining) {
            kept.push(truncated);
        }
        break;
    }

    kept.reverse();
    kept
}

fn is_real_user_message(message: &Message) -> bool {
    message.role == MessageRole::User && !message.content.trim().is_empty()
}

fn truncate_user_message(message: &Message, token_budget: usize) -> Option<Message> {
    if token_budget <= 4 {
        return None;
    }

    let available_content_tokens = token_budget.saturating_sub(4);
    let truncated =
        truncate_to_token_limit(message.content.as_text().as_ref(), available_content_tokens);
    let trimmed = truncated.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(Message::user(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{CompactionConfig, compact_history};
    use crate::llm::provider::{
        LLMError, LLMProvider, LLMRequest, LLMResponse, Message, MessageRole,
    };
    use async_trait::async_trait;

    struct StubProvider;

    struct NativeCompactionProvider;

    #[async_trait]
    impl LLMProvider for StubProvider {
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
    }

    #[async_trait]
    impl LLMProvider for NativeCompactionProvider {
        fn name(&self) -> &str {
            "native"
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

        fn supports_responses_compaction(&self, _model: &str) -> bool {
            true
        }

        async fn compact_history(
            &self,
            _model: &str,
            _history: &[Message],
        ) -> Result<Vec<Message>, LLMError> {
            Ok(vec![Message::system("provider compacted".to_string())])
        }
    }

    #[tokio::test]
    async fn compact_history_rebuilds_history_around_summary_and_users() {
        let history = vec![
            Message::assistant("setup".to_string()),
            Message::user("first request".to_string()),
            Message::assistant("working".to_string()),
            Message::tool_response("call-1".to_string(), "done".to_string()),
            Message::user("second request".to_string()),
            Message::assistant("final reply".to_string()),
        ];
        let config = CompactionConfig {
            always_summarize: true,
            ..CompactionConfig::default()
        };

        let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
            .await
            .expect("compacted history");

        assert_eq!(compacted.len(), 3);
        assert_eq!(
            compacted[0].content.as_text(),
            "Previous conversation summary:\nsummary"
        );
        assert_eq!(compacted[1].content.as_text(), "first request");
        assert_eq!(compacted[2].content.as_text(), "second request");
        assert!(compacted.iter().all(|message| {
            message.role == MessageRole::System || message.role == MessageRole::User
        }));
    }

    #[tokio::test]
    async fn compact_history_truncates_oldest_retained_user_message_to_budget() {
        let history = vec![
            Message::user("alpha beta gamma delta epsilon zeta".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("newest request".to_string()),
        ];
        let config = CompactionConfig {
            always_summarize: true,
            retained_user_message_tokens: 8,
            ..CompactionConfig::default()
        };

        let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
            .await
            .expect("compacted history");

        assert_eq!(compacted.len(), 2);
        assert_eq!(compacted[1].content.as_text(), "newest request");
    }

    #[tokio::test]
    async fn compact_history_caps_retained_user_message_count() {
        let history = vec![
            Message::user("first request".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("second request".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("third request".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("fourth request".to_string()),
            Message::assistant("ack".to_string()),
            Message::user("fifth request".to_string()),
        ];
        let config = CompactionConfig {
            always_summarize: true,
            retained_user_messages: 4,
            ..CompactionConfig::default()
        };

        let compacted = compact_history(&StubProvider, "stub-model", &history, &config)
            .await
            .expect("compacted history");

        let retained = compacted
            .iter()
            .skip(1)
            .map(|message| message.content.as_text().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            retained,
            vec![
                "second request".to_string(),
                "third request".to_string(),
                "fourth request".to_string(),
                "fifth request".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn compact_history_forces_local_summary_when_always_summarize_is_enabled() {
        let history = vec![
            Message::user("first request".to_string()),
            Message::assistant("working".to_string()),
            Message::user("second request".to_string()),
        ];
        let config = CompactionConfig {
            always_summarize: true,
            ..CompactionConfig::default()
        };

        let compacted = compact_history(&NativeCompactionProvider, "stub-model", &history, &config)
            .await
            .expect("compacted history");

        assert_eq!(compacted.len(), 3);
        assert_eq!(
            compacted[0].content.as_text(),
            "Previous conversation summary:\nsummary"
        );
        assert_eq!(compacted[1].content.as_text(), "first request");
        assert_eq!(compacted[2].content.as_text(), "second request");
    }

    #[test]
    fn default_summary_prompt_preserves_required_compaction_context() {
        let prompt = CompactionConfig::default().summary_prompt;

        assert!(prompt.contains("acceptance criteria"));
        assert!(prompt.contains("file paths that were read or modified"));
        assert!(prompt.contains("test results and error messages"));
        assert!(prompt.contains("decisions with their reasoning"));
    }
}
