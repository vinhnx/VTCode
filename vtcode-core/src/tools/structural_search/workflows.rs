#![allow(unused_imports)]

#[allow(unused_imports)]
use super::*;

pub(super) async fn execute_structural_query(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path()).await?;
    let globs = request.normalized_globs();
    if request.pattern().is_some()
        && let Some(hint) = preflight_parseable_pattern(request)?
    {
        return Ok(build_fragment_result(
            request,
            &search_path.display_path,
            hint,
        ));
    }
    let command_path = search_path.command_arg.clone();

    if let Some(debug_query) = &request.debug_query {
        let pattern = request
            .pattern()
            .ok_or_else(|| anyhow!("pattern is required for debug query"))?;
        let lang = request
            .lang
            .as_deref()
            .filter(|l| !l.trim().is_empty())
            .ok_or_else(|| anyhow!("lang is required for debug query"))?;
        let mut command = ast_grep_command(ast_grep, workspace_root, "run");
        command
            .arg(format!("--pattern={pattern}"))
            .arg("--lang")
            .arg(lang)
            .arg(format!("--debug-query={}", debug_query.as_str()))
            .arg(&command_path);

        let output =
            run_ast_grep_command(&mut command, "failed to run ast-grep debug query").await?;

        if !output.status.success() {
            bail!(
                "{}",
                format_ast_grep_failure(
                    AstGrepFailureOrigin::Search,
                    "ast-grep debug query failed",
                    stderr_or_stdout(&output.stderr, &output.stdout)
                )
            );
        }

        return Ok(build_debug_query_result(
            request,
            &search_path.display_path,
            debug_query,
            &output.stdout,
        ));
    }

    // When nth_child, range, relational rules, composite rules, transforms,
    // or constraints are present, use YAML rule generation because these
    // operators cannot be expressed via CLI flags.
    if request.nth_child.is_some()
        || request.range.is_some()
        || request.has.is_some()
        || request.inside.is_some()
        || request.follows.is_some()
        || request.precedes.is_some()
        || request.constraints.is_some()
        || request.matches.is_some()
        || request.all.is_some()
        || request.any.is_some()
        || request.not.is_some()
        || request.utils.is_some()
        || request.transform.is_some()
    {
        return execute_atomic_rule_query(workspace_root, request, ast_grep, &search_path).await;
    }

    let mut command = ast_grep_command(ast_grep, workspace_root, "run");
    if let Some(pattern) = request.pattern() {
        command.arg(format!("--pattern={pattern}"));
    }
    command.arg("--json=compact").arg("--color=never");

    if let Some(kind) = request.kind() {
        command.arg("--kind").arg(kind);
    }
    if let Some(regex) = request.regex_pattern() {
        command.arg("--regex").arg(regex);
    }
    apply_common_run_flags(&mut command, request, &globs);
    command.arg(&command_path);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural search").await?;

    let no_matches = output.status.code() == Some(AST_GREP_NO_MATCHES_EXIT);
    if !output.status.success() && !no_matches {
        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Search,
                "ast-grep structural search failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let matches = if no_matches && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
        Vec::new()
    } else {
        parse_compact_matches(&output.stdout)?
    };
    Ok(build_query_result(
        request,
        &search_path.display_path,
        matches,
    ))
}

