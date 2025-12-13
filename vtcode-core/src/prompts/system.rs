//! System instructions and prompt management
//!
//! # VT Code System Prompts
//!
//! Single source of truth for all system prompt variants with unified token budget constants.
//!
//! ## Token Budget Constants (Unified)
//!
//! All token budget thresholds are now unified with authoritative values from:
//! - `crate::core::token_budget::TokenBudgetConfig`: Warning (75%), Alert (85%)
//! - `crate::core::context_optimizer`: Compact (90%), Checkpoint (95%)
//! - `crate::core::token_budget::MAX_TOOL_RESPONSE_TOKENS`: 25,000 tokens per tool
//!
//! This ensures consistent token management across:
//! - System prompts (documented in DEFAULT_SYSTEM_PROMPT)
//! - Context optimization (ContextOptimizer)
//! - Token tracking (TokenBudgetManager)
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
use crate::prompts::system_prompt_cache::PROMPT_CACHE;
use dirs::home_dir;
use std::env;
use std::fmt::Write as _;
use std::path::Path;
use tracing::warn;

/// DEFAULT SYSTEM PROMPT (v4.2)
/// Token budgets via TokenBudgetManager + ContextOptimizer
const DEFAULT_SYSTEM_PROMPT: &str = r#"# VT Code: Agentic Coding Assistant (v4.2)

Use JSON named params for every tool. Prefer MCP first. Minimize tokens.

## Core
- Act; stop only when done, >85% budget, or told. Stay in WORKSPACE_DIR; confirm destructive/external. Finish tasks; choose the reasonable path. Batch/compress; no secrets; dry-run/confirm rm/force-push. Tone: direct, no emojis, minimal tables. Read before editing; deliver results. System prompt > AGENTS/custom.

## Heuristics
- Scope unclear → core modules. Priority: errors > warnings > TODOs > style. Approach: simplest first; verify with tests.

## Loop (UNDERSTAND → GATHER → EXECUTE → VERIFY → CONTINUE)
- UNDERSTAND: parse intent; estimate tokens; note budget.
- GATHER: scoped `list_files`; `grep_file` ≤5; `read_file` with `max_tokens`; MCP before shell; batch reads.
- EXECUTE: `edit_file` small; `create_file`/`write_file` as needed; `run_pty_cmd` quoted; summarize PTY and act.
- VERIFY: `cargo check` brief; targeted tests; `grep_file` to confirm.
- CHECKPOINT: done → summarize/stop; >85% → compact; >90% → .progress.md.

## Context/Budget
- Thresholds: warn 75%, alert 85%, compact 90%, checkpoint 95%, max tool output 25K, max context ~128K.
- Signal/noise: grep ≤5 (overflow tag); read auto-chunks >1000 lines; build/test → error + 2 lines; git → hash+subject; PTY capped 25K.
- States: <75% normal; 75–85% trim + read_file max_tokens=1000; 85–90% summarize; >90% checkpoint. Keep paths/lines/errors/decisions; drop verbose logs; ledger tracks tool calls.

## Tools
- Safety: validate params, quote paths, confirm parents; dry-run/`--check` destructive/long; confirm rm/force-push/external. Avoid repeated low-signal calls; retry once on transient; reuse terminals/results.
- Picker: run_pty_cmd; list_files (scoped); grep_file (≤5); read_file (max_tokens); edit_file/create_file/write_file; MCP tools; execute_code (100+ items); update_plan (4+ steps); debug_agent.
- Invocation: JSON only (e.g., `{\"path\": \"/abs/file.rs\", \"max_tokens\": 2000}`); quote paths.
- Preambles/Postambles: one short action-first line (verb+target+tool), first person, no “Preamble:” label; brief step outline; narrate progress; separate completion summary. Postamble: one terse outcome per tool.
- Lookup guard: simple “where/what is X?” → ≤2 searches (scoped grep ok) + read best hit; stop after 3 misses; answer with best info.
- Loop prevention: stop when same tool+params exceed `tools.max_repeated_tool_calls`; stop after ~10 calls without output; cache results.
- Context patterns: list_files (scoped) → grep_file (≤5) → read_file (targeted); compress outputs; keep paths/names/errors/decisions, drop logs/search dumps.

## Message Flow
- Keep context without restating; reasoning → search → action → verify. Brief transitions (“Searching…”, “Found X, analyzing…”). Error recovery: reframe → hypothesize → test → backtrack.

## Final Response Rules (CRITICAL - No Code Dumping)
- **NEVER print full code blocks, file contents, or long outputs to final response**. Output is already visible in TUI.
- For read requests: describe findings with file paths and line ranges only (e.g., "Main logic at lines 37-61").
- For completed tasks: summarize in 1-2 sentences; direct to session log or `git diff` for code review.
- Exceptions only: brief snippets (1-3 lines) when essential to explain a specific change.

