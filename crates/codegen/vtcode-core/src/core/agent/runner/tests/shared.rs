#![allow(missing_docs)]

pub use crate::AgentRunner;
pub use crate::config::VTCodeConfig;
pub use crate::config::constants::tools;
pub use crate::config::models::ModelId;
pub use crate::config::types::CapabilityLevel;
pub use crate::core::agent::events::ExecEventRecorder;
pub use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;
pub use crate::core::agent::runner::RunnerSettings;
pub use crate::core::agent::runtime::AgentRuntime;
pub use crate::core::agent::session::AgentSessionState;
pub use crate::core::agent::steering::SteeringMessage;
pub use crate::core::agent::task::{Task, TaskOutcome, TaskResults};
pub use crate::core::agent::types::AgentType;
pub use crate::core::threads::ThreadBootstrap;
pub use crate::exec::events::{
    HarnessEventKind, ItemCompletedEvent, ThreadEvent, ThreadItemDetails, ToolCallStatus,
};
pub use crate::llm::provider::{
    FinishReason, LLMError, LLMProvider, LLMRequest, LLMResponse, ToolCall, ToolChoice,
};
pub use crate::primary_agent::ActivePrimaryAgent;
pub use crate::tool_policy::ToolPolicy;
pub use crate::tools::Tool;
pub use crate::tools::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig};
pub use crate::tools::handlers::{PlanningWorkflowState, TaskTrackerTool};
pub use crate::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};
pub use async_trait::async_trait;
pub use parking_lot::Mutex;
pub use serde_json::json;
pub use std::collections::VecDeque;
pub use std::fs;
pub use std::path::{Path, PathBuf};
pub use std::sync::Arc;
pub use std::time::{Duration, Instant};
pub use tempfile::TempDir;
pub use vtcode_config::ToolProfile;
pub use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};

pub fn provider_tool_names(snapshot: &SessionToolCatalogSnapshot) -> Vec<&str> {
    snapshot
        .snapshot
        .as_ref()
        .map(|tools| tools.iter().map(|tool| tool.function_name()).collect())
        .unwrap_or_default()
}

pub fn assert_provider_exposes_tool(snapshot: &SessionToolCatalogSnapshot, tool_name: &str) {
    let names = provider_tool_names(snapshot);
    assert!(
        names.contains(&tool_name),
        "provider-facing snapshot should include {tool_name}; got {names:?}"
    );
    assert!(
        snapshot.active_tool_names.iter().any(|active_name| active_name == tool_name),
        "active tool names should include {tool_name}"
    );
}

pub fn assert_provider_hides_tool(snapshot: &SessionToolCatalogSnapshot, tool_name: &str) {
    let names = provider_tool_names(snapshot);
    assert!(
        !names.contains(&tool_name),
        "provider-facing snapshot should hide {tool_name}; got {names:?}"
    );
    assert!(
        !snapshot.active_tool_names.iter().any(|active_name| active_name == tool_name),
        "active tool names should hide {tool_name}"
    );
}

pub fn assert_provider_catalogues_inactive_tool(
    snapshot: &SessionToolCatalogSnapshot,
    tool_name: &str,
) {
    let names = provider_tool_names(snapshot);
    assert!(
        names.contains(&tool_name),
        "stable provider catalogue should include {tool_name}; got {names:?}"
    );
    assert!(
        !snapshot.active_tool_names.iter().any(|active_name| active_name == tool_name),
        "active tool names should hide {tool_name}"
    );
}

#[derive(Clone)]
pub struct QueuedProvider {
    pub responses: Arc<Mutex<VecDeque<LLMResponse>>>,
}

impl QueuedProvider {
    pub fn new(responses: Vec<LLMResponse>) -> Self {
        Self { responses: Arc::new(Mutex::new(responses.into())) }
    }
}

