use std::env;
use std::env::VarError;
use std::path::Path;
use std::time::Duration;

use tracing::warn;
use update_informer::{Check, registry};
use vtcode_core::config::constants::{
    env as env_constants, project_doc as project_doc_constants, ui as ui_constants,
};
use vtcode_core::config::core::AgentOnboardingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::project_doc;
use vtcode_core::ui::slash::SLASH_COMMANDS;
use vtcode_core::ui::styled::Styles;
use vtcode_core::ui::theme;
use vtcode_core::utils::utils::{
    ProjectOverview, build_project_overview, summarize_workspace_languages,
};

const PACKAGE_NAME: &str = env!("CARGO_PKG_NAME");
const PACKAGE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default, Clone)]
pub(crate) struct SessionBootstrap {
    pub welcome_text: Option<String>,
    pub placeholder: Option<String>,
    pub prompt_addendum: Option<String>,
    pub language_summary: Option<String>,
    pub mcp_enabled: Option<bool>,
    pub mcp_providers: Option<Vec<vtcode_core::config::mcp::McpProviderConfig>>,
    pub mcp_error: Option<String>,
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

    let project_overview = build_project_overview(&runtime_cfg.workspace);
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

    let update_notice = if onboarding_cfg.enabled {
        compute_update_notice()
    } else {
        None
    };

    let welcome_text = if onboarding_cfg.enabled {
        Some(render_welcome_text(
            &onboarding_cfg,
            project_overview.as_ref(),
            language_summary.as_deref(),
            guideline_highlights.as_deref(),
            update_notice.as_deref(),
        ))
    } else {
        None
    };

    let prompt_addendum = if onboarding_cfg.enabled {
        build_prompt_addendum(
            &onboarding_cfg,
            project_overview.as_ref(),
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
        welcome_text,
        placeholder,
        prompt_addendum,
        language_summary,
        mcp_enabled: vt_cfg.map(|cfg| cfg.mcp.enabled),
        mcp_providers: vt_cfg.map(|cfg| cfg.mcp.providers.clone()),
        mcp_error,
    }
}

fn render_welcome_text(
    onboarding_cfg: &AgentOnboardingConfig,
    overview: Option<&ProjectOverview>,
    language_summary: Option<&str>,
    guideline_highlights: Option<&[String]>,
    update_notice: Option<&str>,
) -> String {
    let mut lines = Vec::new();
    // Skip intro_text and use the fancy banner instead

    if let Some(notice) = update_notice {
        lines.push(notice.to_string());
    }

    let mut sections: Vec<SectionBlock> = Vec::new();

    if onboarding_cfg.include_project_overview
        && let Some(project) = overview
    {
        let summary = project.short_for_display();
        let mut details: Vec<String> = summary
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| line.to_string())
            .collect();

        if let Some(first) = details.first_mut() {
            *first = format!("**{}**", first);
        }

        if !details.is_empty() {
            let mut section = Vec::with_capacity(details.len() + 1);
            section.push("**Project Overview**".to_string());
            section.extend(details);
            sections.push(SectionBlock::new(section, SectionSpacing::Normal));
        }
    }

    if onboarding_cfg.include_language_summary
        && let Some(summary) = language_summary
    {
        let trimmed = summary.trim();
        if !trimmed.is_empty() {
            add_section(
                &mut sections,
                "Detected Languages",
                vec![trimmed.to_string()],
                SectionSpacing::Normal,
            );
        }
    }

    if onboarding_cfg.include_guideline_highlights
        && let Some(highlights) = guideline_highlights
        && !highlights.is_empty()
    {
        let details: Vec<String> = highlights
            .iter()
            .take(2)
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .map(|item| format!("- {}", item))
            .collect();
        add_section(
            &mut sections,
            style_section_title("Key Guidelines"),
            details,
            SectionSpacing::Flush,
        );
    }

    if onboarding_cfg.include_usage_tips_in_welcome {
        add_list_section(
            &mut sections,
            "Usage Tips",
            &onboarding_cfg.usage_tips,
            SectionSpacing::Compact,
        );
    }

    if onboarding_cfg.include_recommended_actions_in_welcome {
        add_list_section(
            &mut sections,
            "Suggested Next Actions",
            &onboarding_cfg.recommended_actions,
            SectionSpacing::Compact,
        );
    }

    add_keyboard_shortcut_section(&mut sections);
    add_slash_command_section(&mut sections);

    let mut previous_spacing: Option<SectionSpacing> = None;
    for section in sections {
        if !lines.is_empty() && section.spacing.needs_blank_line(previous_spacing) {
            lines.push(String::new());
        }
        lines.extend(section.lines);
        previous_spacing = Some(section.spacing);
    }

