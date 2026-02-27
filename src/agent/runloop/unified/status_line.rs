use std::path::Path;
use std::str::FromStr;
use std::time::{Duration, Instant};

use anyhow::Result;

use vtcode_core::config::constants::ui;
use vtcode_core::config::{StatusLineConfig, StatusLineMode};
use vtcode_core::models::ModelId;
use vtcode_tui::InlineHandle;

use super::status_line_command::run_status_line_command;
use crate::agent::runloop::git::{GitStatusSummary, git_status_summary};
use vtcode_core::llm::providers::clean_reasoning_text;
use vtcode_core::terminal_setup::detector::TerminalType;

#[derive(Default, Clone)]
pub(crate) struct InputStatusState {
    pub(crate) terminal_name: Option<String>,
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
    #[allow(dead_code)]
    pub(crate) semantic_value_per_token: Option<f64>,
    pub(crate) is_cancelling: bool,
    // Dynamic context discovery status
    pub(crate) spooled_files_count: Option<usize>,
    pub(crate) team_label: Option<String>,
    pub(crate) delegate_mode: bool,
}

const GIT_STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// Update context efficiency metrics in the status state
#[allow(dead_code)]
pub(crate) fn update_context_efficiency(
    state: &mut InputStatusState,
    utilization: f64,
    tokens: usize,
    semantic_per_token: f64,
) {
    state.context_utilization = Some(utilization);
    state.context_tokens = Some(tokens);
    state.semantic_value_per_token = Some(semantic_per_token);
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

pub(crate) fn clear_context_budget(state: &mut InputStatusState) {
    state.context_utilization = None;
    state.context_tokens = None;
    state.context_limit_tokens = None;
    state.context_remaining_tokens = None;
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
    let mode = status_config
        .map(|cfg| cfg.mode.clone())
        .unwrap_or(StatusLineMode::Auto);

    if matches!(mode, StatusLineMode::Hidden) {
        state.last_git_refresh = None;
        state.git_left = None;
        state.git_summary = None;
        state.terminal_name = None;
    } else {
        // Detect terminal name if not already cached
        if state.terminal_name.is_none() {
            state.terminal_name = TerminalType::detect()
                .ok()
                .filter(|t| *t != TerminalType::Unknown)
                .map(|t| t.name().to_string());
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
                    let git_display = if summary.branch.is_empty() {
                        indicator.to_string()
                    } else {
                        format!("{}{}", summary.branch, indicator)
                    };

                    let display = if let Some(term) = &state.terminal_name {
                        format!("{} | {}", term, git_display)
                    } else {
                        git_display
                    };

                    state.git_left = Some(display);
                    state.git_summary = Some(summary);
                }
                Ok(None) => {
                    state.git_left = state.terminal_name.clone();
                    state.git_summary = None;
                }
                Err(error) => {
                    tracing::warn!(
                        workspace = %workspace.display(),
                        error = ?error,
                        "Failed to resolve git status"
                    );
                    state.git_summary = None;
                    state.git_left = state.terminal_name.clone();
                }
            }

            state.last_git_refresh = Some(Instant::now());
        }
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
            let right = build_model_status_with_context_and_spooled(
                trimmed_model,
                trimmed_reasoning,
                state.context_utilization,
                state.context_tokens,
                state.is_cancelling,
                state.spooled_files_count,
                state.team_label.as_deref(),
                state.delegate_mode,
            );
            (state.git_left.clone(), right)
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
                    let right = build_model_status_with_context_and_spooled(
                        trimmed_model,
                        trimmed_reasoning,
                        state.context_utilization,
                        state.context_tokens,
                        state.is_cancelling,
                        state.spooled_files_count,
                        state.team_label.as_deref(),
                        state.delegate_mode,
                    );
                    (state.git_left.clone(), right)
                }
            } else {
                state.command_value = None;
                let right = build_model_status_with_context_and_spooled(
                    trimmed_model,
                    trimmed_reasoning,
                    state.context_utilization,
                    state.context_tokens,
                    state.is_cancelling,
                    state.spooled_files_count,
                    state.team_label.as_deref(),
                    state.delegate_mode,
                );
                (state.git_left.clone(), right)
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

#[allow(dead_code)]
fn build_model_status_right(model: &str, reasoning: &str) -> Option<String> {
    let mut parts = Vec::new();

    if model.is_empty() {
        None
    } else if reasoning.is_empty() {
        if parts.is_empty() {
            Some(model.to_string())
        } else {
            parts.push(model.to_string());
            Some(parts.join(" | "))
        }
    } else {
        parts.push(format!("{} ({})", model, reasoning));
        Some(parts.join(" | "))
    }
}

/// Build status display with context efficiency metrics
///
/// Format: "model | 65% context left"
#[allow(dead_code)]
pub(crate) fn build_model_status_with_context(
    model: &str,
    reasoning: &str,
    context_utilization: Option<f64>,
    total_tokens: Option<usize>,
    is_cancelling: bool,
) -> Option<String> {
    build_model_status_with_context_and_spooled(
        model,
        reasoning,
        context_utilization,
        total_tokens,
        is_cancelling,
        None,
        None,
        false,
    )
}

/// Build model status with all context indicators including spooled files
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_model_status_with_context_and_spooled(
    model: &str,
    reasoning: &str,
    context_utilization: Option<f64>,
    total_tokens: Option<usize>,
    is_cancelling: bool,
    spooled_files: Option<usize>,
    team_label: Option<&str>,
    delegate_mode: bool,
) -> Option<String> {
    let mut parts = Vec::new();

    if is_cancelling {
        parts.push("CANCELLING...".to_string());
    }

    parts.push(model.to_string());

    if total_tokens.is_some_and(|tokens| tokens > 0)
        && let Some(util) = context_utilization
    {
        parts.push(format!("{:.0}% context left", util.clamp(0.0, 100.0)));
    }

    // Show spooled files indicator when files have been spooled
    if let Some(count) = spooled_files
        && count > 0
    {
        parts.push(format!("{} spooled", count));
    }

    if let Some(label) = team_label
        && !label.trim().is_empty()
    {
        parts.push(label.trim().to_string());
    }

    if delegate_mode {
        parts.push("delegate".to_string());
    }

    if !reasoning.is_empty() {
        parts.push(format!("({})", reasoning));
    }

    Some(parts.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::build_model_status_with_context_and_spooled;

    #[test]
    fn status_line_shows_context_left_percent() {
        let status = build_model_status_with_context_and_spooled(
            "gemini-3-flash-preview",
            "low",
            Some(17.0),
            Some(83_000),
            false,
            None,
            None,
            false,
        );

        assert_eq!(
            status.as_deref(),
            Some("gemini-3-flash-preview | 17% context left | (low)")
        );
    }

    #[test]
    fn status_line_clamps_context_left_percent() {
        let high = build_model_status_with_context_and_spooled(
            "model",
            "",
            Some(150.0),
            Some(1_000),
            false,
            None,
            None,
            false,
        );
        let low = build_model_status_with_context_and_spooled(
            "model",
            "",
            Some(-10.0),
            Some(1_000),
            false,
            None,
            None,
            false,
        );

        assert_eq!(high.as_deref(), Some("model | 100% context left"));
        assert_eq!(low.as_deref(), Some("model | 0% context left"));
    }

    #[test]
    fn status_line_hides_context_left_when_total_tokens_zero() {
        let status = build_model_status_with_context_and_spooled(
            "model",
            "low",
            Some(100.0),
            Some(0),
            false,
            None,
            None,
            false,
        );

        assert_eq!(status.as_deref(), Some("model | (low)"));
    }
}

/// Update spooled files count in status state
pub(crate) fn update_spooled_files_count(state: &mut InputStatusState, count: usize) {
    state.spooled_files_count = Some(count);
}

pub(crate) fn update_team_status(
    state: &mut InputStatusState,
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
) {
    let Some(team_context) = session_stats.team_context.as_ref() else {
        state.team_label = None;
        state.delegate_mode = false;
        return;
    };

    let label = match team_context.role {
        vtcode_core::agent_teams::TeamRole::Lead => session_stats
            .team_state
            .as_ref()
            .and_then(|team| team.active_teammate())
            .map(|name| format!("team:{} -> {}", team_context.team_name, name))
            .unwrap_or_else(|| format!("team:{} (lead)", team_context.team_name)),
        vtcode_core::agent_teams::TeamRole::Teammate => {
            let name = team_context.teammate_name.as_deref().unwrap_or("teammate");
            format!("team:{} as {}", team_context.team_name, name)
        }
    };

    state.team_label = Some(label);
    state.delegate_mode = session_stats.is_delegate_mode();
}
