use anyhow::{Result, Context};
use std::sync::Arc;
use vtcode_core::config::types::CapabilityLevel;
use vtcode_core::skills::executor::SkillToolAdapter;
use vtcode_core::tools::ToolRegistration;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::llm::provider as uni;
use crate::agent::runloop::handle_skill_command;
use crate::agent::runloop::{SkillCommandAction, SkillCommandOutcome};
use crate::agent::runloop::unified::turn::utils::{enforce_history_limits, truncate_message_content};
use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_manage_skills(
    ctx: &SlashCommandContext<'_>,
    action: SkillCommandAction,
) -> Result<SlashCommandControl> {
    let outcome = handle_skill_command(action, ctx.config.workspace.clone()).await?;

    match outcome {
        SkillCommandOutcome::Handled { message } => {
            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::LoadSkill { skill, message } => {
            let skill_name = skill.name().to_string();

            // Create adapter and register as tool in tool registry
            let adapter = SkillToolAdapter::new(skill.clone());
            let adapter_arc = Arc::new(adapter);

            // SAFETY: skill_name is converted to static for Tool trait.
            // The ToolAdapter's name() method already returns 'static.
            let name_static: &'static str = Box::leak(Box::new(skill_name.clone()));

            let registration = ToolRegistration::from_tool(
                name_static,
                CapabilityLevel::Bash,
                adapter_arc,
            );

            if let Err(e) = ctx.tool_registry.register_tool(registration) {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to register skill as tool: {}", e),
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            // Store in session loaded skills registry
            ctx.loaded_skills
                .write()
                .await
                .insert(skill_name.clone(), skill.clone());

            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::UnloadSkill { name } => {
            // Remove from loaded skills registry
            ctx.loaded_skills.write().await.remove(&name);

            // Unregister from tool registry (if support exists)
            // Note: Current tool registry does not support dynamic unregistration.
            // Future enhancement: Add tool unregistration support to `ToolRegistry`.

            ctx.renderer
                .line(MessageStyle::Info, &format!("Unloaded skill: {}", name))?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::UseSkill { skill, input } => {
            // Phase 5: Execute skill with LLM sub-call support
            use vtcode_core::skills::execute_skill_with_sub_llm;

            let skill_name = skill.name().to_string();
            let available_tools = ctx.tools.read().await.clone();
            let model = ctx.config.model.clone();

            // Execute skill with LLM sub-calls
            match execute_skill_with_sub_llm(
                &skill,
                input,
                ctx.provider_client.as_ref(),
                ctx.tool_registry,
                available_tools,
                model,
            )
            .await
            {
                Ok(result) => {
                    // Display result to user
                    ctx.renderer.line(MessageStyle::Output, &result)?;

                    // Add to conversation history for context
                    ctx.conversation_history.push(uni::Message::user(format!(
                        "/skills use {} [executed]",
                        skill_name
                    )));

                    let result_string: String = result;
                    let limited = truncate_message_content(&result_string);
                    ctx.conversation_history
                        .push(uni::Message::assistant(limited));
                    enforce_history_limits(&mut ctx.conversation_history);

                    Ok(SlashCommandControl::Continue)
                }
                Err(e) => {
                    ctx.renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to execute skill: {}", e),
                    )?;
                    Ok(SlashCommandControl::Continue)
                }
            }
        }
        SkillCommandOutcome::Error { message } => {
            ctx.renderer.line(MessageStyle::Error, &message)?;
            Ok(SlashCommandControl::Continue)
        }
    }
}
