use anstyle::{Reset, Style as AnsiStyle};
use std::borrow::Cow;
use std::io::{self, Write};
use std::time::Duration;
use vtcode_commons::color_policy::color_output_enabled;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::ui::theme;

use crate::agent::runloop::unified::state::FirstCallComposition;

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
    pub final_response: Option<&'a str>,
    pub resume_identifier: Option<&'a str>,
    pub budget_limit: Option<(f64, f64)>,
    /// Cache-aware estimated session spend when pricing is known. Shown even
    /// without a configured USD budget so cancelled/failed runs still report
    /// what they cost. `None` means unknown pricing, not free.
    pub total_cost_usd: Option<f64>,
    /// Terminal outcome label for the stats line (`completed` / `cancelled` /
    /// `error` / …). Always shown so a cancelled or errored run is labeled.
    pub end_reason_label: &'static str,
    /// First assembled request composition (harness-tax breakdown). When
    /// present, the stats line surfaces the per-call fixed overhead.
    pub first_call_composition: Option<FirstCallComposition>,
    /// How the session ended. Controls exit feedback:
    /// - `Completed` re-prints the final response so it survives in the main
    ///   scrollback after the alternate buffer is cleared.
    /// - `Exit`/`Cancelled`/`Error` suppress the full dump (it is already
    ///   visible in the TUI transcript) and print a concise notice instead,
    ///   avoiding fullscreen noise on Ctrl+C.
    pub session_end_reason: vtcode_core::hooks::SessionEndReason,
}

pub(crate) fn print_exit_summary(data: ExitData<'_>) {
    // Arm the graceful-exit window before the first write so a late double
    // Ctrl+C during teardown cannot hard-exit between postamble writes.
    crate::agent::runloop::unified::session_setup::mark_exit_postamble_armed();
    // Finish the deferred raw-mode transition: drain pending input while echo is
    // still off (so late reports are consumed, not echoed), then return the tty
    // to the cooked state the shell expects.
    vtcode_ui::tui::panic_hook::finish_deferred_raw_mode_restore();
    // Belt-and-braces for paths that never deferred (emergency exit, partial
    // init): without a cooked tty the postamble would staircase with `ONLCR` off.
    // crossterm's disable is a no-op when raw mode is already off.
    vtcode_ui::tui::panic_hook::ensure_raw_mode_disabled();
    // Completed sessions re-print the answer through the markdown renderer so
    // it survives in the main scrollback after the inline frame is torn down.
    if matches!(data.session_end_reason, vtcode_core::hooks::SessionEndReason::Completed)
        && let Some(response) = data.final_response.filter(|response| !response.trim().is_empty())
    {
        render_final_response(response);
    }
    // One buffered write for the notice + metrics block: the shell sees the
    // whole summary or nothing, never a truncated first line.
    write_postamble(&build_exit_postamble(&data, color_output_enabled()));
}

/// Builds the exit postamble (interrupt notice + metrics block) as a single
/// string.
///
/// Every row carries a leading `\r` so the block starts at column 0 even when
/// output processing (`ONLCR`) is off, and the block opens on a fresh row so
/// it never overwrites the leftover inline TUI frame that the canonical
/// restore left behind.
fn build_exit_postamble(data: &ExitData<'_>, color_enabled: bool) -> String {
    let styles = PostambleStyles::resolve(color_enabled);
    let mut out = String::with_capacity(640);
    // Fresh row below any leftover inline frame content.
    out.push_str("\r\n");

    if matches!(
        data.session_end_reason,
        vtcode_core::hooks::SessionEndReason::Exit | vtcode_core::hooks::SessionEndReason::Cancelled
    ) {
        // Interrupted exits get concise feedback, not a full transcript dump.
        // The in-TUI answer (if any) stays in the alternate buffer history;
        // re-printing it here is the fullscreen noise reported on Ctrl+C.
        push_row(&mut out, styles.notice("Interrupted — session exited. Transcript saved; resume to continue."));
    }

    out.push_str("\r\n");
    push_row(&mut out, styles.banner(&format!("> {} ({})", data.app_name, data.version)));

    let trust = build_trust_label(data.trust_label);
    if !trust.is_empty() {
        push_row(&mut out, styles.muted(&trust));
    }

    let model_line = build_model_line(&styles, data);
    if !model_line.is_empty() {
        push_row(&mut out, model_line);
    }

    push_row(&mut out, styles.muted(&build_stats_line(data)));

    if let Some((max_budget_usd, actual_cost_usd)) = data.budget_limit {
        push_row(&mut out, styles.muted(&format!("Budget at ${actual_cost_usd:.2} / ${max_budget_usd:.2}")));
    }

    if let Some(session_id) = data.resume_identifier {
        push_row(
            &mut out,
            format!("{} {}", styles.muted("Resume:"), styles.accent(&format!("vtcode --resume {session_id}")),),
        );
    }

    out.push_str("\r\n");
    out
}