pub(super) async fn execute_structural_scan(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path()).await?;
    let config_path =
        resolve_config_path(workspace_root, request.requested_config_path(), true).await?;
    let globs = request.normalized_globs();

    // When --format is set to a CI pipeline format (github/sarif), we skip
    // --json and --include-metadata because the output format changes and we
    // return raw output instead. For "files_with_matches" and "count", we still
    // use --json=stream to get structured data, then post-process.
    let format_value = request.effective_format();
    let use_ci_format = matches!(format_value, Some("github" | "sarif"));
    let use_files_with_matches = format_value == Some("files_with_matches");
    let use_count = format_value == Some("count");

    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&config_path.command_arg)
        .arg("--color=never");

    if use_ci_format {
        command.arg(format!(
            "--format={}",
            format_value
                .ok_or_else(|| anyhow!("format_value must be Some when use_ci_format is true"))?
        ));
    } else {
        command.arg("--json=stream").arg("--include-metadata");
    }

    if let Some(filter) = request.filter() {
        command.arg("--filter").arg(filter);
    }

    // --no-ignore flags.
    if let Some(no_ignore) = request.effective_no_ignore() {
        for value in no_ignore {
            command.arg("--no-ignore").arg(value.trim());
        }
    }

    // --follow flag.
    if request.effective_follow() {
        command.arg("--follow");
    }

    // --threads flag.
    if let Some(threads) = request.effective_threads() {
        command.arg("--threads").arg(threads.to_string());
    }

    // --report-style flag.
    if let Some(style) = request.effective_report_style() {
        command.arg(format!("--report-style={style}"));
    }

    // Built-in rules as severity override flags (e.g. --error=unused-suppression).
    if let Some(builtin_rules) = request.effective_builtin_rules() {
        for rule_entry in builtin_rules {
            let (rule_name, severity) = match rule_entry.split_once(':') {
                Some((name, sev)) => (name.trim(), sev.trim()),
                None => (rule_entry.trim(), ""),
            };
            if severity.is_empty() {
                // Activate at built-in default severity.
                command.arg(format!("--hint={rule_name}"));
            } else {
                command.arg(format!("--{severity}={rule_name}"));
            }
        }
    }

    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    command.arg(&search_path.command_arg);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural scan").await?;

    let findings_with_error_exit = output.status.code() == Some(AST_GREP_NO_MATCHES_EXIT);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Search,
                "ast-grep structural scan failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    // When --format is set to a CI pipeline format, return the raw formatted
    // output instead of parsing as JSON stream.
    if use_ci_format {
        let raw = String::from_utf8_lossy(&output.stdout);
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "scan",
            "config_path": config_path.display_path,
            "path": search_path.display_path,
            "format": request.effective_format(),
            "output": truncate_auxiliary_output(&raw),
            "exit_code": output.status.code(),
        }));
    }

    let findings =
        if findings_with_error_exit && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            Vec::new()
        } else {
            parse_stream_findings(&output.stdout)?
        };

    // For "files_with_matches" format, return only unique file paths.
    if use_files_with_matches {
        let mut files: Vec<String> = findings.iter().map(|f| f.file.clone()).collect();
        files.sort();
        files.dedup();
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "scan",
            "config_path": config_path.display_path,
            "path": search_path.display_path,
            "format": "files_with_matches",
            "files": files,
            "count": files.len(),
        }));
    }

    // For "count" format, return match counts per file.
    if use_count {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for finding in &findings {
            *counts.entry(finding.file.clone()).or_insert(0) += 1;
        }
        let total: usize = counts.values().sum();
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "scan",
            "config_path": config_path.display_path,
            "path": search_path.display_path,
            "format": "count",
            "counts": counts,
            "total": total,
        }));
    }

    Ok(build_scan_result(
        request,
        &search_path.display_path,
        &config_path.display_path,
        findings,
    ))
}

pub(super) async fn execute_structural_test(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let config_path =
        resolve_config_path(workspace_root, request.requested_config_path(), true).await?;

    let mut command = ast_grep_command(ast_grep, workspace_root, "test");
    command.arg("--config").arg(&config_path.command_arg);

    if let Some(filter) = request.filter() {
        command.arg("--filter").arg(filter);
    }
    if request.skip_snapshot_tests == Some(true) {
        command.arg("--skip-snapshot-tests");
    }
    if request.update_all == Some(true) {
        command.arg("--update-all");
    }
    if request.interactive == Some(true) {
        command.arg("--interactive");
    }
    if let Some(test_dir) = request.test_dir.as_deref().filter(|s| !s.trim().is_empty()) {
        command.arg("--test-dir").arg(test_dir);
    }
    if let Some(snapshot_dir) = request
        .snapshot_dir
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        command.arg("--snapshot-dir").arg(snapshot_dir);
    }
    if request.include_off == Some(true) {
        command.arg("--include-off");
    }

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural test").await?;

    Ok(build_test_result(
        &config_path.display_path,
        output.status.success(),
        &output.stdout,
        &output.stderr,
    ))
}

