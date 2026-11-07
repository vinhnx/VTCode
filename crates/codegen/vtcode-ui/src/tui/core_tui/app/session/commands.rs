use super::*;

pub(super) fn to_core_command(command: &InlineCommand) -> Option<crate::tui::core_tui::types::InlineCommand> {
    use crate::tui::core_tui::types::InlineCommand as CoreCommand;

    Some(match command {
        InlineCommand::Diagnostic(diagnostic) => CoreCommand::Diagnostic(diagnostic.clone()),
        InlineCommand::AppendLine { kind, segments } => {
            CoreCommand::AppendLine { kind: *kind, segments: segments.clone() }
        }
        InlineCommand::AppendLines { lines } => CoreCommand::AppendLines { lines: lines.clone() },
        InlineCommand::AppendPastedMessage { kind, text, line_count } => CoreCommand::AppendPastedMessage {
            kind: *kind,
            text: text.clone(),
            line_count: *line_count,
        },
        InlineCommand::Inline { kind, segment } => CoreCommand::Inline { kind: *kind, segment: segment.clone() },
        InlineCommand::ReplaceLast { count, kind, lines, link_ranges } => CoreCommand::ReplaceLast {
            count: *count,
            kind: *kind,
            lines: lines.clone(),
            link_ranges: link_ranges.clone(),
        },
        InlineCommand::RecordToolOutput { .. }
        | InlineCommand::FocusTranscriptReview { .. }
        | InlineCommand::AppendToolOutputLine { .. }
        | InlineCommand::AppendCompactActivity(_)
        | InlineCommand::ReplaceCompactActivity(_)
        | InlineCommand::CollapsePtyBlock(_)
        | InlineCommand::SetKeyBindings { .. } => return None,
        InlineCommand::SetPrompt { prefix, style } => {
            CoreCommand::SetPrompt { prefix: prefix.clone(), style: style.clone() }
        }
        InlineCommand::SetPlaceholder { hint, style } => {
            CoreCommand::SetPlaceholder { hint: hint.clone(), style: style.clone() }
        }
        InlineCommand::SetMessageLabels { agent, user } => {
            CoreCommand::SetMessageLabels { agent: agent.clone(), user: user.clone() }
        }
        InlineCommand::SetHeaderContext { context } => CoreCommand::SetHeaderContext { context: context.clone() },
        InlineCommand::SetInputStatus { left, right } => {
            CoreCommand::SetInputStatus { left: left.clone(), right: right.clone() }
        }
        InlineCommand::SetConfiguredInputStatus { left, right } => {
            CoreCommand::SetConfiguredInputStatus { left: left.clone(), right: right.clone() }
        }
        InlineCommand::ProgramStatus(update) => CoreCommand::ProgramStatus(*update),
        InlineCommand::SetActivityState(state) => CoreCommand::SetActivityState(*state),
        InlineCommand::UpdateProgress(update) => CoreCommand::UpdateProgress(*update),
        InlineCommand::SetTerminalTitleItems { items } => CoreCommand::SetTerminalTitleItems { items: items.clone() },
        InlineCommand::SetTerminalTitleThreadLabel { label } => {
            CoreCommand::SetTerminalTitleThreadLabel { label: label.clone() }
        }
        InlineCommand::SetTerminalTitleGitBranch { branch } => {
            CoreCommand::SetTerminalTitleGitBranch { branch: branch.clone() }
        }
        InlineCommand::SetTheme { theme } => CoreCommand::SetTheme { theme: theme.clone() },
        InlineCommand::SetColorSchemeAuto { enabled } => CoreCommand::SetColorSchemeAuto { enabled: *enabled },
        InlineCommand::SetAppearance { appearance } => CoreCommand::SetAppearance { appearance: appearance.clone() },
        InlineCommand::SetFullscreenInteraction { interaction } => {
            CoreCommand::SetFullscreenInteraction { interaction: *interaction }
        }
        InlineCommand::SetVimModeEnabled(enabled) => CoreCommand::SetVimModeEnabled(*enabled),
        InlineCommand::SetQueuedInputs { entries } => CoreCommand::SetQueuedInputs { entries: entries.clone() },
        InlineCommand::SetSubprocessEntries { entries } => {
            CoreCommand::SetSubprocessEntries { entries: entries.clone() }
        }
        InlineCommand::SetSubagentPreview { text } => CoreCommand::SetSubagentPreview { text: text.clone() },
        InlineCommand::SetLocalAgents { .. } => return None,
        InlineCommand::SetActivePlan { .. } => return None,
        InlineCommand::SetArchivedHistory { .. } => return None,
        InlineCommand::UpdateFilePaletteSearch { .. } => return None,
        InlineCommand::SetSlashCommands { .. } => return None,
        InlineCommand::SetPrimaryAgent { name, color } => {
            CoreCommand::SetPrimaryAgent { name: name.clone(), color: color.clone() }
        }
        InlineCommand::SetCursorVisible(value) => CoreCommand::SetCursorVisible(*value),
        InlineCommand::SetInputEnabled(value) => CoreCommand::SetInputEnabled(*value),
        InlineCommand::SetImageInputEnabled(value) => CoreCommand::SetImageInputEnabled(*value),
        InlineCommand::SetInput(value) => CoreCommand::SetInput(value.clone()),
        InlineCommand::RestoreInputDraft(input) => CoreCommand::RestoreInputDraft(input.clone()),
        InlineCommand::ApplySuggestedPrompt(value) => CoreCommand::ApplySuggestedPrompt(value.clone()),
        InlineCommand::SetInlinePromptSuggestion { suggestion, llm_generated } => {
            CoreCommand::SetInlinePromptSuggestion {
                suggestion: suggestion.clone(),
                llm_generated: *llm_generated,
            }
        }
        InlineCommand::ClearInlinePromptSuggestion => CoreCommand::ClearInlinePromptSuggestion,
        InlineCommand::ClearInput => CoreCommand::ClearInput,
        InlineCommand::ForceRedraw => CoreCommand::ForceRedraw,
        InlineCommand::ClearScreen => CoreCommand::ClearScreen,
        InlineCommand::SuspendEventLoop => CoreCommand::SuspendEventLoop,
        InlineCommand::ResumeEventLoop => CoreCommand::ResumeEventLoop,
        InlineCommand::ClearInputQueue => CoreCommand::ClearInputQueue,
        InlineCommand::StopEventStream => CoreCommand::StopEventStream,
        InlineCommand::StartEventStream => CoreCommand::StartEventStream,
        InlineCommand::SetSkipConfirmations(skip) => CoreCommand::SetSkipConfirmations(*skip),
        InlineCommand::Shutdown => CoreCommand::Shutdown,
        InlineCommand::SetReasoningStage(stage) => CoreCommand::SetReasoningStage(stage.clone()),
        InlineCommand::ShowTransient { .. } | InlineCommand::CloseTransient | InlineCommand::RecordDiffReview(_) => {
            return None;
        }
    })
}
