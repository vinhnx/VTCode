use std::borrow::Cow;
use std::path::Path;

use anyhow::Result;
use serde_json::Value;
use shell_words::split as shell_split;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::streams::{render_stream_section, resolve_stdout_tail_limit, strip_ansi_codes};
use super::styles::{GitStyles, LsStyles};

pub(crate) fn render_terminal_command_panel(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
    vt_config: Option<&VTCodeConfig>,
    allow_ansi: bool,
) -> Result<()> {
    let stdout_raw = payload.get("stdout").and_then(Value::as_str).unwrap_or("");
    let stderr_raw = payload.get("stderr").and_then(Value::as_str).unwrap_or("");
    let command_tokens = parse_command_tokens(payload);
    let stdout = preprocess_terminal_stdout(command_tokens.as_deref(), stdout_raw);
    let stderr = preprocess_terminal_stdout(command_tokens.as_deref(), stderr_raw);

    let output_mode = vt_config
        .map(|cfg| cfg.ui.tool_output_mode)
        .unwrap_or(ToolOutputMode::Compact);
    let tail_limit = resolve_stdout_tail_limit(vt_config);

    if stdout.trim().is_empty() && stderr.trim().is_empty() {
        renderer.line(MessageStyle::Info, "(no output)")?;
        return Ok(());
    }

    renderer.line(
        MessageStyle::Info,
        "─────────────────────────────────────────────────────────────────────────────",
    )?;

    if !stdout.trim().is_empty() {
        render_stream_section(
            renderer,
            "",
            stdout.as_ref(),
            output_mode,
            tail_limit,
            Some(tools::RUN_COMMAND),
            git_styles,
            ls_styles,
            MessageStyle::Response,
            allow_ansi,
            vt_config,
        )?;
    }
    if !stderr.as_ref().trim().is_empty() {
        render_stream_section(
            renderer,
            "stderr",
            stderr.as_ref(),
            output_mode,
            tail_limit,
            Some(tools::RUN_COMMAND),
            git_styles,
            ls_styles,
            MessageStyle::Error,
            allow_ansi,
            vt_config,
        )?;
    }

    renderer.line(
        MessageStyle::Info,
        "─────────────────────────────────────────────────────────────────────────────",
    )?;

    Ok(())
}

fn parse_command_tokens(payload: &Value) -> Option<Vec<String>> {
    if let Some(array) = payload.get("command").and_then(Value::as_array) {
        let mut tokens = Vec::new();
        for value in array {
            if let Some(segment) = value.as_str()
                && !segment.is_empty()
            {
                tokens.push(segment.to_string());
            }
        }
        if !tokens.is_empty() {
            return Some(tokens);
        }
    }

    if let Some(command_str) = payload.get("command").and_then(Value::as_str) {
        if command_str.trim().is_empty() {
            return None;
        }
        if let Ok(segments) = shell_split(command_str)
            && !segments.is_empty()
        {
            return Some(segments);
        }
    }
    None
}

fn normalized_command_name(tokens: &[String]) -> Option<String> {
    tokens
        .first()
        .and_then(|cmd| Path::new(cmd).file_name())
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
}

fn command_is_multicol_listing(tokens: &[String]) -> bool {
    normalized_command_name(tokens)
        .map(|name| {
            matches!(
                name.as_str(),
                "ls" | "dir" | "vdir" | "gls" | "colorls" | "exa" | "eza"
            )
        })
        .unwrap_or(false)
}

fn listing_has_single_column_flag(tokens: &[String]) -> bool {
    tokens.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-1" | "--format=single-column"
                | "--long"
                | "-l"
                | "--tree"
                | "--grid=never"
                | "--no-grid"
        )
    })
}

fn preprocess_terminal_stdout<'a>(tokens: Option<&[String]>, stdout: &'a str) -> Cow<'a, str> {
    if stdout.trim().is_empty() {
        return Cow::Borrowed(stdout);
    }

    let stripped = strip_ansi_codes(stdout);
    let normalized = match stripped {
        Cow::Borrowed(text) => normalize_carriage_returns(text),
        Cow::Owned(text) => normalize_carriage_returns(&text).into_owned().into(),
    };
    let should_strip_numbers = tokens
        .map(command_can_emit_rust_diagnostics)
        .unwrap_or(false)
        && looks_like_rust_diagnostic(normalized.as_ref());

    if should_strip_numbers {
        return strip_rust_diagnostic_columns(normalized);
    }

    if let Some(parts) = tokens
        && command_is_multicol_listing(parts)
        && !listing_has_single_column_flag(parts)
    {
        let plain = strip_ansi_codes(normalized.as_ref());
        let mut rows = String::with_capacity(plain.len());
        for entry in plain.split_whitespace() {
            if entry.is_empty() {
                continue;
            }
            rows.push_str(entry);
            rows.push('\n');
        }
        return Cow::Owned(rows);
    }

    normalized
}

fn command_can_emit_rust_diagnostics(tokens: &[String]) -> bool {
    tokens
        .first()
        .map(|cmd| matches!(cmd.as_str(), "cargo" | "rustc"))
        .unwrap_or(false)
}

