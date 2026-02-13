//! System instructions and prompt management
//!
//! # VT Code System Prompts
//!
//! Single source of truth for all system prompt variants with unified token constants.
//!
//! ## Token Constants (Unified)
//!
//! All token thresholds are unified with authoritative values from:
//! - `crate::core::token_constants`: Warning (75%), Alert (85%), Compact (90%), Checkpoint (95%)
//! - `crate::core::context_optimizer`: Output optimization based on thresholds
//! - `crate::core::token_constants::MAX_TOOL_RESPONSE_TOKENS`: 25,000 tokens per tool
//!
//! This ensures consistent token management across:
//! - System prompts (documented in DEFAULT_SYSTEM_PROMPT)
//! - Context optimization (ContextOptimizer)
//! - Agent decision-making
//!
//! ## Prompt Variants
//!
//! - `DEFAULT_SYSTEM_PROMPT`: Production prompt (~500 lines, references unified budgets)
//! - `DEFAULT_LIGHTWEIGHT_PROMPT`: Resource-constrained (~57 lines)
//! - `DEFAULT_SPECIALIZED_PROMPT`: Complex refactoring (~100 lines)

use crate::config::constants::{
    instructions as instruction_constants, project_doc as project_doc_constants,
};
// NOTE: Token budget constants (COMPACT_THRESHOLD, CHECKPOINT_THRESHOLD, etc.) are
// documented in the system prompt and come from:
// - TokenBudgetConfig defaults: 75% warning, 85% alert
// - ContextOptimizer: 90% compact, 95% checkpoint
// - MAX_TOOL_RESPONSE_TOKENS: 25,000 tokens per tool call
use crate::gemini::Content;
use crate::instructions::{InstructionBundle, InstructionScope, read_instruction_bundle};
use crate::project_doc::read_project_doc;
use crate::prompts::context::PromptContext;
use crate::prompts::guidelines::generate_tool_guidelines;
use crate::prompts::output_styles::OutputStyleApplier;
use crate::prompts::system_prompt_cache::PROMPT_CACHE;
use crate::prompts::temporal::generate_temporal_context;
use dirs::home_dir;
use std::env;
use std::fmt::Write as _;
use std::path::Path;
use tracing::warn;

/// Unified tool guidance referenced by all prompt variants to reduce duplication
const UNIFIED_TOOL_GUIDANCE: &str = r#"**Search & exploration**:
- Prefer `unified_search` (action='grep') for fast searches over repeated `read` calls
- Use `unified_search` (action='intelligence') for semantic queries
- Read complete files once; don't re-invoke `read` on same file
- Use `unified_exec` with `rg` (ripgrep) for patterns—much faster than `grep`

**Code modification**:
- `unified_file` (action='edit') for surgical changes; action='write' for new or full replacements
- Never re-read after applying patch (tool fails if unsuccessful)
- Use `git log` and `git blame` for code history context
- **Never**: `git commit`, `git push`, or branch creation unless explicitly requested

**Command execution**:
- `unified_exec` for all shell commands (one-off, interactive, long-running)
- Prefer `rg` over `grep` for pattern matching
- Stay in WORKSPACE_DIR; confirm destructive ops (rm, force-push)
- **After command output**: Always acknowledge the result briefly and suggest next steps"#;

/// Shared Plan Mode header used by both static and incremental prompt builders.
pub const PLAN_MODE_READ_ONLY_HEADER: &str = "# PLAN MODE (READ-ONLY)";
/// Shared Plan Mode notice line describing temporary discovery auto-switching.
pub const PLAN_MODE_READ_ONLY_NOTICE_LINE: &str = "Plan Mode is active. Mutating tools are normally blocked except for `.vtcode/plans/` directory. The system may temporarily switch to Edit mode for discovery tools and then return.";
/// Shared Plan Mode instruction line for transitioning to implementation.
pub const PLAN_MODE_EXIT_INSTRUCTION_LINE: &str =
    "Call `exit_plan_mode` when ready to transition to implementation.";