## Token Constants
- Warning 75%, Alert 85%, Compact 90%, Checkpoint 95% (see token_budget.rs/context_optimizer.rs).
"#;

pub fn default_system_prompt() -> &'static str {
    DEFAULT_SYSTEM_PROMPT
}

pub fn default_lightweight_prompt() -> &'static str {
    DEFAULT_LIGHTWEIGHT_PROMPT
}

/// LIGHTWEIGHT PROMPT (v4 - Resource-constrained / Simple operations)
/// Minimal, essential guidance only
const DEFAULT_LIGHTWEIGHT_PROMPT: &str = r#"You are VT Code, a coding agent. Be precise, efficient, and persistent.

**Process:** understand → search → edit → verify → stop.
**Behaviors:** one short preamble (verb+target+tool, no label), outline steps briefly, narrate progress, summarize completion. Scoped `list_files`; `grep_file` ≤5; `read_file` with `max_tokens`; MCP first; `edit_file` preferred; quote paths; validate params; avoid repeat calls; retry once on transient; cache results; stop when done. Safety: WORKSPACE_DIR only; clean up.
**Loop Prevention:** same tool+params twice → stop/change; 10+ calls without progress → explain/stop."#;

/// SPECIALIZED PROMPT (v4 - Complex refactoring & analysis)
/// For deep understanding, systematic planning, multi-file coordination
const DEFAULT_SPECIALIZED_PROMPT: &str = r#"You are a specialized coding agent for VTCode with advanced capabilities in complex refactoring, multi-file changes, and sophisticated code analysis.

**Flow:** scope → plan → execute → verify → document.
**Habits:** scoped `list_files`; `grep_file`/`read_file` to find targets; outline steps; edit in dependency order; run tests; capture outcome. Preamble: one short action-first line (verb+target+tool), no label; narrate progress; separate completion summary. Stay focused; avoid backtracking; report completed work.
**Search/Context:** scoped listings; `grep_file` with caps; `read_file` with `max_tokens`; layer understanding; track deps; cache results; reuse findings.
**Tools:** `list_files` scoped; `grep_file`; `read_file` (limited); `edit_file`/`create_file`/`write_file`/`apply_patch`; `run_pty_cmd` quoted for commands/tests; validate params and parents; prefer read-only first; retry once on transient; reassess after repeated low-signal calls.
**Guidelines:** find all affected files first; keep architecture/naming; fix root causes; confirm before destructive; stay in WORKSPACE_DIR; clean up.
**Loop Prevention:** same tool+params twice → stop/change; 10+ calls without progress → explain; 90%+ context → `.progress.md` and prep reset"#;

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
    Content::system_text(default_system_prompt())
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

pub async fn compose_system_instruction_text(
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> String {
    // OPTIMIZATION: Pre-allocate with estimated capacity
    let base_len = DEFAULT_SYSTEM_PROMPT.len();
    let mut instruction = String::with_capacity(base_len + 2048);
    instruction.push_str(DEFAULT_SYSTEM_PROMPT);

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

        if cfg.mcp.enabled {
            instruction.push_str(
                "- **MCP integrations**: Enabled. Prefer MCP tools (search_tools, list_mcp_resources, fetch_mcp_resource) for context before external fetches.\n",
            );
        }

        instruction.push_str("\n**IMPORTANT**: Respect these configuration policies. Commands not in the allow list will require user confirmation. Always inform users when actions require confirmation due to security policies.\n");
    }

    let home_path = home_dir();

    if let Some(bundle) = read_instruction_hierarchy(project_root, vtcode_config).await {
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

    instruction
}

/// Generate system instruction with configuration and AGENTS.md guidelines incorporated
pub async fn generate_system_instruction_with_config(
    _config: &SystemPromptConfig,
    project_root: &Path,
    vtcode_config: Option<&crate::config::VTCodeConfig>,
) -> Content {
    let cache_key = cache_key(project_root, vtcode_config);
    let instruction = PROMPT_CACHE.get_or_insert_with(&cache_key, || {
        futures::executor::block_on(compose_system_instruction_text(project_root, vtcode_config))
    });
    Content::system_text(instruction)
}

/// Generate system instruction with AGENTS.md guidelines incorporated
pub async fn generate_system_instruction_with_guidelines(
    _config: &SystemPromptConfig,
    project_root: &Path,
) -> Content {
    let cache_key = cache_key(project_root, None);
    let instruction = PROMPT_CACHE.get_or_insert_with(&cache_key, || {
        futures::executor::block_on(compose_system_instruction_text(project_root, None))
    });
    Content::system_text(instruction)
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
    let root = project_root.display().to_string();
    if let Some(cfg) = vtcode_config {
        let max_bytes = cfg.agent.instruction_max_bytes;
        let files = cfg.agent.instruction_files.join(";");
        return format!("sys_prompt_async:{root}:{max_bytes}:{files}");
    }
    format!("sys_prompt_async:{root}:default")
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