/// Theme-resolved styles for the exit postamble.
///
/// Resolved once per exit so the block reads as a single surface with a clear
/// hierarchy: a bold banner title, one bold + underlined warning notice row,
/// subdued metadata, and the theme accent on the values the user acts on
/// (model, resume command).
///
/// Every color comes from the active theme — either its contrast-validated
/// `ThemeStyles` tokens or its `banner_style()` — so no hand-picked palette
/// value can fall below the configured WCAG minimum (4.5:1 by default) on a
/// light or dark background. Faint (`SGR 2`) styling is deliberately unused:
/// terminals render it by dimming the foreground, which pushes normal-size
/// text under the same minimum. See `docs/guides/COLOR_GUIDELINES.md`.
struct PostambleStyles {
    /// Bold theme banner color for the `> VT Code (version)` row.
    banner: AnsiStyle,
    /// Bold + underlined warning amber for the interrupt notice — the row
    /// that must be seen. Uses the dedicated `warning` token (scheme-picked
    /// amber, never the brand `logo_accent`) so Ctrl+C reads as a warning,
    /// matching Warning transcript semantics.
    notice: AnsiStyle,
    /// Subdued foreground for metadata rows and inline labels.
    muted: AnsiStyle,
    /// Theme accent for key values (model, resume command).
    accent: AnsiStyle,
    /// `NO_COLOR` / `--no-color` / `--color never` gate.
    color_enabled: bool,
}

impl PostambleStyles {
    fn resolve(color_enabled: bool) -> Self {
        let styles = theme::active_styles();
        Self {
            banner: theme::banner_style(),
            notice: styles.warning.underline(),
            muted: styles.tool_detail,
            accent: styles.primary,
            color_enabled,
        }
    }

    /// Wrap `text` in `style`, or return it unstyled when color is disabled.
    fn paint(&self, style: &AnsiStyle, text: &str) -> String {
        if self.color_enabled {
            format!("{style}{text}{Reset}")
        } else {
            text.to_string()
        }
    }

    fn banner(&self, text: &str) -> String {
        self.paint(&self.banner, text)
    }

    fn notice(&self, text: &str) -> String {
        self.paint(&self.notice, text)
    }

    fn muted(&self, text: &str) -> String {
        self.paint(&self.muted, text)
    }

    fn accent(&self, text: &str) -> String {
        self.paint(&self.accent, text)
    }
}

/// Append one postamble row: carriage-first so the row starts at column 0 even
/// when output processing (`ONLCR`) is off, newline-terminated.
///
/// `content` is pre-styled segment text; keeping the carriage outside the
/// styling means a terminal that ignores `SGR` still lays the rows out.
fn push_row(out: &mut String, content: String) {
    out.push('\r');
    out.push_str(&content);
    out.push('\n');
}

/// Emit the complete block without adding cursor movement or geometry probes.
fn write_postamble_to(writer: &mut impl Write, postamble: &str) -> io::Result<()> {
    writer.write_all(postamble.as_bytes())?;
    writer.flush()
}

/// Decide which streams carry the exit summary.
///
/// Stdout always carries it so piped runs (`vtcode > out.txt`) still capture
/// the resume command. When stdout is redirected but stderr is still the
/// terminal (the TUI renders there), stderr carries a second copy: otherwise
/// a `| tee` / IDE-task run shows the TUI frame yet the summary silently
/// lands in the pipe and the terminal looks like it printed nothing.
fn postamble_destinations(stdout_is_terminal: bool, stderr_is_terminal: bool) -> (bool, bool) {
    (true, !stdout_is_terminal && stderr_is_terminal)
}

