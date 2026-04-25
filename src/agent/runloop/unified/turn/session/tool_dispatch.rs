use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;

use crate::agent::runloop::git::normalize_workspace_path;
use crate::agent::runloop::unified::display::display_user_message;
use crate::agent::runloop::unified::run_loop_context::{HarnessTurnState, TurnId, TurnRunId};
use crate::agent::runloop::unified::shell::shell_quote_if_needed;
use crate::agent::runloop::unified::status_line::InputStatusState;
use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnLoopResult};
use crate::agent::runloop::unified::turn::session::direct_tool_completion::{
    ReplyKind, generate_completion_reply_with_suggestions,
};
use crate::agent::runloop::unified::turn::session::interaction_loop::{
    InteractionLoopContext, InteractionOutcome,
};
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::{
    ToolOutcomeContext, handle_single_tool_call,
};
use vtcode_config::SubagentSpec;
use vtcode_core::command_safety::shell_parser::parse_shell_commands_tree_sitter;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider as uni;
use vtcode_core::session::SessionId;
use vtcode_core::subagents::delegated_task_requires_clarification;

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

pub(crate) async fn handle_direct_tool_execution(
    input: &str,
    ctx: &mut DirectToolContext<'_, '_>,
) -> Result<Option<InteractionOutcome>> {
    let normalized_input =
        normalize_direct_tool_mentions(input, &ctx.interaction_ctx.config.workspace);
    if let Some(args) = detect_direct_subagent_spawn_input(&normalized_input, ctx).await? {
        return execute_direct_tool_call(input, tools::SPAWN_AGENT, args, false, ctx).await;
    }
    let Some(parsed) = parse_direct_tool_input(&normalized_input) else {
        return Ok(None);
    };

    let (tool_name_str, args, is_bang_prefix) = match parsed {
        DirectToolInput::Execute {
            tool_name,
            args,
            is_bang_prefix,
        } => (tool_name, args, is_bang_prefix),
        DirectToolInput::InvalidBang { command, diagnosis } => {
            ctx.interaction_ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Shell mode (!): command rejected (invalid shell syntax).",
            )?;
            ctx.interaction_ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                &format!("Diagnosis: {diagnosis}"),
            )?;
            ctx.interaction_ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                &format!("Recovery: fix syntax and retry as `!{command}`, or remove `!` to ask in natural language."),
            )?;
            return Ok(Some(InteractionOutcome::DirectToolHandled));
        }
    };

    execute_direct_tool_call(input, &tool_name_str, args, is_bang_prefix, ctx).await
}