pub(super) async fn execute_structural_rewrite(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path()).await?;
    let globs = request.normalized_globs();

    if let Some(hint) = preflight_parseable_pattern(request)? {
        return Ok(build_rewrite_fragment_result(
            request,
            &search_path.display_path,
            hint,
        ));
    }

    // When FixConfig with expansion or transform is present, use the YAML
    // rule path because `sg run --rewrite` only supports simple string fixes.
    if request.needs_yaml_rewrite() {
        return execute_fixconfig_rewrite(workspace_root, request, ast_grep, &search_path).await;
    }

    // Simple string rewrite via `sg run --rewrite`.
    let command_path = search_path.command_arg.clone();
    let pattern = request
        .pattern()
        .ok_or_else(|| anyhow!("pattern is required for rewrite"))?;
    let template = request
        .effective_rewrite_template()
        .ok_or_else(|| anyhow!("rewrite template is required"))?;
    let mut command = ast_grep_command(ast_grep, workspace_root, "run");
    command
        .arg(format!("--pattern={pattern}"))
        .arg(format!("--rewrite={template}"))
        .arg("--json=compact")
        .arg("--color=never");

    apply_common_run_flags(&mut command, request, &globs);
    command.arg(&command_path);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural rewrite").await?;

    let no_matches = output.status.code() == Some(AST_GREP_NO_MATCHES_EXIT);
    if !output.status.success() && !no_matches {
        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Rewrite,
                "ast-grep structural rewrite failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let rewrites = if no_matches && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
        Vec::new()
    } else {
        parse_rewrite_matches(&output.stdout)?
    };
    Ok(build_rewrite_result(
        request,
        &search_path.display_path,
        rewrites,
    ))
}

pub(super) async fn execute_structural_count(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path()).await?;
    let globs = request.normalized_globs();

    // When nthChild, range, relational rules, composite rules, transforms, or
    // constraints are present, use YAML rule generation and count scan findings.
    if request.nth_child.is_some()
        || request.range.is_some()
        || request.has.is_some()
        || request.inside.is_some()
        || request.follows.is_some()
        || request.precedes.is_some()
        || request.constraints.is_some()
        || request.matches.is_some()
        || request.all.is_some()
        || request.any.is_some()
        || request.not.is_some()
        || request.utils.is_some()
        || request.transform.is_some()
    {
        return execute_atomic_rule_count(workspace_root, request, ast_grep, &search_path).await;
    }

    let command_path = search_path.command_arg.clone();
    let mut command = ast_grep_command(ast_grep, workspace_root, "run");
    if let Some(pattern) = request.pattern() {
        command.arg(format!("--pattern={pattern}"));
    }
    command.arg("--json=compact").arg("--color=never");

    if let Some(kind) = request.kind() {
        command.arg("--kind").arg(kind);
    }
    if let Some(regex) = request.regex_pattern() {
        command.arg("--regex").arg(regex);
    }
    apply_common_run_flags(&mut command, request, &globs);
    command.arg(&command_path);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep structural count").await?;

    let no_matches = output.status.code() == Some(AST_GREP_NO_MATCHES_EXIT);
    if !output.status.success() && !no_matches {
        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Search,
                "ast-grep structural count failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let count = if no_matches && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
        0
    } else {
        parse_compact_matches(&output.stdout)?.len()
    };

    // The count workflow always returns the full match count -- there is no
    // truncation, so we do not emit a misleading `truncated` field.
    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "count",
        "path": search_path.display_path,
        "count": count,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    Ok(result)
}

