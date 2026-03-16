//! System instructions and prompt management.
//!
//! Prompt variants share a compact tool-guidance block and optional runtime addenda.
//! Keep the base prompt contracts sparse; richer behavior comes from AGENTS.md,
//! dynamic tool guidance, skill metadata, and runtime notices.
//!
//! Prompt variants:
//! - `DEFAULT_SYSTEM_PROMPT`: general-purpose default workflow
//! - `DEFAULT_LIGHTWEIGHT_PROMPT`: smaller contract for simple work
//! - `DEFAULT_SPECIALIZED_PROMPT`: methodical contract for complex changes

use crate::config::constants::{
    instructions as instruction_constants, project_doc as project_doc_constants,
};
use crate::config::types::SystemPromptMode;
use crate::instructions::{
    InstructionBundle, read_instruction_bundle, render_instruction_markdown,
};
use crate::llm::providers::gemini::wire::Content;
use crate::project_doc::read_project_doc;
use crate::prompts::context::PromptContext;
use crate::prompts::guidelines::generate_tool_guidelines;
use crate::prompts::output_styles::OutputStyleApplier;
use crate::prompts::system_prompt_cache::PROMPT_CACHE;
use crate::prompts::temporal::generate_temporal_context;
use crate::skills::render::render_skills_section;
use dirs::home_dir;
use std::env;
use std::fmt::Write as _;
use std::path::Path;
use tracing::warn;

const COMPACT_TOOL_GUIDANCE: &str = r#"**Tools**:
- Prefer `unified_search` over repeated reads; default to `action='structural'` for code search and `action='grep'` for plain text
- For `action='structural'`, set `lang` when known and keep `pattern` parseable code, not fragments like `-> Result<$T>`
- If a structural query is a fragment, retry `unified_search` with a larger parseable pattern before switching tools or loading a skill
- `action='structural'` is syntax-aware only; keep refining `unified_search` first and use `unified_exec` only for raw `sg scan`/`sg test` or rewrite workflows
- Use `unified_file` for edits/writes and avoid redundant re-reads after successful changes
- Prefer named helpers over flattening logic for imagined call savings; trust the optimizer unless profiling shows a real hotspot
- Use `unified_exec` for shell commands; prefer `rg` over shell `grep`; stay in WORKSPACE_DIR and confirm destructive ops
- Keep iterating with tool calls until the task is complete or clearly blocked; do not stop after a single partial result
- If only part of the request is blocked by a missing symbol, path, or placeholder, complete the non-blocked portion first and then ask for the exact missing input
- Hidden capabilities route through `list_skills` and `load_skill`; check routing hints before loading a skill
- Respect runtime-configured loop guards; if a call pattern stalls or repeats, pivot instead of retrying identically"#;

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

/// DEFAULT SYSTEM PROMPT (v6.3 - sparse default runtime contract)
const DEFAULT_SYSTEM_PROMPT: &str = r#"# VT Code Coding Assistant

You are VT Code, a coding agent for VT Code. Be concise, direct, and safe.

## Workflow

- Start with the repo: read `AGENTS.md`, inspect the relevant code, and understand existing patterns before editing.
- For non-trivial work, use `task_tracker` to break the job into small verified slices.
- Use Plan Mode for research/spec work; stay read-only there until implementation intent is explicit.
- Execute changes surgically, match surrounding style, and verify before moving on.
- Report the outcome first and keep the final answer compact.

## Decision Policy

- Default to acting without asking when the next step is reversible and low-risk.
- Ask only when the choice materially changes behavior, API, UX, or requires secrets / external action.
- In Plan Mode, close material unknowns before finalizing one `<proposed_plan>`.
- If context is missing, say so plainly and do not guess, but still complete any unblocked portion of the request before asking for the missing input.

## Working Style

