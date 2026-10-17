//! Agent Trajectory Interchange Format (ATIF) types and builder.
//!
//! Implements the [ATIF specification](https://github.com/laude-institute/harbor/blob/main/docs/rfcs/0001-trajectory-format.md)
//! v1.4 for logging complete agent interaction histories in a standardized,
//! JSON-based format usable across debugging, visualization, SFT, and RL
//! pipelines.
//!
//! # Overview
//!
//! ATIF provides a complete session trajectory: user messages, agent responses,
//! tool executions, observations, and per-step/aggregate LLM metrics. The
//! [`AtifTrajectoryBuilder`] converts live [`ThreadEvent`]
//! streams into a finished [`Trajectory`].
//!
//! # Example
//!
//! ```rust
//! use vtcode_exec_events::atif::*;
//!
//! let agent = AtifAgent::new("vtcode", env!("CARGO_PKG_VERSION"));
//! let mut builder = AtifTrajectoryBuilder::new(agent);
//!
//! // Feed ThreadEvents as they arrive …
//! // builder.process_event(&event);
//!
//! let trajectory = builder.finish(None);
//! let json = serde_json::to_string_pretty(&trajectory).unwrap();
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ThreadEvent, ThreadItemDetails, ToolCallStatus};

/// Current ATIF schema version supported by this implementation.
const ATIF_SCHEMA_VERSION: &str = "ATIF-v1.4";

// ============================================================================
// Core ATIF Types
// ============================================================================

/// Root-level ATIF trajectory object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    /// ATIF schema version (e.g., "ATIF-v1.4").
    schema_version: String,
    /// Unique identifier for the entire agent run.
    session_id: String,
    /// Agent configuration for this trajectory.
    agent: AtifAgent,
    /// Ordered interaction steps.
    steps: Vec<Step>,
    /// Optional developer notes.
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    /// Aggregate metrics for the full trajectory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_metrics: Option<FinalMetrics>,
    /// Optional custom root-level metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<Value>,
}

/// Agent configuration metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtifAgent {
    /// Agent system name (e.g., "vtcode").
    name: String,
    /// Agent system version.
    version: String,
    /// Default LLM model used. Step-level `model_name` overrides this.
    #[serde(skip_serializing_if = "Option::is_none")]
    model_name: Option<String>,
    /// Optional custom agent metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<Value>,
}

impl AtifAgent {
    /// Create a new agent descriptor.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            model_name: None,
            extra: None,
        }
    }

    /// Create a vtcode agent descriptor using the crate version.
    pub fn vtcode() -> Self {
        Self::new("vtcode", env!("CARGO_PKG_VERSION"))
    }

    /// Set the default model name.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model_name = Some(model.into());
        self
    }
}

/// The originator of a step.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StepSource {
    /// System prompt or system-initiated operation.
    System,
    /// User message.
    User,
    /// Agent response.
    Agent,
}

/// Individual interaction step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Ordinal index (starting from 1).
    step_id: u64,
    /// ISO 8601 timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    timestamp: Option<String>,
    /// Originator of this step.
    source: StepSource,
    /// LLM model used for this step (agent steps only).
    #[serde(skip_serializing_if = "Option::is_none")]
    model_name: Option<String>,
    /// Step content — text message or array.
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    /// Agent internal reasoning content.
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    /// Tool/function invocations (agent steps only).
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<AtifToolCall>>,
    /// Environment feedback after actions.
    #[serde(skip_serializing_if = "Option::is_none")]
    observation: Option<Observation>,
    /// LLM operational metrics (agent steps only).
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics: Option<StepMetrics>,
    /// Custom step-level metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<Value>,
}

impl Step {
    /// Create a user step.
    fn user(step_id: u64, message: impl Into<String>) -> Self {
        Self {
            step_id,
            timestamp: Some(Utc::now().to_rfc3339()),
            source: StepSource::User,
            model_name: None,
            message: Some(message.into()),
            reasoning_content: None,
            tool_calls: None,
            observation: None,
            metrics: None,
            extra: None,
        }
    }

    /// Create an agent step.
    fn agent(step_id: u64, message: impl Into<String>) -> Self {
        Self {
            step_id,
            timestamp: Some(Utc::now().to_rfc3339()),
            source: StepSource::Agent,
            model_name: None,
            message: Some(message.into()),
            reasoning_content: None,
            tool_calls: None,
            observation: None,
            metrics: None,
            extra: None,
        }
    }

