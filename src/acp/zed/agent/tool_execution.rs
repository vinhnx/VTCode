use super::ZedAgent;
use crate::acp::reports::{
    TOOL_ERROR_LABEL, TOOL_RESPONSE_KEY_MESSAGE, TOOL_RESPONSE_KEY_STATUS, TOOL_RESPONSE_KEY_TOOL,
    TOOL_SUCCESS_LABEL, ToolExecutionReport,
};
use crate::acp::tooling::{SupportedTool, ToolDescriptor};
use agent_client_protocol::{self as acp, AgentSideConnection, Client};
use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider::ToolCall as ProviderToolCall;

use super::super::types::{RunTerminalMode, SessionHandle, ToolCallResult};

impl ZedAgent {
    pub(super) async fn execute_tool_calls(
        &self,
        session: &SessionHandle,
        session_id: &acp::SessionId,
        calls: &[ProviderToolCall],
    ) -> Result<Vec<ToolCallResult>, acp::Error> {
        if calls.is_empty() {
            return Ok(Vec::with_capacity(0)); // Use with_capacity(0) instead of Vec::new()
        }

        let Some(client) = self.client() else {
            return Ok(calls
                .iter()
                .map(|call| ToolCallResult {
                    tool_call_id: call.id.clone(),
                    llm_response: json!({
                        TOOL_RESPONSE_KEY_STATUS: TOOL_ERROR_LABEL,
                        TOOL_RESPONSE_KEY_TOOL: call.function.as_ref().expect("Tool call must have function").name,
                        TOOL_RESPONSE_KEY_MESSAGE: "Client connection unavailable",
                    })
                    .to_string(),
                })
                .collect());
        };

        let mut results = Vec::with_capacity(calls.len()); // Pre-allocate for all tool call results

        for call in calls {
            let func_ref = call
                .function
                .as_ref()
                .expect("Tool call must have function");
            let tool_descriptor = self.acp_tool_registry.lookup(&func_ref.name);
            let args_value_result: Result<Value, _> = serde_json::from_str(&func_ref.arguments);
            let args_value_for_input = args_value_result.as_ref().ok().cloned();
            let title = match (tool_descriptor, args_value_for_input.as_ref()) {
                (Some(descriptor), Some(args)) => {
                    self.acp_tool_registry
                        .render_title(descriptor, &func_ref.name, args)
                }
                (Some(descriptor), None) => {
                    let null_args = Value::Null;
                    self.acp_tool_registry
                        .render_title(descriptor, &func_ref.name, &null_args)
                }
                (None, _) => format!("{} (unsupported)", func_ref.name),
            };

            let call_id = acp::ToolCallId::new(Arc::from(call.id.clone()));
            let kind = tool_descriptor
                .map(|d| d.kind())
                .unwrap_or_else(|| self.acp_tool_registry.tool_kind(&func_ref.name));
            let initial_call = acp::ToolCall::new(call_id.clone(), title)
                .kind(kind)
                .status(acp::ToolCallStatus::Pending)
                .raw_input(args_value_for_input.clone());

            self.send_update(
                session_id,
                acp::SessionUpdate::ToolCall(initial_call.clone()),
            )
            .await?;

            let permission_override = if session.cancel_flag.get() {
                None
            } else if let (Some(ToolDescriptor::Acp(tool_kind)), Ok(args_value)) =
                (tool_descriptor, args_value_result.as_ref())
            {
                self.permission_prompter
                    .request_tool_permission(
                        client.as_ref(),
                        session_id,
                        &initial_call,
                        tool_kind,
                        args_value,
                    )
                    .await?
            } else {
                None
            };

            if tool_descriptor.is_some()
                && permission_override.is_none()
                && !session.cancel_flag.get()
            {
                let in_progress_fields =
                    acp::ToolCallUpdateFields::default().status(acp::ToolCallStatus::InProgress);
                let progress_update = acp::ToolCallUpdate::new(call_id.clone(), in_progress_fields);
                self.send_update(
                    session_id,
                    acp::SessionUpdate::ToolCallUpdate(progress_update),
                )
                .await?;
            }

            let mut report = if let Some(report) = permission_override {
                report
            } else if session.cancel_flag.get() {
                ToolExecutionReport::cancelled(&func_ref.name)
            } else {
                match (tool_descriptor, args_value_result) {
                    (Some(descriptor), Ok(args_value)) => {
                        self.execute_descriptor(
                            descriptor,
                            &func_ref.name,
                            &client,
                            session_id,
                            &args_value,
                        )
                        .await
                    }
                    (None, Ok(_)) => {
                        ToolExecutionReport::failure(&func_ref.name, "Unsupported tool")
                    }
                    (_, Err(error)) => ToolExecutionReport::failure(
                        &func_ref.name,
                        &format!("Invalid JSON arguments: {error}"),
                    ),
                }
            };

            if session.cancel_flag.get() && matches!(report.status, acp::ToolCallStatus::Completed)
            {
                report = ToolExecutionReport::cancelled(&func_ref.name);
            }

            let mut update_fields = acp::ToolCallUpdateFields::default().status(report.status);
            if !report.content.is_empty() {
                update_fields = update_fields.content(report.content.clone());
            }
            if !report.locations.is_empty() {
                update_fields = update_fields.locations(report.locations.clone());
            }
            if let Some(raw_output) = &report.raw_output {
                update_fields = update_fields.raw_output(raw_output.clone());
            }

            let update = acp::ToolCallUpdate::new(call_id.clone(), update_fields);

            self.send_update(session_id, acp::SessionUpdate::ToolCallUpdate(update))
                .await?;

            results.push(ToolCallResult {
                tool_call_id: call.id.clone(),
                llm_response: report.llm_response,
            });
        }

        Ok(results)
    }

