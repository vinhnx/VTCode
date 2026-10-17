use super::{CompactConversationCommand, SessionLogExportFormat};
use vtcode_core::compaction::ManualCompactionOptions;
use vtcode_core::config::{ReasoningEffortLevel, VerbosityLevel};
use vtcode_core::review::{ReviewSpec, ReviewTarget, build_review_spec, build_review_spec_with_instructions};

/// Iterate over whitespace-separated tokens in `args`, normalising each to
/// lowercase ASCII before passing it to `f`.  Returns the first `Err` produced
/// by `f`, or `Ok(())` once all tokens have been consumed.
///
/// This is the single place that encodes the "split on whitespace, lowercase,
/// then match" idiom shared by every simple flag-parsing function.
pub(super) fn for_each_token(args: &str, mut f: impl FnMut(&str) -> Result<(), String>) -> Result<(), String> {
    for raw in args.split_whitespace() {
        f(&raw.to_ascii_lowercase())?;
    }
    Ok(())
}

pub(super) fn split_command_and_args(input: &str) -> (&str, &str) {
    if let Some((idx, _)) = input.char_indices().find(|(_, ch)| ch.is_whitespace()) {
        let (command, rest) = input.split_at(idx);
        (command, rest)
    } else {
        (input, "")
    }
}

pub(super) fn parse_prompt_template_args(args: &str) -> Result<Vec<String>, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    shell_words::split(trimmed).map_err(|err| format!("Failed to parse template arguments: {err}"))
}

pub(super) fn parse_compact_command(args: &str) -> Result<CompactConversationCommand, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(CompactConversationCommand::Run {
            options: ManualCompactionOptions::default(),
            native_only: false,
        });
    }

    let tokens = shell_words::split(trimmed).map_err(|err| format!("Failed to parse arguments: {err}"))?;
    if tokens.len() == 1 {
        match tokens[0].as_str() {
            "edit-prompt" => return Ok(CompactConversationCommand::EditDefaultPrompt),
            "reset-prompt" => return Ok(CompactConversationCommand::ResetDefaultPrompt),
            _ => {}
        }
    }

    let mut options = ManualCompactionOptions::default();
    let mut native_only = false;
    let mut index = 0;

    while index < tokens.len() {
        let token = &tokens[index];
        let next_value = |flag: &str, index: &mut usize| -> Result<String, String> {
            let Some(value) = tokens.get(*index + 1) else {
                return Err(format!("Missing value for {flag}"));
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
                options.max_output_tokens = Some(
                    value
                        .parse::<u32>()
                        .map_err(|e| format!("Invalid value for --max-output-tokens: {value}: {e}"))?,
                );
            }
            "--reasoning-effort" => {
                let value = next_value("--reasoning-effort", &mut index)?;
                options.reasoning_effort = Some(
                    ReasoningEffortLevel::parse(&value)
                        .ok_or_else(|| format!("Invalid value for --reasoning-effort: {value}"))?,
                );
            }
            "--verbosity" => {
                let value = next_value("--verbosity", &mut index)?;
                options.verbosity = Some(
                    VerbosityLevel::parse(&value).ok_or_else(|| format!("Invalid value for --verbosity: {value}"))?,
                );
            }
            "--native-only" => {
                native_only = true;
                index += 1;
            }
            // Dropped OpenAI-only flags. `/compact` is now provider-agnostic; these
            // Responses-specific options no longer apply and are rejected with a
            // pointer to the supported flags.
            "--include" | "--store" | "--no-store" | "--service-tier" | "--prompt-cache-key" => {
                return Err(format!(
                    "`{token}` is no longer supported by the unified `/compact` command. \
                     `/compact` now works across all providers using only --instructions, \
                     --max-output-tokens, --reasoning-effort, --verbosity, and --native-only. \
                     OpenAI-native server-side options are applied automatically when compacting \
                     against the native OpenAI API."
                ));
            }
            _ => {
                if let Some(value) = token.strip_prefix("--instructions=") {
                    options.instructions = Some(value.to_string());
                    index += 1;
                } else if let Some(value) = token.strip_prefix("--max-output-tokens=") {
                    options.max_output_tokens = Some(
                        value
                            .parse::<u32>()
                            .map_err(|e| format!("Invalid value for --max-output-tokens: {value}: {e}"))?,
                    );
                    index += 1;
                } else if let Some(value) = token.strip_prefix("--reasoning-effort=") {
                    options.reasoning_effort = Some(
                        ReasoningEffortLevel::parse(value)
                            .ok_or_else(|| format!("Invalid value for --reasoning-effort: {value}"))?,
                    );
                    index += 1;
                } else if let Some(value) = token.strip_prefix("--verbosity=") {
                    options.verbosity = Some(
                        VerbosityLevel::parse(value)
                            .ok_or_else(|| format!("Invalid value for --verbosity: {value}"))?,
                    );
                    index += 1;
                } else if token.starts_with('-') {
                    return Err(format!("Unknown option: {token}"));
                } else {
                    return Err(format!("Unexpected argument: {token}"));
                }
            }
        }
    }

    Ok(CompactConversationCommand::Run { options, native_only })
}

