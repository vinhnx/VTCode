use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;

use crate::agent::runloop::git::normalize_workspace_path;
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::inline_events::harness::{turn_completed_event, turn_started_event};
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::shell::shell_quote_if_needed;
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
use crate::agent::runloop::unified::turn::session::direct_tool_completion::{
    ReplyKind, generate_completion_reply_with_suggestions, latest_direct_background_completion_identity,
};
use crate::agent::runloop::unified::turn::session::interaction_loop::{InteractionLoopContext, InteractionOutcome};
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::{ToolOutcomeContext, handle_single_tool_call};
use vtcode_commons::formatting::{strip_optional_word_prefix, trim_wrapping_quotes_and_punctuation};
use vtcode_config::SubagentSpec;
use vtcode_core::command_safety::shell_parser::parse_shell_commands_tree_sitter;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;
use vtcode_core::session::SessionId;
use vtcode_core::subagents::delegated_task_requires_clarification;

struct DirectSubagentInvocation {
    tool_name: &'static str,
    args: serde_json::Value,
}

pub(crate) struct DirectToolContext<'a, 'b> {
    pub interaction_ctx: &'b mut InteractionLoopContext<'a>,
    pub input_status_state: &'b mut InputStatusState,
}

enum DirectToolInput {
    Execute {
        tool_name: String,
        args: serde_json::Value,
        is_bang_prefix: bool,
    },
    InvalidBang {
        command: String,
        diagnosis: String,
    },
}

fn begin_direct_tool_turn(harness_config: &vtcode_config::core::agent::AgentHarnessConfig) -> HarnessTurnState {
    let direct_turn_id = SessionId::generate();
    let direct_turn_id_str = direct_turn_id.as_str().to_string();
    HarnessTurnState::new(
        TurnRunId(direct_turn_id_str.clone()),
        TurnId(direct_turn_id_str),
        harness_config.max_tool_wall_clock_secs,
        harness_config.max_tool_retries,
    )
}

pub(crate) async fn handle_direct_tool_execution(
    input: &str,
    ctx: &mut DirectToolContext<'_, '_>,
) -> Result<Option<InteractionOutcome>> {
    let normalized_input = normalize_direct_tool_mentions(input, &ctx.interaction_ctx.config.workspace);
    if let Some(invocation) = detect_direct_subagent_spawn_input(&normalized_input, ctx).await? {
        return execute_direct_tool_call(input, invocation.tool_name, invocation.args, false, ctx).await;
    }
    let Some(parsed) = parse_direct_tool_input(&normalized_input) else {
        return Ok(None);
    };

    let (tool_name_str, args, is_bang_prefix) = match parsed {
        DirectToolInput::Execute { tool_name, args, is_bang_prefix } => (tool_name, args, is_bang_prefix),
        DirectToolInput::InvalidBang { command, diagnosis } => {
            // TUI stays concise: one rejection line + one actionable line.
            // The full parser diagnosis is kept in trace logs for forensics.
            tracing::debug!(command = %command, diagnosis = %diagnosis, "rejected invalid bang shell command");
            let detail: String = if diagnosis.chars().count() > 160 {
                format!("{}…", diagnosis.chars().take(159).collect::<String>())
            } else {
                diagnosis.clone()
            };
            ctx.interaction_ctx.renderer.line(
                vtcode_tui_runtime::utils::ansi::MessageStyle::Info,
                "Shell mode (!): command rejected (invalid shell syntax).",
            )?;
            ctx.interaction_ctx.renderer.line(
                vtcode_tui_runtime::utils::ansi::MessageStyle::Info,
                &format!("Fix syntax and retry as `!{command}`, or ask without `!`. ({detail})"),
            )?;
            return Ok(Some(InteractionOutcome::DirectToolHandled));
        }
    };

    execute_direct_tool_call(input, &tool_name_str, args, is_bang_prefix, ctx).await
}

