//! System instructions and prompt management.
//!
//! Prompt variants share one canonical base contract plus thin mode deltas and
//! compact runtime addenda. Richer behavior comes from AGENTS.md, dynamic tool
//! guidance, skill metadata, and runtime notices.

use crate::config::constants::{
    instructions as instruction_constants, project_doc as project_doc_constants,
};
use crate::config::types::SystemPromptMode;
use crate::llm::providers::gemini::wire::Content;
use crate::project_doc::read_project_doc;
use crate::prompts::context::PromptContext;
use crate::prompts::guidelines::generate_tool_guidelines;
use crate::prompts::output_styles::OutputStyleApplier;
use crate::prompts::resources::{apply_system_prompt_layers, resolve_system_prompt_layers};
use crate::prompts::system_prompt_cache::PROMPT_CACHE;
use crate::prompts::temporal::generate_temporal_context;
use crate::skills::render::render_prompt_skills_section;
use std::env;
use std::path::Path;
use std::sync::OnceLock;
use tracing::warn;

/// Shared Plan Mode header used by both static and incremental prompt builders.
pub const PLAN_MODE_READ_ONLY_HEADER: &str = "# PLAN MODE (READ-ONLY)";
/// Shared Plan Mode notice line describing strict read-only enforcement.
pub const PLAN_MODE_READ_ONLY_NOTICE_LINE: &str = "Plan Mode is active. Mutating tools are blocked except for optional plan artifact writes under `.vtcode/plans/` (or an explicit custom plan path).";
/// Shared Plan Mode instruction line for transitioning to implementation.
pub const PLAN_MODE_EXIT_INSTRUCTION_LINE: &str =
    "Call `exit_plan_mode` when ready to transition to implementation.";
/// Shared Plan Mode instruction line for decision-complete planning output.
pub const PLAN_MODE_PLAN_QUALITY_LINE: &str = "Explore repository facts first, ask only material blocking questions, keep planning read-only, and emit exactly one decision-complete `<proposed_plan>` block with a summary, implementation steps, test cases, and assumptions/defaults. If something is still unresolved, end with `Next open decision: ...`.";
/// Shared Plan Mode policy line requiring context-aware interview closure before final plans.
pub const PLAN_MODE_INTERVIEW_POLICY_LINE: &str = "In Plan Mode, prefer model-generated `request_user_input` interview questions informed by discovered repository context, keep custom notes/free-form responses available as first-class input, and continue interviewing until material scope/decomposition/verification decisions are closed before finalizing `<proposed_plan>`.";
/// Shared Plan Mode guard line requiring explicit transition from planning to execution.
pub const PLAN_MODE_NO_AUTO_EXIT_LINE: &str = "Do not auto-exit Plan Mode just because a plan exists; wait for explicit implementation intent.";
/// Shared Plan Mode task-tracking line clarifying availability and aliasing.
pub const PLAN_MODE_TASK_TRACKER_LINE: &str =
    "`task_tracker` remains available in Plan Mode (`plan_task_tracker` is a compatibility alias).";
/// Shared reminder appended when presenting plans while still in Plan Mode.
pub const PLAN_MODE_IMPLEMENT_REMINDER: &str = "• Still in Plan Mode (read-only). Say “implement” to execute, or “stay in plan mode” to revise. If automatic Plan->Edit switching fails, manually switch with `/plan off` or `/mode` (or press `Shift+Tab`/`Alt+M` in interactive mode).";

const CANONICAL_SYSTEM_PROMPT: &str = r#"# VT Code

You are VT Code. Be concise, direct, and safe.

## Core Contract

- Start with `AGENTS.md`; inspect code and match local patterns.
- Open indexed instruction or skill files when wording matters.
- Act on safe, reversible steps without asking; ask only for material behavior, API, UX, credential, or external changes.
- If context is missing, say so plainly, do not guess, and finish any unblocked portion first.
- Prefer simple changes; measure before optimizing performance.
- Preserve task goal, acceptance criteria, touched files, test or error outcomes, and decisions with rationale across compaction.
- Use `@file` and `/add-dir` to focus the right code.
- Verify changes yourself; never claim a check passed unless you ran it.
- Respect approval gates and keep destructive or external actions explicit.
- For research or citation-sensitive work, use retrieved evidence and label inference clearly.

## Execution Contract

- Return exactly the requested sections or format; keep outputs concise.
- User instructions override default tone, format, and initiative unless they conflict with safety or honesty.
- Use tools when they improve correctness, completeness, or grounding; do not skip lookup, discovery, or verification.
- Treat the task as incomplete until every requested deliverable is done or explicitly blocked.
- Retry empty, partial, or narrow lookups with a different query or source before concluding nothing exists.
- Before finalizing, check requirements, grounding, format, and permissions.

## Interaction

- Keep user updates brief and high-signal.

## Output

- Keep responses outcome-first. Use file refs when helpful.
- No emoji, filler, or code dumps unless requested. Use ASCII markers only."#;

