use anyhow::{Result, anyhow};
use std::io;
use std::path::Path;

use vtcode_core::config::constants::defaults;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::core::decision_tracker::{Action as DTAction, DecisionOutcome, DecisionTracker};
use vtcode_core::core::router::{Router, TaskClass};
use vtcode_core::gemini::function_calling::{FunctionCall, FunctionResponse};
use vtcode_core::gemini::models::{SystemInstruction, ToolConfig};
use vtcode_core::gemini::{Client as GeminiClient, Content, GenerateContentRequest, Part, Tool};
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::tools::{ToolRegistry, build_function_declarations};
use vtcode_core::ui::{Spinner, theme};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::update_theme_preference;

use super::context::{
    apply_aggressive_trim_gemini, enforce_gemini_context_window, load_context_trim_config,
    prune_gemini_tool_responses,
};
use super::git::confirm_changes_with_git_diff;
use super::is_context_overflow_error;
use super::slash_commands::{SlashCommandOutcome, handle_slash_command};
use super::telemetry::build_trajectory_logger;
use super::text_tools::detect_textual_tool_call;
use super::tool_output::render_tool_output;
use super::ui::render_session_banner;
use super::welcome::prepare_session_bootstrap;

fn persist_theme_preference(renderer: &mut AnsiRenderer, theme_id: &str) -> Result<()> {
    if let Err(err) = update_theme_preference(theme_id) {
        renderer.line(
            MessageStyle::Error,
            &format!("Failed to persist theme preference: {}", err),
        )?;
    }
    Ok(())
}

fn ensure_turn_bottom_gap(renderer: &mut AnsiRenderer, applied: &mut bool) -> Result<()> {
    if !*applied {
        renderer.line(MessageStyle::Output, "")?;
        *applied = true;
    }
    Ok(())
}

