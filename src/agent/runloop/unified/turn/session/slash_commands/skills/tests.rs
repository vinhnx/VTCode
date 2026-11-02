use std::sync::Arc;

use clap::Parser;
use hashbrown::HashMap;
use tempfile::TempDir;
use tokio::sync::RwLock;
use vtcode_core::cli::args::Cli;
use vtcode_core::config::constants::tools;
use vtcode_core::config::types::ModelSelectionSource;
use vtcode_core::config::{ToolProfile, VTCodeConfig};
use vtcode_core::core::agent::config::{RuntimeModelSelection, build_runtime_agent_config};
use vtcode_core::skills::{Skill, SkillManifest};
use vtcode_core::tools::ToolRegistry;

use super::outcome::{configured_tool_profile, skill_invocation_status_message, skill_runtime_for_session};

#[test]
fn skill_runtime_profile_comes_from_effective_config() {
    let mut config = VTCodeConfig::default();
    config.tools.profile = ToolProfile::AdvancedVtCode;

    assert_eq!(configured_tool_profile(Some(&config)), ToolProfile::AdvancedVtCode);
    assert_eq!(configured_tool_profile(None), ToolProfile::VtCode);
}

#[tokio::test]
async fn slash_skill_activation_retains_advanced_catalogue() {
    let temp = TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp.path().to_path_buf()).await;
    let tool_catalog = registry.tool_catalog_state();
    let active_tools = Arc::new(RwLock::new(Vec::new()));
    let active_skills = Arc::new(RwLock::new(HashMap::new()));
    let mut vt_cfg = VTCodeConfig::default();
    vt_cfg.tools.profile = ToolProfile::AdvancedVtCode;
    let config = build_runtime_agent_config(
        &Cli::parse_from(["vtcode"]),
        &vt_cfg,
        temp.path().to_path_buf(),
        RuntimeModelSelection {
            model: "gpt-5".to_string(),
            provider: "openai".to_string(),
            api_key_env: "OPENAI_API_KEY".to_string(),
            model_source: ModelSelectionSource::WorkspaceConfig,
        },
        "test-key".to_string(),
        vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
    );
    let runtime = skill_runtime_for_session(&registry, &active_tools, &tool_catalog, &config, Some(&vt_cfg));
    let skill = Skill::new(
        SkillManifest {
            name: "slash-profile-skill".to_string(),
            description: "Slash profile regression skill".to_string(),
            ..SkillManifest::default()
        },
        temp.path().join("slash-profile-skill"),
        "Use the regression skill.".to_string(),
    )
    .expect("skill");

    runtime
        .activate_skill(&active_skills, skill)
        .await
        .expect("slash skill activation");

    let tool_names = active_tools
        .read()
        .await
        .iter()
        .map(|tool| tool.function_name().to_string())
        .collect::<Vec<_>>();
    assert!(tool_names.contains(&tools::CODE_SEARCH.to_string()));
    assert!(tool_names.contains(&"slash-profile-skill".to_string()));
}

#[test]
fn command_skills_use_slash_alias_in_status() {
    assert_eq!(skill_invocation_status_message("cmd-review"), "Running /review...");
}

#[test]
fn non_command_skills_use_skill_name_in_status() {
    assert_eq!(skill_invocation_status_message("rust-skills"), "Running rust-skills...");
}