const MINIMAL_CANONICAL_SYSTEM_PROMPT: &str = r#"# VT Code

You are VT Code. Be concise, direct, and safe.

## Core Contract

- Start with `AGENTS.md`; inspect code first.
- If context is missing, say so plainly, do not guess, and finish any unblocked portion first.
- Preserve task goal, touched files, test or error outcomes, and decision rationale when compacting history.
- Take only safe, reversible steps without asking.
- Verify changes yourself and use retrieved evidence for citation-sensitive work.

## Execution Contract

- Return exactly the requested sections or format; keep outputs concise.
- Use tools for lookup, discovery, and verification before concluding.
- Treat the task as incomplete until every requested deliverable is done or blocked.
- Before finalizing, check requirements, grounding, format, and permissions.
- Keep user updates brief and high-signal."#;

const ACCURACY_OPTIMIZATION_ADDENDUM: &str = r#"## Accuracy Optimization

- Start simple with success criteria or eval examples when accuracy matters.
- Missing, stale, or proprietary knowledge means optimize context; inconsistent format, style, or reasoning means optimize instructions or training.
- Treat prompting, retrieval, and fine-tuning as additive levers; avoid noisy long context.
- For high-stakes tasks, prefer clarification or human review over guessing."#;

const DEFAULT_MODE_DELTA: &str = r#"## Mode

- Use `task_tracker` for non-trivial work.
- Use Plan Mode for research/spec work; stay read-only there until implementation intent is explicit.
- For repo-level changes, use `AGENTS.md` and `docs/harness/ARCHITECTURAL_INVARIANTS.md`."#;

const MINIMAL_MODE_DELTA: &str = r#"## Mode

- Stay lightweight and precise; use `task_tracker` once the task stops being trivial.
- Say so early when uncertain; hidden capabilities route through `list_skills` and `load_skill`.
- Use `AGENTS.md` as the map and `docs/harness/` when structural rules matter."#;

const LIGHTWEIGHT_MODE_DELTA: &str = r#"## Mode

- Act and verify in one thread.
- Use `task_tracker` for multi-step work and Plan Mode for research/spec work.
- Investigate before edits; match nearby patterns."#;

const SPECIALIZED_MODE_DELTA: &str = r#"## Mode

- Explore, plan, then execute.
- Use `task_tracker` for multi-step work, keep one active slice, and use Plan Mode when you need scope closure.
- End plan work with one `<proposed_plan>`; if a path stalls, re-plan into smaller verified slices.
- For repo-level changes, use `AGENTS.md` and `docs/harness/ARCHITECTURAL_INVARIANTS.md`."#;

static DEFAULT_SYSTEM_PROMPT: OnceLock<String> = OnceLock::new();
static MINIMAL_SYSTEM_PROMPT: OnceLock<String> = OnceLock::new();
static DEFAULT_LIGHTWEIGHT_PROMPT: OnceLock<String> = OnceLock::new();
static DEFAULT_SPECIALIZED_PROMPT: OnceLock<String> = OnceLock::new();

pub fn default_system_prompt() -> &'static str {
    static_mode_prompt(SystemPromptMode::Default)
}

pub fn minimal_system_prompt() -> &'static str {
    static_mode_prompt(SystemPromptMode::Minimal)
}

pub fn default_lightweight_prompt() -> &'static str {
    static_mode_prompt(SystemPromptMode::Lightweight)
}

pub fn specialized_system_prompt() -> &'static str {
    static_mode_prompt(SystemPromptMode::Specialized)
}

pub fn minimal_instruction_text() -> String {
    minimal_system_prompt().to_string()
}

pub fn lightweight_instruction_text() -> String {
    default_lightweight_prompt().to_string()
}

pub fn specialized_instruction_text() -> String {
    specialized_system_prompt().to_string()
}

const STRUCTURED_REASONING_INSTRUCTIONS: &str = r#"
## Structured Reasoning

Use tags when helpful: `<analysis>` facts/options, `<plan>` steps, `<uncertainty>` blockers, `<verification>` checks.
"#;

/// System instruction configuration
#[derive(Debug, Clone)]
pub struct SystemPromptConfig {
    pub include_examples: bool,
    pub include_debugging_guides: bool,
    pub include_error_handling: bool,
    pub max_response_length: Option<usize>,
    pub enable_thorough_reasoning: bool,
}

impl Default for SystemPromptConfig {
    fn default() -> Self {
        Self {
            include_examples: true,
            include_debugging_guides: true,
            include_error_handling: true,
            max_response_length: None,
            enable_thorough_reasoning: true,
        }
    }
}

/// Generate system instruction
pub async fn generate_system_instruction(_config: &SystemPromptConfig) -> Content {
    // OPTIMIZATION: default_system_prompt() is &'static str, use directly
    let instruction = default_system_prompt().to_string();

    // Apply output style if possible (using current directory as project root)
    if let Ok(current_dir) = env::current_dir() {
        let styled_instruction = apply_output_style(instruction, None, &current_dir).await;
        Content::system_text(styled_instruction)
    } else {
        Content::system_text(instruction)
    }
}

