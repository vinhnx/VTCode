//! Unified tool-output envelope.
//!
//! §18.4.3 of *The Hitchhiker's Guide to Agentic AI* prescribes a single
//! orchestrator that walks every tool output through the same six steps:
//! parse → validate → truncate → normalize → retry → inject. The previous
//! VT Code design split these steps across `response_content.rs`,
//! `error_handling.rs`, `execution_attempts.rs`, `summarizers/`, and
//! `helpers.rs` — which made it impossible to apply uniform policies or
//! reason about ordering.
//!
//! This module introduces [`ToolOutputEnvelope`] as the single shape every
//! step writes into, and [`crate::tools::output_pipeline::process_tool_output`]
//! as the orchestrator that fills it. Existing success / failure paths become
//! thin adapters that build an envelope and format it back into the model
//! wire shape.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::output_limits::TruncationReport;

/// Outcome of a tool invocation, before any pipeline step runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPipelineStatus {
    /// Tool finished and produced a result.
    Success,
    /// Tool returned an error before producing a result.
    Failure,
    /// Tool exceeded its wall-clock budget.
    Timeout,
    /// Tool execution was cancelled (HITL refusal, planning denial, …).
    Cancelled,
    /// Tool execution was blocked before it started (loop detector, fuse, …).
    Blocked,
}

/// Raw input to the pipeline.
#[derive(Debug, Clone)]
pub struct ToolOutputInput {
    /// Identifier of the tool call (matches `Message::tool_call_id`).
    pub tool_call_id: String,
    /// Canonical tool name (e.g. `read_file`, `mcp::fetch::fetch`).
    pub tool_name: String,
    /// Initial pipeline status.
    pub status: ToolPipelineStatus,
    /// Raw output (string for textual tools, JSON for structured ones).
    pub raw_output: Option<String>,
    /// Structured output, when available (already parsed by the tool).
    pub structured_output: Option<Value>,
    /// Free-form error message (when `status` is failure / timeout / cancelled
    /// / blocked).
    pub error_message: Option<String>,
    /// Whether the raw output was already JSON-parseable.
    pub raw_is_json: bool,
}

impl ToolOutputInput {
    /// Construct a successful input with text output.
    pub fn success_text(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        raw: impl Into<String>,
    ) -> Self {
        let raw_string = raw.into();
        let raw_is_json = serde_json::from_str::<Value>(&raw_string).is_ok();
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            status: ToolPipelineStatus::Success,
            raw_output: Some(raw_string),
            structured_output: None,
            error_message: None,
            raw_is_json,
        }
    }

    /// Construct a failure input with a human-readable error.
    pub fn failure(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        status: ToolPipelineStatus,
        error: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            status,
            raw_output: None,
            structured_output: None,
            error_message: Some(error.into()),
            raw_is_json: false,
        }
    }
}

/// Result of step 2 (validate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// True when validation succeeded without warnings.
    pub ok: bool,
    /// Optional list of human-readable warnings (e.g. `unknown_field`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
}

impl ValidationReport {
    /// Successful validation.
    #[must_use]
    pub fn ok() -> Self {
        Self {
            ok: true,
            warnings: Vec::new(),
        }
    }

    /// Validation with warnings (still treated as successful — the harness
    /// decides whether warnings should block).
    #[must_use]
    pub fn with_warnings(warnings: Vec<String>) -> Self {
        Self { ok: true, warnings }
    }

    /// Validation failed.
    #[must_use]
    pub fn failed(warnings: Vec<String>) -> Self {
        Self {
            ok: false,
            warnings,
        }
    }
}

/// Result of step 4 (normalize).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedOutput {
    /// Normalized text payload (post-truncation, post-sanitization).
    pub text: String,
    /// Optional structured payload preserved for downstream consumers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured: Option<Value>,
    /// True when the normalized text was synthesized from a failure / error.
    pub synthesized_from_error: bool,
}

/// Final retry classification produced by the pipeline (step 5).
///
/// The pipeline only emits a *recommendation*; the actual retry loop lives in
/// `vtcode-core/src/retry.rs` and the tool execution kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryRecommendation {
    /// Pipeline thinks the call succeeded — no retry.
    NoRetry,
    /// Transient failure — retry with the configured backoff.
    RetryTransient,
    /// Retryable network or rate-limit error.
    RetryNetwork,
    /// Permanent failure — surface to the model without retrying.
    NoRetryPermanent,
    /// Cancellation — caller (HITL / planning) should not retry.
    Cancelled,
}