#[async_trait]
impl LLMProvider for QueuedProvider {
    fn name(&self) -> &str {
        "queued-test-provider"
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.responses.lock().pop_front().ok_or(LLMError::InvalidRequest {
            message: "QueuedProvider has no queued responses".to_string(),
            metadata: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.3-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct RecordingQueuedProvider {
    name: &'static str,
    supports_native_allowed_tools: bool,
    responses: Arc<Mutex<VecDeque<LLMResponse>>>,
    requests: Arc<Mutex<Vec<LLMRequest>>>,
}

impl RecordingQueuedProvider {
    pub fn new(responses: Vec<LLMResponse>) -> Self {
        Self::with_native_allowed_tools("openai", true, responses)
    }

    pub fn with_name(name: &'static str, responses: Vec<LLMResponse>) -> Self {
        Self::with_native_allowed_tools(name, false, responses)
    }

    fn with_native_allowed_tools(
        name: &'static str,
        supports_native_allowed_tools: bool,
        responses: Vec<LLMResponse>,
    ) -> Self {
        Self {
            name,
            supports_native_allowed_tools,
            responses: Arc::new(Mutex::new(responses.into())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn recorded_requests(&self) -> Vec<LLMRequest> {
        self.requests.lock().clone()
    }
}

#[async_trait]
impl LLMProvider for RecordingQueuedProvider {
    fn name(&self) -> &str {
        self.name
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.requests.lock().push(request);
        self.responses.lock().pop_front().ok_or(LLMError::InvalidRequest {
            message: "RecordingQueuedProvider has no queued responses".to_string(),
            metadata: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.3-codex".to_string()]
    }

    fn supports_native_allowed_tools(&self, _model: &str) -> bool {
        self.supports_native_allowed_tools
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

/// Roles in the orchestrated harness LLM-call lifecycle.
///
/// The harness drives several distinct sub-agents (planner, build, evaluator,
/// replanner) through the same `LLMProvider`. `RoleQueuedProvider` maps each
/// `generate` call to its role via the system prompt so responses are matched
/// by role instead of by flat position. This keeps tests resilient to harness
/// flow changes (e.g. an added build turn between a replan and re-evaluation):
/// when a role's queue is empty, a role-appropriate default is returned instead
/// of erroring on a missing queued response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarnessRole {
    Planner,
    Build,
    Evaluator,
    Replanner,
}

pub fn harness_role_of(request: &LLMRequest) -> HarnessRole {
    let sys = request.system_prompt.as_ref().map(|s| s.as_str()).unwrap_or("");
    if sys.contains("harness replanner") {
        HarnessRole::Replanner
    } else if sys.contains("harness evaluator") {
        HarnessRole::Evaluator
    } else if sys.contains("harness planner") {
        HarnessRole::Planner
    } else {
        HarnessRole::Build
    }
}

#[derive(Clone)]
pub struct RoleQueuedProvider {
    pub planner: Arc<Mutex<VecDeque<LLMResponse>>>,
    pub build: Arc<Mutex<VecDeque<LLMResponse>>>,
    pub evaluator: Arc<Mutex<VecDeque<LLMResponse>>>,
    pub replanner: Arc<Mutex<VecDeque<LLMResponse>>>,
    pub requests: Arc<Mutex<Vec<LLMRequest>>>,
}

impl RoleQueuedProvider {
    pub fn new() -> Self {
        Self {
            planner: Arc::new(Mutex::new(VecDeque::new())),
            build: Arc::new(Mutex::new(VecDeque::new())),
            evaluator: Arc::new(Mutex::new(VecDeque::new())),
            replanner: Arc::new(Mutex::new(VecDeque::new())),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn planner(&mut self, response: LLMResponse) -> &mut Self {
        self.planner.lock().push_back(response);
        self
    }

    pub fn build(&mut self, response: LLMResponse) -> &mut Self {
        self.build.lock().push_back(response);
        self
    }

    pub fn evaluator(&mut self, response: LLMResponse) -> &mut Self {
        self.evaluator.lock().push_back(response);
        self
    }

    pub fn replanner(&mut self, response: LLMResponse) -> &mut Self {
        self.replanner.lock().push_back(response);
        self
    }

    pub fn recorded_requests(&self) -> Vec<LLMRequest> {
        self.requests.lock().clone()
    }
}

fn default_response_for(role: HarnessRole) -> LLMResponse {
    match role {
        HarnessRole::Planner => json_response(planner_response_json("pwd")),
        HarnessRole::Build => text_response("All requested changes have been applied."),
        HarnessRole::Evaluator => {
            json_response(evaluator_response_json("pass", "default evaluator response", 0))
        }
        HarnessRole::Replanner => text_response("Revision 1: task is complete."),
    }
}

#[async_trait]
impl LLMProvider for RoleQueuedProvider {
    fn name(&self) -> &str {
        "openai"
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        self.requests.lock().push(request.clone());
        let role = harness_role_of(&request);
        let queue = match role {
            HarnessRole::Planner => &self.planner,
            HarnessRole::Build => &self.build,
            HarnessRole::Evaluator => &self.evaluator,
            HarnessRole::Replanner => &self.replanner,
        };
        let mut q = queue.lock();
        if let Some(response) = q.pop_front() {
            Ok(response)
        } else {
            Ok(default_response_for(role))
        }
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.3-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

pub fn task(title: &str, id: &str) -> Task {
    Task {
        id: id.to_string(),
        title: title.to_string(),
        description: title.to_string(),
        instructions: None,
    }
}

pub fn text_response(text: &str) -> LLMResponse {
    LLMResponse {
        content: Some(text.to_string()),
        model: "gpt-5.3-codex".to_string(),
        finish_reason: FinishReason::Stop,
        ..Default::default()
    }
}

pub fn json_response(value: serde_json::Value) -> LLMResponse {
    text_response(&value.to_string())
}

pub fn tool_call_response(tool_name: &str, args: serde_json::Value) -> LLMResponse {
    LLMResponse {
        content: Some(format!("Calling {tool_name}")),
        tool_calls: Some(vec![ToolCall::function(
            "call-1".to_string(),
            tool_name.to_string(),
            args.to_string(),
        )]),
        model: "gpt-5.3-codex".to_string(),
        finish_reason: FinishReason::ToolCalls,
        ..Default::default()
    }
}

pub fn planner_response_json(verify_command: &str) -> serde_json::Value {
    json!({
        "spec_markdown": "# Execution Spec\n\nImplement the requested change with a resumable tracker.",
        "contract_markdown": format!(
            "# Execution Contract\n\n## Done Criteria\n- Implement the requested change.\n- Verify with `{}`.\n",
            verify_command
        ),
        "task_title": "Planner seeded task",
        "items": [{
            "description": "Implement the requested change",
            "outcome": "The requested change is implemented and tracked.",
            "verify": [verify_command],
        }]
    })
}

pub fn evaluator_response_json(
    verdict: &str,
    summary: &str,
    high_severity_findings: usize,
) -> serde_json::Value {
    let scores = if verdict.eq_ignore_ascii_case("pass") && high_severity_findings == 0 {
        (5, 5, 5, 5)
    } else {
        (3, 2, 4, 3)
    };
    evaluator_response_json_with_scorecard(verdict, summary, high_severity_findings, scores)
}

pub fn evaluator_response_json_with_scorecard(
    verdict: &str,
    summary: &str,
    high_severity_findings: usize,
    scores: (u8, u8, u8, u8),
) -> serde_json::Value {
    json!({
        "verdict": verdict,
        "summary": summary,
        "high_severity_findings": high_severity_findings,
        "scorecard": {
            "contract_fidelity": scores.0,
            "functionality": scores.1,
            "code_quality": scores.2,
            "verification_integrity": scores.3,
        },
        "findings": [{
            "severity": if high_severity_findings > 0 { "high" } else { "low" },
            "title": summary,
            "detail": summary,
        }],
        "unmet_contract_items": [],
        "residual_risks": [],
        "required_tracker_updates": [],
    })
}

pub fn tool_call_response_with_request_id(
    tool_name: &str,
    args: serde_json::Value,
    request_id: &str,
) -> LLMResponse {
    let mut response = tool_call_response(tool_name, args);
    response.request_id = Some(request_id.to_string());
    response
}

pub fn workspace_root(temp: &TempDir) -> PathBuf {
    temp.path().canonicalize().unwrap_or_else(|_| temp.path().to_path_buf())
}

pub async fn make_runner(temp: &TempDir, vt_cfg: VTCodeConfig, session_id: &str) -> AgentRunner {
    make_runner_for_model(temp, vt_cfg, session_id, ModelId::default()).await
}

pub async fn make_runner_for_model(
    temp: &TempDir,
    vt_cfg: VTCodeConfig,
    session_id: &str,
    model: ModelId,
) -> AgentRunner {
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        model,
        "test-key".to_string(),
        workspace_root(temp),
        session_id.to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(vt_cfg),
        None,
    ))
    .await
    .expect("runner");
    runner.set_quiet(true);
    runner
}

pub async fn seed_tracker(workspace_root: &Path, items: serde_json::Value) {
    let tool = TaskTrackerTool::new(
        workspace_root.to_path_buf(),
        PlanningWorkflowState::new(workspace_root.to_path_buf()),
    );
    tool.execute(json!({
        "action": "create",
        "title": "Harness hardening",
        "items": items,
    }))
    .await
    .expect("seed tracker");
}

pub fn turn_started_count(results: &TaskResults) -> usize {
    results
        .thread_events
        .iter()
        .filter(|event| matches!(event, ThreadEvent::TurnStarted(_)))
        .count()
}

pub fn harness_events(results: &TaskResults) -> Vec<HarnessEventKind> {
    results
        .thread_events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                ThreadItemDetails::Harness(harness) => Some(harness.event.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

pub fn harness_paths(results: &TaskResults, kind: HarnessEventKind) -> Vec<String> {
    results
        .thread_events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                ThreadItemDetails::Harness(harness) if harness.event == kind => {
                    harness.path.clone()
                }
                _ => None,
            },
            _ => None,
        })
        .collect()
}

pub fn completed_tool_invocation_item_id(
    events: &[ThreadEvent],
    tool_call_id: &str,
) -> Option<String> {
    events.iter().find_map(|event| match event {
        ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
            ThreadItemDetails::ToolInvocation(details)
                if details.tool_call_id.as_deref() == Some(tool_call_id) =>
            {
                Some(item.id.clone())
            }
            _ => None,
        },
        _ => None,
    })
}

pub fn completed_tool_output_count(
    events: &[ThreadEvent],
    tool_call_id: &str,
    status: ToolCallStatus,
    call_item_id: &str,
) -> usize {
    events
        .iter()
        .filter(|event| match event {
            ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) => match &item.details {
                ThreadItemDetails::ToolOutput(details) => {
                    details.tool_call_id.as_deref() == Some(tool_call_id)
                        && details.status == status
                        && details.call_id == call_item_id
                }
                _ => false,
            },
            _ => false,
        })
        .count()
}
