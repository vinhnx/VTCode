use super::*;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use vtcode_core::llm::provider::{
    ChoiceDecisionAnswer, ChoiceDecisionRequest, ChoiceDecisionResponse, LLMError, LLMProvider, LLMRequest,
    LLMResponse, Usage,
};

struct ScriptedProvider {
    eligible: bool,
    choice: Option<&'static str>,
    decisions_error: bool,
    decisions_delay: Duration,
    usage: bool,
    calls: Mutex<usize>,
    decisions_inputs: Mutex<Vec<String>>,
}

impl Default for ScriptedProvider {
    fn default() -> Self {
        Self {
            eligible: true,
            choice: Some("SAFE"),
            decisions_error: false,
            decisions_delay: Duration::ZERO,
            usage: true,
            calls: Mutex::default(),
            decisions_inputs: Mutex::default(),
        }
    }
}

fn usage(input: u32, output: u32) -> Usage {
    Usage {
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: input + output,
        cached_prompt_tokens: None,
        cache_read_tokens: None,
        cache_creation_tokens: None,
        iterations: None,
    }
}

#[async_trait]
impl LLMProvider for ScriptedProvider {
    fn name(&self) -> &str {
        "openai"
    }

    fn supports_decisions(&self) -> bool {
        self.eligible
    }

    async fn decide_choice(&self, request: ChoiceDecisionRequest) -> Result<ChoiceDecisionResponse, LLMError> {
        assert_eq!(request.name, "computer_action_safety");
        assert!(
            !request.instructions.contains("Respond with"),
            "typed Decisions must not use generation formatting instructions"
        );
        assert_eq!(request.choices.iter().map(|option| option.value.as_str()).collect::<Vec<_>>(), ["SAFE", "SUSPECT"]);
        *self.calls.lock().unwrap() += 1;
        self.decisions_inputs.lock().unwrap().push(request.input);
        tokio::time::sleep(self.decisions_delay).await;
        if self.decisions_error {
            return Err(LLMError::Provider { message: "HTTP error".into(), metadata: None });
        }
        Ok(ChoiceDecisionResponse {
            answer: self.choice.map(|choice| ChoiceDecisionAnswer {
                name: "computer_action_safety".into(),
                choice: choice.into(),
                confidence: 0.9,
                probabilities: vec![],
            }),
            usage: self.usage.then(|| usage(137, 0)),
        })
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        panic!("review must use Decisions, never generation");
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-6-luna".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

fn summary() -> ComputerBatchSummary {
    ComputerBatchSummary {
        task: "verify the login form submits".to_string(),
        target: ComputerUseTarget::Browser { build_dir: PathBuf::from("/approved/build") },
        recipient: "gpt-6-luna (openai)".to_string(),
        action_kinds: vec!["click".to_string(), "type".to_string()],
        action_count: 2,
    }
}

#[tokio::test]
async fn computer_use_review_safe_proceeds_with_usage() {
    let provider = ScriptedProvider::default();
    let outcome = review_computer_batch(&provider, &summary(), true, REVIEW_TIMEOUT, &CancellationToken::new()).await;
    assert_eq!(outcome.verdict, BatchReviewVerdict::Proceed);
    assert_eq!(outcome.usage.unwrap().total_tokens, 137);
    assert_eq!(*provider.calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn computer_use_review_suspect_halts() {
    let provider = ScriptedProvider { choice: Some("SUSPECT"), ..Default::default() };
    let outcome = review_computer_batch(&provider, &summary(), true, REVIEW_TIMEOUT, &CancellationToken::new()).await;
    assert_eq!(outcome.verdict, BatchReviewVerdict::Halt);
}

#[tokio::test]
async fn computer_use_review_degrades_loudly_without_halting() {
    for (choice, error, expected) in [
        (None, false, "inconclusive"),
        (Some("MAYBE"), false, "inconclusive"),
        (Some("SAFE"), true, "request failed"),
    ] {
        let provider = ScriptedProvider {
            choice,
            decisions_error: error,
            ..Default::default()
        };
        let outcome =
            review_computer_batch(&provider, &summary(), true, REVIEW_TIMEOUT, &CancellationToken::new()).await;
        assert_eq!(outcome.verdict, BatchReviewVerdict::Unreviewed { reason: expected });
    }

    let slow = ScriptedProvider {
        decisions_delay: Duration::from_millis(50),
        ..Default::default()
    };
    let outcome =
        review_computer_batch(&slow, &summary(), true, Duration::from_millis(1), &CancellationToken::new()).await;
    assert_eq!(outcome.verdict, BatchReviewVerdict::Unreviewed { reason: "timed out" });

    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let outcome = review_computer_batch(&provider_for_cancelled(), &summary(), true, REVIEW_TIMEOUT, &cancelled).await;
    assert_eq!(outcome.verdict, BatchReviewVerdict::Unreviewed { reason: "cancelled" });

    fn provider_for_cancelled() -> ScriptedProvider {
        ScriptedProvider {
            decisions_delay: Duration::from_secs(30),
            ..Default::default()
        }
    }
}

#[tokio::test]
async fn computer_use_review_skips_quietly_when_disabled_or_unsupported() {
    let provider = ScriptedProvider::default();
    let outcome = review_computer_batch(&provider, &summary(), false, REVIEW_TIMEOUT, &CancellationToken::new()).await;
    assert_eq!(outcome.verdict, BatchReviewVerdict::Proceed);
    assert_eq!(*provider.calls.lock().unwrap(), 0);

    let ineligible = ScriptedProvider { eligible: false, ..Default::default() };
    let outcome = review_computer_batch(&ineligible, &summary(), true, REVIEW_TIMEOUT, &CancellationToken::new()).await;
    assert_eq!(outcome.verdict, BatchReviewVerdict::Proceed);
    assert_eq!(*ineligible.calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn computer_use_review_evidence_carries_shape_only() {
    let provider = ScriptedProvider::default();
    review_computer_batch(&provider, &summary(), true, REVIEW_TIMEOUT, &CancellationToken::new()).await;
    let inputs = provider.decisions_inputs.lock().unwrap();
    assert_eq!(inputs.len(), 1);
    assert!(inputs[0].contains("/approved/build"));
    assert!(inputs[0].contains("gpt-6-luna (openai)"));
    assert!(inputs[0].contains("Proposed actions (2): [click, type]"));
    assert!(inputs[0].contains("verify the login form submits"));
    assert!(!inputs[0].contains("500"));
}