    lines.join("\n")
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
    overview: Option<&ProjectOverview>,
    language_summary: Option<&str>,
    guideline_highlights: Option<&[String]>,
) -> Option<String> {
    let mut lines = Vec::new();
    lines.push("## SESSION CONTEXT".to_string());

    if onboarding_cfg.include_project_overview
        && let Some(project) = overview
    {
        lines.push("### Project Overview".to_string());
        let block = project.as_prompt_block();
        let trimmed = block.trim();
        if !trimmed.is_empty() {
            lines.push(trimmed.to_string());
        }
    }

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

fn style_section_title(title: &str) -> String {
    let primary = theme::active_styles().primary.bold();
    let prefix = Styles::render(&primary);
    let reset = Styles::render_reset();
    format!("{prefix}{title}{reset}")
}

fn add_section(
    sections: &mut Vec<SectionBlock>,
    title: impl Into<String>,
    body: Vec<String>,
    spacing: SectionSpacing,
) {
    if body.is_empty() {
        return;
    }

    let title = title.into();
    let mut section = Vec::with_capacity(body.len() + 1);
    section.push(title.to_string());
    section.extend(body);
    sections.push(SectionBlock::new(section, spacing));
}

fn add_list_section(
    sections: &mut Vec<SectionBlock>,
    title: &str,
    items: &[String],
    spacing: SectionSpacing,
) {
    let entries = collect_non_empty_entries(items);
    if entries.is_empty() {
        return;
    }

    let body = entries
        .into_iter()
        .map(|entry| format!("- {}", entry))
        .collect();

    add_section(sections, title, body, spacing);
}

fn add_keyboard_shortcut_section(sections: &mut Vec<SectionBlock>) {
    let hint = ui_constants::HEADER_SHORTCUT_HINT.trim();
    if hint.is_empty() {
        return;
    }

    let trimmed = hint
        .strip_prefix(ui_constants::WELCOME_SHORTCUT_HINT_PREFIX)
        .map(str::trim)
        .unwrap_or(hint);

    if trimmed.is_empty() {
        return;
    }

    let entries: Vec<String> = trimmed
        .split(ui_constants::WELCOME_SHORTCUT_SEPARATOR)
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| format!("{}{}", ui_constants::WELCOME_SHORTCUT_INDENT, part))
        .collect();

    if entries.is_empty() {
        return;
    }

    add_section(
        sections,
        style_section_title(ui_constants::WELCOME_SHORTCUT_SECTION_TITLE),
        entries,
        SectionSpacing::Flush,
    );
}

