use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

use crate::exec::events::{CompactionMode, CompactionTrigger};
use crate::hooks::lifecycle::types::{NotificationHookType, SessionEndReason};

use super::LifecycleHookEngine;

impl LifecycleHookEngine {
    pub(super) async fn build_session_start_payload(&self) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "SessionStart",
            "source": self.inner.trigger.as_str(),
            "transcript_path": transcript_path,
        }))
    }

    pub(crate) async fn build_session_end_payload(
        &self,
        turn_id: &str,
        reason: SessionEndReason,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "turn_id": turn_id,
            "cwd": cwd,
            "hook_event_name": "SessionEnd",
            "reason": reason.as_str(),
            "transcript_path": transcript_path,
        }))
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
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        Ok(json!({
            "session_id": self.inner.session_id,
            "parent_session_id": parent_session_id,
            "child_thread_id": child_thread_id,
            "agent_name": agent_name,
            "display_label": display_label,
            "background": background,
            "status": status,
            "cwd": cwd,
            "hook_event_name": "SubagentStart",
            "transcript_path": transcript_path.map(|path| path.to_string_lossy().into_owned()),
        }))
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
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        Ok(json!({
            "session_id": self.inner.session_id,
            "parent_session_id": parent_session_id,
            "child_thread_id": child_thread_id,
            "agent_name": agent_name,
            "display_label": display_label,
            "background": background,
            "status": status,
            "cwd": cwd,
            "hook_event_name": "SubagentStop",
            "transcript_path": transcript_path.map(|path| path.to_string_lossy().into_owned()),
        }))
    }

    pub(crate) async fn build_user_prompt_payload(
        &self,
        turn_id: &str,
        prompt: &str,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "turn_id": turn_id,
            "cwd": cwd,
            "hook_event_name": "UserPromptSubmit",
            "prompt": prompt,
            "transcript_path": transcript_path,
        }))
    }

    pub(super) async fn build_pre_tool_payload(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "PreToolUse",
            "tool_name": tool_name,
            "tool_input": tool_input.cloned().unwrap_or(Value::Null),
            "transcript_path": transcript_path,
        }))
    }

    pub(super) async fn build_post_tool_payload(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        tool_output: &Value,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "PostToolUse",
            "tool_name": tool_name,
            "tool_input": tool_input.cloned().unwrap_or(Value::Null),
            "tool_response": tool_output.clone(),
            "transcript_path": transcript_path,
        }))
    }

    pub(super) async fn build_notification_payload(
        &self,
        notification_type: NotificationHookType,
        title: &str,
        message: &str,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "Notification",
            "notification_type": notification_type.as_str(),
            "title": title,
            "message": message,
            "transcript_path": transcript_path,
        }))
    }

    pub(super) async fn build_pre_compact_payload(
        &self,
        trigger: CompactionTrigger,
        mode: CompactionMode,
        original_message_count: usize,
        compacted_message_count: usize,
        history_artifact_path: Option<&str>,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "PreCompact",
            "trigger": trigger,
            "mode": mode,
            "original_message_count": original_message_count,
            "compacted_message_count": compacted_message_count,
            "history_artifact_path": history_artifact_path,
            "transcript_path": transcript_path,
        }))
    }
}
