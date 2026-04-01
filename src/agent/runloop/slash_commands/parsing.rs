use super::{CompactConversationCommand, SessionLogExportFormat};
use vtcode_config::OpenAIServiceTier;
use vtcode_core::config::{ReasoningEffortLevel, VerbosityLevel};
use vtcode_core::llm::provider::ResponsesCompactionOptions;
use vtcode_core::review::{ReviewSpec, build_review_spec};

pub(super) fn split_command_and_args(input: &str) -> (&str, &str) {
    if let Some((idx, _)) = input.char_indices().find(|(_, ch)| ch.is_whitespace()) {
        let (command, rest) = input.split_at(idx);
        (command, rest)
    } else {
        (input, "")
    }
}

pub(super) fn parse_prompt_template_args(args: &str) -> std::result::Result<Vec<String>, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    shell_words::split(trimmed)
        .map_err(|err| format!("Failed to parse template arguments: {}", err))
}

pub(super) fn parse_compact_command(
    args: &str,
) -> std::result::Result<CompactConversationCommand, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(CompactConversationCommand::Run {
            options: ResponsesCompactionOptions::default(),
        });
    }

    let tokens =
        shell_words::split(trimmed).map_err(|err| format!("Failed to parse arguments: {}", err))?;
    if tokens.len() == 1 {
        match tokens[0].as_str() {
            "edit-prompt" => return Ok(CompactConversationCommand::EditDefaultPrompt),
            "reset-prompt" => return Ok(CompactConversationCommand::ResetDefaultPrompt),
            _ => {}
        }
    }

    let mut options = ResponsesCompactionOptions::default();
    let mut index = 0;

    while index < tokens.len() {
        let token = &tokens[index];
        let next_value = |flag: &str, index: &mut usize| -> std::result::Result<String, String> {
            let Some(value) = tokens.get(*index + 1) else {
                return Err(format!("Missing value for {}", flag));
            };
            *index += 2;
            Ok(value.clone())
        };

        match token.as_str() {
            "--instructions" => {
                options.instructions = Some(next_value("--instructions", &mut index)?);
            }
            "--max-output-tokens" => {
                let value = next_value("--max-output-tokens", &mut index)?;
                options.max_output_tokens =
                    Some(value.parse::<u32>().map_err(|_| {
                        format!("Invalid value for --max-output-tokens: {}", value)
                    })?);
            }
            "--reasoning-effort" => {
                let value = next_value("--reasoning-effort", &mut index)?;
                options.reasoning_effort =
                    Some(ReasoningEffortLevel::parse(&value).ok_or_else(|| {
                        format!("Invalid value for --reasoning-effort: {}", value)
                    })?);
            }
            "--verbosity" => {
                let value = next_value("--verbosity", &mut index)?;
                options.verbosity = Some(
                    VerbosityLevel::parse(&value)
                        .ok_or_else(|| format!("Invalid value for --verbosity: {}", value))?,
                );
            }
            "--include" => {
                let value = next_value("--include", &mut index)?;
                options
                    .responses_include
                    .get_or_insert_with(Vec::new)
                    .push(value);
            }
            "--store" => {
                if matches!(options.response_store, Some(false)) {
                    return Err("Use either --store or --no-store, not both.".to_string());
                }
                options.response_store = Some(true);
                index += 1;
            }
            "--no-store" => {
                if matches!(options.response_store, Some(true)) {
                    return Err("Use either --store or --no-store, not both.".to_string());
                }
                options.response_store = Some(false);
                index += 1;
            }
            "--service-tier" => {
                let value = next_value("--service-tier", &mut index)?;
                options.service_tier = Some(
                    OpenAIServiceTier::parse(&value)
                        .ok_or_else(|| format!("Invalid value for --service-tier: {}", value))?
                        .as_str()
                        .to_string(),
                );
            }
            "--prompt-cache-key" => {
                options.prompt_cache_key = Some(next_value("--prompt-cache-key", &mut index)?);
            }
            _ => {
                if let Some(value) = token.strip_prefix("--instructions=") {
                    options.instructions = Some(value.to_string());
                    index += 1;
                } else if let Some(value) = token.strip_prefix("--max-output-tokens=") {
                    options.max_output_tokens = Some(value.parse::<u32>().map_err(|_| {
                        format!("Invalid value for --max-output-tokens: {}", value)
                    })?);
                    index += 1;
                } else if let Some(value) = token.strip_prefix("--reasoning-effort=") {
                    options.reasoning_effort =
                        Some(ReasoningEffortLevel::parse(value).ok_or_else(|| {
                            format!("Invalid value for --reasoning-effort: {}", value)
                        })?);
                    index += 1;
                } else if let Some(value) = token.strip_prefix("--verbosity=") {
                    options.verbosity = Some(
                        VerbosityLevel::parse(value)
                            .ok_or_else(|| format!("Invalid value for --verbosity: {}", value))?,
                    );
                    index += 1;
                } else if let Some(value) = token.strip_prefix("--include=") {
                    options
                        .responses_include
                        .get_or_insert_with(Vec::new)
                        .push(value.to_string());
                    index += 1;
                } else if let Some(value) = token.strip_prefix("--service-tier=") {
                    options.service_tier = Some(
                        OpenAIServiceTier::parse(value)
                            .ok_or_else(|| format!("Invalid value for --service-tier: {}", value))?
                            .as_str()
                            .to_string(),
                    );
                    index += 1;
                } else if let Some(value) = token.strip_prefix("--prompt-cache-key=") {
                    options.prompt_cache_key = Some(value.to_string());
                    index += 1;
                } else if token.starts_with('-') {
                    return Err(format!("Unknown option: {}", token));
                } else {
                    return Err(format!("Unexpected argument: {}", token));
                }
            }
        }
    }

    Ok(CompactConversationCommand::Run { options })
}

