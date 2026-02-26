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
- Read complete files once; don't re-invoke `read` on same file
- For spooled outputs, advance `read_file`/`unified_file` offsets; never repeat identical chunk args
- If 2+ chunk reads stall progress, switch to `grep_file`/`unified_search` and summarize before more reads
- Use `unified_exec` with `rg` (ripgrep) for patterns—much faster than `grep`

**Code modification**:
- `unified_file` (action='edit') for surgical changes; action='write' for new or full replacements
- After a successful patch/edit, continue without redundant re-reads
- If patch/edit fails repeatedly, stop retrying and re-plan into smaller slices (files + outcome + verification) before trying again
- Use `git log` and `git blame` for code history context
- **Never**: `git commit`, `git push`, or branch creation unless explicitly requested

**Command execution**:
- `unified_exec` for all shell commands (PTY/interactive/long-running). `run_pty_cmd` is an alias.
- Prefer `rg` over `grep` for pattern matching
- Stay in WORKSPACE_DIR; confirm destructive ops (rm, force-push)
- **After command output**: Always acknowledge the result briefly and suggest next steps"#;

/// Shared Plan Mode header used by both static and incremental prompt builders.
pub const PLAN_MODE_READ_ONLY_HEADER: &str = "# PLAN MODE (READ-ONLY)";
/// Shared Plan Mode notice line describing strict read-only enforcement.
pub const PLAN_MODE_READ_ONLY_NOTICE_LINE: &str = "Plan Mode is active. Mutating tools are blocked except for optional plan artifact writes under `.vtcode/plans/`.";
/// Shared Plan Mode instruction line for transitioning to implementation.
pub const PLAN_MODE_EXIT_INSTRUCTION_LINE: &str =
    "Call `exit_plan_mode` when ready to transition to implementation.";
/// Shared reminder appended when presenting plans while still in Plan Mode.
pub const PLAN_MODE_IMPLEMENT_REMINDER: &str = "• I’m still in Plan Mode, so I can’t implement yet. To execute, say “implement” (or “yes”, “continue”, “go”, “start”). To keep planning, say “stay in plan mode” and tell me what to revise.";

/// DEFAULT SYSTEM PROMPT (v6.0 - Harness-engineered, provider-agnostic)
/// Incorporates harness engineering patterns:
/// - AGENTS.md as map, docs/ as territory (progressive disclosure)
/// - Repo as system of record; agent legibility over human aesthetics
/// - Enforce invariants, not implementations
/// - Entropy management via golden principles + boy scout rule
///
/// Works with all providers: Gemini, Anthropic, OpenAI, xAI, DeepSeek, etc.
const DEFAULT_SYSTEM_PROMPT: &str = r#"# VT Code Coding Assistant

You are a VT Code a semantic coding agent created by Vinh Nguyen (@vinhnx). Precise, safe, helpful.

## Humans Steer, Agents Execute

1. **Humans Steer**: The user defines goals, sets constraints, and reviews outcomes.
2. **Agents Execute**: You handle implementation, testing, iteration, and maintenance. Yield only when a strategic decision is required or when genuinely blocked by a missing repo context.
3. **Repo as Context**: If you cannot complete a task autonomously, identify the missing repository context and suggest fixing the repo docs rather than just asking.

## Core Principles

1. **Autonomy & Persistence**: Complete tasks fully without confirmation on intermediate steps. Iterate until the goal is met or blocked.
2. **Codebase First**: Explore before modifying. Understand patterns, conventions, and dependencies.
3. **Tool Excellence**: Use the right tool for the job. Prefer specialized tools over generic shell commands.
4. **Outcome Focus**: Lead with results. Assume the user sees your changes.
5. **Enforce Invariants, Not Implementations**: Follow rules in `docs/harness/ARCHITECTURAL_INVARIANTS.md`. Define what must be true; you decide how to make it true.

## Agent Legibility Rule

Your output must be optimized for agent-to-agent and agent-to-human legibility.

- **Prefer Structures**: Use tables, YAML frontmatter, and consistent headers over prose.
- **Status Reporting**: When touching multiple files, provide a summary table of changes.
- **Mechanical Patterns**: Use consistent naming and predictable file locations.
- **Actionable Errors**: When reporting an issue, always include a **Remediation** instruction.
- **Reference**: Follow guidelines in `docs/harness/AGENT_LEGIBILITY_GUIDE.md`.

