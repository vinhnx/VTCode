use crate::agent::runloop::git::CodeChangeDelta;
use std::time::Duration;
use vtcode_core::config::constants::ui;
use vtcode_core::core::telemetry::{ModelUsageStats, TelemetryStats};
use vtcode_tui::InlineHeaderContext;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GREEN: &str = "\x1b[32m";

pub(crate) struct ExitSummaryData {
    pub total_session_time: Duration,
    pub code_changes: Option<CodeChangeDelta>,
    pub telemetry: TelemetryStats,
    pub header_context: Option<InlineHeaderContext>,
    pub resume_identifier: Option<String>,
}

pub(crate) fn print_exit_summary(data: ExitSummaryData) {
    let mut model_rows: Vec<(&String, &ModelUsageStats)> =
        data.telemetry.model_usage.iter().collect();
    model_rows.sort_by(|a, b| b.1.api_time.cmp(&a.1.api_time).then_with(|| a.0.cmp(b.0)));

    let mut lines = Vec::new();
    if let Some(context) = data.header_context.as_ref() {
        lines.push(build_header_info_line(context));
    }
    lines.push(format!(
        "API {} | Session {} | Changes {}",
        format_short_duration(data.telemetry.api_time_spent),
        format_long_duration(data.total_session_time),
        format_code_changes(data.code_changes)
    ));
    if data.telemetry.dropped_metric_updates > 0 {
        lines.push(format!(
            "Metrics: dropped {} low-priority updates to keep runtime non-blocking",
            data.telemetry.dropped_metric_updates
        ));
    }
    if let Some(top_model_line) = build_top_model_line(&model_rows) {
        lines.push(top_model_line);
    }

    let title = build_window_title(data.header_context.as_ref());
    println!();
    let rendered_lines = render_terminal_window(&title, &lines, 110);
    println!("{}", style_terminal_window_lines(&rendered_lines));
    if let Some(session_id) = data.resume_identifier {
        println!();
        println!("{ANSI_DIM}Resume this session with:{ANSI_RESET}");
        println!(
            "{ANSI_BOLD}{ANSI_GREEN}vtcode --resume {}{ANSI_RESET}",
            session_id
        );
    }
    println!();
}

fn build_window_title(header_context: Option<&InlineHeaderContext>) -> String {
    let app_name = header_context
        .map(|context| context.app_name.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(ui::HEADER_VERSION_PREFIX);
    let version = header_context
        .map(|context| context.version.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"));
    format!("> {} ({})", app_name, version)
}

fn build_header_info_line(context: &InlineHeaderContext) -> String {
    let provider = capitalize_first_letter(strip_known_prefix(
        &context.provider,
        ui::HEADER_PROVIDER_PREFIX,
    ));
    let model = strip_known_prefix(&context.model, ui::HEADER_MODEL_PREFIX).to_string();
    let reasoning = strip_known_prefix(&context.reasoning, ui::HEADER_REASONING_PREFIX).to_string();
    let trust = format_trust_badge(&context.workspace_trust);
    let session = format_session_label(&context.mode);
    let tools = format_tools_summary(&context.tools);

    let mut first_segment_parts = Vec::new();
    if !is_unavailable(&provider) {
        first_segment_parts.push(provider);
    }
    if !is_unavailable(&model) {
        first_segment_parts.push(model);
    }
    if !is_unavailable(&reasoning) {
        first_segment_parts.push(reasoning);
    }
    let first_segment = if first_segment_parts.is_empty() {
        "Unknown".to_string()
    } else {
        first_segment_parts.join(" ")
    };

    format!("{} | {} | {} | {}", first_segment, trust, session, tools)
}

fn build_top_model_line(model_rows: &[(&String, &ModelUsageStats)]) -> Option<String> {
    let (model, stats) = model_rows.first()?;
    let overflow_count = model_rows.len().saturating_sub(1);
    let overflow_suffix = if overflow_count > 0 {
        format!(" | +{} more", overflow_count)
    } else {
        String::new()
    };

    Some(format!(
        "Top model: {} {} | {} in | {} out | {} cached{}",
        model,
        format_short_duration(stats.api_time),
        format_token_count(stats.prompt_tokens),
        format_token_count(stats.completion_tokens),
        format_token_count(stats.cached_prompt_tokens),
        overflow_suffix
    ))
}

fn format_code_changes(delta: Option<CodeChangeDelta>) -> String {
    match delta {
        Some(delta) => format!("+{} -{}", delta.additions, delta.deletions),
        None => "n/a".to_string(),
    }
}

fn format_long_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn format_short_duration(duration: Duration) -> String {
    let secs = duration.as_secs_f64();
    if secs >= 3600.0 {
        format!("{:.1}h", secs / 3600.0)
    } else if secs >= 60.0 {
        format!("{:.1}m", secs / 60.0)
    } else {
        format!("{:.1}s", secs)
    }
}

fn format_token_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}m", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn strip_known_prefix<'a>(value: &'a str, prefix: &str) -> &'a str {
    value.trim().strip_prefix(prefix).unwrap_or(value).trim()
}