#[cfg_attr(feature = "profiling", hotpath::measure)]
pub(crate) async fn execute_direct_tool_call(
    input: &str,
    tool_name: &str,
    args: serde_json::Value,
    is_bang_prefix: bool,
    ctx: &mut DirectToolContext<'_, '_>,
) -> Result<Option<InteractionOutcome>> {
    // Construct HarnessTurnState (simplified for direct execution)
    let mut harness_state = begin_direct_tool_turn(&ctx.interaction_ctx.harness_config);
    let direct_turn_id = harness_state.turn_id.0.clone();

    // A directly executed tool is still a turn. Publish the canonical
    // lifecycle so downstream consumers (notably the desktop client, which
    // clears `running` only on a terminal turn event) see it start and finish.
    // Also register the fresh id so tool events are not mis-attributed to the
    // previously completed turn.
    if let Some(emitter) = ctx.interaction_ctx.harness_emitter {
        let _ = emitter.begin_task_turn(&direct_turn_id, input, vtcode_core::exec::events::InputOrigin::User);
        let _ = emitter.emit(turn_started_event());
    }

    let mut auto_finish_planning_attempted = false;

    // Construct TurnProcessingContext to leverage unified execution handlers
    let mut tp_ctx = ctx.interaction_ctx.as_turn_processing_context(
        &mut harness_state,
        &mut auto_finish_planning_attempted,
        ctx.input_status_state,
        direct_tool_skips_confirmations(tool_name),
    );

    let turn_modified_files = {
        let mut repeated_tool_attempts =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker::with_verification_snapshot(
                tp_ctx.session_stats.verification_snapshot(),
            );
        let mut turn_modified_files = BTreeSet::new();
        let mut t_ctx = ToolOutcomeContext {
            ctx: &mut tp_ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };

        // 1. Display user message and push to history
        if is_bang_prefix {
            t_ctx.ctx.renderer.line(
                vtcode_tui_runtime::utils::ansi::MessageStyle::Info,
                "Shell mode (!): executing command directly.",
            )?;
        }
        if matches!(tool_name, tools::SPAWN_AGENT | tools::SPAWN_BACKGROUND_SUBPROCESS)
            && let Some(controller) = t_ctx.ctx.tool_registry.subagent_controller()
        {
            controller.set_turn_delegation_hints_from_input(input).await;
        }
        display_user_message(t_ctx.ctx.renderer, input)?;
        t_ctx.ctx.working_history.push(uni::Message::user(input.to_string()));

        // 2. Inject assistant message with tool call to keep history valid for LLM
        let mut pending_tool = Some((tool_name.to_string(), args));
        let mut consumed_fallback = false;

        while let Some((current_tool_name, current_args)) = pending_tool.take() {
            let tool_call_id = format!("direct_{}_{}", current_tool_name, t_ctx.ctx.working_history.len());
            let tool_call = uni::ToolCall::function(
                tool_call_id.clone(),
                current_tool_name.clone(),
                serde_json::to_string(&current_args).unwrap_or_default(),
            );
            t_ctx
                .ctx
                .working_history
                .push(uni::Message::assistant_with_tools(String::new(), vec![tool_call]));

            // Only this concrete user-input route can exempt a shell invocation.
            // Approval of a later model-authored command cannot create this grant.
            let _inspection_exemption = if !consumed_fallback
                && vtcode_core::tools::tool_intent::is_command_run_tool_call(&current_tool_name, &current_args)
            {
                Some(t_ctx.ctx.tool_registry.trusted_user_command_exemption(&current_args)?)
            } else {
                None
            };
            // 3. Execute through unified pipeline to ensure safety, metrics, and consistent output
            let outcome = handle_single_tool_call(&mut t_ctx, &tool_call_id, &current_tool_name, current_args).await?;

            if let Some(TurnHandlerOutcome::Break(TurnLoopResult::Exit)) = outcome {
                if let Some(emitter) = t_ctx.ctx.harness_emitter {
                    let _ = emitter.emit(turn_completed_event(vtcode_core::exec::events::Usage::default()));
                }
                return Ok(Some(InteractionOutcome::Exit { reason: vtcode_core::hooks::SessionEndReason::Exit }));
            }

            if !consumed_fallback
                && let Some((fallback_tool, fallback_args)) =
                    direct_tool_fallback(t_ctx.ctx.working_history, &tool_call_id, &current_tool_name)
            {
                consumed_fallback = true;
                t_ctx.ctx.renderer.line(
                    vtcode_tui_runtime::utils::ansi::MessageStyle::Info,
                    &format!("Retrying with tool-provided fallback `{fallback_tool}`."),
                )?;
                pending_tool = Some((fallback_tool, fallback_args));
            }
        }

        // 4. Cleanup UI and return outcome
        t_ctx.ctx.reset_input_to_default_placeholder();
        let restore_left = t_ctx.ctx.input_status_state.left.clone();
        let restore_right = t_ctx.ctx.input_status_state.right.clone();
        t_ctx.ctx.restore_input_status(restore_left, restore_right);

        if let Some(reply) = generate_completion_reply_with_suggestions(
            t_ctx.ctx.working_history,
            ReplyKind::Immediate,
            t_ctx.ctx.provider_client.as_ref(),
            &t_ctx.ctx.config.model,
        )
        .await
        {
            t_ctx
                .ctx
                .renderer
                .line(vtcode_tui_runtime::utils::ansi::MessageStyle::Response, &reply)?;
            t_ctx
                .ctx
                .working_history
                .push(uni::Message::assistant(reply).with_phase(Some(uni::AssistantPhase::FinalAnswer)));
        }

        t_ctx
            .ctx
            .session_stats
            .set_verification_snapshot(t_ctx.repeated_tool_attempts.verification_snapshot());
        turn_modified_files
    };

    ctx.interaction_ctx.agent_touched_paths.extend(
        turn_modified_files
            .iter()
            .map(|path| normalize_workspace_path(ctx.interaction_ctx.config.workspace.as_path(), path)),
    );

    // Close the direct turn's lifecycle now that its output is complete.
    if let Some(emitter) = ctx.interaction_ctx.harness_emitter {
        let _ = emitter.emit(turn_completed_event(vtcode_core::exec::events::Usage::default()));
    }

    // Direct tool paths already executed and rendered output; skip creating an
    // immediate LLM turn for this interaction loop iteration. If this call
    // launched background work, retain its stable completion identity so only
    // that user-directed completion stays non-autonomous.
    Ok(Some(
        latest_direct_background_completion_identity(ctx.interaction_ctx.conversation_history)
            .map_or(InteractionOutcome::DirectToolHandled, |completion_identity| {
                InteractionOutcome::DirectBackgroundToolHandled { completion_identity }
            }),
    ))
}

