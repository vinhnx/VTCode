use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::Result;

use vtcode_core::config::constants::ui;
use vtcode_core::config::{StatusLineConfig, StatusLineMode};
use vtcode_core::models::ModelId;
use vtcode_tui::app::InlineHandle;

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
    pub(crate) ide_context_source: Option<String>,
    // Dynamic context discovery status
    pub(crate) spooled_files_count: Option<usize>,
    pub(crate) thread_context: Option<String>,
}

const GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Default)]
struct BottomStatusLayout {
    left: Vec<String>,
    right: Vec<String>,
}

impl BottomStatusLayout {
    fn push_left(&mut self, value: Option<String>) {
        if let Some(value) = value {
            push_unique(&mut self.left, value);
        }
    }

    fn push_right(&mut self, value: Option<String>) {
        if let Some(value) = value {
            push_unique(&mut self.right, value);
        }
    }

    fn into_status(self) -> (Option<String>, Option<String>) {
        (
            join_status_components(self.left),
            join_status_components(self.right),
        )
    }
}

pub(crate) fn update_context_budget(
    state: &mut InputStatusState,
    used_tokens: usize,
    limit_tokens: usize,
) {
    if limit_tokens == 0 {
        state.context_utilization = None;
        state.context_tokens = None;
        state.context_limit_tokens = None;
        state.context_remaining_tokens = None;
        return;
    }

    let used = used_tokens.min(limit_tokens);
    let remaining = limit_tokens.saturating_sub(used);
    let left_percent = (remaining as f64 / limit_tokens as f64) * 100.0;

    state.context_utilization = Some(left_percent.clamp(0.0, 100.0));
    state.context_tokens = Some(used);
    state.context_limit_tokens = Some(limit_tokens);
    state.context_remaining_tokens = Some(remaining);
}

#[allow(clippy::too_many_arguments)]
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
    let mode = status_config
        .map(|cfg| cfg.effective_mode())
        .unwrap_or(StatusLineMode::Auto);
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
    let cleaned_reasoning = clean_reasoning_text(reasoning.trim());
    let trimmed_reasoning = cleaned_reasoning.as_str();
    let model_display = ModelId::from_str(trimmed_model)
        .map(|id| id.display_name().to_string())
        .unwrap_or_else(|_| trimmed_model.to_string());

    let mut command_error: Option<anyhow::Error> = None;

    let (left, right) = match mode {
        StatusLineMode::Hidden => {
            state.command_value = None;
            state.last_command_refresh = None;
            (None, None)
        }
        StatusLineMode::Auto => {
            let mut layout = BottomStatusLayout::default();
            layout.push_left(state.git_left.clone());
            for component in auto_status_components(
                state.thread_context.as_deref(),
                state.ide_context_source.as_deref(),
                state.git_summary.as_ref(),
                trimmed_model,
                trimmed_reasoning,
                state.context_utilization,
                state.context_tokens,
                state.is_cancelling,
                state.spooled_files_count,
            ) {
                layout.push_right(Some(component));
            }
            layout.into_status()
        }
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
                        match run_status_line_command(
                            &command,
                            workspace,
                            trimmed_model,
                            &model_display,
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
                    let mut layout = BottomStatusLayout::default();
                    layout.push_left(state.git_left.clone());
                    for component in auto_status_components(
                        state.thread_context.as_deref(),
                        state.ide_context_source.as_deref(),
                        state.git_summary.as_ref(),
                        trimmed_model,
                        trimmed_reasoning,
                        state.context_utilization,
                        state.context_tokens,
                        state.is_cancelling,
                        state.spooled_files_count,
                    ) {
                        layout.push_right(Some(component));
                    }
                    layout.into_status()
                }
            } else {
                state.command_value = None;
                let mut layout = BottomStatusLayout::default();
                layout.push_left(state.git_left.clone());
                for component in auto_status_components(
                    state.thread_context.as_deref(),
                    state.ide_context_source.as_deref(),
                    state.git_summary.as_ref(),
                    trimmed_model,
                    trimmed_reasoning,
                    state.context_utilization,
                    state.context_tokens,
                    state.is_cancelling,
                    state.spooled_files_count,
                ) {
                    layout.push_right(Some(component));
                }
                layout.into_status()
            }
        }
    };

    if state.left != left || state.right != right {
        handle.set_input_status(left.clone(), right.clone());
        state.left = left;
        state.right = right;
    }

    if let Some(error) = command_error {
        Err(error)
    } else {
        Ok(())
    }
}

/// Build model status with all context indicators including spooled files
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn build_model_status_with_context_and_spooled(
    thread_context: Option<&str>,
    ide_context_source: Option<&str>,
    model: &str,
    reasoning: &str,
    context_utilization: Option<f64>,
    _total_tokens: Option<usize>,
    is_cancelling: bool,
    spooled_files: Option<usize>,
) -> Option<String> {
    join_status_components(auto_status_components(
        thread_context,
        ide_context_source,
        None,
        model,
        reasoning,
        context_utilization,
        _total_tokens,
        is_cancelling,
        spooled_files,
    ))
}

