//! Passive schemas for durable local matrix control and attempt-owned reports.

use serde_json::{Value, json};

/// Model-visible description of deterministic matrix orchestration.
pub const MATRIX_DESCRIPTION: &str = "Manage a durable local task matrix. create persists an explicit specification without launching workers; start freezes it and begins scheduler-owned execution. status shows task/resource/verification progress; pause stops new dispatch, resume reconciles durable state before dispatch, retry requests a coordinator decision for a task, and cancel is terminal. report is worker-only: runtime supplies task and attempt identity and validates durable command evidence. Final success requires every declared check against the final workspace generation; worker summaries are insufficient.";

/// Wire parameters; lifecycle validation and report authorization are runtime-owned.
#[must_use]
pub fn matrix_parameters() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["action"],
        "properties": {
            "action": {"type": "string", "enum": ["create", "start", "status", "pause", "resume", "retry", "cancel", "report"]},
            "matrix_id": {"type": "string", "minLength": 1, "description": "Coordinator controls: matrix to inspect or control."},
            "task_id": {"type": "string", "minLength": 1, "description": "Coordinator retry only: task requiring another attempt. Reports cannot select a task."},
            "spec": matrix_spec_schema(),
            "outcome": {"type": "string", "enum": ["executed", "failed", "interrupted", "timed_out", "permission_denied", "budget_exhausted"], "description": "Worker report only; runtime authenticates the current assignment. executed means instructions finished, not final verification success."},
            "evidence_ids": {"type": "array", "items": {"type": "string", "minLength": 1}, "description": "Worker report only: canonical durable evidence references; unrelated, stale, or cancelled commands cannot verify success."}
        }
    })
}

fn matrix_spec_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "tasks", "resources"],
        "properties": {
            "id": {"type": "string", "minLength": 1},
            "resources": resource_map_schema(),
            "tasks": {"type": "array", "minItems": 1, "items": {
                "type": "object", "additionalProperties": false,
                "required": ["id", "instructions", "workspace", "access", "checks", "timeout_secs"],
                "properties": {
                    "id": {"type": "string", "minLength": 1, "description": "Stable unique task ID."},
                    "instructions": {"type": "string", "minLength": 1},
                    "dependencies": {"type": "array", "items": {"type": "string", "minLength": 1}, "default": []},
                    "workspace": {"type": "string", "minLength": 1, "description": "Session-workspace-relative directory; use . for the root. Absolute paths, traversal, and symlink escapes are rejected."},
                    "access": {"type": "string", "enum": ["read", "write"], "description": "Readers may overlap; a writer excludes every other matrix task."},
                    "checks": {"type": "array", "minItems": 1, "items": {"type": "string", "minLength": 1}, "description": "Standalone verification commands rerun after execution against the final workspace generation."},
                    "inputs": {"type": "array", "items": {"type": "string", "minLength": 1}, "default": [], "description": "Declared untracked input files relative to this task's workspace, included with tracked source in the verification fingerprint."},
                    "resources": resource_map_schema(),
                    "timeout_secs": {"type": "integer", "minimum": 1},
                    "replay_safe": {"type": "boolean", "default": false, "description": "Allows one automatic interrupted/timeout retry only after owned work is confirmed stopped."}
                }
            }}
        }
    })
}

fn resource_map_schema() -> Value {
    json!({"type": "object", "additionalProperties": {"type": "integer", "minimum": 1}, "description": "Named resource capacities on the spec, or required quantities on a task; all quantities are positive integers."})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_schema_keeps_report_identity_runtime_owned() {
        let schema = matrix_parameters();
        assert_eq!(schema["additionalProperties"], false);
        for forbidden in ["attempt_id", "worker_id", "thread_id", "generation"] {
            assert!(schema["properties"].get(forbidden).is_none(), "{forbidden}");
        }
        assert!(
            schema["properties"]["task_id"]["description"]
                .as_str()
                .unwrap()
                .contains("retry only")
        );
        assert_eq!(
            schema["properties"]["spec"]["properties"]["tasks"]["items"]["properties"]["replay_safe"]["default"],
            false
        );
    }

    #[test]
    fn matrix_schema_requires_checks_timeouts_and_positive_resources() {
        let schema = matrix_parameters();
        let task = &schema["properties"]["spec"]["properties"]["tasks"]["items"];
        for field in ["id", "instructions", "workspace", "access", "checks", "timeout_secs"] {
            assert!(task["required"].as_array().unwrap().contains(&json!(field)), "{field}");
        }
        assert_eq!(task["properties"]["checks"]["minItems"], 1);
        assert_eq!(task["properties"]["resources"]["additionalProperties"]["minimum"], 1);
        assert_eq!(task["properties"]["timeout_secs"]["minimum"], 1);
    }
}
