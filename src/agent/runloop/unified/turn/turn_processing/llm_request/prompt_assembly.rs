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

use vtcode_core::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use vtcode_core::core::agent::runner::prompt_alignment;
use vtcode_core::prompts::{
    PromptContext, append_deferred_tools_prompt_section, append_runtime_tool_prompt_sections_for_model,
    upsert_harness_limits_section,
};

use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

use super::snapshot::TurnRequestSnapshot;
use super::tool_shaping::{apply_primary_agent_policy_to_tool_snapshot, uses_out_of_band_copilot_tools};
use super::{prompt_runtime, prompt_sections};

pub(super) use prompt_runtime::render_primary_agent_runtime_context;

pub(super) struct PromptAssemblyInput<'a> {
    pub(super) turn: &'a TurnRequestSnapshot,
}

pub(super) struct PromptAssemblyOutput {
    pub(super) system_prompt: String,
    pub(super) tool_snapshot: SessionToolCatalogSnapshot,
    pub(super) agent_prompt_context: Option<PromptContext>,
    pub(super) few_shot_context: Option<String>,
}

/// Builds the tool-free recovery contract as a standalone tail-position
/// system message. Keeping this text out of the system prompt preserves the
/// prompt-cache prefix: a recovery dispatch's system prompt stays
/// byte-identical to the tool-enabled turns that gathered the evidence being
/// summarized. `build_turn_request` pushes the directive request-side and
/// never persists it, so it cannot outlive the recovery episode.
pub(super) fn recovery_mode_directive(planning_active: bool, recovery_reason: Option<&str>) -> String {
    let mut directive = String::from("[Recovery Mode]\n");
    directive.push_str("- tools_disabled: true\n");
    directive.push_str("- tool_definitions_remain_for_cache_stability_only: true\n");
    directive.push_str("- do_not_request_more_tools: true\n");
    directive.push_str("- do_not_emit_tool_calls: true\n");

    if planning_active {
        directive.push_str(
            "- answer_mode: emit one complete approval-ready plan from evidence already collected in this turn\n",
        );
        directive.push_str("- required_output: exactly one `<proposed_plan>` block\n");
        directive.push_str(
            "- required_headings: `## Summary`, `## Implementation Steps`, `## Test Cases and Validation`, `## Assumptions and Defaults`\n",
        );
        directive.push_str(
            "- step_format: `1. Action -> files: [concrete/path.rs] -> verify: [cargo check --locked]`; replace the examples with evidence-backed targets and one concrete command or observable check (build/test gates such as `cargo nextest run`, inspection commands such as `rg -n`/`sed -n`/`grep -n`, or an observable check); every step must name a concrete file, symbol, or behavior target and generic targets or vague checks are rejected\n",
        );
        directive.push_str(
            "- if evidence is incomplete: state a concrete assumption inside the plan; do not omit a required heading\n",
        );
        directive.push_str("- do_not_emit_prose_outside_plan: true\n");
    } else {
        directive.push_str("- answer_mode: summarize only from evidence already collected in this turn\n");
        directive.push_str("- if evidence is incomplete, say so explicitly\n");
        directive.push_str("- keep_response_brief: true\n");
    }

    if let Some(reason) = recovery_reason {
        let _ = writeln!(directive, "- recovery_reason: {reason}");
    }

    directive
}

