//! Agent system for intelligent conversation management

pub mod beliefs;
pub mod blocked_handoff;
pub mod bootstrap;
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
pub mod orient;
pub mod progress_monitor;
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
pub use blocked_handoff::{AsyncApprovalArtifacts, BlockedHandoffArtifacts};
pub use bootstrap::{AgentComponentBuilder, AgentComponentSet};
pub use context_reset::{ContextResetDecision, ContextResetManifest};
pub use evaluator::{DimensionScore, EvaluationResult, EvaluationRubric, ScoringDimension};
pub use features::{FeatureGate, FeatureSet, FeatureStage, OpenResponsesFeature};
pub use handoff::{BoundaryItem, BoundaryStatus, HandoffReceipt, HandoffRequest};
pub use orient::OrientationContext;
pub use session_config::ResolvedSessionConfig;

pub use types::*;
