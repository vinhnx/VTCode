use super::AgentRunner;
use super::constants::ROLE_USER;
use crate::config::constants::tools;
use crate::core::agent::session::AgentSessionState;
use crate::core::agent::task::TaskOutcome;
use crate::llm::providers::gemini::wire::{Content, Part};
use crate::utils::colors::style;
use serde_json::Value;
use std::path::Path;

impl AgentRunner {
    /// Record a tool call for loop detection and check if a hard limit has been exceeded.
    /// Returns true if execution should halt due to a loop.
    pub(super) fn check_for_loop(
        &self,
        name: &str,
        args: &Value,
        session_state: &mut AgentSessionState,
    ) -> bool {
        if let Some(warning) = self.loop_detector.lock().record_call(name, args) {
            if self.loop_detector.lock().is_hard_limit_exceeded(name) {
                if !self.quiet {
                    println!("{}", style(&warning).red().bold());
                }
                session_state.warnings.push(warning.clone());
                session_state.conversation.push(Content {
                    role: ROLE_USER.to_owned(),
                    parts: vec![Part::Text {
                        text: warning,
                        thought_signature: None,
                    }],
                });
                session_state.is_completed = true;
                session_state.outcome = TaskOutcome::LoopDetected;
                return true;
            }
            if !self.quiet {
                println!("{}", style(&warning).red().bold());
            }
            session_state.warnings.push(warning);
        }
        false
    }

    pub(super) fn normalize_tool_args(
        &self,
        name: &str,
        args: &Value,
        session_state: &mut AgentSessionState,
    ) -> Value {
        let Some(obj) = args.as_object() else {
            return args.clone();
        };

        let mut normalized = obj.clone();
        let workspace_path = self._workspace.to_string_lossy().to_string();
        let fallback_dir = session_state
            .last_dir_path
            .clone()
            .unwrap_or_else(|| workspace_path.clone());

        if name == tools::UNIFIED_SEARCH
            && matches!(
                normalized.get("action").and_then(Value::as_str),
                Some("grep" | "list")
            )
            && !normalized.contains_key("path")
        {
            normalized.insert("path".to_string(), Value::String(fallback_dir.clone()));
        }

        if name == tools::LIST_FILES && !normalized.contains_key("path") {
            normalized.insert("path".to_string(), Value::String(fallback_dir));
        }

        if name == tools::READ_FILE
            && !normalized.contains_key("file_path")
            && !normalized.contains_key("path")
            && let Some(last_file) = session_state.last_file_path.clone()
        {
            normalized.insert("path".to_string(), Value::String(last_file));
        }

        if matches!(
            name,
            tools::WRITE_FILE | tools::EDIT_FILE | tools::CREATE_FILE
        ) && !normalized.contains_key("path")
            && let Some(last_file) = session_state.last_file_path.clone()
        {
            normalized.insert("path".to_string(), Value::String(last_file));
        }

        let normalized = Value::Object(normalized);
        if let Some(transform) = &self.tool_arg_transform {
            return transform(name, normalized);
        }
        normalized
    }

    pub(super) fn update_last_paths_from_args(
        &self,
        name: &str,
        args: &Value,
        session_state: &mut AgentSessionState,
    ) {
        if let Some(path) = args.get("file_path").and_then(|value| value.as_str()) {
            session_state.last_file_path = Some(path.to_string());
            if let Some(parent) = Path::new(path).parent() {
                session_state.last_dir_path = Some(parent.to_string_lossy().to_string());
            }
            return;
        }

        if let Some(path) = args.get("path").and_then(|value| value.as_str()) {
            if matches!(
                name,
                tools::READ_FILE | tools::WRITE_FILE | tools::EDIT_FILE | tools::CREATE_FILE
            ) {
                session_state.last_file_path = Some(path.to_string());
                if let Some(parent) = Path::new(path).parent() {
                    session_state.last_dir_path = Some(parent.to_string_lossy().to_string());
                }
            } else {
                session_state.last_dir_path = Some(path.to_string());
            }
        }
    }
}
