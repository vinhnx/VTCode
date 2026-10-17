use anstyle::{AnsiColor, Effects};
use anyhow::{Context, Result};
use clap::builder::Styles;
use clap::builder::styling::{AnsiColor as ClapAnsiColor, Effects as ClapEffects};
use clap::{ColorChoice as CliColorChoice, CommandFactory};
use std::time::Instant;
use vtcode_commons::color_policy::{self, ColorOutputPolicy, ColorOutputPolicySource};
use vtcode_core::cli::args::Cli;

use crate::startup::StartupContext;

pub(crate) fn debug_runtime_flag_enabled(debug_arg_enabled: bool, env_var: &str) -> bool {
    cfg!(debug_assertions) && (debug_arg_enabled || vtcode_commons::utils::env_flag_enabled(env_var))
}

pub(crate) fn resolve_runtime_color_policy(args: &Cli) -> ColorOutputPolicy {
    if args.no_color {
        return ColorOutputPolicy {
            enabled: false,
            source: ColorOutputPolicySource::CliNoColor,
        };
    }

    match args.color.color {
        CliColorChoice::Always => ColorOutputPolicy {
            enabled: true,
            source: ColorOutputPolicySource::CliColorAlways,
        },
        CliColorChoice::Never => ColorOutputPolicy {
            enabled: false,
            source: ColorOutputPolicySource::CliColorNever,
        },
        CliColorChoice::Auto => {
            if color_policy::no_color_env_active() {
                ColorOutputPolicy {
                    enabled: false,
                    source: ColorOutputPolicySource::NoColorEnv,
                }
            } else {
                ColorOutputPolicy {
                    enabled: true,
                    source: ColorOutputPolicySource::DefaultAuto,
                }
            }
        }
    }
}

pub(crate) fn build_augmented_cli_command() -> clap::Command {
    let mut cmd = Cli::command();
    if let Some(choice) = requested_help_color_choice() {
        cmd = cmd.color(choice);
    }
    cmd = cmd.styles(clap_help_styles());
    cmd = cmd.before_help(QUICK_START_HELP);

    // `long_version()` resolves storage paths and formats environment state —
    // pure overhead for interactive launches that never print `--version`.
    // Compute the full diagnostics string only when a version flag is present;
    // otherwise expose the cheap crate version. Both branches are `'static`
    // without per-launch `Box::leak` churn on the hot path.
    let version_leak: &'static str = if argv_requests_version() {
        Box::leak(vtcode_core::cli::args::long_version().into_boxed_str())
    } else {
        env!("CARGO_PKG_VERSION")
    };
    cmd = cmd.long_version(version_leak);

    let after_help = "\nSlash commands (type / in chat):\n  /init     - Guided AGENTS.md + workspace setup\n  /config   - Browse settings sections and workspace panels (memory, model, checkup, tasks, jobs, log, ...)\n  /status   - Show current configuration\n  /model    - Switch model and reasoning effort\n  /update   - Check for VT Code updates, or run `vtcode update` from the CLI (use --list, --pin, --channel)\n  /plan     - Start or finish the Planning workflow\n  /theme    - Switch UI theme\n  /title    - Configure terminal title items\n  /history  - Open command history picker\n  /help     - Show all slash commands\n\nTips:\n  Mistyped commands show suggestions (e.g., vtcode ch -> chat).\n  Use --continue to resume the most recent session.\n  Use --resume to pick a session interactively.";
    cmd.after_help(after_help)
}

fn clap_help_styles() -> Styles {
    Styles::styled()
        .header(ClapAnsiColor::BrightBlue.on_default().effects(ClapEffects::BOLD))
        .usage(ClapAnsiColor::BrightBlue.on_default().effects(ClapEffects::BOLD))
        .literal(ClapAnsiColor::BrightGreen.on_default().effects(ClapEffects::BOLD))
        .placeholder(ClapAnsiColor::BrightCyan.on_default())
}

fn requested_help_color_choice() -> Option<CliColorChoice> {
    let mut requested = None;
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        if arg == "--no-color" {
            requested = Some(CliColorChoice::Never);
            continue;
        }

        if let Some(value) = arg.strip_prefix("--color=") {
            if let Some(choice) = parse_help_color_choice(value) {
                requested = Some(choice);
            }
            continue;
        }

        if arg == "--color"
            && let Some(value) = args.next()
            && let Some(choice) = parse_help_color_choice(&value)
        {
            requested = Some(choice);
        }
    }

    requested
}

