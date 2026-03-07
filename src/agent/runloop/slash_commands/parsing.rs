use super::SessionLogExportFormat;
use vtcode_core::review::{ReviewSpec, build_review_spec};

pub(super) fn split_command_and_args(input: &str) -> (&str, &str) {
    if let Some((idx, _)) = input.char_indices().find(|(_, ch)| ch.is_whitespace()) {
        let (command, rest) = input.split_at(idx);
        (command, rest)
    } else {
        (input, "")
    }
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

#[cfg(test)]
mod tests {
    use super::{SessionLogExportFormat, parse_review_spec, parse_session_log_export_format};
    use vtcode_core::review::ReviewTarget;

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
    fn review_rejects_missing_target_value() {
        let err = parse_review_spec("--target").expect_err("missing value should fail");
        assert!(err.contains("Missing value"));
    }

    #[test]
    fn review_rejects_unknown_flag() {
        let err = parse_review_spec("--bogus").expect_err("unknown flag should fail");
        assert!(err.contains("Unknown option"));
    }
}