    /// Create a system step.
    fn system(step_id: u64, message: impl Into<String>) -> Self {
        Self {
            step_id,
            timestamp: Some(Utc::now().to_rfc3339()),
            source: StepSource::System,
            model_name: None,
            message: Some(message.into()),
            reasoning_content: None,
            tool_calls: None,
            observation: None,
            metrics: None,
            extra: None,
        }
    }
}

/// Structured tool/function invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtifToolCall {
    /// Unique identifier for the tool call.
    tool_call_id: String,
    /// Function/tool name.
    function_name: String,
    /// Arguments passed to the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<Value>,
}

/// Environment feedback container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    /// Results from tool calls or system operations.
    results: Vec<ObservationResult>,
}

/// Individual observation result tied to a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationResult {
    /// Identifier of the originating tool call.
    source_call_id: String,
    /// Content/output of the observation.
    content: String,
}

/// Per-step LLM operational metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepMetrics {
    /// Total input tokens for this step (cached + non-cached).
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_tokens: Option<u64>,
    /// Completion tokens generated.
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_tokens: Option<u64>,
    /// Subset of prompt_tokens that were cache hits.
    #[serde(skip_serializing_if = "Option::is_none")]
    cached_tokens: Option<u64>,
    /// Estimated cost in USD for this step.
    #[serde(skip_serializing_if = "Option::is_none")]
    cost_usd: Option<f64>,
    /// Log probabilities for completion tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    logprobs: Option<Vec<f64>>,
    /// Completion token IDs for RL training.
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_token_ids: Option<Vec<u64>>,
    /// Prompt token IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_token_ids: Option<Vec<u64>>,
    /// Custom metrics.
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<Value>,
}

impl StepMetrics {
    /// Create metrics from vtcode Usage.
    fn from_usage(usage: &crate::Usage) -> Self {
        Self {
            prompt_tokens: Some(usage.input_tokens),
            completion_tokens: Some(usage.output_tokens),
            cached_tokens: if usage.cached_input_tokens > 0 {
                Some(usage.cached_input_tokens)
            } else {
                None
            },
            cost_usd: None,
            logprobs: None,
            completion_token_ids: None,
            prompt_token_ids: None,
            extra: if usage.cache_creation_tokens > 0 {
                Some(serde_json::json!({
                    "cache_creation_tokens": usage.cache_creation_tokens
                }))
            } else {
                None
            },
        }
    }
}

/// Trajectory-level aggregate metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FinalMetrics {
    /// Sum of all prompt tokens across steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_prompt_tokens: Option<u64>,
    /// Sum of all completion tokens across steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_completion_tokens: Option<u64>,
    /// Sum of all cached tokens across steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cached_tokens: Option<u64>,
    /// Total estimated cost in USD.
    #[serde(skip_serializing_if = "Option::is_none")]
    total_cost_usd: Option<f64>,
    /// Total number of steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    total_steps: Option<u64>,
    /// Custom aggregate metrics.
    #[serde(skip_serializing_if = "Option::is_none")]
    extra: Option<Value>,
}

// ============================================================================
// Builder — converts live ThreadEvent streams into ATIF Trajectory
// ============================================================================

/// Stateful collector that converts a live [`ThreadEvent`] stream into an
/// ATIF-compliant [`Trajectory`].
///
/// Feed events via [`process_event`](Self::process_event) (timestamps at
/// observation time) or [`process_event_at`](Self::process_event_at)
/// (deterministic timestamps for tests). Call [`finish`](Self::finish) to
/// produce the final trajectory.
pub struct AtifTrajectoryBuilder {
    completed_at: Option<String>,
    agent: AtifAgent,
    session_id: Option<String>,
    steps: Vec<Step>,
    next_step_id: u64,
    // Running token accumulators for final metrics
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_cached_tokens: u64,
    num_turns: usize,
    /// Whether any per-turn `TurnCompleted`/`TurnFailed` usage was observed.
    /// Guards the `ThreadCompleted` aggregate from double-counting usage.
    saw_per_turn_usage: bool,
    /// Pending tool invocations awaiting matching ToolOutput.
    pending_tool_calls: Vec<PendingToolCall>,
}

struct PendingToolCall {
    call_id: String,
    tool_call_id: Option<String>,
    tool_name: String,
    arguments: Option<Value>,
    timestamp: String,
}

impl AtifTrajectoryBuilder {
    /// Create a new builder for the given agent.
    pub fn new(agent: AtifAgent) -> Self {
        Self {
            agent,
            completed_at: None,
            session_id: None,
            steps: Vec::new(),
            next_step_id: 1,
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_cached_tokens: 0,
            num_turns: 0,
            saw_per_turn_usage: false,
            pending_tool_calls: Vec::new(),
        }
    }

