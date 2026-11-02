use std::sync::Arc;

use anyhow::Result;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::handlers::ToolModelCapabilities;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::SkillCommandOutcome;
use crate::agent::runloop::unified::tool_catalog::tool_catalog_change_notifier;
use crate::agent::runloop::unified::turn::utils::{enforce_history_limits, truncate_message_content};
use crate::agent::runloop::unified::ui_interaction::start_loading_status;

use crate::agent::runloop::unified::turn::session::slash_commands::{SlashCommandContext, SlashCommandControl};

pub(super) async fn apply_skill_command_outcome(
    ctx: &mut SlashCommandContext<'_>,
    outcome: SkillCommandOutcome,
) -> Result<SlashCommandControl> {
    match outcome {
        SkillCommandOutcome::Handled { message } => {
            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::LoadSkill { skill, message } => {
            if let Err(e) = skill_runtime(ctx).activate_skill(ctx.loaded_skills, skill).await {
                ctx.renderer
                    .line(MessageStyle::Error, &format!("Failed to activate skill: {e}"))?;
                return Ok(SlashCommandControl::Continue);
            }

            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::UnloadSkill { name } => {
            let removed = skill_runtime(ctx).deactivate_skill(ctx.loaded_skills, &name).await?;
            let message = if removed {
                format!("Unloaded skill: {name}")
            } else {
                format!("Skill was not active: {name}")
            };
            ctx.renderer.line(MessageStyle::Info, &message)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::UseSkill { skill, input } => {
            use vtcode_core::skills::execute_skill_with_sub_llm;

            let skill_name = skill.name().to_string();
            let status_message = skill_invocation_status_message(&skill_name);
            let available_tools = ctx.tools.read().await.clone();
            let model = ctx.config.model.clone();
            let loading_spinner = ctx
                .renderer
                .supports_inline_ui()
                .then(|| start_loading_status(ctx.handle, ctx.input_status_state, status_message.clone()));

            ctx.renderer.line(MessageStyle::Info, &status_message)?;

            let started = std::time::Instant::now();
            let run_result = execute_skill_with_sub_llm(
                &skill,
                input,
                ctx.provider_client.as_ref(),
                ctx.tool_registry,
                available_tools,
                model,
            )
            .await;
            let collector = ctx.tool_registry.metrics_collector();
            collector.record_skill_created(skill_name.clone(), skill.variety.as_str().to_string());
            let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            collector.record_skill_execution(skill_name.clone(), duration_ms, run_result.is_ok());

            match run_result {
                Ok(result) => {
                    drop(loading_spinner);
                    ctx.renderer.render_markdown_output(MessageStyle::Output, &result)?;
                    ctx.conversation_history
                        .push(uni::Message::user(format!("/skills use {skill_name} [executed]")));

                    let limited = truncate_message_content(&result);
                    ctx.conversation_history.push(uni::Message::assistant(limited));
                    enforce_history_limits(ctx.conversation_history);
                    Ok(SlashCommandControl::Continue)
                }
                Err(e) => {
                    drop(loading_spinner);
                    ctx.renderer
                        .line(MessageStyle::Error, &format!("Failed to execute skill: {e}"))?;
                    Ok(SlashCommandControl::Continue)
                }
            }
        }
        SkillCommandOutcome::UseBuiltInCommand { name, slash_name, input } => {
            let status_message = skill_invocation_status_message(&name);
            let loading_spinner = ctx
                .renderer
                .supports_inline_ui()
                .then(|| start_loading_status(ctx.handle, ctx.input_status_state, status_message.clone()));

            ctx.renderer.line(MessageStyle::Info, &status_message)?;
            let outcome = crate::agent::runloop::slash_commands::execute_command_skill_by_name(
                &slash_name,
                &input,
                ctx.renderer,
                &ctx.config.workspace,
            )
            .await?;
            drop(loading_spinner);
            let control =
                crate::agent::runloop::unified::turn::session::slash_commands::handle_outcome(outcome, ctx.reborrow())
                    .await?;
            match control {
                SlashCommandControl::Continue => Ok(SlashCommandControl::Continue),
                SlashCommandControl::BackgroundCompletionHandled { completion_identity } => {
                    Ok(SlashCommandControl::BackgroundCompletionHandled { completion_identity })
                }
                SlashCommandControl::SubmitPrompt(prompt) => Ok(SlashCommandControl::SubmitPrompt(prompt)),
                SlashCommandControl::ReplaceInput(content) => Ok(SlashCommandControl::ReplaceInput(content)),
                SlashCommandControl::BreakWithReason(reason) => {
                    ctx.renderer
                        .line(MessageStyle::Info, &format!("Built-in command skill '{name}' exited the session."))?;
                    Ok(SlashCommandControl::BreakWithReason(reason))
                }
                SlashCommandControl::SelectAgent(name) => Ok(SlashCommandControl::SelectAgent(name)),
            }
        }
        SkillCommandOutcome::Doctor => {
            let report = ctx.tool_registry.metrics_collector().get_skill_metrics().doctor_report();
            ctx.renderer.line(MessageStyle::Info, &report)?;
            Ok(SlashCommandControl::Continue)
        }
        SkillCommandOutcome::Error { message } => {
            ctx.renderer.line(MessageStyle::Error, &message)?;
            Ok(SlashCommandControl::Continue)
        }
    }
}

pub(super) fn skill_invocation_status_message(skill_name: &str) -> String {
    let invocation_target = skill_name
        .strip_prefix("cmd-")
        .map(|slash_name| format!("/{slash_name}"))
        .unwrap_or_else(|| skill_name.to_string());
    format!("Running {invocation_target}...")
}

fn skill_runtime(ctx: &SlashCommandContext<'_>) -> vtcode_core::tools::skills::SkillToolSessionRuntime {
    skill_runtime_for_session(ctx.tool_registry, ctx.tools, ctx.tool_catalog, ctx.config, ctx.vt_cfg.as_ref())
}

pub(super) fn skill_runtime_for_session(
    tool_registry: &vtcode_core::tools::ToolRegistry,
    tools: &Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
    tool_catalog: &Arc<crate::agent::runloop::unified::tool_catalog::ToolCatalogState>,
    config: &vtcode_core::config::types::AgentConfig,
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> vtcode_core::tools::skills::SkillToolSessionRuntime {
    let tool_documentation_mode = vt_cfg.map(|cfg| cfg.agent.tool_documentation_mode).unwrap_or_default();

    vtcode_core::tools::skills::SkillToolSessionRuntime::new(
        Arc::new(tool_registry.clone()),
        Some(Arc::clone(tools)),
        tool_documentation_mode,
        ToolModelCapabilities::for_model_name(&config.model),
        Some(tool_catalog_change_notifier(tool_catalog)),
    )
    .with_tool_profile(configured_tool_profile(vt_cfg))
    .with_fork_executor(Arc::new(vtcode_core::skills::executor::ChildAgentSkillExecutor::new(
        Arc::new(tool_registry.clone()),
        vtcode_core::skills::executor::ForkSkillRuntimeConfig {
            workspace: config.workspace.clone(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            openai_chatgpt_auth: config.openai_chatgpt_auth.clone(),
            vt_cfg: vt_cfg.cloned(),
        },
    )))
}

pub(super) fn configured_tool_profile(
    config: Option<&vtcode_core::config::loader::VTCodeConfig>,
) -> vtcode_core::config::ToolProfile {
    config.map(|cfg| cfg.tools.profile).unwrap_or_default()
}