const QUICK_START_HELP: &str = "\
Quick start:\n\
  1. Set a provider API key (for example OPENAI_API_KEY, ANTHROPIC_API_KEY, or GEMINI_API_KEY)\n\
  2. Start interactive chat: vtcode chat --provider openai --model gpt-5\n\
  3. Run one prompt directly: vtcode --print=\"summarize this repository\"\n\n\
Use `vtcode <command> --help` for command-specific details.";
fn parse_help_color_choice(value: &str) -> Option<CliColorChoice> {
    match value.trim().to_ascii_lowercase().as_str() {
        "always" => Some(CliColorChoice::Always),
        "auto" => Some(CliColorChoice::Auto),
        "never" => Some(CliColorChoice::Never),
        _ => None,
    }
}

fn argv_requests_version() -> bool {
    args_request_version(std::env::args_os().skip(1))
}

fn args_request_version(args: impl IntoIterator<Item = std::ffi::OsString>) -> bool {
    args.into_iter().any(|arg| {
        let text = arg.to_string_lossy();
        // Cover `-V`, `--version`, and combined short flags like `-VV`.
        // Values after `=` (e.g. `--version=x`) still count as a version request
        // so diagnostics stay complete whenever clap would print them.
        text == "-V"
            || text == "--version"
            || text.starts_with("--version=")
            || (text.starts_with('-') && !text.starts_with("--") && text.contains('V'))
    })
}

pub(crate) async fn resolve_startup_context(args: &Cli) -> Result<StartupContext> {
    let startup_start = Instant::now();
    let startup_phase = vtcode_commons::startup_trace::phase_started();
    let startup = StartupContext::from_cli_args(args)
        .await
        .context("failed to initialize VT Code startup context")?;
    let startup_ms = startup_start.elapsed().as_millis() as u64;
    tracing::debug!(target = "vtcode.startup", startup_ms, "startup context resolved");
    vtcode_commons::startup_trace::record_phase("startup_context_resolution", startup_phase);
    Ok(startup)
}

// ── "Did you mean?" suggestions for unrecognized CLI input ──────────────────

/// Well-known global flag names that users might type without the `--` prefix.
const GLOBAL_FLAG_CANDIDATES: &[&str] = &[
    "--continue",
    "--resume",
    "--full-auto",
    "--fork-session",
    "--agent",
    "--model",
    "--provider",
    "--print",
    "--debug",
    "--quiet",
    "--color",
    "--no-color",
    "--help",
    "--version",
];

use std::collections::HashSet;
use std::sync::OnceLock;

/// Cached suggestion candidates (subcommand names + aliases + global flags).
/// Built once on first use; the error path is rare so the cost is negligible.
fn cached_candidates() -> &'static Vec<String> {
    static CANDIDATES: OnceLock<Vec<String>> = OnceLock::new();
    CANDIDATES.get_or_init(build_candidate_list)
}

fn build_candidate_list() -> Vec<String> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();

    for cmd in Cli::command().get_subcommands() {
        let name = cmd.get_name().to_string();
        if seen.insert(name.clone()) {
            candidates.push(name);
        }
        for alias in cmd.get_visible_aliases() {
            let alias = alias.to_string();
            if seen.insert(alias.clone()) {
                candidates.push(alias);
            }
        }
    }

    for flag in GLOBAL_FLAG_CANDIDATES {
        let flag = (*flag).to_string();
        if seen.insert(flag.clone()) {
            candidates.push(flag);
        }
    }

    candidates
}

/// Apply ANSI styling to text if the terminal supports color output.
///
/// Uses `colorchoice::ColorChoice::global()` to respect `--color`, `--no-color`,
/// and `NO_COLOR` env var. Falls back to raw text when colors are disabled.
fn styled(text: &str, style: anstyle::Style) -> String {
    if colorchoice::ColorChoice::global() == colorchoice::ColorChoice::Never {
        return text.to_string();
    }
    format!("{style}{text}\x1b[0m")
}

fn style_header(text: &str) -> String {
    styled(
        text,
        anstyle::Style::new()
            .fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightBlue)))
            .effects(Effects::BOLD),
    )
}

fn style_literal(text: &str) -> String {
    styled(
        text,
        anstyle::Style::new()
            .fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightGreen)))
            .effects(Effects::BOLD),
    )
}

fn style_placeholder(text: &str) -> String {
    styled(text, anstyle::Style::new().fg_color(Some(anstyle::Color::Ansi(AnsiColor::BrightCyan))))
}