/// Build a YAML rule string for an atomic count query.
pub(super) async fn execute_atomic_rule_count(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
    search_path: &ResolvedSearchPath,
) -> Result<Value> {
    let lang = request
        .lang
        .as_deref()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("javascript");

    let rule_yaml = build_atomic_rule_yaml(request, lang);

    let temp_dir = tempfile::tempdir().with_context(|| {
        "failed to create temporary directory for atomic rule count".to_string()
    })?;
    let rules_dir = temp_dir.path().join("rules");
    afs::create_dir_all(&rules_dir).await.with_context(|| {
        format!(
            "failed to create rules directory at {}",
            rules_dir.display()
        )
    })?;

    let rule_path = rules_dir.join("atomic-count.yml");
    afs::write(&rule_path, &rule_yaml)
        .await
        .with_context(|| format!("failed to write atomic rule to {}", rule_path.display()))?;

    let sgconfig_path = temp_dir.path().join("sgconfig.yml");
    let sgconfig_content = format!("ruleDirs:\n  - {}\n", rules_dir.display());
    afs::write(&sgconfig_path, &sgconfig_content)
        .await
        .with_context(|| {
            format!(
                "failed to write sgconfig.yml to {}",
                sgconfig_path.display()
            )
        })?;

    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&sgconfig_path)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    let globs = request.normalized_globs();
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    command.arg(&search_path.command_arg);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep atomic rule count").await?;

    let findings_with_error_exit = output.status.code() == Some(AST_GREP_NO_MATCHES_EXIT);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Search,
                "ast-grep atomic rule count failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let findings =
        if findings_with_error_exit && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            Vec::new()
        } else {
            parse_stream_findings(&output.stdout)?
        };

    let count = findings.len();
    let max_results = request.effective_max_results();
    let truncated = count > max_results;

    let mut result = json!({
        "backend": "ast-grep",
        "workflow": "count",
        "path": search_path.display_path,
        "count": count,
        "truncated": truncated,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    Ok(result)
}

/// Execute a query via YAML rule generation when relational rules
/// or constraints are present.
pub(super) async fn execute_atomic_rule_query(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
    search_path: &ResolvedSearchPath,
) -> Result<Value> {
    let lang = request
        .lang
        .as_deref()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("javascript");

    let rule_yaml = build_atomic_rule_yaml(request, lang);

    let temp_dir = tempfile::tempdir().with_context(|| {
        "failed to create temporary directory for atomic rule query".to_string()
    })?;
    let rules_dir = temp_dir.path().join("rules");
    afs::create_dir_all(&rules_dir).await.with_context(|| {
        format!(
            "failed to create rules directory at {}",
            rules_dir.display()
        )
    })?;

    let rule_path = rules_dir.join("atomic-query.yml");
    afs::write(&rule_path, &rule_yaml)
        .await
        .with_context(|| format!("failed to write atomic rule to {}", rule_path.display()))?;

    let sgconfig_path = temp_dir.path().join("sgconfig.yml");
    let sgconfig_content = format!("ruleDirs:\n  - {}\n", rules_dir.display());
    afs::write(&sgconfig_path, &sgconfig_content)
        .await
        .with_context(|| {
            format!(
                "failed to write sgconfig.yml to {}",
                sgconfig_path.display()
            )
        })?;

    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&sgconfig_path)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    let globs = request.normalized_globs();
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    command.arg(&search_path.command_arg);

    let output =
        run_ast_grep_command(&mut command, "failed to run ast-grep atomic rule query").await?;

    let findings_with_error_exit = output.status.code() == Some(AST_GREP_NO_MATCHES_EXIT);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Search,
                "ast-grep atomic rule query failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let findings =
        if findings_with_error_exit && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            Vec::new()
        } else {
            parse_stream_findings(&output.stdout)?
        };

    let max_results = request.effective_max_results();
    let truncated = findings.len() > max_results;
    let normalized_matches = findings
        .into_iter()
        .take(max_results)
        .map(|finding| {
            let mut match_object = Map::new();
            match_object.insert("file".to_string(), Value::String(finding.file));
            match_object.insert("line_number".to_string(), json!(finding.range.start.line));
            match_object.insert("text".to_string(), Value::String(finding.text.clone()));
            match_object.insert(
                "lines".to_string(),
                Value::String(finding.lines.unwrap_or(finding.text)),
            );
            if let Some(language) = finding.language {
                match_object.insert("language".to_string(), Value::String(language));
            }
            match_object.insert("range".to_string(), build_range_value(&finding.range));
            if let Some(message) = finding.message {
                match_object.insert("message".to_string(), Value::String(message));
            }
            if let Some(metadata) = &finding.metadata {
                match_object.insert("metadata".to_string(), metadata.clone());
            }
            Value::Object(match_object)
        })
        .collect::<Vec<_>>();

    let mut result = json!({
        "backend": "ast-grep",
        "path": search_path.display_path,
        "matches": normalized_matches,
        "truncated": truncated,
    });
    if let Some(pattern) = request.pattern() {
        result["pattern"] = json!(pattern);
    }
    if let Some(kind) = request.kind() {
        result["kind"] = json!(kind);
    }
    Ok(result)
}

