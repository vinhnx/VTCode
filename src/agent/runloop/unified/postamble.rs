use std::borrow::Cow;
use std::time::Duration;
use vtcode_commons::ansi_codes::{BOLD, DIM, RESET, fg_256};
use vtcode_commons::color256_theme::rgb_to_ansi256_for_theme;
use vtcode_ui::tui::ui::theme;

/// Zero-allocation exit data — all borrowed, no clones.
/// All token fields (`prompt_tokens`, `completion_tokens`, `cached_tokens`,
/// `cache_creation_tokens`, `cache_hit_rate_percent`) are sourced from the
/// same `session_stats.total_usage()` snapshot so they share one normalized
/// basis. Zero-valued fields are omitted from display.
pub(crate) struct ExitData<'a> {
    pub app_name: &'static str,
    pub version: &'static str,
    pub model: &'a str,
    pub provider: &'a str,
    pub trust_label: &'a str,
    pub reasoning: &'a str,
    pub session_duration: Duration,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub cached_tokens: u64,
    /// Cache-creation (cache-write) tokens accumulated this session.
    pub cache_creation_tokens: u64,
    /// Cache hit rate as a percentage (0-100), when at least one input token
    /// has been recorded this session.
    pub cache_hit_rate_percent: Option<f64>,
    pub code_additions: u64,
    pub code_deletions: u64,
    pub resume_identifier: Option<&'a str>,
    pub budget_limit: Option<(f64, f64)>,
}

pub(crate) fn print_exit_summary(data: ExitData<'_>) {
    let is_light = theme::is_light_theme(&theme::active_theme_id());

    const TITLE_RGB: (u8, u8, u8) = (0xAE, 0xA4, 0x7F);
    const MODEL_RGB: (u8, u8, u8) = (0xCC, 0x8A, 0x3E);
    const RESUME_RGB: (u8, u8, u8) = (0x98, 0xBB, 0x74);

    let title_color = rgb_to_ansi256_for_theme(TITLE_RGB.0, TITLE_RGB.1, TITLE_RGB.2, is_light);
    let model_color = rgb_to_ansi256_for_theme(MODEL_RGB.0, MODEL_RGB.1, MODEL_RGB.2, is_light);
    let resume_color = rgb_to_ansi256_for_theme(RESUME_RGB.0, RESUME_RGB.1, RESUME_RGB.2, is_light);
    let title_style = fg_256(title_color);
    let model_style = fg_256(model_color);
    let resume_style = fg_256(resume_color);

    println!();
    println!(
        "{BOLD}{title_style}> {} ({}){RESET}",
        data.app_name, data.version
    );

    let trust = build_trust_label(data.trust_label);
    if !trust.is_empty() {
        println!("{DIM}{trust}{RESET}");
    }

    print_model_line(data.model, data.provider, data.reasoning, &model_style);

    let stats_line = build_stats_line(&data);
    println!("{DIM}{stats_line}{RESET}");

    if let Some((max_budget_usd, actual_cost_usd)) = data.budget_limit {
        println!("{DIM}Budget at ${actual_cost_usd:.2} / ${max_budget_usd:.2}{RESET}",);
    }

    if let Some(session_id) = data.resume_identifier {
        println!("{DIM}Resume: {resume_style}vtcode --resume {session_id}{RESET}");
    }

    println!();
}

/// Builds the pipe-delimited exit stats line (session duration, token
/// counts, cache read/creation/hit-rate, code diff) with no ANSI styling, so
/// it stays independently testable. `print_exit_summary` wraps the result in
/// the dim/reset styling used for the rest of the exit summary.
///
/// Takes `&ExitData` rather than individual fields since every value it
/// needs already lives on that struct — a single reference avoids an
/// eight-parameter call.
fn build_stats_line(data: &ExitData<'_>) -> String {
    let mut stats = Vec::new();
    stats.push(format!(
        "Session {}",
        format_duration(data.session_duration)
    ));

    if data.prompt_tokens > 0 || data.completion_tokens > 0 {
        stats.push(format!(
            "{} in / {} out",
            format_number(data.prompt_tokens),
            format_number(data.completion_tokens),
        ));
    }

    if data.cached_tokens > 0 {
        let mut cache_stat = format!("Cache {} read", format_number(data.cached_tokens));
        if let Some(hit_rate) = data.cache_hit_rate_percent {
            cache_stat.push_str(&format!(" ({hit_rate:.1}% hit rate)"));
        }
        if data.cache_creation_tokens > 0 {
            cache_stat.push_str(&format!(
                ", {} creation",
                format_number(data.cache_creation_tokens)
            ));
        }
        stats.push(cache_stat);
    }

    if data.code_additions > 0 || data.code_deletions > 0 {
        stats.push(format!(
            "Code +{} / -{}",
            data.code_additions, data.code_deletions
        ));
    }

    stats.join(" | ")
}

