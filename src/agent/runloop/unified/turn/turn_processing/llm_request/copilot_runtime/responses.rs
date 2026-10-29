use super::*;

pub(super) fn emit_terminal_output_event(
    emitter: Option<&HarnessEventEmitter>,
    harness_item_prefix: &str,
    tool_call_id: &str,
    tool_name: &str,
    output: &str,
) {
    let Some(_emitter) = emitter else { return };
    let item_id = harness_call_item_id(harness_item_prefix, tool_call_id, tool_name);
    let _ = _emitter.emit(tool_updated_event(item_id, raw_tool_call_id(tool_call_id), output));
}

pub(super) fn emit_terminal_finished_event(
    emitter: Option<&HarnessEventEmitter>,
    harness_item_prefix: &str,
    tool_call_id: &str,
    tool_name: &str,
    arguments: &Value,
    status: ToolCallStatus,
    output: String,
) {
    let Some(emitter) = emitter else { return };
    let item_id = harness_call_item_id(harness_item_prefix, tool_call_id, tool_name);
    let raw_id = raw_tool_call_id(tool_call_id);
    let _ = emitter.emit(tool_invocation_completed_event(
        item_id.clone(),
        tool_name,
        Some(arguments),
        raw_id,
        status.clone(),
    ));
    let _ = emitter.emit(tool_output_completed_event(item_id, raw_id, status, None, None, output));
}

pub(super) fn filter_copilot_tools(
    available_tools: Option<&Arc<Vec<ToolDefinition>>>,
    allowlist: &[String],
) -> Vec<ToolDefinition> {
    let allowlist: BTreeSet<&str> = allowlist.iter().map(String::as_str).collect();
    available_tools
        .into_iter()
        .flat_map(|tools| tools.iter())
        .filter(|tool| tool_definition_name(tool).is_some_and(|name| allowlist.contains(name)))
        .cloned()
        .collect()
}

pub(super) fn tool_definition_name(tool: &ToolDefinition) -> Option<&str> {
    tool.function.as_ref().map(|function| function.name.as_str())
}

pub(super) struct PermissionPromptSummary {
    pub(super) cache_key: String,
    pub(super) tool_name: String,
    pub(super) display_name: String,
    pub(super) learning_label: String,
    pub(super) tool_args: Option<Value>,
    pub(super) reason: Option<String>,
}

fn scoped_cache_key(prefix: &str, scope: Value) -> String {
    serde_json::to_string(&json!({
        "prefix": prefix,
        "scope": scope,
    }))
    .unwrap_or_else(|_| prefix.to_string())
}

pub(super) fn map_builtin_permission_prompt_decision(
    decision: HitlDecision,
    feedback: Option<String>,
) -> (CopilotPermissionDecision, bool) {
    match decision {
        HitlDecision::Approved | HitlDecision::Enable => (CopilotPermissionDecision::Approved, false),
        HitlDecision::ApprovedSession | HitlDecision::ApprovedPermanent => {
            (CopilotPermissionDecision::ApprovedAlways, true)
        }
        HitlDecision::Denied | HitlDecision::DeniedOnce | HitlDecision::Exit | HitlDecision::Interrupt => {
            (CopilotPermissionDecision::DeniedInteractivelyByUser { feedback }, false)
        }
    }
}

pub(super) fn auto_approve_builtin_permission(
    request: &CopilotPermissionRequest,
) -> Option<(CopilotPermissionDecision, bool)> {
    match request {
        // Copilot ACP keeps asking permission for these descriptive custom-tool
        // requests even though VT Code still controls the actual tool-call path.
        CopilotPermissionRequest::CustomTool { .. } => Some((CopilotPermissionDecision::ApprovedAlways, true)),
        _ => None,
    }
}

pub(super) fn denied_tool_response(tool_name: &str, reason: &str) -> CopilotToolCallResponse {
    CopilotToolCallResponse::Failure(CopilotToolCallFailure {
        text_result_for_llm: format!("VT Code denied the tool `{tool_name}`."),
        error: format!("tool '{tool_name}' {reason}"),
    })
}

