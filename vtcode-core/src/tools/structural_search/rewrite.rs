#![allow(unused_imports)]

#[allow(unused_imports)]
use super::*;

/// Build rewrite-style result from scan findings. Converts scan findings
/// into the same shape as rewrite results so callers get a consistent
/// response format regardless of the internal path taken.
pub(super) fn build_fixconfig_rewrite_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    findings: Vec<AstGrepScanFinding>,
) -> Value {
    let max_results = request.effective_max_results();
    let truncated = findings.len() > max_results;
    let template = request
        .effective_rewrite_template()
        .unwrap_or_default()
        .to_string();

    let normalized_rewrites: Vec<Value> = findings
        .into_iter()
        .take(max_results)
        .map(|finding| {
            let mut rewrite_object = Map::new();
            rewrite_object.insert("file".to_string(), Value::String(finding.file));
            rewrite_object.insert("line_number".to_string(), json!(finding.range.start.line));
            rewrite_object.insert("text".to_string(), Value::String(finding.text.clone()));
            rewrite_object.insert(
                "lines".to_string(),
                Value::String(finding.lines.unwrap_or(finding.text)),
            );
            if let Some(language) = finding.language {
                rewrite_object.insert("language".to_string(), Value::String(language));
            }
            rewrite_object.insert("range".to_string(), build_range_value(&finding.range));
            // The replacement is the FixConfig template. The actual expanded
            // replacement would require applying the rule in-process, which
            // ast-grep handles externally. We include the template as the
            // intended replacement and note the expansion config.
            rewrite_object.insert("replacement".to_string(), Value::String(template.clone()));
            if let Some(message) = finding.message {
                rewrite_object.insert("message".to_string(), Value::String(message));
            }
            Value::Object(rewrite_object)
        })
        .collect();

    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "rewrite",
        "path": display_path,
        "rewrites": normalized_rewrites,
        "truncated": truncated,
        "fix_config": {
            "template": request.fix_config.as_ref().map(|fc| fc.template.clone()),
            "expand_start": request.fix_config.as_ref().and_then(|fc| fc.expand_start.as_ref().map(|es| es.to_yaml_value())),
            "expand_end": request.fix_config.as_ref().and_then(|fc| fc.expand_end.as_ref().map(|ee| ee.to_yaml_value())),
        },
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(template) = request.effective_rewrite_template() {
        result["rewrite"] = json!(template);
    }
    result
}

pub(super) fn build_rewrite_fragment_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    hint: String,
) -> Value {
    let next_action = "Retry `code_search` with `action='structural'` using a larger parseable pattern and `selector` when the real target is a subnode inside that pattern. Do not rerun the same fragment unchanged.";

    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "rewrite",
        "path": display_path,
        "rewrites": [],
        "truncated": false,
        "is_recoverable": true,
        "next_action": next_action,
        "hint": hint,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(rewrite_text) = request.rewrite_text() {
        result["rewrite"] = json!(rewrite_text);
    }
    result
}

pub(super) fn parse_rewrite_matches(stdout: &[u8]) -> Result<Vec<AstGrepRewriteMatch>> {
    serde_json::from_slice(stdout).context("failed to parse ast-grep rewrite JSON output")
}

pub(super) fn build_rewrite_result(
    request: &StructuralSearchRequest,
    display_path: &str,
    rewrites: Vec<AstGrepRewriteMatch>,
) -> Value {
    let max_results = request.effective_max_results();
    let truncated = rewrites.len() > max_results;
    let normalized_rewrites = rewrites
        .into_iter()
        .take(max_results)
        .map(normalize_rewrite_match)
        .collect::<Vec<_>>();

    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "rewrite",
        "path": display_path,
        "rewrites": normalized_rewrites,
        "truncated": truncated,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(rewrite_text) = request.rewrite_text() {
        result["rewrite"] = json!(rewrite_text);
    }
    result
}

pub(super) fn normalize_rewrite_match(entry: AstGrepRewriteMatch) -> Value {
    let mut rewrite_object = Map::new();
    rewrite_object.insert("file".to_string(), Value::String(entry.file));
    rewrite_object.insert("line_number".to_string(), json!(entry.range.start.line));
    rewrite_object.insert("text".to_string(), Value::String(entry.text.clone()));
    rewrite_object.insert(
        "lines".to_string(),
        Value::String(entry.lines.unwrap_or(entry.text)),
    );
    if let Some(language) = entry.language {
        rewrite_object.insert("language".to_string(), Value::String(language));
    }
    rewrite_object.insert("range".to_string(), build_range_value(&entry.range));
    if let Some(replacement) = entry.replacement {
        rewrite_object.insert("replacement".to_string(), Value::String(replacement));
    }
    if let Some(offsets) = entry.replacement_offsets {
        rewrite_object.insert(
            "replacementOffsets".to_string(),
            json!({ "start": offsets.start, "end": offsets.end }),
        );
    }
    if let Some(meta_vars) = entry.meta_variables {
        rewrite_object.insert(
            "metaVariables".to_string(),
            build_meta_variables_value(&meta_vars),
        );
    }
    Value::Object(rewrite_object)
}
