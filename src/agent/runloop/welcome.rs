use std::path::Path;

use tracing::warn;
use unicode_width::UnicodeWidthStr;
use vtcode_core::config::constants::{project_doc as project_doc_constants, ui as ui_constants};
use vtcode_core::config::core::AgentOnboardingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::project_doc;
use vtcode_core::ui::slash::SLASH_COMMANDS;
use vtcode_core::ui::tui::InlineHeaderHighlight;
use vtcode_core::utils::utils::summarize_workspace_languages;

#[derive(Default, Clone)]
pub(crate) struct SessionBootstrap {
    pub placeholder: Option<String>,
    pub prompt_addendum: Option<String>,
    pub language_summary: Option<String>,
    pub mcp_enabled: Option<bool>,
    pub mcp_providers: Option<Vec<vtcode_core::config::mcp::McpProviderConfig>>,
    pub mcp_error: Option<String>,
    pub header_highlights: Vec<InlineHeaderHighlight>,
}

pub(crate) fn prepare_session_bootstrap(
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
    let guideline_highlights = if onboarding_cfg.include_guideline_highlights {
        let max_bytes = vt_cfg
            .map(|cfg| cfg.agent.project_doc_max_bytes)
            .unwrap_or(project_doc_constants::DEFAULT_MAX_BYTES);
        extract_guideline_highlights(
            &runtime_cfg.workspace,
            onboarding_cfg.guideline_highlight_limit,
            max_bytes,
        )
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

    SessionBootstrap {
        placeholder,
        prompt_addendum,
        language_summary,
        mcp_enabled: vt_cfg.map(|cfg| cfg.mcp.enabled),
        mcp_providers: vt_cfg.map(|cfg| cfg.mcp.providers.clone()),
        mcp_error,
        header_highlights,
    }
}

fn build_header_highlights(onboarding_cfg: &AgentOnboardingConfig) -> Vec<InlineHeaderHighlight> {
    let mut highlights = Vec::new();

    if let Some(commands) = slash_commands_highlight() {
        highlights.push(commands);
    }

    highlights
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
    ];

    if limit < commands.len() {
        commands.truncate(limit);
    }

    if commands.is_empty() {
        return None;
    }

    let indent = ui_constants::WELCOME_SLASH_COMMAND_INDENT;
    let max_width = commands
        .iter()
        .map(|(command, _)| UnicodeWidthStr::width(command.as_str()))
        .max()
        .unwrap_or(0);

    let mut lines = Vec::new();
    let intro = ui_constants::WELCOME_SLASH_COMMAND_INTRO.trim();
    if !intro.is_empty() {
        lines.push(format!("{}{}", indent, intro));
    }

    for (command, description) in commands.into_iter() {
        let command_width = UnicodeWidthStr::width(command.as_str());
        let padding = max_width.saturating_sub(command_width);
        let padding_spaces = " ".repeat(padding);
        if description.is_empty() {
            lines.push(format!("{}{}{}", indent, command, padding_spaces));
        } else {
            lines.push(format!(
                "{}{}{}  {}",
                indent, command, padding_spaces, description
            ));
        }
    }

    Some(InlineHeaderHighlight {
        title: ui_constants::WELCOME_SLASH_COMMAND_SECTION_TITLE.to_string(),
        lines,
    })
}

