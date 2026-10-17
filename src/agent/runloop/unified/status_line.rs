use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use chrono::Local;

use vtcode_core::config::constants::ui;
use vtcode_core::config::{StatusLineConfig, StatusLineMode};
use vtcode_core::llm::provider::LLMProvider;
use vtcode_core::models::ModelId;
use vtcode_ui::tui::app::InlineHandle;

use super::status_line_command::run_status_line_command;
use crate::agent::runloop::git::{GitStatusSummary, git_status_summary};
use vtcode_core::llm::providers::clean_reasoning_text;
#[derive(Default, Clone)]
pub(crate) struct InputStatusState {
    pub(crate) left: Option<String>,
    pub(crate) right: Option<String>,
    pub(crate) git_left: Option<String>,
    pub(crate) git_summary: Option<GitStatusSummary>,
    pub(crate) last_git_refresh: Option<Instant>,
    pub(crate) command_value: Option<String>,
    pub(crate) last_command_refresh: Option<Instant>,
    // Context usage metrics
    pub(crate) context_utilization: Option<f64>,
    pub(crate) context_tokens: Option<usize>,
    pub(crate) context_limit_tokens: Option<usize>,
    pub(crate) context_remaining_tokens: Option<usize>,
    pub(crate) is_cancelling: bool,
    pub(crate) is_blocked: bool,
    pub(crate) clock: Option<String>,
    // Dynamic context discovery status
    pub(crate) spooled_files_count: Option<usize>,
    pub(crate) thread_context: Option<String>,
    // Balance & cost info (shown for DeepSeek and OpenAI providers)
    pub(crate) show_costs: bool,
    pub(crate) balance: Option<String>,
    pub(crate) cost_usd: Option<f64>,
    pub(crate) cache_hit_pct: Option<f64>,
    pub(crate) last_balance_refresh: Option<Instant>,
    /// Set to `true` whenever a status-affecting field actually changes.
    /// The interaction loop clears it after a successful status rebuild and
    /// uses it to skip `update_input_status_if_changed` when nothing changed
    /// and the clock second hasn't ticked.
    pub(crate) status_dirty: bool,
    /// Set when an error/idle path writes a terminal status directly. The
    /// next status refresh must send even when the cached layout is unchanged,
    /// because the direct write may have been made by another UI component.
    pub(crate) input_status_sync_pending: bool,
    /// The Unix-second of the last formatted clock value. Used to detect when
    /// the `HH:MM:SS` display would actually change (once per second) so the
    /// status rebuild can be skipped between ticks.
    pub(crate) last_clock_second: Option<u64>,
    /// Cached model display name. `ModelId::from_str` is only re-run when the
    /// raw model string changes (e.g. user switches models), not every tick.
    pub(crate) cached_model_input: Option<String>,
    pub(crate) cached_model_display: Option<String>,
    /// Cached cleaned reasoning text. `clean_reasoning_text` is only re-run
    /// when the raw reasoning string changes, not every tick.
    pub(crate) cached_reasoning_input: Option<String>,
    pub(crate) cached_reasoning_cleaned: String,
    /// The Unix-second for which `state.clock` was last formatted. When a
    /// dirty-only status update runs (no clock tick), the clock string is
    /// reused instead of calling `chrono::Local::now()` again.
    pub(crate) clock_string_second: Option<u64>,
    /// Last values sent to the terminal-title setters. The interaction loop
    /// compares against these before enqueuing `InlineCommand`s so idle ticks
    /// don't produce 3 channel sends + `needs_redraw` for unchanged titles.
    pub(crate) last_title_items: Option<Vec<String>>,
    pub(crate) last_title_thread_label: Option<String>,
    pub(crate) last_title_git_branch: Option<String>,
}

const GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
struct BottomStatusLayout {
    left: Vec<String>,
    right: Vec<String>,
}

impl BottomStatusLayout {
    fn into_status(self) -> (Option<String>, Option<String>) {
        (join_status_components(self.left), join_status_components(self.right))
    }
}

