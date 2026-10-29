use super::*;

#[test]
fn test_default_prompt_token_count() {
    let approx_tokens = estimate_token_count(default_system_prompt());
    // Includes direct patch calls and one bounded context-mismatch recovery read.
    assert!(approx_tokens <= 1210, "Default prompt should stay compact, got ~{approx_tokens}");
}
#[tokio::test]
async fn test_default_live_prompt_budget_with_instruction_inline() {
    use crate::project_doc::build_instruction_appendix_with_context;

    let workspace = tempfile::TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join(".git"), "gitdir: /tmp/git").expect("git marker");
    std::fs::write(
        workspace.path().join("AGENTS.md"),
        "- run ./scripts/check.sh\n- avoid adding to vtcode-core\n- use Conventional Commits\n- start with docs/ARCHITECTURE.md\n",
    )
    .expect("write agents");
    std::fs::create_dir_all(workspace.path().join(".vtcode/rules")).expect("rules dir");
    std::fs::write(
        workspace.path().join(".vtcode/rules/rust.md"),
        "---\npaths:\n  - \"**/*.rs\"\n---\n# Rust\n- keep changes surgical\n",
    )
    .expect("write rust rule");
    std::fs::create_dir_all(workspace.path().join("src")).expect("src dir");
    std::fs::write(workspace.path().join("src/lib.rs"), "pub fn main() {}\n").expect("write lib.rs");

    let mut config = VTCodeConfig::default();
    // Pin Default: this gate covers the fuller profile with instructions.
    config.agent.system_prompt_mode = SystemPromptMode::Default;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    let base = compose_system_instruction_text(workspace.path(), Some(&config), None).await;
    let appendix = build_instruction_appendix_with_context(
        &config.agent,
        workspace.path(),
        &[workspace.path().join("src/lib.rs")],
    )
    .await
    .expect("instruction appendix");
    let prompt = format!("{base}\n\n# INSTRUCTIONS\n{appendix}");
    let approx_tokens = estimate_token_count(&prompt);

    assert!(prompt.contains("### Instruction map"));
    assert!(prompt.contains("# Rust"));
    assert!(prompt.contains("- keep changes surgical"));
    assert!(!prompt.contains("### On-demand loading"));
    // Raised from 1250 for the reuse-reads runtime guidance bullet (measured 1284).
    // Raised to 1320 for the hypothesis-loop runtime bullet (measured 1306).
    // Named HOVER guidance measures 1321 tokens.
    assert!(approx_tokens <= 1480, "got ~{approx_tokens} tokens");
}
#[tokio::test]
async fn test_generated_prompts_do_not_use_deprecated_update_plan() {
    let project_root = PathBuf::from(".");

    for (mode_name, mode) in [
        ("default", SystemPromptMode::Default),
        ("minimal", SystemPromptMode::Minimal),
        ("specialized", SystemPromptMode::Specialized),
    ] {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = mode;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(&project_root, Some(&config), None).await;

        assert!(!result.contains("update_plan"), "{mode_name} prompt should not reference deprecated update_plan");
    }
}
#[tokio::test]
async fn test_default_prompt_omits_non_baseline_tools() {
    let project_root = PathBuf::from(".");
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Default;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;

    let result = compose_system_instruction_text(&project_root, Some(&config), None).await;

    assert!(result.contains("`exec_command`, `write_stdin`, and `apply_patch`"));
    assert!(result.contains("exec_command.cmd"));
    assert!(result.contains("## Shell Profile"));
    assert!(!result.contains("task_tracker"));
    assert!(!result.contains("list_files"));
    assert!(!result.contains("read_file"));
}
#[tokio::test]
async fn test_default_and_specialized_prompts_drop_rigid_summary_template() {
    let project_root = PathBuf::from(".");

    for (mode_name, mode) in [
        ("default", SystemPromptMode::Default),
        ("specialized", SystemPromptMode::Specialized),
    ] {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = mode;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(&project_root, Some(&config), None).await;

        assert!(!result.contains("References\n"), "{mode_name} prompt should not force a References section");
        assert!(!result.contains("Next action"), "{mode_name} prompt should not force a Next action section");
        assert!(
            !result.contains("Scope checkpoint"),
            "{mode_name} prompt should not require the old plan blueprint bullets"
        );
    }
}
#[tokio::test]
async fn test_generated_prompts_keep_sparse_execution_contract() {
    let project_root = PathBuf::from(".");

    for (mode_name, mode) in [
        ("default", SystemPromptMode::Default),
        ("minimal", SystemPromptMode::Minimal),
        ("lightweight", SystemPromptMode::Lightweight),
        ("specialized", SystemPromptMode::Specialized),
    ] {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = mode;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(&project_root, Some(&config), None).await;
        let normalized = result.to_ascii_lowercase();

        assert!(
            normalized.contains("compact") || normalized.contains("concise"),
            "{mode_name} prompt should keep output guidance compact"
        );
        assert!(
            normalized.contains("low-risk") || normalized.contains("reversible"),
            "{mode_name} prompt should include follow-through guidance"
        );
        assert!(
            result.contains(crate::prompts::runtime_guidance::VERIFICATION_OUTCOME_LINE),
            "{mode_name} prompt should include the verification outcome rule"
        );
        assert_eq!(
            result
                .matches("HOVER: On mismatch inspect evidence and revise within limits.")
                .count(),
            1,
            "{mode_name} prompt should include bounded HOVER guidance once"
        );
        assert!(result.contains("Stop after required passes."));
        assert!(normalized.contains("do not guess"), "{mode_name} prompt should gate missing context");
        assert!(
            normalized.contains("do the rest and state plainly what is missing"),
            "{mode_name} prompt should require partial progress when part of the task is blocked"
        );
        assert!(
            normalized.contains("update only on findings, direction changes, or blockers")
                && normalized.contains("finish with the outcome")
                && !normalized.contains("before tools: state the next phase in one line"),
            "{mode_name} prompt should define user-facing progress updates"
        );
        assert!(
            normalized.contains("cite `path:line`"),
            "{mode_name} prompt should include grounding/citation guidance"
        );
        assert!(!result.contains('ƒ'), "{mode_name} prompt should not contain stray prompt characters");
    }
}
#[test]
fn test_prompt_text_avoids_hardcoded_loop_thresholds() {
    let specialized_prompt = specialized_instruction_text();
    assert!(!default_system_prompt().contains("stuck twice"));
    assert!(!minimal_system_prompt().contains("stuck twice"));
    assert!(!specialized_prompt.contains("stuck twice"));
    assert!(!specialized_prompt.contains("10+ calls without progress"));
    assert!(!specialized_prompt.contains("Same tool+params twice"));
}
#[test]
fn test_harness_awareness_in_prompts() {
    assert!(default_system_prompt().contains("AGENTS.md"), "Default prompt should reference AGENTS.md as map");
    assert!(
        specialized_instruction_text().contains("the architecture documents the instruction map points to"),
        "Specialized prompt should point at the repo's architecture documents"
    );
    assert!(minimal_system_prompt().contains("AGENTS.md"), "Minimal prompt should still reference AGENTS.md");
}
#[test]
fn test_prompts_reject_guessing_when_context_is_missing() {
    assert!(default_system_prompt().contains("do not guess"), "Default prompt should reject guessing");
    assert!(specialized_instruction_text().contains("do not guess"), "Specialized prompt should reject guessing");
    assert!(minimal_system_prompt().contains("do not guess"), "Minimal prompt should still reject guessing");
}
#[test]
fn test_prompts_include_compaction_preservation_contract() {
    assert!(
        default_system_prompt().contains("touched files"),
        "Default prompt should preserve touched files across compaction"
    );
    assert!(
        default_system_prompt().contains("decisions made so far"),
        "Default prompt should preserve decision rationale across compaction"
    );
    assert!(
        default_system_prompt().contains("tracker state"),
        "Default prompt should preserve tracker state across compaction"
    );
    assert!(
        default_system_prompt().contains("verification status"),
        "Default prompt should preserve verification status across compaction"
    );
    assert!(
        minimal_system_prompt().contains("touched files"),
        "Minimal prompt should preserve touched files across compaction"
    );
    for (mode_name, prompt) in [
        ("default", default_system_prompt()),
        ("minimal", minimal_system_prompt()),
        ("lightweight", default_lightweight_prompt()),
        ("specialized", specialized_system_prompt()),
    ] {
        assert_eq!(
            prompt.matches("never claim a check passed unless you ran it").count(),
            1,
            "{mode_name} prompt should state the verification outcome rule exactly once"
        );
    }
}
#[test]
fn test_default_prompt_stays_lean_but_complete() {
    let prompt = default_system_prompt();

    assert!(prompt.contains("## Contract"), "Default prompt should include the lean contract section");
    assert!(prompt.contains("Small tasks: up to four bullets"), "Default prompt should clamp output shape");
    assert!(
        prompt.contains(crate::prompts::runtime_guidance::VERIFICATION_OUTCOME_LINE),
        "Default prompt should require honest verification reporting"
    );
    assert!(
        prompt.contains("for a teammate who is catching up"),
        "Default prompt should shape progress updates for a reader"
    );
    assert!(prompt.contains("For tests, start from the risks"));
    assert!(prompt.contains("boundaries and asymmetric cases from both sides"));
    assert!(prompt.contains("without the code's own helpers"));
}
#[test]
fn test_default_prompt_omits_removed_model_facing_tool_names() {
    let prompt = default_system_prompt();

    assert_no_removed_model_facing_tool_names(prompt);
    assert!(prompt.contains("exec_command"), "Default prompt should keep baseline shell guidance");
}
#[tokio::test]
async fn test_composed_default_prompt_omits_removed_model_facing_tool_names() {
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Default;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;

    let prompt = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert_no_removed_model_facing_tool_names(&prompt);
    assert!(prompt.contains("exec_command"), "Composed default prompt should keep baseline shell guidance");
    assert!(prompt.contains("## Shell Profile"));
    assert!(prompt.contains("controls prompt examples and expected command syntax only"));
}
#[tokio::test]
async fn test_composed_prompts_render_explicit_shell_profiles() {
    let project_root = PathBuf::from(".");
    let mut config = VTCodeConfig::default();
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;

    config.agent.shell_prompt_profile = ShellPromptProfile::UnixLike;
    let unix_prompt = compose_system_instruction_text(&project_root, Some(&config), None).await;
    assert!(unix_prompt.contains("Active shell profile: `unix_like`"));
    assert!(unix_prompt.contains("does not rewrite GNU flags for macOS BSD tools"));
    assert!(unix_prompt.contains("does not translate GNU-to-BSD"));

    config.agent.shell_prompt_profile = ShellPromptProfile::PowerShell;
    let powershell_prompt = compose_system_instruction_text(&project_root, Some(&config), None).await;
    assert!(powershell_prompt.contains("Active shell profile: `powershell`"));
    assert!(powershell_prompt.contains("`Get-ChildItem`"));
    assert!(powershell_prompt.contains("use WSL"));
    assert!(powershell_prompt.contains("Unix-to-PowerShell"));
    assert!(!powershell_prompt.contains("`ls`, `rg`, `find`, `cat`, `sed`, and `awk`"));
}
#[test]
fn test_planning_notice_omits_removed_model_facing_tool_names() {
    assert_no_removed_model_facing_tool_names(PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE);
    assert!(PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE.contains("exec_command"));
    assert!(PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE.contains("return the final plan without more tool calls"));
    assert!(PLANNING_WORKFLOW_READ_ONLY_NOTICE_LINE.contains("apply_patch"));
}
#[test]
fn test_all_prompt_modes_treat_completion_as_checkpoint_not_proof() {
    for (mode_name, prompt) in [
        ("default", default_system_prompt()),
        ("minimal", minimal_system_prompt()),
        ("lightweight", default_lightweight_prompt()),
        ("specialized", specialized_instruction_text().as_str()),
    ] {
        assert_eq!(
            prompt
                .matches(crate::prompts::runtime_guidance::VERIFICATION_OUTCOME_LINE)
                .count(),
            1,
            "{mode_name} prompt should report completion only after verification"
        );
        assert!(
            prompt.contains("- Finish the whole task; do the rest and state plainly what is missing if blocked."),
            "{mode_name} prompt should require finishing the whole task"
        );
    }
}
#[test]
fn test_prompts_encode_explicit_delegation_contract() {
    let prompt = default_system_prompt();

    assert!(
        prompt.contains("keep small tasks and verification in the main thread"),
        "Default prompt should keep control on the main thread"
    );
    assert!(
        prompt.contains("Delegate only sizeable, independent work to subagents"),
        "Default prompt should restrict delegation to sizeable independent work"
    );
    assert!(
        prompt
            .contains("- Brief a subagent fully the first time, and use its findings rather than redoing the work.\n"),
        "Default prompt should tell the parent to reuse subagent findings"
    );
    // When to delegate has one home in Runtime Guidance; the Default line
    // covers only how to brief and reuse.
    assert!(!prompt.contains("reserve them for work like"), "Default prompt should not restate when to delegate");
    assert!(
        minimal_system_prompt().contains("Delegate only sizeable, independent work to subagents"),
        "Minimal prompt should preserve the delegation contract"
    );
}
#[test]
fn test_default_prompt_includes_grounding_and_action_bias() {
    let prompt = default_system_prompt();
    assert!(prompt.contains("Read code before claims"), "Default prompt should include grounding guidance");
    assert!(
        prompt.contains("at the intended scope"),
        "Default prompt should include anti-overengineering guidance"
    );
    assert!(
        prompt.contains("make it with the tools rather than describing it"),
        "Default prompt should include action bias for tool-using agents"
    );
}
#[test]
fn test_default_prompt_omits_accuracy_addendum() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let config = VTCodeConfig::default();
    let prompt = runtime.block_on(compose_system_instruction_text(&PathBuf::from("."), Some(&config), None));

    assert!(
        !prompt.contains("## Accuracy Optimization"),
        "Runtime prompt should omit the accuracy optimization section"
    );
    assert!(prompt.contains("do not guess"), "Prompt should still preserve the uncertainty guardrail");
}
#[tokio::test]
async fn test_generated_prompts_keep_operating_profiles_bounded() {
    let project_root = PathBuf::from(".");

    for (mode_name, mode) in [
        ("default", SystemPromptMode::Default),
        ("minimal", SystemPromptMode::Minimal),
        ("lightweight", SystemPromptMode::Lightweight),
        ("specialized", SystemPromptMode::Specialized),
    ] {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = mode;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result = compose_system_instruction_text(&project_root, Some(&config), None).await;

        assert!(result.contains("## Contract"), "{mode_name} prompt should reuse the canonical base prompt");
        assert!(
            result.matches("## Operating Profile").count() == 1,
            "{mode_name} prompt should add only one operating profile"
        );
    }
}
#[test]
fn test_search_guidance_prefers_structural_and_rg() {
    let guidelines = generate_tool_guidelines_for_profile(
        &[tools::EXEC_COMMAND.to_string()],
        None,
        ResolvedShellPromptProfile::UnixLike,
        true,
    );
    assert!(
        guidelines.contains("`exec_command.cmd` with `ls`, `rg`"),
        "Tool guidance should browse through shell commands"
    );
    assert!(guidelines.contains("git diff -- <path>"), "Tool guidance should keep diff guidance explicit");
}
// ENHANCEMENT TESTS

