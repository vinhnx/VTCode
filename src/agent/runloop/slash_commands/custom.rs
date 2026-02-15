use std::collections::BTreeMap;

use anyhow::{Context, Result};
use shell_words::split as shell_split;
use vtcode_core::prompts::{
    CustomPrompt, CustomPromptRegistry, CustomSlashCommandRegistry, PromptInvocation,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::SlashCommandOutcome;

pub(super) fn handle_custom_prompt(
    name: &str,
    args: &str,
    renderer: &mut AnsiRenderer,
    registry: &CustomPromptRegistry,
) -> Result<SlashCommandOutcome> {
    if !registry.enabled() {
        renderer.line(
            MessageStyle::Error,
            "Custom prompts are disabled. Set `agent.custom_prompts.enabled = true` in vtcode.toml.",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }

    if registry.is_empty() {
        renderer.line(
            MessageStyle::Error,
            "No custom prompts found. Create markdown files in your custom prompts directory or run /prompt (or /prompts) for setup guidance.",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }

    let prompt = match registry.get(name) {
        Some(prompt) => prompt,
        None => {
            renderer.line(
                MessageStyle::Error,
                &format!(
                    "Unknown custom prompt `{}`. Run /prompt (or /prompts) to list available prompts.",
                    name
                ),
            )?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    let invocation = match PromptInvocation::parse(args) {
        Ok(invocation) => invocation,
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to parse arguments: {}", err),
            )?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    match prompt.expand(&invocation) {
        Ok(expanded) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Expanding custom prompt /prompt:{}", prompt.name),
            )?;
            Ok(SlashCommandOutcome::SubmitPrompt { prompt: expanded })
        }
        Err(err) => {
            renderer.line(MessageStyle::Error, &err.to_string())?;
            Ok(SlashCommandOutcome::Handled)
        }
    }
}

pub(super) fn handle_custom_slash_command(
    name: &str,
    args: &str,
    renderer: &mut AnsiRenderer,
    registry: &CustomSlashCommandRegistry,
) -> Result<SlashCommandOutcome> {
    if !registry.enabled() {
        renderer.line(
            MessageStyle::Error,
            "Custom slash commands are disabled. Enable them in configuration.",
        )?;
        return Ok(SlashCommandOutcome::Handled);
    }

    let command = match registry.get(name) {
        Some(command) => command,
        None => {
            renderer.line(
                MessageStyle::Error,
                &format!("Unknown custom slash command `{}`.", name),
            )?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    // Parse arguments similar to how custom prompts work
    let invocation = match parse_command_arguments(args) {
        Ok(invocation) => invocation,
        Err(err) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to parse arguments: {}", err),
            )?;
            return Ok(SlashCommandOutcome::Handled);
        }
    };

    // Check if the command has bash execution (contains !`command`)
    if command.has_bash_execution {
        renderer.line(
            MessageStyle::Error,
            &format!(
                "Command `{}` contains bash execution which is not yet supported in this implementation.",
                name
            ),
        )?;
        // For now, we'll just expand the content without executing bash commands
        let expanded = expand_command_content_with_args(&command.content, &invocation);
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Expanding custom slash command /{} (bash execution skipped)",
                command.name
            ),
        )?;
        return Ok(SlashCommandOutcome::SubmitPrompt { prompt: expanded });
    }

    let expanded = expand_command_content_with_args(&command.content, &invocation);
    renderer.line(
        MessageStyle::Info,
        &format!("Expanding custom slash command /{}", command.name),
    )?;
    Ok(SlashCommandOutcome::SubmitPrompt { prompt: expanded })
}

// Parse arguments for custom slash commands (similar to custom prompts)
fn parse_command_arguments(raw: &str) -> Result<CommandInvocation> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(CommandInvocation::default());
    }

    let tokens = shell_split(trimmed)
        .with_context(|| "failed to parse custom slash command arguments".to_owned())?;

    let mut positional = Vec::new();
    let mut named = BTreeMap::new();
    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            let key_trimmed = key.trim();
            if key_trimmed.is_empty() {
                positional.push(token);
            } else {
                named.insert(key_trimmed.to_owned(), value.to_owned());
            }
        } else {
            positional.push(token);
        }
    }

    let all_arguments = if positional.is_empty() {
        None
    } else {
        Some(positional.join(" "))
    };

    Ok(CommandInvocation {
        positional,
        named,
        all_arguments,
    })
}