fn add_slash_command_section(sections: &mut Vec<SectionBlock>) {
    let limit = ui_constants::WELCOME_SLASH_COMMAND_LIMIT;
    if limit == 0 {
        return;
    }

    let command_iter = SLASH_COMMANDS.iter().take(limit);

    let entries: Vec<String> = command_iter
        .map(|info| {
            let command = format!(
                "{}{}",
                ui_constants::WELCOME_SLASH_COMMAND_PREFIX,
                info.name
            );
            format!(
                "{}{} {}",
                ui_constants::WELCOME_SLASH_COMMAND_INDENT,
                command,
                info.description
            )
        })
        .collect();

    if entries.is_empty() {
        return;
    }

    let mut body = Vec::with_capacity(entries.len() + 1);
    let intro = ui_constants::WELCOME_SLASH_COMMAND_INTRO.trim();
    if !intro.is_empty() {
        body.push(intro.to_string());
    }
    body.extend(entries);

    add_section(
        sections,
        ui_constants::WELCOME_SLASH_COMMAND_SECTION_TITLE,
        body,
        SectionSpacing::Compact,
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SectionSpacing {
    Normal,
    Compact,
    Flush,
}

struct SectionBlock {
    lines: Vec<String>,
    spacing: SectionSpacing,
}

impl SectionBlock {
    fn new(lines: Vec<String>, spacing: SectionSpacing) -> Self {
        Self { lines, spacing }
    }
}

impl SectionSpacing {
    fn needs_blank_line(self, previous: Option<Self>) -> bool {
        match self {
            SectionSpacing::Normal => true,
            SectionSpacing::Compact => previous != Some(SectionSpacing::Compact),
            SectionSpacing::Flush => false,
        }
    }
}

fn compute_update_notice() -> Option<String> {
    if !should_check_for_updates() {
        return None;
    }

    let informer = update_informer::new(registry::Crates, PACKAGE_NAME, PACKAGE_VERSION)
        .interval(Duration::ZERO);

    match informer.check_version() {
        Ok(Some(new_version)) => {
            let install_command = format!("cargo install {} --locked --force", PACKAGE_NAME);
            Some(format!(
                "Update available: {} {} → {}. Upgrade with `{}`.",
                PACKAGE_NAME, PACKAGE_VERSION, new_version, install_command
            ))
        }
        Ok(None) => None,
        Err(err) => {
            warn!(%err, "update check failed");
            None
        }
    }
}

fn should_check_for_updates() -> bool {
    match env::var(env_constants::UPDATE_CHECK) {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            !matches!(normalized.as_str(), "0" | "false" | "off" | "no")
        }
        Err(VarError::NotPresent) => true,
        Err(VarError::NotUnicode(_)) => {
            warn!("update check env var contains invalid unicode");
            false
        }
    }
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
    use vtcode_core::ui::{styled::Styles, theme};

    fn strip_ansi_codes(input: &str) -> String {
        let mut output = String::with_capacity(input.len());
        let mut chars = input.chars();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' {
                if let Some(next) = chars.next() {
                    if next == '[' {
                        while let Some(terminator) = chars.next() {
                            if ('@'..='~').contains(&terminator) {
                                break;
                            }
                        }
                        continue;
                    }
                }
            }
            output.push(ch);
        }
        output
    }

    #[test]
    fn test_prepare_session_bootstrap_builds_sections() {
        let key = env_constants::UPDATE_CHECK;
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, "off");
        }

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

        let welcome = bootstrap.welcome_text.expect("welcome text");
        let plain = strip_ansi_codes(&welcome);
        let styled_title = theme::active_styles().primary.bold();
        let prefix = Styles::render(&styled_title);
        let reset = Styles::render_reset();
        let styled_guidelines = format!("{prefix}Key Guidelines{reset}");
        let styled_shortcuts = format!(
            "{prefix}{}{reset}",
            ui_constants::WELCOME_SHORTCUT_SECTION_TITLE
        );

        assert!(welcome.contains("**Project Overview**"));
        assert!(welcome.contains("**Project:"));
        assert!(welcome.contains("Tip one"));
        assert!(welcome.contains("Follow workspace guidelines"));
        assert!(welcome.contains(&styled_guidelines));
        assert!(welcome.contains(&styled_shortcuts));
        assert!(plain.contains("Keyboard Shortcuts"));
        assert!(plain.contains("Key Guidelines"));
        assert!(welcome.contains("Slash Commands"));
        assert!(welcome.contains(ui_constants::WELCOME_SLASH_COMMAND_INTRO));
        assert!(welcome.contains(&format!(
            "{}{}init",
            ui_constants::WELCOME_SLASH_COMMAND_INDENT,
            ui_constants::WELCOME_SLASH_COMMAND_PREFIX
        )));
        assert!(welcome.contains(&format!(
            "{}Ctrl+Enter",
            ui_constants::WELCOME_SHORTCUT_INDENT
        )));
        assert!(!plain.contains("\n\nKey Guidelines"));
        assert!(!plain.contains("\n\nKeyboard Shortcuts"));

        let prompt = bootstrap.prompt_addendum.expect("prompt addendum");
        assert!(prompt.contains("## SESSION CONTEXT"));
        assert!(prompt.contains("Suggested Next Actions"));

        assert_eq!(bootstrap.placeholder.as_deref(), Some("Type your plan"));
        if let Some(value) = previous {
            unsafe {
                std::env::set_var(key, value);
            }
        } else {
            unsafe {
                std::env::remove_var(key);
            }
        }
    }

    #[test]
    fn test_welcome_hides_optional_sections_by_default() {
        let key = env_constants::UPDATE_CHECK;
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, "off");
        }

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
        let welcome = bootstrap.welcome_text.expect("welcome text");
        let plain = strip_ansi_codes(&welcome);
        let styled_title = theme::active_styles().primary.bold();
        let prefix = Styles::render(&styled_title);
        let reset = Styles::render_reset();
        let styled_shortcuts = format!(
            "{prefix}{}{reset}",
            ui_constants::WELCOME_SHORTCUT_SECTION_TITLE
        );

        assert!(!welcome.contains("Usage Tips"));
        assert!(!welcome.contains("Suggested Next Actions"));
        assert!(welcome.contains(&styled_shortcuts));
        assert!(plain.contains("Keyboard Shortcuts"));
        assert!(!plain.contains("\n\nKeyboard Shortcuts"));
        assert!(welcome.contains("Slash Commands"));
        assert!(welcome.contains(ui_constants::WELCOME_SLASH_COMMAND_INTRO));
        assert!(welcome.contains(&format!(
            "{}{}command",
            ui_constants::WELCOME_SLASH_COMMAND_INDENT,
            ui_constants::WELCOME_SLASH_COMMAND_PREFIX
        )));
        assert!(welcome.contains(&format!("{}Esc", ui_constants::WELCOME_SHORTCUT_INDENT)));

        if let Some(value) = previous {
            unsafe {
                std::env::set_var(key, value);
            }
        } else {
            unsafe {
                std::env::remove_var(key);
            }
        }
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
