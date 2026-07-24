//! System-prompt / message assembly.
//!
//! Builds the per-turn system prompt (base prompt, primary-agent skills,
//! harness limits, runtime tool catalog section, deferred-tools summary,
//! Copilot out-of-band guidance, few-shot examples, and active
//! primary-agent runtime-state block) and the tool-catalog snapshot that
//! goes with it, then validates that the two stay in alignment before
//! handing them to the request-builder orchestrator. Invariant: the
//! returned [`PromptAssemblyOutput`] is always alignment-checked against
//! its `tool_snapshot` (see `validate_prompt_output_with_rebuild`) before
//! this module's output is used to build a wire request.

use std::fmt::Write as _;
use std::sync::Arc;

use anyhow::Result;
use dirs::home_dir;

use vtcode_core::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use vtcode_core::core::agent::runner::prompt_alignment;
use vtcode_core::llm::provider as uni;
use vtcode_core::prompts::{
    DEFAULT_FEW_SHOT_BUDGET_TOKENS, FewShotStore, PromptContext, append_deferred_tools_prompt_section,
    append_runtime_tool_prompt_sections, render_few_shot_section, temporal::generate_temporal_context,
    upsert_harness_limits_section,
};
use vtcode_core::subagents::load_primary_memory_appendix;
use vtcode_core::{ActivePrimaryAgent, apply_primary_agent_prompt_context};

use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

use super::snapshot::TurnRequestSnapshot;
use super::tool_shaping::{apply_primary_agent_policy_to_tool_snapshot, uses_out_of_band_copilot_tools};

pub(super) struct PromptAssemblyInput<'a> {
    pub turn: &'a TurnRequestSnapshot,
}

pub(super) struct PromptAssemblyOutput {
    pub system_prompt: String,
    pub tool_snapshot: SessionToolCatalogSnapshot,
    pub agent_prompt_context: Option<PromptContext>,
}

/// Build the few-shot section for the current turn.
///
/// Returns `None` when no examples match or when selection is skipped
/// (no query available, empty store). The selection uses keyword-tag
/// overlap with the most recent user message and is bounded by
/// [`DEFAULT_FEW_SHOT_BUDGET_TOKENS`].
fn build_few_shot_section(ctx: &mut TurnProcessingContext<'_>) -> Option<String> {
    let query = latest_user_query(ctx.working_history.as_slice())?;
    let store = FewShotStore::load(Some(ctx.config.workspace.as_path()), home_dir().as_deref());
    if store.is_empty() {
        return None;
    }
    let chosen = store.select(&query, DEFAULT_FEW_SHOT_BUDGET_TOKENS);
    if chosen.is_empty() {
        return None;
    }
    Some(render_few_shot_section(&chosen))
}

/// Return the text of the most recent user message in `history`. Used as
/// the query for few-shot selection. Returns `None` when no user message
/// is present (e.g., empty / tool-only history).
fn latest_user_query(history: &[uni::Message]) -> Option<String> {
    history
        .iter()
        .rev()
        .find(|message| matches!(message.role, uni::MessageRole::User))
        .map(|message| message.content.as_text().into_owned())
        .filter(|text: &String| !text.trim().is_empty())
}

fn append_copilot_runtime_guidance(system_prompt: &mut String) {
    let _ = writeln!(
        system_prompt,
        "\n[GitHub Copilot Client Tools]\n- the VT Code tools named in this prompt are exposed as Copilot client tools outside the normal JSON tool list\n- when a tool is needed, emit the actual client tool call instead of describing the call in plain text\n- do not claim a tool was rejected, blocked, or unavailable unless the runtime returned that result"
    );
}

#[hotpath::measure]
pub(super) async fn assemble_prompt(
    ctx: &mut TurnProcessingContext<'_>,
    input: PromptAssemblyInput<'_>,
) -> Result<PromptAssemblyOutput> {
    let prompt_output = build_prompt_output(ctx, PromptAssemblyInput { turn: input.turn }).await?;

    validate_prompt_output_with_rebuild(ctx, input.turn, prompt_output).await
}

