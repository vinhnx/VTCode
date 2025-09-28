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

pub(super) fn astgrep_to_concise(v: Value) -> Value {
    let mut out = Vec::new();
    match v {
        Value::Array(arr) => {
            for item in arr.into_iter() {
                let mut path = None;
                let mut line = None;
                let mut text = None;

                if let Some(p) = item.get("path").and_then(|p| p.as_str()) {
                    path = Some(p.to_string());
                }
                if line.is_none() {
                    line = item
                        .get("range")
                        .and_then(|r| r.get("start"))
                        .and_then(|s| s.get("line"))
                        .and_then(|l| l.as_u64())
                        .or(item
                            .get("start")
                            .and_then(|s| s.get("line"))
                            .and_then(|l| l.as_u64()));
                }
                if text.is_none() {
                    text = item
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                        .or(item
                            .get("lines")
                            .and_then(|l| l.get("text"))
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string()))
                        .or(item
                            .get("matched")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string()));
                }

                out.push(json!({
                    "path": path.unwrap_or_default(),
                    "line_number": line.unwrap_or(0),
                    "text": text.unwrap_or_default(),
                }));
            }
            Value::Array(out)
        }
        other => other,
    }
}

pub(super) fn astgrep_issues_to_concise(v: Value) -> Value {
    let mut out = Vec::new();
    match v {
        Value::Array(arr) => {
            for item in arr.into_iter() {
                let path = item
                    .get("path")
                    .or_else(|| item.get("file"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("")
                    .to_string();
                let line = item
                    .get("range")
                    .and_then(|r| r.get("start"))
                    .and_then(|s| s.get("line"))
                    .and_then(|l| l.as_u64())
                    .or(item
                        .get("start")
                        .and_then(|s| s.get("line"))
                        .and_then(|l| l.as_u64()))
                    .or(item.get("line").and_then(|l| l.as_u64()))
                    .unwrap_or(0);
                let message = item
                    .get("message")
                    .and_then(|m| m.as_str())
                    .or(item.get("text").and_then(|t| t.as_str()))
                    .unwrap_or("")
                    .to_string();
                let severity = item.get("severity").and_then(|s| s.as_str()).unwrap_or("");
                let rule = item
                    .get("rule")
                    .or_else(|| item.get("rule_id"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("");
                out.push(json!({
                    "path": path,
                    "line_number": line,
                    "message": message,
                    "severity": severity,
                    "rule": rule,
                }));
            }
            Value::Array(out)
        }
        other => other,
    }
}

pub(super) fn astgrep_changes_to_concise(v: Value) -> Value {
    let mut out = Vec::new();
    match v {
        Value::Array(arr) => {
            for item in arr.into_iter() {
                let path = item
                    .get("path")
                    .or_else(|| item.get("file"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("")
                    .to_string();
                let line = item
                    .get("range")
                    .and_then(|r| r.get("start"))
                    .and_then(|s| s.get("line"))
                    .and_then(|l| l.as_u64())
                    .or(item
                        .get("start")
                        .and_then(|s| s.get("line"))
                        .and_then(|l| l.as_u64()))
                    .or(item.get("line").and_then(|l| l.as_u64()))
                    .unwrap_or(0);
                let before = item
                    .get("text")
                    .and_then(|t| t.as_str())
                    .or(item.get("matched").and_then(|t| t.as_str()))
                    .or(item.get("before").and_then(|t| t.as_str()))
                    .unwrap_or("");
                let after = item
                    .get("replacement")
                    .and_then(|t| t.as_str())
                    .or(item.get("after").and_then(|t| t.as_str()))
                    .unwrap_or("");
                let note = if !after.is_empty() {
                    format!("{} -> {}", truncate(before, 80), truncate(after, 80))
                } else {
                    truncate(before, 120)
                };
                out.push(json!({
                    "path": path,
                    "line_number": line,
                    "note": note,
                }));
            }
            Value::Array(out)
        }
        other => other,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}