/// Keep each stream lock across the complete block so exit paths cannot
/// interleave between rows.
fn write_postamble(postamble: &str) {
    use std::io::IsTerminal;
    let (to_stdout, to_stderr) = postamble_destinations(io::stdout().is_terminal(), io::stderr().is_terminal());
    // Print at the restored cursor position: the canonical restore leaves an
    // alternate-screen session at the pre-TUI cursor and an inline session at
    // the end of its transcript, both visible. Jumping to the viewport bottom
    // (CUP to the last row) would scroll a viewport-height run of blanks into
    // the scrollback — the huge blank gap reported on exit — so no cursor
    // anchor belongs here.
    if to_stdout {
        let mut stdout = io::stdout().lock();
        if let Err(error) = write_postamble_to(&mut stdout, postamble) {
            tracing::warn!(%error, "failed to write the exit postamble to stdout");
        }
    }
    if to_stderr {
        let mut stderr = io::stderr().lock();
        if let Err(error) = write_postamble_to(&mut stderr, postamble) {
            tracing::warn!(%error, "failed to write the exit postamble to stderr");
        }
    }
    // Last defense before the process exits: anything that arrived while the
    // remaining teardown (git snapshot, runtime drop) ran is consumed here so
    // it cannot be echoed by the tty or read by the shell as stray input.
    vtcode_ui::tui::panic_hook::drain_pending_terminal_input();
}

fn render_final_response(response: &str) {
    let mut renderer = AnsiRenderer::stdout();
    if let Err(error) = renderer.line(MessageStyle::Response, response) {
        tracing::warn!(%error, "failed to render final response during session exit");
    }
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
    stats.push(format!("Session {}", format_duration(data.session_duration)));

    if data.prompt_tokens > 0 || data.completion_tokens > 0 {
        stats
            .push(format!("{} in / {} out", format_number(data.prompt_tokens), format_number(data.completion_tokens),));
    }

    if data.cached_tokens > 0 {
        let mut cache_stat = format!("Cache {} read", format_number(data.cached_tokens));
        if let Some(hit_rate) = data.cache_hit_rate_percent {
            cache_stat.push_str(&format!(" ({hit_rate:.1}% hit rate)"));
        }
        if data.cache_creation_tokens > 0 {
            cache_stat.push_str(&format!(", {} creation", format_number(data.cache_creation_tokens)));
        }
        stats.push(cache_stat);
    }

    if data.code_additions > 0 || data.code_deletions > 0 {
        stats.push(format!("Code +{} / -{}", data.code_additions, data.code_deletions));
    }

    // Always surface estimated spend when pricing is known, including
    // cancelled runs — a 17M-token interrupt must not look free.
    if let Some(cost) = data.total_cost_usd {
        stats.push(format!("Cost ${cost:.2}"));
    }

    if !data.end_reason_label.is_empty() {
        stats.push(format!("End {}", data.end_reason_label));
    }

    if let Some(overhead) = data.first_call_composition {
        let fixed = overhead.fixed_overhead_tokens();
        if fixed > 0 {
            stats.push(format!(
                "First-call overhead {} (system {} + tools {})",
                format_number(fixed as u64),
                format_number(overhead.system_prompt_tokens as u64),
                format_number(overhead.tool_schema_tokens as u64),
            ));
        }
    }

    stats.join(" | ")
}

