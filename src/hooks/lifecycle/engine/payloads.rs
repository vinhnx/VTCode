use anyhow::Result;
use serde_json::{Value, json};

use crate::hooks::lifecycle::types::SessionEndReason;

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

    pub(super) async fn build_session_end_payload(
        &self,
        reason: SessionEndReason,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "SessionEnd",
            "reason": reason.as_str(),
            "transcript_path": transcript_path,
        }))
    }

    pub(super) async fn build_user_prompt_payload(&self, prompt: &str) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
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

    #[allow(dead_code)]
    pub(super) async fn build_task_completion_payload(
        &self,
        task_name: &str,
        status: &str,
        details: Option<&Value>,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "TaskCompletion",
            "task_name": task_name,
            "status": status,
            "details": details.cloned().unwrap_or(Value::Null),
            "transcript_path": transcript_path,
        }))
    }

    pub(super) async fn build_teammate_idle_payload(
        &self,
        teammate: &str,
        details: Option<&Value>,
    ) -> Result<Value> {
        let cwd = self.inner.workspace.to_string_lossy().into_owned();
        let transcript_path = self.current_transcript_path().await;
        Ok(json!({
            "session_id": self.inner.session_id,
            "cwd": cwd,
            "hook_event_name": "TeammateIdle",
            "teammate": teammate,
            "details": details.cloned().unwrap_or(Value::Null),
            "transcript_path": transcript_path,
        }))
    }
}