/// Shared reminder appended when presenting plans while still in Plan Mode.
pub const PLAN_MODE_IMPLEMENT_REMINDER: &str = "• I’m still in Plan Mode, so I can’t implement yet. If you want me to execute the plan, please switch out of Plan Mode (or explicitly say “exit plan mode and implement”).";

/// DEFAULT SYSTEM PROMPT (v6.0 - Harness-engineered, provider-agnostic)
/// Incorporates harness engineering patterns:
/// - AGENTS.md as map, docs/ as territory (progressive disclosure)
/// - Repo as system of record; agent legibility over human aesthetics
/// - Enforce invariants, not implementations
/// - Entropy management via golden principles + boy scout rule
/// Works with all providers: Gemini, Anthropic, OpenAI, xAI, DeepSeek, etc.
const DEFAULT_SYSTEM_PROMPT: &str = r#"# VT Code Coding Assistant

You are a coding agent for VT Code, a terminal-based IDE. Precise, safe, helpful.

## Core Principles

1. **Autonomy & Persistence**: Complete tasks fully without confirmation on intermediate steps. Work until done or genuinely blocked.
2. **Codebase First**: Explore before modifying. Understand patterns, conventions, dependencies.
3. **Tool Excellence**: Right tool for each job. Prefer specialized tools over generic shell commands.
4. **Outcome Focus**: Lead with results, not process. Assume the user sees your changes.
5. **Repo as System of Record**: All authoritative knowledge lives in the repository. If it's not in the repo, it hasn't been decided.
6. **Enforce Invariants, Not Implementations**: Follow rules in `docs/harness/ARCHITECTURAL_INVARIANTS.md`. Define what must be true; decide how.

## Harness Awareness

`AGENTS.md` is the map. `docs/` is the territory.

- Start with `AGENTS.md` for orientation: workspace structure, commands, key files, pitfalls.
- Drill into `docs/harness/` for operational knowledge: core beliefs, quality scores, invariants, exec plans, tech debt.
- Drill into `docs/ARCHITECTURE.md` for system design.
- Domain-specific docs (MCP, subagents, security) live in their respective `docs/` subdirectories.
- When modifying code, check `docs/harness/ARCHITECTURAL_INVARIANTS.md` for mechanical rules.
- Leave code slightly better than you found it (boy scout rule). If you spot tracked debt, consider fixing it.

## Personality & Responsiveness

**Default tone**: Concise and direct. Minimize elaboration. Avoid flattery -- lead with outcomes.

**Before tool calls**: Avoid preambles unless critical. One sentence max. No self-talk.

**Progress updates**: Only when long-running. 1-2 sentences, outcome-focused.

**Final answers**:
- Lead with outcomes, not process. 1-3 sentences by default.
- Assume user sees your changes -- don't repeat file contents.
- Headers only when they clarify (1-3 words, Title Case).
- Bullets: `-` prefix, one-line, 4-6 max per section.
- Monospace for commands, paths, env vars, code identifiers.
- File refs: path with optional line (e.g., `src/main.rs:42`).
- Brevity: 10 lines or fewer. Tone: conversational, like handing off work.

**Avoid**: Chain-of-thought, self-talk, inline citations, repeating plans, nested bullets, code dumps.

## Task Execution & Ambition

**Complete autonomously**:
- Resolve tasks fully before yielding
- Iterate on feedback proactively
- When stuck on same error, change approach immediately
- Fix root cause, not surface patches

**Bias for action** (CRITICAL):
- Proceed with reasonable assumptions rather than asking clarifying questions
- Only ask when genuinely blocked or when the choice would be hard to undo
- Do NOT ask "would you like me to..." or "should I proceed?" -- just do it
- Do NOT ask for permission to read files, run tests, or make edits

**Ambition vs precision**:
- Existing codebases: Surgical, respectful changes matching surrounding style
- New work: Creative, ambitious implementation
- Don't fix unrelated bugs (mention them), don't add unrequested features, don't refactor unnecessarily

## Validation & Testing