#[derive(Debug, Clone, Default)]
struct CommandInvocation {
    positional: Vec<String>,
    named: BTreeMap<String, String>,
    all_arguments: Option<String>,
}

impl CommandInvocation {
    fn all_arguments(&self) -> Option<&str> {
        self.all_arguments.as_deref()
    }

    fn positional(&self) -> &[String] {
        &self.positional
    }

    fn named(&self) -> &BTreeMap<String, String> {
        &self.named
    }
}

fn expand_command_content_with_args(content: &str, invocation: &CommandInvocation) -> String {
    let mut result = content.to_string();

    // Replace $ARGUMENTS with all arguments
    if let Some(all_args) = invocation.all_arguments() {
        result = result.replace("$ARGUMENTS", all_args);
    }

    // Replace $1, $2, etc. with positional arguments
    for (i, arg) in invocation.positional().iter().enumerate() {
        let placeholder = format!("${}", i + 1);
        result = result.replace(&placeholder, arg);
    }

    // Replace named placeholders like $FILE, $TASK, etc.
    for (key, value) in invocation.named() {
        let placeholder = format!("${}", key);
        result = result.replace(&placeholder, value);
    }

    // Replace $$ with literal $
    result = result.replace("$$", "$");

    result
}

pub(super) fn render_custom_prompt_list(
    renderer: &mut AnsiRenderer,
    registry: &CustomPromptRegistry,
) -> Result<()> {
    if !registry.enabled() {
        renderer.line(
            MessageStyle::Info,
            "Custom prompts are disabled. Enable them with `agent.custom_prompts.enabled = true` in vtcode.toml.",
        )?;
        return Ok(());
    }

    if registry.is_empty() {
        renderer.line(
            MessageStyle::Info,
            "No custom prompts are registered yet. Add .md files to your prompts directory and restart the session.",
        )?;
    } else {
        renderer.line(
            MessageStyle::Info,
            "Custom prompts available (invoke with /prompt:<name>):",
        )?;
        for prompt in registry.iter() {
            render_prompt_summary(renderer, prompt)?;
        }
    }

    if !registry.directories().is_empty() {
        let (existing_dirs, missing_dirs): (Vec<_>, Vec<_>) = registry
            .directories()
            .iter()
            .partition(|path| path.exists());

        if !existing_dirs.is_empty() {
            renderer.line(MessageStyle::Info, "Prompt directories:")?;
            for path in existing_dirs {
                renderer.line(MessageStyle::Info, &format!("  - {}", path.display()))?;
            }
        }

        if !missing_dirs.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "Configured prompt directories (create these to enable discovery):",
            )?;
            for path in missing_dirs {
                renderer.line(MessageStyle::Info, &format!("  - {}", path.display()))?;
            }
        }
    }

    Ok(())
}

fn render_prompt_summary(renderer: &mut AnsiRenderer, prompt: &CustomPrompt) -> Result<()> {
    let mut line = format!("  /prompt:{}", prompt.name);
    if let Some(description) = &prompt.description
        && !description.trim().is_empty()
    {
        line.push_str(" — ");
        line.push_str(description.trim());
    }
    renderer.line(MessageStyle::Info, &line)?;

    if let Some(hint) = &prompt.argument_hint
        && !hint.trim().is_empty()
    {
        renderer.line(MessageStyle::Info, &format!("      hint: {}", hint.trim()))?;
    }

    Ok(())
}
