use crate::agent::runloop::git::CodeChangeDelta;
use std::time::Duration;
use vtcode_core::config::constants::ui;
use vtcode_core::core::telemetry::TelemetryStats;
use vtcode_tui::InlineHeaderContext;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_MAGENTA: &str = "\x1b[35m";

pub(crate) struct ExitSummaryData {
    pub total_session_time: Duration,
    pub code_changes: Option<CodeChangeDelta>,
    pub telemetry: TelemetryStats,
    pub header_context: Option<InlineHeaderContext>,
    pub resume_identifier: Option<String>,
}

pub(crate) fn print_exit_summary(data: ExitSummaryData) {
    let (prompt, completion) = aggregate_tokens(&data.telemetry.model_usage);
    let diff = data.code_changes.unwrap_or_default();
    let top_model = top_model(&data.telemetry.model_usage);

    let title = build_title(&data.header_context);
    println!();
    println!("{ANSI_BOLD}{ANSI_CYAN}{title}{ANSI_RESET}");

    if let Some(ctx) = &data.header_context {
        println!("{ANSI_DIM}{}{ANSI_RESET}", build_trust_line(ctx));
    }

    println!(
        "{ANSI_DIM}Session {} | API {} | Tokens {} in / {} out | Code +{} / -{}{ANSI_RESET}",
        format_duration(data.total_session_time),
        format_duration(data.telemetry.api_time_spent),
        format_number(prompt),
        format_number(completion),
        diff.additions,
        diff.deletions
    );

    if let Some((model, stats)) = top_model {
        let p = stats.prompt_tokens + stats.cached_prompt_tokens;
        println!(
            "{ANSI_DIM}Model: {ANSI_BOLD}{ANSI_MAGENTA}{model}{ANSI_RESET}{ANSI_DIM} | {} | {} in / {} out{ANSI_RESET}",
            format_duration(stats.api_time),
            format_number(p),
            format_number(stats.completion_tokens)
        );
    }

    if let Some(session_id) = data.resume_identifier {
        println!("{ANSI_DIM}Resume: {ANSI_GREEN}vtcode --resume {session_id}{ANSI_RESET}");
    }
    println!();
}

fn aggregate_tokens(
    usage: &std::collections::HashMap<String, vtcode_core::core::telemetry::ModelUsageStats>,
) -> (u64, u64) {
    usage.values().fold((0, 0), |(p, c), s| {
        (
            p + s.prompt_tokens + s.cached_prompt_tokens,
            c + s.completion_tokens,
        )
    })
}

fn top_model(
    usage: &std::collections::HashMap<String, vtcode_core::core::telemetry::ModelUsageStats>,
) -> Option<(&String, &vtcode_core::core::telemetry::ModelUsageStats)> {
    usage.iter().max_by_key(|(_, s)| s.api_time)
}

fn build_title(ctx: &Option<InlineHeaderContext>) -> String {
    let app = ctx
        .as_ref()
        .map(|c| c.app_name.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(ui::HEADER_VERSION_PREFIX);
    let ver = ctx
        .as_ref()
        .map(|c| c.version.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"));
    format!("> {app} ({ver})")
}

fn build_trust_line(ctx: &InlineHeaderContext) -> String {
    let trust = ctx
        .workspace_trust
        .trim()
        .strip_prefix(ui::HEADER_TRUST_PREFIX)
        .unwrap_or(&ctx.workspace_trust)
        .trim();
    let trust = trust.to_ascii_lowercase();
    if trust.contains("full auto") {
        "Accept edits".into()
    } else if trust.contains("tools policy") {
        "Safe tools".into()
    } else if trust.is_empty() || trust == "unknown" {
        "Trust: n/a".into()
    } else {
        format!("Trust: {}", trust.replace('_', " "))
    }
}

fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let sec = s % 60;
    if h > 0 {
        format!("{h}h {m}m {sec}s")
    } else if m > 0 {
        format!("{m}m {sec}s")
    } else {
        format!("{sec}s")
    }
}

fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}m", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_duration() {
        assert_eq!(format_duration(Duration::from_secs(55)), "55s");
        assert_eq!(format_duration(Duration::from_secs(95)), "1m 35s");
        assert_eq!(format_duration(Duration::from_secs(3670)), "1h 1m 10s");
    }

    #[test]
    fn formats_numbers() {
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(12_345), "12.3k");
        assert_eq!(format_number(8_900_000), "8.9m");
    }
}