    async fn execute_descriptor(
        &self,
        descriptor: ToolDescriptor,
        tool_name: &str,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> ToolExecutionReport {
        if tool_name == tools::RUN_PTY_CMD
            && let Some(report) = self
                .execute_terminal_via_client(tool_name, client, session_id, args)
                .await
        {
            return report;
        }

        match descriptor {
            ToolDescriptor::Acp(tool) => {
                self.execute_acp_tool(tool, client, session_id, args).await
            }
            ToolDescriptor::Local => self.execute_local_tool(tool_name, args).await,
        }
    }

    async fn execute_terminal_via_client(
        &self,
        tool_name: &str,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> Option<ToolExecutionReport> {
        if !self.client_supports_terminal() {
            return None;
        }

        match Self::requested_terminal_mode(args) {
            Ok(RunTerminalMode::Terminal) => None,
            Ok(RunTerminalMode::Pty) => Some(
                match self
                    .launch_client_terminal(tool_name, client, session_id, args)
                    .await
                {
                    Ok(report) => report,
                    Err(message) => ToolExecutionReport::failure(tool_name, &message),
                },
            ),
            Err(message) => Some(ToolExecutionReport::failure(tool_name, &message)),
        }
    }

    async fn launch_client_terminal(
        &self,
        tool_name: &str,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> Result<ToolExecutionReport, String> {
        let command_parts = Self::parse_terminal_command(args)?;
        let (program, rest) = command_parts
            .split_first()
            .ok_or_else(|| "command array cannot be empty".to_string())?;

        let working_dir = self.resolve_terminal_working_dir(args)?;
        let location_display = self.describe_terminal_location(working_dir.as_ref());
        let command_display = command_parts.join(" ");

        let request = acp::CreateTerminalRequest::new(session_id.clone(), program.to_string())
            .args(rest.to_vec())
            .cwd(working_dir.clone());

        let response = client
            .create_terminal(request)
            .await
            .map_err(|error| format!("Failed to create terminal: {error}"))?;
        let terminal_id = response.terminal_id;

        let mut content = Vec::with_capacity(5); // Pre-allocate for typical content sections
        let summary = match location_display.as_deref() {
            Some(".") | None => format!("Started terminal command: {command_display}"),
            Some(location) => {
                format!("Started terminal command in {location}: {command_display}")
            }
        };
        content.push(acp::ToolCallContent::from(summary));
        content.push(acp::ToolCallContent::Terminal(acp::Terminal::new(
            terminal_id.clone(),
        )));

        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: TOOL_SUCCESS_LABEL,
            TOOL_RESPONSE_KEY_TOOL: tool_name,
            "result": {
                "terminal_id": terminal_id.to_string(),
                "mode": "pty",
                "command": command_parts,
                "working_dir": location_display,
            }
        });

        Ok(ToolExecutionReport::success(
            content,
            Vec::with_capacity(0),
            payload,
        )) // Use with_capacity(0)
    }

    async fn execute_acp_tool(
        &self,
        tool: SupportedTool,
        client: &AgentSideConnection,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> ToolExecutionReport {
        match tool {
            SupportedTool::ReadFile => self
                .run_read_file(client, session_id, args)
                .await
                .unwrap_or_else(|message| ToolExecutionReport::failure(tools::READ_FILE, &message)),
            SupportedTool::ListFiles => self.run_list_files(args).await.unwrap_or_else(|message| {
                ToolExecutionReport::failure(tools::LIST_FILES, &message)
            }),
            SupportedTool::SwitchMode => self
                .run_switch_mode(session_id, args)
                .await
                .unwrap_or_else(|message| ToolExecutionReport::failure("switch_mode", &message)),
        }
    }

    pub(crate) async fn run_switch_mode(
        &self,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> Result<ToolExecutionReport, String> {
        let mode_id = args
            .get("mode_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing mode_id".to_string())?;

        let session = self
            .session_handle(session_id)
            .ok_or_else(|| "unknown session".to_string())?;

        let acp_mode_id = acp::SessionModeId::new(mode_id);

        if self.update_session_mode(&session, acp_mode_id.clone()) {
            // Signal mode change to the client
            self.send_update(
                session_id,
                acp::SessionUpdate::CurrentModeUpdate(acp::CurrentModeUpdate::new(acp_mode_id)),
            )
            .await
            .map_err(|e| format!("Failed to send mode update: {e}"))?;

            // Refresh available commands for the new mode
            self.send_available_commands_update(session_id)
                .await
                .map_err(|e| format!("Failed to refresh available commands: {e}"))?;
        }

        let payload = json!({
            TOOL_RESPONSE_KEY_STATUS: TOOL_SUCCESS_LABEL,
            TOOL_RESPONSE_KEY_TOOL: "switch_mode",
            "result": {
                "mode_id": mode_id,
            }
        });

        Ok(ToolExecutionReport::success(
            vec![acp::ToolCallContent::from(format!(
                "Successfully switched to mode: {mode_id}"
            ))],
            Vec::new(),
            payload,
        ))
    }
}
