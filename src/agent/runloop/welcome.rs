use std::path::{Path, PathBuf};

use tracing::warn;
use vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode;
use vtcode_core::config::constants::{
    instructions as instruction_constants, project_doc as project_doc_constants, ui as ui_constants,
};
use vtcode_core::config::core::AgentOnboardingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::project_doc::{self, ProjectDocOptions};
use vtcode_core::terminal_setup::detector::{TerminalFeature, TerminalType};
use vtcode_core::ui::slash::SLASH_COMMANDS;
use vtcode_core::utils::common::summarize_workspace_languages;
use vtcode_tui::InlineHeaderHighlight;

#[derive(Default, Clone)]
pub(crate) struct SessionBootstrap {
    pub placeholder: Option<String>,
    pub prompt_addendum: Option<String>,
    pub mcp_enabled: Option<bool>,
    pub mcp_providers: Option<Vec<vtcode_core::config::mcp::McpProviderConfig>>,
    pub mcp_error: Option<String>,
    pub header_highlights: Vec<InlineHeaderHighlight>,
    pub acp_workspace_trust: Option<AgentClientProtocolZedWorkspaceTrustMode>,
}

pub(crate) async fn prepare_session_bootstrap(
    runtime_cfg: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    mcp_error: Option<String>,
) -> SessionBootstrap {
    let onboarding_cfg = vt_cfg
        .map(|cfg| cfg.agent.onboarding.clone())
        .unwrap_or_default();
    let todo_planning_enabled = vt_cfg
        .map(|cfg| cfg.agent.todo_planning_mode)
        .unwrap_or(true);

    let language_summary = summarize_workspace_languages(&runtime_cfg.workspace);
    let extra_instruction_files = vt_cfg
        .map(|cfg| cfg.agent.instruction_files.clone())
        .unwrap_or_default();
    let instruction_budget = vt_cfg
        .map(|cfg| cfg.agent.instruction_max_bytes)
        .unwrap_or(instruction_constants::DEFAULT_MAX_BYTES);
    let project_doc_budget = vt_cfg
        .map(|cfg| cfg.agent.project_doc_max_bytes)
        .unwrap_or(project_doc_constants::DEFAULT_MAX_BYTES);
    let effective_budget = instruction_budget.min(project_doc_budget);

    let guideline_highlights = if onboarding_cfg.include_guideline_highlights {
        extract_guideline_highlights(
            &runtime_cfg.workspace,
            onboarding_cfg.guideline_highlight_limit,
            effective_budget,
            &extra_instruction_files,
        )
        .await
    } else {
        None
    };

    let header_highlights = if onboarding_cfg.enabled {
        build_header_highlights(&onboarding_cfg)
    } else {
        Vec::new()
    };

    let prompt_addendum = if onboarding_cfg.enabled {
        build_prompt_addendum(
            &onboarding_cfg,
            language_summary.as_deref(),
            guideline_highlights.as_deref(),
        )
    } else {
        None
    };

    let placeholder = if onboarding_cfg.enabled && todo_planning_enabled {
        onboarding_cfg.chat_placeholder.as_ref().and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    } else {
        None
    };

    let acp_workspace_trust = vt_cfg
        .filter(|cfg| cfg.acp.zed.enabled)
        .map(|cfg| cfg.acp.zed.workspace_trust);

    SessionBootstrap {
        placeholder,
        prompt_addendum,
        mcp_enabled: vt_cfg.map(|cfg| cfg.mcp.enabled),
        mcp_providers: vt_cfg.map(|cfg| cfg.mcp.providers.clone()),
        mcp_error,
        header_highlights,
        acp_workspace_trust,
    }
}

fn build_header_highlights(onboarding_cfg: &AgentOnboardingConfig) -> Vec<InlineHeaderHighlight> {
    let mut highlights = Vec::with_capacity(3); // Typically have 3 highlight sections

    if let Some(commands) = slash_commands_highlight() {
        highlights.push(commands);
    }

    if onboarding_cfg.include_usage_tips_in_welcome
        && let Some(usage) = usage_tips_highlight(&onboarding_cfg.usage_tips)
    {
        highlights.push(usage);
    }

    if onboarding_cfg.include_recommended_actions_in_welcome
        && let Some(actions) = recommended_actions_highlight(&onboarding_cfg.recommended_actions)
    {
        highlights.push(actions);
    }

    highlights.push(terminal_info_highlight());

    highlights
}

fn terminal_info_highlight() -> InlineHeaderHighlight {
    let (title, lines) = match TerminalType::detect() {
        Ok(term) if term != TerminalType::Unknown => {
            let mut lines = Vec::new();
            lines.push(format!("Terminal: {}", term.name()));

            let features = [
                TerminalFeature::Multiline,
                TerminalFeature::CopyPaste,
                TerminalFeature::ShellIntegration,
                TerminalFeature::ThemeSync,
            ];

            let supported: Vec<&str> = features
                .iter()
                .filter(|f| term.supports_feature(**f))
                .map(|f| match f {
                    TerminalFeature::Multiline => "Multiline",
                    TerminalFeature::CopyPaste => "Copy/Paste",
                    TerminalFeature::ShellIntegration => "Shell Integration",
                    TerminalFeature::ThemeSync => "Theme Sync",
                    TerminalFeature::Notifications => "Notifications",
                })
                .collect();

            if !supported.is_empty() {
                lines.push(format!("Capabilities: {}", supported.join(", ")));
            }

            if term.requires_manual_setup() {
                lines.push("Status: Setup Required (Run /terminal-setup)".to_string());
            } else if term == TerminalType::Ghostty || term == TerminalType::Kitty {
                // For fully supported terminals, show they are ready
                lines.push("Status: Optimized for VT Code".to_string());
            }

            ("Terminal Environment".to_string(), lines)
        }
        _ => (
            "Terminal Environment".to_string(),
            vec!["Detected: Generic / Unknown".to_string()],
        ),
    };

    InlineHeaderHighlight { title, lines }
}