/// Score how similar `candidate` is to `input` (0.0 ..= 1.0, exclusive).
///
/// Uses prefix matching, substring containment, and character bigram overlap.
/// Returns values strictly below 1.0 for non-exact matches so that
/// `suggest_similar_commands` never filters out a valid prefix match.
fn similarity_score(input: &str, candidate: &str) -> f64 {
    if input == candidate {
        return 1.0;
    }

    let input_lower = input.to_ascii_lowercase();
    let cand_lower = candidate.trim_start_matches('-').to_ascii_lowercase();

    if cand_lower == input_lower {
        return 1.0;
    }

    // Prefix match (strong signal for CLIs) — clamp below 1.0
    if cand_lower.starts_with(&input_lower) {
        let ratio = input_lower.len() as f64 / cand_lower.len() as f64;
        return 0.8 + 0.15 * ratio;
    }

    // Input is prefix of candidate
    if input_lower.starts_with(&cand_lower) {
        let ratio = cand_lower.len() as f64 / input_lower.len() as f64;
        return 0.7 + 0.25 * ratio;
    }

    // Substring containment
    if cand_lower.contains(&input_lower) {
        return 0.6;
    }
    if input_lower.contains(&cand_lower) {
        return 0.55;
    }

    // Character bigram overlap (fuzzy matching)
    bigram_overlap(&input_lower, &cand_lower)
}

fn bigram_overlap(a: &str, b: &str) -> f64 {
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }

    let a_bigrams: HashSet<[u8; 2]> = a.as_bytes().windows(2).map(|w| [w[0], w[1]]).collect();
    let b_bigrams: Vec<[u8; 2]> = b.as_bytes().windows(2).map(|w| [w[0], w[1]]).collect();

    let matches = b_bigrams.iter().filter(|bg| a_bigrams.contains(&**bg)).count();
    let total = a_bigrams.len().max(b_bigrams.len()) as f64;
    if total == 0.0 {
        return 0.0;
    }

    matches as f64 / total
}

/// Return up to `n` suggestions for `input` from `candidates`, scored above
/// the threshold.
fn suggest_similar_commands(input: &str, candidates: &[String], n: usize) -> Vec<String> {
    const THRESHOLD: f64 = 0.5;

    let mut scored: Vec<(f64, &String)> = candidates
        .iter()
        .map(|c| (similarity_score(input, c), c))
        .filter(|(score, _)| *score >= THRESHOLD && *score < 1.0)
        .collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(n);

    scored.into_iter().map(|(_, name)| name.clone()).collect()
}

/// Try to extract the invalid value from a clap workspace-path error and
/// produce an enhanced error message with "Did you mean: …?" suggestions.
///
/// The output is designed for dual consumption:
/// - **Humans**: clear, actionable guidance with the closest match listed first.
/// - **Agents/LLMs**: structured suggestions in a consistent format that can be
///   parsed programmatically (one suggestion per line, indented).
///
/// Returns `Some(enhanced_message)` on success, `None` if the error doesn't
/// match the expected pattern or no good suggestions were found.
#[must_use]
pub(crate) fn try_enhance_clap_error(err_text: &str) -> Option<String> {
    let value = extract_workspace_invalid_value(err_text)?;
    let candidates = cached_candidates();
    let suggestions = suggest_similar_commands(&value, candidates, 3);

    if suggestions.is_empty() {
        return None;
    }

    let suggestion_lines = suggestions
        .iter()
        .map(|s| format!("    {}", style_literal(s)))
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!(
        "{err_text}\n\n\
         {} `{}` is not a workspace path. Did you mean one of these?\n\
         {suggestion_lines}\n\n\
         For more information, try `vtcode --help`.",
        style_header("help"),
        style_placeholder(&value),
    ))
}