- Prefer simple, readable changes over clever abstractions.
- Reuse existing helpers and patterns before adding new structure.
- Follow repo rules and architectural invariants when they apply; `AGENTS.md` is the map and `docs/harness/ARCHITECTURAL_INVARIANTS.md` is the constraint set for structural work.
- If you cannot complete a task autonomously, identify the missing repo context instead of hand-waving.

## Context Shortcuts

- Users can focus work with `@file` references.
- Active editor state may arrive through IDE context; use it when present.
- Use `/add-dir` when relevant code lives outside the current workspace root.

## Validation and Safety

- Run targeted checks yourself after behavior changes and before concluding.
- Never claim something passed unless you actually ran the command.
- Respect approval gates and keep destructive or external actions explicit.
- Never print or commit secrets.
- For research or citation-sensitive work, use retrieved evidence and label inference clearly.

## Tool Guidelines

__UNIFIED_TOOL_GUIDANCE__

## Output

- Keep responses compact, grounded, and directly tied to the task.
- Lead with outcomes. Use file references when they help; do not force a rigid template.
- No emoji, no code dumps unless requested, and no filler about context limits."#;

pub fn default_system_prompt() -> &'static str {
    DEFAULT_SYSTEM_PROMPT
}

pub fn minimal_system_prompt() -> &'static str {
    MINIMAL_SYSTEM_PROMPT
}

pub fn default_lightweight_prompt() -> &'static str {
    DEFAULT_LIGHTWEIGHT_PROMPT
}

pub fn minimal_instruction_text() -> String {
    render_prompt_template(MINIMAL_SYSTEM_PROMPT, SystemPromptMode::Minimal)
}

pub fn lightweight_instruction_text() -> String {
    render_prompt_template(DEFAULT_LIGHTWEIGHT_PROMPT, SystemPromptMode::Lightweight)
}

pub fn specialized_instruction_text() -> String {
    render_prompt_template(DEFAULT_SPECIALIZED_PROMPT, SystemPromptMode::Specialized)
}

/// MINIMAL PROMPT: compact contract for capable models.
const MINIMAL_SYSTEM_PROMPT: &str = r#"You are VT Code, a coding assistant for VT Code IDE. Precise, safe, helpful.

**Principles**: Codebase-first, tool excellence, outcome focus, consistency with surrounding code, KISS, DRY, repo as system of record.
**Classic rules**: Explicit > implicit. Readability counts. Simple > complex > complicated. In ambiguity, refuse to guess. Prefer one obvious way. If hard to explain, redesign. Separate changing vs stable concerns; avoid temporal coupling (decomplect first).

**Decision policy**: Default — act without asking. Proceed with reasonable assumptions. State assumptions in one line and continue. Ask (via `request_user_input`) only when requirements materially change behavior/UX/API or credentials are needed. If `request_user_input` is unavailable, fall back to this prompt's standard decision policy and state assumptions explicitly. When genuinely uncertain, surface the ambiguity early rather than guessing.

**Execution contract**: Keep outputs compact and in the requested format. If the next step is reversible and low-risk, proceed without asking. Use tools when they improve correctness, resolve prerequisites before acting, retry empty or partial lookups with a different approach, and verify before finalizing. Treat the task as incomplete until every requested item is covered or marked blocked. If required context is missing, do not guess, but still complete any unblocked portion before asking for the missing input. For research or citation-sensitive work, base claims only on retrieved or provided evidence and cite only retrieved sources.

**Harness**: `AGENTS.md` is the map. `docs/harness/` has invariants, quality scores, exec plans, tech debt. Check invariants before modifying code. Boy scout rule: leave code better than you found it.

**Validation**: Run tests/checks yourself. Verify at least once per slice and before concluding. Never claim "tested/passed" unless you actually ran the command.

**Planning**: For non-trivial scope, use `task_tracker` — composable steps with outcome + verification each. Use Plan Mode for research/spec work and keep one active step at a time.

