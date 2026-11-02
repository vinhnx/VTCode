use super::*;

#[tokio::test]
async fn planning_prompt_size_fixed_fixture() {
    use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;
    use crate::llm::provider::ToolDefinition;
    use crate::llm::providers::OpenAIProvider;
    use crate::prompts::{
        RuntimePromptContract, append_runtime_mode_sections, append_runtime_tool_prompt_sections_for_model,
    };

    struct PlanningPromptSizeCase {
        density: &'static str,
        prompt_budget_tokens: u64,
        max_prompt_bytes: usize,
        max_estimated_tokens: usize,
    }

    let workspace = tempfile::TempDir::new().expect("workspace");
    let provider = OpenAIProvider::new("offline-fixture".into());
    let names = [
        tools::EXEC_COMMAND,
        tools::CODE_SEARCH,
        tools::TASK_TRACKER,
        tools::REQUEST_USER_INPUT,
    ];
    let snapshot = SessionToolCatalogSnapshot::new(
        7,
        9,
        true,
        true,
        Some(std::sync::Arc::new(
            names
                .iter()
                .map(|name| {
                    ToolDefinition::function(
                        (*name).to_string(),
                        "Fixture tool".to_string(),
                        serde_json::json!({"type": "object"}),
                    )
                })
                .collect(),
        )),
        false,
    );
    // Source inspection adds one shared runtime sentence (about 60 tokens).
    // Retain the canonical output contract and HOVER guidance. Before the tracker
    // examples, Default/Minimal measured 9847/10180 bytes and 2107/2205 tokens.
    // Fresh completion guidance adds 248 bytes and about 45 estimated tokens.
    // Zero-on-success planning/runtime guidance adds 253 bytes and about 51 tokens.
    // Action-specific tracker examples measure 9997 bytes/2146 tokens for Default
    // and 10269 bytes/2216 tokens for Minimal; keep tight caps and presence checks.
    // Cancelled planning resumes on fresh input; retain a 50-byte allowance for this policy.
    // Terminal planning guidance adds 128 bytes and about 30 estimated tokens.
    // Partial-commit and conflict-reload runtime guidance adds 229 bytes and
    // about 45 estimated tokens (Default measures 11328 bytes/2398 tokens,
    // Minimal measures 11600 bytes/2468 tokens).
    for case in [
        PlanningPromptSizeCase {
            density: "Default",
            prompt_budget_tokens: 100_000,
            max_prompt_bytes: 11_400,
            max_estimated_tokens: 2_435,
        },
        PlanningPromptSizeCase {
            density: "Minimal",
            prompt_budget_tokens: 1,
            max_prompt_bytes: 11_670,
            max_estimated_tokens: 2_505,
        },
    ] {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Minimal;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = true;
        config.agent.instruction_max_bytes = 0;
        config.agent.shell_prompt_profile = ShellPromptProfile::UnixLike;
        let mut context = PromptContext {
            available_tools: names.iter().map(|name| (*name).to_string()).collect(),
            ..Default::default()
        };
        context.set_current_directory(PathBuf::from("/workspace"));
        let mut prompt = compose_system_instruction_text(workspace.path(), Some(&config), Some(&context)).await;
        append_runtime_mode_sections(
            &mut prompt,
            RuntimePromptContract {
                planning_active: true,
                request_user_input_enabled: true,
                ..Default::default()
            },
        );
        config.agent.max_system_prompt_tokens = case.prompt_budget_tokens;
        append_runtime_tool_prompt_sections_for_model(
            &mut prompt,
            &snapshot,
            true,
            ResolvedShellPromptProfile::UnixLike,
            &provider,
            crate::config::constants::models::openai::DEFAULT_MODEL,
            Some(&config),
        );
        eprintln!(
            "planning fixture {}: {} bytes, {} estimated tokens",
            case.density,
            prompt.len(),
            estimate_tokens(&prompt)
        );
        assert!(prompt.contains(PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE));
        assert!(prompt.contains("Checklists do not persist plans"));
        assert!(prompt.contains("Rejected plans stay blocked; repair before approval"));
        assert!(prompt.len() <= case.max_prompt_bytes, "{} planning fixture exceeded byte budget", case.density);
        assert!(
            estimate_tokens(&prompt) <= case.max_estimated_tokens,
            "{} planning fixture exceeded token budget",
            case.density
        );
    }
}
#[tokio::test]
async fn test_minimal_mode_selection() {
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Minimal;
    // Disable enhancements for base prompt size testing
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    // Minimal prompt should remain compact and deterministic without AGENTS.md injection.
    // Char bound is a smoke check only; tokens are authoritative.
    // Includes direct patch calls and bounded context-mismatch recovery.
    // Raised from 3700 for the reuse-reads and exact-whitespace patch-copy guidance (measured 3917).
    // Raised from 4700 for the fold-by-default delegation rule (measured 4719).
    // Raised for the partial-commit and conflict-reload runtime guidance (measured 4948).
    assert!(result.len() < 5010, "Minimal mode should produce <5.01K chars (was {} chars)", result.len());
    assert!(result.contains("VT Code") || result.contains("VT Code"), "Should contain VT Code identifier");
}
#[tokio::test]
async fn test_default_prompt_selection() {
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Default;
    // Disable enhancements for base prompt size testing
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    // Includes fresh completion evidence guidance (+248 bytes, about 45 tokens).
    // Generated verifier success guidance adds 114 bytes (about 25 tokens).
    // Includes patch recovery guidance; token tests stay authoritative.
    // Raised from 5400 for the reuse-reads runtime guidance bullet (measured 5496).
    // Named HOVER guidance measures 5616 bytes; retain a narrow byte allowance.
    // Raised from 6350 for the fold-by-default delegation rule (measured 6375).
    // Raised for the partial-commit and conflict-reload runtime guidance (measured 6604).
    assert!(
        result.len() <= 6670,
        "Default mode should stay sparse with runtime guidance (<=6670 bytes, was {} bytes)",
        result.len()
    );
    assert!(result.contains("`exec_command`, `write_stdin`, and `apply_patch`"));
    assert!(result.contains("## Shell Profile"));
    assert!(!result.contains("task_tracker"));
    assert!(!result.contains("@file"));
    assert!(result.contains("Planning workflow"));
}
#[tokio::test]
async fn test_lightweight_mode_selection() {
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
    // Disable enhancements for base prompt size testing
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(result.len() > 100, "Lightweight should be >100 chars");
    // Includes fresh completion evidence guidance (+248 bytes, about 45 tokens).
    // Generated verifier success guidance adds 114 bytes (about 25 tokens).
    // Includes patch recovery guidance; token tests stay authoritative.
    // Raised from 4800 for the reuse-reads runtime guidance bullet (measured 4862).
    // Raised for the partial-commit and conflict-reload runtime guidance (measured 5970).
    assert!(
        result.len() < 6040,
        "Lightweight should be compact with runtime guidance (<6.04K chars, was {} chars)",
        result.len()
    );
    assert!(result.contains("task_tracker"));
    assert!(!result.contains("@file"));
    assert!(result.contains("act directly in this thread"));
}
#[tokio::test]
async fn test_lightweight_mode_skips_structured_reasoning_by_default() {
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;
    config.agent.include_structured_reasoning_tags = None;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(
        !result.contains("## Structured Reasoning"),
        "Lightweight mode should omit structured reasoning by default"
    );
}
#[tokio::test]
async fn test_lightweight_mode_allows_explicit_structured_reasoning() {
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;
    config.agent.include_structured_reasoning_tags = Some(true);

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(
        result.contains("## Structured Reasoning"),
        "Lightweight mode should include structured reasoning when explicitly enabled"
    );
    assert!(result.contains("<reasoning_plan>"));
    assert!(!result.contains("`<plan>` steps"), "<plan> is reserved for approval artifacts");
}
#[tokio::test]
async fn test_default_prompt_omits_structured_reasoning_by_default() {
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Default;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;
    config.agent.include_structured_reasoning_tags = None;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(
        !result.contains("## Structured Reasoning"),
        "Default mode should omit structured reasoning by default"
    );
}
#[tokio::test]
async fn test_specialized_prompt_omits_structured_reasoning_by_default() {
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Specialized;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;
    config.agent.include_structured_reasoning_tags = None;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    assert!(
        !result.contains("## Structured Reasoning"),
        "Specialized mode should omit structured reasoning unless explicitly enabled"
    );
}
#[tokio::test]
async fn test_specialized_mode_selection() {
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Specialized;
    // Disable enhancements for base prompt size testing
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;

    let result = compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

    // Includes fresh completion evidence guidance (+248 bytes, about 45 tokens).
    // Generated verifier success guidance adds 114 bytes (about 25 tokens).
    // Includes patch recovery guidance; token tests stay authoritative.
    // Raised from 5400 for the reuse-reads runtime guidance bullet (measured 5569).
    // Raised for the partial-commit and conflict-reload runtime guidance (measured 6677).
    assert!(
        result.len() <= 6740,
        "Specialized should stay sparse with runtime guidance (<=6.74K chars, was {} chars)",
        result.len()
    );
    assert!(result.contains("task_tracker"));
    assert!(result.contains("<proposed_plan>"));
    assert!(result.contains(
        "- When repo-wide invariants matter, also read the architecture documents the instruction map points to."
    ));
    // Shipped prompts serve any repository, so they name no VT Code-specific doc.
    assert!(!result.contains("ARCHITECTURAL_INVARIANTS"));
}
#[test]
fn test_prompt_mode_enum_parsing() {
    assert_eq!(SystemPromptMode::parse("minimal"), Some(SystemPromptMode::Minimal));
    assert_eq!(SystemPromptMode::parse("LIGHTWEIGHT"), Some(SystemPromptMode::Lightweight));
    assert_eq!(SystemPromptMode::parse("Default"), Some(SystemPromptMode::Default));
    assert_eq!(SystemPromptMode::parse("specialized"), Some(SystemPromptMode::Specialized));
    assert_eq!(SystemPromptMode::parse("invalid"), None);
}
#[test]
fn operating_profile_deltas_share_canonical_sentences() {
    assert!(MINIMAL_OPERATING_PROFILE_DELTA.contains(OPERATING_TASK_TRACKER));
    assert!(LIGHTWEIGHT_OPERATING_PROFILE_DELTA.contains(OPERATING_TASK_TRACKER));
    assert!(OPERATING_TASK_TRACKER.contains("use `task_tracker` updates over lists"));
    // `start_planning` guidance has one home: the tool-gated Active Tools
    // line, which is present only when the tool is.
    for delta in [
        DEFAULT_OPERATING_PROFILE_DELTA,
        MINIMAL_OPERATING_PROFILE_DELTA,
        LIGHTWEIGHT_OPERATING_PROFILE_DELTA,
        SPECIALIZED_OPERATING_PROFILE_DELTA,
    ] {
        assert!(!delta.contains("start_planning"), "{delta}");
    }
    assert!(!DEFAULT_OPERATING_PROFILE_DELTA.contains("task_tracker"));
}
/// Regression guard: `PLANNING_WORKFLOW_PLAN_QUALITY_LINE` must keep
/// instructing the model to write file:symbol references as plain text
/// / inline code, not as markdown links or editor/IDE URI schemes (a
/// model was observed emitting `vscode-file://` pseudo-links pointing at
/// the editor binary instead of the referenced repo file).
#[test]
fn plan_quality_line_requires_scope_section() {
    let line = PLANNING_WORKFLOW_PLAN_QUALITY_LINE;
    assert!(line.contains("`## Scope`"));
    assert!(line.contains("never tracker steps"));
}
#[test]
fn plan_quality_line_keeps_rejected_work_blocked_until_repaired() {
    assert!(PLANNING_WORKFLOW_PLAN_QUALITY_LINE.contains("Rejected plans stay blocked; repair before approval"));
}
#[test]
fn plan_quality_line_forbids_markdown_link_file_references() {
    let line = PLANNING_WORKFLOW_PLAN_QUALITY_LINE;
    assert!(line.contains("never as markdown links or editor/IDE URIs"));
    assert!(line.contains("vscode-file://"));
    assert!(line.contains("plain text or inline code"));
}
/// The initial prompt must show the exact step grammar the validator
/// enforces, not just describe it — the repair directive prints the
/// canonical example only after a first rejection (turn_912/913 failed
/// every implementation step on "lacks a concrete target or verification"
/// without the model ever seeing an example). Keep the inline literal in
/// sync with the validator's canonical format.
#[test]
fn plan_quality_line_shows_canonical_step_format() {
    assert!(
        PLANNING_WORKFLOW_PLAN_QUALITY_LINE
            .contains(crate::tools::handlers::planning_workflow::artifacts::CANONICAL_STEP_FORMAT),
        "quality line must embed artifacts::CANONICAL_STEP_FORMAT verbatim"
    );
}
#[test]
fn plan_quality_line_requires_concrete_verify_checks() {
    assert!(PLANNING_WORKFLOW_PLAN_QUALITY_LINE.contains("never as a semicolon chain"));
    assert!(PLANNING_WORKFLOW_PLAN_QUALITY_LINE.contains("npx markdownlint-cli2 README.md"));
    let line = PLANNING_WORKFLOW_PLAN_QUALITY_LINE;
    assert!(line.contains("must exit 0 on success"));
    assert!(line.contains("grep/rg exit 1 cannot satisfy completion"));
    assert!(line.contains("both present and absent inputs"));
    assert!(line.contains("one concrete `verify:`/`verification:` command or observable check"));
    assert!(line.contains("vague prose"));
    assert!(line.contains("comma-separated verify entries that are not commands or observable checks"));
    assert!(
        line.contains("Commas inside single or double quotes stay inside one verify item"),
        "quality line must state the quoted-comma bracket rule so first-attempt plans do not fragment shell patterns"
    );
    assert!(
        line.contains("sed -n") && line.contains("grep -n"),
        "quality line must name inspection commands that recovery synthesis naturally emits"
    );
    assert!(
        line.contains("`git log`")
            && line.contains("`git show`")
            && line.contains("`git diff`")
            && line.contains("`git blame`")
    );
    assert!(line.contains("Reuse evidence; focus output"));
}
#[test]
fn planning_workflow_persistence_policy_assigns_plan_lifecycle_to_runtime() {
    let line = PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE;
    assert!(line.contains("Emit exactly one final `<proposed_plan>` block"));
    assert!(line.contains("no surrounding prose"));
    assert!(line.contains("Do not use shell commands or file-writing tools to create or modify `.vtcode/plans/`"));
    assert!(line.contains("runtime owns plan/tracker persistence and validation"));
    assert!(line.contains("approval controls only after successful persistence"));
    assert!(line.contains("Checklists do not persist plans"));
    assert!(line.contains("Resume cancelled planning only on fresh input"));
    assert!(PLANNING_WORKFLOW_PLAN_POLICY_LINE.contains("Exhausted plan validation or recovery waits for retry"));
    assert!(PLANNING_WORKFLOW_PLAN_POLICY_LINE.contains("Explicit Plan mode requires approval even in full-auto"));
    assert!(PLANNING_WORKFLOW_PLAN_POLICY_LINE.contains("User answers never approve implementation"));
}
#[test]
fn test_minimal_prompt_token_count() {
    let approx_tokens = estimate_token_count(minimal_system_prompt());
    // Raised from 400: the shared runtime guidance is now full sentences with reasons.
    // Includes direct patch calls and one bounded context-mismatch recovery read.
    // Raised from 665 for the reuse-reads runtime guidance bullet (measured 688).
    // Raised to 725 for the hypothesis-loop runtime bullet (measured 710).
    // Raised to 890 for the fold-by-default delegation rule (measured 878).
    // Raised for the partial-commit and conflict-reload runtime guidance (measured 920).
    assert!(approx_tokens <= 955, "Minimal prompt should stay compact, got ~{approx_tokens}");
}
