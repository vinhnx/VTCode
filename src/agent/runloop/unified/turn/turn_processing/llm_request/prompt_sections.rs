//! Independently testable prompt sections used by the assembly orchestrator.
//!
//! These helpers own section-local formatting and resource selection. They do
//! not decide the order of sections or construct the provider request; those
//! responsibilities stay in [`super::prompt_assembly`].

use std::fmt::Write as _;

use dirs::home_dir;
use vtcode_core::ActivePrimaryAgent;
use vtcode_core::apply_primary_agent_prompt_context;
use vtcode_core::llm::provider as uni;
use vtcode_core::prompts::{DEFAULT_FEW_SHOT_BUDGET_TOKENS, FewShotStore, PromptContext, render_few_shot_section};

use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

/// Build the few-shot section for the current turn.
///
/// Returns `None` when no examples match or when selection is skipped (no
/// query available, empty store). The selection uses keyword-tag overlap with
/// the most recent user message and is bounded by
/// [`DEFAULT_FEW_SHOT_BUDGET_TOKENS`].
pub(super) async fn build_few_shot_section(ctx: &TurnProcessingContext<'_>) -> Option<String> {
    let query = latest_user_query(ctx.working_history.as_slice())?;
    let store = FewShotStore::load_async(Some(ctx.config.workspace.as_path()), home_dir().as_deref()).await;
    if store.is_empty() {
        return None;
    }
    let chosen = store.select(&query, DEFAULT_FEW_SHOT_BUDGET_TOKENS);
    if chosen.is_empty() {
        return None;
    }
    Some(render_few_shot_section(&chosen))
}

/// Return the text of the most recent user message in `history`.
fn latest_user_query(history: &[uni::Message]) -> Option<String> {
    use crate::agent::runloop::unified::turn::context::last_user_message;
    last_user_message(history)
        .map(|message| message.content.as_text().into_owned())
        .filter(|text: &String| !text.trim().is_empty())
}

pub(super) fn append_copilot_runtime_guidance(system_prompt: &mut String) {
    let _ = writeln!(
        system_prompt,
        "\n[GitHub Copilot Client Tools]\n- the VT Code tools named in this prompt are exposed as Copilot client tools outside the normal JSON tool list\n- when a tool is needed, emit the actual client tool call instead of describing the call in plain text\n- do not claim a tool was rejected, blocked, or unavailable unless the runtime returned that result"
    );
}

pub(super) fn active_primary_agent_prompt_context(
    ctx: &TurnProcessingContext<'_>,
    agent: &ActivePrimaryAgent,
) -> PromptContext {
    let mut prompt_context =
        PromptContext::from_workspace_tools(ctx.config.workspace.as_path(), std::iter::empty::<String>());
    apply_primary_agent_prompt_context(&mut prompt_context, agent);
    prompt_context
}

pub(super) fn append_active_primary_agent_skills(
    system_prompt: &mut String,
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