/// Read AGENTS.md file if present and extract agent guidelines
pub async fn read_agent_guidelines(project_root: &Path) -> Option<String> {
    let max_bytes =
        project_doc_constants::DEFAULT_MAX_BYTES.min(instruction_constants::DEFAULT_MAX_BYTES);
    match read_project_doc(project_root, max_bytes).await {
        Ok(Some(bundle)) => Some(bundle.contents),
        Ok(None) => None,
        Err(err) => {
            warn!("failed to load project documentation: {err:#}");
            None
        }
    }
}

/// Compose the base system instruction plus compact tool/skill/environment addenda.
pub async fn compose_system_instruction_text(
    _project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> String {
    let prompt_mode = vtcode_config
        .map(|c| c.agent.system_prompt_mode)
        .unwrap_or(SystemPromptMode::Default);
    let static_base_prompt = static_mode_prompt(prompt_mode);
    let resolved_layers = resolve_system_prompt_layers(_project_root).await;
    let base_prompt = apply_system_prompt_layers(static_base_prompt, &resolved_layers);

    tracing::trace!(
        mode = ?prompt_mode,
        base_tokens_approx = base_prompt.len() / 4, // rough token estimate
        "Selected system prompt mode"
    );

    let base_len = base_prompt.len();
    let config_overhead = vtcode_config.map_or(0, |_| 1024);
    let estimated_capacity = base_len + config_overhead + 1024;
    let mut instruction = String::with_capacity(estimated_capacity);
    instruction.push_str(&base_prompt);
    if should_include_structured_reasoning(vtcode_config, prompt_mode) {
        append_prompt_section(&mut instruction, STRUCTURED_REASONING_INSTRUCTIONS);
    }
    if should_include_accuracy_optimization(prompt_mode) {
        append_prompt_section(&mut instruction, ACCURACY_OPTIMIZATION_ADDENDUM);
    }

    if let Some(ctx) = prompt_context {
        let guidelines = generate_tool_guidelines(&ctx.available_tools, ctx.capability_level);
        if !guidelines.is_empty() {
            append_prompt_section(&mut instruction, guidelines.trim_start_matches('\n'));
        }
        if let Some(skills_section) = render_prompt_skills_section(&ctx.available_skill_metadata) {
            append_prompt_section(&mut instruction, &skills_section);
        }
    }

    if let Some(environment_section) = render_environment_addenda(vtcode_config, prompt_context) {
        append_prompt_section(&mut instruction, &environment_section);
    }

    instruction
}

fn append_prompt_section(prompt: &mut String, section: &str) {
    prompt.push_str("\n\n");
    prompt.push_str(section);
}

fn static_mode_prompt(prompt_mode: SystemPromptMode) -> &'static str {
    match prompt_mode {
        SystemPromptMode::Default => DEFAULT_SYSTEM_PROMPT
            .get_or_init(|| build_mode_prompt(CANONICAL_SYSTEM_PROMPT, DEFAULT_MODE_DELTA)),
        SystemPromptMode::Minimal => MINIMAL_SYSTEM_PROMPT
            .get_or_init(|| build_mode_prompt(MINIMAL_CANONICAL_SYSTEM_PROMPT, MINIMAL_MODE_DELTA)),
        SystemPromptMode::Lightweight => DEFAULT_LIGHTWEIGHT_PROMPT
            .get_or_init(|| build_mode_prompt(CANONICAL_SYSTEM_PROMPT, LIGHTWEIGHT_MODE_DELTA)),
        SystemPromptMode::Specialized => DEFAULT_SPECIALIZED_PROMPT
            .get_or_init(|| build_mode_prompt(CANONICAL_SYSTEM_PROMPT, SPECIALIZED_MODE_DELTA)),
    }
}

fn build_mode_prompt(base_prompt: &str, mode_delta: &str) -> String {
    let mut prompt = String::with_capacity(base_prompt.len() + mode_delta.len() + 2);
    prompt.push_str(base_prompt);
    prompt.push_str("\n\n");
    prompt.push_str(mode_delta);
    prompt
}