pub(crate) async fn run_single_agent_loop_gemini(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
) -> Result<()> {
    let trim_config = load_context_trim_config(vt_cfg);
    let session_bootstrap = prepare_session_bootstrap(config, vt_cfg);
    let mut renderer = AnsiRenderer::stdout();
    render_session_banner(&mut renderer, config, &session_bootstrap)?;

    if let Some(text) = session_bootstrap.welcome_text.as_ref() {
        renderer.line(MessageStyle::Response, text)?;
        renderer.line(MessageStyle::Output, "")?;
    }

    let placeholder_hint = session_bootstrap.placeholder.clone();
    let mut placeholder_shown = false;

    let mut client = GeminiClient::new(config.api_key.clone(), config.model.clone());
    let traj = build_trajectory_logger(&config.workspace, vt_cfg);
    let mut ledger = DecisionTracker::new();

    let mut tool_registry = ToolRegistry::new(config.workspace.clone());
    tool_registry.initialize_async().await?;
    if let Some(cfg) = vt_cfg {
        if let Err(err) = tool_registry.apply_config_policies(&cfg.tools) {
            eprintln!(
                "Warning: Failed to apply tool policies from config: {}",
                err
            );
        }
    }

    if full_auto {
        let automation_cfg = vt_cfg
            .map(|cfg| cfg.automation.full_auto.clone())
            .ok_or_else(|| anyhow!("Full-auto configuration unavailable"))?;

        tool_registry.enable_full_auto_mode(&automation_cfg.allowed_tools);
        let allowlist = tool_registry
            .current_full_auto_allowlist()
            .unwrap_or_default();
        if allowlist.is_empty() {
            renderer.line(
                MessageStyle::Info,
                "Full-auto mode enabled with no tool permissions; tool calls will be skipped.",
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Full-auto mode enabled. Permitted tools: {}",
                    allowlist.join(", ")
                ),
            )?;
        }
    }
    let function_declarations = build_function_declarations();
    let tools = vec![Tool {
        function_declarations,
    }];
    let available_tool_names: Vec<String> = tools
        .iter()
        .flat_map(|tool| {
            tool.function_declarations
                .iter()
                .map(|declaration| declaration.name.clone())
        })
        .collect();

    let mut conversation_history: Vec<Content> = vec![];
    let base_system_prompt = read_system_prompt(
        &config.workspace,
        session_bootstrap.prompt_addendum.as_deref(),
    );

    renderer.line(
        MessageStyle::Info,
        "Type 'exit' to quit, 'help' for commands",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Slash commands: /help, /list-themes, /theme <id>, /command <program>",
    )?;
    loop {
        if !placeholder_shown {
            if let Some(ref hint) = placeholder_hint {
                renderer.line(MessageStyle::Info, &format!("Suggested input: {}", hint))?;
            }
            placeholder_shown = true;
        }
        let styles = theme::active_styles();
        renderer.inline_with_style(styles.primary, "❯ ")?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        match input {
            "" => {
                continue;
            }
            "exit" | "quit" => {
                renderer.line(MessageStyle::Info, "Goodbye!")?;
                break;
            }
            "help" => {
                renderer.line(MessageStyle::Info, "Commands: exit, help")?;
                continue;
            }
            _ => {}
        }

        if let Some(command_input) = input.strip_prefix('/') {
            match handle_slash_command(command_input, &mut renderer)? {
                SlashCommandOutcome::Handled => {
                    continue;
                }
                SlashCommandOutcome::ThemeChanged(theme_id) => {
                    persist_theme_preference(&mut renderer, &theme_id)?;
                    continue;
                }
                SlashCommandOutcome::ExecuteTool { name, args } => {
                    match tool_registry.preflight_tool_permission(&name) {
                        Ok(true) => {
                            let tool_spinner = Spinner::new(&format!("Running tool: {}", name));
                            match tool_registry.execute_tool(&name, args.clone()).await {
                                Ok(tool_output) => {
                                    tool_spinner.finish_and_clear();
                                    traj.log_tool_call(
                                        conversation_history.len(),
                                        &name,
                                        &args,
                                        true,
                                    );
                                    render_tool_output(Some(name.as_str()), &tool_output);
                                }
                                Err(err) => {
                                    tool_spinner.finish_and_clear();
                                    traj.log_tool_call(
                                        conversation_history.len(),
                                        &name,
                                        &args,
                                        false,
                                    );
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Tool '{}' failed: {}", name, err),
                                    )?;
                                }
                            }
                        }
                        Ok(false) => {
                            let denial = ToolExecutionError::new(
                                name.clone(),
                                ToolErrorType::PolicyViolation,
                                format!("Tool '{}' execution denied by policy", name),
                            )
                            .to_json_value();
                            traj.log_tool_call(conversation_history.len(), &name, &args, false);
                            render_tool_output(Some(name.as_str()), &denial);
                        }
                        Err(err) => {
                            traj.log_tool_call(conversation_history.len(), &name, &args, false);
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to evaluate policy for tool '{}': {}", name, err),
                            )?;
                        }
                    }
                    continue;
                }
                SlashCommandOutcome::Exit => {
                    renderer.line(MessageStyle::Info, "Goodbye!")?;
                    break;
                }
            }
        }

        conversation_history.push(Content::user_text(input));
        let _pruned_tools = prune_gemini_tool_responses(
            &mut conversation_history,
            trim_config.preserve_recent_turns,
        );
        // Removed: Tool response pruning message
        let trim_result = enforce_gemini_context_window(&mut conversation_history, trim_config);
        if trim_result.is_trimmed() {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                    trim_result.removed_messages, trim_config.max_tokens,
                ),
            )?;
        }

        let decision = if let Some(vt) = vt_cfg {
            Router::route_async(vt, config, &config.api_key, input).await
        } else {
            Router::route(&VTCodeConfig::default(), config, input)
        };
        traj.log_route(
            conversation_history.len(),
            &decision.selected_model,
            match decision.class {
                TaskClass::Simple => "simple",
                TaskClass::Standard => "standard",
                TaskClass::Complex => "complex",
                TaskClass::CodegenHeavy => "codegen_heavy",
                TaskClass::RetrievalHeavy => "retrieval_heavy",
            },
            &input.chars().take(120).collect::<String>(),
        );

        if decision.selected_model != config.model {
            client = GeminiClient::new(config.api_key.clone(), decision.selected_model.clone());
        }

        let mut working_history = conversation_history.clone();
        let max_tool_loops = vt_cfg
            .map(|cfg| cfg.tools.max_tool_loops)
            .filter(|&value| value > 0)
            .unwrap_or(defaults::DEFAULT_MAX_TOOL_LOOPS);
        let mut loop_guard = 0usize;
        let mut any_write_effect = false;
        let mut bottom_gap_applied = false;

        'outer: loop {
            if loop_guard == 0 {
                renderer.line(MessageStyle::Output, "")?;
            }
            loop_guard += 1;
            if loop_guard >= max_tool_loops {
                if !bottom_gap_applied {
                    renderer.line(MessageStyle::Output, "")?;
                }
                let notice = format!(
                    "I reached the configured tool-call limit of {} for this turn and paused further tool execution. Increase `tools.max_tool_loops` in vtcode.toml if you need more, then ask me to continue.",
                    max_tool_loops
                );
                renderer.line(MessageStyle::Response, &notice)?;
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                let notice_content = Content {
                    role: "model".to_string(),
                    parts: vec![Part::Text { text: notice }],
                };
                working_history.push(notice_content.clone());
                conversation_history.push(notice_content);
                break 'outer;
            }

            let mut final_text: Option<String> = None;

            let _ = enforce_gemini_context_window(&mut working_history, trim_config);

            let (gen_cfg, _parallel_any) = if let Some(vt) = vt_cfg {
                let decision = Router::route_async(vt, config, &config.api_key, input).await;
                let key = match decision.class {
                    TaskClass::Simple => "simple",
                    TaskClass::Standard => "standard",
                    TaskClass::Complex => "complex",
                    TaskClass::CodegenHeavy => "codegen_heavy",
                    TaskClass::RetrievalHeavy => "retrieval_heavy",
                };
                if let Some(budget) = vt.router.budgets.get(key) {
                    let mut cfg = serde_json::json!({});
                    if let Some(max_tokens) = budget.max_tokens {
                        cfg["maxOutputTokens"] = serde_json::json!(max_tokens as u32);
                    }
                    (Some(cfg), budget.max_parallel_tools.unwrap_or(0) > 1)
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            };

            ledger.start_turn(working_history.len(), Some(input.to_string()));
            ledger.update_available_tools(available_tool_names.clone());

            let (lg_enabled, lg_max, lg_include) = vt_cfg
                .map(|cfg| {
                    (
                        cfg.context.ledger.enabled,
                        cfg.context.ledger.max_entries,
                        cfg.context.ledger.include_in_prompt,
                    )
                })
                .unwrap_or((true, 12, true));

            ledger.start_turn(
                working_history.len(),
                working_history.last().and_then(|message| {
                    message.parts.first().and_then(|part| match part {
                        Part::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                }),
            );
            let tool_names: Vec<String> = tools
                .iter()
                .flat_map(|tool| {
                    tool.function_declarations
                        .iter()
                        .map(|declaration| declaration.name.clone())
                })
                .collect();
            ledger.update_available_tools(tool_names);

            let system_prompt = if lg_enabled && lg_include {
                format!(
                    "{}\n\n[Decision Ledger]\n{}",
                    base_system_prompt,
                    ledger.render_ledger_brief(lg_max)
                )
            } else {
                base_system_prompt.clone()
            };
            let sys_inst = SystemInstruction::new(&system_prompt);

            let mut attempt_history = working_history.clone();
            let mut retry_attempts = 0usize;
            let response = loop {
                retry_attempts += 1;
                let _ = enforce_gemini_context_window(&mut attempt_history, trim_config);

                let req = GenerateContentRequest {
                    contents: attempt_history.clone(),
                    tools: Some(tools.clone()),
                    tool_config: Some(ToolConfig::auto()),
                    system_instruction: Some(sys_inst.clone()),
                    generation_config: gen_cfg.clone(),
                };

                // Use the existing thinking spinner instead of creating a new one
                match client.generate(&req).await {
                    Ok(result) => {
                        working_history = attempt_history.clone();
                        break result;
                    }
                    Err(error) => {
                        if is_context_overflow_error(&error.to_string())
                            && retry_attempts <= vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT
                        {
                            let removed_tool_messages = prune_gemini_tool_responses(
                                &mut attempt_history,
                                trim_config.preserve_recent_turns,
                            );
                            let removed_turns =
                                apply_aggressive_trim_gemini(&mut attempt_history, trim_config);
                            let total_removed = removed_tool_messages + removed_turns;
                            if total_removed > 0 {
                                renderer.line(MessageStyle::Info, "↻ Adjusting context")?;
                                renderer.line(
                                    MessageStyle::Info,
                                    &format!(
                                        "Context overflow detected; removed {} older messages (retry {}/{}).",
                                        total_removed,
                                        retry_attempts,
                                        vtcode_core::config::constants::context::CONTEXT_ERROR_RETRY_LIMIT,
                                    ),
                                )?;
                                conversation_history.clone_from(&attempt_history);
                                continue;
                            }
                        }
                        let has_tool = working_history
                            .iter()
                            .any(|content| matches!(content.role.as_str(), "tool"));
                        if has_tool {
                            eprintln!("Provider error (suppressed): {error}");
                            let reply = working_history
                                .iter()
                                .rev()
                                .find(|content| content.role == "tool")
                                .and_then(|tool_content| {
                                    tool_content.parts.iter().find_map(|part| match part {
                                        Part::Text { text } if !text.trim().is_empty() => {
                                            Some(text.clone())
                                        }
                                        _ => None,
                                    })
                                })
                                .unwrap_or_else(|| "Command completed successfully.".to_string());
                            renderer.line(MessageStyle::Response, &reply)?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            conversation_history.push(Content {
                                role: "model".to_string(),
                                parts: vec![Part::Text { text: reply }],
                            });
                            break 'outer;
                        } else {
                            renderer
                                .line(MessageStyle::Error, &format!("Provider error: {error}"))?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            break 'outer;
                        }
                    }
                }
            };

            let mut aggregated_text: Vec<String> = Vec::new();
            let mut function_calls: Vec<FunctionCall> = Vec::new();
            let mut response_content: Option<Content> = None;
            if let Some(candidate) = response.candidates.first() {
                response_content = Some(candidate.content.clone());
                for part in &candidate.content.parts {
                    match part {
                        Part::Text { text } => aggregated_text.push(text.clone()),
                        Part::FunctionCall { function_call } => {
                            function_calls.push(function_call.clone())
                        }
                        _ => {}
                    }
                }
            }

            if !aggregated_text.is_empty() {
                final_text = Some(aggregated_text.join("\n"));
            }

            if function_calls.is_empty()
                && let Some(text) = final_text.clone()
                && let Some((name, args)) = detect_textual_tool_call(&text)
            {
                let args_display =
                    serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Interpreting textual tool request as {} {}",
                        &name, &args_display
                    ),
                )?;
                let fc = FunctionCall {
                    name,
                    args,
                    id: None,
                };
                if let Some(ref mut content) = response_content {
                    content.parts = vec![Part::FunctionCall {
                        function_call: fc.clone(),
                    }];
                } else {
                    response_content = Some(Content {
                        role: "model".to_string(),
                        parts: vec![Part::FunctionCall {
                            function_call: fc.clone(),
                        }],
                    });
                }
                function_calls.push(fc);
                final_text = None;
            }

            if let Some(content) = response_content {
                working_history.push(content);
            }

            if function_calls.is_empty() {
                if let Some(text) = final_text.clone()
                    && !text.trim().is_empty()
                {
                    renderer.line(MessageStyle::Response, &text)?;
                }
                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                break 'outer;
            }

            for call in function_calls {
                let name = call.name.as_str();
                let args = call.args.clone();
                renderer.line(MessageStyle::Tool, &format!("[TOOL] {} {}", name, args))?;
                let decision_id = ledger.record_decision(
                    format!("Execute tool '{}' to progress task", name),
                    DTAction::ToolCall {
                        name: name.to_string(),
                        args: args.clone(),
                        expected_outcome: "Use tool output to decide next step".to_string(),
                    },
                    None,
                );
                match tool_registry.preflight_tool_permission(name) {
                    Ok(true) => {
                        let tool_spinner = Spinner::new(&format!("Running tool: {}", name));
                        match tool_registry.execute_tool(name, args.clone()).await {
                            Ok(tool_output) => {
                                tool_spinner.finish_and_clear();
                                render_tool_output(Some(name), &tool_output);
                                let modified_files: Vec<String> = if let Some(files) = tool_output
                                    .get("modified_files")
                                    .and_then(|value| value.as_array())
                                {
                                    files
                                        .iter()
                                        .filter_map(|file| {
                                            file.as_str().map(|value| value.to_string())
                                        })
                                        .collect()
                                } else {
                                    vec![]
                                };

                                if matches!(
                                    name,
                                    "write_file"
                                        | "edit_file"
                                        | "create_file"
                                        | "delete_file"
                                        | "srgn"
                                ) {
                                    any_write_effect = true;
                                }

                                if !modified_files.is_empty()
                                    && confirm_changes_with_git_diff(
                                        &modified_files,
                                        skip_confirmations,
                                    )
                                    .await?
                                {
                                    renderer.line(
                                        MessageStyle::Info,
                                        "Changes applied successfully.",
                                    )?;
                                } else if !modified_files.is_empty() {
                                    renderer.line(MessageStyle::Info, "Changes discarded.")?;
                                }

                                let fr = FunctionResponse {
                                    name: call.name.clone(),
                                    response: tool_output,
                                };
                                working_history.push(Content::user_parts(vec![
                                    Part::FunctionResponse {
                                        function_response: fr,
                                    },
                                ]));
                                ledger.record_outcome(
                                    &decision_id,
                                    DecisionOutcome::Success {
                                        result: "tool_ok".to_string(),
                                        metrics: Default::default(),
                                    },
                                );
                            }
                            Err(error) => {
                                tool_spinner.finish_and_clear();
                                renderer
                                    .line(MessageStyle::Tool, &format!("Tool {} failed.", name))?;
                                renderer
                                    .line(MessageStyle::Error, &format!("Tool error: {error}"))?;
                                let err = serde_json::json!({ "error": error.to_string() });
                                let fr = FunctionResponse {
                                    name: call.name.clone(),
                                    response: err,
                                };
                                working_history.push(Content::user_parts(vec![
                                    Part::FunctionResponse {
                                        function_response: fr,
                                    },
                                ]));
                                ledger.record_outcome(
                                    &decision_id,
                                    DecisionOutcome::Failure {
                                        error: error.to_string(),
                                        recovery_attempts: 0,
                                        context_preserved: true,
                                    },
                                );
                            }
                        }
                    }
                    Ok(false) => {
                        let denial = ToolExecutionError::new(
                            name.to_string(),
                            ToolErrorType::PolicyViolation,
                            format!("Tool '{}' execution denied by policy", name),
                        )
                        .to_json_value();
                        traj.log_tool_call(working_history.len(), name, &args, false);
                        render_tool_output(Some(name), &denial);
                        let fr = FunctionResponse {
                            name: call.name.clone(),
                            response: denial,
                        };
                        working_history.push(Content::user_parts(vec![Part::FunctionResponse {
                            function_response: fr,
                        }]));
                        ledger.record_outcome(
                            &decision_id,
                            DecisionOutcome::Failure {
                                error: format!("Tool '{}' execution denied by policy", name),
                                recovery_attempts: 0,
                                context_preserved: true,
                            },
                        );
                        continue;
                    }
                    Err(err) => {
                        traj.log_tool_call(working_history.len(), name, &args, false);
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to evaluate policy for tool '{}': {}", name, err),
                        )?;
                        let err_json = serde_json::json!({
                            "error": format!(
                                "Policy evaluation error for '{}' : {}",
                                name, err
                            )
                        });
                        let fr = FunctionResponse {
                            name: call.name.clone(),
                            response: err_json,
                        };
                        working_history.push(Content::user_parts(vec![Part::FunctionResponse {
                            function_response: fr,
                        }]));
                        ledger.record_outcome(
                            &decision_id,
                            DecisionOutcome::Failure {
                                error: format!(
                                    "Failed to evaluate policy for tool '{}': {}",
                                    name, err
                                ),
                                recovery_attempts: 0,
                                context_preserved: true,
                            },
                        );
                        continue;
                    }
                }
            }
        }

        conversation_history = working_history;

        let _pruned_after_turn = prune_gemini_tool_responses(
            &mut conversation_history,
            trim_config.preserve_recent_turns,
        );
        // Removed: Tool response pruning message after completion
        let post_trim = enforce_gemini_context_window(&mut conversation_history, trim_config);
        if post_trim.is_trimmed() {
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Trimmed {} earlier messages to respect the context window (~{} tokens).",
                    post_trim.removed_messages, trim_config.max_tokens,
                ),
            )?;
        }

        if let Some(last) = conversation_history.last()
            && let Some(text) = last.parts.first().and_then(|part| part.as_text())
        {
            let claims_write = text.contains("I've updated")
                || text.contains("I have updated")
                || text.contains("updated the `");
            if claims_write && !any_write_effect {
                renderer.line(MessageStyle::Output, "")?;
                renderer.line(
                    MessageStyle::Info,
                    "Note: The assistant mentioned edits but no write tool ran.",
                )?;
            }
        }
    }

    Ok(())
}

fn read_system_prompt(workspace: &Path, session_addendum: Option<&str>) -> String {
    let mut prompt = vtcode_core::prompts::read_system_prompt_from_md()
        .unwrap_or_else(|_| "You are a helpful coding assistant for a Rust workspace.".to_string());

    if let Some(overview) = vtcode_core::utils::utils::build_project_overview(workspace) {
        prompt.push_str("\n\n## PROJECT OVERVIEW\n");
        prompt.push_str(&overview.as_prompt_block());
    }

    if let Some(guidelines) = vtcode_core::prompts::system::read_agent_guidelines(workspace) {
        prompt.push_str("\n\n## AGENTS.MD GUIDELINES\n");
        prompt.push_str(&guidelines);
    }

    if let Some(addendum) = session_addendum {
        let trimmed = addendum.trim();
        if !trimmed.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(trimmed);
        }
    }

    prompt
}