    /// Set the session ID explicitly. If not set, it will be derived from
    /// `ThreadStarted` or `ThreadCompleted` events.
    pub fn set_session_id(&mut self, id: impl Into<String>) {
        self.session_id = Some(id.into());
    }

    /// Process a thread event using the current wall-clock time.
    pub fn process_event(&mut self, event: &ThreadEvent) {
        self.process_event_at(event, Utc::now());
    }

    /// Process a thread event with an explicit timestamp (for deterministic tests).
    pub fn process_event_at(&mut self, event: &ThreadEvent, ts: DateTime<Utc>) {
        let first_step = self.steps.len();
        let ts_str = ts.to_rfc3339();
        match event {
            ThreadEvent::ThreadStarted(e) => {
                if self.session_id.is_none() {
                    self.session_id = Some(e.thread_id.clone());
                }
            }
            ThreadEvent::ThreadCompleted(e) => {
                self.completed_at = e.completed_at.clone();
                if self.session_id.is_none() {
                    self.session_id = Some(e.session_id.clone());
                }
                self.num_turns = e.num_turns;
                // `ThreadCompleted` carries the *aggregate* usage, which equals
                // the sum of per-turn usage already accumulated by the
                // `TurnCompleted`/`TurnFailed` arms. Only fall back to it when
                // no per-turn usage was observed (e.g. a harness that emits
                // only the thread aggregate), otherwise totals double-count.
                if !self.saw_per_turn_usage {
                    self.total_input_tokens = self.total_input_tokens.saturating_add(e.usage.input_tokens);
                    self.total_output_tokens = self.total_output_tokens.saturating_add(e.usage.output_tokens);
                    self.total_cached_tokens = self.total_cached_tokens.saturating_add(e.usage.cached_input_tokens);
                }
            }
            ThreadEvent::TurnCompleted(e) => {
                self.saw_per_turn_usage = true;
                self.total_input_tokens = self.total_input_tokens.saturating_add(e.usage.input_tokens);
                self.total_output_tokens = self.total_output_tokens.saturating_add(e.usage.output_tokens);
                self.total_cached_tokens = self.total_cached_tokens.saturating_add(e.usage.cached_input_tokens);
                self.num_turns += 1;

                let mut step = Step::system(self.next_step_id, "turn_completed");
                step.timestamp = Some(ts_str);
                step.metrics = Some(StepMetrics::from_usage(&e.usage));
                if !e.in_progress_exec_sessions.is_empty() {
                    step.extra = Some(serde_json::json!({
                        "in_progress_exec_sessions": e.in_progress_exec_sessions,
                    }));
                }
                self.push_step(step);
            }
            ThreadEvent::TurnFailed(e) => {
                if let Some(usage) = &e.usage {
                    self.saw_per_turn_usage = true;
                    self.total_input_tokens = self.total_input_tokens.saturating_add(usage.input_tokens);
                    self.total_output_tokens = self.total_output_tokens.saturating_add(usage.output_tokens);
                    self.total_cached_tokens = self.total_cached_tokens.saturating_add(usage.cached_input_tokens);
                }
                let mut step = Step::system(self.next_step_id, &e.message);
                step.timestamp = Some(ts_str);
                step.metrics = e.usage.as_ref().map(StepMetrics::from_usage);
                self.push_step(step);
            }
            ThreadEvent::TurnBlocked(e) => {
                if let Some(usage) = &e.usage {
                    self.saw_per_turn_usage = true;
                    self.total_input_tokens = self.total_input_tokens.saturating_add(usage.input_tokens);
                    self.total_output_tokens = self.total_output_tokens.saturating_add(usage.output_tokens);
                    self.total_cached_tokens = self.total_cached_tokens.saturating_add(usage.cached_input_tokens);
                }
                let mut step = Step::system(self.next_step_id, &e.message);
                step.timestamp = Some(ts_str);
                step.metrics = e.usage.as_ref().map(StepMetrics::from_usage);
                step.extra = Some(serde_json::json!({
                    "last_tool": e.last_tool,
                    "blocked_streak": e.blocked_streak,
                    "blocked_total": e.blocked_total,
                    "consecutive_cap": e.consecutive_cap,
                    "total_cap": e.total_cap,
                    "recovery_active": e.recovery_active,
                }));
                self.push_step(step);
            }
            ThreadEvent::ItemCompleted(e) => {
                self.process_item_completed(&e.item.id, &e.item.details, &ts_str);
            }
            ThreadEvent::ThreadCompactBoundary(e) => {
                let msg = format!(
                    "context_compaction: {} messages -> {} messages ({})",
                    e.original_message_count,
                    e.compacted_message_count,
                    e.trigger.as_str()
                );
                let mut step = Step::system(self.next_step_id, msg);
                step.timestamp = Some(ts_str);
                self.push_step(step);
            }
            ThreadEvent::ContextReset(e) => {
                let msg = format!(
                    "context_reset: {}% context used; plan preserved: {}; tool budget reset: {}",
                    e.previous_context_usage_percent, e.plan_preserved, e.tool_budget_reset
                );
                let mut step = Step::system(self.next_step_id, msg);
                step.timestamp = Some(ts_str);
                step.extra = Some(serde_json::json!({
                    "thread_id": e.thread_id,
                    "turn_id": e.turn_id,
                    "trigger": e.trigger,
                    "plan_preserved": e.plan_preserved,
                    "previous_context_usage_percent": e.previous_context_usage_percent,
                    "tool_budget_reset": e.tool_budget_reset,
                }));
                self.push_step(step);
            }
            ThreadEvent::Error(e) => {
                let mut step = Step::system(self.next_step_id, &e.message);
                step.timestamp = Some(ts_str);
                self.push_step(step);
            }
            // Skip streaming/lifecycle events that don't map to ATIF steps
            ThreadEvent::TurnStarted(e) => {
                if let Some(context) = &e.context {
                    let mut step = Step::agent(self.next_step_id, context.goal.as_deref().unwrap_or("Turn started"));
                    step.source = StepSource::User;
                    step.timestamp = Some(context.timestamp.clone());
                    step.extra = Some(serde_json::json!({"execution_context": context}));
                    self.push_step(step);
                }
            }
            ThreadEvent::MatrixUpdated(_)
            | ThreadEvent::ItemStarted(_)
            | ThreadEvent::ItemUpdated(_)
            | ThreadEvent::PlanDelta(_)
            | ThreadEvent::PlanApprovalRequested(_)
            | ThreadEvent::PlanApprovalResolved(_)
            | ThreadEvent::PermissionRequested(_)
            | ThreadEvent::PermissionResolved(_)
            | ThreadEvent::Interjected(_)
            | ThreadEvent::Unknown => {}
        }
        for step in self.steps.iter_mut().skip(first_step) {
            let context = match event {
                ThreadEvent::ItemCompleted(e) => e.item.context.as_deref(),
                _ => None,
            };
            if let Some(context) = context {
                step.timestamp = Some(context.timestamp.clone());
                let extra = step.extra.get_or_insert_with(|| serde_json::json!({}));
                if let Some(extra) = extra.as_object_mut() {
                    let _ = extra.insert("item_context".into(), serde_json::json!(context));
                }
            }
            match event {
                ThreadEvent::TurnCompleted(e) => {
                    if let Some(ts) = &e.completed_at {
                        step.timestamp = Some(ts.to_string());
                    }
                }
                ThreadEvent::TurnFailed(e) => {
                    if let Some(ts) = &e.completed_at {
                        step.timestamp = Some(ts.to_string());
                    }
                }
                ThreadEvent::TurnBlocked(e) => {
                    if let Some(ts) = &e.completed_at {
                        step.timestamp = Some(ts.clone());
                    }
                }
                _ => {}
            }
        }
    }