pub(crate) fn update_context_budget(state: &mut InputStatusState, used_tokens: usize, limit_tokens: usize) {
    if limit_tokens == 0 {
        if state.context_utilization.is_some() {
            state.context_utilization = None;
            state.context_tokens = None;
            state.context_limit_tokens = None;
            state.context_remaining_tokens = None;
            state.status_dirty = true;
        }
        return;
    }

    let used = used_tokens.min(limit_tokens);
    let remaining = limit_tokens.saturating_sub(used);
    let left_percent = (remaining as f64 / limit_tokens as f64) * 100.0;
    let utilization = Some(left_percent.clamp(0.0, 100.0));

    if state.context_utilization != utilization
        || state.context_tokens != Some(used)
        || state.context_limit_tokens != Some(limit_tokens)
        || state.context_remaining_tokens != Some(remaining)
    {
        state.context_utilization = utilization;
        state.context_tokens = Some(used);
        state.context_limit_tokens = Some(limit_tokens);
        state.context_remaining_tokens = Some(remaining);
        state.status_dirty = true;
    }
}

pub(crate) fn status_line_shows_auto_components(status_config: Option<&StatusLineConfig>) -> bool {
    match status_config.map(|cfg| cfg.effective_mode()).unwrap_or(StatusLineMode::Auto) {
        StatusLineMode::Auto | StatusLineMode::Unknown => true,
        StatusLineMode::Command => status_config
            .and_then(|cfg| cfg.command.as_deref())
            .map(|command| command.trim().is_empty())
            .unwrap_or(true),
        StatusLineMode::Hidden => false,
    }
}

pub(crate) fn status_line_shows_clock(status_config: Option<&StatusLineConfig>) -> bool {
    status_config.map(|cfg| cfg.show_clock).unwrap_or(true)
}

fn now_statusline_clock() -> String {
    Local::now().format("%H:%M:%S").to_string()
}

/// Cheap `SystemTime` read returning the current Unix second without any
/// timezone resolution or string allocation. Used to detect whether the
/// `HH:MM:SS` clock display would actually differ from the last frame.
fn current_second() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn cached_model_display<'a>(state: &'a mut InputStatusState, model: &str) -> &'a str {
    let trimmed_model = model.trim();
    if state.cached_model_input.as_deref() != Some(trimmed_model) {
        let display = ModelId::from_str(trimmed_model)
            .map(|id| id.display_name().to_string())
            .unwrap_or_else(|_| trimmed_model.to_string());
        state.cached_model_input = Some(trimmed_model.to_string());
        state.cached_model_display = Some(display);
    }
    state.cached_model_display.as_deref().unwrap_or_default()
}

fn cached_cleaned_reasoning<'a>(state: &'a mut InputStatusState, reasoning: &str) -> &'a str {
    let trimmed_reasoning = reasoning.trim();
    if state.cached_reasoning_input.as_deref() != Some(trimmed_reasoning) {
        state.cached_reasoning_input = Some(trimmed_reasoning.to_string());
        state.cached_reasoning_cleaned = clean_reasoning_text(trimmed_reasoning);
    }
    &state.cached_reasoning_cleaned
}

fn update_clock(state: &mut InputStatusState, show_clock: bool) {
    if !show_clock {
        state.clock = None;
        state.clock_string_second = None;
        return;
    }

    let now_sec = current_second();
    if state.clock.is_none() || state.clock_string_second != Some(now_sec) {
        state.clock_string_second = Some(now_sec);
        state.clock = Some(now_statusline_clock());
    }
}

/// Returns `true` when the clock display (HH:MM:SS) would change since the
/// last recorded second, updating the tracked second in the process.
pub(crate) fn clock_second_ticked(state: &mut InputStatusState) -> bool {
    let now = current_second();
    let changed = state.last_clock_second.is_none_or(|prev| prev != now);
    state.last_clock_second = Some(now);
    changed
}