/// Execute a FixConfig rewrite and return raw scan findings as rewrite-like
/// matches. The `replacement` field is set to the template string and
/// `replacement_offsets` is `None` because scan findings do not include
/// byte-offset replacement data.
pub(super) async fn execute_fixconfig_rewrite_to_matches(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
    search_path: &ResolvedSearchPath,
) -> Result<Vec<AstGrepRewriteMatch>> {
    let fix_config = request
        .fix_config
        .as_ref()
        .ok_or_else(|| anyhow!("fix_config is required for fixconfig rewrite"))?;
    let pattern = request
        .pattern()
        .ok_or_else(|| anyhow!("pattern is required for fixconfig rewrite"))?;
    let lang = request
        .lang
        .as_deref()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("javascript");
    let template = request
        .effective_rewrite_template()
        .unwrap_or_default()
        .to_string();

    let rule_yaml = build_fixconfig_rule_yaml(
        pattern,
        lang,
        fix_config,
        request.selector.as_deref(),
        request.transform.as_ref(),
    );

    let temp_dir = tempfile::tempdir().with_context(|| {
        "failed to create temporary directory for FixConfig rewrite matches".to_string()
    })?;
    let rules_dir = temp_dir.path().join("rules");
    afs::create_dir_all(&rules_dir).await.with_context(|| {
        format!(
            "failed to create rules directory at {}",
            rules_dir.display()
        )
    })?;

    let rule_path = rules_dir.join("fixconfig-rewrite.yml");
    afs::write(&rule_path, &rule_yaml)
        .await
        .with_context(|| format!("failed to write FixConfig rule to {}", rule_path.display()))?;

    let sgconfig_path = temp_dir.path().join("sgconfig.yml");
    let sgconfig_content = format!("ruleDirs:\n  - {}\n", rules_dir.display());
    afs::write(&sgconfig_path, &sgconfig_content)
        .await
        .with_context(|| {
            format!(
                "failed to write sgconfig.yml to {}",
                sgconfig_path.display()
            )
        })?;

    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&sgconfig_path)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    let globs = request.normalized_globs();
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    command.arg(&search_path.command_arg);

    let output = run_ast_grep_command(
        &mut command,
        "failed to run ast-grep FixConfig rewrite scan for apply",
    )
    .await?;

    let findings_with_error_exit = output.status.code() == Some(AST_GREP_NO_MATCHES_EXIT);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Rewrite,
                "ast-grep FixConfig rewrite scan failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let findings =
        if findings_with_error_exit && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            Vec::new()
        } else {
            parse_stream_findings(&output.stdout)?
        };

    // Convert scan findings to rewrite-like matches. The replacement is the
    // template string; byte offsets are not available from scan findings.
    Ok(findings
        .into_iter()
        .map(|f| AstGrepRewriteMatch {
            file: f.file,
            text: f.text,
            lines: f.lines,
            language: f.language,
            range: f.range,
            meta_variables: None,
            replacement: Some(template.clone()),
            replacement_offsets: None,
        })
        .collect())
}