#[cfg_attr(feature = "profiling", hotpath::measure)]
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
    let system_prompt_future =
        ctx.context_manager
            .build_system_prompt(crate::agent::runloop::unified::context_manager::SystemPromptParams {
                full_auto: input.turn.full_auto,
                planning_active: input.turn.planning_active,
                request_user_input_enabled: input.turn.request_user_input_enabled,
            });
    let mut system_prompt = {
        #[cfg(feature = "profiling")]
        {
            use tracing::Instrument as _;
            system_prompt_future
                .instrument(tracing::debug_span!(target: "vtcode.prompt", "prompt_assembly.base_prompt"))
                .await?
        }
        #[cfg(not(feature = "profiling"))]
        {
            system_prompt_future.await?
        }
    };

    let agent = &input.turn.active_primary_agent;
    vtcode_core::prompts::apply_coordinator_role_guidance(&mut system_prompt, agent.name() == "coordinator");
    let agent_prompt_context = if agent.skills.is_empty() {
        None
    } else {
        Some(prompt_sections::active_primary_agent_prompt_context(ctx, agent))
    };

    {
        #[cfg(feature = "profiling")]
        let _phase_span = tracing::debug_span!(target: "vtcode.prompt", "prompt_assembly.agent_context").entered();

        prompt_sections::append_active_primary_agent_skills(&mut system_prompt, agent, agent_prompt_context.as_ref());

        upsert_harness_limits_section(
            &mut system_prompt,
            input.turn.execution.max_tool_calls,
            input.turn.execution.max_tool_wall_clock_secs,
            input.turn.execution.max_tool_retries,
        );
    }

    let tool_snapshot = {
        // Keep the real tool catalog during tool-free recovery so the wire
        // tools array and the [Runtime Tool Catalog] section stay byte-stable
        // with tool-enabled turns (prompt-cache prefix reuse). The recovery
        // directive (a request-only tail message, see `build_turn_request`)
        // plus `tool_choice: none` (or Merge's omitted choice) prevent calls.
        if !input.turn.capabilities.tools {
            SessionToolCatalogSnapshot::new(
                ctx.tool_catalog.current_version(),
                ctx.tool_catalog.current_epoch(),
                input.turn.planning_active,
                input.turn.request_user_input_enabled,
                None,
                false,
            )
        } else {
            let stable_catalog = ctx
                .vt_cfg
                .map(|cfg| cfg.prompt_cache.stable_tool_catalog_across_modes)
                .unwrap_or(true);
            let base_snapshot_future = ctx.tool_catalog.filtered_snapshot_with_stats_ex(
                ctx.tools,
                input.turn.planning_active,
                input.turn.request_user_input_enabled,
                stable_catalog,
            );
            #[cfg(feature = "profiling")]
            let base_snapshot = {
                use tracing::Instrument as _;
                base_snapshot_future
                    .instrument(tracing::debug_span!(target: "vtcode.prompt", "prompt_assembly.tool_catalog"))
                    .await
            };
            #[cfg(not(feature = "profiling"))]
            let base_snapshot = base_snapshot_future.await;
            apply_primary_agent_policy_to_tool_snapshot(
                base_snapshot,
                &input.turn.active_primary_agent,
                &ctx.config.workspace,
                ctx.vt_cfg,
            )
        }
    };

    {
        #[cfg(feature = "profiling")]
        let _phase_span = tracing::debug_span!(target: "vtcode.prompt", "prompt_assembly.tool_sections").entered();

        append_runtime_tool_prompt_sections_for_model(
            &mut system_prompt,
            &tool_snapshot,
            !input.turn.prompt_cache_shaping_mode.is_enabled(),
            ctx.vt_cfg
                .map(|cfg| cfg.agent.shell_prompt_profile)
                .unwrap_or_default()
                .resolve_for_current_platform(),
            &**ctx.provider_client,
            &input.turn.active_model,
            ctx.vt_cfg,
        );

        if input.turn.client_local_tool_deferral {
            // Client-local deferral omits deferred tools from the wire payload
            // (see `build_turn_request`); tell the model what it can still
            // reach through the relevant discovery tool. `tool_snapshot` still
            // carries the full, un-filtered tool list at this point, so
            // `deferred_count`/namespace metadata reflect what is actually
            // being withheld this turn. Recovery keeps the same wire filter
            // for cache stability; tools are present but not callable
            // (`tool_choice: none` / Merge omits the choice).
            append_deferred_tools_prompt_section(
                &mut system_prompt,
                tool_snapshot.snapshot.as_deref().map_or(&[], |tools| tools.as_slice()),
            );
        }

        if tool_snapshot.has_tools() && uses_out_of_band_copilot_tools(&input.turn.provider_name) {
            prompt_sections::append_copilot_runtime_guidance(&mut system_prompt);
        }
    }

    // Section 18.3.3 of the agentic-AI guide: inject at most
    // DEFAULT_FEW_SHOT_BUDGET_TOKENS of relevant few-shot examples selected
    // from `.vtcode/prompts/examples/`. Skip in recovery mode: that path has
    // a dedicated output contract, and extra examples would distract from it.
    let few_shot_context = {
        if input.turn.tool_free_recovery {
            None
        } else {
            let few_shot_future = prompt_sections::build_few_shot_section(ctx);
            #[cfg(feature = "profiling")]
            {
                use tracing::Instrument as _;
                few_shot_future
                    .instrument(tracing::debug_span!(target: "vtcode.prompt", "prompt_assembly.few_shot"))
                    .await
            }
            #[cfg(not(feature = "profiling"))]
            {
                few_shot_future.await
            }
        }
    };

    Ok(PromptAssemblyOutput {
        system_prompt,
        tool_snapshot,
        agent_prompt_context,
        few_shot_context,
    })
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use vtcode_core::config::types::ShellPromptProfile;
    use vtcode_core::core::agent::harness_kernel::SessionToolCatalogSnapshot;
    use vtcode_core::prompts::append_runtime_tool_prompt_sections_for_profile;

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
            few_shot_context: None,
        };

        let aligned_snapshot = make_snapshot();
        let mut aligned_prompt = "Base prompt".to_string();
        append_runtime_tool_prompt_sections_for_profile(
            &mut aligned_prompt,
            &aligned_snapshot,
            true,
            ShellPromptProfile::Auto.resolve_for_current_platform(),
        );
        let aligned_output = PromptAssemblyOutput {
            system_prompt: aligned_prompt,
            tool_snapshot: aligned_snapshot,
            agent_prompt_context: None,
            few_shot_context: None,
        };

        let err = validate_prompt_output_alignment(&misaligned_output, &turn)
            .expect_err("stale runtime metadata should be rejected");
        assert!(err.should_rebuild_runtime_prompt());
        validate_prompt_output_alignment(&aligned_output, &turn).expect("aligned runtime metadata should pass");
    }

    #[test]
    fn planning_recovery_directive_requires_complete_plan_artifact() {
        let directive = super::recovery_mode_directive(true, Some("per-file read cap"));

        assert!(directive.contains("[Recovery Mode]"));
        assert!(directive.contains("tools_disabled: true"));
        assert!(directive.contains("exactly one `<proposed_plan>` block"));
        assert!(directive.contains("`## Summary`"));
        assert!(directive.contains("`## Implementation Steps`"));
        assert!(directive.contains("`## Test Cases and Validation`"));
        assert!(directive.contains("`## Assumptions and Defaults`"));
        assert!(directive.contains("Action -> files: [concrete/path.rs] -> verify: [cargo check --locked]"));
        assert!(directive.contains("recovery_reason: per-file read cap"));
        assert!(!directive.contains("keep_response_brief"));
    }

    #[test]
    fn execution_recovery_directive_keeps_brief_summary_contract() {
        let directive = super::recovery_mode_directive(false, Some("follow-up failure"));

        assert!(directive.contains("summarize only from evidence"));
        assert!(directive.contains("keep_response_brief: true"));
        assert!(!directive.contains("exactly one `<proposed_plan>` block"));
    }
}
