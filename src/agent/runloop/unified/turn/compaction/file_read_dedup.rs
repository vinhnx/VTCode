use super::*;

fn is_read_file_tool_name(tool_name: &str) -> bool {
    tool_name == tool_names::READ_FILE || tool_name.ends_with(".read_file")
}

fn collect_file_read_tool_kinds(history: &[Message]) -> HashMap<String, FileReadToolKind> {
    let mut kinds = HashMap::new();
    for message in history {
        let Some(tool_calls) = message.tool_calls.as_ref() else {
            continue;
        };
        for tc in tool_calls {
            let Some(tn) = tc.tool_name() else {
                continue;
            };
            let kind = if is_read_file_tool_name(tn) {
                Some(FileReadToolKind::ReadFile)
            } else if tn == tool_names::UNIFIED_FILE {
                tc.execution_arguments().ok().and_then(|args| {
                    args.get("action")
                        .and_then(Value::as_str)
                        .filter(|a| *a == "read")
                        .map(|_| FileReadToolKind::UnifiedFileRead)
                })
            } else {
                None
            };
            if let Some(k) = kind {
                kinds.insert(tc.id.clone(), k);
            }
        }
    }
    kinds
}

fn normalize_file_read_target(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.replace('\\', "/"))
}

fn build_file_read_dedup_key(payload: &Value) -> Option<FileReadDedupKey> {
    let obj = payload.as_object()?;
    if obj.get("items").is_some()
        || obj.get("error").is_some()
        || obj
            .get("spool_chunked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || obj
            .get("has_more")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return None;
    }
    let target = obj
        .get("file_path")
        .and_then(Value::as_str)
        .or_else(|| obj.get("path").and_then(Value::as_str))
        .and_then(normalize_file_read_target)?;
    Some(FileReadDedupKey {
        target,
        start_line: obj.get("start_line").and_then(Value::as_u64),
        end_line: obj.get("end_line").and_then(Value::as_u64),
        spool_path: obj
            .get("spool_path")
            .and_then(Value::as_str)
            .and_then(normalize_file_read_target),
    })
}

fn build_file_read_placeholder_content(payload: &Value, key: &FileReadDedupKey) -> String {
    let mut p = serde_json::Map::new();
    p.insert("deduped_read".into(), Value::Bool(true));
    p.insert(
        "note".into(),
        Value::String(DEDUPED_FILE_READ_NOTE.to_string()),
    );

    fn maybe_str(p: &mut serde_json::Map<String, Value>, payload: &Value, key: &str) {
        if let Some(s) = payload
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            p.insert(key.into(), Value::String(s.to_string()));
        }
    }

    maybe_str(&mut p, payload, "file_path");
    maybe_str(&mut p, payload, "path");
    if let Some(sl) = key.start_line {
        p.insert("start_line".into(), json!(sl));
    }
    if let Some(el) = key.end_line {
        p.insert("end_line".into(), json!(el));
    }
    if let Some(sp) = key.spool_path.as_deref() {
        p.insert("spool_path".into(), json!(sp));
    }
    Value::Object(p).to_string()
}

fn file_read_dedup_candidate(
    message: &Message,
    tool_kinds: &HashMap<String, FileReadToolKind>,
) -> Option<FileReadDedupCandidate> {
    if message.role != MessageRole::Tool {
        return None;
    }

    let kind = message
        .tool_call_id
        .as_deref()
        .and_then(|tool_call_id| tool_kinds.get(tool_call_id).copied())
        .or_else(|| {
            message.origin_tool.as_deref().and_then(|tool_name| {
                is_read_file_tool_name(tool_name).then_some(FileReadToolKind::ReadFile)
            })
        })?;

    if !matches!(
        kind,
        FileReadToolKind::ReadFile | FileReadToolKind::UnifiedFileRead
    ) {
        return None;
    }

    let payload: Value = serde_json::from_str(message.content.as_text().as_ref()).ok()?;
    let key = build_file_read_dedup_key(&payload)?;

    Some(FileReadDedupCandidate {
        placeholder_content: build_file_read_placeholder_content(&payload, &key),
        key,
    })
}

pub(super) fn dedup_repeated_file_reads_for_local_compaction(history: &[Message]) -> Vec<Message> {
    let tool_kinds = collect_file_read_tool_kinds(history);
    let mut last_idx = HashMap::new();
    let mut candidates = Vec::new();
    for (i, msg) in history.iter().enumerate() {
        let Some(c) = file_read_dedup_candidate(msg, &tool_kinds) else {
            continue;
        };
        last_idx.insert(c.key.clone(), i);
        candidates.push((i, c));
    }
    let mut deduped = history.to_vec();
    let mut changed = false;
    for (idx, c) in candidates {
        if last_idx.get(&c.key).copied() == Some(idx) {
            continue;
        }
        if let Some(msg) = deduped.get_mut(idx) {
            msg.content = c.placeholder_content.into();
            changed = true;
        }
    }
    if changed { deduped } else { history.to_vec() }
}
