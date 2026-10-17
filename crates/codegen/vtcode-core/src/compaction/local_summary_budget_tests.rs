use super::*;
use async_trait::async_trait;
use std::sync::Mutex;

struct BudgetRecordingProvider {
    provider_capacity: usize,
    request_ceiling: usize,
    requests: Mutex<Vec<LLMRequest>>,
}

fn request_tokens(request: &LLMRequest) -> usize {
    // Independently count the full request, including prefix and output.
    let system_tokens = request
        .system_prompt
        .as_ref()
        .map_or(0, |text| vtcode_commons::estimate_tokens(text));
    let tool_bytes: usize = request
        .tools
        .as_ref()
        .map_or(0, |tools| tools.iter().map(|tool| serde_json::to_vec(tool).unwrap().len()).sum());
    system_tokens
        + tool_bytes.div_ceil(4)
        + request.messages.iter().map(Message::estimate_tokens).sum::<usize>()
        + request.max_tokens.unwrap_or(0) as usize
}

#[async_trait]
impl LLMProvider for BudgetRecordingProvider {
    fn name(&self) -> &str {
        "summary-budget-test"
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        self.provider_capacity
    }

    async fn generate(&self, request: LLMRequest) -> std::result::Result<LLMResponse, crate::llm::provider::LLMError> {
        let tokens = request_tokens(&request);
        self.requests.lock().unwrap().push(request);
        if self.request_ceiling > 0 && tokens > self.request_ceiling {
            return Err(crate::llm::provider::LLMError::InvalidRequest {
                message: format!("context length exceeded: {tokens} > {}", self.request_ceiling),
                metadata: None,
            });
        }
        Ok(LLMResponse::new("summary-budget-model", "Evidence inspected. Continue the newest task."))
    }

    fn supported_models(&self) -> Vec<String> {
        Vec::new()
    }

    fn validate_request(&self, _request: &LLMRequest) -> std::result::Result<(), crate::llm::provider::LLMError> {
        Ok(())
    }
}

fn history() -> Vec<Message> {
    [700, 1_400, 900, 1_800, 1_100, 1_600]
        .into_iter()
        .enumerate()
        .map(|(index, repeats)| Message::user(format!("observation_{index} ").repeat(repeats)))
        .collect()
}

fn parent() -> CompactionParentContext {
    CompactionParentContext {
        system_prompt: Some(Arc::from("stable-prefix ".repeat(1_500))),
        tools: Some(Arc::new(vec![ToolDefinition::function(
            "read_file".into(),
            "schema-description ".repeat(300),
            json!({"type":"object"}),
        )])),
    }
}

#[tokio::test]
async fn local_summary_budget_reserves_parent_prefix_and_explicit_output_before_first_call() {
    struct BudgetCase {
        provider_capacity: usize,
        session_capacity: usize,
        hierarchical: bool,
    }
    let cases = [
        BudgetCase {
            provider_capacity: 20_000,
            session_capacity: 12_000,
            hierarchical: false,
        },
        BudgetCase {
            provider_capacity: 12_000,
            session_capacity: 20_000,
            hierarchical: false,
        },
        BudgetCase {
            provider_capacity: 20_000,
            session_capacity: 12_000,
            hierarchical: true,
        },
        BudgetCase {
            provider_capacity: 12_000,
            session_capacity: 20_000,
            hierarchical: true,
        },
    ];
    for BudgetCase { provider_capacity, session_capacity, hierarchical } in cases {
        let provider = BudgetRecordingProvider {
            provider_capacity,
            request_ceiling: 12_000,
            requests: Mutex::new(Vec::new()),
        };
        let source = history();
        let original = source.clone();
        let parent = parent();
        let config = CompactionConfig {
            always_summarize: true,
            hierarchical,
            ..Default::default()
        };
        let options = ManualCompactionOptions {
            max_output_tokens: Some(2_000),
            ..Default::default()
        };
        let (_compacted, mode) = compact_history_manual_with_parent_context(
            &provider,
            "summary-budget-model",
            &source,
            &config,
            &options,
            Some(session_capacity),
            Some(&parent),
        )
        .await
        .unwrap();
        assert_eq!(mode, CompactionMode::Local);
        let requests = provider.requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            if hierarchical { 2 } else { 1 },
            "no avoidable capacity retries: provider={provider_capacity}, session={session_capacity}"
        );
        for request in requests.iter() {
            assert!(request_tokens(request) <= 12_000);
            assert_eq!(request.system_prompt, parent.system_prompt);
            assert_eq!(request.tools, parent.tools);
            assert!(matches!(request.tool_choice, Some(ToolChoice::None)));
        }
        assert_eq!(requests.last().unwrap().max_tokens, Some(2_000));
        assert_eq!(source, original);
    }
}