async fn build_prompt_output(
    ctx: &mut TurnProcessingContext<'_>,
    input: PromptAssemblyInput<'_>,
) -> Result<PromptAssemblyOutput> {
    let mut system_prompt = ctx
        .context_manager
        .build_system_prompt(crate::agent::runloop::unified::context_manager::SystemPromptParams {
            full_auto: input.turn.full_auto,
            auto_permission: input.turn.auto_permission,
            planning_active: input.turn.planning_active,
            request_user_input_enabled: input.turn.request_user_input_enabled,
        })
        .await?;

    let agent = &input.turn.active_primary_agent;
    let agent_prompt_context = if agent.skills.is_empty() {
        None
    } else {
        Some(active_primary_agent_prompt_context(ctx, agent))
    };

    append_active_primary_agent_skills(&mut system_prompt, ctx, agent, agent_prompt_context.as_ref());

    upsert_harness_limits_section(
        &mut system_prompt,
        input.turn.execution.max_tool_calls,
        input.turn.execution.max_tool_wall_clock_secs,
        input.turn.execution.max_tool_retries,
    );

    let tool_snapshot = if input.turn.tool_free_recovery {
        let _ = writeln!(
            system_prompt,
            "\n[Recovery Mode]\n- tools_disabled: true\n- answer_mode: summarize only from evidence already collected in this turn\n- if evidence is incomplete, say so explicitly\n- do_not_request_more_tools: true\n- keep_response_brief: true"
        );
        if let Some(reason) = input.turn.recovery_reason.as_deref() {
            let _ = writeln!(system_prompt, "- recovery_reason: {reason}");
        }
        SessionToolCatalogSnapshot::new(
            ctx.tool_catalog.current_version(),
            ctx.tool_catalog.current_epoch(),
            input.turn.planning_active,
            input.turn.request_user_input_enabled,
            None,
            false,
        )
    } else if !input.turn.capabilities.tools {
        SessionToolCatalogSnapshot::new(
            ctx.tool_catalog.current_version(),
            ctx.tool_catalog.current_epoch(),
            input.turn.planning_active,
            input.turn.request_user_input_enabled,
            None,
            false,
        )
    } else {
        let base_snapshot = ctx
            .tool_catalog
            .filtered_snapshot_with_stats(ctx.tools, input.turn.planning_active, input.turn.request_user_input_enabled)
            .await;
        apply_primary_agent_policy_to_tool_snapshot(
            base_snapshot,
            &input.turn.active_primary_agent,
            &ctx.config.workspace,
            ctx.vt_cfg,
        )
    };

    append_runtime_tool_prompt_sections(
        &mut system_prompt,
        &tool_snapshot,
        !input.turn.prompt_cache_shaping_mode.is_enabled(),
    );

    if input.turn.client_local_tool_deferral && !input.turn.tool_free_recovery {
        // Client-local deferral omits deferred tools from the wire payload
        // (see `build_turn_request`); tell the model what it can still
        // reach through the relevant discovery tool. `tool_snapshot` still
        // carries the full, un-filtered tool list at this point, so
        // `deferred_count`/namespace metadata reflect what is actually
        // being withheld this turn. Skip during tool-free recovery: that
        // path sends `tools: None` (see `build_turn_request`), so the model
        // cannot load deferred tools even if told about them.
        append_deferred_tools_prompt_section(
            &mut system_prompt,
            tool_snapshot.snapshot.as_deref().map_or(&[], |tools| tools.as_slice()),
        );
    }

    if tool_snapshot.has_tools() && uses_out_of_band_copilot_tools(&input.turn.provider_name) {
        append_copilot_runtime_guidance(&mut system_prompt);
    }

    // Section 18.3.3 of the agentic-AI guide: inject at most
    // DEFAULT_FEW_SHOT_BUDGET_TOKENS of relevant few-shot examples selected
    // from `.vtcode/prompts/examples/`. Skip in recovery mode (the model is
    // in "summarize only" mode and adding examples would distract).
    if !input.turn.tool_free_recovery
        && let Some(section) = build_few_shot_section(ctx)
    {
        let _ = writeln!(system_prompt, "\n{section}");
    }

    Ok(PromptAssemblyOutput { system_prompt, tool_snapshot, agent_prompt_context })
}

fn active_primary_agent_prompt_context(ctx: &TurnProcessingContext<'_>, agent: &ActivePrimaryAgent) -> PromptContext {
    let mut prompt_context =
        PromptContext::from_workspace_tools(ctx.config.workspace.as_path(), std::iter::empty::<String>());
    apply_primary_agent_prompt_context(&mut prompt_context, agent);
    prompt_context
}

