use super::*;
use async_trait::async_trait;
use std::sync::Mutex;

struct ScriptedProvider {
    eligible: bool,
    choice: Option<&'static str>,
    decisions_error: bool,
    generation: Option<&'static str>,
    decisions_delay: Duration,
    generation_delay: Duration,
    usage: bool,
    decisions_inputs: Mutex<Vec<String>>,
    generation_requests: Mutex<Vec<uni::LLMRequest>>,
}

impl Default for ScriptedProvider {
    fn default() -> Self {
        Self {
            eligible: true,
            choice: Some("SAFE"),
            decisions_error: false,
            generation: Some("SUSPECT"),
            decisions_delay: Duration::ZERO,
            generation_delay: Duration::ZERO,
            usage: true,
            decisions_inputs: Mutex::default(),
            generation_requests: Mutex::default(),
        }
    }
}

fn usage(input: u32, output: u32) -> uni::Usage {
    uni::Usage {
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
impl uni::LLMProvider for ScriptedProvider {
    fn name(&self) -> &str {
        "openai"
    }
    fn supports_decisions(&self) -> bool {
        self.eligible
    }
    async fn decide_choice(
        &self,
        request: uni::ChoiceDecisionRequest,
    ) -> Result<uni::ChoiceDecisionResponse, uni::LLMError> {
        assert_eq!(request.name, "tool_output_injection");
        assert_eq!(request.choices.iter().map(|option| option.value.as_str()).collect::<Vec<_>>(), ["SAFE", "SUSPECT"]);
        self.decisions_inputs.lock().unwrap().push(request.input);
        tokio::time::sleep(self.decisions_delay).await;
        if self.decisions_error {
            return Err(uni::LLMError::Provider { message: "HTTP error".into(), metadata: None });
        }
        Ok(uni::ChoiceDecisionResponse {
            answer: self.choice.map(|choice| uni::ChoiceDecisionAnswer {
                name: "tool_output_injection".into(),
                choice: choice.into(),
                confidence: 0.1,
                probabilities: vec![],
            }),
            usage: self.usage.then(|| usage(137, 0)),
        })
    }
    async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        self.generation_requests.lock().unwrap().push(request);
        tokio::time::sleep(self.generation_delay).await;
        let Some(content) = self.generation else {
            return Err(uni::LLMError::Provider { message: "generation error".into(), metadata: None });
        };
        Ok(uni::LLMResponse {
            content: Some(content.into()),
            model: "gpt-6-luna".into(),
            tool_calls: None,
            usage: self.usage.then(|| usage(311, 2)),
            finish_reason: uni::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
            compaction: None,
        })
    }
    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-6-luna".into(), "gpt-6-astra".into()]
    }
    fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

fn config() -> CoreAgentConfig {
    let mut config = super::super::tests::runtime_config();
    config.provider = "openai".into();
    config.model = "gpt-6-astra".into();
    config
}

fn permissions(enabled: bool) -> PermissionsConfig {
    let mut permissions = PermissionsConfig::default();
    permissions.auto_permission.use_decisions_probe = enabled;
    permissions.auto_permission.probe_model = "gpt-6-luna".into();
    permissions
}

async fn run(provider: &mut ScriptedProvider, enabled: bool) -> (Result<Option<ProbeWarning>>, SessionStats) {
    let mut stats = SessionStats::default();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let result = probe_tool_output(
        provider,
        &config(),
        None,
        &permissions(enabled),
        "inspect the file",
        "file contents",
        &mut ProbeRuntime { stats: &mut stats, stop: &stop, notify: &notify },
    )
    .await;
    (result, stats)
}

#[tokio::test]
async fn decisions_probe_safe_and_suspect_use_one_request_and_original_advisory() {
    for choice in ["SAFE", "SUSPECT"] {
        let mut provider = ScriptedProvider { choice: Some(choice), ..Default::default() };
        let (result, stats) = run(&mut provider, true).await;
        let warning = result.unwrap();
        assert_eq!(
            warning.as_ref().map(|warning| warning.warning.as_str()),
            (choice == "SUSPECT").then_some(PROBE_WARNING_TEXT)
        );
        assert_eq!(provider.decisions_inputs.lock().unwrap().len(), 1);
        assert!(provider.generation_requests.lock().unwrap().is_empty());
        assert_eq!(stats.total_usage().input_tokens, 137);
        assert_eq!(stats.total_usage().output_tokens, 0);
        assert!((stats.total_cost_usd().unwrap() - 0.0000137).abs() < 1e-12);
    }
}

#[tokio::test]
async fn decisions_probe_inconclusive_and_http_error_fall_back_to_configured_probe_once() {
    for error in [false, true] {
        let mut provider = ScriptedProvider {
            choice: None,
            decisions_error: error,
            ..Default::default()
        };
        let (result, stats) = run(&mut provider, true).await;
        assert!(result.unwrap().is_some());
        let requests = provider.generation_requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].model, "gpt-6-luna");
        assert_eq!(requests[0].messages[0].content.as_text(), provider.decisions_inputs.lock().unwrap()[0]);
        assert_eq!(stats.total_usage().input_tokens, if error { 311 } else { 448 });
        assert_eq!(stats.total_usage().output_tokens, 2);
        if error {
            assert!(stats.total_cost_usd().is_none());
        } else {
            // 137 Decisions input + 311 generation input at $0.10/M,
            // plus 2 generation output at $0.50/M.
            assert!((stats.total_cost_usd().unwrap() - 0.0000458).abs() < 1e-12);
        }
    }
}