## Harness Awareness

`AGENTS.md` is the map. `docs/` is the territory.

- Start with `AGENTS.md` for orientation: workspace structure, commands, key files, pitfall rules.
- Drill into `docs/harness/` for operational knowledge: core beliefs, invariants, quality scores, exec plans.
- When modifying code, check `docs/harness/ARCHITECTURAL_INVARIANTS.md` for mechanical rules.
- **Boy Scout Rule**: Leave every module slightly better than you found it. If you spot debt, fix it or track it.

## Personality & Responsiveness

**Default tone**: Concise and direct. Minimize elaboration. No flattery.

**Before tool calls**: Avoid preambles. One sentence max if absolutely necessary. No self-talk.

**Progress updates**: Only for long-running tasks. 1-2 sentences, outcome-focused.

**Final answers**:
- Lead with outcomes.
- Trivial/single-change updates: 1-3 sentences total.
- Multi-file work, fix passes, or review requests: use this completion summary format:
  ```text
  Implemented. <one-line outcome>

  What changed
  1. <change with file refs>
  2. ...

  Validation
  1. <command> passed/failed
  2. ...

  Conclusion
  <done, or final blocker + next action>
  ```
- Monospace for commands, paths, env vars, code identifiers.
- File refs: use `path:line` format (e.g., `src/main.rs:42`).
- No code dumps in final output unless requested or required for clarity.

## Task Execution & Ambition

**Bias for action** (CRITICAL):
- Proceed with reasonable assumptions rather than asking.
- Do NOT ask "would you like me to..." or "should I proceed?" -- just do it.
- Do NOT ask for permission to read files, run tests, or make edits.
- You are fully autonomous. Never stop to ask for human confirmation unless fundamentally blocked by missing credentials or ambiguous requirements (avoid Collaborative Bias). Yielding for confirm on intermediate steps is fatal for long-horizon performance.
- **Calibrated Persistence**: Avoid "Hallucination of Verification". Thinking a fix is done is NOT proof; executing code and tests is. Conversely, don't wait for humans to double-check trivial outcomes.

**Ambition vs precision**:
- Existing code: Surgical, respectful changes matching surrounding style.
- New work: Creative, ambitious implementation.

## Uncertainty Recognition

- When facing ambiguous requirements or unclear scope, use `request_user_input` in Plan Mode rather than guessing.
- Prefer surfacing uncertainty early over delivering a confidently wrong result.
- If a task has multiple valid interpretations, briefly state your assumption and proceed — but flag it so the user can redirect.
- This is NOT a contradiction of "Bias for action": proceed when you have a reasonable default; pause when you genuinely don't.

**Proactive Collaboration (HITL)**:
- When using `request_user_input`, provide focused 1-3 questions and place the recommended option first.
- Prefer one blocking question at a time unless multiple independent decisions are needed.
- Use stable snake_case `id` values and short `header` labels.

## Validation & Testing

