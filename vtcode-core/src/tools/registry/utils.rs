use serde_json::{Value, json};

pub(super) fn normalize_tool_output(mut val: Value) -> Value {
    if !val.is_object() {
        return json!({ "success": true, "result": val });
    }
    let obj = val.as_object_mut().unwrap();
    obj.entry("success").or_insert(json!(true));
    let is_command_like_output = is_command_like_output(obj);
    let is_git_diff_output = obj
        .get("content_type")
        .and_then(|value| value.as_str())
        .map(|value| value == "git_diff")
        .unwrap_or(false);

    if let Some(stdout) = obj.get_mut("stdout")
        && let Some(s) = stdout.as_str()
    {
        *stdout = json!(s.trim_end());
    }

    if is_git_diff_output || is_command_like_output {
        let output_trimmed = obj
            .get("output")
            .and_then(|value| value.as_str())
            .map(str::trim_end);
        let stdout_trimmed = obj
            .get("stdout")
            .and_then(|value| value.as_str())
            .map(str::trim_end);
        if output_trimmed.is_some() && output_trimmed == stdout_trimmed {
            obj.remove("stdout");
        }
    }

    if let Some(stderr) = obj.get_mut("stderr")
        && let Some(s) = stderr.as_str()
    {
        *stderr = json!(s.trim_end());
    }
    val
}

fn is_command_like_output(obj: &serde_json::Map<String, Value>) -> bool {
    obj.contains_key("command")
        || obj.contains_key("working_directory")
        || obj.contains_key("session_id")
        || obj.contains_key("process_id")
        || obj.contains_key("spool_path")
        || obj.contains_key("is_exited")
        || obj.contains_key("exit_code")
        || obj.contains_key("rows")
        || obj.contains_key("cols")
        || obj
            .get("content_type")
            .and_then(Value::as_str)
            .is_some_and(|value| value == "exec_inspect")
}

pub(super) fn lines_match(content_lines: &[&str], expected_lines: &[&str]) -> bool {
    if content_lines.len() != expected_lines.len() {
        return false;
    }

    content_lines
        .iter()
        .zip(expected_lines.iter())
        .all(|(content_line, expected_line)| content_line.trim() == expected_line.trim())
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
    fn inspect_output_does_not_backfill_stdout() {
        let normalized = normalize_tool_output(json!({
            "output": "1: src/main.rs",
            "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
            "content_type": "exec_inspect"
        }));

        assert!(normalized.get("stdout").is_none());
        assert_eq!(normalized["output"], "1: src/main.rs");
    }
}