pub(super) fn parse_session_log_export_format(
    args: &str,
) -> std::result::Result<SessionLogExportFormat, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(SessionLogExportFormat::Json);
    }

    let tokens =
        shell_words::split(trimmed).map_err(|err| format!("Failed to parse arguments: {}", err))?;

    if tokens.is_empty() {
        return Ok(SessionLogExportFormat::Json);
    }

    let format_token = if tokens.len() == 1 {
        if let Some(value) = tokens[0].strip_prefix("--format=") {
            value
        } else {
            tokens[0].as_str()
        }
    } else if tokens.len() == 2 && tokens[0].eq_ignore_ascii_case("--format") {
        tokens[1].as_str()
    } else {
        return Err(
            "Usage: /share-log [json|markdown|md] (or /share-log --format <json|markdown|md>)"
                .to_string(),
        );
    };

    match format_token.to_ascii_lowercase().as_str() {
        "json" => Ok(SessionLogExportFormat::Json),
        "markdown" | "md" => Ok(SessionLogExportFormat::Markdown),
        _ => Err("Unknown format. Use one of: json, markdown, md.".to_string()),
    }
}

pub(super) fn parse_review_spec(args: &str) -> std::result::Result<ReviewSpec, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return build_review_spec(false, None, Vec::new(), None).map_err(|err| err.to_string());
    }

    let tokens =
        shell_words::split(trimmed).map_err(|err| format!("Failed to parse arguments: {}", err))?;

    let mut last_diff = false;
    let mut target: Option<String> = None;
    let mut style: Option<String> = None;
    let mut files = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        let token = &tokens[index];
        if token == "--last-diff" {
            last_diff = true;
            index += 1;
            continue;
        }
        if let Some(value) = token.strip_prefix("--target=") {
            target = Some(value.to_string());
            index += 1;
            continue;
        }
        if token == "--target" {
            let Some(value) = tokens.get(index + 1) else {
                return Err("Missing value for --target".to_string());
            };
            target = Some(value.clone());
            index += 2;
            continue;
        }
        if let Some(value) = token.strip_prefix("--style=") {
            style = Some(value.to_string());
            index += 1;
            continue;
        }
        if token == "--style" {
            let Some(value) = tokens.get(index + 1) else {
                return Err("Missing value for --style".to_string());
            };
            style = Some(value.clone());
            index += 2;
            continue;
        }
        if let Some(value) = token.strip_prefix("--file=") {
            files.push(value.to_string());
            index += 1;
            continue;
        }
        if token == "--file" {
            let Some(value) = tokens.get(index + 1) else {
                return Err("Missing value for --file".to_string());
            };
            files.push(value.clone());
            index += 2;
            continue;
        }
        if token.starts_with('-') {
            return Err(format!("Unknown option: {}", token));
        }

        files.push(token.clone());
        index += 1;
    }

    build_review_spec(last_diff, target, files, style).map_err(|err| err.to_string())
}