/// Per-step metrics emitted at the end of the pipeline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OutputMetrics {
    /// Step durations in microseconds.
    pub parse_us: u64,
    /// Step durations in microseconds.
    pub validate_us: u64,
    /// Step durations in microseconds.
    pub truncate_us: u64,
    /// Step durations in microseconds.
    pub normalize_us: u64,
    /// Final size in bytes of the injected text.
    pub injected_bytes: usize,
    /// Original size in bytes before any truncation.
    pub original_bytes: usize,
}

/// The unified envelope produced by the pipeline. Every step writes a
/// distinct field so dashboards and tests can introspect each stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutputEnvelope {
    /// Identifier of the tool call this envelope is the response to.
    pub tool_call_id: String,
    /// Canonical tool name.
    pub tool_name: String,
    /// Pipeline status (may differ from input status if normalization turned a
    /// partial failure into a structured payload).
    pub status: ToolPipelineStatus,
    /// Raw output as received (None for failure inputs).
    pub raw: Option<String>,
    /// Parsed JSON value when the raw output was structured.
    pub parsed: Option<Value>,
    /// Validation result (step 2).
    pub validation: ValidationReport,
    /// Truncation result (step 3, None when no truncation was needed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<TruncationReport>,
    /// Normalized payload (step 4).
    pub normalized: NormalizedOutput,
    /// Retry recommendation (step 5).
    pub retry_decision: RetryRecommendation,
    /// Per-step metrics.
    pub metrics: OutputMetrics,
}

impl ToolOutputEnvelope {
    /// True when the pipeline produced an injectable success payload.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self.status, ToolPipelineStatus::Success) && !self.normalized.text.is_empty()
    }

    /// Convenience: short summary suitable for the audit log.
    #[must_use]
    pub fn summary(&self) -> String {
        let prefix = match self.status {
            ToolPipelineStatus::Success => "success",
            ToolPipelineStatus::Failure => "failure",
            ToolPipelineStatus::Timeout => "timeout",
            ToolPipelineStatus::Cancelled => "cancelled",
            ToolPipelineStatus::Blocked => "blocked",
        };
        let bytes = self.metrics.injected_bytes;
        let mut out = String::with_capacity(prefix.len() + 16);
        out.push_str(prefix);
        out.push(':');
        out.push_str(&bytes.to_string());
        out.push_str("b");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_text_input_detects_json() {
        let input = ToolOutputInput::success_text("call_1", "read_file", "{\"a\":1}");
        assert!(input.raw_is_json);

        let input = ToolOutputInput::success_text("call_1", "read_file", "hello");
        assert!(!input.raw_is_json);
    }

    #[test]
    fn failure_input_carries_error_message() {
        let input = ToolOutputInput::failure(
            "call_2",
            "bash",
            ToolPipelineStatus::Failure,
            "command not found",
        );
        assert_eq!(input.status, ToolPipelineStatus::Failure);
        assert_eq!(input.error_message.as_deref(), Some("command not found"));
        assert!(input.raw_output.is_none());
    }

    #[test]
    fn envelope_is_success_when_status_and_text_present() {
        let env = ToolOutputEnvelope {
            tool_call_id: "c".into(),
            tool_name: "t".into(),
            status: ToolPipelineStatus::Success,
            raw: None,
            parsed: None,
            validation: ValidationReport::ok(),
            truncated: None,
            normalized: NormalizedOutput {
                text: "ok".into(),
                structured: None,
                synthesized_from_error: false,
            },
            retry_decision: RetryRecommendation::NoRetry,
            metrics: OutputMetrics {
                injected_bytes: 2,
                ..OutputMetrics::default()
            },
        };
        assert!(env.is_success());
        assert_eq!(env.summary(), "success:2b");
    }

    #[test]
    fn envelope_summary_distinguishes_statuses() {
        let mut env = ToolOutputEnvelope {
            tool_call_id: "c".into(),
            tool_name: "t".into(),
            status: ToolPipelineStatus::Failure,
            raw: None,
            parsed: None,
            validation: ValidationReport::failed(vec!["oops".into()]),
            truncated: None,
            normalized: NormalizedOutput {
                text: "error: oops".into(),
                structured: None,
                synthesized_from_error: true,
            },
            retry_decision: RetryRecommendation::NoRetryPermanent,
            metrics: OutputMetrics {
                injected_bytes: 11,
                ..Default::default()
            },
        };
        assert_eq!(env.summary(), "failure:11b");
        env.status = ToolPipelineStatus::Timeout;
        assert_eq!(env.summary(), "timeout:11b");
    }
}
