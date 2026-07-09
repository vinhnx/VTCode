#![allow(unused_imports)]

#[allow(unused_imports)]
use super::*;

/// Extract a rule summary from a YAML file's content.
pub(super) fn extract_rule_summary(content: &str, path: &Path) -> Option<Value> {
    let mut id = None;
    let mut language = None;
    let mut severity = None;
    let mut message = None;
    let mut has_fix = false;
    let mut utils_list = Vec::new();
    let mut in_utils = false;
    let mut utils_indent = 0usize;

    for line in content.lines() {
        let trimmed = line.trim();
        // Track whether we are inside the top-level `utils:` section.
        // We stay in the utils section until we hit a new top-level key
        // (a line at the same indentation as `utils:` that contains a colon).
        if in_utils {
            let indent = line.len() - line.trim_start().len();
            if indent <= utils_indent && !trimmed.is_empty() && !trimmed.starts_with('#') {
                // Hit a new top-level key; exit the utils section.
                in_utils = false;
            } else if indent == utils_indent + 2
                && let Some(name) = trimmed.strip_suffix(':')
            {
                let name = name.trim();
                if !name.is_empty() && !name.contains(' ') && !name.starts_with('#') {
                    utils_list.push(name.to_string());
                }
            }
        }
        if trimmed == "utils:" && !in_utils {
            in_utils = true;
            utils_indent = line.len() - line.trim_start().len();
        }

        if trimmed.starts_with("id:") && id.is_none() {
            id = Some(trimmed.strip_prefix("id:")?.trim().to_string());
        } else if trimmed.starts_with("language:") && language.is_none() {
            language = Some(trimmed.strip_prefix("language:")?.trim().to_string());
        } else if trimmed.starts_with("severity:") && severity.is_none() {
            severity = Some(trimmed.strip_prefix("severity:")?.trim().to_string());
        } else if trimmed.starts_with("message:") && message.is_none() {
            message = Some(
                trimmed
                    .strip_prefix("message:")?
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string(),
            );
        } else if trimmed.starts_with("fix:") && !has_fix {
            has_fix = true;
        }
    }

    let id = id?;
    let file_name = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    let mut summary = json!({
        "id": id,
        "file": file_name,
    });
    if let Some(lang) = language {
        summary["language"] = json!(lang);
    }
    if let Some(sev) = severity {
        summary["severity"] = json!(sev);
    }
    if let Some(msg) = message {
        summary["message"] = json!(msg);
    }
    summary["has_fix"] = json!(has_fix);
    if !utils_list.is_empty() {
        summary["utils"] = json!(utils_list);
    }

    Some(summary)
}

pub(super) fn preflight_parseable_pattern(
    request: &StructuralSearchRequest,
) -> Result<Option<String>> {
    let Some(language) = request
        .lang
        .as_deref()
        .and_then(AstGrepLanguage::from_user_value)
    else {
        return Ok(None);
    };

    if !language.has_local_parser() {
        return Ok(None);
    }

    let pattern = request
        .pattern()
        .ok_or_else(|| anyhow!("pattern must be present after validation"))?;
    validate_metavariable_syntax(pattern)?;

    let (sanitized_pattern, contains_metavariables) = sanitize_pattern_for_tree_sitter(pattern);
    let tree = match parse_source(language, &sanitized_pattern) {
        Ok(tree) => tree,
        Err(_) if contains_metavariables => {
            return Ok(Some(fragment_pattern_hint(request, language)));
        }
        Err(detail) => {
            bail!(
                "{}",
                format_ast_grep_failure(
                    AstGrepFailureOrigin::Preflight,
                    "structural pattern preflight failed",
                    format!(
                        "pattern is not parseable as {} syntax ({detail})",
                        language.display_name()
                    ),
                )
            );
        }
    };

    if tree.root_node().has_error() {
        if contains_metavariables {
            return Ok(Some(fragment_pattern_hint(request, language)));
        }

        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Preflight,
                "structural pattern preflight failed",
                format!(
                    "pattern is not parseable as {} syntax",
                    language.display_name()
                ),
            )
        );
    }

    Ok(None)
}

pub(super) fn sanitize_pattern_for_tree_sitter(pattern: &str) -> (String, bool) {
    let mut contains_metavariables = false;
    let sanitized =
        AST_GREP_METAVARIABLE_RE.replace_all(pattern, |captures: &regex::Captures<'_>| {
            contains_metavariables = true;
            let matched = captures.get(0).map(|m| m.as_str()).unwrap_or("");
            // `$$$NAME` (multi-metavariable) and `$$NAME` (unnamed node)
            // both need multiple placeholders; `$NAME` (named node) gets one.
            if matched.starts_with("$$") {
                "placeholders"
            } else {
                "placeholder"
            }
        });

    (sanitized.into_owned(), contains_metavariables)
}