#[tokio::test]
async fn test_dynamic_guidelines_read_only() {
    use crate::config::types::CapabilityLevel;

    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Default;

    let ctx = PromptContext {
        capability_level: Some(CapabilityLevel::FileReading),
        ..PromptContext::default()
    };

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

    assert!(
        result.contains("Capabilities: read-only"),
        "Should detect read-only capabilities when no edit/write/exec tools available"
    );
    assert!(result.contains("do not modify files"), "Should explain read-only constraints");
}
#[tokio::test]
async fn test_dynamic_guidelines_tool_preferences() {
    let config = VTCodeConfig::default();

    let mut ctx = PromptContext::default();
    ctx.add_tool(tools::EXEC_COMMAND.to_string());
    ctx.add_tool(tools::WRITE_STDIN.to_string());
    ctx.add_tool(tools::APPLY_PATCH.to_string());

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

    assert!(
        result.contains("exec_command") && result.contains("apply_patch"),
        "Should suggest baseline shell and patch tools"
    );
    assert_no_removed_model_facing_tool_names(&result);
}
#[tokio::test]
async fn test_live_prompt_renders_workspace_language_hints() {
    let workspace = tempfile::TempDir::new().expect("workspace tempdir");
    std::fs::create_dir_all(workspace.path().join("src")).expect("create src");
    std::fs::create_dir_all(workspace.path().join("web")).expect("create web");
    std::fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");
    std::fs::write(workspace.path().join("web/app.ts"), "const app = 1;\n").expect("write ts");

    let config = VTCodeConfig::default();
    let ctx = PromptContext::from_workspace_tools(workspace.path(), [tools::EXEC_COMMAND]);
    let result = compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

    assert!(result.contains("## Environment"));
    assert!(result.contains("Rust, TypeScript"));
    assert!(result.contains("structural-search `lang`"));
}
#[tokio::test]
async fn test_live_prompt_omits_workspace_language_hints_without_languages() {
    let workspace = tempfile::TempDir::new().expect("workspace tempdir");
    let config = VTCodeConfig::default();
    let ctx = PromptContext::from_workspace_tools(workspace.path(), [tools::EXEC_COMMAND]);
    let result = compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

    assert!(!result.contains("Languages:"));
}
#[tokio::test]
async fn test_live_prompt_omits_project_docs_and_user_instructions_from_base_prompt() {
    let workspace = tempfile::TempDir::new().expect("workspace tempdir");
    std::fs::write(workspace.path().join("AGENTS.md"), "- Root summary\n\nFollow the root guidance.\n")
        .expect("write agents");

    let mut config = VTCodeConfig::default();
    config.agent.user_instructions = Some("keep responses terse".to_string());
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 4096;

    let result = compose_system_instruction_text(workspace.path(), Some(&config), None).await;

    assert!(!result.contains("## AGENTS.MD INSTRUCTION HIERARCHY"));
    assert!(!result.contains("### Instruction map"));
    assert!(!result.contains("### Key points"));
    assert!(!result.contains("keep responses terse"));
    assert!(!result.contains("Root summary"));
    assert!(!result.contains("Follow the root guidance."));
}
#[tokio::test]
async fn test_workspace_prompt_resources_override_base_and_keep_dynamic_sections() {
    use crate::skills::model::{SkillMetadata, SkillScope};

    let workspace = tempfile::TempDir::new().expect("workspace tempdir");
    let prompts_dir = workspace.path().join(".vtcode/prompts");
    std::fs::create_dir_all(&prompts_dir).expect("create prompts dir");
    std::fs::write(prompts_dir.join("system.md"), "# Workspace system base").expect("system");
    std::fs::write(prompts_dir.join("append-system.md"), "Workspace prompt appendix").expect("append");

    let mut config = VTCodeConfig::default();
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = true;

    let mut ctx = PromptContext::default();
    ctx.add_tool(tools::EXEC_COMMAND.to_string());
    ctx.add_skill_metadata(SkillMetadata {
        name: "skill-creator".to_string(),
        description: "Create skills".to_string(),
        short_description: None,
        path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
        scope: SkillScope::System,
        manifest: None,
    });
    ctx.set_current_directory(workspace.path().to_path_buf());

    let result = compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

    assert!(result.starts_with("# Workspace system base"));
    assert!(result.contains(crate::prompts::runtime_guidance::RUNTIME_GUIDANCE_SECTION));
    assert_eq!(
        result
            .matches(crate::prompts::runtime_guidance::RUNTIME_GUIDANCE_SECTION)
            .count(),
        1
    );
    assert!(result.contains("Workspace prompt appendix"));
    assert!(result.contains("## Active Tools"));
    assert!(result.contains("## Skills"));
    assert!(result.contains("## Environment"));

    let appendix_pos = result.find("Workspace prompt appendix").expect("append text");
    let tools_pos = result.find("## Active Tools").expect("tools section");
    let skills_pos = result.find("## Skills").expect("skills section");
    let env_pos = result.find("## Environment").expect("environment section");

    assert!(appendix_pos < skills_pos);
    assert!(skills_pos < tools_pos);
    assert!(tools_pos < env_pos);
}