fn direct_tool_skips_confirmations(tool_name: &str) -> bool {
    matches!(tool_name, tools::EXEC_COMMAND | tools::UNIFIED_EXEC)
}

async fn detect_direct_subagent_spawn_input(
    input: &str,
    ctx: &DirectToolContext<'_, '_>,
) -> Result<Option<DirectSubagentInvocation>> {
    let Some(controller) = ctx.interaction_ctx.tool_registry.subagent_controller() else {
        return Ok(None);
    };
    let specs = controller.effective_specs().await;
    let Some(args) = direct_subagent_spawn_args(input, &specs) else {
        return Ok(None);
    };
    let tool_name = direct_subagent_tool_name(&args, &specs);
    Ok(Some(DirectSubagentInvocation { tool_name, args }))
}

fn parse_direct_tool_input(input: &str) -> Option<DirectToolInput> {
    if let Some(args) = detect_direct_file_operation_read(input) {
        return Some(DirectToolInput::Execute {
            tool_name: tools::UNIFIED_FILE.to_string(),
            args,
            is_bang_prefix: false,
        });
    }

    // Check for shell mode with '!' prefix or explicit 'run' command
    let trimmed = input.trim_start();
    if let Some(rest) = trimmed.strip_prefix('!') {
        let shell_command = rest.trim();
        if shell_command.is_empty() {
            return None;
        }
        return match validate_bang_shell_command(shell_command) {
            Ok(()) => Some(DirectToolInput::Execute {
                tool_name: tools::EXEC_COMMAND.to_string(),
                args: serde_json::json!({ "action": "run", "command": shell_command }),
                is_bang_prefix: true,
            }),
            Err(diagnosis) => Some(DirectToolInput::InvalidBang { command: shell_command.to_string(), diagnosis }),
        };
    }

    crate::agent::runloop::unified::shell::detect_explicit_run_command(input)
        .map(|(tool_name, args)| DirectToolInput::Execute { tool_name, args, is_bang_prefix: false })
}

fn detect_direct_file_operation_read(input: &str) -> Option<serde_json::Value> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_run = strip_prefix_case_insensitive(trimmed, "run ").unwrap_or(trimmed).trim_start();
    let tool_prefix = strip_prefix_case_insensitive(without_run, "file_operation")?;
    let after_tool = tool_prefix.trim_start();
    let after_read = strip_prefix_case_insensitive(after_tool, "read")?.trim_start();
    if after_read.is_empty() {
        return None;
    }

    let (path_part, _) = split_path_and_suffix(after_read);
    let path = normalize_file_operation_path(path_part);
    if path.is_empty() {
        return None;
    }

    Some(serde_json::json!({
        "action": "read",
        "path": path,
        "condense": false
    }))
}