pub(super) fn parse_analyze_scope(args: &str) -> std::result::Result<Option<String>, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let tokens =
        shell_words::split(trimmed).map_err(|err| format!("Failed to parse arguments: {}", err))?;

    if tokens.len() != 1 {
        return Err("Usage: /analyze [full|security|performance]".to_string());
    }

    let scope = tokens[0].to_ascii_lowercase();
    match scope.as_str() {
        "full" | "security" | "performance" => Ok(Some(scope)),
        _ => Err(format!(
            "Unknown analysis scope '{}'. Use full, security, or performance.",
            tokens[0]
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompactConversationCommand, SessionLogExportFormat, parse_analyze_scope,
        parse_compact_command, parse_review_spec, parse_session_log_export_format,
    };
    use vtcode_core::config::{ReasoningEffortLevel, VerbosityLevel};
    use vtcode_core::review::ReviewTarget;

    #[test]
    fn compact_defaults_to_automatic_run() {
        assert_eq!(
            parse_compact_command("").expect("compact command"),
            CompactConversationCommand::Run {
                options: Default::default(),
            }
        );
    }

    #[test]
    fn compact_parses_prompt_subcommands() {
        assert_eq!(
            parse_compact_command("edit-prompt").expect("edit prompt"),
            CompactConversationCommand::EditDefaultPrompt
        );
        assert_eq!(
            parse_compact_command("reset-prompt").expect("reset prompt"),
            CompactConversationCommand::ResetDefaultPrompt
        );
    }

    #[test]
    fn compact_parses_direct_flags() {
        let parsed = parse_compact_command(
            "--instructions \"keep only decisions\" --max-output-tokens 128 --reasoning-effort minimal --verbosity high --include reasoning.encrypted_content --include output_text.logprobs --store --service-tier priority --prompt-cache-key lineage-1",
        )
        .expect("compact flags should parse");

        assert_eq!(
            parsed,
            CompactConversationCommand::Run {
                options: vtcode_core::llm::provider::ResponsesCompactionOptions {
                    instructions: Some("keep only decisions".to_string()),
                    max_output_tokens: Some(128),
                    reasoning_effort: Some(ReasoningEffortLevel::Minimal),
                    verbosity: Some(VerbosityLevel::High),
                    responses_include: Some(vec![
                        "reasoning.encrypted_content".to_string(),
                        "output_text.logprobs".to_string(),
                    ]),
                    response_store: Some(true),
                    service_tier: Some("priority".to_string()),
                    prompt_cache_key: Some("lineage-1".to_string()),
                }
            }
        );
    }

    #[test]
    fn compact_rejects_invalid_flags() {
        assert!(parse_compact_command("--max-output-tokens nope").is_err());
        assert!(parse_compact_command("--reasoning-effort absurd").is_err());
        assert!(parse_compact_command("--verbosity louder").is_err());
        assert!(parse_compact_command("--service-tier turbo").is_err());
        assert!(parse_compact_command("--store --no-store").is_err());
    }

    #[test]
    fn share_log_defaults_to_json() {
        assert_eq!(
            parse_session_log_export_format("").expect("format"),
            SessionLogExportFormat::Json
        );
    }

    #[test]
    fn share_log_supports_markdown_aliases() {
        assert_eq!(
            parse_session_log_export_format("markdown").expect("format"),
            SessionLogExportFormat::Markdown
        );
        assert_eq!(
            parse_session_log_export_format("md").expect("format"),
            SessionLogExportFormat::Markdown
        );
        assert_eq!(
            parse_session_log_export_format("--format=md").expect("format"),
            SessionLogExportFormat::Markdown
        );
        assert_eq!(
            parse_session_log_export_format("--format markdown").expect("format"),
            SessionLogExportFormat::Markdown
        );
    }

    #[test]
    fn share_log_rejects_unknown_format() {
        assert!(parse_session_log_export_format("xml").is_err());
    }

    #[test]
    fn review_defaults_to_current_diff() {
        let spec = parse_review_spec("").expect("review spec");
        assert!(matches!(spec.target, ReviewTarget::CurrentDiff));
        assert_eq!(spec.style, None);
    }

    #[test]
    fn review_parses_target_and_style() {
        let spec = parse_review_spec("--target HEAD~1..HEAD --style security").expect("spec");
        assert!(matches!(spec.target, ReviewTarget::Custom(ref value) if value == "HEAD~1..HEAD"));
        assert_eq!(spec.style.as_deref(), Some("security"));
    }

    #[test]
    fn review_accepts_file_flag() {
        let spec = parse_review_spec("--file src/main.rs").expect("spec");
        assert!(
            matches!(spec.target, ReviewTarget::Files(ref files) if files == &["src/main.rs".to_string()])
        );
    }

    #[test]
    fn review_accepts_multiple_positional_files() {
        let spec = parse_review_spec("src/main.rs src/lib.rs").expect("spec");
        assert!(
            matches!(spec.target, ReviewTarget::Files(ref files) if files == &["src/main.rs".to_string(), "src/lib.rs".to_string()])
        );
        assert_eq!(spec.style, None);
    }

    #[test]
    fn review_rejects_conflicting_target_selectors() {
        let err = parse_review_spec("--last-diff --target HEAD~1..HEAD")
            .expect_err("conflicting selectors should fail");
        assert!(err.contains("--last-diff"));
    }

    #[test]
    fn review_rejects_missing_target_value() {
        let err = parse_review_spec("--target").expect_err("missing value should fail");
        assert!(err.contains("Missing value"));
    }

    #[test]
    fn review_rejects_unknown_flag() {
        let err = parse_review_spec("--bogus").expect_err("unknown flag should fail");
        assert!(err.contains("Unknown option"));
    }

    #[test]
    fn analyze_defaults_to_full_when_empty() {
        assert_eq!(parse_analyze_scope("").expect("analyze scope"), None);
    }

    #[test]
    fn analyze_accepts_known_scopes() {
        assert_eq!(
            parse_analyze_scope("security").expect("analyze scope"),
            Some("security".to_string())
        );
        assert_eq!(
            parse_analyze_scope("PERFORMANCE").expect("analyze scope"),
            Some("performance".to_string())
        );
    }

    #[test]
    fn analyze_rejects_unknown_or_extra_arguments() {
        let err = parse_analyze_scope("foo").expect_err("unknown scope should fail");
        assert!(err.contains("Unknown analysis scope"));

        let err = parse_analyze_scope("security extra").expect_err("extra args should fail");
        assert!(err.contains("Usage: /analyze"));
    }
}
