use super::*;

#[tokio::test]
async fn test_temporal_context_inclusion() {
    let mut config = VTCodeConfig::default();
    config.agent.include_temporal_context = true;
    config.prompt_cache.cache_friendly_prompt_shaping = false;
    config.agent.temporal_context_use_utc = false; // Local time

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(result.contains("Date:"), "Should include date context when enabled");
    assert!(!result.contains("Current date and time"), "System prompt must stay date-only for cache stability");
    let env_pos = result.find("## Environment");
    let temporal_pos = result.find("Date:");
    if let (Some(t), Some(e)) = (temporal_pos, env_pos) {
        assert!(t > e, "Temporal context should appear inside the environment section");
    }
}
#[tokio::test]
async fn test_temporal_context_utc_format() {
    let mut config = VTCodeConfig::default();
    config.agent.include_temporal_context = true;
    config.prompt_cache.cache_friendly_prompt_shaping = false;
    config.agent.temporal_context_use_utc = true; // UTC format

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(result.contains("UTC"), "Should indicate UTC when temporal_context_use_utc is true");
    assert!(result.contains("Date"), "Should carry the cache-friendly date label");
}
#[tokio::test]
async fn test_temporal_context_disabled() {
    let mut config = VTCodeConfig::default();
    config.agent.include_temporal_context = false;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(!result.contains("Date:"), "Should not include temporal context when disabled");
}
#[tokio::test]
async fn test_cache_friendly_temporal_context_stays_out_of_base_prompt() {
    let mut config = VTCodeConfig::default();
    config.agent.include_temporal_context = true;
    config.prompt_cache.cache_friendly_prompt_shaping = true;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(result.contains("Date:"), "Session-start date should be frozen in the cached prompt");
}
#[tokio::test]
async fn test_configuration_awareness_stays_behavior_focused() {
    let mut config = VTCodeConfig::default();
    config.security.human_in_the_loop = true;
    config.chat.ask_questions.enabled = false;
    config.mcp.enabled = true;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(result.contains("## Environment"));
    assert!(!result.contains("approval may gate"));
    assert!(result.contains("request_user_input"));
    assert!(result.contains("Sources: prefer MCP"));
    assert!(!result.contains("PTY functionality"));
    assert!(!result.contains("Loop guards"));
    assert!(!result.contains(".vtcode/context/tool_outputs/"));
    assert!(!result.contains("IDE context:"));
}
#[tokio::test]
async fn test_configuration_awareness_mentions_reduced_approval_when_disabled() {
    let mut config = VTCodeConfig::default();
    config.security.human_in_the_loop = false;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(!result.contains("approval reduced by config"));
}
#[tokio::test]
async fn test_default_environment_omits_default_interaction_guidance() {
    let config = VTCodeConfig::default();

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(!result.contains("Interaction:"), "Default-on interaction guidance should stay out of the prompt");
}
#[tokio::test]
async fn test_working_directory_inclusion() {
    let mut config = VTCodeConfig::default();
    config.agent.include_working_directory = true;

    let mut ctx = PromptContext::default();
    ctx.set_current_directory(PathBuf::from("/tmp/test"));

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

    assert!(result.contains("Working directory"), "Should include working directory label");
    assert!(result.contains("/tmp/test"), "Should show actual directory path");
    let wd_pos = result.find("Working directory");
    let env_pos = result.find("## Environment");
    if let (Some(w), Some(e)) = (wd_pos, env_pos) {
        assert!(w > e, "Working directory should appear inside the environment section");
    }
}
#[tokio::test]
async fn test_working_directory_disabled() {
    let mut config = VTCodeConfig::default();
    config.agent.include_working_directory = false;

    let mut ctx = PromptContext::default();
    ctx.set_current_directory(PathBuf::from("/tmp/test"));

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

    assert!(!result.contains("Working directory"), "Should not include working directory when disabled");
}
#[tokio::test]
async fn test_backward_compatibility() {
    let config = VTCodeConfig::default();

    // Old signature: no prompt context
    let result = compose_system_instruction_text(
        &PathBuf::from("."),
        Some(&config),
        None, // No context - backward compatible
    )
    .await;

    // Should still work without new features
    assert!(result.len() > 600, "Should generate substantial prompt");
    assert!(result.contains("VT Code"), "Should contain base prompt content");
    // Should not have dynamic guidelines without context
    assert!(!result.contains("## Active Tools"), "Should not have tool guidelines without prompt context");
}
#[tokio::test]
async fn test_all_enhancements_combined() {
    use crate::skills::model::{SkillMetadata, SkillScope};

    let mut config = VTCodeConfig::default();
    config.agent.include_temporal_context = true;
    config.agent.include_working_directory = true;
    config.prompt_cache.cache_friendly_prompt_shaping = false;

    let mut ctx = PromptContext::default();
    ctx.add_tool(tools::APPLY_PATCH.to_string());
    ctx.add_tool(tools::EXEC_COMMAND.to_string());
    ctx.infer_capability_level();
    ctx.set_current_directory(PathBuf::from("/workspace"));
    ctx.add_skill_metadata(SkillMetadata {
        name: "rust-skills".to_string(),
        description: "Rust coding guidance".to_string(),
        short_description: None,
        path: PathBuf::from("/tmp/rust-skills/SKILL.md"),
        scope: SkillScope::System,
        manifest: None,
    });

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

    // Verify all enhancements present
    assert!(result.contains("## Active Tools"), "Should have dynamic guidelines");
    assert!(result.contains("## Skills"), "Should have lean skills routing");
    assert!(result.contains("## Environment"), "Should have environment addenda");
    assert!(result.contains("Date:"), "Should have date context");
    assert!(result.contains("Working directory"), "Should have working directory");
    assert!(result.contains("/workspace"), "Should show workspace path");

    // Verify specific guideline for this tool set
    assert!(result.contains("after inspection"), "Should have read-before-edit guideline");
    assert_no_removed_model_facing_tool_names(&result);
}
#[tokio::test]
async fn test_prompt_layers_render_in_stable_order() {
    use crate::skills::model::{SkillMetadata, SkillScope};

    let mut config = VTCodeConfig::default();
    config.agent.include_temporal_context = true;
    config.agent.include_working_directory = true;

    let mut ctx = PromptContext::default();
    ctx.add_tool(tools::EXEC_COMMAND.to_string());
    ctx.add_tool(tools::APPLY_PATCH.to_string());
    ctx.add_skill_metadata(SkillMetadata {
        name: "skill-creator".to_string(),
        description: "Create skills".to_string(),
        short_description: None,
        path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
        scope: SkillScope::System,
        manifest: None,
    });
    ctx.add_language("Rust".to_string());
    ctx.set_current_directory(PathBuf::from("/workspace"));

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

    let mode_pos = result.find("## Operating Profile").expect("operating profile section");
    let tools_pos = result.find("## Active Tools").expect("tools section");
    let skills_pos = result.find("## Skills").expect("skills section");
    let env_pos = result.find("## Environment").expect("environment section");

    assert!(mode_pos < skills_pos, "operating profile should precede skills");
    assert!(skills_pos < tools_pos, "stable skills should precede dynamic tools");
    assert!(tools_pos < env_pos, "tools should precede environment");
}
#[tokio::test]
async fn test_skills_section_stays_lean_and_routing_focused() {
    use crate::skills::model::SkillScope;
    use crate::skills::types::SkillManifest;

    let config = VTCodeConfig::default();
    let mut ctx = PromptContext::default();
    ctx.available_skill_metadata.push(crate::skills::model::SkillMetadata {
        name: "skill-creator".to_string(),
        description: "Create or update skills".to_string(),
        short_description: None,
        path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
        scope: SkillScope::System,
        manifest: Some(
            SkillManifest {
                when_to_use: Some("Use when creating or updating a skill.".to_string()),
                when_not_to_use: Some("Avoid for unrelated implementation work.".to_string()),
                ..SkillManifest::default()
            }
            .into(),
        ),
    });

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

    assert!(result.contains("## Skills"));
    assert!(result.contains("skill-creator: Create or update skills"));
    assert!(result.contains("Use a skill only when the user names it"));
    assert!(!result.contains("Discovery: Available skills are listed"));
    assert!(!result.contains("/tmp/skill-creator/SKILL.md"));
    assert!(!result.contains("use: Use when creating or updating a skill."));
    assert!(!result.contains("avoid: Avoid for unrelated implementation work."));
}
#[test]
fn test_static_prompts_have_no_placeholders() {
    let _minimal = generate_minimal_instruction();
    let _lightweight = generate_lightweight_instruction();
    let _specialized = generate_specialized_instruction();

    let minimal_text = minimal_instruction_text();
    let lightweight_text = lightweight_instruction_text();
    let specialized_text = specialized_instruction_text();

    assert!(!minimal_text.contains("__UNIFIED_TOOL_GUIDANCE__"), "Minimal prompt has uninterpolated placeholder");
    assert!(
        !lightweight_text.contains("__UNIFIED_TOOL_GUIDANCE__"),
        "Lightweight prompt has uninterpolated placeholder"
    );
    assert!(
        !specialized_text.contains("__UNIFIED_TOOL_GUIDANCE__"),
        "Specialized prompt has uninterpolated placeholder"
    );
    assert!(
        !default_system_prompt().contains("__UNIFIED_TOOL_GUIDANCE__"),
        "Default prompt has uninterpolated placeholder"
    );
}
#[test]
fn test_agent_identity_labels() {
    // Test known agent names
    assert_eq!(agent_identity_label("build"), "VT Code (Build mode)");
    assert_eq!(agent_identity_label("auto"), "VT Code (Auto mode)");
    assert_eq!(agent_identity_label("duck"), "VT Code (Duck mode)");
    assert_eq!(agent_identity_label("plan"), "VT Code (Plan mode)");
    assert_eq!(agent_identity_label("explorer"), "VT Code (Explorer mode)");
    assert_eq!(agent_identity_label("worker"), "VT Code (Worker mode)");

    // Test unknown agent names
    assert_eq!(agent_identity_label("unknown"), "VT Code (unknown)");
    assert_eq!(agent_identity_label("custom"), "VT Code (custom)");
}
#[test]
fn test_apply_agent_identity() {
    let prompt = format!("{PROMPT_TITLE}\n\n{PROMPT_INTRO}\n\n## Contract\n- Rule 1");
    let result = apply_agent_identity(&prompt, "VT Code (Build mode)");
    assert_eq!(
        result,
        "# VT Code (Build mode)\n\nYou are VT Code (Build mode), a coding agent working in the user's repository and terminal.\n\n## Contract\n- Rule 1"
    );
    // The identity name appears once in each substituted line, so a label
    // that itself contains `VT Code` is not substituted twice.
    assert_eq!(PROMPT_INTRO.matches(PROMPT_IDENTITY_NAME).count(), 1);
    assert_eq!(PROMPT_TITLE.matches(PROMPT_IDENTITY_NAME).count(), 1);
}
#[tokio::test]
async fn test_system_prompt_includes_agent_identity() {
    let mut config = VTCodeConfig {
        default_primary_agent: "build".to_string(),
        ..Default::default()
    };
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(result.starts_with("# VT Code (Build mode)"), "Should start with agent identity: {}", &result[..50]);
    assert!(
        result.contains("You are VT Code (Build mode), a coding agent working in the user's repository and terminal."),
        "Should include agent identity in intro"
    );
}
#[tokio::test]
async fn test_system_prompt_auto_agent_identity() {
    let mut config = VTCodeConfig {
        default_primary_agent: "auto".to_string(),
        ..Default::default()
    };
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(result.starts_with("# VT Code (Auto mode)"), "Should start with auto agent identity");
}
#[tokio::test]
async fn test_system_prompt_duck_agent_identity() {
    let mut config = VTCodeConfig {
        default_primary_agent: "duck".to_string(),
        ..Default::default()
    };
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(result.starts_with("# VT Code (Duck mode)"), "Should start with duck agent identity");
}
#[tokio::test]
async fn test_coordinator_role_presence_and_budget_in_default_and_minimal() {
    let project_root = tempfile::tempdir().unwrap();
    for mode in [SystemPromptMode::Default, SystemPromptMode::Minimal] {
        let mut config = VTCodeConfig {
            default_primary_agent: "coordinator".to_string(),
            ..Default::default()
        };
        config.agent.system_prompt_mode = mode;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        let result = compose_system_instruction_text(project_root.path(), Some(&config), None).await;
        assert!(result.starts_with("# VT Code (Coordinator mode)"));
        assert!(result.contains("Delegate all shell execution, file changes, and verification"));
        assert!(result.contains("while the matrix is idle"));
        assert!(!result.contains("Prefer skills and tool search"));
        assert_eq!(result.matches("## Coordinator Role").count(), 1);
        // Raised for the partial-commit and conflict-reload runtime guidance
        // (measured 1182 minimal).
        let max_tokens = match mode {
            SystemPromptMode::Minimal => 1220,
            _ => 2000,
        };
        assert!(estimate_tokens(&result) <= max_tokens, "{mode:?}: {} tokens", estimate_tokens(&result));
    }
}
#[test]
fn test_coordinator_guidance_follows_active_selection_and_preserves_sections() {
    let mut prompt = minimal_system_prompt().to_string();
    apply_coordinator_role_guidance(&mut prompt, true);
    prompt.push_str("\n## Environment\n- asymmetric sentinel\n");
    let before_reapplying = prompt.clone();
    apply_coordinator_role_guidance(&mut prompt, true);
    assert_eq!(prompt, before_reapplying);
    assert_eq!(prompt.matches("## Coordinator Role").count(), 1);
    assert!(prompt.contains("- asymmetric sentinel"));
    assert!(!prompt.contains("Prefer skills and tool search"));
    apply_coordinator_role_guidance(&mut prompt, false);
    assert!(!prompt.contains("## Coordinator Role"));
    assert!(prompt.contains("Prefer skills and tool search"));
    assert!(prompt.contains("- asymmetric sentinel"));
}
#[test]
fn test_estimate_token_count() {
    assert_eq!(estimate_token_count(""), 0);
    assert_eq!(estimate_token_count("hello"), estimate_tokens("hello") as u64);
    assert_eq!(estimate_token_count("1234"), estimate_tokens("1234") as u64);
    assert_eq!(estimate_token_count("12345"), estimate_tokens("12345") as u64);

    // Realistic prompt size check — these are estimates, not exact token counts
    let minimal_tokens = estimate_token_count(minimal_system_prompt());
    let default_tokens = estimate_token_count(default_system_prompt());
    // Same budgets as the dedicated token-count tests above.
    // Minimal raised from 665 for the reuse-reads runtime guidance bullet (measured 688).
    // Raised to 725 for the hypothesis-loop runtime bullet (measured 710).
    // Raised to 890/1220 for the fold-by-default delegation rule naming the three
    // keep-conditions (measured 878 minimal).
    // Raised for the partial-commit and conflict-reload runtime guidance
    // (measured 920 minimal, 1257 default).
    assert!(minimal_tokens <= 955, "Minimal prompt tokens: {minimal_tokens}");
    assert!(default_tokens <= 1290, "Default prompt tokens: {default_tokens}");
}