fn render_environment_addenda(
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> Option<String> {
    let mut lines = Vec::new();

    if let Some(ctx) = prompt_context
        && !ctx.languages.is_empty()
    {
        lines.push(format!(
            "- Languages: {}. Match structural-search `lang` when needed.",
            ctx.languages.join(", ")
        ));
    }

    if let Some(cfg) = vtcode_config {
        if let Some(interaction_line) = render_interaction_addendum(cfg) {
            lines.push(interaction_line);
        }

        if cfg.mcp.enabled {
            lines.push("- Sources: prefer MCP before external fetches when available.".to_string());
        }

        if cfg.agent.include_temporal_context && !cfg.prompt_cache.cache_friendly_prompt_shaping {
            lines.push(
                generate_temporal_context(cfg.agent.temporal_context_use_utc)
                    .trim()
                    .replacen("Current date and time", "- Time", 1)
                    .to_string(),
            );
        }

        if cfg.agent.include_working_directory
            && let Some(ctx) = prompt_context
            && let Some(cwd) = &ctx.current_directory
        {
            lines.push(format!("- Working directory: {}", cwd.display()));
        }
    }

    if lines.is_empty() {
        None
    } else {
        Some(format!("## Environment\n{}", lines.join("\n")))
    }
}

fn render_interaction_addendum(cfg: &crate::config::VTCodeConfig) -> Option<String> {
    match (cfg.security.human_in_the_loop, cfg.chat.ask_questions.enabled) {
        (true, true) => None,
        (true, false) => Some(
            "- Interaction: approval may gate sensitive actions; no `request_user_input`, so make reasonable assumptions unless Plan Mode needs follow-up.".to_string(),
        ),
        (false, true) => Some(
            "- Interaction: approval reduced by config; use `request_user_input` for material blockers.".to_string(),
        ),
        (false, false) => Some(
            "- Interaction: approval reduced by config; no `request_user_input`, so make reasonable assumptions unless Plan Mode needs follow-up.".to_string(),
        ),
    }
}

fn should_include_structured_reasoning(
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    mode: SystemPromptMode,
) -> bool {
    if let Some(cfg) = vtcode_config {
        return cfg.agent.should_include_structured_reasoning_tags();
    }

    // Backward-compatible fallback when no config is available.
    matches!(
        mode,
        SystemPromptMode::Default | SystemPromptMode::Specialized
    )
}

fn should_include_accuracy_optimization(mode: SystemPromptMode) -> bool {
    matches!(mode, SystemPromptMode::Default)
}

/// Generate the stable base system instruction with configuration-aware sections.
///
/// Note: This function maintains backward compatibility by not accepting prompt_context.
/// For enhanced prompts with dynamic guidelines, call `compose_system_instruction_text` directly.
pub async fn generate_system_instruction_with_config(
    _config: &SystemPromptConfig,
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> Content {
    let cache_key = cache_key(project_root, vtcode_config);
    let instruction = match PROMPT_CACHE.get(&cache_key) {
        Some(cached) => cached,
        None => {
            let built = compose_system_instruction_text(project_root, vtcode_config, None).await;
            PROMPT_CACHE.insert(cache_key, built.clone());
            built
        }
    };

    // Apply output style if configured
    let styled_instruction = apply_output_style(instruction, vtcode_config, project_root).await;
    Content::system_text(styled_instruction)
}

/// Generate the stable base system instruction without workspace configuration.
pub async fn generate_system_instruction_with_guidelines(
    _config: &SystemPromptConfig,
    project_root: &Path,
) -> Content {
    let cache_key = cache_key(project_root, None);
    let instruction = match PROMPT_CACHE.get(&cache_key) {
        Some(cached) => cached,
        None => {
            let built = compose_system_instruction_text(project_root, None, None).await;
            PROMPT_CACHE.insert(cache_key, built.clone());
            built
        }
    };
    // Apply output style if configured
    let styled_instruction = apply_output_style(instruction, None, project_root).await;
    Content::system_text(styled_instruction)
}

/// Apply output style to a generated system instruction
pub async fn apply_output_style(
    instruction: String,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    project_root: &Path,
) -> String {
    if let Some(config) = vtcode_config {
        let output_style_applier = OutputStyleApplier::new();
        if let Err(e) = output_style_applier
            .load_styles_from_config(config, project_root)
            .await
        {
            tracing::warn!("Failed to load output styles: {}", e);
            instruction // Return original if loading fails
        } else {
            output_style_applier
                .apply_style(&config.output_style.active_style, &instruction, config)
                .await
        }
    } else {
        instruction // Return original if no config
    }
}

fn cache_key(project_root: &Path, vtcode_config: Option<&crate::config::VTCodeConfig>) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();

    // Core key: project root
    project_root.hash(&mut hasher);

    if let Some(cfg) = vtcode_config {
        // Config fields that affect prompt generation
        cfg.agent.include_working_directory.hash(&mut hasher);
        cfg.agent.include_temporal_context.hash(&mut hasher);
        cfg.prompt_cache
            .cache_friendly_prompt_shaping
            .hash(&mut hasher);
        cfg.agent
            .include_structured_reasoning_tags
            .hash(&mut hasher);
        // Use discriminant since SystemPromptMode doesn't derive Hash
        std::mem::discriminant(&cfg.agent.system_prompt_mode).hash(&mut hasher);
    } else {
        "default".hash(&mut hasher);
    }

    format!("sys_prompt:{:016x}", hasher.finish())
}

/// Generate a minimal system instruction (pi-inspired, <1K tokens)
pub fn generate_minimal_instruction() -> Content {
    Content::system_text(minimal_instruction_text())
}

/// Generate a lightweight system instruction for simple operations
pub fn generate_lightweight_instruction() -> Content {
    Content::system_text(lightweight_instruction_text())
}

/// Generate a specialized system instruction for advanced operations
pub fn generate_specialized_instruction() -> Content {
    Content::system_text(specialized_instruction_text())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::VTCodeConfig;
    use crate::config::types::SystemPromptMode;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_minimal_mode_selection() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Minimal;
        // Disable enhancements for base prompt size testing
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        // Minimal prompt should remain compact and deterministic without AGENTS.md injection
        assert!(
            result.len() < 2200,
            "Minimal mode should produce <2.2K chars (was {} chars)",
            result.len()
        );
        assert!(
            result.contains("VT Code") || result.contains("VT Code"),
            "Should contain VT Code identifier"
        );
    }

    #[tokio::test]
    async fn test_default_mode_selection() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;
        // Disable enhancements for base prompt size testing
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            result.len() <= 2600,
            "Default mode should stay sparse (<=2.6K chars, was {} chars)",
            result.len()
        );
        assert!(result.contains("task_tracker"));
        assert!(result.contains("@file"));
        assert!(result.contains("Plan Mode"));
    }

    #[tokio::test]
    async fn test_lightweight_mode_selection() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
        // Disable enhancements for base prompt size testing
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        // Lightweight is optimized for simple operations (v4.2)
        assert!(result.len() > 100, "Lightweight should be >100 chars");
        assert!(
            result.len() < 2100,
            "Lightweight should be compact (<2.1K chars, was {} chars)",
            result.len()
        );
        assert!(result.contains("task_tracker"));
        assert!(result.contains("@file"));
        assert!(result.contains("Plan Mode"));
    }

    #[tokio::test]
    async fn test_lightweight_mode_skips_structured_reasoning_by_default() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;
        config.agent.include_structured_reasoning_tags = None;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

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

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            result.contains("## Structured Reasoning"),
            "Lightweight mode should include structured reasoning when explicitly enabled"
        );
    }

    #[tokio::test]
    async fn test_default_mode_includes_structured_reasoning_by_default() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 0;
        config.agent.include_structured_reasoning_tags = None;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            result.contains("## Structured Reasoning"),
            "Default mode should include structured reasoning by default"
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

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            result.len() <= 2500,
            "Specialized should stay sparse (<=2.5K chars, was {} chars)",
            result.len()
        );
        assert!(result.contains("task_tracker"));
        assert!(result.contains("<proposed_plan>"));
        assert!(result.contains("/add-dir"));
    }

    #[test]
    fn test_prompt_mode_enum_parsing() {
        assert_eq!(
            SystemPromptMode::parse("minimal"),
            Some(SystemPromptMode::Minimal)
        );
        assert_eq!(
            SystemPromptMode::parse("LIGHTWEIGHT"),
            Some(SystemPromptMode::Lightweight)
        );
        assert_eq!(
            SystemPromptMode::parse("Default"),
            Some(SystemPromptMode::Default)
        );
        assert_eq!(
            SystemPromptMode::parse("specialized"),
            Some(SystemPromptMode::Specialized)
        );
        assert_eq!(SystemPromptMode::parse("invalid"), None);
    }

    #[test]
    fn test_minimal_prompt_token_count() {
        // Rough estimate: 1 token ≈ 4 characters
        let approx_tokens = minimal_system_prompt().len() / 4;
        assert!(
            approx_tokens < 380,
            "Minimal prompt should stay compact, got ~{}",
            approx_tokens
        );
    }

    #[test]
    fn test_default_prompt_token_count() {
        let approx_tokens = default_system_prompt().len() / 4;
        assert!(
            approx_tokens < 500,
            "Default prompt should stay compact, got ~{}",
            approx_tokens
        );
    }

    #[tokio::test]
    async fn test_generated_prompts_use_task_tracker_not_update_plan() {
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

            assert!(
                result.contains("task_tracker"),
                "{mode_name} prompt should reference task_tracker"
            );
            assert!(
                !result.contains("update_plan"),
                "{mode_name} prompt should not reference deprecated update_plan"
            );
        }
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

            assert!(
                !result.contains("References\n"),
                "{mode_name} prompt should not force a References section"
            );
            assert!(
                !result.contains("Next action"),
                "{mode_name} prompt should not force a Next action section"
            );
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
                normalized.contains("verify") || normalized.contains("validation"),
                "{mode_name} prompt should include verification guidance"
            );
            assert!(
                normalized.contains("do not guess"),
                "{mode_name} prompt should gate missing context"
            );
            assert!(
                normalized.contains("unblocked portion")
                    || normalized.contains("unblocked slices")
                    || normalized.contains("answerable without a missing detail"),
                "{mode_name} prompt should require partial progress before clarification"
            );
            assert!(
                normalized.contains("retrieved sources")
                    || normalized.contains("retrieved evidence"),
                "{mode_name} prompt should include grounding/citation guidance"
            );
            assert!(
                !result.contains('ƒ'),
                "{mode_name} prompt should not contain stray prompt characters"
            );
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
        assert!(
            default_system_prompt().contains("AGENTS.md"),
            "Default prompt should reference AGENTS.md as map"
        );
        assert!(
            default_system_prompt().contains("ARCHITECTURAL_INVARIANTS"),
            "Default prompt should reference architectural invariants"
        );
        assert!(
            specialized_instruction_text().contains("ARCHITECTURAL_INVARIANTS"),
            "Specialized prompt should reference architectural invariants"
        );
        assert!(
            minimal_system_prompt().contains("docs/harness/"),
            "Minimal prompt should reference harness knowledge base"
        );
    }

    #[test]
    fn test_prompts_reject_guessing_when_context_is_missing() {
        assert!(
            default_system_prompt().contains("do not guess"),
            "Default prompt should reject guessing"
        );
        assert!(
            specialized_instruction_text().contains("do not guess"),
            "Specialized prompt should reject guessing"
        );
        assert!(
            minimal_system_prompt().contains("uncertain"),
            "Minimal prompt should still mention uncertainty"
        );
    }

    #[test]
    fn test_prompts_include_compaction_preservation_contract() {
        assert!(
            default_system_prompt().contains("acceptance criteria"),
            "Default prompt should preserve acceptance criteria across compaction"
        );
        assert!(
            default_system_prompt().contains("decisions with rationale"),
            "Default prompt should preserve decision rationale across compaction"
        );
        assert!(
            minimal_system_prompt().contains("touched files"),
            "Minimal prompt should preserve touched files across compaction"
        );
    }

    #[test]
    fn test_prompts_include_gpt54_execution_contract_defaults() {
        let prompt = default_system_prompt();

        assert!(
            prompt.contains("## Execution Contract"),
            "Default prompt should include the execution contract section"
        );
        assert!(
            prompt.contains("Return exactly the requested sections or format"),
            "Default prompt should clamp output shape"
        );
        assert!(
            prompt
                .contains("Treat the task as incomplete until every requested deliverable is done"),
            "Default prompt should require completeness"
        );
        assert!(
            prompt.contains("Before finalizing, check requirements, grounding, format"),
            "Default prompt should require verification before finalizing"
        );
        assert!(
            prompt.contains("Keep user updates brief and high-signal"),
            "Default prompt should constrain progress updates"
        );
    }

    #[test]
    fn test_prompts_include_accuracy_optimization_contract() {
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let config = VTCodeConfig::default();
        let prompt = runtime.block_on(compose_system_instruction_text(
            &PathBuf::from("."),
            Some(&config),
            None,
        ));

        assert!(
            prompt.contains("## Accuracy Optimization"),
            "Runtime prompt should include the accuracy optimization section"
        );
        assert!(
            prompt.contains("Missing, stale, or proprietary knowledge means optimize context"),
            "Prompt should distinguish context failures from behavior failures"
        );
        assert!(
            prompt.contains("Treat prompting, retrieval, and fine-tuning as additive levers"),
            "Prompt should treat optimization levers as additive"
        );
        assert!(
            prompt.contains("avoid noisy long context"),
            "Prompt should warn against noisy retrieval/context"
        );
        assert!(
            prompt.contains("clarification or human review"),
            "Prompt should prefer safer fallbacks for high-stakes tasks"
        );
    }

    #[tokio::test]
    async fn test_generated_prompts_keep_mode_deltas_bounded() {
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

            assert!(
                result.contains("## Core Contract"),
                "{mode_name} prompt should reuse the canonical base prompt"
            );
            assert!(
                result.matches("## Mode").count() == 1,
                "{mode_name} prompt should add only one mode delta"
            );
        }
    }

    #[test]
    fn test_search_guidance_prefers_structural_and_rg() {
        let guidelines = generate_tool_guidelines(
            &["unified_search".to_string(), "unified_exec".to_string()],
            None,
        );
        assert!(
            guidelines.contains("`action='structural'`"),
            "Tool guidance should prefer structural search for code"
        );
        assert!(
            guidelines.contains("prefer `rg` over shell `grep`"),
            "Tool guidance should prefer ripgrep in shell"
        );
        assert!(
            guidelines.contains("git diff -- <path>"),
            "Tool guidance should keep diff guidance explicit"
        );
    }

    // ENHANCEMENT TESTS

    #[tokio::test]
    async fn test_dynamic_guidelines_read_only() {
        use crate::config::types::CapabilityLevel;

        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;

        let mut ctx = PromptContext::default();
        ctx.add_tool("unified_search".to_string());
        ctx.capability_level = Some(CapabilityLevel::FileReading);

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(
            result.contains("Mode: read-only"),
            "Should detect read-only mode when no edit/write/exec tools available"
        );
        assert!(
            result.contains("do not modify files"),
            "Should explain read-only constraints"
        );
    }

    #[tokio::test]
    async fn test_dynamic_guidelines_tool_preferences() {
        let config = VTCodeConfig::default();

        let mut ctx = PromptContext::default();
        ctx.add_tool("unified_exec".to_string());
        ctx.add_tool("unified_search".to_string());
        ctx.add_tool("unified_file".to_string());

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(
            result.contains("unified_search") || result.contains("unified_file"),
            "Should suggest canonical search/file tools"
        );
    }

    #[tokio::test]
    async fn test_live_prompt_renders_workspace_language_hints() {
        let workspace = tempfile::TempDir::new().expect("workspace tempdir");
        std::fs::create_dir_all(workspace.path().join("src")).expect("create src");
        std::fs::create_dir_all(workspace.path().join("web")).expect("create web");
        std::fs::write(workspace.path().join("src/lib.rs"), "fn alpha() {}\n").expect("write rust");
        std::fs::write(workspace.path().join("web/app.ts"), "const app = 1;\n").expect("write ts");

        let config = VTCodeConfig::default();
        let ctx = PromptContext::from_workspace_tools(workspace.path(), ["unified_search"]);
        let result =
            compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

        assert!(result.contains("## Environment"));
        assert!(result.contains("Rust, TypeScript"));
        assert!(result.contains("structural-search `lang`"));
    }

    #[tokio::test]
    async fn test_live_prompt_omits_workspace_language_hints_without_languages() {
        let workspace = tempfile::TempDir::new().expect("workspace tempdir");
        let config = VTCodeConfig::default();
        let ctx = PromptContext::from_workspace_tools(workspace.path(), ["unified_search"]);
        let result =
            compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

        assert!(!result.contains("Languages:"));
    }

    #[tokio::test]
    async fn test_live_prompt_omits_project_docs_and_user_instructions_from_base_prompt() {
        let workspace = tempfile::TempDir::new().expect("workspace tempdir");
        std::fs::write(
            workspace.path().join("AGENTS.md"),
            "- Root summary\n\nFollow the root guidance.\n",
        )
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
        std::fs::write(
            prompts_dir.join("append-system.md"),
            "Workspace prompt appendix",
        )
        .expect("append");

        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = true;

        let mut ctx = PromptContext::default();
        ctx.add_tool("unified_search".to_string());
        ctx.add_skill_metadata(SkillMetadata {
            name: "skill-creator".to_string(),
            description: "Create skills".to_string(),
            short_description: None,
            path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
            scope: SkillScope::System,
            manifest: None,
        });
        ctx.set_current_directory(workspace.path().to_path_buf());

        let result =
            compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

        assert!(result.starts_with("# Workspace system base"));
        assert!(result.contains("Workspace prompt appendix"));
        assert!(result.contains("## Active Tools"));
        assert!(result.contains("## Skills"));
        assert!(result.contains("## Environment"));

        let appendix_pos = result
            .find("Workspace prompt appendix")
            .expect("append text");
        let tools_pos = result.find("## Active Tools").expect("tools section");
        let skills_pos = result.find("## Skills").expect("skills section");
        let env_pos = result.find("## Environment").expect("environment section");

        assert!(appendix_pos < tools_pos);
        assert!(tools_pos < skills_pos);
        assert!(skills_pos < env_pos);
    }

    #[tokio::test]
    async fn test_temporal_context_inclusion() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.prompt_cache.cache_friendly_prompt_shaping = false;
        config.agent.temporal_context_use_utc = false; // Local time

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            result.contains("Time:"),
            "Should include temporal context when enabled"
        );
        let env_pos = result.find("## Environment");
        let temporal_pos = result.find("Time:");
        if let (Some(t), Some(e)) = (temporal_pos, env_pos) {
            assert!(
                t > e,
                "Temporal context should appear inside the environment section"
            );
        }
    }

    #[tokio::test]
    async fn test_temporal_context_utc_format() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.prompt_cache.cache_friendly_prompt_shaping = false;
        config.agent.temporal_context_use_utc = true; // UTC format

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            result.contains("UTC"),
            "Should indicate UTC when temporal_context_use_utc is true"
        );
        assert!(
            result.contains("T") && result.contains("Z"),
            "Should use RFC3339 format for UTC (contains T and Z)"
        );
    }

    #[tokio::test]
    async fn test_temporal_context_disabled() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = false;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            !result.contains("Time:"),
            "Should not include temporal context when disabled"
        );
    }

    #[tokio::test]
    async fn test_cache_friendly_temporal_context_stays_out_of_base_prompt() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.prompt_cache.cache_friendly_prompt_shaping = true;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            !result.contains("Time:"),
            "Stable system prompt should omit temporal context when cache-friendly shaping is enabled"
        );
    }

    #[tokio::test]
    async fn test_configuration_awareness_stays_behavior_focused() {
        let mut config = VTCodeConfig::default();
        config.security.human_in_the_loop = true;
        config.chat.ask_questions.enabled = false;
        config.mcp.enabled = true;
        config.ide_context.enabled = true;
        config.ide_context.inject_into_prompt = true;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.contains("## Environment"));
        assert!(result.contains("Interaction: approval may gate sensitive actions"));
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

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.contains("Interaction: approval reduced by config"));
    }

    #[tokio::test]
    async fn test_default_environment_omits_default_interaction_guidance() {
        let config = VTCodeConfig::default();

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            !result.contains("Interaction:"),
            "Default-on interaction guidance should stay out of the prompt"
        );
    }

    #[tokio::test]
    async fn test_working_directory_inclusion() {
        let mut config = VTCodeConfig::default();
        config.agent.include_working_directory = true;

        let mut ctx = PromptContext::default();
        ctx.set_current_directory(PathBuf::from("/tmp/test"));

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(
            result.contains("Working directory"),
            "Should include working directory label"
        );
        assert!(
            result.contains("/tmp/test"),
            "Should show actual directory path"
        );
        let wd_pos = result.find("Working directory");
        let env_pos = result.find("## Environment");
        if let (Some(w), Some(e)) = (wd_pos, env_pos) {
            assert!(
                w > e,
                "Working directory should appear inside the environment section"
            );
        }
    }

    #[tokio::test]
    async fn test_working_directory_disabled() {
        let mut config = VTCodeConfig::default();
        config.agent.include_working_directory = false;

        let mut ctx = PromptContext::default();
        ctx.set_current_directory(PathBuf::from("/tmp/test"));

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(
            !result.contains("Working directory"),
            "Should not include working directory when disabled"
        );
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
        assert!(
            result.contains("VT Code"),
            "Should contain base prompt content"
        );
        // Should not have dynamic guidelines without context
        assert!(
            !result.contains("## Active Tools"),
            "Should not have tool guidelines without prompt context"
        );
    }

    #[tokio::test]
    async fn test_all_enhancements_combined() {
        use crate::skills::model::{SkillMetadata, SkillScope};

        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.agent.include_working_directory = true;
        config.prompt_cache.cache_friendly_prompt_shaping = false;

        let mut ctx = PromptContext::default();
        ctx.add_tool("unified_file".to_string());
        ctx.add_tool("unified_search".to_string());
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

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        // Verify all enhancements present
        assert!(
            result.contains("## Active Tools"),
            "Should have dynamic guidelines"
        );
        assert!(
            result.contains("## Skills"),
            "Should have lean skills routing"
        );
        assert!(
            result.contains("## Environment"),
            "Should have environment addenda"
        );
        assert!(result.contains("Time:"), "Should have temporal context");
        assert!(
            result.contains("Working directory"),
            "Should have working directory"
        );
        assert!(result.contains("/workspace"), "Should show workspace path");

        // Verify specific guideline for this tool set
        assert!(
            result.contains("`unified_file`") && result.contains("Read before `edit`"),
            "Should have read-before-edit guideline"
        );
    }

    #[tokio::test]
    async fn test_prompt_layers_render_in_stable_order() {
        use crate::skills::model::{SkillMetadata, SkillScope};

        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.agent.include_working_directory = true;

        let mut ctx = PromptContext::default();
        ctx.add_tool("unified_search".to_string());
        ctx.add_tool("unified_exec".to_string());
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

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        let mode_pos = result.find("## Mode").expect("mode section");
        let tools_pos = result.find("## Active Tools").expect("tools section");
        let skills_pos = result.find("## Skills").expect("skills section");
        let env_pos = result.find("## Environment").expect("environment section");

        assert!(mode_pos < tools_pos, "mode should precede tools");
        assert!(tools_pos < skills_pos, "tools should precede skills");
        assert!(skills_pos < env_pos, "skills should precede environment");
    }

    #[tokio::test]
    async fn test_skills_section_stays_lean_and_routing_focused() {
        use crate::skills::model::SkillScope;
        use crate::skills::types::SkillManifest;

        let config = VTCodeConfig::default();
        let mut ctx = PromptContext::default();
        ctx.available_skill_metadata
            .push(crate::skills::model::SkillMetadata {
                name: "skill-creator".to_string(),
                description: "Create or update skills".to_string(),
                short_description: None,
                path: PathBuf::from("/tmp/skill-creator/SKILL.md"),
                scope: SkillScope::System,
                manifest: Some(SkillManifest {
                    when_to_use: Some("Use when creating or updating a skill.".to_string()),
                    when_not_to_use: Some("Avoid for unrelated implementation work.".to_string()),
                    ..SkillManifest::default()
                }),
            });

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(result.contains("## Skills"));
        assert!(result.contains("skill-creator: Create or update skills"));
        assert!(result.contains("(file: /tmp/skill-creator/SKILL.md)"));
        assert!(result.contains("- Routing: Use a skill"));
        assert!(!result.contains("Discovery: Available skills are listed"));
        assert!(!result.contains("scope: system"));
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

        assert!(
            !minimal_text.contains("__UNIFIED_TOOL_GUIDANCE__"),
            "Minimal prompt has uninterpolated placeholder"
        );
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
}