/// Invalidate state that depends on the live status-line configuration.
///
/// Configuration reloads can change the status mode, command, clock, or
/// visibility of Git data without changing any runtime counters. Reset the
/// refresh gates so the next scheduled update observes the new configuration
/// immediately rather than waiting for an old interval to expire.
pub(crate) fn invalidate_for_config_reload(state: &mut InputStatusState) {
    state.status_dirty = true;
    state.input_status_sync_pending = true;
    state.last_git_refresh = None;
    state.last_command_refresh = None;
    state.command_value = None;
}

pub(crate) fn clear_input_status(handle: &InlineHandle, state: &mut InputStatusState) {
    handle.set_input_status(None, None);
    state.left = None;
    state.right = None;
    state.status_dirty = true;
    state.input_status_sync_pending = true;
}

pub(crate) fn sync_terminal_title(handle: &InlineHandle, state: &mut InputStatusState, title_items: Option<&[String]>) {
    if state.last_title_items.as_deref() != title_items {
        let next = title_items.map(<[String]>::to_vec);
        handle.set_terminal_title_items(next.clone());
        state.last_title_items = next;
    }

    if state.last_title_thread_label.as_deref() != state.thread_context.as_deref() {
        let next = state.thread_context.clone();
        handle.set_terminal_title_thread_label(next.clone());
        state.last_title_thread_label = next;
    }

    let git_branch = state
        .git_summary
        .as_ref()
        .map(|summary| summary.branch.as_str())
        .filter(|branch| !branch.trim().is_empty());
    if state.last_title_git_branch.as_deref() != git_branch {
        let next = git_branch.map(str::to_owned);
        handle.set_terminal_title_git_branch(next.clone());
        state.last_title_git_branch = next;
    }
}

pub(crate) async fn update_input_status_if_changed(
    handle: &InlineHandle,
    workspace: &Path,
    model: &str,
    reasoning: &str,
    status_config: Option<&StatusLineConfig>,
    state: &mut InputStatusState,
) -> Result<()> {
    // Get the effective status line mode, using defaults when config is unset
    // (following Codex PR #12015 pattern for default configuration fallback)
    let mode = status_config.map(|cfg| cfg.effective_mode()).unwrap_or(StatusLineMode::Auto);
    let status_line_hidden = matches!(mode, StatusLineMode::Hidden);

    if status_line_hidden {
        state.git_left = None;
    }

    let should_refresh_git = match state.last_git_refresh {
        Some(last_refresh) => last_refresh.elapsed() >= GIT_STATUS_REFRESH_INTERVAL,
        None => true,
    };

    if should_refresh_git {
        let workspace_buf = workspace.to_path_buf();
        match tokio::task::spawn_blocking(move || git_status_summary(&workspace_buf)).await? {
            Ok(Some(summary)) => {
                let indicator = if summary.dirty {
                    ui::HEADER_GIT_DIRTY_SUFFIX
                } else {
                    ui::HEADER_GIT_CLEAN_SUFFIX
                };
                let display = if summary.branch.is_empty() {
                    indicator.to_string()
                } else {
                    format!("{}{}", summary.branch, indicator)
                };
                state.git_left = (!status_line_hidden).then_some(display);
                state.git_summary = Some(summary);
            }
            Ok(None) => {
                state.git_left = None;
                state.git_summary = None;
            }
            Err(error) => {
                tracing::warn!(
                    workspace = %workspace.display(),
                    error = ?error,
                    "Failed to resolve git status"
                );
                state.git_summary = None;
                state.git_left = None;
            }
        }

        state.last_git_refresh = Some(Instant::now());
    }

    let trimmed_model = model.trim();

    let mut command_error: Option<anyhow::Error> = None;

    // Reuse the formatted clock string when the displayed second hasn't
    // changed (dirty-only update without a clock tick). Avoids a
    // chrono::Local::now() + format call and String clone on every rebuild.
    update_clock(state, status_line_shows_clock(status_config));

    let (left, right) = match mode {
        StatusLineMode::Hidden => {
            state.command_value = None;
            state.last_command_refresh = None;
            (None, None)
        }
        StatusLineMode::Auto | StatusLineMode::Unknown => auto_status_layout(state).into_status(),
        StatusLineMode::Command => {
            if let Some(cfg) = status_config {
                if let Some(command) = cfg
                    .command
                    .as_ref()
                    .map(|cmd| cmd.trim().to_string())
                    .filter(|cmd| !cmd.is_empty())
                {
                    let refresh_interval = Duration::from_millis(cfg.refresh_interval_ms);
                    let should_refresh_command = match state.last_command_refresh {
                        Some(last_refresh) => {
                            if refresh_interval.is_zero() {
                                true
                            } else {
                                last_refresh.elapsed() >= refresh_interval
                            }
                        }
                        None => true,
                    };

                    if should_refresh_command {
                        state.last_command_refresh = Some(Instant::now());
                        cached_model_display(state, trimmed_model);
                        cached_cleaned_reasoning(state, reasoning);
                        let model_display = state.cached_model_display.as_deref().unwrap_or_default();
                        let trimmed_reasoning = &state.cached_reasoning_cleaned;
                        match run_status_line_command(
                            &command,
                            workspace,
                            trimmed_model,
                            model_display,
                            trimmed_reasoning,
                            state.git_summary.as_ref(),
                            cfg,
                        )
                        .await
                        {
                            Ok(output) => {
                                state.command_value = output;
                            }
                            Err(error) => {
                                command_error = Some(error);
                            }
                        }
                    }

                    (state.command_value.clone(), None)
                } else {
                    state.command_value = None;
                    auto_status_layout(state).into_status()
                }
            } else {
                state.command_value = None;
                auto_status_layout(state).into_status()
            }
        }
    };

    if state.input_status_sync_pending || state.left != left || state.right != right {
        handle.set_configured_input_status(left.clone(), right.clone());
        state.left = left;
        state.right = right;
        state.input_status_sync_pending = false;
    }

    if let Some(error) = command_error {
        Err(error)
    } else {
        Ok(())
    }
}