#[tokio::test]
async fn decisions_probe_failed_fallback_does_not_retry_main_model() {
    let mut provider = ScriptedProvider {
        choice: None,
        generation: None,
        ..Default::default()
    };
    assert!(run(&mut provider, true).await.0.is_err());
    assert_eq!(provider.decisions_inputs.lock().unwrap().len(), 1);
    assert_eq!(provider.generation_requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn decisions_disabled_and_ineligible_paths_do_not_dispatch_decisions_and_keep_main_retry() {
    for (enabled, eligible) in [(false, true), (true, false)] {
        let mut provider = ScriptedProvider { eligible, generation: None, ..Default::default() };
        assert!(run(&mut provider, enabled).await.0.is_err());
        assert!(provider.decisions_inputs.lock().unwrap().is_empty());
        let requests = provider.generation_requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].model, "gpt-6-luna");
        assert_eq!(requests[1].model, "gpt-6-astra");
    }
}

#[tokio::test]
async fn decisions_probe_missing_usage_and_both_inconclusive_results_do_not_become_zero_cost() {
    let mut provider = ScriptedProvider {
        choice: None,
        generation: Some("unknown"),
        usage: false,
        ..Default::default()
    };
    let (result, stats) = run(&mut provider, true).await;
    assert!(result.is_err());
    assert!(stats.total_cost_usd().is_none());
    assert_eq!(provider.generation_requests.lock().unwrap().len(), 1);
    let mut provider = ScriptedProvider { usage: false, ..Default::default() };
    let (result, stats) = run(&mut provider, true).await;
    assert!(result.unwrap().is_none());
    assert!(stats.total_cost_usd().is_none());
}

#[tokio::test]
async fn decisions_probe_shares_evidence_bounds_with_generation() {
    let mut provider = ScriptedProvider { choice: None, ..Default::default() };
    let history = vec![
        uni::Message::user("一".repeat(300)),
        uni::Message::assistant("excluded".into()),
        uni::Message::user("b".repeat(300)),
    ];
    let context = recent_user_context(&history);
    assert!(context.chars().count() <= 481);
    let mut stats = SessionStats::default();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    probe_tool_output(
        &mut provider,
        &config(),
        None,
        &permissions(true),
        &context,
        &format!("{}TAIL_ATTACK", "é".repeat(2401)),
        &mut ProbeRuntime { stats: &mut stats, stop: &stop, notify: &notify },
    )
    .await
    .unwrap();
    let inputs = provider.decisions_inputs.lock().unwrap();
    let input = &inputs[0];
    assert!(input.contains("[truncated]"));
    assert!(!input.contains("TAIL_ATTACK"));
    assert!(!input.contains("excluded"));
    let output = input.split("Tool output:\n").nth(1).unwrap();
    assert!(output.chars().count() <= 2400);
    assert_eq!(provider.generation_requests.lock().unwrap()[0].messages[0].content.as_text(), input.as_str());
}