fn append_active_primary_agent_skills(
    system_prompt: &mut String,
    _ctx: &TurnProcessingContext<'_>,
    agent: &ActivePrimaryAgent,
    prompt_context: Option<&PromptContext>,
) {
    if agent.skills.is_empty() {
        return;
    }

    let Some(prompt_context) = prompt_context else {
        return;
    };

    let _ = writeln!(system_prompt, "\n## Active Primary Agent Skills");
    let _ = writeln!(system_prompt, "These skills are scoped to the active primary agent for this request.");

    if prompt_context.available_skill_metadata.is_empty() {
        for skill in &agent.skills {
            let _ = writeln!(system_prompt, "- {skill}");
        }
    } else {
        let mut skills: Vec<_> = prompt_context.available_skill_metadata.iter().collect();
        skills.sort_by(|left, right| left.name.cmp(&right.name));
        for skill in skills {
            let _ = writeln!(system_prompt, "- {}: {}", skill.name, skill.description);
        }
    }
}

fn validate_prompt_output_alignment(
    prompt_output: &PromptAssemblyOutput,
    turn: &TurnRequestSnapshot,
) -> Result<(), prompt_alignment::AlignmentError> {
    prompt_alignment::validate_prompt_catalog_alignment(
        &prompt_output.system_prompt,
        &prompt_output.tool_snapshot,
        turn.planning_active,
        turn.request_user_input_enabled,
    )
}

async fn validate_prompt_output_with_rebuild(
    ctx: &mut TurnProcessingContext<'_>,
    turn: &TurnRequestSnapshot,
    prompt_output: PromptAssemblyOutput,
) -> Result<PromptAssemblyOutput> {
    let rebuild_turn = Arc::new(turn.clone());
    prompt_alignment::rebuild_once_on_alignment_mismatch(
        ctx,
        prompt_output,
        move |ctx| {
            let arc_for_call = rebuild_turn.clone();
            Box::pin(async move { build_prompt_output(ctx, PromptAssemblyInput { turn: &arc_for_call }).await })
        },
        |_, prompt_output| validate_prompt_output_alignment(prompt_output, turn),
        "prompt/catalog alignment mismatch during unified request assembly; rebuilding prompt",
        "prompt/catalog alignment mismatch persisted after unified prompt rebuild",
    )
    .await
}

pub(super) async fn render_primary_agent_runtime_context(
    ctx: &TurnProcessingContext<'_>,
    turn_snapshot: &TurnRequestSnapshot,
    tool_snapshot: &SessionToolCatalogSnapshot,
    agent: &ActivePrimaryAgent,
    reasoning_effort: Option<vtcode_core::config::types::ReasoningEffortLevel>,
    agent_prompt_context: Option<&PromptContext>,
) -> String {
    let mut buf = String::with_capacity(1024);
    let _ = writeln!(buf, "## Active Primary Agent Runtime State");
    let _ = writeln!(buf, "- Active agent: {}", agent.display_name);
    let _ = writeln!(buf, "- Spec name: {}", agent.identity.name);
    let _ = writeln!(buf, "- Request model: {}", turn_snapshot.active_model);
    if let Some(model) = agent
        .model
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty() && !model.eq_ignore_ascii_case("inherit"))
    {
        let _ = writeln!(buf, "- Agent model: {model}");
    }
    if let Some(effort) = reasoning_effort {
        let _ = writeln!(buf, "- Request reasoning effort: {}", effort.as_str());
    }
    if let Some(raw_effort) = agent.reasoning_effort {
        let _ = writeln!(buf, "- Agent reasoning effort: {}", raw_effort.as_str());
    }
    let _ = writeln!(
        buf,
        "- Session state: planning_workflow={}, auto_permission={}, full_auto={}",
        turn_snapshot.planning_active, turn_snapshot.auto_permission, turn_snapshot.full_auto
    );
    let _ =
        writeln!(buf, "- Active primary permission default: {}", permission_default_label(agent.permissions.default));
    let _ = writeln!(buf, "- Effective request tools: {}", render_tool_names(tool_snapshot));
    if let Some(tools) = agent.tools.as_ref().filter(|tools| !tools.is_empty()) {
        let _ = writeln!(buf, "- Primary-agent tool allow-list: {}", tools.join(", "));
    }
    if !agent.disallowed_tools.is_empty() {
        let _ = writeln!(buf, "- Primary-agent disallowed tools: {}", agent.disallowed_tools.join(", "));
    }
    if !agent.skills.is_empty() {
        if let Some(prompt_context) = agent_prompt_context {
            if prompt_context.available_skill_metadata.is_empty() {
                let _ = writeln!(buf, "- Active primary skills: {}", agent.skills.join(", "));
            } else {
                let mut names = prompt_context
                    .available_skill_metadata
                    .iter()
                    .map(|skill| skill.name.as_str())
                    .collect::<Vec<_>>();
                names.sort_unstable();
                let _ = writeln!(buf, "- Active primary skills: {}", names.join(", "));
            }
        } else {
            let _ = writeln!(buf, "- Active primary skills: {}", agent.skills.join(", "));
        }
    }
    if let Some(cfg) = ctx.vt_cfg
        && cfg.agent.include_temporal_context
    {
        let _ = writeln!(buf, "{}", generate_temporal_context(cfg.agent.temporal_context_use_utc).trim());
    }

    let _ = writeln!(buf, "### Instructions");
    let _ = writeln!(buf, "{}", agent.instructions.trim());
    if let Some(memory_appendix) = active_primary_agent_memory_appendix(ctx, agent) {
        let _ = writeln!(buf, "### Memory Appendix");
        let _ = writeln!(buf, "{memory_appendix}");
    }
    buf
}

