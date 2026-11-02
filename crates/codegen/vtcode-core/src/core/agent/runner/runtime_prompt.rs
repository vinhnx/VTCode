//! Headless request envelopes and catalog-aware prompt refresh.
//!
//! Task execution retains lifecycle, budgets and cleanup ownership. This module
//! composes and validates provider-visible prefixes and preserves frozen envelopes
//! when catalog bookkeeping changes without a wire-visible change.

use super::{AgentRunner, prompt_alignment};
use crate::config::types::SystemPromptMode;
use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use crate::core::agent::hash_utils::stable_system_prefix_hash;
use crate::llm::provider::ToolDefinition;
use crate::prompts::{
    PromptContext, RuntimePromptContract, append_runtime_mode_sections, append_runtime_tool_prompt_sections_for_model,
    upsert_harness_limits_section,
};
use anyhow::Result;
use std::sync::Arc;
use tracing::debug;

pub(super) struct RuntimePromptBundle {
    prompt_policy_hash: u64,
    pub(super) request_envelope: crate::core::agent::request_envelope::SessionRequestEnvelope,
    pub(super) tool_snapshot: SessionToolCatalogSnapshot,
    /// Estimated token overhead of `request_tools`, computed once per
    /// snapshot (see [`crate::llm::usage_cost::estimate_tool_definition_tokens`]).
    pub(super) tool_def_tokens: u64,
    /// Token-budget report for the composed system instruction sections
    /// (measured before the runtime-mode/tool-guideline/harness-limits
    /// sections appended in `build_runtime_prompt_bundle`, which are runtime
    /// scaffolding rather than trimmable prompt layers). Drives the
    /// preflight token check and the over-budget user warning.
    pub(super) system_prompt_report: crate::prompts::system::SystemPromptReport,
}

impl RuntimePromptBundle {
    /// Provider-visible prefix unchanged across a catalog version bump.
    ///
    /// Compares only wire-visible identity (stable prompt digest, tool bytes,
    /// active set, planning flags). Version/epoch and token estimates are
    /// bookkeeping and intentionally excluded so no-op MCP refreshes preserve
    /// the frozen envelope and keep the provider prefix cache hot.
    fn has_same_provider_visible_prefix(&self, other: &Self) -> bool {
        self.prompt_policy_hash == other.prompt_policy_hash
            && self.tool_snapshot.tool_catalog_hash == other.tool_snapshot.tool_catalog_hash
            && self.tool_snapshot.planning_active == other.tool_snapshot.planning_active
            && self.tool_snapshot.request_user_input_enabled == other.tool_snapshot.request_user_input_enabled
            && self.tool_snapshot.active_tool_names == other.tool_snapshot.active_tool_names
            && self.request_envelope.instruction_digest() == other.request_envelope.instruction_digest()
            && self.request_envelope.catalog_hash() == other.request_envelope.catalog_hash()
    }
}

impl AgentRunner {
    fn runtime_prompt_policy_hash(&self) -> u64 {
        let model = self.get_selected_model();
        let cfg = self.config();
        let identity = crate::core::agent::hash_utils::PromptCapabilityIdentity::resolve(
            self.provider_client.as_ref(),
            &model,
            self.provider_client
                .sampling_overrides(&model)
                .reasoning_effort
                .or(self.reasoning_effort),
            0,
        );
        crate::core::agent::hash_utils::hash_value(&(
            identity,
            cfg.context.max_context_tokens,
            cfg.agent.max_system_prompt_tokens,
            cfg.agent.harness.max_budget_usd.map(f64::to_bits),
            format!("{:?}", cfg.agent.shell_prompt_profile),
            cfg.agent.harness.max_tool_calls_per_turn,
            cfg.agent.harness.max_tool_wall_clock_secs,
            cfg.agent.harness.max_tool_retries,
        ))
    }

