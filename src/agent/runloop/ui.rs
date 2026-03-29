use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, RgbColor};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::Path;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::constants::ui;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::subagents::SubagentStatusEntry;
use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
use vtcode_core::tools::search_tool_bundle_status;
use vtcode_core::utils::dot_config::load_workspace_trust_level;
use vtcode_tui::app::{
    InlineHandle, InlineHeaderBadge, InlineHeaderContext, InlineHeaderStatusBadge,
    InlineHeaderStatusTone, InlineTextStyle,
};
use vtcode_tui::core::ThemeConfigParser;

use tracing::warn;

use super::git::git_status_summary;
use super::welcome::SessionBootstrap;
use dirs::home_dir;

const MAX_VISIBLE_SUBAGENT_BADGES: usize = 3;
const SUBAGENT_BADGE_FALLBACK_COLOR: &str = "blue";

#[derive(Clone, Debug)]
enum ToolStatusSummary {
    Available {
        allow: usize,
        prompt: usize,
        deny: usize,
    },
    Unavailable,
}

#[derive(Clone, Debug)]
enum McpStatusSummary {
    Enabled {
        active_providers: Vec<String>,
        configured: bool,
    },
    Disabled,
    Error(String),
    Unknown,
}

#[derive(Clone, Debug)]
struct InlineStatusDetails {
    workspace_trust: Option<WorkspaceTrustLevel>,
    tool_status: ToolStatusSummary,
    mcp_status: McpStatusSummary,
}

