use crate::agent::runloop::git::CodeChangeDelta;
use std::time::Duration;
use vtcode_commons::color256_theme::rgb_to_ansi256_for_theme;
use vtcode_core::config::constants::ui;
use vtcode_core::core::telemetry::TelemetryStats;
use vtcode_tui::InlineHeaderContext;
use vtcode_tui::ui::theme;

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";

pub(crate) struct ExitSummaryData {
    pub total_session_time: Duration,
    pub code_changes: Option<CodeChangeDelta>,
    pub telemetry: TelemetryStats,
    pub header_context: Option<InlineHeaderContext>,
    pub resume_identifier: Option<String>,
}

pub(crate) fn print_exit_summary(data: ExitSummaryData) {
    let (prompt, completion, cache_read, cache_creation) =
        aggregate_tokens(&data.telemetry.model_usage);
    let diff = data.code_changes.unwrap_or_default();
    let models = sorted_models(&data.telemetry.model_usage);

    let current_theme = theme::active_theme_id();
    let is_light = theme::is_light_theme(&current_theme);

    // Ciapre theme colors (RGB)
    const TITLE_RGB: (u8, u8, u8) = (0xAE, 0xA4, 0x7F); // primary_accent
    const MODEL_RGB: (u8, u8, u8) = (0xCC, 0x8A, 0x3E); // secondary_accent (yellow/gold)
    const RESUME_RGB: (u8, u8, u8) = (0x98, 0xBB, 0x74); // green variant

    let title_color = rgb_to_ansi256_for_theme(TITLE_RGB.0, TITLE_RGB.1, TITLE_RGB.2, is_light);
    let model_color = rgb_to_ansi256_for_theme(MODEL_RGB.0, MODEL_RGB.1, MODEL_RGB.2, is_light);
    let resume_color = rgb_to_ansi256_for_theme(RESUME_RGB.0, RESUME_RGB.1, RESUME_RGB.2, is_light);

    let title = build_title(&data.header_context);
    println!();
    println!("{ANSI_BOLD}\x1b[38;5;{title_color}m{title}{ANSI_RESET}");

    if let Some(ctx) = &data.header_context {
        println!("{ANSI_DIM}{}{ANSI_RESET}", build_trust_line(ctx));
    }

    println!(
        "{ANSI_DIM}Session {} | API {} | Tokens {} in / {} out{} | Code +{} / -{}{ANSI_RESET}",
        format_duration(data.total_session_time),
        format_duration(data.telemetry.api_time_spent),
        format_number(prompt),
        format_number(completion),
        format_cache_fragment(cache_read, cache_creation),
        diff.additions,
        diff.deletions
    );

    for (model, stats) in models {
        println!(
            "{ANSI_DIM}Model: {ANSI_BOLD}\x1b[38;5;{model_color}m{model}{ANSI_RESET}{ANSI_DIM} | {} | {} in / {} out{}{}{ANSI_RESET}",
            format_duration(stats.api_time),
            format_number(stats.prompt_tokens),
            format_number(stats.completion_tokens),
            format_cache_fragment(stats.cache_read_tokens, stats.cache_creation_tokens),
            format_cache_hit_ratio(stats.cache_read_tokens, stats.cache_creation_tokens),
        );
    }

    if let Some(session_id) = data.resume_identifier {
        println!(
            "{ANSI_DIM}Resume: \x1b[38;5;{resume_color}mvtcode --resume {session_id}{ANSI_RESET}"
        );
    }
    println!();
}

fn aggregate_tokens(
    usage: &hashbrown::HashMap<String, vtcode_core::core::telemetry::ModelUsageStats>,
) -> (u64, u64, u64, u64) {
    usage.values().fold((0, 0, 0, 0), |(p, c, r, w), s| {
        (
            p + s.prompt_tokens,
            c + s.completion_tokens,
            r + s.cache_read_tokens,
            w + s.cache_creation_tokens,
        )
    })
}

fn sorted_models(
    usage: &hashbrown::HashMap<String, vtcode_core::core::telemetry::ModelUsageStats>,
) -> Vec<(&String, &vtcode_core::core::telemetry::ModelUsageStats)> {
    let mut models: Vec<_> = usage.iter().collect();
    models.sort_by_key(|(_, stats)| std::cmp::Reverse(stats.api_time));
    models
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

fn format_cache_fragment(cache_read: u64, cache_creation: u64) -> String {
    if cache_read == 0 && cache_creation == 0 {
        String::new()
    } else {
        format!(
            " | Cache {} read / {} write",
            format_number(cache_read),
            format_number(cache_creation)
        )
    }
}

fn format_cache_hit_ratio(cache_read: u64, cache_creation: u64) -> String {
    let total = cache_read + cache_creation;
    if total == 0 {
        String::new()
    } else {
        format!(
            " | Hit {:>5.1}%",
            (cache_read as f64 / total as f64) * 100.0
        )
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

    #[test]
    fn formats_cache_metrics() {
        assert_eq!(format_cache_fragment(0, 0), "");
        assert_eq!(
            format_cache_fragment(12_000, 3_000),
            " | Cache 12.0k read / 3.0k write"
        );
        assert_eq!(format_cache_hit_ratio(0, 0), "");
        assert_eq!(format_cache_hit_ratio(9, 1), " | Hit  90.0%");
    }
}