fn extract_guideline_highlights(
    workspace: &Path,
    limit: usize,
    max_bytes: usize,
) -> Option<Vec<String>> {
    if limit == 0 {
        return None;
    }
    match project_doc::read_project_doc(workspace, max_bytes) {
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

fn build_prompt_addendum(
    onboarding_cfg: &AgentOnboardingConfig,
    language_summary: Option<&str>,
    guideline_highlights: Option<&[String]>,
) -> Option<String> {
    let mut lines = Vec::new();
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::tempdir;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::models::Provider;
    use vtcode_core::config::types::{
        ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
    };

    #[test]
    fn test_prepare_session_bootstrap_builds_sections() {
        let tmp = tempdir().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndescription = \"Demo project\"\n",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            tmp.path().join("AGENTS.md"),
            "- Follow workspace guidelines\n- Prefer 4-space indentation\n- Run cargo fmt before commits\n",
        )
        .unwrap();
        fs::write(tmp.path().join("README.md"), "Demo workspace\n").unwrap();

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.onboarding.include_language_summary = false;
        vt_cfg.agent.onboarding.guideline_highlight_limit = 2;
        vt_cfg.agent.onboarding.include_usage_tips_in_welcome = true;
        vt_cfg
            .agent
            .onboarding
            .include_recommended_actions_in_welcome = true;
        vt_cfg.agent.onboarding.usage_tips = vec!["Tip one".into()];
        vt_cfg.agent.onboarding.recommended_actions = vec!["Do something".into()];
        vt_cfg.agent.onboarding.chat_placeholder = Some("Type your plan".into());

        let runtime_cfg = CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_2_5_FLASH_PREVIEW
                .to_string(),
            api_key: "test".to_string(),
            provider: "gemini".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: tmp.path().to_path_buf(),
            verbose: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
        };

        let bootstrap = prepare_session_bootstrap(&runtime_cfg, Some(&vt_cfg), None);

        assert_eq!(bootstrap.header_highlights.len(), 1);

        let slash_commands = &bootstrap.header_highlights[0];
        assert_eq!(
            slash_commands.title,
            ui_constants::WELCOME_SLASH_COMMAND_SECTION_TITLE
        );
        assert!(
            slash_commands
                .lines
                .iter()
                .any(|line| line.contains("/{command}"))
        );
        assert!(
            slash_commands
                .lines
                .iter()
                .any(|line| line.contains("/help"))
        );

        let prompt = bootstrap.prompt_addendum.expect("prompt addendum");
        assert!(prompt.contains("## SESSION CONTEXT"));
        assert!(prompt.contains("Suggested Next Actions"));

        assert_eq!(bootstrap.placeholder.as_deref(), Some("Type your plan"));
    }

    #[test]
    fn test_welcome_hides_optional_sections_by_default() {
        let tmp = tempdir().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndescription = \"Demo project\"\n",
        )
        .unwrap();
        fs::write(tmp.path().join("README.md"), "Demo workspace\n").unwrap();

        let runtime_cfg = CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_2_5_FLASH_PREVIEW
                .to_string(),
            api_key: "test".to_string(),
            provider: "gemini".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: tmp.path().to_path_buf(),
            verbose: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
        };

        let vt_cfg = VTCodeConfig::default();
        let bootstrap = prepare_session_bootstrap(&runtime_cfg, Some(&vt_cfg), None);

        assert_eq!(bootstrap.header_highlights.len(), 1);
        let slash_commands = &bootstrap.header_highlights[0];
        assert_eq!(
            slash_commands.title,
            ui_constants::WELCOME_SLASH_COMMAND_SECTION_TITLE
        );
        assert!(
            slash_commands
                .lines
                .iter()
                .any(|line| line.contains("/{command}"))
        );
    }

    #[test]
    fn test_prepare_session_bootstrap_hides_placeholder_when_planning_disabled() {
        let tmp = tempdir().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\ndescription = \"Demo\"\n",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src/lib.rs"), "pub fn demo() {}\n").unwrap();

        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.todo_planning_mode = false;
        vt_cfg.agent.onboarding.chat_placeholder = Some("Type your plan".into());

        let runtime_cfg = CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_2_5_FLASH_PREVIEW
                .to_string(),
            api_key: "test".to_string(),
            provider: "gemini".to_string(),
            api_key_env: Provider::Gemini.default_api_key_env().to_string(),
            workspace: tmp.path().to_path_buf(),
            verbose: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
            custom_api_keys: BTreeMap::new(),
        };

        let bootstrap = prepare_session_bootstrap(&runtime_cfg, Some(&vt_cfg), None);
        assert!(bootstrap.placeholder.is_none());
    }
}