#[allow(clippy::too_many_arguments)]
fn auto_status_components(
    thread_context: Option<&str>,
    ide_context_source: Option<&str>,
    git_summary: Option<&GitStatusSummary>,
    model: &str,
    reasoning: &str,
    context_utilization: Option<f64>,
    _total_tokens: Option<usize>,
    is_cancelling: bool,
    spooled_files: Option<usize>,
) -> Vec<String> {
    let mut parts: Vec<Option<String>> = Vec::new();
    if is_cancelling {
        parts.push(Some("CANCELLING...".to_string()));
    }

    parts.push(normalize_thread_context(thread_context, git_summary));
    parts.push(
        ide_context_source
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    );
    parts.push((!model.trim().is_empty()).then(|| model.trim().to_string()));

    if let Some(util) = context_utilization {
        parts.push(Some(format!("{:.0}% context left", util.clamp(0.0, 100.0))));
    }

    if let Some(count) = spooled_files
        && count > 0
    {
        parts.push(Some(format!("{count} spooled")));
    }

    if !reasoning.trim().is_empty() {
        parts.push(Some(format!("({})", reasoning.trim())));
    }

    let mut deduped = Vec::new();
    for value in parts.into_iter().flatten() {
        push_unique(&mut deduped, value);
    }
    deduped
}

fn normalize_thread_context(
    thread_context: Option<&str>,
    git_summary: Option<&GitStatusSummary>,
) -> Option<String> {
    let thread = thread_context
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    let Some(branch) = git_summary
        .and_then(|summary| {
            let value = summary.branch.trim();
            (!value.is_empty()).then_some(value)
        })
        .map(normalize_for_dedup)
    else {
        return Some(thread.to_string());
    };

    let thread_normalized = normalize_for_dedup(thread);
    if thread_normalized == branch {
        return None;
    }

    if let Some((head, tail)) = thread.split_once('|')
        && normalize_for_dedup(head) == branch
    {
        let tail = tail.trim();
        if tail.is_empty() {
            None
        } else {
            Some(tail.to_string())
        }
    } else {
        Some(thread.to_string())
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return;
    }
    let normalized = normalize_for_dedup(trimmed);
    if values
        .iter()
        .any(|existing| normalize_for_dedup(existing) == normalized)
    {
        return;
    }
    values.push(trimmed.to_string());
}

fn normalize_for_dedup(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
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
        Some(parts.join(" | "))
    }
}

/// Update spooled files count in status state
pub(crate) fn update_spooled_files_count(state: &mut InputStatusState, count: usize) {
    state.spooled_files_count = Some(count);
}

pub(crate) fn update_ide_context_source(state: &mut InputStatusState, source: Option<String>) {
    state.ide_context_source = source;
}

pub(crate) fn update_thread_context(
    state: &mut InputStatusState,
    thread_label: &str,
    local_agent_count: usize,
) {
    let mut value = thread_label.trim().to_string();
    if local_agent_count > 0 {
        let suffix = format!(
            "{} local agent{} | ↓ explore",
            local_agent_count,
            if local_agent_count == 1 { "" } else { "s" }
        );
        if value.is_empty() {
            value = suffix;
        } else {
            value.push_str(" | ");
            value.push_str(&suffix);
        }
    }
    state.thread_context = (!value.trim().is_empty()).then_some(value);
}

#[cfg(test)]
mod tests {
    use super::{
        InputStatusState, build_model_status_with_context_and_spooled, normalize_thread_context,
        update_thread_context,
    };
    use crate::agent::runloop::git::GitStatusSummary;

    #[test]
    fn status_line_shows_context_left_percent() {
        let status = build_model_status_with_context_and_spooled(
            None,
            None,
            "gemini-3-flash-preview",
            "low",
            Some(17.0),
            Some(83_000),
            false,
            None,
        );

        assert_eq!(
            status.as_deref(),
            Some("gemini-3-flash-preview | 17% context left | (low)")
        );
    }

    #[test]
    fn status_line_clamps_context_left_percent() {
        let high = build_model_status_with_context_and_spooled(
            None,
            None,
            "model",
            "",
            Some(150.0),
            Some(1_000),
            false,
            None,
        );
        let low = build_model_status_with_context_and_spooled(
            None,
            None,
            "model",
            "",
            Some(-10.0),
            Some(1_000),
            false,
            None,
        );

        assert_eq!(high.as_deref(), Some("model | 100% context left"));
        assert_eq!(low.as_deref(), Some("model | 0% context left"));
    }

    #[test]
    fn status_line_includes_compact_ide_context_source() {
        let status = build_model_status_with_context_and_spooled(
            None,
            Some("IDE Context (VS Code): vtcode-config/src/core/agent.rs"),
            "model",
            "",
            None,
            None,
            false,
            None,
        );

        assert_eq!(
            status.as_deref(),
            Some("IDE Context (VS Code): vtcode-config/src/core/agent.rs | model")
        );
    }

    #[test]
    fn thread_context_keeps_zero_active_subagent_count_visible() {
        let mut state = InputStatusState::default();
        update_thread_context(&mut state, "main", 0);

        assert_eq!(state.thread_context.as_deref(), Some("main"));
    }

    #[test]
    fn thread_context_shows_local_agent_badge_when_present() {
        let mut state = InputStatusState::default();
        update_thread_context(&mut state, "main", 2);

        assert_eq!(
            state.thread_context.as_deref(),
            Some("main | 2 local agents | ↓ explore")
        );
    }

    #[test]
    fn thread_context_hides_duplicate_git_branch_label() {
        let thread = normalize_thread_context(
            Some("main"),
            Some(&GitStatusSummary {
                branch: "main".to_string(),
                dirty: false,
            }),
        );
        assert_eq!(thread, None);
    }

    #[test]
    fn thread_context_strips_duplicate_branch_prefix_when_suffix_exists() {
        let thread = normalize_thread_context(
            Some("main | 2 local agents | ↓ explore"),
            Some(&GitStatusSummary {
                branch: "main".to_string(),
                dirty: true,
            }),
        );
        assert_eq!(thread.as_deref(), Some("2 local agents | ↓ explore"));
    }
}