/// Execute a FixConfig rewrite by generating a temporary YAML rule with
/// `fix` as a `FixConfig` object (template + expandStart/expandEnd) and
/// running `sg scan` against it.
///
/// This is necessary because `sg run --rewrite` only supports simple
/// string fixes. FixConfig with expandStart/expandEnd requires the YAML
/// rule file path.
pub(super) async fn execute_fixconfig_rewrite(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
    search_path: &ResolvedSearchPath,
) -> Result<Value> {
    let fix_config = request
        .fix_config
        .as_ref()
        .ok_or_else(|| anyhow!("fix_config is required for fixconfig rewrite"))?;
    let pattern = request
        .pattern()
        .ok_or_else(|| anyhow!("pattern is required for fixconfig rewrite"))?;
    let lang = request
        .lang
        .as_deref()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or("javascript");

    // Build the YAML rule content.
    let rule_yaml = build_fixconfig_rule_yaml(
        pattern,
        lang,
        fix_config,
        request.selector.as_deref(),
        request.transform.as_ref(),
    );

    // Create a temporary directory with the rule file and sgconfig.yml.
    let temp_dir = tempfile::tempdir().with_context(|| {
        "failed to create temporary directory for FixConfig rewrite rule".to_string()
    })?;
    let rules_dir = temp_dir.path().join("rules");
    afs::create_dir_all(&rules_dir).await.with_context(|| {
        format!(
            "failed to create rules directory at {}",
            rules_dir.display()
        )
    })?;

    let rule_path = rules_dir.join("fixconfig-rewrite.yml");
    afs::write(&rule_path, &rule_yaml)
        .await
        .with_context(|| format!("failed to write FixConfig rule to {}", rule_path.display()))?;

    let sgconfig_path = temp_dir.path().join("sgconfig.yml");
    let sgconfig_content = format!("ruleDirs:\n  - {}\n", rules_dir.display());
    afs::write(&sgconfig_path, &sgconfig_content)
        .await
        .with_context(|| {
            format!(
                "failed to write sgconfig.yml to {}",
                sgconfig_path.display()
            )
        })?;

    // Run `sg scan` with the temporary config.
    let mut command = ast_grep_command(ast_grep, workspace_root, "scan");
    command
        .arg("--config")
        .arg(&sgconfig_path)
        .arg("--json=stream")
        .arg("--include-metadata")
        .arg("--color=never");

    let globs = request.normalized_globs();
    apply_context_and_globs(
        &mut command,
        request.context_lines,
        request.effective_before_lines(),
        request.effective_after_lines(),
        &globs,
    );
    command.arg(&search_path.command_arg);

    let output = run_ast_grep_command(
        &mut command,
        "failed to run ast-grep FixConfig rewrite scan",
    )
    .await?;

    let findings_with_error_exit = output.status.code() == Some(AST_GREP_NO_MATCHES_EXIT);
    if !output.status.success() && !findings_with_error_exit {
        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Rewrite,
                "ast-grep FixConfig rewrite scan failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    let findings =
        if findings_with_error_exit && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            Vec::new()
        } else {
            parse_stream_findings(&output.stdout)?
        };

    // Convert scan findings to rewrite-style output.
    Ok(build_fixconfig_rewrite_result(
        request,
        &search_path.display_path,
        findings,
    ))
}