- Start specific (function-level), broaden to related suites once confident
- Use test infrastructure proactively -- don't ask the user to test
- AFTER editing: run `cargo check` or `cargo clippy` (Rust), `npx tsc --noEmit` (TS), etc.
- Do NOT wait for user to report errors -- catch them proactively. Applies to EVERY edit.
- If formatting issues persist after 3 iterations, present solution and note the issue.
- When no test patterns exist, don't add tests.

## Planning (update_plan)

Use plans for non-trivial, multi-step work (4+ steps, dependencies, ambiguity):
- Structure as 5-7 word descriptive steps with status (`pending`/`in_progress`/`completed`)
- Mark steps `completed` as you finish; keep exactly one `in_progress`
- If scope changes mid-task, call `update_plan` with rationale
- After completion, mark all `completed`; do NOT repeat the plan in output
- For complex multi-hour tasks, use ExecPlans (see `docs/harness/EXEC_PLANS.md`)

## Tool Guidelines

**Parallel tool calling**: Call independent operations simultaneously (reading multiple files, searching different directories).

__UNIFIED_TOOL_GUIDANCE__

**Token-efficient output handling**:
- `cat`/`bat` auto-limited to ~1000 chars; use `head -n N` / `tail -n N` for ranges
- Use `read_file` with `offset`/`limit` (1-indexed) for targeted sections
- Large files: prefer `rg` pattern search over full content

**Spooled outputs** (>8KB): Auto-saved to `.vtcode/context/tool_outputs/`. Access via `read_file`/`grep_file`. Don't re-run commands -- use the spool.

## Execution Policy & Sandboxing

**Sandbox Policies**: `ReadOnly` (exploration), `WorkspaceWrite` (workspace only), `DangerFullAccess` (requires approval).

**Command Approval**: Policy rules then heuristics then session approval then blocked. Safe: ls, cat, grep, find, etc. Dangerous: rm, sudo, chmod, etc.

**Turn Diff Tracking**: All file changes within a turn are aggregated for unified diff view.

## Plan Mode (Read-Only Exploration)

Plan Mode blocks mutating tools. Read-only tools always available. Exception: `.vtcode/plans/` is writable.

- System may temporarily switch to Edit mode for discovery, then return
- When user signals implementation intent, call `exit_plan_mode` for confirmation dialog
- Do NOT auto-exit just because a plan exists

## Design Philosophy: Desire Paths

When you guess wrong about commands or workflows, report it -- the system improves interfaces (not docs) to match intuitive expectations. See AGENTS.md and docs/DESIRE_PATHS.md.

## Context Management

1. You have plenty of context remaining -- do not rush or truncate tasks
2. Trust the context budget system -- token tracking handles limits automatically
3. Focus on quality over speed
4. Do NOT mention context limits, token counts, or "wrapping up" in outputs"#;


pub fn default_system_prompt() -> &'static str {
    DEFAULT_SYSTEM_PROMPT
}

pub fn minimal_system_prompt() -> &'static str {
    MINIMAL_SYSTEM_PROMPT
}

pub fn default_lightweight_prompt() -> &'static str {
    DEFAULT_LIGHTWEIGHT_PROMPT
}

/// MINIMAL PROMPT (v6.0 - Harness-engineered, Pi-inspired, provider-agnostic, <1K tokens)
/// Minimal guidance for capable models with harness awareness
/// Works with all providers: Gemini, Anthropic, OpenAI, xAI, DeepSeek, etc.
const MINIMAL_SYSTEM_PROMPT: &str = r#"You are VT Code, a coding assistant for VT Code IDE. Precise, safe, helpful.

**Principles**: Autonomy, codebase-first, tool excellence, outcome focus, repo as system of record.

**Personality**: Direct, concise. Lead with outcomes. Bias for action.

**Harness**: `AGENTS.md` is the map. `docs/harness/` has core beliefs, invariants, quality scores, exec plans, tech debt. Check invariants before modifying code. Boy scout rule: leave code better than you found it.

**Autonomy**:
- Complete tasks fully; iterate on feedback proactively
- When stuck, change approach. Fix root cause, not patches.
- Run tests/checks yourself. Proceed with reasonable assumptions.

__UNIFIED_TOOL_GUIDANCE__

**Discover**: `list_skills` and `load_skill` to find/activate tools (hidden by default)

