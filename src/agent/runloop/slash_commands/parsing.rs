use super::SessionLogExportFormat;

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

#[cfg(test)]
mod tests {
    use super::{SessionLogExportFormat, parse_session_log_export_format};

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
}
