use std::path::{Path, PathBuf};
use std::time::Instant;

use rustc_hash::{FxHashMap, FxHashSet};
use vtcode_core::core::agent::refusal;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::names::canonical_tool_name;
use vtcode_core::tools::tool_intent::{
    ShellActivity, classify_shell_activity, shell_args_as_executed, shell_command_contains_verifier,
    shell_command_is_admitted_verification_attempt,
};

use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::tool_outcomes::read_extent;
use crate::agent::runloop::unified::turn::tool_outcomes::{
    is_empty_shell_search, is_grep_style_no_match, output_field_is_empty,
};

mod auto_continue;
mod history_dedup;
mod loop_tracker;
mod mutation_gate;
mod tracker_probe;
mod verification_config;

pub(crate) use auto_continue::*;
pub(crate) use history_dedup::*;
pub(crate) use loop_tracker::*;
pub(crate) use mutation_gate::*;
pub(crate) use tracker_probe::*;
pub(crate) use verification_config::*;

#[cfg(test)]
mod tracker_continue_tests;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod navigation_evidence_tests;
