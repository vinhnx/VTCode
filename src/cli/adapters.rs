use vtcode::startup::StartupContext;
use vtcode_core::cli::args::{AskCommandOptions, AskOutputFormat, Cli};

use super::SkillsCommandOptions;

pub(super) fn ask_options(
    args: &Cli,
    output_format: Option<AskOutputFormat>,
    skip_confirmations: bool,
) -> AskCommandOptions {
    AskCommandOptions {
        output_format,
        allowed_tools: args.allowed_tools.clone(),
        disallowed_tools: args.disallowed_tools.clone(),
        skip_confirmations,
    }
}

pub(super) fn skills_options(startup: &StartupContext) -> SkillsCommandOptions {
    SkillsCommandOptions {
        workspace: startup.workspace.clone(),
    }
}