    fn process_item_completed(&mut self, item_id: &str, details: &ThreadItemDetails, ts: &str) {
        match details {
            ThreadItemDetails::Decision(d) => {
                let mut step = Step::agent(self.next_step_id, &d.summary);
                step.timestamp = Some(ts.to_owned());
                step.extra = Some(
                    serde_json::json!({"vtcode_item_type": "decision", "public_rationale": d.rationale, "alternatives": d.alternatives, "evidence_ids": d.evidence_ids}),
                );
                self.push_step(step);
            }
            ThreadItemDetails::AgentMessage(msg) => {
                let mut step = Step::agent(self.next_step_id, &msg.text);
                step.timestamp = Some(ts.to_string());
                self.push_step(step);
            }
            ThreadItemDetails::Plan(plan) => {
                let mut step = Step::agent(self.next_step_id, &plan.text);
                step.timestamp = Some(ts.to_string());
                step.extra = Some(serde_json::json!({ "vtcode_item_type": "plan" }));
                self.push_step(step);
            }
            ThreadItemDetails::Reasoning(r) => {
                let mut step = Step::agent(self.next_step_id, "");
                step.timestamp = Some(ts.to_string());
                step.reasoning_content = Some(r.text.clone());
                step.message = None;
                self.push_step(step);
            }
            ThreadItemDetails::ToolInvocation(inv) => {
                // Buffer the invocation; we'll pair it with the ToolOutput
                self.pending_tool_calls.push(PendingToolCall {
                    call_id: item_id.to_string(),
                    tool_call_id: inv.tool_call_id.clone(),
                    tool_name: inv.tool_name.clone(),
                    arguments: inv.arguments.clone(),
                    timestamp: ts.to_string(),
                });
            }
            ThreadItemDetails::ToolOutput(output) => {
                // Find the matching pending invocation
                let pending_idx = self.pending_tool_calls.iter().position(|p| p.call_id == output.call_id);

                let (tool_name, arguments, tool_call_id, inv_ts) = if let Some(idx) = pending_idx {
                    let p = self.pending_tool_calls.remove(idx);
                    (p.tool_name, p.arguments, p.tool_call_id, p.timestamp)
                } else {
                    ("unknown".to_string(), None, output.tool_call_id.clone(), ts.to_string())
                };

                let call_id = tool_call_id.clone().unwrap_or_else(|| output.call_id.clone());

                let mut step = Step::agent(self.next_step_id, "");
                step.timestamp = Some(inv_ts);
                step.message = None;
                step.tool_calls = Some(vec![AtifToolCall {
                    tool_call_id: call_id.clone(),
                    function_name: tool_name,
                    arguments,
                }]);

                let status_suffix = match output.status {
                    ToolCallStatus::Failed => " [FAILED]",
                    ToolCallStatus::InProgress => " [IN_PROGRESS]",
                    ToolCallStatus::Completed => "",
                };
                let content = format!("{}{}", output.output, status_suffix);
                step.observation = Some(Observation {
                    results: vec![ObservationResult { source_call_id: call_id, content }],
                });
                self.push_step(step);
            }
            ThreadItemDetails::CommandExecution(cmd) => {
                let call_id = item_id.to_string();
                let mut step = Step::agent(self.next_step_id, "");
                step.timestamp = Some(ts.to_string());
                step.message = None;
                step.tool_calls = Some(vec![AtifToolCall {
                    tool_call_id: call_id.clone(),
                    function_name: "command_execution".to_string(),
                    arguments: Some(serde_json::json!({
                        "command": cmd.command,
                        "arguments": cmd.arguments,
                    })),
                }]);
                step.observation = Some(Observation {
                    results: vec![ObservationResult {
                        source_call_id: call_id,
                        content: cmd.aggregated_output.clone(),
                    }],
                });
                if let Some(exit_code) = cmd.exit_code {
                    step.extra = Some(serde_json::json!({ "exit_code": exit_code }));
                }
                self.push_step(step);
            }
            ThreadItemDetails::McpToolCall(mcp) => {
                let call_id = item_id.to_string();
                let mut step = Step::agent(self.next_step_id, "");
                step.timestamp = Some(ts.to_string());
                step.message = None;
                step.tool_calls = Some(vec![AtifToolCall {
                    tool_call_id: call_id.clone(),
                    function_name: mcp.tool_name.clone(),
                    arguments: mcp.arguments.clone(),
                }]);
                if let Some(result) = &mcp.result {
                    step.observation = Some(Observation {
                        results: vec![ObservationResult { source_call_id: call_id, content: result.clone() }],
                    });
                }
                self.push_step(step);
            }
            ThreadItemDetails::FileChange(fc) => {
                let changes: Vec<String> = fc.changes.iter().map(|c| format!("{}: {:?}", c.path, c.kind)).collect();
                let msg = format!("file_changes: {}", changes.join(", "));
                let mut step = Step::system(self.next_step_id, msg);
                step.timestamp = Some(ts.to_string());
                self.push_step(step);
            }
            ThreadItemDetails::WebSearch(ws) => {
                let mut step = Step::system(self.next_step_id, format!("web_search: {}", ws.query));
                step.timestamp = Some(ts.to_string());
                if let Some(results) = &ws.results {
                    step.observation = Some(Observation {
                        results: results
                            .iter()
                            .enumerate()
                            .map(|(i, r)| ObservationResult {
                                source_call_id: format!("search_{i}"),
                                content: r.clone(),
                            })
                            .collect(),
                    });
                }
                self.push_step(step);
            }
            ThreadItemDetails::Harness(h) => {
                let msg = format!("harness: {:?}", h.event);
                let mut step = Step::system(self.next_step_id, msg);
                step.timestamp = Some(ts.to_string());
                let mut extra = serde_json::Map::new();
                if let Some(m) = &h.message {
                    let _ = extra.insert("harness_message".to_string(), Value::String(m.clone()));
                }
                if h.event == crate::HarnessEventKind::BackgroundSubprocessCompleted {
                    for (key, value) in [
                        ("task_id", h.task_id.as_ref()),
                        ("session_id", h.session_id.as_ref()),
                        ("exec_session_id", h.exec_session_id.as_ref()),
                        ("status", h.status.as_ref()),
                        ("transcript_path", h.transcript_path.as_ref()),
                        ("archive_path", h.archive_path.as_ref()),
                        ("error_category", h.error_category.as_ref()),
                    ] {
                        if let Some(value) = value {
                            let _ = extra.insert(key.to_string(), Value::String(value.clone()));
                        }
                    }
                    if let Some(exit_code) = h.exit_code {
                        let _ = extra.insert("exit_code".to_string(), Value::from(exit_code));
                    }
                }
                if !extra.is_empty() {
                    step.extra = Some(Value::Object(extra));
                }
                self.push_step(step);
            }
            ThreadItemDetails::Error(e) => {
                let mut step = Step::system(self.next_step_id, &e.message);
                step.timestamp = Some(ts.to_string());
                self.push_step(step);
            }
        }
    }

