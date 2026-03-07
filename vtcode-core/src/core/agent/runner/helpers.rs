use serde_json::Value;

pub(super) fn detect_textual_exec_tool_call(text: &str) -> Option<Value> {
    const FENCE_PREFIXES: [&str; 4] = [
        "```tool:unified_exec",
        "```unified_exec",
        "```tool:run_pty_cmd",
        "```run_pty_cmd",
    ];

    let (start_idx, prefix) = FENCE_PREFIXES
        .iter()
        .filter_map(|candidate| text.find(candidate).map(|idx| (idx, *candidate)))
        .min_by_key(|(idx, _)| *idx)?;

    // Require a fenced block owned by the model to avoid executing echoed examples.
    let mut remainder = &text[start_idx + prefix.len()..];
    if remainder.starts_with('\r') {
        remainder = &remainder[1..];
    }
    remainder = remainder.strip_prefix('\n')?;

    let fence_close = remainder.find("```")?;
    let block = remainder[..fence_close].trim();
    if block.is_empty() {
        return None;
    }

    let parsed = serde_json::from_str::<Value>(block)
        .or_else(|_| json5::from_str::<Value>(block))
        .ok()?;
    parsed.as_object()?;
    Some(parsed)
}

#[cfg(test)]
mod tests {
    use super::detect_textual_exec_tool_call;
    use serde_json::json;

    #[test]
    fn detects_unified_exec_fence() {
        let text = "```tool:unified_exec\n{\"command\":\"pwd\"}\n```";
        assert_eq!(
            detect_textual_exec_tool_call(text),
            Some(json!({"command":"pwd"}))
        );
    }

    #[test]
    fn detects_legacy_run_pty_cmd_fence() {
        let text = "```run_pty_cmd\n{\"command\":\"pwd\"}\n```";
        assert_eq!(
            detect_textual_exec_tool_call(text),
            Some(json!({"command":"pwd"}))
        );
    }

    #[test]
    fn ignores_non_object_payloads() {
        let text = "```unified_exec\n[\"pwd\"]\n```";
        assert!(detect_textual_exec_tool_call(text).is_none());
    }
}