fn direct_subagent_spawn_args(input: &str, specs: &[SubagentSpec]) -> Option<serde_json::Value> {
    let trimmed = input.trim();
    for spec in specs.iter().filter(|spec| spec.is_subagent()) {
        for candidate in std::iter::once(spec.name.as_str()).chain(spec.aliases.iter().map(String::as_str)) {
            let Some(task) = parse_direct_subagent_task(trimmed, candidate) else {
                continue;
            };
            let message = match task.message {
                Some(message) => {
                    if delegated_task_requires_clarification(message) {
                        spec.initial_prompt.clone()?
                    } else {
                        message.to_string()
                    }
                }
                None => spec.initial_prompt.clone()?,
            };
            return Some(serde_json::json!({
                "agent_type": spec.name.as_str(),
                "message": message,
                "background": spec.background || task.background
            }));
        }
    }
    None
}

fn direct_subagent_tool_name(args: &serde_json::Value, specs: &[SubagentSpec]) -> &'static str {
    let agent_type = args.get("agent_type").and_then(serde_json::Value::as_str);
    let background_requested = args.get("background").and_then(serde_json::Value::as_bool).unwrap_or(false);
    if specs
        .iter()
        .any(|spec| Some(spec.name.as_str()) == agent_type && spec.background)
        && background_requested
    {
        tools::SPAWN_BACKGROUND_SUBPROCESS
    } else {
        tools::SPAWN_AGENT
    }
}

fn direct_tool_fallback(
    history: &[uni::Message],
    tool_call_id: &str,
    current_tool_name: &str,
) -> Option<(String, serde_json::Value)> {
    let payload = history
        .iter()
        .rev()
        .find(|message| message.tool_call_id.as_deref() == Some(tool_call_id))
        .and_then(|message| serde_json::from_str::<serde_json::Value>(message.get_text_content().as_ref()).ok())?;
    let fallback_tool = payload
        .get("fallback_tool")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|tool| !tool.is_empty() && *tool != current_tool_name)?
        .to_string();
    let fallback_args = payload.get("fallback_tool_args")?.clone();
    Some((fallback_tool, fallback_args))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectSubagentTask<'a> {
    message: Option<&'a str>,
    background: bool,
}

fn parse_direct_subagent_task<'a>(input: &'a str, candidate: &str) -> Option<DirectSubagentTask<'a>> {
    for prefix in ["run", "use", "spawn"] {
        let Some(rest) = strip_bounded_prefix_case_insensitive(input.trim(), prefix) else {
            continue;
        };
        let rest = strip_optional_word_prefix_case_insensitive(rest, "the");
        let Some(rest) = strip_bounded_prefix_case_insensitive(rest, candidate) else {
            continue;
        };
        let rest = if let Some(value) = strip_bounded_prefix_case_insensitive(rest, "subagent") {
            value
        } else {
            let Some(value) = strip_bounded_prefix_case_insensitive(rest, "agent") else {
                continue;
            };
            value
        };
        return parse_direct_subagent_task_suffix(rest);
    }
    None
}

fn parse_direct_subagent_task_suffix(input: &str) -> Option<DirectSubagentTask<'_>> {
    let (trimmed, background) = strip_background_directive(input.trim());
    if trimmed.is_empty() {
        return Some(DirectSubagentTask { message: None, background });
    }

    for prefix in ["and", "to", "for"] {
        if let Some(rest) = strip_bounded_prefix_case_insensitive(trimmed, prefix) {
            let task = trim_wrapping_quotes_and_punctuation(rest).trim();
            return (!task.is_empty()).then_some(DirectSubagentTask { message: Some(task), background });
        }
    }

    if let Some(rest) = trimmed.strip_prefix(':').or_else(|| trimmed.strip_prefix('-')) {
        let task = trim_wrapping_quotes_and_punctuation(rest).trim();
        return (!task.is_empty()).then_some(DirectSubagentTask { message: Some(task), background });
    }

    None
}

fn strip_background_directive(input: &str) -> (&str, bool) {
    let trimmed = input.trim();
    if let Some(rest) = strip_bounded_prefix_case_insensitive(trimmed, "in") {
        let rest = strip_optional_word_prefix_case_insensitive(rest, "the");
        if let Some(rest) = strip_bounded_prefix_case_insensitive(rest, "background") {
            return (rest.trim(), true);
        }
    }
    if let Some(rest) = strip_bounded_prefix_case_insensitive(trimmed, "background") {
        return (rest.trim(), true);
    }
    (trimmed, false)
}