pub(crate) async fn execute_direct_tool_call(
    input: &str,
    tool_name: &str,
    args: serde_json::Value,
    is_bang_prefix: bool,
    ctx: &mut DirectToolContext<'_, '_>,
) -> Result<Option<InteractionOutcome>> {
    // Construct HarnessTurnState (simplified for direct execution)
    let direct_turn_id = SessionId::new();
    let mut harness_state = HarnessTurnState::new(
        TurnRunId(direct_turn_id.0.clone()),
        TurnId(direct_turn_id.0),
        ctx.interaction_ctx.harness_config.max_tool_calls_per_turn,
        ctx.interaction_ctx.harness_config.max_tool_wall_clock_secs,
        ctx.interaction_ctx.harness_config.max_tool_retries,
    );

    let mut auto_exit_plan_mode_attempted = false;

    // Construct TurnProcessingContext to leverage unified execution handlers
    let mut tp_ctx = ctx.interaction_ctx.as_turn_processing_context(
        &mut harness_state,
        &mut auto_exit_plan_mode_attempted,
        ctx.input_status_state,
    );

    let turn_modified_files = {
        let mut repeated_tool_attempts =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();
        let mut t_ctx = ToolOutcomeContext {
            ctx: &mut tp_ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };

        // 1. Display user message and push to history
        if is_bang_prefix {
            t_ctx.ctx.renderer.line(
                vtcode_core::utils::ansi::MessageStyle::Info,
                "Shell mode (!): executing command directly.",
            )?;
        }
        if tool_name == tools::SPAWN_AGENT
            && let Some(controller) = t_ctx.ctx.tool_registry.subagent_controller()
        {
            controller.set_turn_delegation_hints_from_input(input).await;
        }
        display_user_message(t_ctx.ctx.renderer, input)?;
        t_ctx
            .ctx
            .working_history
            .push(uni::Message::user(input.to_string()));

        // 2. Inject assistant message with tool call to keep history valid for LLM
        let tool_call_id = format!("direct_{}_{}", tool_name, t_ctx.ctx.working_history.len());
        let tool_call = uni::ToolCall::function(
            tool_call_id.clone(),
            tool_name.to_string(),
            serde_json::to_string(&args).unwrap_or_default(),
        );
        t_ctx
            .ctx
            .working_history
            .push(uni::Message::assistant_with_tools(
                String::new(),
                vec![tool_call],
            ));

        // 3. Execute through unified pipeline to ensure safety, metrics, and consistent output
        let outcome = handle_single_tool_call(&mut t_ctx, &tool_call_id, tool_name, args).await?;

        // 4. Cleanup UI and return outcome
        t_ctx.ctx.reset_input_to_default_placeholder();
        let restore_left = t_ctx.ctx.input_status_state.left.clone();
        let restore_right = t_ctx.ctx.input_status_state.right.clone();
        t_ctx.ctx.restore_input_status(restore_left, restore_right);

        if let Some(TurnHandlerOutcome::Break(TurnLoopResult::Exit)) = outcome {
            return Ok(Some(InteractionOutcome::Exit {
                reason: vtcode_core::hooks::SessionEndReason::Exit,
            }));
        }

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
                .line(vtcode_core::utils::ansi::MessageStyle::Response, &reply)?;
            t_ctx.ctx.working_history.push(
                uni::Message::assistant(reply).with_phase(Some(uni::AssistantPhase::FinalAnswer)),
            );
        }

        turn_modified_files
    };

    ctx.interaction_ctx
        .agent_touched_paths
        .extend(turn_modified_files.iter().map(|path| {
            normalize_workspace_path(ctx.interaction_ctx.config.workspace.as_path(), path)
        }));

    // Direct tool paths already executed and rendered output; skip creating an
    // immediate LLM turn for this interaction loop iteration.
    Ok(Some(InteractionOutcome::DirectToolHandled))
}

async fn detect_direct_subagent_spawn_input(
    input: &str,
    ctx: &DirectToolContext<'_, '_>,
) -> Result<Option<serde_json::Value>> {
    let Some(controller) = ctx.interaction_ctx.tool_registry.subagent_controller() else {
        return Ok(None);
    };
    let specs = controller.effective_specs().await;
    Ok(direct_subagent_spawn_args(input, &specs))
}

fn parse_direct_tool_input(input: &str) -> Option<DirectToolInput> {
    if let Some(args) = detect_direct_unified_file_read(input) {
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
                tool_name: tools::UNIFIED_EXEC.to_string(),
                args: serde_json::json!({ "action": "run", "command": shell_command }),
                is_bang_prefix: true,
            }),
            Err(diagnosis) => Some(DirectToolInput::InvalidBang {
                command: shell_command.to_string(),
                diagnosis,
            }),
        };
    }

    crate::agent::runloop::unified::shell::detect_explicit_run_command(input).map(
        |(tool_name, args)| DirectToolInput::Execute {
            tool_name,
            args,
            is_bang_prefix: false,
        },
    )
}

