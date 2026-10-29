use super::*;

fn append_trailing_text_part(content: &mut uni::MessageContent, trailing_text: String) {
    match content {
        uni::MessageContent::Text(text) => text.push_str(&trailing_text),
        uni::MessageContent::Parts(parts) => parts.push(uni::ContentPart::text(trailing_text)),
    }
}

pub(super) fn append_file_reference_metadata(content: &mut uni::MessageContent, input: &str, workspace: &Path) {
    let Some(metadata) = build_file_reference_metadata(input, workspace) else {
        return;
    };

    append_trailing_text_part(content, metadata);
}

pub(super) fn append_agent_reference_metadata(content: &mut uni::MessageContent, selected_agents: &[String]) {
    if selected_agents.is_empty() {
        return;
    }

    let mut metadata = String::from("\n\n[agent_reference_metadata]\n");
    for mention in selected_agents {
        let _ = writeln!(metadata, "selected=@agent-{mention}");
    }

    append_trailing_text_part(content, metadata);
}

pub(super) fn supports_native_openai_file_inputs(
    provider_name: &str,
    model_supports_responses_compaction: bool,
) -> bool {
    provider_name.eq_ignore_ascii_case("openai") && model_supports_responses_compaction
}

pub(super) async fn handle_inline_prompt_suggestion_request(
    ctx: &mut InteractionLoopContext<'_>,
    state: &mut InteractionState<'_>,
    draft: &str,
) -> Result<()> {
    let Some(suggestion) = generate_inline_prompt_suggestion(
        ctx.provider_client.as_ref(),
        ctx.config,
        ctx.vt_cfg.as_ref(),
        &ctx.config.workspace,
        ctx.conversation_history,
        ctx.session_stats,
        ctx.tool_registry,
        draft,
    )
    .await
    else {
        ctx.handle.clear_inline_prompt_suggestion();
        ctx.renderer
            .line(MessageStyle::Info, "No inline prompt suggestion is available for the current draft.")?;
        return Ok(());
    };

    if suggestion.source == PromptSuggestionSource::Llm
        && ctx
            .vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.prompt_suggestions.show_cost_notice)
            .unwrap_or(true)
        && !*state.inline_prompt_cost_notice_shown
    {
        ctx.renderer.line(
            MessageStyle::Info,
            "Inline prompt suggestions may use tokens when VT Code calls your configured LLM provider.",
        )?;
        *state.inline_prompt_cost_notice_shown = true;
    }

    ctx.handle
        .set_inline_prompt_suggestion(suggestion.prompt, suggestion.source == PromptSuggestionSource::Llm);
    Ok(())
}

pub(super) fn build_file_reference_metadata(input: &str, workspace: &Path) -> Option<String> {
    let mut alias_to_full_path = BTreeMap::new();
    for at_match in vtcode_commons::at_pattern::find_at_patterns(input) {
        let alias = at_match.path.trim();
        if alias.is_empty() {
            continue;
        }
        if let Some(full_path) = resolve_full_path_for_alias(alias, workspace) {
            alias_to_full_path.insert(format!("@{alias}"), full_path);
        }
    }

    if alias_to_full_path.is_empty() {
        return None;
    }

    let mut metadata = String::from("\n\n[file_reference_metadata]\n");
    for (alias, full_path) in &alias_to_full_path {
        let _ = writeln!(metadata, "{alias}={full_path}");
    }
    metadata.push_str(
        "Hint: Read each referenced file once using the resolved path above. Do not re-read unless truncated.\n",
    );

    Some(metadata)
}

fn resolve_full_path_for_alias(alias: &str, workspace: &Path) -> Option<String> {
    let trimmed = alias.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("data:")
        || !vtcode_commons::paths::is_safe_relative_path(trimmed)
    {
        return None;
    }

    let resolved = vtcode_commons::paths::resolve_workspace_path(workspace, Path::new(trimmed)).ok()?;
    Some(resolved.to_string_lossy().to_string())
}

pub(in crate::agent::runloop::unified::turn::session::interaction_loop_runner) async fn build_user_message_content(
    ctx: &mut InteractionLoopContext<'_>,
    input: &SubmittedInput,
) -> uni::MessageContent {
    let text = input.text.as_str();
    let allow_structured_non_image_file_inputs = supports_native_openai_file_inputs(
        &ctx.config.provider,
        ctx.provider_client.supports_responses_compaction(&ctx.config.model),
    );
    let processed_content = match vtcode_core::utils::at_pattern::parse_at_patterns_with_options(
        text,
        &ctx.config.workspace,
        vtcode_core::utils::at_pattern::AtPatternOptions {
            allow_local_non_image_file_inputs: allow_structured_non_image_file_inputs,
            allow_remote_non_image_file_inputs: allow_structured_non_image_file_inputs,
        },
    )
    .await
    {
        Ok(content) => content,
        Err(err) => {
            tracing::warn!("Failed to parse @ patterns: {}", err);
            uni::MessageContent::text(text.to_string())
        }
    };

    let refined_content = match &processed_content {
        uni::MessageContent::Text(text) => {
            let refined_text = refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref()).await;
            uni::MessageContent::text(refined_text)
        }
        uni::MessageContent::Parts(parts) => {
            let mut refined_parts = Vec::new();
            for part in parts {
                match part {
                    uni::ContentPart::Text { text } => {
                        let refined_text = refine_and_enrich_prompt(text, ctx.config, ctx.vt_cfg.as_ref()).await;
                        refined_parts.push(uni::ContentPart::text(refined_text));
                    }
                    _ => refined_parts.push(part.clone()),
                }
            }
            uni::MessageContent::parts(refined_parts)
        }
    };
    let selected_agents: Vec<String> = if let Some(controller) = ctx.tool_registry.subagent_controller() {
        controller.set_turn_delegation_hints_from_input(text).await
    } else {
        Vec::new()
    };
    let mut refined_content = refined_content;
    append_submitted_attachments(&mut refined_content, input.attachments.as_slice());
    append_file_reference_metadata(&mut refined_content, text, &ctx.config.workspace);
    append_agent_reference_metadata(&mut refined_content, selected_agents.as_slice());
    refined_content
}

pub(super) fn append_submitted_attachments(content: &mut uni::MessageContent, attachments: &[UiContentPart]) {
    if attachments.is_empty() {
        return;
    }

    let mut parts = match std::mem::take(content) {
        uni::MessageContent::Text(text) => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![uni::ContentPart::text(text)]
            }
        }
        uni::MessageContent::Parts(parts) => parts,
    };

    parts.extend(attachments.iter().map(|attachment| match attachment {
        UiContentPart::Text { text } => uni::ContentPart::text(text.clone()),
        UiContentPart::Image { data, media_type } => uni::ContentPart::image(data.to_string(), media_type.clone()),
    }));

    *content = uni::MessageContent::parts(parts);
}