**Context**: Use `@file`, IDE context, and `/add-dir` to keep the right code in focus.

__UNIFIED_TOOL_GUIDANCE__

**Discover**: `list_skills` and `load_skill` to find/activate tools (hidden by default).
**Delegation**: Use focused plans and clear step handoffs inside the main conversation.

**Output**: No emoji — use plain Unicode symbols (✓, ✗, →, •, ■, ▸, —) instead. Keep replies compact, outcome-first, and grounded. Use file refs when they help. No chain-of-thought or code dumps unless requested.

**Security**: Never print/log secrets. Never commit secrets to repo. Redact if encountered.

**Git**: Never `git commit`, `git push`, or branch unless explicitly requested.

**AGENTS.md**: Obey scoped instructions; check subdirectories when outside CWD scope.

Stop when done."#;

/// LIGHTWEIGHT PROMPT: compact default for simple operations.
const DEFAULT_LIGHTWEIGHT_PROMPT: &str = r#"VT Code - efficient coding agent.

- Act and verify. Direct tone. No emoji — use plain Unicode symbols (✓, ✗, →, •).
- Keep outputs compact and exactly in the requested format; avoid repeating the user's request.
- If the next step is reversible and low-risk, proceed without asking. Ask only for irreversible, external, or outcome-changing choices.
- If part of the task is answerable without a missing detail, complete that portion before asking for the exact missing input.
- Use `task_tracker` for multi-step work and Plan Mode for research/spec work.
- Use `@file`, IDE context, and `/add-dir` to focus the relevant code.
- Scoped: unified_search (≤5), unified_file (max_tokens).
- Use `unified_exec` for shell commands; set `tty=true` only when an interactive PTY is required.
- Tools hidden by default. `list_skills --search <term>` to find them.
- Resolve prerequisites before acting, retry empty or narrow lookups with a different approach, and verify before finalizing.
- Treat tasks as incomplete until each requested item is covered or marked blocked. If required context is missing, do not guess.
- For research or citation-sensitive work, ground claims in retrieved or provided evidence and cite only retrieved sources.
- Keep investigation and implementation explicit in a single thread; summarize findings before edits.
- Keep code consistent with nearby patterns; prefer KISS and DRY.
- Prefer explicit, readable, simple code. In ambiguity, refuse to guess.
- WORKSPACE_DIR only. Confirm destructive ops.

__UNIFIED_TOOL_GUIDANCE__"#;

/// SPECIALIZED PROMPT (v6.3 - sparse methodical refactoring)
const DEFAULT_SPECIALIZED_PROMPT: &str = r#"# VT Code Specialized Agent

For complex refactors and multi-file changes, stay methodical and outcome-focused.

## Decision Policy & Execution

- Explore first, then plan, then execute.
- Use `task_tracker` for multi-step work and keep one active slice at a time.
- Use Plan Mode when you need research or scope closure; keep it read-only and end with one `<proposed_plan>`.
- Act without asking when the next step is reversible and local. Ask only for material outcome changes, secrets, or external actions.
- If only one slice is blocked by missing context, complete the unblocked slices first and ask for the exact missing input only for the blocked remainder.
- If a path stalls, re-plan into smaller slices instead of repeating the same move.

## Validation

- Verify each completed slice and the final result.
- Run targeted checks first, then broaden when the change is risky.
- Never claim success without execution evidence.
- For research or citation-sensitive work, rely on retrieved evidence and say when it is incomplete.

## Repo Context

- `AGENTS.md` is the map. Use `docs/harness/ARCHITECTURAL_INVARIANTS.md` when structural rules matter.
- Use `@file`, IDE context, and `/add-dir` to stay focused on the right code.
- Keep changes surgical unless the task explicitly calls for deeper restructuring.

## Tool Strategy

__UNIFIED_TOOL_GUIDANCE__

## Output