fn split_path_and_suffix(input: &str) -> (&str, &str) {
    let lower = input.to_ascii_lowercase();
    if let Some(idx) = lower.find(" with ") {
        return (&input[..idx], &input[idx + 1..]);
    }
    if let Some(idx) = lower.find(" mode omitted") {
        return (&input[..idx], &input[idx..]);
    }
    (input, "")
}

fn strip_prefix_case_insensitive<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let prefix_len = prefix.len();
    if input.len() < prefix_len || !input.is_char_boundary(prefix_len) {
        return None;
    }
    let head = &input[..prefix_len];
    if head.eq_ignore_ascii_case(prefix) {
        Some(input[prefix_len..].trim_start())
    } else {
        None
    }
}

fn strip_bounded_prefix_case_insensitive<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let prefix_len = prefix.len();
    if input.len() < prefix_len || !input.is_char_boundary(prefix_len) {
        return None;
    }
    let head = &input[..prefix_len];
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let remainder = &input[prefix_len..];
    if remainder
        .chars()
        .next()
        .is_some_and(|ch| !ch.is_whitespace() && ch != ':' && ch != '-')
    {
        return None;
    }
    Some(remainder.trim_start())
}

fn strip_optional_word_prefix_case_insensitive<'a>(input: &'a str, prefix: &str) -> &'a str {
    strip_bounded_prefix_case_insensitive(input, prefix).unwrap_or(input)
}

fn normalize_file_operation_path(input: &str) -> String {
    let mut normalized = input.trim();
    normalized = strip_optional_word_prefix(normalized, "on");
    normalized = strip_optional_word_prefix(normalized, "from");
    normalized = normalized.trim_prefix('@');
    trim_wrapping_quotes_and_punctuation(normalized).to_string()
}

fn normalize_direct_tool_mentions(input: &str, workspace_root: &Path) -> String {
    let matches = vtcode_commons::at_pattern::find_at_patterns(input);
    if matches.is_empty() {
        return input.to_string();
    }

    let mut normalized = String::with_capacity(input.len());
    let mut last_end = 0usize;
    let mut replaced_any = false;

    for at_match in matches {
        if at_match.start < last_end {
            continue;
        }

        let replacement = resolve_direct_tool_mention(at_match.path, input, at_match.start, workspace_root);

        normalized.push_str(&input[last_end..at_match.start]);
        if let Some(replacement) = replacement {
            normalized.push_str(&replacement);
            replaced_any = true;
        } else {
            normalized.push_str(at_match.full_match);
        }
        last_end = at_match.end;
    }

    if !replaced_any {
        return input.to_string();
    }

    normalized.push_str(&input[last_end..]);
    normalized
}

fn resolve_direct_tool_mention(alias: &str, input: &str, at_pos: usize, workspace_root: &Path) -> Option<String> {
    let trimmed = alias.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("data:")
        || !vtcode_commons::paths::is_safe_relative_path(trimmed)
    {
        return None;
    }

    if is_package_manager_command_context(input, at_pos) && !looks_like_explicit_path(trimmed) {
        return None;
    }

    let resolved = vtcode_commons::paths::resolve_workspace_path(workspace_root, Path::new(trimmed)).ok()?;
    if !resolved.exists() {
        return None;
    }

    Some(shell_quote_if_needed(trimmed))
}

fn is_package_manager_command_context(input: &str, at_pos: usize) -> bool {
    let before_at = &input[..at_pos];
    ["npm", "npx", "yarn", "pnpm", "bun", "bunx"]
        .iter()
        .any(|cmd| before_at.split_whitespace().any(|word| word == *cmd))
}

fn looks_like_explicit_path(value: &str) -> bool {
    value.starts_with("./")
        || value.starts_with("../")
        || value.starts_with('/')
        || value.starts_with("~/")
        || value.contains('.')
        || value.contains('\\')
}

fn validate_bang_shell_command(command: &str) -> std::result::Result<(), String> {
    match parse_shell_commands_tree_sitter(command) {
        Ok(commands) if !commands.is_empty() => Ok(()),
        Ok(_) => Err("No executable shell command found.".to_string()),
        Err(err) => Err(err),
    }
}
#[cfg(test)]
#[path = "tool_dispatch_tests.rs"]
mod tests;