/// Extract the invalid value from clap's workspace-path error text.
///
/// Handles values that may contain single quotes by looking for the
/// `' for '[WORKSPACE]'` pattern that follows the value, rather than
/// simply finding the first `'` after the prefix.
fn extract_workspace_invalid_value(err_text: &str) -> Option<String> {
    let marker = "invalid value '";
    let start = err_text.find(marker)? + marker.len();
    let rest = &err_text[start..];
    let end = rest.find("' for '")?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        args_request_version, build_augmented_cli_command, cached_candidates, extract_workspace_invalid_value,
        similarity_score, suggest_similar_commands, try_enhance_clap_error,
    };
    use clap::Parser;
    use vtcode_core::cli::args::Cli;

    #[test]
    fn help_uses_static_quick_start_text() {
        let mut command = build_augmented_cli_command();
        let error = command
            .try_get_matches_from_mut(["vtcode", "--help"])
            .expect_err("help should exit through clap");
        let help = error.to_string();
        assert!(help.contains("Set a provider API key"));
        assert!(!help.contains("already configured"));
    }

    #[test]
    fn invalid_positional_workspace_fails_during_cli_parse() {
        let mut command = build_augmented_cli_command();
        // Workspace is a `last = true` positional; it is only parsed after `--`.
        let err = command
            .try_get_matches_from_mut(["vtcode", "--", "hellp"])
            .expect_err("invalid positional workspace should fail at clap parsing");
        let err_text = err.to_string();
        assert!(
            err_text.contains("is not a valid workspace path or subcommand"),
            "unexpected clap error: {err_text}"
        );
    }

    #[test]
    fn invalid_positional_workspace_fails_with_derive_parser_too() {
        let err = Cli::try_parse_from(["vtcode", "--", "hellp"])
            .expect_err("invalid positional workspace should fail at derive parser");
        let err_text = err.to_string();
        assert!(
            err_text.contains("is not a valid workspace path or subcommand"),
            "unexpected clap error: {err_text}"
        );
    }

    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    fn similarity_score_exact_match() {
        assert_eq!(similarity_score("chat", "chat"), 1.0);
    }

    #[test]
    fn similarity_score_prefix_match() {
        let score = similarity_score("ch", "chat");
        assert!(score > 0.8, "prefix match should score high: {score}");
    }

    #[test]
    fn similarity_score_substring_match() {
        let score = similarity_score("hat", "chat");
        assert!(score >= 0.5, "substring match should score >= 0.5: {score}");
    }

    #[test]
    fn suggest_similar_commands_finds_chat() {
        let candidates = cached_candidates();
        let suggestions = suggest_similar_commands("ch", candidates, 3);
        assert!(suggestions.iter().any(|s| s == "chat"), "should suggest 'chat' for 'ch': {suggestions:?}");
    }

    #[test]
    fn suggest_similar_commands_finds_continue_flag() {
        let candidates = cached_candidates();
        let suggestions = suggest_similar_commands("contnue", candidates, 3);
        assert!(
            suggestions.iter().any(|s| s == "--continue"),
            "should suggest '--continue' for 'contnue': {suggestions:?}"
        );
    }

    #[test]
    fn suggest_similar_commands_no_match_for_gibberish() {
        let candidates = cached_candidates();
        let suggestions = suggest_similar_commands("xyzzy", candidates, 3);
        assert!(suggestions.is_empty(), "should not suggest anything for 'xyzzy': {suggestions:?}");
    }

    #[test]
    fn extract_workspace_invalid_value_parses_correctly() {
        let err = "invalid value 'ch' for '[WORKSPACE]': 'ch' is not a valid workspace path or subcommand.";
        assert_eq!(extract_workspace_invalid_value(err).as_deref(), Some("ch"));
    }

    #[test]
    fn try_enhance_clap_error_adds_suggestion() {
        let err = "invalid value 'ch' for '[WORKSPACE]': 'ch' is not a valid workspace path or subcommand.";
        let enhanced = try_enhance_clap_error(err);
        assert!(enhanced.is_some(), "should enhance error for 'ch'");
        let text = enhanced.unwrap();
        assert!(text.contains("Did you mean one of these?"), "should contain suggestion header: {text}");
        assert!(text.contains("chat"), "should suggest 'chat': {text}");
    }

    #[test]
    fn try_enhance_clap_error_returns_none_for_gibberish() {
        let err = "invalid value 'xyzzy' for '[WORKSPACE]': Workspace path does not exist: xyzzy";
        assert!(try_enhance_clap_error(err).is_none());
    }

    #[test]
    fn try_enhance_clap_error_returns_none_for_non_workspace_error() {
        let err = "some other clap error";
        assert!(try_enhance_clap_error(err).is_none());
    }

    #[test]
    fn version_flag_detection_is_asymmetric() {
        use std::ffi::OsString;
        let version_cases = [
            vec!["--version"],
            vec!["-V"],
            vec!["chat", "--version"],
            vec!["-VV"],
            vec!["--version=json"],
        ];
        for args in version_cases {
            let owned = args.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert!(args_request_version(owned), "version flag must be detected");
        }

        let non_version_cases: Vec<Vec<&str>> = vec![
            vec![],
            vec!["chat"],
            vec!["--help"],
            vec!["--verbose"],
            vec!["--versioned"],
            vec!["-v"],
        ];
        for args in non_version_cases {
            let owned = args.into_iter().map(OsString::from).collect::<Vec<_>>();
            assert!(!args_request_version(owned), "non-version args must skip full diagnostics");
        }
    }
}