/// Build a YAML rule string for a FixConfig rewrite.
///
/// The rule has:
/// - `id`: a descriptive identifier
/// - `language`: the target language
/// - `severity: info` (rewrite, not a lint warning)
/// - `rule.pattern` or `rule.pattern` + `rule.selector`
/// - `fix`: a FixConfig object with `template` and optional
///   `expandStart`/`expandEnd`
/// - `transform`: optional transform pipeline for meta-variable substitution
pub(super) async fn execute_structural_inspect(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
) -> Result<Value> {
    let requested_config = request.requested_config_path();
    let config_path = resolve_config_path(workspace_root, requested_config, false).await?;

    let resolved_full = if Path::new(&config_path.command_arg).is_absolute() {
        PathBuf::from(&config_path.command_arg)
    } else {
        workspace_root.join(&config_path.command_arg)
    };
    let config_exists = resolved_full.is_file();

    let rule_dir_hints = if config_exists {
        extract_rule_dirs(&resolved_full).await
    } else {
        Vec::new()
    };

    let language_injections = if config_exists {
        extract_language_injections(&resolved_full).await
    } else {
        Vec::new()
    };

    let custom_languages = if config_exists {
        extract_custom_languages(&resolved_full).await
    } else {
        Value::Object(Map::new())
    };

    let language_globs = if config_exists {
        extract_language_globs(&resolved_full).await
    } else {
        Value::Object(Map::new())
    };

    let test_configs = if config_exists {
        extract_test_configs(&resolved_full).await
    } else {
        Vec::new()
    };

    let util_dirs = if config_exists {
        extract_util_dirs(&resolved_full).await
    } else {
        Vec::new()
    };

    let discovered = if !config_exists {
        let is_default = requested_config == DEFAULT_AST_GREP_CONFIG_PATH;
        if is_default {
            match discover_project_config(workspace_root).await {
                Some(found) => {
                    let display = found
                        .strip_prefix(workspace_root)
                        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_else(|_| found.to_string_lossy().replace('\\', "/"));
                    vec![display]
                }
                None => Vec::new(),
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let search_path = resolve_search_path(workspace_root, request.requested_path()).await?;

    Ok(json!({
        "backend": "ast-grep",
        "workflow": "inspect",
        "project_dir": search_path.display_path,
        "config_path": config_path.display_path,
        "config_exists": config_exists,
        "rule_dir_hints": rule_dir_hints,
        "test_configs": test_configs,
        "util_dirs": util_dirs,
        "language_injections": language_injections,
        "custom_languages": custom_languages,
        "language_globs": language_globs,
        "discovered_configs": discovered,
    }))
}

pub(super) async fn execute_structural_rules(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
) -> Result<Value> {
    let requested_config = request.requested_config_path();
    let config_path = resolve_config_path(workspace_root, requested_config, false).await?;

    let resolved_full = if Path::new(&config_path.command_arg).is_absolute() {
        PathBuf::from(&config_path.command_arg)
    } else {
        workspace_root.join(&config_path.command_arg)
    };
    let config_exists = resolved_full.is_file();

    if !config_exists {
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "rules",
            "config_path": config_path.display_path,
            "config_exists": false,
            "rules": [],
        }));
    }

    let rule_dirs = extract_rule_dirs(&resolved_full).await;
    let config_parent = resolved_full.parent().unwrap_or(workspace_root);

    let mut rules = Vec::new();
    for dir in &rule_dirs {
        let dir_path = config_parent.join(dir);
        if !dir_path.is_dir() {
            continue;
        }
        collect_rules_from_dir(&dir_path, &mut rules).await;
    }

    Ok(json!({
        "backend": "ast-grep",
        "workflow": "rules",
        "config_path": config_path.display_path,
        "config_exists": true,
        "rule_dirs": rule_dirs,
        "rules": rules,
    }))
}

pub(super) async fn execute_structural_new(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let subcommand = request
        .new_subcommand
        .as_deref()
        .ok_or_else(|| anyhow!("new_subcommand must be present after validation"))?;

    let mut command = ast_grep_command(ast_grep, workspace_root, "new");
    command.arg(subcommand).arg("--yes");

    if let Some(name) = request.new_name.as_deref().filter(|s| !s.trim().is_empty()) {
        command.arg(name);
    }

    if let Some(lang) = request.lang.as_deref().filter(|s| !s.trim().is_empty()) {
        command.arg("--lang").arg(lang);
    }

    if let Some(config) = request
        .config_path
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        command.arg("--config").arg(config);
    }

    let output = run_ast_grep_command(&mut command, "failed to run ast-grep new").await?;

    if !output.status.success() {
        bail!(
            "{}",
            format_ast_grep_failure(
                AstGrepFailureOrigin::Search,
                "ast-grep new failed",
                stderr_or_stdout(&output.stderr, &output.stdout)
            )
        );
    }

    Ok(json!({
        "backend": "ast-grep",
        "workflow": "new",
        "subcommand": subcommand,
        "name": request.new_name,
        "output": String::from_utf8_lossy(&output.stdout).trim(),
    }))
}