async fn gather_inline_status_details(
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
) -> Result<InlineStatusDetails> {
    let workspace_trust = if session_bootstrap.acp_workspace_trust.is_some() {
        None
    } else {
        load_workspace_trust_level(&config.workspace)
            .await
            .context("Failed to determine workspace trust level for banner")?
    };

    let tool_status = match ToolPolicyManager::new_with_workspace(&config.workspace).await {
        Ok(manager) => {
            let summary = manager.get_policy_summary();
            let mut allow = 0usize;
            let mut prompt = 0usize;
            let mut deny = 0usize;
            for policy in summary.values() {
                match policy {
                    ToolPolicy::Allow => allow += 1,
                    ToolPolicy::Prompt => prompt += 1,
                    ToolPolicy::Deny => deny += 1,
                }
            }
            ToolStatusSummary::Available {
                allow,
                prompt,
                deny,
            }
        }
        Err(err) => {
            warn!("failed to load tool policy summary: {err:#}");
            ToolStatusSummary::Unavailable
        }
    };

    let mcp_status = if let Some(error) = &session_bootstrap.mcp_error {
        McpStatusSummary::Error(error.clone())
    } else if let Some(enabled) = session_bootstrap.mcp_enabled {
        if enabled {
            let configured = session_bootstrap.mcp_providers.is_some();
            let active_providers = session_bootstrap
                .mcp_providers
                .as_ref()
                .map(|providers| {
                    providers
                        .iter()
                        .filter(|provider| provider.enabled)
                        .map(|provider| provider.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            McpStatusSummary::Enabled {
                active_providers,
                configured,
            }
        } else {
            McpStatusSummary::Disabled
        }
    } else {
        McpStatusSummary::Unknown
    };

    Ok(InlineStatusDetails {
        workspace_trust,
        tool_status,
        mcp_status,
    })
}

fn is_home_directory(workspace_path: &Path) -> bool {
    if let Some(home_dir) = home_dir() {
        return workspace_path == home_dir;
    }
    false
}

pub(crate) fn build_search_tools_badge(workspace: &Path) -> InlineHeaderStatusBadge {
    let status = search_tool_bundle_status(workspace);
    let tone = if status.all_ready() {
        InlineHeaderStatusTone::Ready
    } else if status.has_errors() || status.all_unavailable() {
        InlineHeaderStatusTone::Error
    } else {
        InlineHeaderStatusTone::Warning
    };

    InlineHeaderStatusBadge {
        text: status.header_summary(),
        tone,
    }
}

pub(crate) async fn build_inline_header_context(
    config: &CoreAgentConfig,
    session_bootstrap: &SessionBootstrap,
    provider_label: String,
    model_label: String,
    context_window_size: usize,
    mode_label: String,
    reasoning_label: String,
) -> Result<InlineHeaderContext> {
    let InlineStatusDetails {
        workspace_trust,
        tool_status,
        mcp_status,
    } = gather_inline_status_details(config, session_bootstrap).await?;

    // Check if we're running in the home directory and add a warning if so
    let mut highlights = session_bootstrap.header_highlights.clone();
    if is_home_directory(&config.workspace) {
        highlights.push(vtcode_tui::app::InlineHeaderHighlight {
            title: "Warning".to_string(),
            lines: vec![
                "You are running VT Code in your home directory. It is recommended to run in a project-specific directory for better organization and safety."
                    .to_string(),
            ],
        });
    }

    let git_value = match git_status_summary(&config.workspace) {
        Ok(Some(summary)) => {
            let suffix = if summary.dirty {
                ui::HEADER_GIT_DIRTY_SUFFIX
            } else {
                ui::HEADER_GIT_CLEAN_SUFFIX
            };
            format!("{}{}{}", ui::HEADER_GIT_PREFIX, summary.branch, suffix)
        }
        Ok(None) => format!(
            "{}{}",
            ui::HEADER_GIT_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
        Err(error) => {
            warn!(
                workspace = %config.workspace.display(),
                error = ?error,
                "Failed to read git status for inline header"
            );
            format!(
                "{}{}",
                ui::HEADER_GIT_PREFIX,
                ui::HEADER_UNKNOWN_PLACEHOLDER
            )
        }
    };

    let version = env!("CARGO_PKG_VERSION").to_string();
    let provider_value = if provider_label.trim().is_empty() {
        format!(
            "{}{}",
            ui::HEADER_PROVIDER_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        )
    } else {
        format!("{}{}", ui::HEADER_PROVIDER_PREFIX, provider_label.trim())
    };
    let model_value = if model_label.trim().is_empty() {
        format!(
            "{}{}",
            ui::HEADER_MODEL_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        )
    } else {
        format!("{}{}", ui::HEADER_MODEL_PREFIX, model_label.trim())
    };
    let trimmed_mode = mode_label.trim();
    let mode = if trimmed_mode.is_empty() {
        ui::HEADER_MODE_INLINE.to_string()
    } else {
        trimmed_mode.to_string()
    };

    let reasoning = if reasoning_label.trim().is_empty() {
        format!(
            "{}{}",
            ui::HEADER_REASONING_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        )
    } else {
        format!("{}{}", ui::HEADER_REASONING_PREFIX, reasoning_label.trim())
    };

    let trust_value = match session_bootstrap.acp_workspace_trust {
        Some(level) => {
            let level_str = match level {
                vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::FullAuto => {
                    "full_auto"
                }
                vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy => {
                    "tools_policy"
                }
            };
            format!("{}acp:{}", ui::HEADER_TRUST_PREFIX, level_str)
        }
        None => match workspace_trust {
            Some(level) => format!("{}{}", ui::HEADER_TRUST_PREFIX, level),
            None => format!(
                "{}{}",
                ui::HEADER_TRUST_PREFIX,
                ui::HEADER_UNKNOWN_PLACEHOLDER
            ),
        },
    };

    let tools_value = match tool_status {
        ToolStatusSummary::Available {
            allow,
            prompt,
            deny,
        } => format!(
            "{}allow {} · prompt {} · deny {}",
            ui::HEADER_TOOLS_PREFIX,
            allow,
            prompt,
            deny
        ),
        ToolStatusSummary::Unavailable => format!(
            "{}{}",
            ui::HEADER_TOOLS_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
    };

    let mcp_value = match mcp_status {
        McpStatusSummary::Error(message) => {
            format!("{}error - {}", ui::HEADER_MCP_PREFIX, message)
        }
        McpStatusSummary::Enabled {
            active_providers,
            configured,
        } => {
            if !active_providers.is_empty() {
                format!(
                    "{}enabled ({})",
                    ui::HEADER_MCP_PREFIX,
                    active_providers.join(", ")
                )
            } else if configured {
                format!("{}enabled (no providers)", ui::HEADER_MCP_PREFIX)
            } else {
                format!("{}enabled", ui::HEADER_MCP_PREFIX)
            }
        }
        McpStatusSummary::Disabled => format!("{}disabled", ui::HEADER_MCP_PREFIX),
        McpStatusSummary::Unknown => format!(
            "{}{}",
            ui::HEADER_MCP_PREFIX,
            ui::HEADER_UNKNOWN_PLACEHOLDER
        ),
    };

    let context = InlineHeaderContext {
        app_name: vtcode_core::config::constants::app::DISPLAY_NAME.to_string(),
        provider: provider_value,
        model: model_value,
        context_window_size: Some(context_window_size),
        version,
        search_tools: Some(build_search_tools_badge(&config.workspace)),
        persistent_memory: None,
        pr_review: None,
        editor_context: None,
        git: git_value,
        mode,
        reasoning,
        workspace_trust: trust_value,
        tools: tools_value,
        mcp: mcp_value,
        highlights, // Use the modified highlights that may include the home directory warning
        subagent_badges: Vec::new(),
        editing_mode: vtcode_tui::app::EditingMode::default(),
        autonomous_mode: false,
        reasoning_stage: None,
    };
    Ok(context)
}

pub(crate) fn sync_active_subagent_badges(
    header_context: &mut InlineHeaderContext,
    handle: &InlineHandle,
    entries: &[SubagentStatusEntry],
) {
    let next = build_active_subagent_badges(entries);
    if header_context.subagent_badges != next {
        header_context.subagent_badges = next;
        handle.set_header_context(header_context.clone());
    }
}

fn build_active_subagent_badges(entries: &[SubagentStatusEntry]) -> Vec<InlineHeaderBadge> {
    let mut grouped: BTreeMap<(String, Option<String>), usize> = BTreeMap::new();
    for entry in entries.iter().filter(|entry| !entry.status.is_terminal()) {
        let key = (entry.agent_name.clone(), entry.color.clone());
        *grouped.entry(key).or_default() += 1;
    }

    let mut badges = grouped
        .into_iter()
        .map(|((agent_name, color), count)| InlineHeaderBadge {
            text: if count > 1 {
                format!("{agent_name} ×{count}")
            } else {
                agent_name
            },
            style: build_subagent_badge_style(color.as_deref()),
            full_background: true,
        })
        .collect::<Vec<_>>();

    if badges.len() > MAX_VISIBLE_SUBAGENT_BADGES {
        let hidden = badges.len() - MAX_VISIBLE_SUBAGENT_BADGES;
        badges.truncate(MAX_VISIBLE_SUBAGENT_BADGES);
        badges.push(InlineHeaderBadge {
            text: format!("+{hidden}"),
            style: build_subagent_badge_style(Some("bright black")),
            full_background: true,
        });
    }

    badges
}

fn build_subagent_badge_style(color_spec: Option<&str>) -> InlineTextStyle {
    let parser = ThemeConfigParser::default();
    let parsed = color_spec
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| parser.parse_flexible(value).ok())
        .or_else(|| parser.parse_flexible(SUBAGENT_BADGE_FALLBACK_COLOR).ok());

    let effects = parsed
        .as_ref()
        .map(|style| style.get_effects() | Effects::BOLD)
        .unwrap_or(Effects::BOLD);

    match parsed {
        Some(style) => {
            let parsed_fg = style.get_fg_color();
            let parsed_bg = style.get_bg_color();
            let background = parsed_bg.or(parsed_fg);
            let foreground = parsed_fg
                .filter(|_| parsed_bg.is_some())
                .or_else(|| background.map(contrasting_badge_text_color))
                .or(Some(AnsiColor::White.into()));
            InlineTextStyle {
                color: foreground,
                bg_color: background,
                effects,
            }
        }
        None => InlineTextStyle {
            color: Some(AnsiColor::White.into()),
            bg_color: Some(AnsiColor::Blue.into()),
            effects,
        },
    }
}

fn contrasting_badge_text_color(background: AnsiColorEnum) -> AnsiColorEnum {
    match background {
        AnsiColorEnum::Rgb(rgb) => {
            if relative_luminance(rgb) >= 0.55 {
                AnsiColor::Black.into()
            } else {
                AnsiColor::White.into()
            }
        }
        AnsiColorEnum::Ansi(color) => match color {
            AnsiColor::White
            | AnsiColor::BrightWhite
            | AnsiColor::Yellow
            | AnsiColor::BrightYellow
            | AnsiColor::Cyan
            | AnsiColor::BrightCyan
            | AnsiColor::Green
            | AnsiColor::BrightGreen => AnsiColor::Black.into(),
            _ => AnsiColor::White.into(),
        },
        AnsiColorEnum::Ansi256(value) => {
            if value.index() >= 244 {
                AnsiColor::Black.into()
            } else {
                AnsiColor::White.into()
            }
        }
    }
}

fn relative_luminance(rgb: RgbColor) -> f32 {
    let normalize = |component: u8| component as f32 / 255.0;
    (0.2126 * normalize(rgb.0)) + (0.7152 * normalize(rgb.1)) + (0.0722 * normalize(rgb.2))
}

#[cfg(test)]
mod tests {
    use super::{build_active_subagent_badges, build_subagent_badge_style};
    use anstyle::{AnsiColor, Color as AnsiColorEnum, RgbColor};
    use chrono::Utc;
    use vtcode_core::subagents::{SubagentStatus, SubagentStatusEntry};

    fn status_entry(
        agent_name: &str,
        color: Option<&str>,
        status: SubagentStatus,
    ) -> SubagentStatusEntry {
        SubagentStatusEntry {
            id: format!("id-{agent_name}"),
            session_id: "session".to_string(),
            parent_thread_id: "parent".to_string(),
            agent_name: agent_name.to_string(),
            display_label: agent_name.to_string(),
            description: "test".to_string(),
            source: "builtin".to_string(),
            color: color.map(ToString::to_string),
            status,
            background: false,
            depth: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            summary: None,
            error: None,
            transcript_path: None,
            nickname: None,
        }
    }

    #[test]
    fn active_subagent_badges_group_duplicate_agent_names() {
        let badges = build_active_subagent_badges(&[
            status_entry("worker", Some("magenta"), SubagentStatus::Running),
            status_entry("worker", Some("magenta"), SubagentStatus::Waiting),
            status_entry("explorer", Some("cyan"), SubagentStatus::Completed),
        ]);

        assert_eq!(badges.len(), 1);
        assert_eq!(badges[0].text, "worker ×2");
        assert!(badges[0].full_background);
    }

    #[test]
    fn subagent_badge_style_promotes_single_color_to_background() {
        let style = build_subagent_badge_style(Some("#4f8fd8"));
        assert_eq!(
            style.bg_color,
            Some(AnsiColorEnum::Rgb(RgbColor(0x4F, 0x8F, 0xD8)))
        );
        assert_eq!(style.color, Some(AnsiColor::White.into()));
    }

    #[test]
    fn subagent_badge_style_preserves_explicit_foreground_and_background() {
        let style = build_subagent_badge_style(Some("white #4f8fd8"));
        assert_eq!(
            style.bg_color,
            Some(AnsiColorEnum::Rgb(RgbColor(0x4F, 0x8F, 0xD8)))
        );
        assert_eq!(style.color, Some(AnsiColor::White.into()));
    }
}