fn auto_status_layout(state: &InputStatusState) -> BottomStatusLayout {
    let right_components =
        auto_status_components(state.thread_context.as_deref(), state.is_cancelling, state.spooled_files_count, state);
    let right = right_components
        .into_iter()
        .filter(|component| {
            state
                .git_summary
                .as_ref()
                .is_none_or(|summary| !component.trim().eq_ignore_ascii_case(summary.branch.trim()))
        })
        .collect();
    BottomStatusLayout {
        left: state.git_left.clone().into_iter().collect(),
        right,
    }
}

/// Build model status with all context indicators including spooled files
#[cfg(test)]
fn build_model_status_with_context_and_spooled(
    thread_context: Option<&str>,
    is_cancelling: bool,
    spooled_files: Option<usize>,
) -> Option<String> {
    let default_state = InputStatusState::default();
    join_status_components(auto_status_components(thread_context, is_cancelling, spooled_files, &default_state))
}

fn auto_status_components(
    thread_context: Option<&str>,
    is_cancelling: bool,
    spooled_files: Option<usize>,
    state: &InputStatusState,
) -> Vec<String> {
    let mut parts: Vec<Option<String>> = Vec::new();
    if is_cancelling {
        parts.push(Some("CANCELLING...".to_string()));
    }
    if state.is_blocked {
        parts.push(Some("[BLOCKED]".to_string()));
    }

    parts.push(
        thread_context
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    );

    // Provider-specific balance/cost/cache display (DeepSeek, OpenAI)
    if state.show_costs {
        if let Some(bal) = &state.balance {
            parts.push(Some(format!("Balance: {bal}")));
        }
        if let Some(cost) = state.cost_usd {
            parts.push(Some(format!("Cost: ${cost:.4}")));
        }
        if let Some(pct) = state.cache_hit_pct {
            parts.push(Some(format!("Cache: {pct:.0}%")));
        }
    }

    if let Some(count) = spooled_files
        && count > 0
    {
        parts.push(Some(format!("{count} spooled")));
    }

    // Current system time, pinned to the right edge of the status line.
    parts.push(state.clock.clone());

    let mut deduped = Vec::new();
    for value in parts.into_iter().flatten() {
        push_unique(&mut deduped, value);
    }
    deduped
}