pub(super) async fn execute_structural_apply(
    workspace_root: &Path,
    request: &StructuralSearchRequest,
    ast_grep: &Path,
) -> Result<Value> {
    let search_path = resolve_search_path(workspace_root, request.requested_path()).await?;
    let globs = request.normalized_globs();

    if let Some(hint) = preflight_parseable_pattern(request)? {
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "apply",
            "path": search_path.display_path,
            "files_modified": [],
            "total_replacements": 0,
            "is_recoverable": true,
            "hint": hint,
        }));
    }

    let rewrites: Vec<AstGrepRewriteMatch> = if request.needs_yaml_rewrite() {
        execute_fixconfig_rewrite_to_matches(workspace_root, request, ast_grep, &search_path)
            .await?
    } else {
        let pattern = request
            .pattern()
            .ok_or_else(|| anyhow!("pattern is required for apply"))?;
        let template = request
            .effective_rewrite_template()
            .ok_or_else(|| anyhow!("rewrite template is required for apply"))?;
        let command_path = search_path.command_arg.clone();
        let mut command = ast_grep_command(ast_grep, workspace_root, "run");
        command
            .arg(format!("--pattern={pattern}"))
            .arg(format!("--rewrite={template}"))
            .arg("--json=compact")
            .arg("--color=never");

        apply_common_run_flags(&mut command, request, &globs);
        command.arg(&command_path);

        let output =
            run_ast_grep_command(&mut command, "failed to run ast-grep structural apply").await?;

        let no_matches = output.status.code() == Some(AST_GREP_NO_MATCHES_EXIT);
        if !output.status.success() && !no_matches {
            bail!(
                "{}",
                format_ast_grep_failure(
                    AstGrepFailureOrigin::Rewrite,
                    "ast-grep structural apply failed",
                    stderr_or_stdout(&output.stderr, &output.stdout)
                )
            );
        }

        if no_matches && String::from_utf8_lossy(&output.stdout).trim().is_empty() {
            Vec::new()
        } else {
            parse_rewrite_matches(&output.stdout)?
        }
    };

    if rewrites.is_empty() {
        return Ok(json!({
            "backend": "ast-grep",
            "workflow": "apply",
            "path": search_path.display_path,
            "files_modified": [],
            "total_replacements": 0,
        }));
    }

    // Group rewrites by file.
    let mut by_file: BTreeMap<String, Vec<&AstGrepRewriteMatch>> = BTreeMap::new();
    for rw in &rewrites {
        by_file.entry(rw.file.clone()).or_default().push(rw);
    }

    let mut files_modified = Vec::new();
    let mut total_replacements = 0usize;

    for (file_path, file_rewrites) in &by_file {
        let abs_path = workspace_root.join(file_path);
        let content = afs::read_to_string(&abs_path)
            .await
            .with_context(|| format!("failed to read {file_path} for apply"))?;
        let mut bytes = content.into_bytes();

        // Sort by byte offset descending so we apply from end to start.
        let mut sorted: Vec<_> = file_rewrites.iter().collect();
        sorted.sort_by(|a, b| {
            let a_start = a.replacement_offsets.as_ref().map(|o| o.start).unwrap_or(0);
            let b_start = b.replacement_offsets.as_ref().map(|o| o.start).unwrap_or(0);
            b_start.cmp(&a_start)
        });

        let mut applied = 0usize;
        for rw in &sorted {
            let Some(replacement) = &rw.replacement else {
                continue;
            };
            let Some(offsets) = &rw.replacement_offsets else {
                continue;
            };
            if offsets.start > offsets.end || offsets.end > bytes.len() {
                continue;
            }
            let replacement_bytes = replacement.as_bytes();
            bytes.splice(
                offsets.start..offsets.end,
                replacement_bytes.iter().cloned(),
            );
            applied += 1;
        }

        if applied > 0 {
            afs::write(&abs_path, &bytes)
                .await
                .with_context(|| format!("failed to write {file_path}"))?;
            total_replacements += applied;
            files_modified.push(json!({
                "file": file_path,
                "replacements": applied,
            }));
        }
    }

    Ok(json!({
        "backend": "ast-grep",
        "workflow": "apply",
        "path": search_path.display_path,
        "files_modified": files_modified,
        "total_replacements": total_replacements,
    }))
}

/// Recursively collect rule summaries from YAML files in a directory.
pub(super) async fn collect_rules_from_dir(dir: &Path, rules: &mut Vec<Value>) {
    let mut entries = match afs::read_dir(dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            Box::pin(collect_rules_from_dir(&path, rules)).await;
            continue;
        }
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !matches!(ext, "yml" | "yaml") {
            continue;
        }
        let Ok(content) = afs::read_to_string(&path).await else {
            continue;
        };
        if let Some(summary) = extract_rule_summary(&content, &path) {
            rules.push(summary);
        }
    }
}