- Be concise and explicit about what changed and how it was verified.
- Use file references when useful, but do not force a rigid response template.
- No emoji, no filler, and do not guess when evidence is missing.
"#;

const STRUCTURED_REASONING_INSTRUCTIONS: &str = r#"
## Structured Reasoning

Use these tags when they help:
- `<analysis>` for repo facts and options
- `<plan>` for concrete steps
- `<uncertainty>` for blocking ambiguity before guessing
- `<verification>` for checks and regressions
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

/// Compose the system instruction text for the agent
///
/// ## Skills Integration Note
///
/// VT Code implements a **Tiered Disclosure** model for skills:
/// 1. **Discovery Profile**: Names and descriptions are available via `list_skills` and summarized in the system prompt.
/// 2. **Active Instructions**: Full `SKILL.md` content is loaded via `load_skill` and then persists in the incremental system prompt.
/// 3. **Deep Resources**: Level 3 assets (scripts, technical refs) are lazy-loaded via `load_skill_resource`.
///
/// This approach follows the Agent Skills spec while optimizing context usage.
///
/// # Arguments
/// * `project_root` - Root directory of the project
/// * `vtcode_config` - Configuration loaded from vtcode.toml
/// * `prompt_context` - Optional context with tool information for dynamic enhancements
pub async fn compose_system_instruction_text(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    prompt_context: Option<&PromptContext>,
) -> String {
    // OPTIMIZATION: Pre-allocate with improved capacity estimation
    // Read instruction hierarchy once upfront for accurate sizing
    let home_path = home_dir();
    let instruction_bundle = read_instruction_hierarchy(project_root, vtcode_config).await;

    // Select base prompt based on configured mode
    use crate::config::types::SystemPromptMode;
    let prompt_mode = vtcode_config
        .map(|c| c.agent.system_prompt_mode)
        .unwrap_or(SystemPromptMode::Default);
    let (base_prompt, mode_name) = match prompt_mode {
        SystemPromptMode::Minimal => (MINIMAL_SYSTEM_PROMPT, "minimal"),
        SystemPromptMode::Lightweight => (DEFAULT_LIGHTWEIGHT_PROMPT, "lightweight"),
        SystemPromptMode::Specialized => (DEFAULT_SPECIALIZED_PROMPT, "specialized"),
        SystemPromptMode::Default => (DEFAULT_SYSTEM_PROMPT, "default"),
    };
    let rendered_base_prompt = render_prompt_template(base_prompt, prompt_mode);

    tracing::debug!(
        mode = mode_name,
        base_tokens_approx = rendered_base_prompt.len() / 4, // rough token estimate
        "Selected system prompt mode"
    );

    let base_len = rendered_base_prompt.len();
    let config_overhead = vtcode_config.map_or(0, |_| 1024);
    let instruction_hierarchy_size = instruction_bundle
        .as_ref()
        .map(|b| {
            b.segments
                .iter()
                .map(|s| s.contents.len() + 200)
                .sum::<usize>()
        })
        .unwrap_or(0);

    let estimated_capacity = base_len + config_overhead + instruction_hierarchy_size + 1024; // +512 for enhancements
    let mut instruction = String::with_capacity(estimated_capacity);
    instruction.push_str(&rendered_base_prompt);
    if should_include_structured_reasoning(vtcode_config, prompt_mode) {
        instruction.push_str("\n\n");
        instruction.push_str(STRUCTURED_REASONING_INSTRUCTIONS);
    }

    // ENHANCEMENT 1: Dynamic tool-aware guidelines (behavioral - goes early)
    if let Some(ctx) = prompt_context {
        let guidelines = generate_tool_guidelines(&ctx.available_tools, ctx.capability_level);
        if !guidelines.is_empty() {
            instruction.push_str(&guidelines);
        }
        if let Some(language_hints) = render_workspace_language_hints(&ctx.languages) {
            instruction.push_str("\n\n");
            instruction.push_str(&language_hints);
        }
        if let Some(skills_section) = render_skills_section(&ctx.available_skill_metadata) {
            instruction.push_str("\n\n");
            instruction.push_str(&skills_section);
        }
    }

    if let Some(cfg) = vtcode_config {
        instruction.push_str("\n\n## CONFIGURATION AWARENESS\n");
        instruction.push_str("Only the active behavior-relevant policies are listed here.\n\n");

        if cfg.security.human_in_the_loop {
            instruction.push_str("- Sensitive actions may require approval.\n");
        } else {
            instruction.push_str(
                "- Approval prompts are reduced by config; still treat destructive or external actions explicitly.\n",
            );
        }

        if cfg.chat.ask_questions.enabled {
            instruction
                .push_str("- `request_user_input` is available for material blocking questions.\n");
        } else {
            instruction.push_str("- `request_user_input` is disabled in Edit mode; make reasonable assumptions unless Plan Mode requires follow-up.\n");
        }

        if cfg.ide_context.enabled && cfg.ide_context.inject_into_prompt {
            instruction.push_str(
                "- IDE context can inject the active editor selection and file focus into the prompt.\n",
            );
        }

        if cfg.mcp.enabled {
            instruction.push_str(
                "- MCP context sources are enabled; prefer them before external fetches when they can answer the question.\n",
            );
        }
    }

    if !prompt_context
        .map(|ctx| ctx.skip_standard_instructions)
        .unwrap_or(false)
        && let Some(cfg) = vtcode_config
        && let Some(user_inst) = &cfg.agent.user_instructions
    {
        instruction.push_str("\n\n## USER INSTRUCTIONS\n");
        instruction.push_str(user_inst);
    }

    if let Some(bundle) = instruction_bundle {
        let home_ref = home_path.as_deref();
        instruction.push_str("\n\n");
        instruction.push_str(&render_instruction_markdown(
            "AGENTS.MD INSTRUCTION HIERARCHY",
            &bundle.segments,
            bundle.truncated,
            project_root,
            home_ref,
            3,
            "instruction content was truncated due to size limits. Review the source files for full details.",
        ));
    }

    // ENHANCEMENT 2: Temporal context (metadata - goes at end)
    if let Some(cfg) = vtcode_config
        && cfg.agent.include_temporal_context
    {
        let temporal = generate_temporal_context(cfg.agent.temporal_context_use_utc);
        instruction.push_str(&temporal);
    }

    // ENHANCEMENT 3: Working directory context (metadata - goes at end)
    if let Some(cfg) = vtcode_config
        && cfg.agent.include_working_directory
        && let Some(ctx) = prompt_context
        && let Some(cwd) = &ctx.current_directory
    {
        let _ = write!(
            instruction,
            "\n\nCurrent working directory: {}",
            cwd.display()
        );
    }

    instruction
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

/// Generate system instruction with configuration and AGENTS.md guidelines incorporated
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

/// Generate system instruction with AGENTS.md guidelines incorporated
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

async fn read_instruction_hierarchy(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> Option<InstructionBundle> {
    let (max_bytes, extra_sources, fallback_filenames) = match vtcode_config {
        Some(cfg) => (
            cfg.agent.instruction_max_bytes,
            cfg.agent.instruction_files.clone(),
            cfg.agent.project_doc_fallback_filenames.clone(),
        ),
        None => (
            instruction_constants::DEFAULT_MAX_BYTES,
            Vec::new(),
            Vec::new(),
        ),
    };

    if max_bytes == 0 {
        return None;
    }

    let current_dir = env::current_dir().unwrap_or_else(|_| project_root.to_path_buf());
    let home = home_dir();
    match read_instruction_bundle(
        &current_dir,
        project_root,
        home.as_deref(),
        &extra_sources,
        &fallback_filenames,
        max_bytes,
    )
    .await
    {
        Ok(Some(bundle)) => Some(bundle),
        Ok(None) => None,
        Err(err) => {
            warn!("failed to load instruction hierarchy: {err:#}");
            None
        }
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
        cfg.agent.instruction_max_bytes.hash(&mut hasher);
        cfg.agent.instruction_files.hash(&mut hasher);
        cfg.agent.include_working_directory.hash(&mut hasher);
        cfg.agent.include_temporal_context.hash(&mut hasher);
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

fn render_prompt_template(prompt_template: &str, prompt_mode: SystemPromptMode) -> String {
    prompt_template.replace(
        "__UNIFIED_TOOL_GUIDANCE__",
        tool_guidance_for_mode(prompt_mode),
    )
}

fn tool_guidance_for_mode(_prompt_mode: SystemPromptMode) -> &'static str {
    COMPACT_TOOL_GUIDANCE
}

fn render_workspace_language_hints(languages: &[String]) -> Option<String> {
    if languages.is_empty() {
        return None;
    }

    Some(format!(
        "## Workspace Language Hints\n- Detected workspace languages: {}\n- Prefer matching structural-search `lang` values when working in these files; omitting `lang` is only safe when `path` or positive `globs` make the language unambiguous.",
        languages.join(", ")
    ))
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
            result.len() < 6000,
            "Minimal mode should produce <6.0K chars (was {} chars)",
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
            result.len() <= 6500,
            "Default mode should stay sparse (<=6.5K chars, was {} chars)",
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
            result.len() < 4400,
            "Lightweight should be compact (<4.4K chars, was {} chars)",
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
            result.len() <= 4000,
            "Specialized should stay sparse (<=4.0K chars, was {} chars)",
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
        let approx_tokens = MINIMAL_SYSTEM_PROMPT.len() / 4;
        assert!(
            approx_tokens < 1000,
            "Minimal prompt should be <1K tokens, got ~{}",
            approx_tokens
        );
    }

    #[test]
    fn test_default_prompt_token_count() {
        let approx_tokens = DEFAULT_SYSTEM_PROMPT.len() / 4;
        assert!(
            approx_tokens < 1300,
            "Default prompt should stay under ~1.3K tokens, got ~{}",
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
        let specialized_prompt =
            DEFAULT_SPECIALIZED_PROMPT.replace("__UNIFIED_TOOL_GUIDANCE__", COMPACT_TOOL_GUIDANCE);
        assert!(!DEFAULT_SYSTEM_PROMPT.contains("stuck twice"));
        assert!(!MINIMAL_SYSTEM_PROMPT.contains("stuck twice"));
        assert!(!specialized_prompt.contains("stuck twice"));
        assert!(!specialized_prompt.contains("10+ calls without progress"));
        assert!(!specialized_prompt.contains("Same tool+params twice"));
        assert!(specialized_prompt.contains("runtime-configured"));
    }

    #[test]
    fn test_harness_awareness_in_prompts() {
        assert!(
            DEFAULT_SYSTEM_PROMPT.contains("AGENTS.md"),
            "Default prompt should reference AGENTS.md as map"
        );
        assert!(
            DEFAULT_SYSTEM_PROMPT.contains("ARCHITECTURAL_INVARIANTS"),
            "Default prompt should reference architectural invariants"
        );
        assert!(
            DEFAULT_SPECIALIZED_PROMPT.contains("ARCHITECTURAL_INVARIANTS"),
            "Specialized prompt should reference architectural invariants"
        );
        assert!(
            MINIMAL_SYSTEM_PROMPT.contains("docs/harness/"),
            "Minimal prompt should reference harness knowledge base"
        );
    }

    #[test]
    fn test_prompts_reject_guessing_when_context_is_missing() {
        assert!(
            DEFAULT_SYSTEM_PROMPT.contains("do not guess"),
            "Default prompt should reject guessing"
        );
        assert!(
            DEFAULT_SPECIALIZED_PROMPT.contains("do not guess"),
            "Specialized prompt should reject guessing"
        );
        assert!(
            MINIMAL_SYSTEM_PROMPT.contains("uncertain"),
            "Minimal prompt should still mention uncertainty"
        );
    }

    #[tokio::test]
    async fn test_generated_prompts_prefer_named_extractions_over_imagined_indirection_savings() {
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
                normalized.contains("named helpers over flattening logic"),
                "{mode_name} prompt should prefer named helpers over flattening logic"
            );
            assert!(
                normalized.contains("trust the optimizer"),
                "{mode_name} prompt should trust the optimizer before flattening code"
            );
            assert!(
                normalized.contains("real hotspot"),
                "{mode_name} prompt should require profiling evidence for indirection tradeoffs"
            );
        }
    }

    #[test]
    fn test_search_guidance_prefers_structural_and_rg() {
        assert!(
            COMPACT_TOOL_GUIDANCE.contains("default to `action='structural'` for code search"),
            "Tool guidance should prefer structural search for code"
        );
        assert!(
            COMPACT_TOOL_GUIDANCE.contains("parseable code"),
            "Tool guidance should warn that structural patterns must be parseable"
        );
        assert!(
            COMPACT_TOOL_GUIDANCE.contains("prefer `rg` over shell `grep`"),
            "Tool guidance should prefer ripgrep in shell"
        );
        assert!(
            COMPACT_TOOL_GUIDANCE.contains("load_skill"),
            "Tool guidance should route hidden capabilities through skills"
        );
        assert!(
            !COMPACT_TOOL_GUIDANCE.contains(">8KB"),
            "Tool guidance should avoid stale hardcoded spool thresholds"
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
            result.contains("READ-ONLY MODE"),
            "Should detect read-only mode when no edit/write/exec tools available"
        );
        assert!(
            result.contains("cannot modify files"),
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

        assert!(result.contains("## Workspace Language Hints"));
        assert!(result.contains("Rust, TypeScript"));
        assert!(result.contains("positive `globs`"));
    }

    #[tokio::test]
    async fn test_live_prompt_omits_workspace_language_hints_without_languages() {
        let workspace = tempfile::TempDir::new().expect("workspace tempdir");
        let config = VTCodeConfig::default();
        let ctx = PromptContext::from_workspace_tools(workspace.path(), ["unified_search"]);
        let result =
            compose_system_instruction_text(workspace.path(), Some(&config), Some(&ctx)).await;

        assert!(!result.contains("## Workspace Language Hints"));
    }

    #[tokio::test]
    async fn test_live_prompt_renders_instruction_map_and_key_points() {
        let workspace = tempfile::TempDir::new().expect("workspace tempdir");
        std::fs::write(
            workspace.path().join("AGENTS.md"),
            "- Root summary\n\nFollow the root guidance.\n",
        )
        .expect("write agents");

        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;
        config.agent.instruction_max_bytes = 4096;

        let result = compose_system_instruction_text(workspace.path(), Some(&config), None).await;

        assert!(result.contains("## AGENTS.MD INSTRUCTION HIERARCHY"));
        assert!(result.contains("### Instruction map"));
        assert!(result.contains("### Key points"));
        assert!(result.contains("AGENTS.md (workspace)"));
        assert!(result.contains("Root summary"));
    }

    #[tokio::test]
    async fn test_temporal_context_inclusion() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.agent.temporal_context_use_utc = false; // Local time

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(
            result.contains("Current date and time:"),
            "Should include temporal context when enabled"
        );
        // Should appear at the end (after AGENTS.MD would be)
        let temporal_pos = result.find("Current date and time:");
        let config_pos = result.find("CONFIGURATION AWARENESS");
        if let (Some(t), Some(c)) = (temporal_pos, config_pos) {
            assert!(
                t > c,
                "Temporal context should come after configuration section"
            );
        }
    }

    #[tokio::test]
    async fn test_temporal_context_utc_format() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
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
            !result.contains("Current date and time"),
            "Should not include temporal context when disabled"
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

        assert!(result.contains("CONFIGURATION AWARENESS"));
        assert!(result.contains("Sensitive actions may require approval"));
        assert!(result.contains("request_user_input"));
        assert!(result.contains("IDE context can inject"));
        assert!(result.contains("MCP context sources are enabled"));
        assert!(!result.contains("PTY functionality"));
        assert!(!result.contains("Loop guards"));
        assert!(!result.contains(".vtcode/context/tool_outputs/"));
    }

    #[tokio::test]
    async fn test_configuration_awareness_mentions_reduced_approval_when_disabled() {
        let mut config = VTCodeConfig::default();
        config.security.human_in_the_loop = false;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        assert!(result.contains("Approval prompts are reduced by config"));
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
            result.contains("Current working directory"),
            "Should include working directory label"
        );
        assert!(
            result.contains("/tmp/test"),
            "Should show actual directory path"
        );
        // Should appear at the end
        let wd_pos = result.find("Current working directory");
        let config_pos = result.find("CONFIGURATION AWARENESS");
        if let (Some(w), Some(c)) = (wd_pos, config_pos) {
            assert!(
                w > c,
                "Working directory should come after configuration section"
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
            !result.contains("Current working directory"),
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
        assert!(result.len() > 1000, "Should generate substantial prompt");
        assert!(
            result.contains("VT Code"),
            "Should contain base prompt content"
        );
        // Should not have dynamic guidelines without context
        assert!(
            !result.contains("TOOL USAGE GUIDELINES"),
            "Should not have tool guidelines without prompt context"
        );
    }

    #[tokio::test]
    async fn test_all_enhancements_combined() {
        let mut config = VTCodeConfig::default();
        config.agent.include_temporal_context = true;
        config.agent.include_working_directory = true;

        let mut ctx = PromptContext::default();
        ctx.add_tool("unified_file".to_string());
        ctx.add_tool("unified_search".to_string());
        ctx.infer_capability_level();
        ctx.set_current_directory(PathBuf::from("/workspace"));

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        // Verify all enhancements present
        assert!(
            result.contains("TOOL USAGE GUIDELINES"),
            "Should have dynamic guidelines"
        );
        assert!(
            result.contains("Current date and time"),
            "Should have temporal context"
        );
        assert!(
            result.contains("Current working directory"),
            "Should have working directory"
        );
        assert!(result.contains("/workspace"), "Should show workspace path");

        // Verify specific guideline for this tool set
        assert!(
            result.contains("unified_file") && result.contains("before"),
            "Should have read-before-edit guideline"
        );
    }

    #[tokio::test]
    async fn test_skills_section_renders_scope_and_routing_hints() {
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
        assert!(result.contains("scope: system"));
        assert!(result.contains("use: Use when creating or updating a skill."));
        assert!(result.contains("avoid: Avoid for unrelated implementation work."));
    }

    #[test]
    fn test_no_uninterpolated_placeholders() {
        let _minimal = generate_minimal_instruction();
        let _lightweight = generate_lightweight_instruction();
        let _specialized = generate_specialized_instruction();

        let minimal_text = render_prompt_template(MINIMAL_SYSTEM_PROMPT, SystemPromptMode::Minimal);
        let lightweight_text =
            render_prompt_template(DEFAULT_LIGHTWEIGHT_PROMPT, SystemPromptMode::Lightweight);
        let specialized_text =
            render_prompt_template(DEFAULT_SPECIALIZED_PROMPT, SystemPromptMode::Specialized);

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
            !render_prompt_template(DEFAULT_SYSTEM_PROMPT, SystemPromptMode::Default)
                .contains("__UNIFIED_TOOL_GUIDANCE__"),
            "Default prompt has uninterpolated placeholder"
        );
    }
}