/// Validate metavariable syntax in an ast-grep pattern.
pub(super) fn validate_metavariable_syntax(pattern: &str) -> Result<()> {
    static AST_GREP_DOLLAR_TOKEN_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\$\$?\$?[A-Za-z0-9_]+").expect("ast-grep dollar token regex must compile")
    });

    for mat in AST_GREP_DOLLAR_TOKEN_RE.find_iter(pattern) {
        let token = mat.as_str();
        if !AST_GREP_VALID_METAVAR_RE.is_match(token) {
            if let Some(rest) = token.strip_prefix("$$$") {
                if rest.is_empty()
                    || !rest
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_uppercase() || c == '_')
                {
                    bail!(
                        "invalid metavariable `{token}`: multi-metavariable `$$$` must be followed by an uppercase name (e.g. `$$$ARGS`); got `{rest}`"
                    );
                }
            } else if token == "$" || token == "$$" {
                bail!(
                    "bare `{token}` is not a valid metavariable; use `$NAME` (named node), `$$NAME` (unnamed node), or `$$$NAME` (zero or more nodes)"
                );
            } else {
                let prefix = if token.starts_with("$$") { "$$" } else { "$" };
                let name = &token[prefix.len()..];
                if name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
                    bail!(
                        "invalid metavariable `{token}`: names must start with an uppercase letter                          or underscore; use `{prefix}{upper}` instead",
                        upper = name.to_ascii_uppercase()
                    );
                }
                bail!(
                    "invalid metavariable `{token}`: names must match `[A-Z_][A-Z0-9_]*` after the `$` or `$$` prefix"
                );
            }
        }
    }
    Ok(())
}

pub(super) fn reject_forbidden_args(args: &Value) -> Result<()> {
    let Some(object) = args.as_object() else {
        return Ok(());
    };

    for key in STRUCTURAL_FORBIDDEN_KEYS {
        if has_argument_key(object, key) {
            bail!(
                "action='structural' is read-only; remove `{key}`. For `sg scan`, `sg test`, `sg new`, `sgconfig.yml`, or rewrite-oriented ast-grep tasks, load the bundled `ast-grep` skill first and use `exec_command.cmd` only when the public structural surface cannot express the needed CLI flow."
            );
        }
    }

    Ok(())
}

pub(super) fn has_argument_key(object: &Map<String, Value>, key: &str) -> bool {
    object.get(key).is_some()
        || key
            .contains('_')
            .then(|| key.replace('_', "-"))
            .as_ref()
            .is_some_and(|hyphenated| object.get(hyphenated).is_some())
}

pub(super) fn parse_compact_matches(stdout: &[u8]) -> Result<Vec<AstGrepMatch>> {
    serde_json::from_slice(stdout).context("failed to parse ast-grep JSON output")
}

pub(super) fn parse_stream_findings(stdout: &[u8]) -> Result<Vec<AstGrepScanFinding>> {
    let stdout = String::from_utf8_lossy(stdout);
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line).with_context(|| {
                format!("failed to parse ast-grep JSON stream output line: {line}")
            })
        })
        .collect()
}

/// Parse a simple YAML key-value pair like `hostLanguage: js` or `libraryPath: graphql.so`.
/// Returns (key, Value) where key is the trimmed left side and Value is the trimmed right side.
pub(super) fn parse_yaml_simple_kv(input: &str) -> Option<(String, Value)> {
    let colon_pos = input.find(':')?;
    let key = input[..colon_pos].trim().to_string();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    let raw_value = input[colon_pos + 1..].trim();

    // Skip keys with empty values -- these are typically parent keys
    // with nested content (e.g. `rule:` followed by indented sub-keys).
    if raw_value.is_empty() {
        return None;
    }

    // Handle inline array: extensions: [graphql]
    if raw_value.starts_with('[') && raw_value.ends_with(']') {
        let inner = &raw_value[1..raw_value.len() - 1];
        let items: Vec<Value> = inner
            .split(',')
            .map(|s| Value::String(s.trim().trim_matches('"').trim_matches('\'').to_string()))
            .filter(|v| !v.as_str().is_some_and(str::is_empty))
            .collect();
        return Some((key, Value::Array(items)));
    }

    let value = raw_value.trim_matches('"').trim_matches('\'');
    Some((key, Value::String(value.to_string())))
}