fn looks_like_rust_diagnostic(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    let mut snippet_lines = 0usize;
    let mut pointer_lines = 0usize;
    let mut has_location_marker = false;

    for line in text.lines().take(200) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("--> ") {
            has_location_marker = true;
        }
        if trimmed.starts_with('|') {
            pointer_lines += 1;
        }
        if let Some((prefix, _)) = trimmed.split_once('|') {
            let prefix_trimmed = prefix.trim();
            if !prefix_trimmed.is_empty() && prefix_trimmed.chars().all(|ch| ch.is_ascii_digit()) {
                snippet_lines += 1;
            }
        }
        if snippet_lines >= 1 && pointer_lines >= 1 {
            return true;
        }
        if snippet_lines >= 2 && has_location_marker {
            return true;
        }
    }

    false
}

fn strip_rust_diagnostic_columns<'a>(content: Cow<'a, str>) -> Cow<'a, str> {
    match content {
        Cow::Borrowed(text) => strip_rust_diagnostic_columns_from_str(text)
            .map(Cow::Owned)
            .unwrap_or_else(|| Cow::Borrowed(text)),
        Cow::Owned(text) => {
            if let Some(stripped) = strip_rust_diagnostic_columns_from_str(&text) {
                Cow::Owned(stripped)
            } else {
                Cow::Owned(text)
            }
        }
    }
}

fn strip_rust_diagnostic_columns_from_str(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }

    let mut output = String::with_capacity(input.len());
    let mut changed = false;

    for chunk in input.split_inclusive('\n') {
        let (line, had_newline) = chunk
            .strip_suffix('\n')
            .map(|line| (line, true))
            .unwrap_or((chunk, false));

        if let Some(prefix_end) = rust_diagnostic_prefix_end(line) {
            changed = true;
            output.push_str(&line[prefix_end..]);
        } else {
            output.push_str(line);
        }

        if had_newline {
            output.push('\n');
        }
    }

    if changed { Some(output) } else { None }
}

fn rust_diagnostic_prefix_end(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let len = bytes.len();

    let mut idx = 0usize;
    while idx < len && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if idx >= len {
        return None;
    }

    if bytes[idx].is_ascii_digit() {
        let mut cursor = idx;
        while cursor < len && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
        if cursor == idx {
            return None;
        }
        while cursor < len && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if cursor < len && bytes[cursor] == b'|' {
            cursor += 1;
            if cursor < len && bytes[cursor] == b' ' {
                cursor += 1;
            }
            return Some(cursor);
        }
        return None;
    }

    if bytes[idx] == b'|' {
        let mut cursor = idx + 1;
        if cursor < len && bytes[cursor] == b' ' {
            cursor += 1;
        }
        return Some(cursor);
    }

    None
}

fn normalize_carriage_returns(input: &str) -> Cow<'_, str> {
    if !input.contains('\r') {
        return Cow::Borrowed(input);
    }

    let mut output = String::with_capacity(input.len());
    let mut current_line = String::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if matches!(chars.peek(), Some('\n')) {
                    chars.next();
                    output.push_str(&current_line);
                    output.push('\n');
                    current_line.clear();
                } else {
                    current_line.clear();
                }
            }
            '\n' => {
                output.push_str(&current_line);
                output.push('\n');
                current_line.clear();
            }
            _ => current_line.push(ch),
        }
    }

    if !current_line.is_empty() {
        output.push_str(&current_line);
    }

    Cow::Owned(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocess_strips_rust_line_numbers_for_cargo_output() {
        let tokens = vec!["cargo".to_string(), "check".to_string()];
        let input = "\
warning: this is a warning
  --> src/main.rs:12:5
   |
12 |     let x = 5;
   |     ----- value defined here
   |
   = note: additional context
";
        let processed = preprocess_terminal_stdout(Some(&tokens), input);
        let output = processed.to_string();
        assert!(!output.contains("12 |"));
        assert!(output.contains("let x = 5;"));
        assert!(output.contains("----- value defined here"));
    }

    #[test]
    fn detects_rust_diagnostic_shape() {
        let sample = "\
warning: something
  --> src/lib.rs:7:9
   |
 7 |     println!(\"hi\");
   |     ^^^^^^^^^^^^^^^
";
        assert!(
            looks_like_rust_diagnostic(sample),
            "should detect diagnostic structure"
        );
    }

    #[test]
    fn rust_prefix_end_handles_pointer_lines() {
        let line = "   |         ^ expected struct `Foo`, found enum `Bar`";
        let idx = rust_diagnostic_prefix_end(line).expect("prefix");
        assert_eq!(
            &line[idx..],
            "        ^ expected struct `Foo`, found enum `Bar`"
        );
    }

    #[test]
    fn strip_rust_columns_returns_none_when_unmodified() {
        let sample = "no diagnostics here";
        assert!(strip_rust_diagnostic_columns_from_str(sample).is_none());
    }
}