    async fn compose_task_system_prompt(
        &self,
        prompt_tools: Arc<Vec<ToolDefinition>>,
        is_simple_task: bool,
    ) -> Result<(String, crate::prompts::system::SystemPromptReport)> {
        if !is_simple_task {
            return Ok((self.system_prompt.clone(), self.system_prompt_report.clone()));
        }

        let mut config = self.config().clone();
        config.agent.system_prompt_mode = SystemPromptMode::Minimal;
        let mut prompt_context = PromptContext::from_workspace_tools(
            self._workspace.as_path(),
            prompt_tools.iter().map(|tool| tool.function_name().to_string()),
        );
        prompt_context.set_current_directory(self._workspace.clone());
        prompt_context.load_available_skills_async().await;

        let (prompt, report) =
            super::helpers::compose_system_prompt_with_appendix(self._workspace.as_path(), &config, &prompt_context)
                .await?;

        Ok((prompt, report))
    }

    async fn build_runtime_prompt_bundle(&self, is_simple_task: bool) -> Result<RuntimePromptBundle> {
        let tool_snapshot = self.build_universal_tool_snapshot().await?;
        let request_tools = tool_snapshot.snapshot.clone();
        let prompt_tools = request_tools.clone().unwrap_or_else(|| Arc::new(Vec::new()));
        let (mut system_prompt, mut system_prompt_report) =
            self.compose_task_system_prompt(prompt_tools, is_simple_task).await?;
        self.append_active_primary_agent_context(&mut system_prompt);

        let planning_active = self.tool_registry.is_planning_active();
        let request_user_input_enabled = self.features().request_user_input_enabled(planning_active, false);
        let full_auto_active = self.tool_registry.current_full_auto_allowlist().await.is_some();

        append_runtime_mode_sections(
            &mut system_prompt,
            RuntimePromptContract {
                full_auto: full_auto_active,
                planning_active,
                request_user_input_enabled,
            },
        );
        upsert_harness_limits_section(
            &mut system_prompt,
            self.config().agent.harness.max_tool_calls_per_turn,
            self.config().agent.harness.max_tool_wall_clock_secs,
            self.config().agent.harness.max_tool_retries,
        );
        let shell_profile = self.config().agent.shell_prompt_profile.resolve_for_current_platform();
        append_runtime_tool_prompt_sections_for_model(
            &mut system_prompt,
            &tool_snapshot,
            true,
            shell_profile,
            self.provider_client.as_ref(),
            &self.get_selected_model(),
            Some(self.config()),
        );

        let tool_def_tokens = request_tools
            .as_deref()
            .map(|tools| crate::llm::usage_cost::estimate_tool_definition_tokens(tools))
            .unwrap_or(0);
        let tool_count = request_tools.as_deref().map_or(0, Vec::len);
        self.tool_registry
            .metrics_collector()
            .record_sdk_tool_definition_tokens(tool_def_tokens);
        debug!(tool_def_tokens, tool_count, "tool definition overhead");

        let instruction_digest = stable_system_prefix_hash(&system_prompt);
        let capability_digest = crate::core::agent::hash_utils::PromptCapabilityIdentity::resolve(
            self.provider_client.as_ref(),
            &self.get_selected_model(),
            self.reasoning_effort,
            tool_snapshot.epoch,
        )
        .digest();
        let system_instruction_prefix_hash =
            crate::core::agent::hash_utils::hash_value(&(instruction_digest, capability_digest));
        system_prompt_report.token_estimate = crate::prompts::system::estimate_token_count(&system_prompt);
        system_prompt_report.over_budget =
            system_prompt_report.token_estimate > self.config().agent.max_system_prompt_tokens;
        let request_envelope = crate::core::agent::request_envelope::SessionRequestEnvelope::with_prefix_hash(
            format!("{}-catalog-{}-{}", self.session_id, tool_snapshot.version, tool_snapshot.epoch),
            system_prompt,
            request_tools.as_deref().map_or_else(Vec::new, |tools| tools.clone()),
            instruction_digest,
            system_instruction_prefix_hash,
        );
        Ok(RuntimePromptBundle {
            prompt_policy_hash: self.runtime_prompt_policy_hash(),
            request_envelope,
            tool_snapshot,
            tool_def_tokens,
            system_prompt_report,
        })
    }

