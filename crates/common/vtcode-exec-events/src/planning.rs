//! Persisted planning recovery state. Answers to recovery prompts never grant approval.
use serde::{Deserialize, Serialize};

/// Runtime-owned planning outcome, independent of model prose and diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlanningOutcome {
    #[default]
    Researching,
    Synthesizing,
    Repairing,
    AwaitingInput,
    AwaitingApproval,
}

/// Observable response shape, never the contents of private reasoning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PlanningResponseClass {
    EmptyContent,
    ReasoningOnly,
    NativeToolCalls,
    MalformedPlan,
    Truncation,
    ProviderFailure,
    ValidPlan,
    ClarifyingQuestion,
}

/// Complete bounded checkpoint for one planning request and its explicit retry cycles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct PlanningRecoverySnapshot {
    pub outcome: PlanningOutcome,
    /// Headless task identity prevents replaying another request's draft or allowance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Full headless prompt/instructions identity; bounded display text is not sufficient.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_fingerprint: Option<u64>,
    /// Once closed, automatic continuations cannot reopen research.
    pub research_closed: bool,
    /// Model requests admitted in this cycle: one synthesis and two corrections.
    pub requests_used: u8,
    /// Increased only by an explicit human retry, clarification, or model selection.
    pub cycle: u32,
    pub latest_request: String,
    /// Bounded public tool evidence; private reasoning is never stored here.
    pub evidence: Vec<String>,
    pub best_draft: Option<String>,
    pub best_draft_defects: Option<usize>,
    pub validation_report: Option<String>,
    pub response_class: Option<PlanningResponseClass>,
    /// Runtime-owned guard cause retained independently of concise human summaries.
    pub guard_reason: Option<String>,
    pub provider: String,
    pub model: String,
    /// Prevents repeated automatic presentation of the same exhausted incident.
    pub input_prompt_shown: bool,
    /// Opens the existing picker at the next idle interaction boundary.
    pub model_selection_pending: bool,
    /// One explicitly requested research batch for newly missing clarification facts.
    pub targeted_research: Option<String>,
}

impl PlanningRecoverySnapshot {
    /// One synthesis and at most two corrections, shared across continuations.
    pub const MAX_REQUESTS: u8 = 3;

    /// Admit one tool-free request without reopening research or replenishing allowance.
    pub fn admit_request(&mut self) -> bool {
        if matches!(self.outcome, PlanningOutcome::AwaitingInput | PlanningOutcome::AwaitingApproval)
            || self.requests_used >= Self::MAX_REQUESTS
        {
            return false;
        }
        self.outcome = if self.requests_used == 0 {
            PlanningOutcome::Synthesizing
        } else {
            PlanningOutcome::Repairing
        };
        self.requests_used += 1;
        true
    }

    /// Called only after an explicit human retry, clarification, or model selection.
    pub fn begin_explicit_cycle(&mut self) {
        self.requests_used = 0;
        self.cycle = self.cycle.saturating_add(1);
        self.input_prompt_shown = false;
        self.targeted_research = None;
        self.outcome = PlanningOutcome::Synthesizing;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_admission_remains_bounded_after_serialized_resume_and_explicit_retry() {
        let mut snapshot = PlanningRecoverySnapshot {
            research_closed: true,
            best_draft: Some("public draft".into()),
            ..Default::default()
        };
        assert!(snapshot.admit_request());
        assert_eq!(snapshot.outcome, PlanningOutcome::Synthesizing);
        let mut resumed: PlanningRecoverySnapshot =
            serde_json::from_str(&serde_json::to_string(&snapshot).unwrap()).unwrap();
        assert!(resumed.admit_request());
        assert!(resumed.admit_request());
        assert_eq!(resumed.requests_used, 3);
        assert!(!resumed.admit_request());
        resumed.outcome = PlanningOutcome::AwaitingInput;
        assert!(!resumed.admit_request());
        resumed.begin_explicit_cycle();
        assert_eq!(resumed.cycle, 1);
        assert!(resumed.research_closed);
        assert_eq!(resumed.best_draft.as_deref(), Some("public draft"));
        assert!(resumed.admit_request());
    }
}