pub(super) fn parse_session_log_export_format(args: &str) -> Result<SessionLogExportFormat, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(SessionLogExportFormat::Both);
    }

    let tokens = shell_words::split(trimmed).map_err(|err| format!("Failed to parse arguments: {err}"))?;

    if tokens.is_empty() {
        return Ok(SessionLogExportFormat::Both);
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
        return Err("Usage: /share [json|markdown|md|html]".to_string());
    };

    match format_token.to_ascii_lowercase().as_str() {
        "json" => Ok(SessionLogExportFormat::Json),
        "markdown" | "md" => Ok(SessionLogExportFormat::Markdown),
        "html" => Ok(SessionLogExportFormat::Html),
        _ => Err("Unknown format. Use one of: json, markdown, md, html.".to_string()),
    }
}

pub(super) fn parse_review_spec(args: &str) -> Result<ReviewSpec, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return build_review_spec(false, None, Vec::new(), None).map_err(|err| err.to_string());
    }

    let tokens = shell_words::split(trimmed).map_err(|err| format!("Failed to parse arguments: {err}"))?;

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
            return Err(format!("Unknown option: {token}"));
        }

        files.push(token.clone());
        index += 1;
    }

    build_review_spec(last_diff, target, files, style).map_err(|err| err.to_string())
}

/// Parse `/review` input that may be legacy CLI flags or free-form natural
/// language. CLI-looking input keeps strict validation (errors propagate);
/// anything else becomes read-only review instructions on the current diff.
///
/// Raw prose must never become shell arguments: `Review the full diff ...`
/// yields `instructions`, not `Files(["Review", "the", ...])`.
pub(super) fn parse_review_input(args: &str) -> Result<ReviewSpec, String> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return build_review_spec(false, None, Vec::new(), None).map_err(|err| err.to_string());
    }

    if !looks_like_cli_review(trimmed) {
        return build_review_spec_with_instructions(false, None, Vec::new(), None, Some(trimmed.to_string()))
            .map_err(|err| err.to_string());
    }

    match parse_review_spec(trimmed) {
        Ok(spec) => {
            // Guard the mixed case: `--style security Review the full diff ...`
            // parses but its "files" are prose, not paths.
            if let ReviewSpec { target: ReviewTarget::Files(files), style, .. } = &spec {
                if !files.iter().all(|file| is_path_like_token(file)) {
                    return build_review_spec_with_instructions(
                        false,
                        None,
                        Vec::new(),
                        style.clone(),
                        Some(trimmed.to_string()),
                    )
                    .map_err(|err| err.to_string());
                }
            }
            Ok(spec)
        }
        Err(err) => {
            // Unknown flags alongside prose are treated as prose ("--bogus review
            // the diff") rather than a hard error; bare CLI typos still error.
            if err.starts_with("Unknown option:") && !looks_like_cli_review_strict(trimmed) {
                let token_count = shell_words::split(trimmed).map(|tokens| tokens.len()).unwrap_or(0);
                if token_count > 1 {
                    return build_review_spec_with_instructions(
                        false,
                        None,
                        Vec::new(),
                        None,
                        Some(trimmed.to_string()),
                    )
                    .map_err(|err| err.to_string());
                }
            }
            Err(err)
        }
    }
}