pub(super) fn summarize_test_output(stdout: &str, stderr: &str, passed: bool) -> Value {
    let clean_stdout = ANSI_ESCAPE_RE.replace_all(stdout, "");
    let clean_stderr = ANSI_ESCAPE_RE.replace_all(stderr, "");
    let mut summary = Map::new();
    summary.insert(
        "status".to_string(),
        Value::String(if passed { "ok" } else { "failed" }.to_string()),
    );

    if let Some(captures) = AST_GREP_TEST_RESULT_RE.captures(&clean_stdout) {
        let passed_cases = captures
            .get(2)
            .and_then(|value| value.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        let failed_cases = captures
            .get(3)
            .and_then(|value| value.as_str().parse::<usize>().ok())
            .unwrap_or(0);
        summary.insert("passed_cases".to_string(), json!(passed_cases));
        summary.insert("failed_cases".to_string(), json!(failed_cases));
        summary.insert(
            "total_cases".to_string(),
            json!(passed_cases + failed_cases),
        );
    }

    let rules = parse_test_rule_results(&clean_stdout);
    if !rules.is_empty() {
        summary.insert(
            "rules".to_string(),
            Value::Array(
                rules
                    .iter()
                    .map(|r| {
                        json!({
                            "rule_id": r.rule_id,
                            "passed": r.passed,
                            "markers": r.markers,
                        })
                    })
                    .collect(),
            ),
        );
    }

    let failure_details = parse_test_failure_details(&clean_stdout, &clean_stderr);
    if !failure_details.is_empty() {
        summary.insert("failure_details".to_string(), Value::Array(failure_details));
    }

    Value::Object(summary)
}

/// Per-rule result parsed from `sg test` stdout.
pub(super) struct TestRuleResult {
    pub(super) rule_id: String,
    pub(super) passed: bool,
    /// N = Noisy (false positive), M = Missing (false negative).
    pub(super) markers: Vec<String>,
}

/// Parse per-rule PASS/FAIL lines from `sg test` stdout.
///
/// The `sg test` output format is:
/// ```text
/// PASS rule-id .....
/// FAIL rule-id ...N..M
/// ```
/// where dots represent individual test cases and N/M markers indicate
/// Noisy or Missing failures within that rule.
pub(super) fn parse_test_rule_results(stdout: &str) -> Vec<TestRuleResult> {
    let mut results = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(caps) = AST_GREP_TEST_RULE_LINE_RE.captures(trimmed) {
            let status = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let rule_id = caps
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let trailing = caps.get(3).map(|m| m.as_str()).unwrap_or("");
            let markers: Vec<String> = trailing
                .chars()
                .filter_map(|c| match c {
                    'N' => Some("noisy".to_string()),
                    'M' => Some("missing".to_string()),
                    _ => None,
                })
                .collect();
            results.push(TestRuleResult {
                rule_id,
                passed: status == "PASS",
                markers,
            });
        }
    }
    results
}

/// Parse failure detail blocks from `sg test` output.
///
/// The `sg test` output on failure contains blocks like:
/// ```text
/// ----------- Failure Details -----------
/// [Noisy] Expect rule-id to report no issue, but some issues found in:
///
///   <code snippet>
///
/// [Missing] Expect rule rule-id to report issues, but none found in:
///
///   <code snippet>
/// ```
pub(super) fn parse_test_failure_details(stdout: &str, stderr: &str) -> Vec<Value> {
    let combined = format!("{stdout}\n{stderr}");
    let mut details = Vec::new();
    let mut lines = combined.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();

        if let Some(caps) = AST_GREP_TEST_NOISY_RE.captures(trimmed) {
            let rule_id = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let code_snippet = extract_failure_snippet(&mut lines);
            details.push(json!({
                "type": "noisy",
                "rule_id": rule_id,
                "code_snippet": code_snippet,
            }));
        } else if let Some(caps) = AST_GREP_TEST_MISSING_RE.captures(trimmed) {
            let rule_id = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let code_snippet = extract_failure_snippet(&mut lines);
            details.push(json!({
                "type": "missing",
                "rule_id": rule_id,
                "code_snippet": code_snippet,
            }));
        }
    }
    details
}

/// Extract the indented code snippet following a failure detail header.
/// Reads lines until a blank line or another `[`-prefixed header.
pub(super) fn extract_failure_snippet<'a>(
    lines: &mut std::iter::Peekable<std::str::Lines<'a>>,
) -> String {
    let mut snippet_lines = Vec::new();
    // Skip blank lines between the header and the code snippet.
    while let Some(peeked) = lines.peek() {
        if peeked.trim().is_empty() {
            lines.next();
        } else {
            break;
        }
    }
    // Collect indented code lines.
    while let Some(peeked) = lines.peek() {
        let trimmed = peeked.trim();
        if trimmed.is_empty() || trimmed.starts_with('[') {
            break;
        }
        snippet_lines.push(
            lines
                .next()
                .expect("peek() guaranteed next() is Some")
                .to_string(),
        );
    }
    snippet_lines.join("\n")
}