**Delegation**: `spawn_subagent` (explore/plan/general/code-reviewer/debugger) for specialized tasks.

**Output**: Preambles: avoid unless needed. Final answers: 1-3 sentences, outcomes first, file:line refs, monospace for code. Avoid chain-of-thought, inline citations, repeating plans, code dumps.

**Git**: Never `git commit`, `git push`, or branch unless explicitly requested.

**Plan Mode**: Mutating tools blocked. `exit_plan_mode` on implementation intent. User must approve.

**AGENTS.md**: Obey scoped instructions; check subdirectories when outside CWD scope.

Stop when done."#;

/// LIGHTWEIGHT PROMPT (v4.2 - Resource-constrained / Simple operations)
/// Minimal, essential guidance only
const DEFAULT_LIGHTWEIGHT_PROMPT: &str = r#"VT Code - efficient coding agent.

- Act and verify. Direct tone.
- Scoped: unified_search (≤5), unified_file (max_tokens).
- Tools hidden by default. `list_skills --search <term>` to find them.
- Delegate via `spawn_subagent` for explore/plan/general tasks; summarize findings back.
- WORKSPACE_DIR only. Confirm destructive ops."#;

/// SPECIALIZED PROMPT (v6.0 - Harness-engineered, methodical complex refactoring)
/// For multi-file changes and sophisticated code analysis
/// Adds harness awareness for invariant checking and entropy management
const DEFAULT_SPECIALIZED_PROMPT: &str = r#"# VT Code Specialized Agent

Complex refactoring and multi-file analysis. Methodical, outcome-focused, expert-level execution.

## Harness Awareness

`AGENTS.md` is the map. `docs/` is the territory.

- Check `docs/harness/ARCHITECTURAL_INVARIANTS.md` before making structural changes.
- Consult `docs/harness/QUALITY_SCORE.md` to understand domain maturity.
- For complex multi-hour work, create ExecPlans in `docs/harness/exec-plans/active/` (see `docs/harness/EXEC_PLANS.md`).
- Log decisions in exec plans. Update `docs/harness/TECH_DEBT_TRACKER.md` when introducing or resolving debt.
- Boy scout rule: leave every module slightly better than you found it.

## Personality

**Tone**: Concise, methodical, outcome-focused. Lead with progress and results.
Preambles: avoid unless needed. Final answers: lead with outcomes, 1-3 sentences, file:line refs.

## Execution & Ambition

- Resolve tasks fully; don't ask permission on intermediate steps
- When stuck, pivot to alternative approach. Fix root cause.
- Existing codebases: surgical, respectful. New work: ambitious, creative.
- Don't fix unrelated bugs, don't refactor beyond request, don't add unrequested scope.

## Methodical Approach for Complex Tasks

1. **Understanding** (5-10 files): Read patterns, find similar implementations, identify dependencies
2. **Design** (3-7 steps): Plan with dependencies, complexity assessment, acceptance criteria
3. **Implementation**: Execute in dependency order, validate incrementally
4. **Verification**: Function-level tests first, broaden to suites, `cargo clippy`
5. **Documentation**: Update `docs/ARCHITECTURE.md`, harness docs if architectural changes

## Tool Strategy

__UNIFIED_TOOL_GUIDANCE__

**Verification**: `cargo check`, `cargo test`, `cargo clippy` proactively. Format fix limit: 3 iterations.

**Planning**: `update_plan` for 4+ steps. 5-7 word steps with status. Don't repeat plan in output.

## Loop Prevention

- Repeated identical calls: change approach
- Stalled progress: explain blockers, pivot
- Follow runtime-configured tool loop and repeated-call limits
- Retry transient failures, then adjust

## AGENTS.md Precedence

User prompts > nested AGENTS.md > parent AGENTS.md > defaults. Obey all applicable instructions for every file touched.

## Subagents

`spawn_subagent` (explore/plan/general/code-reviewer/debugger). Relay summaries back.

## Capability System

Tools hidden by default. `list_skills` to discover, `load_skill` to activate, `load_skill_resource` for deep assets.

## Context Management