fn print_model_line(model: &str, provider: &str, reasoning: &str, model_style: &str) {
    let model = model.trim();
    let provider = provider.trim();
    let reasoning = reasoning.trim();

    let show_model = !model.is_empty();
    let show_provider = !provider.is_empty();
    let show_reasoning = !reasoning.is_empty();

    let mut line = match (show_model, show_provider) {
        (true, true) => format!("Model: {BOLD}{model_style}{model}{RESET}{DIM} via {provider}"),
        (true, false) => format!("Model: {BOLD}{model_style}{model}{RESET}"),
        (false, true) => format!("Provider: {provider}"),
        (false, false) => String::new(),
    };

    if show_reasoning {
        let suffix = format!(" · {reasoning}");
        if line.is_empty() {
            line = format!("Reasoning:{suffix}");
        } else {
            line.push_str(&suffix);
        }
    }

    if !line.is_empty() {
        println!("{DIM}{line}{RESET}");
    }
}

/// Returns a borrowed trust label — empty string if unknown, no allocation.
fn build_trust_label(trust_label: &str) -> Cow<'static, str> {
    let t = trust_label.trim().to_ascii_lowercase().replace('_', " ");
    if t.contains("full auto") {
        Cow::Borrowed("Full-auto trust")
    } else if t.contains("tools policy") {
        Cow::Borrowed("Safe tools")
    } else if t.is_empty() || t == "unknown" {
        Cow::Borrowed("")
    } else {
        Cow::Owned(format!("Trust: {t}"))
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
    fn trust_line_full_auto() {
        assert_eq!(build_trust_label("full auto").as_ref(), "Full-auto trust");
        assert_eq!(build_trust_label("full_auto").as_ref(), "Full-auto trust");
    }

    #[test]
    fn trust_line_safe_tools() {
        assert_eq!(build_trust_label("tools policy").as_ref(), "Safe tools");
        assert_eq!(build_trust_label("tools_policy").as_ref(), "Safe tools");
    }

    #[test]
    fn trust_line_empty_or_unknown() {
        assert_eq!(build_trust_label("").as_ref(), "");
        assert_eq!(build_trust_label("unknown").as_ref(), "");
    }

    #[test]
    fn trust_line_fallback() {
        assert_eq!(
            build_trust_label("some_other").as_ref(),
            "Trust: some other"
        );
    }

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

    /// Builds an `ExitData` with only the stats-line-relevant fields set;
    /// the rest are dummy values `build_stats_line` never reads.
    fn stats_test_data(
        session_duration: Duration,
        prompt_tokens: u64,
        completion_tokens: u64,
        cached_tokens: u64,
        cache_creation_tokens: u64,
        cache_hit_rate_percent: Option<f64>,
        code_additions: u64,
        code_deletions: u64,
    ) -> ExitData<'static> {
        ExitData {
            app_name: "VT Code",
            version: "0.0.0",
            model: "",
            provider: "",
            trust_label: "",
            reasoning: "",
            session_duration,
            prompt_tokens,
            completion_tokens,
            cached_tokens,
            cache_creation_tokens,
            cache_hit_rate_percent,
            code_additions,
            code_deletions,
            resume_identifier: None,
            budget_limit: None,
        }
    }

    #[test]
    fn stats_line_with_zero_input_omits_token_and_cache_segments() {
        let data = stats_test_data(Duration::from_secs(30), 0, 0, 0, 0, None, 0, 0);
        let line = build_stats_line(&data);
        assert_eq!(line, "Session 30s");
    }

    #[test]
    fn stats_line_with_cache_includes_hit_rate_and_creation_tokens() {
        let data = stats_test_data(
            Duration::from_secs(95),
            1_000,
            200,
            800,
            50,
            Some(80.0),
            10,
            2,
        );
        let line = build_stats_line(&data);
        assert_eq!(
            line,
            "Session 1m 35s | 1.0k in / 200 out | Cache 800 read (80.0% hit rate), 50 creation | Code +10 / -2"
        );
    }

    #[test]
    fn stats_line_with_cache_but_no_creation_omits_creation_segment() {
        let data = stats_test_data(Duration::from_secs(10), 500, 100, 400, 0, Some(80.0), 0, 0);
        let line = build_stats_line(&data);
        assert_eq!(
            line,
            "Session 10s | 500 in / 100 out | Cache 400 read (80.0% hit rate)"
        );
    }
}