    fn push_step(&mut self, step: Step) {
        self.next_step_id = step.step_id + 1;
        self.steps.push(step);
    }

    /// Consume the builder and produce the final ATIF trajectory.
    ///
    /// Pass optional `FinalMetrics` to override the accumulated values.
    /// If `None`, final metrics are derived from observed events.
    pub fn finish(self, override_metrics: Option<FinalMetrics>) -> Trajectory {
        let final_metrics = override_metrics.unwrap_or_else(|| FinalMetrics {
            total_prompt_tokens: Some(self.total_input_tokens),
            total_completion_tokens: Some(self.total_output_tokens),
            total_cached_tokens: if self.total_cached_tokens > 0 {
                Some(self.total_cached_tokens)
            } else {
                None
            },
            total_cost_usd: None,
            total_steps: Some(self.steps.len() as u64),
            extra: Some(serde_json::json!({ "num_turns": self.num_turns })),
        });

        Trajectory {
            schema_version: ATIF_SCHEMA_VERSION.to_string(),
            session_id: self.session_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            agent: self.agent,
            steps: self.steps,
            notes: None,
            final_metrics: Some(final_metrics),
            extra: self.completed_at.map(|timestamp| serde_json::json!({"completed_at":timestamp})),
        }
    }

    /// Returns the number of steps collected so far.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }
}

