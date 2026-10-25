use super::ZedAgent;
use crate::acp;
use crate::acp::Error as SdkError;
use crate::permissions::PermissionToolContext;
use crate::reports::ToolExecutionReport;
use crate::tooling::{SupportedTool, ToolDescriptor};
use crate::zed::connection::ConnectionHandle;
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use tokio::time::Instant;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider::ToolCall as ProviderToolCall;

use super::super::types::{SessionHandle, ToolCallResult};

impl ZedAgent {
    pub(super) async fn execute_tool_calls(
        &self,
        session: &SessionHandle,
        session_id: &acp::SessionId,
        calls: &[ProviderToolCall],
    ) -> Result<Vec<ToolCallResult>, SdkError> {
        if calls.is_empty() {
            return Ok(Vec::new());
        }

        let Some(client) = self.client() else {
            return Ok(calls
                .iter()
                .map(|call| {
                    Self::tool_call_result_from_report(
                        call,
                        ToolExecutionReport::failure(Self::tool_name_from_call(call), "Client connection unavailable"),
                    )
                })
                .collect());
        };

        let mut results = Vec::with_capacity(calls.len());

        for call in calls {
            self.pace_tool_call(session).await;
            results.push(self.execute_tool_call(session, session_id, call, client.as_ref()).await?);
        }

        Ok(results)
    }

    async fn execute_tool_call(
        &self,
        session: &SessionHandle,
        session_id: &acp::SessionId,
        call: &ProviderToolCall,
        client: &ConnectionHandle,
    ) -> Result<ToolCallResult, SdkError> {
        let Some(func_ref) = call.function.as_ref() else {
            return Ok(Self::tool_call_result_from_report(
                call,
                ToolExecutionReport::failure("unknown", "Malformed tool call: missing function payload"),
            ));
        };
        let tool_descriptor = self.acp_tool_registry.lookup(&func_ref.name);
        let args_value_result: Result<Value, _> = serde_json::from_str(&func_ref.arguments);
        let args_value_for_input = args_value_result.as_ref().ok().cloned();
        let title = match (tool_descriptor, args_value_for_input.as_ref()) {
            (Some(descriptor), Some(args)) => self.acp_tool_registry.render_title(descriptor, &func_ref.name, args),
            (Some(descriptor), None) => {
                let null_args = Value::Null;
                self.acp_tool_registry.render_title(descriptor, &func_ref.name, &null_args)
            }
            (None, _) => format!("{} (unsupported)", func_ref.name),
        };

        let call_id = acp::ToolCallId::new(Arc::from(call.id.clone()));
        let kind = match tool_descriptor {
            Some(ToolDescriptor::Acp(tool)) => tool.kind(),
            Some(ToolDescriptor::Local) | None => self
                .acp_tool_registry
                .tool_kind_for_call(&func_ref.name, args_value_for_input.as_ref()),
        };
        let initial_call = acp::ToolCall::new(call_id.clone(), title)
            .kind(kind)
            .status(acp::ToolCallStatus::Pending)
            .raw_input(args_value_for_input.clone());

        self.send_update(session_id, acp::SessionUpdate::ToolCall(initial_call.clone()))
            .await?;

        let cancel_active = session.cancel_flag.load(Ordering::Relaxed);
        let mut permission_granted = false;
        let permission_override = if let (false, Some(descriptor), Ok(args_value)) =
            (cancel_active, tool_descriptor, args_value_result.as_ref())
        {
            let outcome = self
                .request_tool_permission(
                    client,
                    session_id,
                    &initial_call,
                    descriptor,
                    PermissionToolContext::new(&func_ref.name, kind, initial_call.title.as_str()),
                    args_value,
                )
                .await?;
            // `None` means the client answered the permission request with an
            // allow option; the registry policy gate still needs that approval.
            permission_granted = outcome.is_none();
            outcome
        } else {
            None
        };

        let cancel_after_permission = session.cancel_flag.load(Ordering::Relaxed);
        if tool_descriptor.is_some() && permission_override.is_none() && !cancel_after_permission {
            let in_progress_fields = acp::ToolCallUpdateFields::default().status(acp::ToolCallStatus::InProgress);
            let progress_update = acp::ToolCallUpdate::new(call_id.clone(), in_progress_fields);
            self.send_update(session_id, acp::SessionUpdate::ToolCallUpdate(progress_update))
                .await?;
        }

        let mut report = if let Some(report) = permission_override {
            report
        } else if cancel_after_permission {
            ToolExecutionReport::cancelled(&func_ref.name)
        } else {
            match (tool_descriptor, args_value_result) {
                (Some(descriptor), Ok(args_value)) => {
                    self.execute_descriptor(
                        descriptor,
                        &func_ref.name,
                        client,
                        session_id,
                        call.id.as_str(),
                        &args_value,
                        permission_granted,
                    )
                    .await
                }
                (None, Ok(_)) => ToolExecutionReport::failure(&func_ref.name, "Unsupported tool"),
                (_, Err(error)) => {
                    ToolExecutionReport::failure(&func_ref.name, &format!("Invalid JSON arguments: {error}"))
                }
            }
        };

        if session.cancel_flag.load(Ordering::Relaxed) && matches!(report.status, acp::ToolCallStatus::Completed) {
            report = ToolExecutionReport::cancelled(&func_ref.name);
        }

        let update = acp::ToolCallUpdate::new(call_id, Self::update_fields_from_report(&report));
        self.send_update(session_id, acp::SessionUpdate::ToolCallUpdate(update)).await?;

        Ok(Self::tool_call_result_from_report(call, report))
    }