fn push_unique(values: &mut Vec<String>, value: String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    let normalized = normalize_for_dedup(trimmed);
    if values.iter().any(|existing| normalize_for_dedup(existing) == normalized) {
        return;
    }
    values.push(trimmed.to_string());
}

fn normalize_for_dedup(value: &str) -> String {
    value.split_whitespace().fold(String::new(), |mut acc, s| {
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(s);
        acc
    })
}

fn join_status_components(values: Vec<String>) -> Option<String> {
    let parts = values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

/// Update spooled files count in status state
pub(crate) fn update_spooled_files_count(state: &mut InputStatusState, count: usize) {
    if state.spooled_files_count != Some(count) {
        state.spooled_files_count = Some(count);
        state.status_dirty = true;
    }
}

pub(crate) fn update_thread_context(state: &mut InputStatusState, thread_label: &str, local_agent_count: usize) {
    let mut value = thread_label.trim().to_string();
    if local_agent_count > 0 {
        let suffix = format!("{} agent{}", local_agent_count, if local_agent_count == 1 { "" } else { "s" });
        if value.is_empty() {
            value = suffix;
        } else {
            value.push_str(" · ");
            value.push_str(&suffix);
        }
    }
    let new_value = (!value.trim().is_empty()).then_some(value);
    if state.thread_context != new_value {
        state.thread_context = new_value;
        state.status_dirty = true;
    }
}

/// Refresh account balance from the provider and update the status line.
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(crate) async fn refresh_balance_info(
    provider_client: &dyn LLMProvider,
    handle: &InlineHandle,
    workspace: &Path,
    model: &str,
    reasoning_effort: &str,
    status_config: Option<&StatusLineConfig>,
    state: &mut InputStatusState,
) {
    const BALANCE_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
    const BALANCE_REQUEST_TIMEOUT: Duration = Duration::from_millis(750);

    let stale = state
        .last_balance_refresh
        .map(|t| t.elapsed() >= BALANCE_REFRESH_INTERVAL)
        .unwrap_or(true);
    if !stale {
        return;
    }

    state.last_balance_refresh = Some(Instant::now());

    match tokio::time::timeout(BALANCE_REQUEST_TIMEOUT, provider_client.get_balance()).await {
        Ok(Ok(Some(bal))) => {
            let warn = if bal.is_available { "" } else { " !" };
            let new_balance = Some(format!("{}{}", bal.display, warn));
            if state.balance != new_balance {
                state.balance = new_balance;
                state.status_dirty = true;
            }
            if let Err(e) =
                update_input_status_if_changed(handle, workspace, model, reasoning_effort, status_config, state).await
            {
                tracing::debug!("Failed to refresh status after balance fetch: {e}");
            }
        }
        Ok(Ok(None)) => {
            if state.balance.is_some() {
                state.balance = None;
                state.status_dirty = true;
            }
        }
        Ok(Err(e)) => {
            tracing::debug!("Failed to fetch provider balance: {e}");
        }
        Err(_) => {
            tracing::debug!("Timed out fetching provider balance");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InputStatusState, build_model_status_with_context_and_spooled, cached_cleaned_reasoning, cached_model_display,
        clear_input_status, invalidate_for_config_reload, status_line_shows_clock, sync_terminal_title,
        update_thread_context,
    };
    use vtcode_core::config::StatusLineConfig;
    use vtcode_ui::tui::app::{InlineCommand, InlineHandle};

    fn drain_commands(receiver: &mut tokio::sync::mpsc::UnboundedReceiver<InlineCommand>) -> Vec<InlineCommand> {
        let mut commands = Vec::new();
        while let Ok(command) = receiver.try_recv() {
            commands.push(command);
        }
        commands
    }

    #[test]
    fn status_line_shows_thread_context() {
        let status = build_model_status_with_context_and_spooled(Some("main"), false, None);

        assert_eq!(status.as_deref(), Some("main"));
    }

    #[test]
    fn clearing_input_status_marks_the_cached_layout_for_resync() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut state = InputStatusState {
            left: Some("stale loading".to_string()),
            right: Some("model".to_string()),
            ..Default::default()
        };

        clear_input_status(&handle, &mut state);

        assert!(state.left.is_none());
        assert!(state.right.is_none());
        assert!(state.status_dirty);
        assert!(state.input_status_sync_pending);
        assert!(matches!(
            receiver.try_recv().expect("status clear command"),
            InlineCommand::SetInputStatus { left: None, right: None }
        ));
    }

    #[test]
    fn status_line_shows_spooled_files() {
        let status = build_model_status_with_context_and_spooled(None, false, Some(3));

        assert_eq!(status.as_deref(), Some("3 spooled"));
    }

    #[test]
    fn thread_context_keeps_zero_active_subagent_count_visible() {
        let mut state = InputStatusState::default();
        update_thread_context(&mut state, "main", 0);

        assert_eq!(state.thread_context.as_deref(), Some("main"));
    }

    #[test]
    fn thread_context_shows_compact_agent_badge_when_present() {
        let mut state = InputStatusState::default();
        update_thread_context(&mut state, "main", 2);

        assert_eq!(state.thread_context.as_deref(), Some("main · 2 agents"));
    }

    #[test]
    fn status_line_shows_clock_when_set() {
        let state = InputStatusState {
            clock: Some("14:30:05".to_string()),
            ..Default::default()
        };

        let components = super::auto_status_components(None, false, None, &state);

        assert_eq!(components, vec!["14:30:05"]);
    }

    #[test]
    fn status_line_clock_respects_show_clock_config() {
        assert!(status_line_shows_clock(None), "unset config defaults to showing the clock");

        let on = StatusLineConfig { show_clock: true, ..StatusLineConfig::default() };
        assert!(status_line_shows_clock(Some(&on)));

        let off = StatusLineConfig { show_clock: false, ..StatusLineConfig::default() };
        assert!(!status_line_shows_clock(Some(&off)));
    }

    #[test]
    fn clock_second_ticked_returns_true_on_first_call() {
        let mut state = InputStatusState::default();
        assert!(super::clock_second_ticked(&mut state), "first call must report a tick");
        assert!(state.last_clock_second.is_some(), "second should be tracked");
    }

    #[test]
    fn clock_second_ticked_returns_false_within_same_second() {
        let mut state = InputStatusState::default();
        super::clock_second_ticked(&mut state);
        // A second call within the same second must not report a tick.
        assert!(!super::clock_second_ticked(&mut state), "same-second call must not tick");
    }

    #[test]
    fn update_spooled_files_count_sets_dirty_only_on_change() {
        let mut state = InputStatusState::default();
        super::update_spooled_files_count(&mut state, 3);
        assert!(state.status_dirty, "first set must mark dirty");

        state.status_dirty = false;
        super::update_spooled_files_count(&mut state, 3);
        assert!(!state.status_dirty, "unchanged value must not mark dirty");

        super::update_spooled_files_count(&mut state, 5);
        assert!(state.status_dirty, "changed value must mark dirty");
    }

    #[test]
    fn update_context_budget_sets_dirty_only_on_change() {
        let mut state = InputStatusState::default();
        super::update_context_budget(&mut state, 1000, 10_000);
        assert!(state.status_dirty, "first set must mark dirty");

        state.status_dirty = false;
        super::update_context_budget(&mut state, 1000, 10_000);
        assert!(!state.status_dirty, "unchanged values must not mark dirty");

        super::update_context_budget(&mut state, 2000, 10_000);
        assert!(state.status_dirty, "changed used tokens must mark dirty");
    }

    #[test]
    fn clock_string_second_cleared_when_clock_hidden() {
        // Simulate a prior formatted clock.
        let mut state = InputStatusState {
            clock: Some("14:30:05".to_string()),
            clock_string_second: Some(1234567890),
            ..Default::default()
        };

        // When show_clock is false, the clock and its tracking second must clear.
        let off = StatusLineConfig { show_clock: false, ..StatusLineConfig::default() };
        super::update_clock(&mut state, status_line_shows_clock(Some(&off)));

        assert!(state.clock.is_none(), "clock must clear when disabled");
        assert!(state.clock_string_second.is_none(), "clock second must clear when disabled");
    }

    #[test]
    fn model_cache_invalidates_on_change() {
        let mut state = InputStatusState::default();
        let model_a = "claude-sonnet-4-20250514";
        let model_b = "gpt-4o";

        let display_a = cached_model_display(&mut state, model_a).to_string();

        assert_eq!(state.cached_model_input.as_deref(), Some(model_a), "input must be cached");
        assert!(state.cached_model_display.is_some(), "display must be cached");

        let display_a_again = cached_model_display(&mut state, model_a);
        assert_eq!(display_a, display_a_again, "same model must produce same display");

        let display_b = cached_model_display(&mut state, model_b).to_string();

        assert_eq!(state.cached_model_input.as_deref(), Some(model_b), "cache must update to new model");
        assert_ne!(display_a, display_b, "different models must produce different display names");
    }

    #[test]
    fn reasoning_cache_invalidates_on_change() {
        let mut state = InputStatusState::default();
        let reasoning_a = "high";
        let reasoning_b = "low";

        let cleaned_a = cached_cleaned_reasoning(&mut state, reasoning_a).to_string();

        assert_eq!(state.cached_reasoning_input.as_deref(), Some(reasoning_a), "input must be cached");

        let cleaned_a_again = cached_cleaned_reasoning(&mut state, reasoning_a);
        assert_eq!(cleaned_a, cleaned_a_again, "same reasoning must produce same cleaned text");

        let cleaned_b = cached_cleaned_reasoning(&mut state, reasoning_b).to_string();

        assert_eq!(state.cached_reasoning_input.as_deref(), Some(reasoning_b), "cache must update");
        assert_ne!(cleaned_a, cleaned_b, "different reasoning must produce different cleaned text");
    }

    #[test]
    fn title_dedup_fields_start_empty() {
        let state = InputStatusState::default();
        assert!(state.last_title_items.is_none(), "title items cache must start empty");
        assert!(state.last_title_thread_label.is_none(), "title thread label cache must start empty");
        assert!(state.last_title_git_branch.is_none(), "title git branch cache must start empty");
    }

    #[test]
    fn config_reload_invalidates_status_refresh_gates() {
        let mut state = InputStatusState {
            last_git_refresh: Some(std::time::Instant::now()),
            last_command_refresh: Some(std::time::Instant::now()),
            command_value: Some("stale status".to_string()),
            ..Default::default()
        };

        invalidate_for_config_reload(&mut state);

        assert!(state.status_dirty, "configuration changes must force a status refresh");
        assert!(state.last_git_refresh.is_none(), "Git visibility may have changed");
        assert!(state.last_command_refresh.is_none(), "a command/config change must run immediately");
        assert!(state.command_value.is_none(), "stale command output must not survive reload");
    }

    #[tokio::test]
    async fn configured_status_modes_and_command_reload_preserve_existing_options() {
        use vtcode_core::config::StatusLineMode;

        struct StatusLineCase {
            mode: StatusLineMode,
            command: Option<&'static str>,
            expected_left: Option<&'static str>,
            expected_right: Option<&'static str>,
        }

        let workspace = tempfile::tempdir().unwrap();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut state = InputStatusState {
            thread_context: Some("configured thread".into()),
            ..Default::default()
        };
        for case in [
            StatusLineCase {
                mode: StatusLineMode::Auto,
                command: None,
                expected_left: None,
                expected_right: Some("configured thread"),
            },
            StatusLineCase {
                mode: StatusLineMode::Command,
                command: Some("printf 'Running custom dashboard\\n'"),
                expected_left: Some("Running custom dashboard"),
                expected_right: None,
            },
            StatusLineCase {
                mode: StatusLineMode::Command,
                command: Some("printf 'custom reloaded\\n'"),
                expected_left: Some("custom reloaded"),
                expected_right: None,
            },
            StatusLineCase {
                mode: StatusLineMode::Hidden,
                command: None,
                expected_left: None,
                expected_right: None,
            },
            StatusLineCase {
                mode: StatusLineMode::Command,
                command: None,
                expected_left: None,
                expected_right: Some("configured thread"),
            },
            StatusLineCase {
                mode: StatusLineMode::Command,
                command: Some(" "),
                expected_left: None,
                expected_right: Some("configured thread"),
            },
            StatusLineCase {
                mode: StatusLineMode::Unknown,
                command: None,
                expected_left: None,
                expected_right: Some("configured thread"),
            },
        ] {
            let config = StatusLineConfig {
                mode: case.mode,
                command: case.command.map(str::to_owned),
                refresh_interval_ms: 3_600_000,
                command_timeout_ms: 2_000,
                show_clock: false,
            };
            invalidate_for_config_reload(&mut state);
            super::update_input_status_if_changed(
                &handle,
                workspace.path(),
                "gpt-6.1-sol",
                "high",
                Some(&config),
                &mut state,
            )
            .await
            .unwrap();
            match receiver.try_recv().unwrap() {
                InlineCommand::SetConfiguredInputStatus { left, right } => {
                    assert_eq!(left.as_deref(), case.expected_left);
                    assert_eq!(right.as_deref(), case.expected_right);
                }
                _ => panic!("expected configured status command"),
            }
            assert!(state.clock.is_none(), "show_clock=false must remain effective");
            assert!(receiver.try_recv().is_err(), "one atomic configured update");
            super::update_input_status_if_changed(
                &handle,
                workspace.path(),
                "gpt-6.1-sol",
                "high",
                Some(&config),
                &mut state,
            )
            .await
            .unwrap();
            assert!(receiver.try_recv().is_err(), "unchanged status must stay deduplicated");
        }
    }

    #[test]
    fn terminal_title_sync_only_sends_changed_values() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut state = InputStatusState::default();
        let title_items = vec!["directory".to_string(), "model".to_string()];

        state.thread_context = Some("main".to_string());
        state.git_summary =
            Some(crate::agent::runloop::git::GitStatusSummary { branch: "feature/login".to_string(), dirty: false });

        sync_terminal_title(&handle, &mut state, Some(&title_items));

        let initial_commands = drain_commands(&mut receiver);
        assert_eq!(initial_commands.len(), 3, "each initial title field should be sent once");
        for command in initial_commands {
            match command {
                InlineCommand::SetTerminalTitleItems { items } => assert_eq!(items, Some(title_items.clone())),
                InlineCommand::SetTerminalTitleThreadLabel { label } => assert_eq!(label.as_deref(), Some("main")),
                InlineCommand::SetTerminalTitleGitBranch { branch } => {
                    assert_eq!(branch.as_deref(), Some("feature/login"));
                }
                _ => panic!("unexpected command in terminal title sync"),
            }
        }

        sync_terminal_title(&handle, &mut state, Some(&title_items));
        assert!(receiver.try_recv().is_err(), "unchanged title fields must not enqueue commands");

        state.git_summary.as_mut().expect("git summary set above").branch = "develop".to_string();
        sync_terminal_title(&handle, &mut state, Some(&title_items));
        match receiver.try_recv() {
            Ok(InlineCommand::SetTerminalTitleGitBranch { branch }) => {
                assert_eq!(branch.as_deref(), Some("develop"));
            }
            Ok(_) => panic!("only the changed branch should be sent"),
            Err(error) => panic!("changed branch was not sent: {error}"),
        }
        assert!(receiver.try_recv().is_err(), "only changed title fields should enqueue commands");
    }

    #[test]
    fn blocked_status_indicator_appears_when_blocked() {
        let mut state = InputStatusState::default();
        let normal_components = super::auto_status_components(Some("main"), false, None, &state);
        assert!(!normal_components.contains(&"[BLOCKED]".to_string()));

        state.is_blocked = true;
        let blocked_components = super::auto_status_components(Some("main"), false, None, &state);
        assert!(blocked_components.contains(&"[BLOCKED]".to_string()));
    }
}