fn is_unavailable(value: &str) -> bool {
    let normalized = value.trim();
    normalized.is_empty()
        || normalized.eq_ignore_ascii_case(ui::HEADER_UNKNOWN_PLACEHOLDER)
        || normalized.eq_ignore_ascii_case("unknown")
}

fn format_trust_badge(trust_value: &str) -> String {
    let trust = strip_known_prefix(trust_value, ui::HEADER_TRUST_PREFIX).to_ascii_lowercase();
    if trust.contains("full auto") || trust.contains("full_auto") {
        "Accept edits".to_string()
    } else if trust.contains("tools policy") || trust.contains("tools_policy") {
        "Safe tools".to_string()
    } else if is_unavailable(&trust) {
        "Trust: n/a".to_string()
    } else {
        format!("Trust: {}", trust.replace('_', " "))
    }
}

fn format_session_label(mode: &str) -> String {
    let normalized = mode.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "std" => "Session: Standard".to_string(),
        "inline" | "inline session" => "Session: Inline".to_string(),
        "auto" | "auto session" => "Session: Auto".to_string(),
        "alt" | "alternate session" => "Session: Alternate".to_string(),
        "" | "unavailable" => "Session: n/a".to_string(),
        _ => {
            if mode.trim().starts_with("Session:") {
                mode.trim().to_string()
            } else {
                format!("Session: {}", mode.trim())
            }
        }
    }
}

fn format_tools_summary(value: &str) -> String {
    let body = strip_known_prefix(value, ui::HEADER_TOOLS_PREFIX);
    if is_unavailable(body) {
        return "Tools: n/a".to_string();
    }

    if let Some(allow_count) = parse_allow_count(body) {
        format!("Tools: {}", allow_count)
    } else {
        format!("Tools: {}", body)
    }
}

fn parse_allow_count(value: &str) -> Option<usize> {
    for part in value.split('·') {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix("allow ")
            && let Some(count) = rest.split_whitespace().next()
            && let Ok(parsed) = count.parse::<usize>()
        {
            return Some(parsed);
        }
    }
    None
}

