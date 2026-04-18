use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, RgbColor};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::constants::ui;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::subagents::SubagentStatusEntry;
use vtcode_core::tools::search_tool_bundle_status;
use vtcode_core::utils::dot_config::load_workspace_trust_level;
use vtcode_tui::app::{
    InlineHandle, InlineHeaderBadge, InlineHeaderContext, InlineHeaderStatusBadge,
    InlineHeaderStatusTone, InlineTextStyle,
};
use vtcode_tui::core::ThemeConfigParser;

use super::welcome::SessionBootstrap;
use dirs::home_dir;

const MAX_VISIBLE_SUBAGENT_BADGES: usize = 3;
const SUBAGENT_BADGE_FALLBACK_COLOR: &str = "blue";

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
        mcp_status,
    })
}

#[derive(Debug, Default)]
struct WorkspaceHeaderSignals {
    memory_enabled: Option<bool>,
    optimization_label: Option<String>,
    custom_hint: Option<String>,
}

fn parse_workspace_header_signals(workspace: &Path) -> WorkspaceHeaderSignals {
    let config_path = workspace.join("vtcode.toml");
    let Ok(content) = fs::read_to_string(config_path) else {
        return WorkspaceHeaderSignals::default();
    };

    let Ok(root) = toml::from_str::<toml::Value>(&content) else {
        return WorkspaceHeaderSignals::default();
    };

    extract_workspace_header_signals(&root)
}

fn extract_workspace_header_signals(root: &toml::Value) -> WorkspaceHeaderSignals {
    let memory_enabled = root
        .get("memory")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("enabled"))
        .and_then(toml::Value::as_bool)
        .or_else(|| {
            root.get("agent")
                .and_then(toml::Value::as_table)
                .and_then(|agent| agent.get("persistent_memory"))
                .and_then(toml::Value::as_table)
                .and_then(|memory| memory.get("enabled"))
                .and_then(toml::Value::as_bool)
        });

    let optimization_label = root
        .get("optimization")
        .and_then(toml::Value::as_table)
        .and_then(|table| {
            let enabled = table
                .get("enabled")
                .and_then(toml::Value::as_bool)
                .unwrap_or(true);
            if !enabled {
                return None;
            }

            let strategy = table
                .get("strategy")
                .and_then(toml::Value::as_str)
                .or_else(|| {
                    table
                        .get("agent_execution")
                        .and_then(toml::Value::as_table)
                        .and_then(|agent_execution| agent_execution.get("strategy"))
                        .and_then(toml::Value::as_str)
                });

            Some(match strategy {
                Some(raw) => format!("Optimization: {}", format_strategy_label(raw)),
                None => "Optimization: On".to_string(),
            })
        });

    let custom_hint = root
        .get("header")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get("hint"))
        .and_then(toml::Value::as_str)
        .and_then(non_empty_trimmed)
        .or_else(|| {
            root.get("hint")
                .and_then(toml::Value::as_str)
                .and_then(non_empty_trimmed)
        })
        .or_else(|| first_non_empty_array_entry(root.get("tips")))
        .or_else(|| first_non_empty_array_entry(root.get("hints")))
        .or_else(|| {
            root.get("agent")
                .and_then(toml::Value::as_table)
                .and_then(|agent| agent.get("onboarding"))
                .and_then(toml::Value::as_table)
                .and_then(|onboarding| first_non_empty_array_entry(onboarding.get("usage_tips")))
        })
        .or_else(|| {
            root.get("agent")
                .and_then(toml::Value::as_table)
                .and_then(|agent| agent.get("onboarding"))
                .and_then(toml::Value::as_table)
                .and_then(|onboarding| onboarding.get("chat_placeholder"))
                .and_then(toml::Value::as_str)
                .and_then(non_empty_trimmed)
        })
        .map(|tip| truncate_header_text(&tip, 64));

    WorkspaceHeaderSignals {
        memory_enabled,
        optimization_label,
        custom_hint,
    }
}