fn detect_direct_unified_file_read(input: &str) -> Option<serde_json::Value> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_run = strip_prefix_case_insensitive(trimmed, "run ")
        .unwrap_or(trimmed)
        .trim_start();
    let tool_prefix = strip_prefix_case_insensitive(without_run, "unified_file")?;
    let after_tool = tool_prefix.trim_start();
    let after_read = strip_prefix_case_insensitive(after_tool, "read")?.trim_start();
    if after_read.is_empty() {
        return None;
    }

    let (path_part, _) = split_path_and_suffix(after_read);
    let path = normalize_unified_file_path(path_part);
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
    for spec in specs {
        for candidate in
            std::iter::once(spec.name.as_str()).chain(spec.aliases.iter().map(String::as_str))
        {
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
                "background": task.background
            }));
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DirectSubagentTask<'a> {
    message: Option<&'a str>,
    background: bool,
}

fn parse_direct_subagent_task<'a>(
    input: &'a str,
    candidate: &str,
) -> Option<DirectSubagentTask<'a>> {
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
        return Some(DirectSubagentTask {
            message: None,
            background,
        });
    }

    for prefix in ["and", "to", "for"] {
        if let Some(rest) = strip_bounded_prefix_case_insensitive(trimmed, prefix) {
            let task = trim_wrapping_quotes_and_punctuation(rest).trim();
            return (!task.is_empty()).then_some(DirectSubagentTask {
                message: Some(task),
                background,
            });
        }
    }

    if let Some(rest) = trimmed
        .strip_prefix(':')
        .or_else(|| trimmed.strip_prefix('-'))
    {
        let task = trim_wrapping_quotes_and_punctuation(rest).trim();
        return (!task.is_empty()).then_some(DirectSubagentTask {
            message: Some(task),
            background,
        });
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

fn normalize_unified_file_path(input: &str) -> String {
    let mut normalized = input.trim();
    normalized = strip_optional_word_prefix(normalized, "on");
    normalized = strip_optional_word_prefix(normalized, "from");
    normalized = normalized.strip_prefix('@').unwrap_or(normalized);
    trim_wrapping_quotes_and_punctuation(normalized).to_string()
}

fn strip_optional_word_prefix<'a>(input: &'a str, word: &str) -> &'a str {
    let Some(prefix) = input.get(..word.len()) else {
        return input;
    };
    if !prefix.eq_ignore_ascii_case(word) {
        return input;
    }
    let remainder = &input[word.len()..];
    if remainder.chars().next().is_some_and(char::is_whitespace) {
        remainder.trim_start()
    } else {
        input
    }
}