    async fn pace_tool_call(&self, session: &SessionHandle) {
        let Some(delay) = self.tool_call_delay else {
            return;
        };

        let sleep_for = {
            let Ok(data) = session.data.lock() else {
                return;
            };
            data.last_tool_call_at.and_then(|last_call| {
                let elapsed = last_call.elapsed();
                delay.checked_sub(elapsed)
            })
        };

        if let Some(duration) = sleep_for {
            tokio::time::sleep(duration).await;
        }

        if let Ok(mut data) = session.data.lock() {
            data.last_tool_call_at = Some(Instant::now());
        }
    }

    fn tool_name_from_call(call: &ProviderToolCall) -> &str {
        call.function
            .as_ref()
            .map(|function| function.name.as_str())
            .unwrap_or("unknown")
    }

    fn tool_call_result_from_report(call: &ProviderToolCall, report: ToolExecutionReport) -> ToolCallResult {
        ToolCallResult {
            tool_call_id: call.id.clone(),
            llm_response: report.llm_response,
        }
    }

    fn update_fields_from_report(report: &ToolExecutionReport) -> acp::ToolCallUpdateFields {
        let mut fields = acp::ToolCallUpdateFields::default().status(report.status);
        if !report.content.is_empty() {
            fields = fields.content(report.content.clone());
        }
        if !report.locations.is_empty() {
            fields = fields.locations(report.locations.clone());
        }
        if let Some(raw_output) = &report.raw_output {
            fields = fields.raw_output(raw_output.clone());
        }
        fields
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Descriptor, ACP call context, arguments, and the permission decision form one dispatch boundary."
    )]
    async fn execute_descriptor(
        &self,
        descriptor: ToolDescriptor,
        tool_name: &str,
        client: &ConnectionHandle,
        session_id: &acp::SessionId,
        call_id: &str,
        args: &Value,
        permission_granted: bool,
    ) -> ToolExecutionReport {
        match descriptor {
            ToolDescriptor::Acp(tool) => self.execute_acp_tool(tool, client, session_id, args).await,
            ToolDescriptor::Local => self.execute_local_tool(tool_name, args, call_id, permission_granted).await,
        }
    }

    async fn request_tool_permission(
        &self,
        client: &ConnectionHandle,
        session_id: &acp::SessionId,
        call: &acp::ToolCall,
        descriptor: ToolDescriptor,
        tool: PermissionToolContext<'_>,
        args: &Value,
    ) -> Result<Option<ToolExecutionReport>, SdkError> {
        match descriptor {
            ToolDescriptor::Acp(tool) => {
                self.permission_prompter
                    .request_tool_permission(client, session_id, call, tool, args)
                    .await
            }
            ToolDescriptor::Local => {
                self.permission_prompter
                    .request_named_tool_permission(client, session_id, call, tool, args)
                    .await
            }
        }
    }

    async fn execute_acp_tool(
        &self,
        tool: SupportedTool,
        client: &ConnectionHandle,
        session_id: &acp::SessionId,
        args: &Value,
    ) -> ToolExecutionReport {
        match tool {
            SupportedTool::ReadFile => self
                .run_read_file(client, session_id, args)
                .await
                .unwrap_or_else(|message| ToolExecutionReport::failure(tools::READ_FILE, &message)),
            SupportedTool::ListFiles => self
                .run_list_files(args)
                .await
                .unwrap_or_else(|message| ToolExecutionReport::failure(tools::LIST_FILES, &message)),
        }
    }
}

#[cfg(test)]
mod tests;