impl crate::EventEmitter for AtifTrajectoryBuilder {
    fn emit(&mut self, event: &ThreadEvent) {
        self.process_event(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AgentMessageItem, CompactionMode, CompactionTrigger, HarnessEventItem, HarnessEventKind, ItemCompletedEvent,
        ThreadCompactBoundaryEvent, ThreadItem, ThreadStartedEvent, ToolInvocationItem, ToolOutputItem,
        TurnCompletedEvent, TurnStartedEvent, Usage,
    };

    fn fixed_ts() -> DateTime<Utc> {
        "2025-01-15T10:30:00Z".parse().unwrap()
    }

    #[test]
    fn terminal_timestamps_survive_export_without_synthetic_steps() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let timestamp = "2026-10-03T01:02:03Z";
        let blocked = serde_json::from_value(
            serde_json::json!({"type":"turn.blocked", "message":"Blocked", "completed_at":timestamp}),
        )
        .unwrap();
        builder.process_event_at(&blocked, fixed_ts());
        let completed = serde_json::from_value(serde_json::json!({"type":"thread.completed", "completed_at":timestamp, "thread_id":"thread", "session_id":"session", "subtype":"success", "outcome_code":"completed", "usage":Usage::default(), "num_turns":1})).unwrap();
        builder.process_event_at(&completed, fixed_ts());
        let trajectory = builder.finish(None);
        assert_eq!(trajectory.steps.len(), 1);
        assert_eq!(trajectory.steps[0].timestamp.as_deref(), Some(timestamp));
        assert_eq!(trajectory.extra.as_ref().unwrap()["completed_at"], timestamp);
    }