fn is_path_like_token(token: &str) -> bool {
    if token.starts_with('-') {
        return false;
    }
    token.contains('/') || token.contains('\\') || token.contains('.')
}

/// Fast pre-check: does this look like legacy CLI at all?
fn looks_like_cli_review(trimmed: &str) -> bool {
    if trimmed.starts_with('-') {
        return true;
    }
    let Ok(tokens) = shell_words::split(trimmed) else {
        return false;
    };
    if tokens.is_empty() {
        return true;
    }
    if tokens.len() == 1 {
        return true;
    }
    // Any-length all-path lists such as `src/main.rs src/lib.rs` stay CLI.
    // Prose always contains a non-path word, so no length cap is needed.
    tokens.iter().all(|token| is_path_like_token(token))
}

/// Strict CLI shape: every token is a known flag, `flag=value`, or path-like.
fn looks_like_cli_review_strict(trimmed: &str) -> bool {
    let Ok(tokens) = shell_words::split(trimmed) else {
        return false;
    };
    if tokens.is_empty() {
        return true;
    }
    tokens.iter().all(|token| {
        matches!(token.as_str(), "--last-diff" | "--target" | "--style" | "--file")
            || token.starts_with("--target=")
            || token.starts_with("--style=")
            || token.starts_with("--file=")
            || is_path_like_token(token)
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CompactConversationCommand, SessionLogExportFormat, parse_compact_command, parse_review_input,
        parse_review_spec, parse_session_log_export_format,
    };
    use vtcode_core::compaction::ManualCompactionOptions;
    use vtcode_core::config::{ReasoningEffortLevel, VerbosityLevel};
    use vtcode_core::review::ReviewTarget;

    #[test]
    fn compact_defaults_to_automatic_run() {
        assert_eq!(
            parse_compact_command("").expect("compact command"),
            CompactConversationCommand::Run { options: Default::default(), native_only: false }
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
            "--instructions \"keep only decisions\" --max-output-tokens 128 --reasoning-effort minimal --verbosity high --native-only",
        )
        .expect("compact flags should parse");

        assert_eq!(
            parsed,
            CompactConversationCommand::Run {
                options: ManualCompactionOptions {
                    instructions: Some("keep only decisions".to_string()),
                    max_output_tokens: Some(128),
                    reasoning_effort: Some(ReasoningEffortLevel::Minimal),
                    verbosity: Some(VerbosityLevel::High),
                    ..ManualCompactionOptions::default()
                },
                native_only: true,
            }
        );
    }

    #[test]
    fn compact_parses_equals_form_flags() {
        let parsed = parse_compact_command(
            "--instructions=keep-decisions --max-output-tokens=64 --reasoning-effort=minimal --verbosity=high",
        )
        .expect("compact equals-form flags should parse");

        assert_eq!(
            parsed,
            CompactConversationCommand::Run {
                options: ManualCompactionOptions {
                    instructions: Some("keep-decisions".to_string()),
                    max_output_tokens: Some(64),
                    reasoning_effort: Some(ReasoningEffortLevel::Minimal),
                    verbosity: Some(VerbosityLevel::High),
                    ..ManualCompactionOptions::default()
                },
                native_only: false,
            }
        );
    }

    #[test]
    fn compact_rejects_invalid_flags() {
        parse_compact_command("--max-output-tokens nope").unwrap_err();
        parse_compact_command("--reasoning-effort absurd").unwrap_err();
        parse_compact_command("--verbosity louder").unwrap_err();
        parse_compact_command("--bogus-flag").unwrap_err();
    }

    #[test]
    fn compact_rejects_dropped_openai_only_flags() {
        for dropped in [
            "--include reasoning.encrypted_content",
            "--store",
            "--no-store",
            "--service-tier priority",
            "--prompt-cache-key lineage-1",
        ] {
            let err = parse_compact_command(dropped).unwrap_err();
            assert!(err.contains("no longer supported"), "expected deprecation message for `{dropped}`, got: {err}");
        }
    }

    #[test]
    fn share_log_defaults_to_json_and_html() {
        assert_eq!(parse_session_log_export_format("").expect("format"), SessionLogExportFormat::Both);
    }

    #[test]
    fn share_log_supports_markdown_aliases() {
        assert_eq!(parse_session_log_export_format("markdown").expect("format"), SessionLogExportFormat::Markdown);
        assert_eq!(parse_session_log_export_format("md").expect("format"), SessionLogExportFormat::Markdown);
        assert_eq!(parse_session_log_export_format("--format=md").expect("format"), SessionLogExportFormat::Markdown);
        assert_eq!(
            parse_session_log_export_format("--format markdown").expect("format"),
            SessionLogExportFormat::Markdown
        );
    }

    #[test]
    fn share_log_supports_html() {
        assert_eq!(parse_session_log_export_format("html").expect("format"), SessionLogExportFormat::Html);
        assert_eq!(parse_session_log_export_format("--format=html").expect("format"), SessionLogExportFormat::Html);
    }

    #[test]
    fn share_log_rejects_unknown_format() {
        parse_session_log_export_format("xml").unwrap_err();
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
        assert!(matches!(spec.target, ReviewTarget::Files(ref files) if files == &["src/main.rs".to_string()]));
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
        let err =
            parse_review_spec("--last-diff --target HEAD~1..HEAD").expect_err("conflicting selectors should fail");
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
    fn review_input_treats_prose_as_instructions_not_files() {
        let spec = parse_review_input("Review the full diff and nearby code for correctness, regressions")
            .expect("prose should parse");
        assert!(matches!(spec.target, ReviewTarget::CurrentDiff));
        assert_eq!(
            spec.instructions.as_deref(),
            Some("Review the full diff and nearby code for correctness, regressions")
        );
    }

    #[test]
    fn review_input_treats_bare_bogus_prose_as_instructions() {
        // Asymmetric counterpart: same unknown flag with prose falls back to NL,
        // while the bare flag above still errors in strict parsing.
        let spec = parse_review_input("--bogus review the full diff").expect("prose should parse");
        assert!(matches!(spec.target, ReviewTarget::CurrentDiff));
        assert!(spec.instructions.as_deref() == Some("--bogus review the full diff"));
    }

    #[test]
    fn review_input_keeps_short_file_lists_as_cli() {
        let spec = parse_review_input("src/main.rs src/lib.rs").expect("files should parse");
        assert!(
            matches!(spec.target, ReviewTarget::Files(ref files) if files == &["src/main.rs".to_string(), "src/lib.rs".to_string()])
        );
        assert_eq!(spec.instructions, None);
    }

    #[test]
    fn review_input_keeps_long_file_lists_as_cli() {
        // Regression: the file list must not be capped; five paths are still
        // a target, not prose, and must not fall back to the current diff.
        let spec = parse_review_input("a.rs b.rs c.rs d.rs e.rs").expect("long file list should parse");
        assert!(matches!(spec.target, ReviewTarget::Files(ref files) if files.len() == 5));
        assert_eq!(spec.instructions, None);
    }

    #[test]
    fn review_input_keeps_flags_as_cli_and_prose_with_flags_as_instructions() {
        let cli = parse_review_input("--target HEAD~1..HEAD --style security").expect("flags should parse");
        assert!(matches!(cli.target, ReviewTarget::Custom(ref value) if value == "HEAD~1..HEAD"));
        assert_eq!(cli.instructions, None);

        let mixed =
            parse_review_input("--style security Review the full diff for regressions").expect("mixed should parse");
        assert!(matches!(mixed.target, ReviewTarget::CurrentDiff));
        assert_eq!(mixed.style.as_deref(), Some("security"));
        assert!(mixed.instructions.as_deref() == Some("--style security Review the full diff for regressions"));
    }
}