pub(super) fn tool_not_exposed_response(tool_name: &str) -> CopilotToolCallResponse {
    denied_tool_response(tool_name, "is not allowlisted in VT Code")
}

pub(super) fn tool_exceeded_budget_response(tool_name: &str, max_tool_calls: usize) -> CopilotToolCallResponse {
    CopilotToolCallResponse::Failure(CopilotToolCallFailure {
        text_result_for_llm: format!(
            "VT Code denied the tool `{tool_name}` because the turn exceeded its tool-call budget."
        ),
        error: format!("tool '{tool_name}' exceeded max tool calls per turn ({max_tool_calls})"),
    })
}

pub(super) fn copilot_failure_response_with_diagnosis(
    tool_name: &str,
    evidence: &str,
    error: &str,
    diagnosis: &ToolFailureDiagnosis,
) -> CopilotToolCallResponse {
    let escaped_evidence = escape_untrusted_evidence(evidence);
    let failure_text =
        copilot_tool_result_text(tool_name, format!("VT Code failed to execute the tool `{tool_name}`."));
    CopilotToolCallResponse::Failure(CopilotToolCallFailure {
        text_result_for_llm: format!(
            "{failure_text}\n\n<untrusted_tool_evidence>\n{escaped_evidence}\n</untrusted_tool_evidence>\n\n{}",
            diagnosis.render_text(tool_name),
        ),
        error: bounded_diagnostic_field(error),
    })
}

pub(super) fn copilot_tool_result_text(tool_name: &str, text: String) -> String {
    if vtcode_core::tools::tool_intent::is_command_tool(tool_name) {
        format!("{text}\n\n{COLLAPSED_TOOL_OUTPUT_NOTICE}")
    } else {
        text
    }
}

pub(super) fn tool_cancelled_response(tool_name: &str) -> CopilotToolCallResponse {
    denied_tool_response(tool_name, "execution cancelled")
}

/// Copilot tool calls are answered directly through ACP rather than the normal
/// turn-processing context, so they cannot receive the shared system-message
/// flush. Append the same one-shot grant guidance to the tool result instead.
pub(super) fn append_session_limit_grant_guidance(
    response: CopilotToolCallResponse,
    grant_pending: bool,
) -> CopilotToolCallResponse {
    if !grant_pending {
        return response;
    }

    append_tool_reply_guidance(response, SESSION_LIMIT_GRANT_DIRECTIVE)
}

pub(super) fn append_tool_reply_guidance(response: CopilotToolCallResponse, guidance: &str) -> CopilotToolCallResponse {
    let append_guidance = |text: &mut String| {
        text.push_str("\n\n");
        text.push_str(guidance);
    };

    match response {
        CopilotToolCallResponse::Success(mut success) => {
            append_guidance(&mut success.text_result_for_llm);
            CopilotToolCallResponse::Success(success)
        }
        CopilotToolCallResponse::Failure(mut failure) => {
            append_guidance(&mut failure.text_result_for_llm);
            CopilotToolCallResponse::Failure(failure)
        }
    }
}

