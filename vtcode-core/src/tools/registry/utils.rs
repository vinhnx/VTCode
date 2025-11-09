use serde_json::{Value, json};

pub(super) fn normalize_tool_output(mut val: Value) -> Value {
    if !val.is_object() {
        return json!({ "success": true, "result": val });
    }
    let obj = val.as_object_mut().unwrap();
    obj.entry("success").or_insert(json!(true));
    if !obj.contains_key("stdout") {
        if let Some(output) = obj.get("output").and_then(|v| v.as_str()) {
            obj.insert("stdout".into(), json!(output.trim_end()));
        }
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
    val
}

pub(super) fn normalize_whitespace(s: &str) -> String {
    s.lines()
        .map(|line| {
            let trimmed = line.trim_end();
            trimmed.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
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
