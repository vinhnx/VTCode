use std::borrow::Cow;
use std::time::Duration;
use vtcode_commons::ansi_codes::{BOLD, DIM, RESET, fg_256};
use vtcode_commons::color256_theme::rgb_to_ansi256_for_theme;
use vtcode_ui::tui::ui::theme;

/// Zero-allocation exit data — all borrowed, no clones.
/// All token fields come from the ATIF builder (already computed during session).
/// Zero-valued fields are omitted from display.
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
        stats.push(format!("Cache {} read", format_number(data.cached_tokens)));
    }

    if data.code_additions > 0 || data.code_deletions > 0 {
        stats.push(format!(
            "Code +{} / -{}",
            data.code_additions, data.code_deletions,
        ));
    }

    if stats.len() > 1 {
        println!("{DIM}{}{RESET}", stats.join(" | "));
    } else {
        println!("{DIM}{}{RESET}", stats[0]);
    }

    if let Some((max_budget_usd, actual_cost_usd)) = data.budget_limit {
        println!("{DIM}Budget at ${actual_cost_usd:.2} / ${max_budget_usd:.2}{RESET}",);
    }

    if let Some(session_id) = data.resume_identifier {
        println!("{DIM}Resume: {resume_style}vtcode --resume {session_id}{RESET}");
    }

    println!();
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
}
