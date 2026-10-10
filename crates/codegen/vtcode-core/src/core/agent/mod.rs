//! Agent system for intelligent conversation management

pub mod beliefs;
pub mod blocked_handoff;
pub mod bootstrap;
pub mod cache_health;
pub mod compaction_checkpoint;
pub mod completion;
pub mod context_reset;
pub mod harness_artifacts;
pub mod harness_kernel;

pub mod config;
pub mod conversation;
pub mod core;
pub mod display;
pub mod error_recovery;
pub mod evaluator;
pub mod events;
pub mod features;
pub mod handoff;
pub mod hash_utils;
pub mod hypothesis;
pub mod orient;
pub mod progress_monitor;
pub mod refusal;
pub mod request_envelope;
pub mod request_plan;
pub mod result_reducers;
pub mod runner;
pub mod runtime;
pub mod session;
pub mod session_config;
pub mod snapshots;
pub mod state;
pub mod steering;
pub mod task;
pub mod task_history;
pub mod tool_batching;
pub mod tool_catalog;
pub mod types;

// Re-export main types for convenience
pub use blocked_handoff::{
    AsyncApprovalArtifacts, BlockedHandoffArtifacts, BlockedHandoffInfo, BlockedHandoffResume,
    clear_current_blocked_handoff, clear_current_blocked_handoff_for_session, read_current_blocked_handoff,
    write_blocked_handoff, write_blocked_handoff_with_resume,
};
pub use bootstrap::{AgentComponentBuilder, AgentComponentSet};
pub use context_reset::{ContextResetDecision, ContextResetManifest};
pub use evaluator::{
    DimensionScore, EvaluationResult, EvaluationRubric, ScoringDimension, default_code_rubric_with_hypothesis_revision,
    hypothesis_revision_dimension, score_hypothesis_revision,
};
pub use features::{FeatureGate, FeatureSet, FeatureStage, OpenResponsesFeature};
pub use handoff::{BoundaryItem, BoundaryStatus, HandoffReceipt, HandoffRequest};
pub use hypothesis::{MismatchEvidence, MismatchKind, append_revision_guidance, classify_mismatch, revision_guidance};
pub use orient::OrientationContext;
pub use session_config::ResolvedSessionConfig;

pub use types::*;
