use std::time::Duration;
use vtcode_core::config::constants::ui;
use vtcode_core::core::telemetry::{ModelUsageStats, TelemetryStats};
use vtcode_tui::InlineHeaderContext;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_MAGENTA: &str = "\x1b[35m";
const ANSI_YELLOW: &str = "\x1b[33m";

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

    let total_prompt_tokens: u64 = data
        .telemetry
        .model_usage
        .values()
        .map(|s| s.prompt_tokens + s.cached_prompt_tokens)
        .sum();
    let total_completion_tokens: u64 = data
        .telemetry
        .model_usage
        .values()
        .map(|s| s.completion_tokens)
        .sum();

    let total_diff = data
        .code_changes
        .as_ref()
        .map(|d| d.additions + d.deletions)
        .unwrap_or(0);

    let mut lines = Vec::new();
    if let Some(context) = data.header_context.as_ref() {
        lines.push(build_compact_header(context));
    }
    lines.push(format!(
        "Session {} | API {} | {} in/{} out | {} diff",
        format_long_duration(data.total_session_time),
        format_short_duration(data.telemetry.api_time_spent),
        format_token_count(total_prompt_tokens),
        format_token_count(total_completion_tokens),
        total_diff
    ));
    if let Some(top_model_line) = build_top_model_line(&model_rows) {
        lines.push(top_model_line);
    }

    let title = build_window_title(data.header_context.as_ref());
    println!();
    let rendered_lines = render_terminal_window(&title, &lines, 110);
    println!("{}", style_terminal_window_lines(&rendered_lines));
    if let Some(session_id) = data.resume_identifier {
        println!(
            "{ANSI_DIM}Resume:{ANSI_RESET} {ANSI_DIM}{ANSI_GREEN}vtcode --resume {}{ANSI_RESET}",
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

fn build_compact_header(context: &InlineHeaderContext) -> String {
    let provider = capitalize_first_letter(strip_known_prefix(
        &context.provider,
        ui::HEADER_PROVIDER_PREFIX,
    ));
    let model = strip_known_prefix(&context.model, ui::HEADER_MODEL_PREFIX);
    let reasoning = strip_known_prefix(&context.reasoning, ui::HEADER_REASONING_PREFIX);
    let trust = format_trust_badge(&context.workspace_trust);

    let mut parts = Vec::new();
    if !is_unavailable(&provider) {
        parts.push(provider);
    }
    if !is_unavailable(model) {
        parts.push(model.to_string());
    }
    if !is_unavailable(reasoning) {
        parts.push(reasoning.to_string());
    }
    if parts.is_empty() {
        parts.push("Unknown".to_string());
    }

    format!("{} | {}", parts.join(" "), trust)
}

fn build_top_model_line(model_rows: &[(&String, &ModelUsageStats)]) -> Option<String> {
    let (model, stats) = model_rows.first()?;
    Some(format!(
        "Model: {} | {} | {} in/{} out",
        model,
        format_short_duration(stats.api_time),
        format_token_count(stats.prompt_tokens + stats.cached_prompt_tokens),
        format_token_count(stats.completion_tokens)
    ))
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

fn capitalize_first_letter(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn render_terminal_window(title: &str, lines: &[String], _max_content_width: usize) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len() + 1);
    out.push(title.to_string());
    for line in lines {
        out.push(line.clone());
    }
    out
}

fn style_terminal_window_lines(lines: &[String]) -> String {
    let mut styled = String::new();
    let last = lines.len().saturating_sub(1);

    for (index, line) in lines.iter().enumerate() {
        let line_render = if index == 0 {
            style_title_line(line)
        } else {
            style_dim_line(line)
        };

        styled.push_str(&line_render);
        if index < last {
            styled.push('\n');
        }
    }

    styled
}

fn style_title_line(line: &str) -> String {
    format!("{ANSI_BOLD}{ANSI_CYAN}{line}{ANSI_RESET}")
}

fn style_dim_line(line: &str) -> String {
    format!("{ANSI_DIM}{line}{ANSI_RESET}")
}

#[cfg(test)]
mod tests {
    use super::{format_long_duration, format_short_duration, format_token_count, render_terminal_window};
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
    fn terminal_window_returns_title_and_lines() {
        let rows = vec![
            "Ollama gpt-oss | Accept edits".to_string(),
            "Session 1m | API 4s | 28k in/239 out".to_string(),
            "Model: gpt-oss 4s | 28k in/239 out".to_string(),
        ];
        let rendered = render_terminal_window("> VT Code (0.85.1)", &rows, 110);
        assert_eq!(rendered[0], "> VT Code (0.85.1)");
        assert_eq!(rendered.len(), 4);
    }
}
