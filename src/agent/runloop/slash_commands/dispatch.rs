use std::path::Path;

use anyhow::Result;
use vtcode_core::prompts::{expand_prompt_template, find_prompt_template};
use vtcode_core::skills::{CommandSkillBackend, CommandSkillSpec, find_command_skill_by_slash_name};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::builtins::execute_built_in_command_skill;
use super::models::SlashCommandOutcome;
use super::parsing::{parse_prompt_template_args, parse_review_input, split_command_and_args};

pub(crate) async fn handle_slash_command(
    input: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(SlashCommandOutcome::Handled);
    }

    let (command, rest) = split_command_and_args(trimmed);
    let command_key = command.to_ascii_lowercase();
    let command_key = normalize_command_key(&command_key);
    let args = rest.trim();

    if let Some(section) = config_consolidated_section(command_key) {
        let section_args = if args.is_empty() {
            section.to_string()
        } else {
            format!("{section} {args}")
        };
        if let Some(spec) = find_command_skill_by_slash_name("config") {
            return execute_command_skill_spec(spec, &section_args, trimmed, renderer, workspace).await;
        }
    }

    if let Some(spec) = find_command_skill_by_slash_name(command_key) {
        return execute_command_skill_spec(spec, args, trimmed, renderer, workspace).await;
    }

    if let Some(template) = find_prompt_template(workspace, command_key).await {
        let template_args = match parse_prompt_template_args(args) {
            Ok(parsed) => parsed,
            Err(message) => {
                renderer.line(MessageStyle::Error, &message)?;
                return Ok(SlashCommandOutcome::Handled);
            }
        };
        let expanded = expand_prompt_template(&template.body, &template_args);
        return Ok(SlashCommandOutcome::ReplaceInput { content: expanded });
    }

    Ok(SlashCommandOutcome::SubmitPrompt { prompt: format!("/{}", input.trim()) })
}

pub(crate) async fn execute_command_skill_by_name(
    slash_name: &str,
    input: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    let command_key = normalize_command_key(slash_name.trim());
    if let Some(section) = config_consolidated_section(command_key) {
        let Some(spec) = find_command_skill_by_slash_name("config") else {
            anyhow::bail!("unknown command skill '{slash_name}'");
        };
        let section_args = if input.trim().is_empty() {
            section.to_string()
        } else {
            format!("{section} {}", input.trim())
        };
        return execute_command_skill_spec(spec, &section_args, input.trim(), renderer, workspace).await;
    }
    let Some(spec) = find_command_skill_by_slash_name(command_key) else {
        anyhow::bail!("unknown command skill '{slash_name}'");
    };

    execute_command_skill_spec(spec, input.trim(), input.trim(), renderer, workspace).await
}

async fn execute_command_skill_spec(
    spec: &'static CommandSkillSpec,
    args: &str,
    input: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    match spec.backend {
        CommandSkillBackend::TraditionalSkill { skill_name, .. } => {
            dispatch_traditional_command_skill(spec, skill_name, args, renderer)
        }
        CommandSkillBackend::BuiltInCommand { .. } => {
            execute_built_in_command_skill(spec, args, input, renderer, workspace).await
        }
    }
}

fn dispatch_traditional_command_skill(
    spec: &CommandSkillSpec,
    skill_name: &str,
    args: &str,
    renderer: &mut AnsiRenderer,
) -> Result<SlashCommandOutcome> {
    let input = match spec.slash_name {
        "review" => {
            if matches!(args.trim(), "--help" | "help") {
                renderer.line(
                    MessageStyle::Info,
                    "Usage: /review [instructions | --last-diff | --target <expr> | --file <path> | files...] [--style <style>]",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            // NL-first: free-form prose becomes review instructions; only
            // malformed CLI-looking input errors here.
            if let Err(err) = parse_review_input(args) {
                renderer.line(MessageStyle::Error, &err)?;
                renderer.line(
                    MessageStyle::Info,
                    "Usage: /review [instructions | --last-diff | --target <expr> | --file <path> | files...] [--style <style>]",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            args.trim().to_string()
        }
        _ => args.trim().to_string(),
    };

    Ok(SlashCommandOutcome::ManageSkills {
        action: crate::agent::runloop::SkillCommandAction::Use { name: skill_name.to_string(), input },
    })
}

pub(in crate::agent::runloop::slash_commands) fn normalize_command_key(command_key: &str) -> &str {
    match command_key {
        "settings" | "setttings" => "config",
        "subprocesses" => "subprocess",
        "context" => "compact",
        other => other,
    }
}

/// Hidden backward-compat aliases consolidated under `/config`.
///
/// These names no longer appear in the `/` palette or skill registry; typing
/// them still works by routing through `/config <section>`.
pub(in crate::agent::runloop::slash_commands) fn config_consolidated_section(
    command_key: &str,
) -> Option<&'static str> {
    match command_key {
        "permissions" => Some("permissions"),
        "tasks" => Some("tasks"),
        "jobs" => Some("jobs"),
        "log" => Some("log"),
        "subprocess" | "subprocesses" => Some("subprocess"),
        "notify" => Some("notify"),
        "checkup" | "doctor" => Some("checkup"),
        _ => None,
    }
}
