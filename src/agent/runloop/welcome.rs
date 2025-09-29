use std::env;
use std::env::VarError;
use std::path::Path;
use std::time::Duration;

use tracing::warn;
use update_informer::{Check, registry};
use vtcode_core::config::constants::{env as env_constants, project_doc as project_doc_constants};
use vtcode_core::config::core::AgentOnboardingConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::project_doc;
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
    pub human_in_the_loop: Option<bool>,
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

    let placeholder = {
        let trimmed = onboarding_cfg.chat_placeholder.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };

    SessionBootstrap {
        welcome_text,
        placeholder,
        prompt_addendum,
        language_summary,
        human_in_the_loop: vt_cfg.map(|cfg| cfg.security.human_in_the_loop),
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

    if onboarding_cfg.include_project_overview
        && let Some(project) = overview
    {
        let summary = project.short_for_display();
        if let Some(first_line) = summary.lines().next() {
            push_section_header(&mut lines, "Project context summary:");
            lines.push(format!("  - {}", first_line.trim()));
        }
    }

    if onboarding_cfg.include_language_summary
        && let Some(summary) = language_summary
    {
        push_section_header(&mut lines, "Detected stack:");
        lines.push(format!("  - {}", summary));
    }

    if onboarding_cfg.include_guideline_highlights
        && let Some(highlights) = guideline_highlights
        && !highlights.is_empty()
    {
        push_section_header(&mut lines, "Key guidelines:");
        for item in highlights.iter().take(2) {
            lines.push(format!("  - {}", item));
        }
    }

    push_usage_tips(&mut lines, &onboarding_cfg.usage_tips);
    push_recommended_actions(&mut lines, &onboarding_cfg.recommended_actions);

    lines.join("\n")
}

fn push_section_header(lines: &mut Vec<String>, header: &str) {
    if !lines.is_empty() && !lines.last().map(|line| line.is_empty()).unwrap_or(false) {
        lines.push(String::new());
    }
    lines.push(header.to_string());
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

fn push_usage_tips(lines: &mut Vec<String>, tips: &[String]) {
    let entries = collect_non_empty_entries(tips);
    if entries.is_empty() {
        return;
    }

    push_section_header(lines, "Usage tips:");
    for tip in entries {
        lines.push(format!("  - {}", tip));
    }
}

fn push_recommended_actions(lines: &mut Vec<String>, actions: &[String]) {
    let entries = collect_non_empty_entries(actions);
    if entries.is_empty() {
        return;
    }

    push_section_header(lines, "Suggested Next Actions:");
    for action in entries {
        lines.push(format!("  - {}", action));
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
    use std::fs;
    use tempfile::tempdir;
    use vtcode_core::config::core::PromptCachingConfig;
    use vtcode_core::config::types::{
        ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
    };

    #[test]
    fn test_prepare_session_bootstrap_builds_sections() {
        let key = env_constants::UPDATE_CHECK;
        let previous = std::env::var(key).ok();
        std::env::set_var(key, "off");

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
        vt_cfg.agent.onboarding.usage_tips = vec!["Tip one".into()];
        vt_cfg.agent.onboarding.recommended_actions = vec!["Do something".into()];
        vt_cfg.agent.onboarding.chat_placeholder = "Type your plan".into();

        let runtime_cfg = CoreAgentConfig {
            model: vtcode_core::config::constants::models::google::GEMINI_2_5_FLASH_PREVIEW
                .to_string(),
            api_key: "test".to_string(),
            provider: "gemini".to_string(),
            workspace: tmp.path().to_path_buf(),
            verbose: false,
            theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
            reasoning_effort: ReasoningEffortLevel::default(),
            ui_surface: UiSurfacePreference::default(),
            prompt_cache: PromptCachingConfig::default(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        };

        let bootstrap = prepare_session_bootstrap(&runtime_cfg, Some(&vt_cfg), None);

        let welcome = bootstrap.welcome_text.expect("welcome text");
        assert!(welcome.contains("Tip one"));
        assert!(welcome.contains("Follow workspace guidelines"));

        let prompt = bootstrap.prompt_addendum.expect("prompt addendum");
        assert!(prompt.contains("## SESSION CONTEXT"));
        assert!(prompt.contains("Suggested Next Actions"));

        assert_eq!(bootstrap.placeholder.as_deref(), Some("Type your plan"));
        assert_eq!(bootstrap.human_in_the_loop, Some(true));

        if let Some(value) = previous {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }
}
