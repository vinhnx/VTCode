use super::AgentRunner;
use super::constants::{LOOP_THROTTLE_BASE_MS, LOOP_THROTTLE_MAX_MS};
use super::types::ToolFailureContext;
use crate::config::constants::tools;
use crate::core::agent::events::ExecEventRecorder;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::steering::SteeringMessage;
use crate::exec::events::CommandExecutionStatus;
use crate::llm::provider::ToolCall;
use crate::utils::colors::style;
use anyhow::{Result, anyhow};
use tokio::time::Duration;

impl AgentRunner {
    /// Execute multiple tool calls in parallel. Only safe for read-only operations.
    pub(super) async fn execute_parallel_tool_calls(
        &self,
        tool_calls: Vec<ToolCall>,
        session_state: &mut AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<()> {
        use futures::future::join_all;

        let mut prepared_calls = Vec::with_capacity(tool_calls.len());
        for call in tool_calls {
            let Some(func) = call.function.as_ref() else {
                continue;
            };
            let name = func.name.clone();
            let args = match call.parsed_arguments() {
                Ok(args) => args,
                Err(err) => {
                    let error_msg = format!("Invalid arguments for tool '{}': {}", name, err);
                    if !self.quiet {
                        println!("{} {} {}", agent_prefix, style("(ERR)").red(), error_msg);
                    }
                    session_state.push_tool_error(call.id.clone(), &name, error_msg, is_gemini);
                    continue;
                }
            };
            let args = self.normalize_tool_args(&name, &args, session_state);
            if self.check_for_loop(&name, &args, session_state) {
                return Ok(());
            }
            prepared_calls.push((call, name, args));
        }

        let total_calls = prepared_calls.len();
        if !self.quiet {
            println!(
                "{} [{}] Executing {} tools in parallel",
                style("[PARALLEL]").cyan().bold(),
                self.agent_type,
                total_calls
            );
        }

        let mut futures = Vec::with_capacity(prepared_calls.len());
        for (call, name, args) in prepared_calls {
            let call_id = call.id.clone();

            if !self.is_valid_tool(&name).await {
                self.record_tool_denied(
                    agent_prefix,
                    session_state,
                    event_recorder,
                    &call_id,
                    &name,
                    None,
                    is_gemini,
                );
                continue;
            }

            let tool_registry = self.tool_registry.clone();
            let args_clone = args.clone();
            futures.push(async move {
                let registry = tool_registry;
                let result = registry
                    .execute_tool_ref(&name, &args_clone)
                    .await
                    .map_err(|e| anyhow!("Tool '{}' failed: {}", name, e));
                (name, call_id, args_clone, result)
            });
        }

        let results = join_all(futures).await;
        let mut halt_turn = false;
        for (name, call_id, args, result) in results {
            let command_event = event_recorder.command_started(&name);
            match result {
                Ok(result) => {
                    if !self.quiet {
                        println!(
                            "{} {} {} tool executed successfully",
                            agent_prefix,
                            style("(OK)").green(),
                            name
                        );
                    }

                    let optimized_result = self.optimize_tool_result(&name, result).await;
                    let tool_result = serde_json::to_string(&optimized_result)?;

                    self.update_last_paths_from_args(&name, &args, session_state);

                    session_state.push_tool_result(call_id, &name, tool_result, is_gemini);
                    event_recorder.command_finished(
                        &command_event,
                        CommandExecutionStatus::Completed,
                        None,
                        "",
                    );
                }
                Err(e) => {
                    let error_msg = format!("Error executing {}: {}", name, e);
                    if !self.quiet {
                        println!("{} {} {}", agent_prefix, style("(ERR)").red(), error_msg);
                    }
                    let err_lower = error_msg.to_lowercase();
                    if err_lower.contains("rate limit") {
                        session_state.warnings.push(
                            "Tool was rate limited; halting further tool calls this turn.".into(),
                        );
                        session_state.mark_tool_loop_limit_hit();
                        halt_turn = true;
                    } else if err_lower.contains("denied by policy")
                        || err_lower.contains("not permitted while full-auto")
                    {
                        session_state.warnings.push(
                            "Tool denied by policy; halting further tool calls this turn.".into(),
                        );
                        halt_turn = true;
                    }
                    session_state.push_tool_error(call_id, &name, error_msg, is_gemini);
                    event_recorder.command_finished(
                        &command_event,
                        CommandExecutionStatus::Failed,
                        None,
                        &e.to_string(),
                    );
                    if halt_turn {
                        break;
                    }
                }
            }
        }
        if halt_turn {
            tokio::time::sleep(Duration::from_millis(250)).await;
            return Ok(());
        }
        Ok(())
    }

    /// Execute multiple tool calls sequentially.
    pub(super) async fn execute_sequential_tool_calls(
        &mut self,
        tool_calls: Vec<ToolCall>,
        session_state: &mut AgentSessionState,
        event_recorder: &mut ExecEventRecorder,
        agent_prefix: &str,
        is_gemini: bool,
    ) -> Result<()> {
        for call in tool_calls {
            // Check for steering messages before each tool call
            if let Some(msg) = self.check_steering() {
                match msg {
                    SteeringMessage::Stop => {
                        if !self.quiet {
                            println!(
                                "{} {}",
                                agent_prefix,
                                style("Stopped by steering signal.").red().bold()
                            );
                        }
                        return Ok(());
                    }
                    SteeringMessage::Pause => {
                        if !self.quiet {
                            println!(
                                "{} {}",
                                agent_prefix,
                                style("Paused by steering signal. Waiting for Resume...")
                                    .yellow()
                                    .bold()
                            );
                        }
                        // Wait for resume
                        loop {
                            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            if let Some(SteeringMessage::Resume) = self.check_steering() {
                                if !self.quiet {
                                    println!(
                                        "{} {}",
                                        agent_prefix,
                                        style("Resumed by steering signal.").green().bold()
                                    );
                                }
                                break;
                            } else if let Some(SteeringMessage::Stop) = self.check_steering() {
                                return Ok(());
                            }
                        }
                    }
                    SteeringMessage::Resume => {}
                    SteeringMessage::InjectInput(_) => {
                        // Input injection during tool calls is deferred until the next turn
                        // effectively, but we log it.
                        if !self.quiet {
                            println!(
                                "{} {}",
                                agent_prefix,
                                style("Input injection deferred until next turn").yellow()
                            );
                        }
                    }
                }
            }

            let name = match call.function.as_ref() {
                Some(func) => func.name.clone(),
                None => continue,
            };
            let args = match call.parsed_arguments() {
                Ok(args) => args,
                Err(err) => {
                    let error_msg = format!("Invalid arguments for tool '{}': {}", name, err);
                    if !self.quiet {
                        println!("{} {} {}", agent_prefix, style("(ERR)").red(), error_msg);
                    }
                    session_state.push_tool_error(call.id.clone(), &name, error_msg, is_gemini);
                    continue;
                }
            };
            let args = self.normalize_tool_args(&name, &args, session_state);

            if self.check_for_loop(&name, &args, session_state) {
                break;
            }

            if !self.quiet {
                println!(
                    "{} [{}] Calling tool: {}",
                    style("[TOOL_CALL]").cyan().bold(),
                    self.agent_type,
                    name
                );
            }

            let command_event = event_recorder.command_started(&name);
            if !self.is_valid_tool(&name).await {
                self.record_tool_denied(
                    agent_prefix,
                    session_state,
                    event_recorder,
                    &call.id,
                    &name,
                    Some(&command_event),
                    is_gemini,
                );
                continue;
            }

            let repeat_count = self.loop_detector.lock().get_call_count(&name);
            if repeat_count > 1 {
                let delay_ms =
                    (LOOP_THROTTLE_BASE_MS * repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }

            // Use internal execution since is_valid_tool was already called above
            match self.execute_tool_internal(&name, &args).await {
                Ok(result) => {
                    if !self.quiet {
                        println!(
                            "{} {} {} tool executed successfully",
                            agent_prefix,
                            style("(OK)").green(),
                            name
                        );
                    }

                    let optimized_result = self.optimize_tool_result(&name, result).await;
                    let tool_result = serde_json::to_string(&optimized_result)?;

                    self.update_last_paths_from_args(&name, &args, session_state);

                    session_state.push_tool_result(call.id.clone(), &name, tool_result, is_gemini);
                    event_recorder.command_finished(
                        &command_event,
                        CommandExecutionStatus::Completed,
                        None,
                        "",
                    );

                    if name == tools::WRITE_FILE
                        && let Some(filepath) = args.get("path").and_then(|p| p.as_str())
                    {
                        session_state.modified_files.push(filepath.to_owned());
                        event_recorder.file_change_completed(filepath);
                    }
                }
                Err(e) => {
                    let err_msg = e.to_string();
                    let err_lower = err_msg.to_lowercase();
                    if err_lower.contains("rate limit") {
                        session_state.warnings.push(
                            "Tool was rate limited; halting further tool calls this turn.".into(),
                        );
                        session_state.mark_tool_loop_limit_hit();
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            session_state,
                            event_recorder,
                            command_event: &command_event,
                            is_gemini,
                        };
                        self.record_tool_failure(
                            &mut failure_ctx,
                            &name,
                            &e,
                            Some(call.id.as_str()),
                        );
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        break;
                    } else if err_lower.contains("denied by policy")
                        || err_lower.contains("not permitted while full-auto")
                    {
                        session_state.warnings.push(
                            "Tool denied by policy; halting further tool calls this turn.".into(),
                        );
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            session_state,
                            event_recorder,
                            command_event: &command_event,
                            is_gemini,
                        };
                        self.record_tool_failure(
                            &mut failure_ctx,
                            &name,
                            &e,
                            Some(call.id.as_str()),
                        );
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        break;
                    } else {
                        let mut failure_ctx = ToolFailureContext {
                            agent_prefix,
                            session_state,
                            event_recorder,
                            command_event: &command_event,
                            is_gemini,
                        };
                        self.record_tool_failure(
                            &mut failure_ctx,
                            &name,
                            &e,
                            Some(call.id.as_str()),
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
