use super::*;
use crate::prompts::runtime_guidance::RUNTIME_GUIDANCE_SECTION;

#[tokio::test]
async fn test_golden_under_budget_output_is_byte_identical() {
    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut config = VTCodeConfig::default();
    // Pin the Default profile: this golden tracks that profile's composed
    // text, not the configured default (Minimal after lean harness defaults).
    config.agent.system_prompt_mode = SystemPromptMode::Default;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = false;
    config.agent.instruction_max_bytes = 0;

    let result = compose_system_instruction_text(workspace.path(), Some(&config), None).await;

    // Assembled from parts so the shared runtime guidance has one home
    // (`RUNTIME_GUIDANCE_SECTION`); the `assert_eq!` below still pins the
    // composed bytes exactly, including section order and separators.
    let expected = [
        r#"# VT Code (Build mode)

You are VT Code (Build mode), a coding agent working in the user's repository and terminal.

Work the way a senior engineer on this codebase would: understand the relevant code before changing it, make the change the task calls for, and report what you actually observed. Scale effort to the ask.

"#,
        RUNTIME_GUIDANCE_SECTION,
        r#"- Across compaction, preserve the task goal, tracker state, touched files, verification status, and decisions made so far.
- Start from the project instruction map (`AGENTS.md`/`CLAUDE.md`) and the code itself, and follow the conventions they show.
- Write for a teammate catching up: complete sentences, no fragments, arrow chains, or invented labels.
- Answer a simple question directly in prose; use headers, lists, and tables only when the content has real structure.
- Correct an earlier statement only when it changes the user's code, conclusions, or decisions, and do it in one plain sentence.
- Match the surrounding code's naming, idiom, and comment density, and comment only on constraints the code cannot show.
- For tests, start from the risks: check boundaries and asymmetric cases from both sides, derive high-risk expected values without the code's own helpers, and assert observable behavior, not just the absence of a panic.

## Operating Profile

- The core tools are `exec_command`, `write_stdin`, and `apply_patch`; `code_search` becomes available in Planning workflow.
- Shell commands go in `exec_command.cmd`, not a separate tool; follow the active shell profile's syntax.
- When the user asks for a change, make it with the tools rather than describing it, unless the mode is read-only.
- Use Planning workflow for research and spec work, and stay read-only until the user states implementation intent.

## Shell Profile
- Active shell profile: `unix_like`.
- On macOS, write BSD-compatible flags for BSD tools. VT Code does not rewrite GNU flags for macOS BSD tools.
- Commands run under the user's login shell, which may be zsh: avoid bash-only syntax such as `${PIPESTATUS[...]}` (it expands empty outside bash); rely on the harness-reported exit status instead of capturing codes through pipes.
- The shell profile controls prompt examples and expected command syntax only; command policy, sandboxing, and approvals remain separate runtime checks. VT Code does not translate GNU-to-BSD, BSD-to-GNU, Unix-to-PowerShell, or PowerShell-to-Unix command flags."#,
    ]
    .concat();
    assert_eq!(result, expected, "single-section base-contract output must stay byte-identical");
}
#[tokio::test]
async fn test_golden_multi_section_output_is_byte_identical() {
    use crate::skills::model::{SkillMetadata, SkillScope};

    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = true;
    config.agent.instruction_max_bytes = 0;
    config.agent.include_structured_reasoning_tags = Some(true);

    let mut ctx = PromptContext::default();
    ctx.add_tool(tools::CODE_SEARCH.to_string());
    ctx.add_tool(tools::EXEC_COMMAND.to_string());
    ctx.add_skill_metadata(SkillMetadata {
        name: "skill-creator".to_string(),
        description: "Create skills".to_string(),
        short_description: None,
        path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
        scope: SkillScope::System,
        manifest: None,
    });
    ctx.set_current_directory(PathBuf::from("/workspace"));

    let result = compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

    // Same compositional split as the base-contract golden above.
    let expected = [
        r#"# VT Code (Build mode)

You are VT Code (Build mode), a coding agent working in the user's repository and terminal.

"#,
        RUNTIME_GUIDANCE_SECTION,
        r#"- Across compaction, preserve the task goal, tracker state, touched files, verification status, and decisions made so far.
- Start from the project instruction map (`AGENTS.md`/`CLAUDE.md`) and the code itself, and follow the conventions they show.
- Write for a teammate catching up: complete sentences, no fragments, arrow chains, or invented labels.
- Answer a simple question directly in prose; use headers, lists, and tables only when the content has real structure.
- Correct an earlier statement only when it changes the user's code, conclusions, or decisions, and do it in one plain sentence.
- Match the surrounding code's naming, idiom, and comment density, and comment only on constraints the code cannot show.
- For tests, start from the risks: check boundaries and asymmetric cases from both sides, derive high-risk expected values without the code's own helpers, and assert observable behavior, not just the absence of a panic.

## Operating Profile

- This profile is for simple work: act directly in this thread and keep the loop short.
- Track multi-step work; use `task_tracker` updates over lists.

## Structured Reasoning

When visible structure helps, tag conclusions and supporting evidence with `<analysis>`, advisory steps with `<reasoning_plan>`, blockers with `<uncertainty>`, and observed checks with `<verification>`. Do not expose private chain-of-thought. `<plan>` is reserved for the planning workflow's approval artifact. When code or tools will consume a decision, prefer JSON or a function call over prose.

## Shell Profile
- Active shell profile: `unix_like`.
- On macOS, write BSD-compatible flags for BSD tools. VT Code does not rewrite GNU flags for macOS BSD tools.
- Commands run under the user's login shell, which may be zsh: avoid bash-only syntax such as `${PIPESTATUS[...]}` (it expands empty outside bash); rely on the harness-reported exit status instead of capturing codes through pipes.
- The shell profile controls prompt examples and expected command syntax only; command policy, sandboxing, and approvals remain separate runtime checks. VT Code does not translate GNU-to-BSD, BSD-to-GNU, Unix-to-PowerShell, or PowerShell-to-Unix command flags.

## Skills
Use a skill only when the user names it or the task clearly matches. Load details on demand.
- skill-creator: Create skills

## Active Tools
- `verify: [skip Markdown lint if unavailable]`: report skipped; review diff/links without installing tools. Lint errors remain failures.
- Browse the repository with `exec_command.cmd`. Prefer `code_search` over `rg`/`grep` for code.
- Batch independent read-only calls; order dependent reads, and serialize mutations.
- In one-shot `exec_command` calls, do not use `!!`, `!$`, `!ssh`, or `fc`; write full command arguments explicitly from conversation or tool results. Interactive shells: review-safe history expansion (Bash `histverify`, zsh `HIST_VERIFY`).
- For long-lived commands, use `background: true` and drive the session with `write_stdin`.
- Run verifiers standalone with `max_output_tokens`. Pure `head`/`tail` tails run standalone; static read-only filtering pipelines use fail-closed `pipefail`. Only terminal exit 0 clears verification; dynamic syntax, mutating tails, `;`, and `||` do not qualify. Recheck after `--fix`.
- Run a real check that exercises the change; a syntax-only or failed-to-start check does not count. Install missing deps via the project's package manager, never sudo; if no check can run, say which and why.
- Run fast checks before full builds.
- `code_search` queries are literal and smart-case; `|` separates alternatives. On a truncated result, narrow the query; on an unavailable result, do not retry identically, drop `file_types` or broaden `path`. Do not JSON-encode arrays or integers as strings. Use `exec_command` or a skill for syntax patterns.
- Build and Auto share tools and safety gates; Auto changes confirmation behavior only after explicit approval or full-auto policy.
- Run independent tools in parallel when inputs do not depend on each other.

## Environment
- Working directory: /workspace"#,
    ]
    .concat();
    assert_eq!(result, expected, "multi-section joined output must stay byte-identical");
}
#[tokio::test]
async fn test_over_budget_without_trim_keeps_full_text_and_reports_over_budget() {
    use crate::skills::model::{SkillMetadata, SkillScope};

    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = true;
    config.agent.instruction_max_bytes = 0;
    config.agent.include_structured_reasoning_tags = Some(true);
    config.agent.max_system_prompt_tokens = 1;
    config.agent.trim_system_prompt = false;
    config.agent.system_prompt_budget_warning = true;

    let mut ctx = PromptContext::default();
    ctx.add_tool(tools::CODE_SEARCH.to_string());
    ctx.add_tool(tools::EXEC_COMMAND.to_string());
    ctx.add_skill_metadata(SkillMetadata {
        name: "skill-creator".to_string(),
        description: "Create skills".to_string(),
        short_description: None,
        path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
        scope: SkillScope::System,
        manifest: None,
    });
    ctx.set_current_directory(PathBuf::from("/workspace"));

    let sections = build_prompt_sections(workspace.path(), Some(&config), Some(&ctx)).await;
    let full_text = join_prompt_sections(&sections);
    let full_tokens = estimate_token_count(&full_text);
    assert!(full_tokens > config.agent.max_system_prompt_tokens, "test setup must exceed the configured budget");

    let (text, report) = compose_system_instruction_with_report(workspace.path(), Some(&config), Some(&ctx)).await;

    assert_eq!(text, full_text, "trim disabled: full untrimmed text must still be used");
    assert!(report.over_budget, "token estimate exceeds configured budget");
    assert_eq!(report.token_estimate, full_tokens);
    assert!(report.trimmed_sections.is_empty(), "no sections should be dropped when trimming is disabled");
}
#[tokio::test]
async fn test_over_budget_with_trim_drops_sections_in_priority_order() {
    use crate::skills::model::{SkillMetadata, SkillScope};

    let workspace = tempfile::TempDir::new().expect("workspace");
    let mut config = VTCodeConfig::default();
    config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
    config.agent.include_temporal_context = false;
    config.agent.include_working_directory = true;
    config.agent.instruction_max_bytes = 0;
    config.agent.include_structured_reasoning_tags = Some(true);
    config.agent.trim_system_prompt = true;
    config.agent.system_prompt_budget_warning = true;

    let mut ctx = PromptContext::default();
    ctx.add_tool(tools::CODE_SEARCH.to_string());
    ctx.add_tool(tools::EXEC_COMMAND.to_string());
    ctx.add_skill_metadata(SkillMetadata {
        name: "skill-creator".to_string(),
        description: "Create skills".to_string(),
        short_description: None,
        path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
        scope: SkillScope::System,
        manifest: None,
    });
    ctx.set_current_directory(PathBuf::from("/workspace"));

    let sections = build_prompt_sections(workspace.path(), Some(&config), Some(&ctx)).await;
    // Budget set to exactly the base-contract-only token count so every
    // droppable (trim_priority = Some(_)) section must be dropped, while
    // the untrimmable base contract always survives.
    let base_only_tokens = sections
        .iter()
        .find(|section| section.kind == SectionKind::BaseContract)
        .map(|section| estimate_token_count(&section.text))
        .expect("base contract section is always present");
    config.agent.max_system_prompt_tokens = base_only_tokens;

    let (text, report) = compose_system_instruction_with_report(workspace.path(), Some(&config), Some(&ctx)).await;

    assert_eq!(
        report.trimmed_sections,
        vec!["structured_reasoning", "skills", "environment_addenda"],
        "only advisory sections may be dropped in priority order"
    );
    assert!(text.contains("## Rules"), "base contract must never be dropped");
    assert!(!text.contains("## Structured Reasoning"));
    assert!(!text.contains("## Skills"));
    assert!(!text.contains("## Environment"));
    assert!(text.contains("## Shell Profile"), "shell safety guidance must be retained");
    assert!(text.contains("## Active Tools"), "active-tool contract must be retained");
    assert!(report.over_budget, "untrimmable safety sections may keep a tiny prompt over budget");
}
#[test]
fn test_cache_key_changes_with_model_capability_configuration() {
    let root = PathBuf::from("/workspace");
    let mut config = VTCodeConfig::default();
    let initial = cache_key(&root, Some(&config), 1);
    config.agent.default_model = "different-model".into();
    let model_changed = cache_key(&root, Some(&config), 1);
    assert_ne!(initial, model_changed);
    config.agent.reasoning_effort = crate::config::types::ReasoningEffortLevel::Max;
    let reasoning_changed = cache_key(&root, Some(&config), 1);
    assert_ne!(model_changed, reasoning_changed);
    assert_ne!(reasoning_changed, cache_key(&root, Some(&config), 2));
}
#[test]
fn stable_cache_identity_and_synthetic_second_turn_usage_are_reported_separately() {
    let root = PathBuf::from("/workspace");
    let config = VTCodeConfig::default();
    assert_eq!(cache_key(&root, Some(&config), 7), cache_key(&root, Some(&config), 7));

    // This fixture validates only local usage accounting. It is not
    // evidence that a provider served the second turn from its cache.
    let second_turn_usage = vtcode_commons::llm::Usage {
        prompt_tokens: 100,
        completion_tokens: 10,
        total_tokens: 110,
        cached_prompt_tokens: Some(75),
        cache_creation_tokens: Some(25),
        cache_read_tokens: None,
        iterations: None,
    };
    assert_eq!(second_turn_usage.cache_hit_rate(), Some(75.0));
}
#[test]
fn test_cache_key_changes_with_budget_settings() {
    let project_root = PathBuf::from("/workspace");
    let base_config = VTCodeConfig::default();
    let base_key = cache_key(&project_root, Some(&base_config), 0);

    let mut max_tokens_changed = VTCodeConfig::default();
    max_tokens_changed.agent.max_system_prompt_tokens += 1;
    assert_ne!(
        base_key,
        cache_key(&project_root, Some(&max_tokens_changed), 0),
        "cache key must change when max_system_prompt_tokens changes"
    );

    let mut warning_changed = VTCodeConfig::default();
    warning_changed.agent.system_prompt_budget_warning = !warning_changed.agent.system_prompt_budget_warning;
    assert_ne!(
        base_key,
        cache_key(&project_root, Some(&warning_changed), 0),
        "cache key must change when system_prompt_budget_warning changes"
    );

    let mut trim_changed = VTCodeConfig::default();
    trim_changed.agent.trim_system_prompt = !trim_changed.agent.trim_system_prompt;
    assert_ne!(
        base_key,
        cache_key(&project_root, Some(&trim_changed), 0),
        "cache key must change when trim_system_prompt changes"
    );
}
#[test]
fn test_cache_key_changes_with_default_primary_agent() {
    let project_root = PathBuf::from("/workspace");
    let base_config = VTCodeConfig {
        default_primary_agent: "build".to_string(),
        ..Default::default()
    };
    let base_key = cache_key(&project_root, Some(&base_config), 0);

    let auto_config = VTCodeConfig {
        default_primary_agent: "auto".to_string(),
        ..Default::default()
    };
    assert_ne!(
        base_key,
        cache_key(&project_root, Some(&auto_config), 0),
        "cache key must change when default_primary_agent changes, since \
         agent_identity_label rewrites the composed prompt"
    );
}
#[tokio::test]
async fn measure_system_prompt_size_returns_non_empty_report_for_empty_workspace() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let config = VTCodeConfig::default();
    let report = measure_system_prompt_size(temp.path(), &config).await;
    assert!(
        report.token_estimate > 0,
        "default system prompt should be non-empty, got {} tokens",
        report.token_estimate
    );
    assert!(
        !report.over_budget,
        "default config should be within default budget, got {} tokens",
        report.token_estimate
    );
    assert!(report.trimmed_sections.is_empty());
}
#[tokio::test]
async fn measure_system_prompt_size_flags_over_budget() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let mut config = VTCodeConfig::default();
    config.agent.max_system_prompt_tokens = 1;
    let report = measure_system_prompt_size(temp.path(), &config).await;
    assert!(report.over_budget, "tiny budget should flag as over budget");
}
#[tokio::test]
async fn measure_system_prompt_size_respects_max_budget_setting() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let mut config = VTCodeConfig::default();
    config.agent.max_system_prompt_tokens = 8_000;
    let report = measure_system_prompt_size(temp.path(), &config).await;
    // Default base prompt is well under 8k tokens for an empty workspace.
    assert!(!report.over_budget, "default prompt should fit within 8k tokens, got {}", report.token_estimate);
}
#[test]
fn planning_guidance_bounds_recovery_and_separates_scope_checks() {
    assert!(PLANNING_WORKFLOW_PLAN_POLICY_LINE.contains("one synthesis and two corrections"));
    assert!(PLANNING_WORKFLOW_PLAN_POLICY_LINE.contains("continuations and resume"));
    assert!(PLANNING_WORKFLOW_PLAN_POLICY_LINE.contains("Within execution budgets"));
    assert!(PLANNING_WORKFLOW_PLAN_POLICY_LINE.contains("User answers never approve implementation"));
    assert!(PLANNING_WORKFLOW_PLAN_QUALITY_LINE.contains("scope checks never replace content verification"));
    assert!(PLANNING_WORKFLOW_PLAN_POLICY_LINE.len() <= 470, "keep shipped planning guidance bounded");
}