fn usage_tips_highlight(tips: &[String]) -> Option<InlineHeaderHighlight> {
    let entries = collect_non_empty_entries(tips);
    if entries.is_empty() {
        return None;
    }

    let lines = entries
        .into_iter()
        .map(|tip| format!("- {}", tip))
        .collect();

    Some(InlineHeaderHighlight {
        title: "Usage Tips".to_string(),
        lines,
    })
}

fn recommended_actions_highlight(actions: &[String]) -> Option<InlineHeaderHighlight> {
    let entries = collect_non_empty_entries(actions);
    if entries.is_empty() {
        return None;
    }

    let lines = entries
        .into_iter()
        .map(|action| format!("- {}", action))
        .collect();

    Some(InlineHeaderHighlight {
        title: "Suggested Next Actions".to_string(),
        lines,
    })
}

fn slash_commands_highlight() -> Option<InlineHeaderHighlight> {
    let limit = ui_constants::WELCOME_SLASH_COMMAND_LIMIT;
    if limit == 0 {
        return None;
    }

    let prefix = ui_constants::WELCOME_SLASH_COMMAND_PREFIX;
    let mut commands: Vec<(String, String)> = vec![
        (format!("{}{{command}}", prefix), String::new()),
        (
            format!("{}help", prefix),
            SLASH_COMMANDS
                .iter()
                .find(|info| info.name == "help")
                .map(|info| info.description.trim().to_string())
                .unwrap_or_default(),
        ),
        ("Enter".to_string(), "Submit message".to_string()),
        ("Escape".to_string(), "Cancel input".to_string()),
    ];

    if limit < commands.len() {
        commands.truncate(limit);
    }

    if commands.is_empty() {
        return None;
    }

    let indent = ui_constants::WELCOME_SLASH_COMMAND_INDENT;
    let intro = ui_constants::WELCOME_SLASH_COMMAND_INTRO.trim();

    let segments: Vec<String> = commands
        .into_iter()
        .map(|(command, description)| {
            if description.is_empty() {
                command
            } else {
                format!("{command} {description}")
            }
        })
        .collect();

    if segments.is_empty() {
        return None;
    }

    let mut lines = Vec::with_capacity(10); // Estimate 10 lines for usage tips

    if !intro.is_empty() {
        lines.push(format!("{}{}", indent, intro));
    }

    for segment in segments {
        lines.push(format!("{}- {}", indent, segment));
    }

    Some(InlineHeaderHighlight {
        title: String::new(),
        lines,
    })
}

async fn extract_guideline_highlights(
    workspace: &Path,
    limit: usize,
    max_bytes: usize,
    extra_instruction_files: &[String],
) -> Option<Vec<String>> {
    if limit == 0 || max_bytes == 0 {
        return None;
    }

    let home_dir = determine_home_dir();
    match project_doc::read_project_doc_with_options(&ProjectDocOptions {
        current_dir: workspace,
        project_root: workspace,
        home_dir: home_dir.as_deref(),
        extra_instruction_files,
        max_bytes,
    })
    .await
    {
        Ok(Some(bundle)) => {
            let highlights = bundle.highlights(limit);
            if highlights.is_empty() {
                None
            } else {
                Some(highlights)
            }
        }
        Ok(None) => None,
        Err(err) => {
            warn!("failed to load project documentation for highlights: {err:#}");
            None
        }
    }
}

fn determine_home_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }

    if let Some(profile) = std::env::var_os("USERPROFILE")
        && !profile.is_empty()
    {
        return Some(PathBuf::from(profile));
    }

    None
}

fn build_prompt_addendum(
    onboarding_cfg: &AgentOnboardingConfig,
    language_summary: Option<&str>,
    guideline_highlights: Option<&[String]>,
) -> Option<String> {
    let mut lines = Vec::with_capacity(15); // Estimate 15 lines for prompt addendum
    lines.push("## SESSION CONTEXT".to_string());

    if onboarding_cfg.include_language_summary
        && let Some(summary) = language_summary
    {
        lines.push("### Detected Languages".to_string());
        lines.push(format!("- {}", summary));
    }

    if onboarding_cfg.include_guideline_highlights
        && let Some(highlights) = guideline_highlights
        && !highlights.is_empty()
    {
        lines.push("### Key Guidelines".to_string());
        for item in highlights.iter().take(2) {
            lines.push(format!("- {}", item));
        }
    }

    push_prompt_usage_tips(&mut lines, &onboarding_cfg.usage_tips);
    push_prompt_recommended_actions(&mut lines, &onboarding_cfg.recommended_actions);

    let content = lines.join("\n");
    if content.trim() == "## SESSION CONTEXT" {
        None
    } else {
        Some(content)
    }
}

fn push_prompt_usage_tips(lines: &mut Vec<String>, tips: &[String]) {
    let entries = collect_non_empty_entries(tips);
    if entries.is_empty() {
        return;
    }

    lines.push("### Usage Tips".to_string());
    for tip in entries {
        lines.push(format!("- {}", tip));
    }
}

fn push_prompt_recommended_actions(lines: &mut Vec<String>, actions: &[String]) {
    let entries = collect_non_empty_entries(actions);
    if entries.is_empty() {
        return;
    }

    lines.push("### Suggested Next Actions".to_string());
    for action in entries {
        lines.push(format!("- {}", action));
    }
}

fn collect_non_empty_entries(items: &[String]) -> Vec<&str> {
    items
        .iter()
        .map(|item| item.trim())
        .filter(|item| !item.is_empty())
        .collect()
}
