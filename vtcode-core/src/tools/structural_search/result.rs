#![allow(unused_imports)]

#[allow(unused_imports)]
use super::*;

pub(super) fn build_query_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    matches: Vec<AstGrepMatch>,
) -> Value {
    let max_results = request.effective_max_results();
    let truncated = matches.len() > max_results;
    let normalized_matches = matches
        .into_iter()
        .take(max_results)
        .map(normalize_match)
        .collect::<Vec<_>>();

    let mut result = json!({
        "backend": "ast-grep",
        "path": display_path,
        "matches": normalized_matches,
        "truncated": truncated,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    result
}

pub(super) fn build_scan_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    config_display_path: &str,
    findings: Vec<AstGrepScanFinding>,
) -> Value {
    // Apply post-run severity filter when severities are specified.
    let filtered_findings = if let Some(severities) = request.effective_severities() {
        findings
            .into_iter()
            .filter(|f| {
                f.severity
                    .as_ref()
                    .map(|s| severities.contains(&s.as_str()))
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>()
    } else {
        findings
    };

    let max_results = request.effective_max_results();
    let truncated = filtered_findings.len() > max_results;
    let normalized_findings = filtered_findings
        .iter()
        .take(max_results)
        .map(normalize_scan_finding)
        .collect::<Vec<_>>();

    json!({
        "backend": "ast-grep",
        "workflow": "scan",
        "config_path": config_display_path,
        "path": display_path,
        "findings": normalized_findings,
        "summary": build_scan_summary(&filtered_findings, normalized_findings.len(), truncated),
        "truncated": truncated,
    })
}

pub(super) fn build_fragment_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    hint: String,
) -> Value {
    let next_action = "Retry `code_search` with `action='structural'` using a larger parseable pattern and `selector` when the real target is a subnode inside that pattern. Do not rerun the same fragment unchanged.";

    let mut result = json!({
        "backend": "ast-grep",
        "path": display_path,
        "matches": [],
        "truncated": false,
        "is_recoverable": true,
        "next_action": next_action,
        "hint": hint,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    result
}

pub(super) fn build_range_value(range: &AstGrepRange) -> Value {
    let mut range_object = json!({
        "start": {
            "line": range.start.line,
            "column": range.start.column,
        },
        "end": {
            "line": range.end.line,
            "column": range.end.column,
        },
    });
    if let Some(byte_offset) = &range.byte_offset {
        range_object["byteOffset"] = json!({
            "start": byte_offset.start,
            "end": byte_offset.end,
        });
    }
    range_object
}

pub(super) fn build_meta_var_value(var: &AstGrepMetaVar) -> Value {
    json!({
        "text": var.text,
        "range": build_range_value(&var.range),
    })
}

pub(super) fn build_meta_variables_value(meta_vars: &AstGrepMetaVariables) -> Value {
    let mut single = Map::new();
    for (key, var) in &meta_vars.single {
        single.insert(key.clone(), build_meta_var_value(var));
    }

    let mut multi = Map::new();
    for (key, vars) in &meta_vars.multi {
        let vars_json: Vec<Value> = vars.iter().map(build_meta_var_value).collect();
        multi.insert(key.clone(), Value::Array(vars_json));
    }

    let transformed = if meta_vars.transformed.is_empty() {
        Value::Object(Map::new())
    } else {
        json!(meta_vars.transformed)
    };

    json!({
        "single": Value::Object(single),
        "multi": Value::Object(multi),
        "transformed": transformed,
    })
}

pub(super) fn normalize_match(entry: AstGrepMatch) -> Value {
    let mut match_object = Map::new();
    match_object.insert("file".to_string(), Value::String(entry.file));
    match_object.insert("line_number".to_string(), json!(entry.range.start.line));
    match_object.insert("text".to_string(), Value::String(entry.text.clone()));
    match_object.insert(
        "lines".to_string(),
        Value::String(entry.lines.unwrap_or(entry.text)),
    );
    if let Some(language) = entry.language {
        match_object.insert("language".to_string(), Value::String(language));
    }
    match_object.insert("range".to_string(), build_range_value(&entry.range));
    if let Some(meta_vars) = entry.meta_variables {
        match_object.insert(
            "metaVariables".to_string(),
            build_meta_variables_value(&meta_vars),
        );
    }
    Value::Object(match_object)
}

pub(super) fn normalize_scan_finding(entry: &AstGrepScanFinding) -> Value {
    let mut finding_object = Map::new();
    finding_object.insert("file".to_string(), Value::String(entry.file.clone()));
    finding_object.insert("line_number".to_string(), json!(entry.range.start.line));
    finding_object.insert("text".to_string(), Value::String(entry.text.clone()));
    finding_object.insert(
        "lines".to_string(),
        Value::String(entry.lines.clone().unwrap_or_else(|| entry.text.clone())),
    );
    if let Some(language) = &entry.language {
        finding_object.insert("language".to_string(), Value::String(language.clone()));
    }
    finding_object.insert("range".to_string(), build_range_value(&entry.range));
    finding_object.insert("rule_id".to_string(), json!(entry.rule_id));
    finding_object.insert(
        "severity".to_string(),
        json!(entry.severity.map(|s| s.to_string())),
    );
    finding_object.insert("message".to_string(), json!(entry.message));
    finding_object.insert("note".to_string(), json!(entry.note));
    if let Some(metadata) = &entry.metadata {
        if let Some(url) = metadata.get("url").or_else(|| metadata.get("docs")) {
            finding_object.insert("url".to_string(), url.clone());
        }
        finding_object.insert("metadata".to_string(), metadata.clone());
    }
    if !entry.labels.is_empty() {
        let labels_json: Vec<Value> = entry
            .labels
            .iter()
            .map(|label| {
                let mut label_obj = Map::new();
                label_obj.insert("text".to_string(), Value::String(label.text.clone()));
                label_obj.insert("range".to_string(), build_range_value(&label.range));
                if let Some(source) = &label.source {
                    label_obj.insert("source".to_string(), Value::String(source.clone()));
                }
                Value::Object(label_obj)
            })
            .collect();
        finding_object.insert("labels".to_string(), Value::Array(labels_json));
    }
    Value::Object(finding_object)
}

pub(super) fn build_scan_summary(
    findings: &[AstGrepScanFinding],
    returned: usize,
    truncated: bool,
) -> Value {
    let mut by_severity = BTreeMap::new();
    let mut by_rule = BTreeMap::new();
    let mut has_error_findings = false;

    for finding in findings {
        let severity = finding
            .severity
            .as_ref()
            .map(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string();
        if severity == "error" {
            has_error_findings = true;
        }
        *by_severity.entry(severity).or_insert(0usize) += 1;

        let rule = finding.rule_id.as_deref().unwrap_or("unknown").to_string();
        *by_rule.entry(rule).or_insert(0usize) += 1;
    }

    json!({
        "total_findings": findings.len(),
        "returned_findings": returned,
        "truncated": truncated,
        "has_error_findings": has_error_findings,
        "by_severity": by_severity,
        "by_rule": by_rule,
    })
}

pub(super) fn build_test_result(
    config_display_path: &str,
    passed: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> Value {
    let stdout = truncate_auxiliary_output(&String::from_utf8_lossy(stdout));
    let stderr = truncate_auxiliary_output(&String::from_utf8_lossy(stderr));

    json!({
        "backend": "ast-grep",
        "workflow": "test",
        "config_path": config_display_path,
        "passed": passed,
        "stdout": stdout,
        "stderr": stderr,
        "summary": summarize_test_output(&stdout, &stderr, passed),
    })
}

pub(super) fn truncate_auxiliary_output(text: &str) -> String {
    let char_count = text.chars().count();
    if char_count <= MAX_AUXILIARY_OUTPUT_CHARS {
        return text.to_string();
    }

    let truncated = text
        .chars()
        .take(MAX_AUXILIARY_OUTPUT_CHARS)
        .collect::<String>();
    format!(
        "{truncated}\n...[truncated {} chars]",
        char_count - MAX_AUXILIARY_OUTPUT_CHARS
    )
}

pub(crate) fn stderr_or_stdout(stderr: &[u8], stdout: &[u8]) -> String {
    let stderr_text = String::from_utf8_lossy(stderr).trim().to_string();
    if !stderr_text.is_empty() {
        return stderr_text;
    }

    let stdout_text = String::from_utf8_lossy(stdout).trim().to_string();
    if !stdout_text.is_empty() {
        return stdout_text;
    }

    "ast-grep exited without output".to_string()
}