#[tokio::test]
async fn local_summary_budget_rejects_impossible_fixed_prefix_without_provider_call() {
    struct FixedInputCase {
        system_chars: usize,
        output_tokens: u32,
        instruction_chars: usize,
        hierarchical: bool,
    }
    let cases = [
        FixedInputCase {
            system_chars: 80_000,
            output_tokens: 1_000,
            instruction_chars: 0,
            hierarchical: false,
        },
        FixedInputCase {
            system_chars: 100,
            output_tokens: 20_000,
            instruction_chars: 0,
            hierarchical: false,
        },
        FixedInputCase {
            system_chars: 100,
            output_tokens: 20_000,
            instruction_chars: 0,
            hierarchical: true,
        },
        FixedInputCase {
            system_chars: 100,
            output_tokens: 1_000,
            instruction_chars: 100_000,
            hierarchical: false,
        },
        FixedInputCase {
            system_chars: 100,
            output_tokens: 1_000,
            instruction_chars: 100_000,
            hierarchical: true,
        },
    ];
    for FixedInputCase {
        system_chars,
        output_tokens,
        instruction_chars,
        hierarchical,
    } in cases
    {
        let provider = BudgetRecordingProvider {
            provider_capacity: 20_000,
            request_ceiling: 12_000,
            requests: Mutex::new(Vec::new()),
        };
        let parent = CompactionParentContext {
            system_prompt: Some(Arc::from("x".repeat(system_chars))),
            tools: None,
        };
        let config = CompactionConfig {
            always_summarize: true,
            hierarchical,
            ..Default::default()
        };
        let options = ManualCompactionOptions {
            max_output_tokens: Some(output_tokens),
            instructions: (instruction_chars > 0).then(|| "x".repeat(instruction_chars)),
            ..Default::default()
        };
        let error = compact_history_manual_with_parent_context(
            &provider,
            "summary-budget-model",
            &history(),
            &config,
            &options,
            Some(12_000),
            Some(&parent),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("local summary request budget"), "{error:#}");
        assert!(provider.requests.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn local_summary_budget_rejects_protocol_minimum_above_remaining_capacity() {
    for hierarchical in [false, true] {
        let provider = BudgetRecordingProvider {
            provider_capacity: 20_000,
            request_ceiling: 20_000,
            requests: Mutex::new(Vec::new()),
        };
        let parent = CompactionParentContext {
            system_prompt: Some(Arc::from("p ".repeat(16_000))),
            tools: None,
        };
        let assistant_count = if hierarchical { 3_000 } else { 1_000 };
        let source: Vec<_> = std::iter::once(Message::user(String::new()))
            .chain((0..assistant_count).map(|_| Message::assistant("evidence".into())))
            .chain(hierarchical.then(|| Message::user("recent task".into())))
            .collect();
        let config = CompactionConfig {
            always_summarize: true,
            hierarchical,
            ..Default::default()
        };
        let options = ManualCompactionOptions {
            instructions: Some("x".into()),
            ..Default::default()
        };
        let error = compact_history_manual_with_parent_context(
            &provider,
            "summary-budget-model",
            &source,
            &config,
            &options,
            Some(20_000),
            Some(&parent),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("local summary request budget"), "{error:#}");
        assert!(provider.requests.lock().unwrap().is_empty(), "hierarchical={hierarchical}");
    }
}

#[tokio::test]
async fn local_summary_budget_unknown_capacity_preserves_prefix_and_history() {
    let provider = BudgetRecordingProvider {
        provider_capacity: 0,
        request_ceiling: 0,
        requests: Mutex::new(Vec::new()),
    };
    let source = vec![
        Message::user("old observation".into()),
        Message::assistant("new evidence".into()),
    ];
    let parent = parent();
    let config = CompactionConfig { always_summarize: true, ..Default::default() };
    compact_history_manual_with_parent_context(
        &provider,
        "summary-budget-model",
        &source,
        &config,
        &ManualCompactionOptions::default(),
        None,
        Some(&parent),
    )
    .await
    .unwrap();
    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].messages.starts_with(&source));
    assert_eq!(requests[0].system_prompt, parent.system_prompt);
    assert_eq!(requests[0].tools, parent.tools);
}
