use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

use crate::exec::events::{CompactionMode, CompactionTrigger};
use crate::hooks::lifecycle::types::{NotificationHookType, SessionEndReason};
use crate::permissions::{PermissionRequest, PermissionRequestKind};

use super::LifecycleHookEngine;

impl LifecycleHookEngine {
    async fn base_payload(&self, hook_event_name: &str) -> Result<serde_json::Map<String, Value>> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        let permission_mode = *self.inner.permission_mode.read().await;
        Ok(serde_json::Map::from_iter([
            (
                "session_id".to_string(),
                Value::String(self.inner.session_id.clone()),
            ),
            ("cwd".to_string(), Value::String(cwd)),
            (
                "hook_event_name".to_string(),
                Value::String(hook_event_name.to_string()),
            ),
            (
                "permission_mode".to_string(),
                serde_json::to_value(permission_mode)?,
            ),
            (
                "transcript_path".to_string(),
                transcript_path.map(Value::String).unwrap_or(Value::Null),
            ),
        ]))
    }

    pub(super) async fn build_session_start_payload(&self) -> Result<Value> {
        let mut payload = self.base_payload("SessionStart").await?;
        payload.insert(
            "source".to_string(),
            Value::String(self.inner.trigger.as_str().to_owned()),
        );
        Ok(Value::Object(payload))
    }

    pub(crate) async fn build_session_end_payload(
        &self,
        turn_id: &str,
        reason: SessionEndReason,
    ) -> Result<Value> {
        let mut payload = self.base_payload("SessionEnd").await?;
        payload.insert("turn_id".to_string(), Value::String(turn_id.to_owned()));
        payload.insert(
            "reason".to_string(),
            Value::String(reason.as_str().to_owned()),
        );
        Ok(Value::Object(payload))
    }

    pub(crate) async fn build_subagent_start_payload(
        &self,
        parent_session_id: &str,
        child_thread_id: &str,
        agent_name: &str,
        display_label: &str,
        background: bool,
        status: &str,
        transcript_path: Option<&Path>,
    ) -> Result<Value> {
        let mut payload = self.base_payload("SubagentStart").await?;
        payload.insert(
            "parent_session_id".to_string(),
            Value::String(parent_session_id.to_owned()),
        );
        payload.insert(
            "child_thread_id".to_string(),
            Value::String(child_thread_id.to_owned()),
        );
        payload.insert(
            "agent_name".to_string(),
            Value::String(agent_name.to_owned()),
        );
        payload.insert(
            "display_label".to_string(),
            Value::String(display_label.to_owned()),
        );
        payload.insert("background".to_string(), Value::Bool(background));
        payload.insert("status".to_string(), Value::String(status.to_owned()));
        payload.insert(
            "transcript_path".to_string(),
            transcript_path
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .unwrap_or(Value::Null),
        );
        Ok(Value::Object(payload))
    }

    pub(crate) async fn build_subagent_stop_payload(
        &self,
        parent_session_id: &str,
        child_thread_id: &str,
        agent_name: &str,
        display_label: &str,
        background: bool,
        status: &str,
        transcript_path: Option<&Path>,
    ) -> Result<Value> {
        let mut payload = self.base_payload("SubagentStop").await?;
        payload.insert(
            "parent_session_id".to_string(),
            Value::String(parent_session_id.to_owned()),
        );
        payload.insert(
            "child_thread_id".to_string(),
            Value::String(child_thread_id.to_owned()),
        );
        payload.insert(
            "agent_name".to_string(),
            Value::String(agent_name.to_owned()),
        );
        payload.insert(
            "display_label".to_string(),
            Value::String(display_label.to_owned()),
        );
        payload.insert("background".to_string(), Value::Bool(background));
        payload.insert("status".to_string(), Value::String(status.to_owned()));
        payload.insert(
            "transcript_path".to_string(),
            transcript_path
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .unwrap_or(Value::Null),
        );
        Ok(Value::Object(payload))
    }

    pub(crate) async fn build_user_prompt_payload(
        &self,
        turn_id: &str,
        prompt: &str,
    ) -> Result<Value> {
        let mut payload = self.base_payload("UserPromptSubmit").await?;
        payload.insert("turn_id".to_string(), Value::String(turn_id.to_owned()));
        payload.insert("prompt".to_string(), Value::String(prompt.to_owned()));
        Ok(Value::Object(payload))
    }

    pub(super) async fn build_pre_tool_payload(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        tool_call_id: Option<&str>,
    ) -> Result<Value> {
        let mut payload = self.base_payload("PreToolUse").await?;
        payload.insert("tool_name".to_string(), Value::String(tool_name.to_owned()));
        payload.insert(
            "tool_input".to_string(),
            tool_input.cloned().unwrap_or(Value::Null),
        );
        payload.insert(
            "tool_call_id".to_string(),
            tool_call_id
                .map(|id| Value::String(id.to_owned()))
                .unwrap_or(Value::Null),
        );
        Ok(Value::Object(payload))
    }

    pub(super) async fn build_post_tool_payload(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        tool_output: &Value,
        tool_call_id: Option<&str>,
    ) -> Result<Value> {
        let mut payload = self.base_payload("PostToolUse").await?;
        payload.insert("tool_name".to_string(), Value::String(tool_name.to_owned()));
        payload.insert(
            "tool_input".to_string(),
            tool_input.cloned().unwrap_or(Value::Null),
        );
        payload.insert("tool_response".to_string(), tool_output.clone());
        payload.insert(
            "tool_call_id".to_string(),
            tool_call_id
                .map(|id| Value::String(id.to_owned()))
                .unwrap_or(Value::Null),
        );
        Ok(Value::Object(payload))
    }

    pub(super) async fn build_permission_request_payload(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        permission_request: &PermissionRequest,
        permission_suggestions: &[Value],
    ) -> Result<Value> {
        let mut payload = self.base_payload("PermissionRequest").await?;
        payload.insert("tool_name".to_string(), Value::String(tool_name.to_owned()));
        payload.insert(
            "tool_input".to_string(),
            tool_input.cloned().unwrap_or(Value::Null),
        );
        payload.insert(
            "permission_request".to_string(),
            build_permission_request_summary(permission_request),
        );
        payload.insert(
            "permission_suggestions".to_string(),
            Value::Array(permission_suggestions.to_vec()),
        );
        Ok(Value::Object(payload))
    }

    pub(super) async fn build_stop_payload(
        &self,
        last_assistant_message: &str,
        stop_hook_active: bool,
    ) -> Result<Value> {
        let mut payload = self.base_payload("Stop").await?;
        payload.insert(
            "last_assistant_message".to_string(),
            Value::String(last_assistant_message.to_owned()),
        );
        payload.insert(
            "stop_hook_active".to_string(),
            Value::Bool(stop_hook_active),
        );
        Ok(Value::Object(payload))
    }

    pub(super) async fn build_notification_payload(
        &self,
        notification_type: NotificationHookType,
        title: &str,
        message: &str,
    ) -> Result<Value> {
        let mut payload = self.base_payload("Notification").await?;
        payload.insert(
            "notification_type".to_string(),
            Value::String(notification_type.as_str().to_owned()),
        );
        payload.insert("title".to_string(), Value::String(title.to_owned()));
        payload.insert("message".to_string(), Value::String(message.to_owned()));
        Ok(Value::Object(payload))
    }

    pub(super) async fn build_pre_compact_payload(
        &self,
        trigger: CompactionTrigger,
        mode: CompactionMode,
        original_message_count: usize,
        compacted_message_count: usize,
        history_artifact_path: Option<&str>,
    ) -> Result<Value> {
        let mut payload = self.base_payload("PreCompact").await?;
        payload.insert("trigger".to_string(), serde_json::to_value(trigger)?);
        payload.insert("mode".to_string(), serde_json::to_value(mode)?);
        payload.insert(
            "original_message_count".to_string(),
            json!(original_message_count),
        );
        payload.insert(
            "compacted_message_count".to_string(),
            json!(compacted_message_count),
        );
        payload.insert(
            "history_artifact_path".to_string(),
            history_artifact_path
                .map(|path| json!(path))
                .unwrap_or(Value::Null),
        );
        Ok(Value::Object(payload))
    }
}

fn build_permission_request_summary(permission_request: &PermissionRequest) -> Value {
    let (kind, details) = match &permission_request.kind {
        PermissionRequestKind::Bash { command } => ("bash", json!({ "command": command })),
        PermissionRequestKind::Read { paths } => ("read", json!({ "paths": paths })),
        PermissionRequestKind::Edit { paths } => ("edit", json!({ "paths": paths })),
        PermissionRequestKind::Write { paths } => ("write", json!({ "paths": paths })),
        PermissionRequestKind::WebFetch { domains } => ("webfetch", json!({ "domains": domains })),
        PermissionRequestKind::Mcp { server, tool } => {
            ("mcp", json!({ "server": server, "tool": tool }))
        }
        PermissionRequestKind::Other => ("other", Value::Object(Default::default())),
    };

    json!({
        "tool_name": permission_request.exact_tool_name,
        "kind": kind,
        "details": details,
        "builtin_file_mutation": permission_request.builtin_file_mutation,
        "protected_write_paths": permission_request.protected_write_paths,
        "requires_protected_write_prompt": permission_request.requires_protected_write_prompt(),
    })
}