    fn append_active_primary_agent_context(&self, system_prompt: &mut String) {
        let Some(active_primary_agent) = self.active_primary_agent.as_ref() else {
            return;
        };
        crate::prompts::apply_coordinator_role_guidance(
            system_prompt,
            active_primary_agent.identity.name == "coordinator",
        );

        system_prompt.push_str("\n\n## Active Primary Agent Runtime State\n");
        system_prompt.push_str("- Active agent: ");
        system_prompt.push_str(&active_primary_agent.display_name);
        system_prompt.push_str("\n- Spec name: ");
        system_prompt.push_str(&active_primary_agent.identity.name);
        if let Some(model) = active_primary_agent
            .model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty() && !model.eq_ignore_ascii_case("inherit"))
        {
            system_prompt.push_str("\n- Agent model: ");
            system_prompt.push_str(model);
        }
        if let Some(reasoning_effort) = active_primary_agent.reasoning_effort.as_ref().map(|e| e.as_str()) {
            system_prompt.push_str("\n- Agent reasoning effort: ");
            system_prompt.push_str(reasoning_effort);
        }
        system_prompt.push_str("\n\n## Active Primary Agent Instructions\n");
        system_prompt.push_str(active_primary_agent.instructions.trim());
    }

    pub(super) async fn build_validated_runtime_prompt_bundle(
        &self,
        is_simple_task: bool,
    ) -> Result<RuntimePromptBundle> {
        let mut runner = self;
        let bundle = runner.build_runtime_prompt_bundle(is_simple_task).await?;
        prompt_alignment::rebuild_once_on_alignment_mismatch(
            &mut runner,
            bundle,
            |runner| Box::pin((*runner).build_runtime_prompt_bundle(is_simple_task)),
            |runner, bundle| {
                let planning_active = runner.tool_registry.is_planning_active();
                let request_user_input_enabled = runner.features().request_user_input_enabled(planning_active, false);
                prompt_alignment::validate_prompt_catalog_alignment(
                    bundle.request_envelope.system_prompt().as_ref(),
                    &bundle.tool_snapshot,
                    planning_active,
                    request_user_input_enabled,
                )
            },
            "prompt/catalog alignment mismatch; rebuilding runtime prompt bundle",
            "prompt/catalog alignment mismatch persisted after rebuild",
        )
        .await
    }

    pub(super) async fn refresh_runtime_prompt_bundle_if_catalog_changed(
        &self,
        bundle: &mut RuntimePromptBundle,
        is_simple_task: bool,
    ) -> Result<bool> {
        let current_version = self.tool_registry.tool_catalog_state().current_version();
        let current_policy = self.runtime_prompt_policy_hash();
        if current_version == bundle.tool_snapshot.version && current_policy == bundle.prompt_policy_hash {
            return Ok(false);
        }

        debug!(
            old_version = bundle.tool_snapshot.version,
            new_version = current_version,
            "Tool catalog changed mid-task; refreshing runtime prompt bundle"
        );
        let new_bundle = self.build_validated_runtime_prompt_bundle(is_simple_task).await?;
        // No-op refresh: version/epoch bumped without provider-visible change
        // (e.g. MCP refresh returning identical tools). Preserve the frozen
        // envelope, token estimates, and reports so the provider prefix cache
        // stays hot; only advance snapshot bookkeeping so the next version
        // check passes.
        if new_bundle.has_same_provider_visible_prefix(bundle) {
            debug!(
                old_version = bundle.tool_snapshot.version,
                new_version = new_bundle.tool_snapshot.version,
                "Tool catalog version bumped without provider-visible change; preserving request envelope"
            );
            bundle.tool_snapshot = new_bundle.tool_snapshot;
            return Ok(false);
        }
        *bundle = new_bundle;
        Ok(true)
    }
}
