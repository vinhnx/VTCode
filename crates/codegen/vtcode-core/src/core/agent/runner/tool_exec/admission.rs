//! Admission and batch layout for runner tool calls.

use super::super::AgentRunner;
use super::super::tool_rejection::{reject_denied_tool, reject_invalid_args, reject_policy_denied};
use super::super::tool_types::{PreparedRunnerToolBatch, PreparedRunnerToolCall, RunnerCallAdmission};
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::harness_kernel::PreparedToolBatch;
use crate::core::agent::runtime::AgentRuntime;
use crate::llm::provider::ToolCall;
use crate::tools::error_messages::agent_execution;
use anyhow::Result;

pub(super) fn align_prepared_batches(
    calls: Vec<PreparedRunnerToolCall>,
    allow_parallel: bool,
    max_parallel: usize,
) -> Vec<PreparedRunnerToolBatch> {
    let mut semantic_calls = std::collections::HashSet::new();
    let layout = PreparedToolBatch::plan_layout_with_limit(
        calls.iter().map(|call| {
            let semantic_key = format!("{}:{}", call.prepared.canonical_name, call.prepared.effective_args);
            call.prepared.can_parallelize() && semantic_calls.insert(semantic_key)
        }),
        allow_parallel,
        max_parallel,
    );
    let mut calls = calls.into_iter();

    layout
        .into_iter()
        .map(|(kind, len)| PreparedRunnerToolBatch { kind, calls: calls.by_ref().take(len).collect() })
        .collect()
}

impl AgentRunner {
    pub(super) async fn admit_runner_tool_call(
        &self,
        call: ToolCall,
        runtime: &mut AgentRuntime,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<RunnerCallAdmission> {
        let requested_name = match call.tool_name() {
            Some(name) => name.to_string(),
            None => return Ok(RunnerCallAdmission::Rejected),
        };
        let args = match call.execution_arguments() {
            Ok(args) => args,
            Err(err) => {
                reject_invalid_args(
                    runtime,
                    event_recorder,
                    agent_prefix,
                    &requested_name,
                    call.id.as_str(),
                    None,
                    &err,
                    is_gemini,
                    "Invalid tool arguments",
                );
                return Ok(RunnerCallAdmission::Rejected);
            }
        };
        if self.resolve_executable_tool_name(&requested_name).await.is_none() {
            reject_denied_tool(
                runtime,
                event_recorder,
                agent_prefix,
                &requested_name,
                &call.id,
                Some(&args),
                is_gemini,
                self.quiet,
            );
            return Ok(RunnerCallAdmission::Rejected);
        }
        let prepared = match self.admit_tool_call(&requested_name, args, &mut runtime.state) {
            Ok(prepared) => prepared,
            Err(err) => {
                // A policy gate (planning workflow, command security) denies the
                // call during admission. That is a policy stop, not a malformed
                // call: classify it with the shared classifier so the model gets
                // the gate's own remediation guidance instead of an
                // `Invalid arguments` error it would retry unchanged.
                if agent_execution::preflight_policy_correction(&format!("{err:#}")).is_some() {
                    reject_policy_denied(
                        runtime,
                        event_recorder,
                        agent_prefix,
                        &requested_name,
                        call.id.as_str(),
                        None,
                        &err,
                        is_gemini,
                        self.quiet,
                    );
                } else {
                    reject_invalid_args(
                        runtime,
                        event_recorder,
                        agent_prefix,
                        &requested_name,
                        call.id.as_str(),
                        None,
                        &err,
                        is_gemini,
                        "Tool admission failed",
                    );
                }
                return Ok(RunnerCallAdmission::Rejected);
            }
        };

        if self.check_for_loop(&prepared.canonical_name, &prepared.effective_args, &mut runtime.state) {
            return Ok(RunnerCallAdmission::StopTurn);
        }

        if !self.is_valid_tool(&prepared.canonical_name).await {
            reject_denied_tool(
                runtime,
                event_recorder,
                agent_prefix,
                &prepared.canonical_name,
                &call.id,
                Some(&prepared.effective_args),
                is_gemini,
                self.quiet,
            );
            return Ok(RunnerCallAdmission::Rejected);
        }

        Ok(RunnerCallAdmission::Prepared(Box::new(PreparedRunnerToolCall { tool_call_id: call.id, prepared })))
    }
}