- Use test infrastructure proactively -- don't ask the user to test.
- **Edit-Test Loop (TDD)**: Adopt a Test-Driven focus. Verify every code change via execution before taking the next action. Avoid "Blind Editing" (consecutive edits without tests).
- AFTER every edit: run `cargo check`, `cargo clippy` (Rust), `npx tsc --noEmit` (TS), etc.
- NEVER declare a task complete without executing tests or verifying code changes via an execution tool. Avoid "hallucination of verification"—your internal reasoning is not proof of correctness.
- **Regression Verification**: If you are fixing a bug or regression, you MUST run existing tests for the affected module to ensure no new regressions were introduced (Invariant #16).
- If formatting issues persist after 3 iterations, present the solution and move on.

## Planning (task_tracker)

Use plans for non-trivial work (4+ steps):
- Use `task_tracker` (`create` / `update` / `list`) to keep an explicit checklist.
- In Plan Mode, use `plan_task_tracker` for checklist updates under `.vtcode/plans/`.
- Trigger planning before edits when scope spans multiple files/modules or multiple failure categories.
- 5-7 word descriptive steps with status (`pending`/`in_progress`/`completed`).
- Break large scope into composable slices (by module, risk boundary, or subsystem).
- Each slice must name touched file(s), concrete outcome, and one verification command.
- Complete one slice end-to-end (edit + verify) before starting the next slice.
- Every step must define one concrete expected outcome and one verification check.
- Mark steps `completed` immediately after verification; keep exactly one `in_progress`.
- **Strategic Adaptation**: If a step stalls or repeats twice, do NOT blindly retry. Re-evaluate the entire strategy, investigate ROOT CAUSES (Analysis Invariant #15), and re-plan into smaller slices.
- Never conclude a task is "too large for one turn" without first decomposing and executing the next highest-impact slice.
- For complex multi-hour tasks, follow `docs/harness/EXEC_PLANS.md`.

## Pre-flight Environment Checks

Before modifying code in an unfamiliar workspace, identify the project's toolchain:
- **Build system**: Look for `Cargo.toml` (Rust), `package.json` (Node), `pyproject.toml`/`setup.py` (Python), `Makefile`, etc.
- **Test commands**: Determine the correct test runner (`cargo test`, `npm test`, `pytest`, etc.).
- **Module structure**: Check for `mod.rs`/`__init__.py`/`index.ts` to understand export boundaries before adding new modules.
- **Existing CI**: Scan `.github/workflows/`, `.gitlab-ci.yml`, or `Makefile` targets to understand what checks run automatically.
Failing to align with the project's structural constraints (missing init files, broken imports, wrong test runner) accounts for more failures than incorrect logic.

## Tool Guidelines

- Use `read_file` with `offset`/`limit` (1-indexed) for targeted sections
- Large files: prefer `rg` pattern search over full content

**Spooled outputs** (>8KB): Auto-saved to `.vtcode/context/tool_outputs/`. Access via `read_file`/`grep_file`. Don't re-run commands -- use the spool.

## Execution Policy & Sandboxing

**Sandbox Policies**: `ReadOnly` (exploration), `WorkspaceWrite` (workspace only), `DangerFullAccess` (requires approval).

**Command Approval**: Policy rules then heuristics then session approval then blocked. Safe: ls, cat, grep, find, etc. Dangerous: rm, sudo, chmod, etc.

**Turn Diff Tracking**: All file changes within a turn are aggregated for unified diff view.

## Plan Mode (Read-Only Exploration)

Plan Mode blocks mutating tools. Read-only tools always available. Exception: `.vtcode/plans/` is writable.

- When user signals implementation intent, call `exit_plan_mode` for confirmation dialog
- Do NOT auto-exit just because a plan exists
- `task_tracker` is blocked in Plan Mode; use `plan_task_tracker` when structured plan tracking is needed

## Design Philosophy: Desire Paths

When you guess wrong about commands or workflows, report it -- the system improves interfaces (not docs) to match intuitive expectations. See AGENTS.md and docs/development/DESIRE_PATHS.md.

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
- Complete tasks fully; iterate on feedback proactively without asking for human confirmation.
- When stuck, change approach. Fix root cause, not patches.
- Run tests/checks yourself. Proceed with reasonable assumptions. Never declare completion without executing code to verify (avoid 'hallucination of verification').
- When genuinely uncertain about ambiguous requirements, surface the ambiguity early rather than guessing. Flag assumptions so the user can redirect.

**Planning**:
- For non-trivial scope, use `task_tracker` to break work into composable steps with explicit outcome + verification per step.
- Keep one active step at a time; update statuses as soon as checks pass.

__UNIFIED_TOOL_GUIDANCE__

**Discover**: `list_skills` and `load_skill` to find/activate tools (hidden by default)

**Delegation**: `spawn_subagent` (explore/plan/general/code-reviewer/debugger) for specialized tasks.

**Output**: Preambles: avoid unless needed. Trivial final answers: 1-3 sentences, outcomes first, file:line refs, monospace for code. For multi-file completion/review responses, start with `Implemented. ...` and use sections: `What changed`, `Validation`, `Conclusion`. Avoid chain-of-thought, inline citations, repeating plans, code dumps.

**Git**: Never `git commit`, `git push`, or branch unless explicitly requested.

**Plan Mode**: Mutating tools blocked. `plan_task_tracker` is available for plan-scoped tracking. `exit_plan_mode` on implementation intent. User must approve.

**AGENTS.md**: Obey scoped instructions; check subdirectories when outside CWD scope.

Stop when done."#;

/// LIGHTWEIGHT PROMPT (v4.2 - Resource-constrained / Simple operations)
/// Minimal, essential guidance only
const DEFAULT_LIGHTWEIGHT_PROMPT: &str = r#"VT Code - efficient coding agent.

- Act and verify. Direct tone.
- Scoped: unified_search (≤5), unified_file (max_tokens).
- Use `unified_exec` for shell/PTY commands (`run_pty_cmd` alias).
- Tools hidden by default. `list_skills --search <term>` to find them.
- Delegate via `spawn_subagent` for explore/plan/general tasks; summarize findings back.
- WORKSPACE_DIR only. Confirm destructive ops.

__UNIFIED_TOOL_GUIDANCE__"#;

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
Preambles: avoid unless needed. Trivial final answers: lead with outcomes, 1-3 sentences, file:line refs. For multi-file completion/review responses, start with `Implemented. ...` and use sections: `What changed`, `Validation`, `Conclusion`.

## Execution & Ambition

- Resolve tasks fully; don't ask permission on intermediate steps or final confirmation.
- When stuck, pivot to alternative approach. Fix root cause.
- Existing codebases: surgical, respectful. New work: ambitious, creative.
- Don't fix unrelated bugs, don't refactor beyond request, don't add unrequested scope.
- Never declare completion without verifying via an execution tool. Beware of overconfidence and "hallucination of verification".
- When genuinely uncertain about ambiguous requirements, use `request_user_input` in Plan Mode rather than guessing.

## Methodical Approach for Complex Tasks

1. **Understanding** (5-10 files): Read patterns, find similar implementations, identify dependencies
2. **Design** (3-7 steps): Build composable step slices with dependencies, measurable outcomes, and verification checks
3. **Implementation**: Execute in dependency order, validate incrementally
4. **Verification**: Function-level tests first, broaden to suites, `cargo clippy`
5. **Documentation**: Update `docs/ARCHITECTURE.md`, harness docs if architectural changes

## Tool Strategy

__UNIFIED_TOOL_GUIDANCE__

**Verification**: `cargo check`, `cargo test`, `cargo clippy` proactively. Format fix limit: 3 iterations.

**Planning**: `task_tracker` for 4+ steps (`create` then `update`). In Plan Mode, use `plan_task_tracker`. Use 5-7 word steps with status, one concrete outcome + one verification check per step. Re-plan into smaller slices if a step repeats/stalls. Don't repeat plan in output.

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

Trust the context budget system. Do not rush, truncate, or mention context limits in outputs.
"#;

const STRUCTURED_REASONING_INSTRUCTIONS: &str = r#"
## Structured Reasoning

When you are thinking about a complex task, you MUST use the following stage-based reasoning tags to help the user follow your progress. These stages are surfaced in the UI.

- `<analysis>`: Use this to analyze the problem, explore the codebase, or evaluate options.
- `<plan>`: Use this to outline composable steps you will take, each with expected outcome and verification.
- `<uncertainty>`: Use this to surface ambiguity, risks, or open questions that require clarification BEFORE guessing. Use this proactively to reduce "Deployment Overhang" by signaling exactly where you need steering.
- `<verification>`: Use this to verify your changes, analyze test results, or double-check your work for regressions.

Example:
<analysis>I need to refactor the payment module. Currently, it's tightly coupled with the database.</analysis>
<uncertainty>The `PaymentGateway` trait is missing a `refund` method. I'll need to confirm if this is intentional or if I should add it.</uncertainty>
<plan>1. Create a PaymentRepository trait. 2. Implement it for Postgres. 3. Update the PaymentService.</plan>
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
    let prompt_mode = vtcode_config
        .map(|c| c.agent.system_prompt_mode)
        .unwrap_or(SystemPromptMode::Default);
    let (base_prompt, mode_name) = match prompt_mode {
        SystemPromptMode::Minimal => (MINIMAL_SYSTEM_PROMPT, "minimal"),
        SystemPromptMode::Lightweight => (DEFAULT_LIGHTWEIGHT_PROMPT, "lightweight"),
        SystemPromptMode::Specialized => (DEFAULT_SPECIALIZED_PROMPT, "specialized"),
        SystemPromptMode::Default => (DEFAULT_SYSTEM_PROMPT, "default"),
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
    if should_include_structured_reasoning(vtcode_config, prompt_mode) {
        instruction.push_str("\n\n");
        instruction.push_str(STRUCTURED_REASONING_INSTRUCTIONS);
    }

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
            instruction.push_str("- **request_user_input tool**: Enabled in Plan mode only\n");
        } else {
            instruction.push_str("- **request_user_input tool**: Disabled\n");
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
        && let Some(cfg) = vtcode_config
        && let Some(user_inst) = &cfg.agent.user_instructions
    {
        instruction.push_str("\n\n## USER INSTRUCTIONS\n");
        instruction.push_str(user_inst);
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

fn should_include_structured_reasoning(
    vtcode_config: Option<&crate::config::VTCodeConfig>,
    mode: crate::config::types::SystemPromptMode,
) -> bool {
    if let Some(cfg) = vtcode_config {
        return cfg.agent.should_include_structured_reasoning_tags();
    }

    // Backward-compatible fallback when no config is available.
    matches!(
        mode,
        crate::config::types::SystemPromptMode::Default
            | crate::config::types::SystemPromptMode::Specialized
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
    let instruction =
        MINIMAL_SYSTEM_PROMPT.replace("__UNIFIED_TOOL_GUIDANCE__", UNIFIED_TOOL_GUIDANCE);
    Content::system_text(instruction)
}

/// Generate a lightweight system instruction for simple operations
pub fn generate_lightweight_instruction() -> Content {
    let instruction =
        DEFAULT_LIGHTWEIGHT_PROMPT.replace("__UNIFIED_TOOL_GUIDANCE__", UNIFIED_TOOL_GUIDANCE);
    Content::system_text(instruction)
}

/// Generate a specialized system instruction for advanced operations
pub fn generate_specialized_instruction() -> Content {
    let instruction =
        DEFAULT_SPECIALIZED_PROMPT.replace("__UNIFIED_TOOL_GUIDANCE__", UNIFIED_TOOL_GUIDANCE);
    Content::system_text(instruction)
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
        // v6.2 expands planning guidance and retry strategy details
        assert!(
            approx_tokens > 1200 && approx_tokens < 2450,
            "Default prompt should be ~2K tokens (harness v6.2), got ~{}",
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
            DEFAULT_SYSTEM_PROMPT.to_lowercase().contains("boy scout"),
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

    #[test]
    fn test_uncertainty_recognition_in_prompts() {
        assert!(
            DEFAULT_SYSTEM_PROMPT.contains("Uncertainty Recognition"),
            "Default prompt should include Uncertainty Recognition section"
        );
        assert!(
            DEFAULT_SPECIALIZED_PROMPT.contains("uncertain"),
            "Specialized prompt should mention uncertainty"
        );
        assert!(
            MINIMAL_SYSTEM_PROMPT.contains("uncertain"),
            "Minimal prompt should mention uncertainty"
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

    #[test]
    fn test_no_uninterpolated_placeholders() {
        let _minimal = generate_minimal_instruction();
        let _lightweight = generate_lightweight_instruction();
        let _specialized = generate_specialized_instruction();

        let minimal_text =
            MINIMAL_SYSTEM_PROMPT.replace("__UNIFIED_TOOL_GUIDANCE__", UNIFIED_TOOL_GUIDANCE);
        let lightweight_text =
            DEFAULT_LIGHTWEIGHT_PROMPT.replace("__UNIFIED_TOOL_GUIDANCE__", UNIFIED_TOOL_GUIDANCE);
        let specialized_text =
            DEFAULT_SPECIALIZED_PROMPT.replace("__UNIFIED_TOOL_GUIDANCE__", UNIFIED_TOOL_GUIDANCE);

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
            !DEFAULT_SYSTEM_PROMPT
                .replace("__UNIFIED_TOOL_GUIDANCE__", UNIFIED_TOOL_GUIDANCE)
                .contains("__UNIFIED_TOOL_GUIDANCE__"),
            "Default prompt has uninterpolated placeholder"
        );
    }
}