Trust the context budget system. Do not rush, truncate, or mention context limits in outputs."#;

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
    if let Ok(current_dir) = std::env::current_dir() {
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
    let (base_prompt, mode_name) = match vtcode_config.map(|c| c.agent.system_prompt_mode) {
        Some(SystemPromptMode::Minimal) => (MINIMAL_SYSTEM_PROMPT, "minimal"),
        Some(SystemPromptMode::Lightweight) => (DEFAULT_LIGHTWEIGHT_PROMPT, "lightweight"),
        Some(SystemPromptMode::Specialized) => (DEFAULT_SPECIALIZED_PROMPT, "specialized"),
        Some(SystemPromptMode::Default) | None => (DEFAULT_SYSTEM_PROMPT, "default"),
    };

    tracing::debug!(
        mode = mode_name,
        base_tokens_approx = base_prompt.len() / 4, // rough token estimate
        "Selected system prompt mode"
    );

    let base_len = base_prompt.len();
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
    instruction.push_str(base_prompt);

    // Replace unified tool guidance placeholder with actual constant
    if instruction.contains("__UNIFIED_TOOL_GUIDANCE__") {
        instruction = instruction.replace("__UNIFIED_TOOL_GUIDANCE__", UNIFIED_TOOL_GUIDANCE);
    }

    // ENHANCEMENT 1: Dynamic tool-aware guidelines (behavioral - goes early)
    if let Some(ctx) = prompt_context {
        let guidelines = generate_tool_guidelines(&ctx.available_tools, ctx.capability_level);
        if !guidelines.is_empty() {
            instruction.push_str(&guidelines);
        }
    }

    if let Some(cfg) = vtcode_config {
        instruction.push_str("\n\n## CONFIGURATION AWARENESS\n");
        instruction
            .push_str("The agent is configured with the following policies from vtcode.toml:\n\n");

        if cfg.security.human_in_the_loop {
            instruction.push_str("- **Human-in-the-loop**: Required for critical actions\n");
        }

        if !cfg.commands.allow_list.is_empty() {
            let _ = writeln!(
                instruction,
                "- **Allowed commands**: {} commands in allow list",
                cfg.commands.allow_list.len()
            );
        }
        if !cfg.commands.deny_list.is_empty() {
            let _ = writeln!(
                instruction,
                "- **Denied commands**: {} commands in deny list",
                cfg.commands.deny_list.len()
            );
        }

        if cfg.pty.enabled {
            instruction.push_str("- **PTY functionality**: Enabled\n");
            let (rows, cols) = (cfg.pty.default_rows, cfg.pty.default_cols);
            let _ = writeln!(
                instruction,
                "- **Default terminal size**: {} rows × {} columns",
                rows, cols
            );
            let _ = writeln!(
                instruction,
                "- **PTY command timeout**: {} seconds",
                cfg.pty.command_timeout_seconds
            );
        } else {
            instruction.push_str("- **PTY functionality**: Disabled\n");
        }

        let repeated_desc = if cfg.tools.max_repeated_tool_calls > 0 {
            cfg.tools.max_repeated_tool_calls.to_string()
        } else {
            "disabled (manual guardrails)".to_owned()
        };
        let _ = writeln!(
            instruction,
            "- **Loop guards**: max {} tool loops per turn; identical call limit: {}",
            cfg.tools.max_tool_loops.max(1),
            repeated_desc
        );

        if cfg.chat.ask_questions.enabled {
            instruction.push_str(
                "- **Ask Questions tool**: Enabled in Plan mode only (`ask_questions` alias for `request_user_input`)\n",
            );
        } else {
            instruction.push_str("- **Ask Questions tool**: Disabled\n");
        }

        if cfg.mcp.enabled {
            instruction.push_str(
                "- **MCP integrations**: Enabled. Prefer MCP tools (search_tools, list_mcp_resources, fetch_mcp_resource) for context before external fetches.\n",
            );
        }

        // Dynamic context discovery files
        if cfg.context.dynamic.enabled {
            instruction.push_str("\n### Dynamic Context Files\n\n");
            instruction.push_str(
                "Large outputs and context are written to files for on-demand retrieval:\n\n",
            );
            instruction.push_str("- `.vtcode/context/tool_outputs/` - Large tool outputs (use `read_file` or `grep_file` to explore)\n");
            instruction
                .push_str("- `.vtcode/history/` - Conversation history during summarization\n");
            instruction.push_str("- `.vtcode/mcp/tools/` - MCP tool descriptions and schemas\n");
            instruction
                .push_str("- `.vtcode/terminals/` - Terminal session output with metadata\n");
            instruction.push_str("- `.agents/skills/INDEX.md` - Available skills index\n\n");
            instruction.push_str("**Tip**: When a tool result says 'spooled to file', use `read_file` to access the full output.\n");
        }

        instruction.push_str("\n**IMPORTANT**: Respect these configuration policies. Commands not in the allow list will require user confirmation. Always inform users when actions require confirmation due to security policies.\n");
    }

    if !prompt_context
        .map(|ctx| ctx.skip_standard_instructions)
        .unwrap_or(false)
    {
        if let Some(cfg) = vtcode_config {
            if let Some(user_inst) = &cfg.agent.user_instructions {
                instruction.push_str("\n\n## USER INSTRUCTIONS\n");
                instruction.push_str(user_inst);
            }
        }
    }

    if let Some(bundle) = instruction_bundle {
        let home_ref = home_path.as_deref();
        instruction.push_str("\n\n## AGENTS.MD INSTRUCTION HIERARCHY\n");
        instruction.push_str(
                "Instructions are listed from lowest to highest precedence. When conflicts exist, defer to the later entries.\n\n",
            );

        for (index, segment) in bundle.segments.iter().enumerate() {
            let scope = match segment.source.scope {
                InstructionScope::Global => "global",
                InstructionScope::Workspace => "workspace",
                InstructionScope::Custom => "custom",
            };
            let display_path =
                format_instruction_path(&segment.source.path, project_root, home_ref);

            let _ = write!(
                instruction,
                "### {}. {} ({})\n\n",
                index + 1,
                display_path,
                scope
            );
            instruction.push_str(segment.contents.trim());
            instruction.push('\n');
        }

        if bundle.truncated {
            instruction.push_str(
                    "\n_Note: instruction content was truncated due to size limits. Review the source files for full details._",
                );
        }
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
    let (max_bytes, extra_sources) = match vtcode_config {
        Some(cfg) => (
            cfg.agent.instruction_max_bytes,
            cfg.agent.instruction_files.clone(),
        ),
        None => (instruction_constants::DEFAULT_MAX_BYTES, Vec::new()),
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

fn format_instruction_path(path: &Path, project_root: &Path, home_dir: Option<&Path>) -> String {
    if let Ok(relative) = path.strip_prefix(project_root) {
        let display = relative.display().to_string();
        if !display.is_empty() {
            return display;
        }

        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            return name.to_string();
        }
    }

    if let Some(home) = home_dir
        && let Ok(relative) = path.strip_prefix(home)
    {
        let display = relative.display().to_string();
        if display.is_empty() {
            return "~".to_string();
        }

        return format!("~/{display}");
    }

    path.display().to_string()
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
        // Use discriminant since SystemPromptMode doesn't derive Hash
        std::mem::discriminant(&cfg.agent.system_prompt_mode).hash(&mut hasher);
    } else {
        "default".hash(&mut hasher);
    }

    format!("sys_prompt:{:016x}", hasher.finish())
}

/// Generate a minimal system instruction (pi-inspired, <1K tokens)
pub fn generate_minimal_instruction() -> Content {
    // OPTIMIZATION: MINIMAL_SYSTEM_PROMPT is &'static str, use directly
    Content::system_text(MINIMAL_SYSTEM_PROMPT)
}

/// Generate a lightweight system instruction for simple operations
pub fn generate_lightweight_instruction() -> Content {
    // OPTIMIZATION: DEFAULT_LIGHTWEIGHT_PROMPT is &'static str, use directly
    Content::system_text(DEFAULT_LIGHTWEIGHT_PROMPT)
}

/// Generate a specialized system instruction for advanced operations
pub fn generate_specialized_instruction() -> Content {
    // OPTIMIZATION: DEFAULT_SPECIALIZED_PROMPT is &'static str, use directly
    Content::system_text(DEFAULT_SPECIALIZED_PROMPT)
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

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        // Minimal prompt should be much shorter than default
        // Note: composed prompt includes AGENTS.md content which can add size
        assert!(
            result.len() < 5000,
            "Minimal mode should produce <5K chars (was {} chars)",
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

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        // After v4.4 optimization, prompts are more concise
        // Default mode with configuration awareness should still have substantial content
        assert!(result.len() > 700, "Default mode should produce >700 chars");
        // Don't check for specific strings - prompt content may vary
        assert!(!result.is_empty(), "Should produce non-empty prompt");
    }

    #[tokio::test]
    async fn test_lightweight_mode_selection() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Lightweight;
        // Disable enhancements for base prompt size testing
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        // Lightweight is optimized for simple operations (v4.2)
        assert!(result.len() > 100, "Lightweight should be >100 chars");
        assert!(
            result.len() < 2000,
            "Lightweight should be compact (<2K chars)"
        );
    }

    #[tokio::test]
    async fn test_specialized_mode_selection() {
        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Specialized;
        // Disable enhancements for base prompt size testing
        config.agent.include_temporal_context = false;
        config.agent.include_working_directory = false;

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), None).await;

        // Specialized for complex tasks
        assert!(
            result.len() > 1000,
            "Specialized should have substantial content"
        );
        // The word "specialized" may not appear in the prompt text
        assert!(result.len() > 0, "Should produce non-empty prompt");
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
        // v6.0 harness-engineered prompt is ~1500 tokens (compressed from ~2700)
        assert!(
            approx_tokens > 1200 && approx_tokens < 2000,
            "Default prompt should be ~1500 tokens (harness v6.0), got ~{}",
            approx_tokens
        );
    }

    #[test]
    fn test_prompt_text_avoids_hardcoded_loop_thresholds() {
        assert!(!DEFAULT_SYSTEM_PROMPT.contains("stuck twice"));
        assert!(!MINIMAL_SYSTEM_PROMPT.contains("stuck twice"));
        assert!(!DEFAULT_SPECIALIZED_PROMPT.contains("stuck twice"));
        assert!(!DEFAULT_SPECIALIZED_PROMPT.contains("10+ calls without progress"));
        assert!(!DEFAULT_SPECIALIZED_PROMPT.contains("Same tool+params twice"));
        assert!(DEFAULT_SPECIALIZED_PROMPT.contains("runtime-configured"));
    }

    #[test]
    fn test_harness_awareness_in_prompts() {
        assert!(
            DEFAULT_SYSTEM_PROMPT.contains("docs/harness/"),
            "Default prompt should reference harness knowledge base"
        );
        assert!(
            DEFAULT_SYSTEM_PROMPT.contains("AGENTS.md"),
            "Default prompt should reference AGENTS.md as map"
        );
        assert!(
            DEFAULT_SYSTEM_PROMPT.contains("boy scout"),
            "Default prompt should include boy scout rule"
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

    // ENHANCEMENT TESTS

    #[tokio::test]
    async fn test_dynamic_guidelines_read_only() {
        use crate::config::types::CapabilityLevel;

        let mut config = VTCodeConfig::default();
        config.agent.system_prompt_mode = SystemPromptMode::Default;

        let mut ctx = PromptContext::default();
        ctx.add_tool("read_file".to_string());
        ctx.add_tool("grep_file".to_string());
        ctx.capability_level = Some(CapabilityLevel::FileReading);

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(
            result.contains("READ-ONLY MODE"),
            "Should detect read-only mode when no edit/write/bash tools available"
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
        ctx.add_tool("grep_file".to_string());
        ctx.add_tool("list_files".to_string());

        let result =
            compose_system_instruction_text(&PathBuf::from("."), Some(&config), Some(&ctx)).await;

        assert!(
            result.contains("grep_file") || result.contains("list_files"),
            "Should suggest grep/list as preferred tools"
        );
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
}