fn trim_wrapping_quotes_and_punctuation(target: &str) -> &str {
    let mut normalized = target.trim();
    loop {
        let previous = normalized;
        normalized = normalized.trim();
        normalized = normalized.strip_prefix('"').unwrap_or(normalized);
        normalized = normalized.strip_suffix('"').unwrap_or(normalized);
        normalized = normalized.strip_prefix('\'').unwrap_or(normalized);
        normalized = normalized.strip_suffix('\'').unwrap_or(normalized);
        normalized = normalized
            .trim_end_matches(['.', ',', ';', '!', '?'])
            .trim();
        if normalized == previous {
            return normalized;
        }
    }
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

        let replacement =
            resolve_direct_tool_mention(at_match.path, input, at_match.start, workspace_root);

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

fn resolve_direct_tool_mention(
    alias: &str,
    input: &str,
    at_pos: usize,
    workspace_root: &Path,
) -> Option<String> {
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

    let resolved =
        vtcode_commons::paths::resolve_workspace_path(workspace_root, Path::new(trimmed)).ok()?;
    if !resolved.exists() {
        return None;
    }

    Some(shell_quote_if_needed(trimmed))
}

fn is_package_manager_command_context(input: &str, at_pos: usize) -> bool {
    let before_at = &input[..at_pos];
    ["npm", "npx", "yarn", "pnpm", "bun"]
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
mod tests {
    use std::fs;

    use super::normalize_direct_tool_mentions;
    use super::{DirectToolInput, direct_subagent_spawn_args, parse_direct_tool_input};
    use tempfile::TempDir;
    use vtcode_config::SubagentSource;
    use vtcode_config::SubagentSpec;

    fn test_subagent_spec(name: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: "test".to_string(),
            prompt: String::new(),
            tools: Some(vec!["read_file".to_string()]),
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permission_mode: None,
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn parses_bang_prefix_with_leading_whitespace() {
        let parsed = parse_direct_tool_input("   !echo hello").expect("direct tool");
        match parsed {
            DirectToolInput::Execute {
                tool_name,
                args,
                is_bang_prefix,
            } => {
                assert_eq!(
                    tool_name,
                    vtcode_core::config::constants::tools::UNIFIED_EXEC
                );
                assert_eq!(args["action"], "run");
                assert_eq!(args["command"], "echo hello");
                assert!(is_bang_prefix);
            }
            DirectToolInput::InvalidBang { .. } => {
                panic!("expected valid !-command to parse");
            }
        }
    }

    #[test]
    fn rejects_invalid_bang_command_with_diagnosis() {
        let parsed = parse_direct_tool_input("! )(").expect("invalid command should be handled");
        match parsed {
            DirectToolInput::InvalidBang { command, diagnosis } => {
                assert_eq!(command, ")(");
                assert!(!diagnosis.trim().is_empty());
            }
            DirectToolInput::Execute { .. } => {
                panic!("expected invalid !-command to be rejected");
            }
        }
    }

    #[test]
    fn rejects_empty_bang_command() {
        assert!(parse_direct_tool_input("!   ").is_none());
    }

    #[test]
    fn parses_likely_run_typo_as_direct_command() {
        let parsed = parse_direct_tool_input("eun cargo check").expect("direct tool");
        match parsed {
            DirectToolInput::Execute {
                tool_name,
                args,
                is_bang_prefix,
            } => {
                assert_eq!(
                    tool_name,
                    vtcode_core::config::constants::tools::UNIFIED_EXEC
                );
                assert_eq!(args["command"], "cargo check");
                assert!(!is_bang_prefix);
            }
            DirectToolInput::InvalidBang { .. } => {
                panic!("expected typoed run prefix to parse as direct command");
            }
        }
    }

    #[test]
    fn normalizes_direct_run_file_mentions_before_parsing() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("src").join("main.rs");
        fs::create_dir_all(file_path.parent().expect("parent")).expect("mkdir");
        fs::write(&file_path, "fn main() {}\n").expect("write file");

        let normalized = normalize_direct_tool_mentions("run cat @src/main.rs", temp_dir.path());
        assert_eq!(normalized, "run cat src/main.rs");

        let parsed = parse_direct_tool_input(&normalized).expect("direct tool");
        match parsed {
            DirectToolInput::Execute { args, .. } => {
                assert_eq!(args["command"], "cat src/main.rs");
            }
            DirectToolInput::InvalidBang { .. } => panic!("expected valid direct command"),
        }
    }

    #[test]
    fn normalizes_bang_file_mentions_with_spaces() {
        let temp_dir = TempDir::new().expect("temp dir");
        let file_path = temp_dir.path().join("docs").join("file with spaces.md");
        fs::create_dir_all(file_path.parent().expect("parent")).expect("mkdir");
        fs::write(&file_path, "# doc\n").expect("write file");

        let normalized =
            normalize_direct_tool_mentions("!cat @\"docs/file with spaces.md\"", temp_dir.path());
        assert_eq!(normalized, "!cat 'docs/file with spaces.md'");

        let parsed = parse_direct_tool_input(&normalized).expect("direct tool");
        match parsed {
            DirectToolInput::Execute {
                args,
                is_bang_prefix,
                ..
            } => {
                assert_eq!(args["command"], "cat 'docs/file with spaces.md'");
                assert!(is_bang_prefix);
            }
            DirectToolInput::InvalidBang { .. } => panic!("expected valid bang command"),
        }
    }

    #[test]
    fn leaves_npm_scoped_packages_unchanged_when_not_a_path() {
        let temp_dir = TempDir::new().expect("temp dir");
        let normalized = normalize_direct_tool_mentions("run npm i @types/node", temp_dir.path());
        assert_eq!(normalized, "run npm i @types/node");
    }

    #[test]
    fn parses_direct_unified_file_read_with_mode_omitted() {
        let parsed =
            parse_direct_tool_input("unified_file read on /tmp/example.md with mode omitted")
                .expect("direct unified_file");
        match parsed {
            DirectToolInput::Execute {
                tool_name, args, ..
            } => {
                assert_eq!(
                    tool_name,
                    vtcode_core::config::constants::tools::UNIFIED_FILE
                );
                assert_eq!(args["action"], "read");
                assert_eq!(args["path"], "/tmp/example.md");
                assert_eq!(args["condense"], false);
            }
            DirectToolInput::InvalidBang { .. } => {
                panic!("expected unified_file read to parse");
            }
        }
    }

    #[test]
    fn parses_run_prefixed_unified_file_read() {
        let parsed = parse_direct_tool_input("run unified_file read /tmp/example.md")
            .expect("direct unified_file");
        match parsed {
            DirectToolInput::Execute {
                tool_name, args, ..
            } => {
                assert_eq!(
                    tool_name,
                    vtcode_core::config::constants::tools::UNIFIED_FILE
                );
                assert_eq!(args["action"], "read");
                assert_eq!(args["path"], "/tmp/example.md");
                assert_eq!(args["condense"], false);
            }
            DirectToolInput::InvalidBang { .. } => {
                panic!("expected unified_file read to parse");
            }
        }
    }

    #[test]
    fn direct_subagent_spawn_args_defers_vague_shortcut_to_main_agent() {
        assert!(
            direct_subagent_spawn_args(
                "run rust-engineer subagent",
                &[test_subagent_spec("rust-engineer")],
            )
            .is_none()
        );
    }

    #[test]
    fn direct_subagent_spawn_args_defers_single_word_follow_up_task() {
        assert!(
            direct_subagent_spawn_args(
                "run rust-engineer subagent and report",
                &[test_subagent_spec("rust-engineer")],
            )
            .is_none()
        );
    }

    #[test]
    fn direct_subagent_spawn_args_extracts_follow_up_task() {
        let args = direct_subagent_spawn_args(
            "use rust-engineer subagent and review code",
            &[test_subagent_spec("rust-engineer")],
        )
        .expect("direct subagent spawn");
        assert_eq!(args["agent_type"], "rust-engineer");
        assert_eq!(args["background"], false);
        assert_eq!(args["message"], "review code");
    }

    #[test]
    fn direct_subagent_spawn_args_requires_explicit_background_request() {
        let args = direct_subagent_spawn_args(
            "spawn rust-engineer subagent in the background and review code",
            &[test_subagent_spec("rust-engineer")],
        )
        .expect("direct subagent spawn");
        assert_eq!(args["agent_type"], "rust-engineer");
        assert_eq!(args["background"], true);
        assert_eq!(args["message"], "review code");
    }

    #[test]
    fn direct_subagent_spawn_args_uses_agent_initial_prompt_when_available() {
        let mut spec = test_subagent_spec("background-demo");
        spec.initial_prompt = Some("Run the demo subprocess and report readiness.".to_string());
        let args = direct_subagent_spawn_args("run background-demo subagent", &[spec])
            .expect("direct subagent spawn");
        assert_eq!(args["agent_type"], "background-demo");
        assert_eq!(
            args["message"],
            "Run the demo subprocess and report readiness."
        );
    }

    #[test]
    fn direct_subagent_spawn_args_falls_back_to_initial_prompt_for_vague_follow_up() {
        let mut spec = test_subagent_spec("background-demo");
        spec.initial_prompt = Some("Run the demo subprocess and report readiness.".to_string());
        let args = direct_subagent_spawn_args("run background-demo subagent and demo", &[spec])
            .expect("direct subagent spawn");
        assert_eq!(args["agent_type"], "background-demo");
        assert_eq!(
            args["message"],
            "Run the demo subprocess and report readiness."
        );
    }
}