    #[test]
    fn explanation_metadata_and_public_rationale_preserve_recorded_time() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let context = crate::ItemContext {
            task_id: "task-1".into(),
            turn_id: "turn-1".into(),
            actor_id: "root".into(),
            parent_actor_id: None,
            timestamp: "2026-10-03T01:02:03Z".into(),
            activity: None,
        };
        builder.process_event_at(
            &ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    id: "decision-1".into(),
                    context: Some(Box::new(context.clone())),
                    details: ThreadItemDetails::Decision(Box::new(crate::DecisionItem {
                        summary: "Reuse parser".into(),
                        rationale: "Preserve checks".into(),
                        alternatives: vec!["Replace parser".into()],
                        evidence_ids: vec!["read-1".into()],
                    })),
                },
            }),
            fixed_ts(),
        );
        let trajectory = builder.finish(None);
        assert_eq!(trajectory.steps.len(), 1);
        let step = &trajectory.steps[0];
        assert_eq!(step.timestamp.as_deref(), Some(context.timestamp.as_str()));
        let extra = step.extra.as_ref().unwrap();
        assert_eq!(extra["public_rationale"], "Preserve checks");
        assert_eq!(extra["item_context"]["task_id"], "task-1");
        assert_eq!(extra["evidence_ids"][0], "read-1");
    }

    #[test]
    fn trajectory_round_trip() {
        let trajectory = Trajectory {
            schema_version: ATIF_SCHEMA_VERSION.to_string(),
            session_id: "test-session".to_string(),
            agent: AtifAgent::vtcode(),
            steps: vec![Step::user(1, "hello")],
            notes: None,
            final_metrics: None,
            extra: None,
        };

        let json = serde_json::to_string_pretty(&trajectory).unwrap();
        let restored: Trajectory = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.schema_version, ATIF_SCHEMA_VERSION);
        assert_eq!(restored.session_id, "test-session");
        assert_eq!(restored.steps.len(), 1);
    }

    #[test]
    fn builder_thread_started_sets_session_id() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread-abc".to_string() });
        builder.process_event_at(&event, fixed_ts());
        let trajectory = builder.finish(None);
        assert_eq!(trajectory.session_id, "thread-abc");
    }

    #[test]
    fn builder_agent_message_step() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: "msg-1".to_string(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Hello, world!".to_string() }),
            },
        });
        builder.process_event_at(&event, fixed_ts());
        let trajectory = builder.finish(None);

        assert_eq!(trajectory.steps.len(), 1);
        let step = &trajectory.steps[0];
        assert_eq!(step.step_id, 1);
        assert_eq!(step.source, StepSource::Agent);
        assert_eq!(step.message.as_deref(), Some("Hello, world!"));
    }

    #[test]
    fn background_completion_preserves_identity_in_atif_extra() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: "background-completion:task-1:exec-1".to_string(),
                details: ThreadItemDetails::Harness(Box::new(HarnessEventItem {
                    event: HarnessEventKind::BackgroundSubprocessCompleted,
                    message: Some("completed successfully".to_string()),
                    command: None,
                    path: None,
                    exit_code: Some(0),
                    attempt: None,
                    error_category: Some("background_subprocess".to_string()),
                    duration_ms: None,
                    task_id: Some("task-1".to_string()),
                    session_id: Some("session-1".to_string()),
                    exec_session_id: Some("exec-1".to_string()),
                    status: Some("stopped".to_string()),
                    transcript_path: Some("/tmp/transcript.jsonl".to_string()),
                    archive_path: Some("/tmp/archive.json".to_string()),
                })),
            },
        });

        builder.process_event_at(&event, fixed_ts());
        let trajectory = builder.finish(None);
        let step = trajectory.steps.first().expect("background completion step");
        let extra = step.extra.as_ref().expect("background completion metadata");
        assert_eq!(step.message.as_deref(), Some("harness: BackgroundSubprocessCompleted"));
        assert_eq!(extra["harness_message"], "completed successfully");
        assert_eq!(extra["task_id"], "task-1");
        assert_eq!(extra["session_id"], "session-1");
        assert_eq!(extra["exec_session_id"], "exec-1");
        assert_eq!(extra["status"], "stopped");
        assert_eq!(extra["exit_code"], 0);
        assert_eq!(extra["transcript_path"], "/tmp/transcript.jsonl");
        assert_eq!(extra["archive_path"], "/tmp/archive.json");
        assert_eq!(extra["error_category"], "background_subprocess");
    }

    #[test]
    fn builder_tool_invocation_with_output() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let ts = fixed_ts();

        // Tool invocation
        let inv_event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: "tool_1".to_string(),
                details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                    tool_name: "read_file".to_string(),
                    arguments: Some(serde_json::json!({"path": "README.md"})),
                    tool_call_id: Some("tc_0".to_string()),
                    status: ToolCallStatus::Completed,
                    outcome: None,
                })),
            },
        });
        builder.process_event_at(&inv_event, ts);

        // Tool output
        let out_event = ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: "tool_1:output".to_string(),
                details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                    call_id: "tool_1".to_string(),
                    tool_call_id: Some("tc_0".to_string()),
                    spool_path: None,
                    output: "file contents here".to_string(),
                    exit_code: Some(0),
                    status: ToolCallStatus::Completed,
                })),
            },
        });
        builder.process_event_at(&out_event, ts);

        let trajectory = builder.finish(None);
        // Only one step: the invocation is buffered until output arrives
        assert_eq!(trajectory.steps.len(), 1);
        let step = &trajectory.steps[0];
        assert_eq!(step.source, StepSource::Agent);

        let calls = step.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function_name, "read_file");
        assert_eq!(calls[0].tool_call_id, "tc_0");

        let obs = step.observation.as_ref().unwrap();
        assert_eq!(obs.results.len(), 1);
        assert_eq!(obs.results[0].content, "file contents here");
    }

    #[test]
    fn builder_turn_completed_accumulates_metrics() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let event = ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage {
                input_tokens: 500,
                cached_input_tokens: 100,
                cache_creation_tokens: 0,
                output_tokens: 200,
            },
            in_progress_exec_sessions: Vec::new(),
        });
        builder.process_event_at(&event, fixed_ts());

        let trajectory = builder.finish(None);
        let fm = trajectory.final_metrics.as_ref().unwrap();
        assert_eq!(fm.total_prompt_tokens, Some(500));
        assert_eq!(fm.total_completion_tokens, Some(200));
        assert_eq!(fm.total_cached_tokens, Some(100));
    }

    #[test]
    fn builder_turn_completed_preserves_in_progress_sessions_for_resume() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let event = ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: vec!["run-7".to_string()],
        });
        builder.process_event_at(&event, fixed_ts());

        let trajectory = builder.finish(None);
        let step = trajectory.steps.last().expect("turn_completed step");
        let extra = step.extra.clone().expect("extra carries resume ids");
        assert_eq!(extra["in_progress_exec_sessions"], serde_json::json!(["run-7"]));

        // Empty ids stay omitted so steady-state export is unchanged.
        let mut empty_builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let empty = ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        });
        empty_builder.process_event_at(&empty, fixed_ts());
        let empty_trajectory = empty_builder.finish(None);
        assert!(empty_trajectory.steps.last().expect("step").extra.is_none());
    }

    #[test]
    fn step_metrics_from_usage() {
        let usage = Usage {
            input_tokens: 1000,
            cached_input_tokens: 200,
            cache_creation_tokens: 50,
            output_tokens: 300,
        };
        let metrics = StepMetrics::from_usage(&usage);
        assert_eq!(metrics.prompt_tokens, Some(1000));
        assert_eq!(metrics.completion_tokens, Some(300));
        assert_eq!(metrics.cached_tokens, Some(200));
        assert!(metrics.extra.is_some());
    }

    #[test]
    fn builder_implements_event_emitter() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let event = ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "t-1".to_string() });
        // Use EventEmitter trait
        crate::EventEmitter::emit(&mut builder, &event);
        assert_eq!(builder.step_count(), 0); // ThreadStarted doesn't create a step
    }

    #[test]
    fn skips_lifecycle_events() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        builder.process_event(&ThreadEvent::TurnStarted(TurnStartedEvent::default()));
        assert_eq!(builder.step_count(), 0);
    }

    #[test]
    fn compact_boundary_with_segment_metadata_preserves_atif_export() {
        let mut builder = AtifTrajectoryBuilder::new(AtifAgent::vtcode());
        let event = ThreadEvent::ThreadCompactBoundary(Box::new(ThreadCompactBoundaryEvent {
            thread_id: "thread-1".to_string(),
            trigger: CompactionTrigger::Auto,
            mode: CompactionMode::Local,
            original_message_count: 12,
            compacted_message_count: 5,
            history_artifact_path: None,
            previous_segment_id: Some("segment-0001".to_string()),
            new_segment_id: Some("segment-0002".to_string()),
            previous_prefix_hash: Some("prefix-before".to_string()),
            new_prefix_hash: Some("prefix-after".to_string()),
            previous_catalog_hash: Some("catalog-before".to_string()),
            new_catalog_hash: Some("catalog-after".to_string()),
        }));

        builder.process_event_at(&event, fixed_ts());
        let trajectory = builder.finish(None);

        assert_eq!(trajectory.steps.len(), 1);
        assert_eq!(trajectory.steps[0].source, StepSource::System);
        assert_eq!(
            trajectory.steps[0].message.as_deref(),
            Some("context_compaction: 12 messages -> 5 messages (auto)")
        );
    }
}