pub(super) fn summarize_permission_request(request: &CopilotPermissionRequest) -> Option<PermissionPromptSummary> {
    #[derive(Debug)]
    struct SummaryDef {
        prefix: &'static str,
        tool_name: String,
        display_name: String,
        cache_scope: Value,
        tool_args: Option<Value>,
        reason: Option<String>,
    }
    let def = match request {
        CopilotPermissionRequest::Shell {
            full_command_text,
            intention,
            possible_paths,
            possible_urls,
            has_write_file_redirection,
            warning,
            ..
        } => SummaryDef {
            prefix: "copilot:shell",
            tool_name: "copilot_shell".to_string(),
            display_name: "GitHub Copilot shell command".to_string(),
            cache_scope: json!({
                "command": full_command_text,
                "paths": possible_paths,
                "urls": possible_urls,
                "write_redirection": has_write_file_redirection,
            }),
            tool_args: Some(json!({
                "command": full_command_text,
                "paths": possible_paths,
                "urls": possible_urls,
            })),
            reason: warning.clone().or_else(|| Some(intention.clone())),
        },
        CopilotPermissionRequest::Write { file_name, intention, .. } => SummaryDef {
            prefix: "copilot:write",
            tool_name: "copilot_write".to_string(),
            display_name: "GitHub Copilot file write".to_string(),
            cache_scope: json!({"file": file_name}),
            tool_args: Some(json!({"file": file_name, "intention": intention})),
            reason: Some(intention.clone()),
        },
        CopilotPermissionRequest::Read { path, intention, .. } => SummaryDef {
            prefix: "copilot:read",
            tool_name: "copilot_read".to_string(),
            display_name: "GitHub Copilot file read".to_string(),
            cache_scope: json!({"path": path}),
            tool_args: Some(json!({"path": path, "intention": intention})),
            reason: Some(intention.clone()),
        },
        CopilotPermissionRequest::Mcp {
            server_name,
            tool_name,
            tool_title,
            args,
            read_only,
            ..
        } => SummaryDef {
            prefix: "copilot:mcp",
            tool_name: format!("copilot_mcp_{tool_name}"),
            display_name: format!("GitHub Copilot MCP tool {tool_title}"),
            cache_scope: json!({"server": server_name, "tool": tool_name, "args": args, "read_only": read_only}),
            tool_args: args.clone(),
            reason: Some(format!("Server: {server_name}")),
        },
        CopilotPermissionRequest::Url { url, intention, .. } => SummaryDef {
            prefix: "copilot:url",
            tool_name: "copilot_url".to_string(),
            display_name: "GitHub Copilot URL access".to_string(),
            cache_scope: json!({"url": url}),
            tool_args: Some(json!({"url": url, "intention": intention})),
            reason: Some(intention.clone()),
        },
        CopilotPermissionRequest::Memory { subject, fact, .. } => SummaryDef {
            prefix: "copilot:memory",
            tool_name: "copilot_memory".to_string(),
            display_name: "GitHub Copilot memory update".to_string(),
            cache_scope: json!({"subject": subject, "fact": fact}),
            tool_args: Some(json!({"subject": subject, "fact": fact})),
            reason: Some("GitHub Copilot wants to store a memory fact.".to_string()),
        },
        CopilotPermissionRequest::CustomTool { tool_name, tool_description, args, .. } => SummaryDef {
            prefix: "copilot:custom-tool",
            tool_name: format!("copilot_custom_{tool_name}"),
            display_name: format!("GitHub Copilot custom tool {tool_name}"),
            cache_scope: json!({"tool": tool_name, "args": args}),
            tool_args: args.clone(),
            reason: Some(tool_description.clone()),
        },
        CopilotPermissionRequest::Hook { tool_name, tool_args, hook_message, .. } => SummaryDef {
            prefix: "copilot:hook",
            tool_name: format!("copilot_hook_{tool_name}"),
            display_name: format!("GitHub Copilot hook {tool_name}"),
            cache_scope: json!({"tool": tool_name, "args": tool_args}),
            tool_args: tool_args.clone(),
            reason: hook_message.clone(),
        },
        CopilotPermissionRequest::Unknown { .. } => return None,
    };
    Some(PermissionPromptSummary {
        cache_key: scoped_cache_key(def.prefix, def.cache_scope),
        tool_name: def.tool_name,
        display_name: def.display_name.clone(),
        learning_label: def.display_name,
        tool_args: def.tool_args,
        reason: def.reason,
    })
}

pub(super) fn map_runtime_error(err: anyhow::Error) -> uni::LLMError {
    uni::LLMError::Provider {
        message: format!("GitHub Copilot runtime bridge failed: {err}"),
        metadata: None,
    }
}

pub(super) fn raw_tool_call_id(tool_call_id: &str) -> Option<&str> {
    (!tool_call_id.trim().is_empty()).then_some(tool_call_id)
}

pub(super) fn harness_call_item_id(prefix: &str, tool_call_id: &str, tool_name: &str) -> String {
    if tool_call_id.trim().is_empty() {
        format!("{prefix}-copilot-tool-{}", tool_name.replace(' ', "_"))
    } else {
        format!("{prefix}-copilot-tool-{tool_call_id}")
    }
}
