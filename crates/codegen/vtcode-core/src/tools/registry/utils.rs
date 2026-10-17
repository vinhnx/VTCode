use serde_json::{Value, json};

pub(super) fn normalize_tool_output(mut val: Value) -> Value {
    if !val.is_object() {
        return json!({ "success": true, "result": val });
    }
    let Some(obj) = val.as_object_mut() else {
        return json!({ "success": true, "result": val });
    };
    let has_spool_reference = obj
        .get("spool_path")
        .and_then(Value::as_str)
        .is_some_and(|path| !path.is_empty());
    obj.entry("success").or_insert(json!(true));
    let should_remove_stdout = {
        let out_trim = obj.get("output").and_then(|v| v.as_str()).map(str::trim_end);
        let std_trim = obj.get("stdout").and_then(|v| v.as_str()).map(str::trim_end);
        out_trim.is_some() && out_trim == std_trim
    };

    if should_remove_stdout {
        obj.remove("stdout");
    } else if let Some(stdout) = obj.get_mut("stdout")
        && let Some(s) = stdout.as_str()
    {
        *stdout = json!(s.trim_end());
    }

    if let Some(stderr) = obj.get_mut("stderr")
        && let Some(s) = stderr.as_str()
    {
        *stderr = json!(s.trim_end());
    }

    if !obj.contains_key("path")
        && obj
            .get("content_kind")
            .and_then(|value| value.as_str())
            .is_some_and(|value| value == "text")
        && let Some(content) = obj.get_mut("content")
        && let Some(s) = content.as_str()
    {
        *content = json!(s.trim_end());
    }

    if obj.get("working_directory").is_some_and(Value::is_null) {
        obj.remove("working_directory");
    }

    if obj.get("id").zip(obj.get("session_id")).is_some_and(|(id, sid)| id == sid) {
        obj.remove("id");
    }

    if obj
        .get("process_id")
        .zip(obj.get("session_id"))
        .is_some_and(|(process_id, sid)| process_id == sid)
    {
        obj.remove("process_id");
    }

    for deprecated_key in [
        "follow_up_prompt",
        "next_poll_args",
        "preferred_next_action",
        "spool_hint",
        "spooled_to_file",
    ] {
        obj.remove(deprecated_key);
    }
    if !has_spool_reference {
        obj.remove("spooled_bytes");
    }
    obj.remove("raw_output");
    val
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::normalize_tool_output;

    #[test]
    fn keeps_git_diff_output_single_field() {
        let normalized = normalize_tool_output(json!({
            "output": "diff --git a/file b/file\n+line\n",
            "content_type": "git_diff"
        }));

        assert_eq!(normalized["output"], "diff --git a/file b/file\n+line\n");
        assert!(normalized.get("stdout").is_none());
    }

    #[test]
    fn does_not_backfill_stdout_for_non_diff_output() {
        let normalized = normalize_tool_output(json!({
            "output": "ok\n"
        }));

        assert!(normalized.get("stdout").is_none());
        assert_eq!(normalized["output"], "ok\n");
    }

    #[test]
    fn does_not_backfill_stdout_for_command_like_output() {
        let normalized = normalize_tool_output(json!({
            "output": "hello\n",
            "command": "ls -la",
            "exit_code": 0,
            "is_exited": true
        }));

        assert!(normalized.get("stdout").is_none());
    }

    #[test]
    fn removes_duplicate_stdout_for_command_like_output() {
        let normalized = normalize_tool_output(json!({
            "output": "hello\n",
            "stdout": "hello\n",
            "command": "ls -la",
            "exit_code": 0,
            "is_exited": true
        }));

        assert!(normalized.get("stdout").is_none());
        assert_eq!(normalized["output"], "hello\n");
    }

    #[test]
    fn preserves_distinct_stdout_for_command_like_output() {
        let normalized = normalize_tool_output(json!({
            "output": "stderr line\n",
            "stdout": "stdout line\n",
            "command": "printf stdout",
            "exit_code": 0,
            "is_exited": true
        }));

        assert_eq!(normalized["output"], "stderr line\n");
        assert_eq!(normalized["stdout"], "stdout line");
    }

    #[test]
    fn inspect_output_does_not_backfill_stdout() {
        let normalized = normalize_tool_output(json!({
            "output": "1: src/main.rs",
            "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
            "content_type": "exec_inspect"
        }));

        assert!(normalized.get("stdout").is_none());
        assert_eq!(normalized["output"], "1: src/main.rs");
    }

    #[test]
    fn strips_internal_exec_raw_output_field() {
        let normalized = normalize_tool_output(json!({
            "output": "preview",
            "raw_output": "full output",
            "command": "cargo check"
        }));

        assert_eq!(normalized["output"], "preview");
        assert!(normalized.get("raw_output").is_none());
    }

    #[test]
    fn drops_redundant_exec_metadata_fields() {
        let normalized = normalize_tool_output(json!({
            "output": "preview\n",
            "stdout": "preview\n",
            "session_id": "run-123",
            "id": "run-123",
            "process_id": "run-123",
            "working_directory": null,
            "spool_hint": "read the spool",
            "spooled_bytes": 4096,
            "spooled_to_file": true,
            "follow_up_prompt": "continue",
            "next_poll_args": {"session_id": "run-123"},
            "preferred_next_action": "poll"
        }));

        assert_eq!(normalized["output"], "preview\n");
        assert!(normalized.get("stdout").is_none());
        assert!(normalized.get("id").is_none());
        assert!(normalized.get("process_id").is_none());
        assert!(normalized.get("working_directory").is_none());
        assert!(normalized.get("spool_hint").is_none());
        assert!(normalized.get("spooled_bytes").is_none());
        assert!(normalized.get("spooled_to_file").is_none());
        assert!(normalized.get("follow_up_prompt").is_none());
        assert!(normalized.get("next_poll_args").is_none());
        assert!(normalized.get("preferred_next_action").is_none());
    }

    #[test]
    fn preserves_spooled_bytes_for_spool_reference() {
        let normalized = normalize_tool_output(json!({
            "output": "preview\n",
            "spool_path": ".vtcode/context/tool_outputs/run-123.txt",
            "spooled_bytes": 4096
        }));

        assert_eq!(normalized["spooled_bytes"], 4096);
    }

    #[test]
    fn preserves_complete_source_text_including_trailing_newline() {
        let normalized = normalize_tool_output(json!({
            "content": "via alias\n",
            "path": "note.txt",
            "content_kind": "text"
        }));

        assert_eq!(normalized["content"], "via alias\n");
    }
}
