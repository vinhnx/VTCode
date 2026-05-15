use std::path::Path;

use anyhow::Result;
use vtcode_core::prompts::{expand_prompt_template, find_prompt_template};
use vtcode_core::skills::{
    CommandSkillBackend, CommandSkillSpec, find_command_skill_by_slash_name,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::builtins::execute_built_in_command_skill;
use super::models::SlashCommandOutcome;
use super::parsing::{
    parse_analyze_scope, parse_prompt_template_args, parse_review_spec, split_command_and_args,
};

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

    Ok(SlashCommandOutcome::SubmitPrompt {
        prompt: format!("/{}", input.trim()),
    })
}

pub(crate) async fn execute_command_skill_by_name(
    slash_name: &str,
    input: &str,
    renderer: &mut AnsiRenderer,
    workspace: &Path,
) -> Result<SlashCommandOutcome> {
    let command_key = normalize_command_key(slash_name.trim());
    let Some(spec) = find_command_skill_by_slash_name(command_key) else {
        anyhow::bail!("unknown command skill '{}'", slash_name);
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
        "command" => {
            if args.trim().is_empty() {
                renderer.line(MessageStyle::Error, "Usage: /command <program> [args...]")?;
                return Ok(SlashCommandOutcome::Handled);
            }
            args.trim().to_string()
        }
        "review" => {
            if matches!(args.trim(), "--help" | "help") {
                renderer.line(
                    MessageStyle::Info,
                    "Usage: /review [--last-diff] [--target <expr>] [--style <style>] [--file <path> | files...]",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            if let Err(err) = parse_review_spec(args) {
                renderer.line(MessageStyle::Error, &err)?;
                renderer.line(
                    MessageStyle::Info,
                    "Usage: /review [--last-diff] [--target <expr>] [--style <style>] [--file <path> | files...]",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            args.trim().to_string()
        }
        "analyze" => {
            if matches!(args.trim(), "--help" | "help") {
                renderer.line(
                    MessageStyle::Info,
                    "Usage: /analyze [full|security|performance]",
                )?;
                return Ok(SlashCommandOutcome::Handled);
            }
            match parse_analyze_scope(args) {
                Ok(Some(scope)) => scope,
                Ok(None) => String::new(),
                Err(err) => {
                    renderer.line(MessageStyle::Error, &err)?;
                    renderer.line(
                        MessageStyle::Info,
                        "Usage: /analyze [full|security|performance]",
                    )?;
                    return Ok(SlashCommandOutcome::Handled);
                }
            }
        }
        _ => args.trim().to_string(),
    };

    Ok(SlashCommandOutcome::ManageSkills {
        action: crate::agent::runloop::SkillCommandAction::Use {
            name: skill_name.to_string(),
            input,
        },
    })
}

pub(in crate::agent::runloop::slash_commands) fn normalize_command_key(command_key: &str) -> &str {
    match command_key {
        "settings" | "setttings" => "config",
        "comman" => "command",
        "subprocesses" => "subprocess",
        "context" => "compact",
        other => other,
    }
}