fn active_primary_agent_memory_appendix(ctx: &TurnProcessingContext<'_>, agent: &ActivePrimaryAgent) -> Option<String> {
    match load_primary_memory_appendix(ctx.config.workspace.as_path(), agent.identity.name.as_str(), agent.memory) {
        Ok(appendix) => appendix,
        Err(err) => {
            tracing::warn!(
                agent_name = %agent.identity.name,
                error = %err,
                "Failed to load active primary-agent memory appendix"
            );
            None
        }
    }
}

fn render_tool_names(tool_snapshot: &SessionToolCatalogSnapshot) -> String {
    let Some(tools) = tool_snapshot.snapshot.as_deref() else {
        return "none".to_string();
    };
    if tools.is_empty() {
        return "none".to_string();
    }

    let mut result = String::new();
    for (i, tool) in tools.iter().enumerate() {
        if i > 0 {
            result.push_str(", ");
        }
        result.push_str(
            tool.function
                .as_ref()
                .map(|function| function.name.as_str())
                .unwrap_or(tool.tool_type.as_str()),
        );
    }
    result
}

fn permission_default_label(default: vtcode_config::core::permissions::PermissionDefault) -> &'static str {
    match default {
        vtcode_config::core::permissions::PermissionDefault::Ask => "ask",
        vtcode_config::core::permissions::PermissionDefault::Allow => "allow",
        vtcode_config::core::permissions::PermissionDefault::Auto => "auto",
        vtcode_config::core::permissions::PermissionDefault::Deny => "deny",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vtcode_core::core::agent::harness_kernel::SessionToolCatalogSnapshot;
    use vtcode_core::prompts::append_runtime_tool_prompt_sections;

    use super::{PromptAssemblyOutput, validate_prompt_output_alignment};
    use crate::agent::runloop::unified::turn::turn_processing::llm_request::snapshot::capture_turn_request_snapshot;
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

    #[tokio::test]
    async fn prompt_alignment_detects_stale_runtime_tool_catalog_metadata() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        let turn = capture_turn_request_snapshot(&mut ctx, "noop-model", false);

        let make_snapshot = || {
            SessionToolCatalogSnapshot::new(
                7,
                11,
                turn.planning_active,
                turn.request_user_input_enabled,
                Some(Arc::new(Vec::new())),
                false,
            )
        };

        let misaligned_prompt = format!(
            "Base prompt\n[Runtime Tool Catalog]\n- version: 1\n- epoch: 11\n- available_tools: 0\n- request_user_input_enabled: {}\n",
            turn.request_user_input_enabled
        );
        let misaligned_output = PromptAssemblyOutput {
            system_prompt: misaligned_prompt,
            tool_snapshot: make_snapshot(),
            agent_prompt_context: None,
        };

        let aligned_snapshot = make_snapshot();
        let mut aligned_prompt = "Base prompt".to_string();
        append_runtime_tool_prompt_sections(&mut aligned_prompt, &aligned_snapshot, true);
        let aligned_output = PromptAssemblyOutput {
            system_prompt: aligned_prompt,
            tool_snapshot: aligned_snapshot,
            agent_prompt_context: None,
        };

        let err = validate_prompt_output_alignment(&misaligned_output, &turn)
            .expect_err("stale runtime metadata should be rejected");
        assert!(err.should_rebuild_runtime_prompt());
        validate_prompt_output_alignment(&aligned_output, &turn).expect("aligned runtime metadata should pass");
    }
}