fn first_non_empty_array_entry(value: Option<&toml::Value>) -> Option<String> {
    value.and_then(toml::Value::as_array).and_then(|items| {
        items
            .iter()
            .find_map(|item| item.as_str().and_then(non_empty_trimmed))
    })
}

fn non_empty_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn format_strategy_label(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_lowercase().replace('_', "-");
    if normalized.contains("actor-critic")
        || (normalized.contains("actor") && normalized.contains("critic"))
    {
        return "Actor-Critic".to_string();
    }
    if normalized.contains("bandit") {
        return "Bandit".to_string();
    }

    raw.split(['-', '_', ' '])
        .filter(|segment| !segment.trim().is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => {
                    let mut word = first.to_uppercase().collect::<String>();
                    word.push_str(&chars.as_str().to_ascii_lowercase());
                    word
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_header_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut out = String::new();
    for c in text.chars().take(max_chars.saturating_sub(1)) {
        out.push(c);
    }
    out.push('…');
    out
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
    vt_cfg: Option<&VTCodeConfig>,
    session_bootstrap: &SessionBootstrap,
    provider_label: String,
    model_label: String,
    context_window_size: usize,
    mode_label: String,
    reasoning_label: String,
) -> Result<InlineHeaderContext> {
    let InlineStatusDetails {
        workspace_trust,
        mcp_status,
    } = gather_inline_status_details(config, session_bootstrap).await?;
    let workspace_signals = parse_workspace_header_signals(&config.workspace);

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

    let memory_enabled = workspace_signals.memory_enabled.unwrap_or_else(|| {
        vt_cfg
            .map(VTCodeConfig::persistent_memory_enabled)
            .unwrap_or(false)
    });
    let persistent_memory = memory_enabled.then_some(InlineHeaderStatusBadge {
        text: "Memory: On".to_string(),
        tone: InlineHeaderStatusTone::Ready,
    });

    let mut chain_entries = Vec::new();
    if let Some(label) = workspace_signals.optimization_label {
        chain_entries.push(label);
    }
    if let Some(tip) = workspace_signals.custom_hint {
        chain_entries.push(format!("Tip: {}", tip));
    }

    let context = InlineHeaderContext {
        app_name: vtcode_core::config::constants::app::DISPLAY_NAME.to_string(),
        provider: provider_value,
        model: model_value,
        context_window_size: Some(context_window_size),
        version,
        search_tools: Some(build_search_tools_badge(&config.workspace)),
        persistent_memory,
        pr_review: None,
        editor_context: None,
        git: chain_entries.get(1).cloned().unwrap_or_default(),
        mode,
        reasoning,
        workspace_trust: trust_value,
        tools: chain_entries.first().cloned().unwrap_or_default(),
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
    use super::{
        build_active_subagent_badges, build_subagent_badge_style, extract_workspace_header_signals,
        format_strategy_label,
    };
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

    #[test]
    fn workspace_header_signals_extract_memory_optimization_and_tip() {
        let config = toml::from_str::<toml::Value>(
            r#"
[agent.persistent_memory]
enabled = true

[optimization]
enabled = true
strategy = "actor_critic"

[agent.onboarding]
usage_tips = ["Keep requests focused"]
"#,
        )
        .expect("valid toml");

        let signals = extract_workspace_header_signals(&config);
        assert_eq!(signals.memory_enabled, Some(true));
        assert_eq!(
            signals.optimization_label.as_deref(),
            Some("Optimization: Actor-Critic")
        );
        assert_eq!(
            signals.custom_hint.as_deref(),
            Some("Keep requests focused")
        );
    }

    #[test]
    fn format_strategy_label_normalizes_common_aliases() {
        assert_eq!(format_strategy_label("bandit"), "Bandit");
        assert_eq!(format_strategy_label("actor-critic"), "Actor-Critic");
        assert_eq!(format_strategy_label("ACTOR_CRITIC"), "Actor-Critic");
    }
}