#[tokio::test]
async fn decisions_probe_times_out_at_four_seconds_and_fallback_uses_only_remaining_deadline() {
    let mut provider = ScriptedProvider {
        decisions_delay: Duration::from_secs(30),
        generation_delay: Duration::from_secs(30),
        ..Default::default()
    };
    let started = Instant::now();
    assert!(run(&mut provider, true).await.0.is_err());
    let elapsed = started.elapsed();
    assert!(elapsed >= Duration::from_secs(8) && elapsed < Duration::from_secs(10), "elapsed {elapsed:?}");
    assert_eq!(provider.decisions_inputs.lock().unwrap().len(), 1);
    assert_eq!(provider.generation_requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn decisions_probe_cancellation_stops_decisions_without_fallback() {
    let mut provider = ScriptedProvider {
        decisions_delay: Duration::from_secs(30),
        ..Default::default()
    };
    let mut stats = SessionStats::default();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        stop.request_local_cancel();
        notify.notify_waiters();
    };
    let started = Instant::now();
    let mut runtime = ProbeRuntime { stats: &mut stats, stop: &stop, notify: &notify };
    let config = config();
    let permissions = permissions(true);
    let (_, result) = tokio::join!(
        cancel,
        probe_tool_output(&mut provider, &config, None, &permissions, "request", "evidence", &mut runtime)
    );
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(provider.decisions_inputs.lock().unwrap().len(), 1);
    assert!(provider.generation_requests.lock().unwrap().is_empty());
    assert!(stats.total_cost_usd().is_none());
}

#[tokio::test]
async fn decisions_probe_cancellation_stops_generation_fallback() {
    let mut provider = ScriptedProvider {
        choice: None,
        generation_delay: Duration::from_secs(30),
        ..Default::default()
    };
    let mut stats = SessionStats::default();
    let stop = CtrlCState::new();
    let notify = Notify::new();
    let config = config();
    let permissions = permissions(true);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        stop.request_local_cancel();
        notify.notify_waiters();
    };
    let mut runtime = ProbeRuntime { stats: &mut stats, stop: &stop, notify: &notify };
    let (_, result) = tokio::join!(
        cancel,
        probe_tool_output(&mut provider, &config, None, &permissions, "request", "output", &mut runtime)
    );
    assert!(result.is_err());
    assert_eq!(provider.decisions_inputs.lock().unwrap().len(), 1);
    assert_eq!(provider.generation_requests.lock().unwrap().len(), 1);
    assert_eq!(stats.total_usage().input_tokens, 137);
    assert!(stats.total_cost_usd().is_none());
}

#[tokio::test]
async fn decisions_probe_stopped_before_dispatch_makes_zero_requests() {
    for handled in [false, true] {
        let mut provider = ScriptedProvider::default();
        let mut stats = SessionStats::default();
        let stop = CtrlCState::new();
        stop.request_local_cancel();
        if handled {
            stop.mark_cancel_handled();
        }
        let notify = Notify::new();
        let result = probe_tool_output(
            &mut provider,
            &config(),
            None,
            &permissions(true),
            "request",
            "output",
            &mut ProbeRuntime { stats: &mut stats, stop: &stop, notify: &notify },
        )
        .await;
        assert!(result.is_err());
        assert!(provider.decisions_inputs.lock().unwrap().is_empty());
        assert!(provider.generation_requests.lock().unwrap().is_empty());
    }
}

#[test]
fn decisions_suggestion_is_local_once_at_eligible_idle_boundary_and_reevaluates_changes() {
    let provider = ScriptedProvider::default();
    let mut config = VTCodeConfig::default();
    let mut stats = SessionStats::default();
    for (interactive, idle, planning) in [(false, true, false), (true, false, false), (true, true, true)] {
        assert!(!stats.take_decisions_probe_suggestion(decisions_suggestion_applicable(
            interactive,
            idle,
            planning,
            &provider,
            Some(&config)
        )));
    }
    config.permissions.auto_permission.use_decisions_probe = true;
    assert!(!stats.take_decisions_probe_suggestion(decisions_suggestion_applicable(
        true,
        true,
        false,
        &provider,
        Some(&config)
    )));
    config.permissions.auto_permission.use_decisions_probe = false;
    let unsupported = ScriptedProvider { eligible: false, ..Default::default() };
    assert!(!stats.take_decisions_probe_suggestion(decisions_suggestion_applicable(
        true,
        true,
        false,
        &unsupported,
        Some(&config)
    )));
    assert!(stats.take_decisions_probe_suggestion(decisions_suggestion_applicable(
        true,
        true,
        false,
        &provider,
        Some(&config)
    )));
    assert!(!stats.take_decisions_probe_suggestion(decisions_suggestion_applicable(
        true,
        true,
        false,
        &provider,
        Some(&config)
    )));
    assert!(provider.decisions_inputs.lock().unwrap().is_empty());
    assert!(provider.generation_requests.lock().unwrap().is_empty());
}