fn capitalize_first_letter(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn render_terminal_window(title: &str, lines: &[String], max_content_width: usize) -> Vec<String> {
    const MIN_CONTENT_WIDTH: usize = 56;
    let title_len = title.chars().count();
    let body_len = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let content_width = title_len
        .max(body_len)
        .clamp(MIN_CONTENT_WIDTH, max_content_width.max(MIN_CONTENT_WIDTH));
    let clipped_title = clip_to_width(title, content_width);
    let title_padding = content_width.saturating_sub(clipped_title.chars().count());

    let mut out = Vec::with_capacity(lines.len() + 2);
    out.push(format!("╭{}{}╮", clipped_title, "─".repeat(title_padding)));
    for line in lines {
        out.push(render_row(line, content_width));
    }
    out.push(format!("╰{}╯", "─".repeat(content_width)));
    out
}

fn clip_to_width(value: &str, width: usize) -> String {
    let mut out = String::new();
    for ch in value.chars().take(width) {
        out.push(ch);
    }
    out
}

fn render_row(content: &str, content_width: usize) -> String {
    let clipped = clip_to_width(content, content_width);
    let pad = content_width.saturating_sub(clipped.chars().count());
    format!("│{}{}│", clipped, " ".repeat(pad))
}

fn style_terminal_window_lines(lines: &[String]) -> String {
    let mut styled = String::new();
    let last = lines.len().saturating_sub(1);

    for (index, line) in lines.iter().enumerate() {
        let line_render = if index == 0 {
            style_top_line(line)
        } else if index == last {
            color(line, &[ANSI_DIM, ANSI_CYAN])
        } else if line.contains("Session:") && line.contains("Tools:") {
            style_bordered_line(line, &[ANSI_BOLD, ANSI_CYAN])
        } else if line.contains("API ") && line.contains("Changes ") {
            style_bordered_line(line, &[ANSI_BOLD])
        } else if line.contains("Top model:") {
            style_bordered_line(line, &[ANSI_GREEN])
        } else if line.contains("Metrics: dropped") {
            style_bordered_line(line, &[ANSI_DIM])
        } else {
            style_bordered_line(line, &[])
        };

        styled.push_str(&line_render);
        if index < last {
            styled.push('\n');
        }
    }

    styled
}

fn style_top_line(line: &str) -> String {
    let Some(body) = line
        .strip_prefix('╭')
        .and_then(|value| value.strip_suffix('╮'))
    else {
        return color(line, &[ANSI_DIM, ANSI_CYAN]);
    };
    let split_index = body.find('─').unwrap_or(body.len());
    let (title, tail) = body.split_at(split_index);

    format!(
        "{ANSI_DIM}{ANSI_CYAN}╭{ANSI_RESET}{ANSI_BOLD}{ANSI_CYAN}{title}{ANSI_RESET}{ANSI_DIM}{ANSI_CYAN}{tail}╮{ANSI_RESET}"
    )
}

fn color(value: &str, styles: &[&str]) -> String {
    let mut out = String::new();
    for style in styles {
        out.push_str(style);
    }
    out.push_str(value);
    out.push_str(ANSI_RESET);
    out
}

fn style_bordered_line(line: &str, content_styles: &[&str]) -> String {
    let Some(inner) = line
        .strip_prefix('│')
        .and_then(|value| value.strip_suffix('│'))
    else {
        return color(line, &[ANSI_DIM, ANSI_CYAN]);
    };

    let mut out = String::new();
    out.push_str(ANSI_DIM);
    out.push_str(ANSI_CYAN);
    out.push('│');
    out.push_str(ANSI_RESET);

    for style in content_styles {
        out.push_str(style);
    }
    out.push_str(inner);
    out.push_str(ANSI_RESET);

    out.push_str(ANSI_DIM);
    out.push_str(ANSI_CYAN);
    out.push('│');
    out.push_str(ANSI_RESET);
    out
}

#[cfg(test)]
mod tests {
    use super::{
        format_long_duration, format_session_label, format_short_duration, format_token_count,
        parse_allow_count, render_terminal_window,
    };
    use std::time::Duration;

    #[test]
    fn duration_formatters_match_expected_units() {
        assert_eq!(format_long_duration(Duration::from_secs(55)), "55s");
        assert_eq!(format_long_duration(Duration::from_secs(95)), "1m 35s");
        assert_eq!(format_long_duration(Duration::from_secs(3670)), "1h 1m 10s");

        assert_eq!(format_short_duration(Duration::from_secs(30)), "30.0s");
        assert_eq!(format_short_duration(Duration::from_secs(600)), "10.0m");
        assert_eq!(format_short_duration(Duration::from_secs(10_800)), "3.0h");
    }

    #[test]
    fn token_formatter_supports_compact_suffixes() {
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(12_345), "12.3k");
        assert_eq!(format_token_count(8_900_000), "8.9m");
    }

    #[test]
    fn mode_labels_are_compact_and_readable() {
        assert_eq!(format_session_label("std"), "Session: Standard");
        assert_eq!(format_session_label("inline"), "Session: Inline");
        assert_eq!(
            format_session_label("alternate session"),
            "Session: Alternate"
        );
    }

    #[test]
    fn allow_count_parser_reads_tools_summary() {
        assert_eq!(parse_allow_count("allow 66 · prompt 1 · deny 0"), Some(66));
        assert_eq!(parse_allow_count("prompt 1 · deny 0"), None);
    }

    #[test]
    fn terminal_window_uses_vtcode_title_prefix() {
        let rows = vec!["API 2.0s | Session 39s | Changes +1 -0".to_string()];
        let rendered = render_terminal_window("> VT Code (0.84.1)", &rows, 96);
        assert!(rendered[0].starts_with("╭> VT Code (0.84.1)"));
    }
}
