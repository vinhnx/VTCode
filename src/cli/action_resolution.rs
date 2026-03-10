use crate::startup::StartupContext;
use anyhow::Result;
use vtcode_core::cli::args::{AskCommandOptions, Cli, Commands};

use super::adapters::ask_options;

#[derive(Debug, Clone)]
pub(crate) enum ResolvedCliAction {
    Ask {
        prompt: Option<String>,
        options: AskCommandOptions,
    },
    FullAuto {
        prompt: String,
    },
    Resume {
        mode: crate::startup::SessionResumeMode,
    },
    Command(Commands),
    Chat,
}

pub(crate) fn resolve_action(
    args: &Cli,
    startup: &StartupContext,
    print_mode: Option<String>,
    potential_prompt: Option<String>,
) -> Result<ResolvedCliAction> {
    if let Some(print_value) = print_mode {
        return Ok(ResolvedCliAction::Ask {
            prompt: Some(crate::main_helpers::build_print_prompt(print_value)?),
            options: ask_options(args, None, startup.skip_confirmations),
        });
    }

    if let Some(prompt) = potential_prompt {
        return Ok(ResolvedCliAction::Ask {
            prompt: Some(prompt),
            options: ask_options(args, None, startup.skip_confirmations),
        });
    }

    if let Some(prompt) = startup.automation_prompt.clone() {
        return Ok(ResolvedCliAction::FullAuto { prompt });
    }

    if let Some(mode) = startup.session_resume.clone() {
        return Ok(ResolvedCliAction::Resume { mode });
    }

    Ok(match args.command.clone() {
        Some(command) => ResolvedCliAction::Command(command),
        None => ResolvedCliAction::Chat,
    })
}