/// Builds the model/provider/reasoning row (unstyled ends are supplied by the
/// caller's DIM wrapper). Empty when neither model nor provider is known.
fn build_model_line(styles: &PostambleStyles, data: &ExitData<'_>) -> String {
    let model = data.model.trim();
    let provider = data.provider.trim();
    let reasoning = data.reasoning.trim();

    let show_model = !model.is_empty();
    let show_provider = !provider.is_empty();
    let show_reasoning = !reasoning.is_empty();

    // Each segment paints itself: a wrapping row style would be cancelled by
    // the inner accent span's reset, leaving the trailing text unstyled.
    let mut line = match (show_model, show_provider) {
        (true, true) => {
            format!("{} {} {}", styles.muted("Model:"), styles.accent(model), styles.muted(&format!("via {provider}")),)
        }
        (true, false) => format!("{} {}", styles.muted("Model:"), styles.accent(model)),
        (false, true) => format!("{} {provider}", styles.muted("Provider:")),
        (false, false) => String::new(),
    };

    if show_reasoning {
        let suffix = format!(" · {reasoning}");
        if line.is_empty() {
            line = styles.muted(&format!("Reasoning:{suffix}"));
        } else {
            line.push_str(&styles.muted(&suffix));
        }
    }

    line
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
        assert_eq!(build_trust_label("some_other").as_ref(), "Trust: some other");
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
            final_response: None,
            resume_identifier: None,
            budget_limit: None,
            total_cost_usd: None,
            end_reason_label: "",
            first_call_composition: None,
            session_end_reason: vtcode_core::hooks::SessionEndReason::Completed,
        }
    }

    #[test]
    fn stats_line_with_zero_input_omits_token_and_cache_segments() {
        let data = stats_test_data(Duration::from_secs(30), 0, 0, 0, 0, None, 0, 0);
        let line = build_stats_line(&data);
        assert_eq!(line, "Session 30s");
    }

    #[test]
    fn stats_line_includes_first_call_overhead_when_present() {
        let data = ExitData {
            first_call_composition: Some(FirstCallComposition {
                system_prompt_tokens: 1_100,
                tool_schema_tokens: 2_100,
                message_history_tokens: 40,
                on_wire_tools: 4,
            }),
            ..stats_test_data(Duration::from_secs(30), 0, 0, 0, 0, None, 0, 0)
        };
        let line = build_stats_line(&data);
        assert!(
            line.contains("First-call overhead 3.2k (system 1.1k + tools 2.1k)"),
            "stats line missing first-call overhead: {line}"
        );
    }

    #[test]
    fn stats_line_omits_zero_first_call_overhead() {
        let data = ExitData {
            first_call_composition: Some(FirstCallComposition {
                system_prompt_tokens: 0,
                tool_schema_tokens: 0,
                message_history_tokens: 0,
                on_wire_tools: 0,
            }),
            ..stats_test_data(Duration::from_secs(30), 0, 0, 0, 0, None, 0, 0)
        };
        let line = build_stats_line(&data);
        assert_eq!(line, "Session 30s");
    }

    #[test]
    fn stats_line_includes_cost_when_pricing_is_known() {
        let data = ExitData {
            total_cost_usd: Some(0.42),
            end_reason_label: "cancelled",
            ..stats_test_data(Duration::from_secs(30), 1_000, 100, 0, 0, None, 0, 0)
        };
        let line = build_stats_line(&data);
        assert!(line.contains("Cost $0.42"), "missing cost: {line}");
        assert!(line.contains("End cancelled"), "missing end reason: {line}");
    }

    #[test]
    fn stats_line_omits_cost_when_pricing_is_unknown() {
        // Unknown pricing must not render as free / $0.
        let data = stats_test_data(Duration::from_secs(30), 1_000, 100, 0, 0, None, 0, 0);
        let line = build_stats_line(&data);
        assert!(!line.contains("Cost"), "unknown pricing must not show cost: {line}");
    }

    #[test]
    fn stats_line_with_cache_includes_hit_rate_and_creation_tokens() {
        let data = stats_test_data(Duration::from_secs(95), 1_000, 200, 800, 50, Some(80.0), 10, 2);
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
        assert_eq!(line, "Session 10s | 500 in / 100 out | Cache 400 read (80.0% hit rate)");
    }

    /// Colored postamble, matching the production path when color output is on.
    fn colored_postamble(data: &ExitData<'_>) -> String {
        build_exit_postamble(data, true)
    }

    #[test]
    fn final_response_is_rendered_before_exit_metrics() {
        // Completed sessions re-print the answer through the markdown renderer
        // (stdout writes), so only the block shape is assertable here: the
        // metrics rows always follow the response branch.
        let data = ExitData {
            final_response: Some("final response"),
            session_end_reason: vtcode_core::hooks::SessionEndReason::Completed,
            ..stats_test_data(Duration::from_secs(30), 0, 0, 0, 0, None, 0, 0)
        };
        let postamble = colored_postamble(&data);

        assert!(postamble.contains("> VT Code (0.0.0)"), "title row missing: {postamble:?}");
        assert!(postamble.contains("Session 30s"), "stats row missing: {postamble:?}");
        assert!(
            !postamble.contains("Interrupted — session exited"),
            "completed exits must not carry the interrupt notice: {postamble:?}"
        );
    }

    #[test]
    fn interrupt_postamble_keeps_full_summary_block() {
        // Regression: a late double Ctrl+C during teardown used to hard-exit
        // between postamble writes, leaving only the notice on screen.
        let data = ExitData {
            final_response: Some("long transcript body that must not leak"),
            session_end_reason: vtcode_core::hooks::SessionEndReason::Exit,
            resume_identifier: Some("session-test-1234"),
            ..stats_test_data(Duration::from_secs(95), 1_000, 200, 800, 50, Some(80.0), 10, 2)
        };
        let postamble = colored_postamble(&data);
        let rows: Vec<&str> = postamble.split('\n').collect();

        let notice_row = rows
            .iter()
            .position(|row| row.contains("Interrupted — session exited"))
            .expect("interrupt notice row");
        let title_row = rows.iter().position(|row| row.contains("> VT Code")).expect("title row");
        let stats_row = rows.iter().position(|row| row.contains("Session 1m 35s")).expect("stats row");
        let resume_row = rows
            .iter()
            .position(|row| row.contains("vtcode --resume session-test-1234"))
            .expect("resume id row");

        assert!(notice_row < title_row, "notice must precede the block: {postamble:?}");
        assert!(title_row < stats_row, "block order broken: {postamble:?}");
        assert!(stats_row < resume_row, "resume id must follow the stats row: {postamble:?}");
        assert!(
            !postamble.contains("long transcript body"),
            "interrupt exits must suppress the final response dump: {postamble:?}"
        );
    }

    #[test]
    fn postamble_rows_are_carriage_prefixed_and_open_on_a_fresh_row() {
        let data = ExitData {
            session_end_reason: vtcode_core::hooks::SessionEndReason::Exit,
            ..stats_test_data(Duration::from_secs(30), 0, 0, 0, 0, None, 0, 0)
        };
        let postamble = colored_postamble(&data);

        assert!(postamble.starts_with("\r\n"), "postamble must open on a fresh row: {postamble:?}");
        for row in postamble.split('\n').filter(|row| !row.is_empty()) {
            assert!(row.starts_with('\r'), "every postamble row must return the carriage: {row:?}");
        }
    }

    #[test]
    fn postamble_avoids_faint_and_unvalidated_palette_styling() {
        // Regression: the summary used to wrap nearly every row in SGR 2 (faint)
        // and hand-pick 256-palette accents. Terminals render faint by dimming
        // the foreground, and the picked RGB values were never checked against
        // the background, so both could drop text below the WCAG AA 4.5:1
        // minimum. Only theme tokens are allowed now.
        let data = ExitData {
            model: "gpt-5.6-sol",
            provider: "openai",
            reasoning: "medium",
            trust_label: "tools policy",
            resume_identifier: Some("session-x"),
            ..stats_test_data(Duration::from_secs(95), 1_000, 200, 800, 50, Some(80.0), 10, 2)
        };
        let postamble = colored_postamble(&data);

        assert!(
            !postamble.contains("\x1b[2m") && !postamble.contains("\x1b[2;"),
            "faint styling must not ship: {postamble:?}"
        );
        assert!(!postamble.contains("38;5;"), "hand-picked 256-palette colors must not ship: {postamble:?}");
        // Every styled span is closed, so no row bleeds into the next one.
        let opens = postamble.matches("\x1b[").count();
        assert!(
            opens >= 4 && postamble.matches(&*Reset.to_string()).count() >= 4,
            "styled spans must be closed: {postamble:?}"
        );
    }

    #[test]
    fn postamble_colors_meet_the_theme_minimum_contrast() {
        // Accessibility oracle: each token the postamble paints with must clear
        // the configured WCAG minimum (4.5:1 by default) against the active
        // theme background. The theme builds every token through its shared
        // contrast pipeline, so a failure here means a token was swapped for
        // something unvalidated.
        let styles = PostambleStyles::resolve(true);
        let minimum = theme::get_minimum_contrast();
        for (name, style) in [
            ("banner", &styles.banner),
            ("notice", &styles.notice),
            ("muted", &styles.muted),
            ("accent", &styles.accent),
        ] {
            let ratio = theme::style_contrast_ratio(style)
                .unwrap_or_else(|| panic!("{name} style must carry an RGB foreground"));
            assert!(ratio >= minimum, "{name} contrast {ratio:.2} below {minimum:.1}");
        }
    }

    #[test]
    fn postamble_emits_the_active_theme_tokens() {
        let styles = PostambleStyles::resolve(true);
        let data = ExitData {
            model: "gpt-5.6-sol",
            provider: "openai",
            resume_identifier: Some("session-x"),
            ..stats_test_data(Duration::from_secs(30), 1_000, 100, 0, 0, None, 0, 0)
        };
        let postamble = colored_postamble(&data);

        assert!(
            postamble.contains(&styles.banner.to_string()),
            "title row must use the theme banner style: {postamble:?}"
        );
        assert!(
            postamble.contains(&styles.accent.to_string()),
            "model and resume command must use the theme accent: {postamble:?}"
        );
        assert!(
            postamble.contains(&styles.muted.to_string()),
            "metadata rows must use the theme muted token: {postamble:?}"
        );

        // The interrupt notice is a separate surface from the metadata block:
        // it must use the warning token, not the muted/info tokens.
        let interrupt_data = ExitData {
            session_end_reason: vtcode_core::hooks::SessionEndReason::Exit,
            ..stats_test_data(Duration::from_secs(30), 0, 0, 0, 0, None, 0, 0)
        };
        let interrupt_postamble = colored_postamble(&interrupt_data);
        assert!(
            interrupt_postamble.contains(&styles.notice.to_string()),
            "interrupt notice must use the theme warning style: {interrupt_postamble:?}"
        );
    }

    #[test]
    fn postamble_notice_uses_warning_amber_with_bold_underline() {
        use anstyle::Effects;

        let styles = PostambleStyles::resolve(true);
        let theme_styles = theme::active_styles();

        // Same foreground as the design-system warning token (scheme-picked
        // amber), never the brand logo accent or the old info token.
        assert_eq!(
            styles.notice.get_fg_color(),
            theme_styles.warning.get_fg_color(),
            "notice must carry the warning foreground"
        );
        assert_ne!(
            styles.notice.get_fg_color(),
            theme_styles.info.get_fg_color(),
            "notice must no longer use the info token"
        );

        // Typographic hierarchy: bold (unless the terminal maps bold to
        // bright) plus underline so the interrupt row stands out from the
        // muted metadata block without resorting to faint.
        let effects = styles.notice.get_effects();
        assert!(effects.contains(Effects::UNDERLINE), "notice must be underlined: {effects:?}");
        if !theme::is_bold_bright_mode() {
            assert!(effects.contains(Effects::BOLD), "notice must stay bold: {effects:?}");
        }
        assert!(!effects.contains(Effects::DIMMED), "notice must not use faint/dimmed: {effects:?}");
    }

    #[test]
    fn postamble_is_plain_text_when_color_is_disabled() {
        // NO_COLOR / --no-color / --color never must produce a summary that is
        // still fully readable — the styled spans degrade to their text.
        let data = ExitData {
            model: "gpt-5.6-sol",
            provider: "openai",
            reasoning: "medium",
            session_end_reason: vtcode_core::hooks::SessionEndReason::Exit,
            resume_identifier: Some("session-x"),
            ..stats_test_data(Duration::from_secs(30), 1_000, 100, 0, 0, None, 0, 0)
        };
        let postamble = build_exit_postamble(&data, false);

        assert!(!postamble.contains('\x1b'), "plain output must carry no escapes: {postamble:?}");
        assert!(postamble.contains("Model: gpt-5.6-sol via openai"), "model row text: {postamble:?}");
        assert!(postamble.contains("Resume: vtcode --resume session-x"), "resume row text: {postamble:?}");
        assert!(
            postamble.contains("Interrupted — session exited"),
            "interrupt notice must survive without color: {postamble:?}"
        );
    }

    #[test]
    fn postamble_destinations_mirror_to_stderr_only_when_stdout_is_redirected() {
        // The TUI renders on stderr. A `| tee` run keeps stderr on the
        // terminal while stdout lands in a pipe: without the mirror the
        // terminal shows the TUI frame yet the summary silently vanishes
        // into the pipe. Both ttys must not duplicate the block.
        assert_eq!(postamble_destinations(true, true), (true, false));
        assert_eq!(postamble_destinations(true, false), (true, false));
        assert_eq!(postamble_destinations(false, true), (true, true));
        assert_eq!(postamble_destinations(false, false), (true, false));
    }

    #[test]
    fn postamble_never_jumps_to_the_viewport_bottom() {
        // Regression: a cursor-to-last-row anchor (ESC[r + CUP to the last
        // row) scrolled a viewport-height run of blanks into the scrollback —
        // the huge blank gap reported on exit. The block must print at the
        // restored cursor with carriage-prefixed rows only.
        let data = ExitData {
            session_end_reason: vtcode_core::hooks::SessionEndReason::Exit,
            resume_identifier: Some("session-x"),
            ..stats_test_data(Duration::from_secs(30), 1_000, 100, 0, 0, None, 0, 0)
        };
        let postamble = colored_postamble(&data);
        let mut bytes = Vec::new();
        write_postamble_to(&mut bytes, &postamble).unwrap();
        assert_eq!(bytes, postamble.as_bytes(), "writer must emit the supplied block without a cursor anchor");
        let postamble = String::from_utf8(bytes).unwrap();

        assert!(!postamble.contains("\x1b[r"), "scroll-region reset must not ship: {postamble:?}");
        assert!(!postamble.contains(";1H"), "viewport-bottom CUP must not ship: {postamble:?}");
        assert!(postamble.contains("Interrupted — session exited"), "notice row: {postamble:?}");
        assert!(postamble.contains("> VT Code (0.0.0)"), "title row: {postamble:?}");
        assert!(postamble.contains("Session 30s"), "stats row: {postamble:?}");
    }

    #[test]
    fn postamble_writer_propagates_write_and_flush_errors() {
        struct FailingWriter {
            fail_write: bool,
            flushed: bool,
        }
        impl Write for FailingWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.fail_write {
                    Err(io::Error::from(io::ErrorKind::BrokenPipe))
                } else {
                    Ok(bytes.len())
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                self.flushed = true;
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            }
        }
        for fail_write in [false, true] {
            let mut writer = FailingWriter { fail_write, flushed: false };
            let error = write_postamble_to(&mut writer, "\rExit summary\n").unwrap_err();
            assert_eq!(writer.flushed, !fail_write);
            assert_eq!(
                error.kind(),
                if fail_write {
                    io::ErrorKind::BrokenPipe
                } else {
                    io::ErrorKind::PermissionDenied
                }
            );
        }
    }

    #[test]
    fn exit_suppresses_final_response_dump() {
        // Ctrl+C exits must not re-print the full TUI transcript: that is the
        // fullscreen noise reported on control+c handling.
        for reason in [
            vtcode_core::hooks::SessionEndReason::Exit,
            vtcode_core::hooks::SessionEndReason::Cancelled,
            vtcode_core::hooks::SessionEndReason::Error,
        ] {
            let data = ExitData {
                final_response: Some("long transcript body that must not leak"),
                session_end_reason: reason,
                ..stats_test_data(Duration::from_secs(30), 0, 0, 0, 0, None, 0, 0)
            };
            let postamble = colored_postamble(&data);

            assert!(
                !postamble.contains("long transcript body"),
                "exit reason {reason:?} must suppress final response dump: {postamble:?}"
            );
            assert!(postamble.contains("Session 30s"), "exit metrics must still render for {reason:?}: {postamble:?}");
        }
    }
}
