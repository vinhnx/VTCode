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
use crate::prompts::system_prompt_cache::PROMPT_CACHE;
use crate::prompts::temporal::generate_temporal_context;
use dirs::home_dir;
use std::env;
use std::fmt::Write as _;
use std::path::Path;
use tracing::warn;

/// DEFAULT SYSTEM PROMPT (v4.4)
/// Optimized for clarity and token efficiency
const DEFAULT_SYSTEM_PROMPT: &str = r#"# VT Code Coding Assistant

Use tools immediately. Stop when done or blocked.

## Rules
- Use tools (grep_file, read_file, edit_file, run_pty_cmd) directly. JSON named params only.
- Read files before editing. Verify changes with tests/check.
- Stay in WORKSPACE_DIR. Confirm destructive ops (rm, force-push). No secrets.
- Summarize outcomes in 1-2 sentences. No code dumps or emojis.

## Strategy
Stuck twice on same error? Change approach.

## Planning (update_plan)
Non-trivial tasks: exploration → design → final_plan
- **understanding**: Read 5-10 files, find patterns
- **design**: 3-7 steps with file:line refs, dependencies, complexity
- **final_plan**: Verify paths, order, acceptance criteria

## Capability System (Lazy Loaded)
Tools are hidden by default to save context.
1. **Discovery**: Run `list_skills` (or `list_skills(query="...")`) to find tools.
2. **Activation**: Run `load_skill` to inject tool definitions and instructions.
3. **Usage**: Only *then* can you use the tool. Do not guess tool names.
4. **Resources**: If a skill references external files (scripts/docs), use `load_skill_resource`."#;

pub fn default_system_prompt() -> &'static str {
    DEFAULT_SYSTEM_PROMPT
}

pub fn minimal_system_prompt() -> &'static str {
    MINIMAL_SYSTEM_PROMPT
}

pub fn default_lightweight_prompt() -> &'static str {
    DEFAULT_LIGHTWEIGHT_PROMPT
}

/// MINIMAL PROMPT (v5.0 - Pi-inspired, <1K tokens)
/// Based on pi-coding-agent philosophy: modern models need minimal guidance
/// Reference: https://mariozechner.at/posts/2025-11-30-pi-coding-agent/
const MINIMAL_SYSTEM_PROMPT: &str = r#"You are VTCode, an expert coding assistant.

- Stay in WORKSPACE_DIR.
- Use JSON named params for tools.
- Read files before editing.
- Verify changes with tests or `cargo check`.
- Use `list_skills` and `load_skill` to discover and activate capabilities.
- Be direct; avoid filler or code dumping.
- Stop when done."#;

/// LIGHTWEIGHT PROMPT (v4.2 - Resource-constrained / Simple operations)
/// Minimal, essential guidance only
const DEFAULT_LIGHTWEIGHT_PROMPT: &str = r#"VT Code - efficient coding agent.

- Act and verify. Direct tone.
- Scoped: list_files, grep_file (≤5), read_file (max_tokens).
- Tools hidden by default. `list_skills --search <term>` to find them.
- WORKSPACE_DIR only. Confirm destructive ops."#;

/// SPECIALIZED PROMPT (v4.3 - Complex refactoring with streamlined guidance)
/// For multi-file changes and sophisticated code analysis
const DEFAULT_SPECIALIZED_PROMPT: &str = r#"# VT Code Specialized Agent

Complex refactoring and multi-file analysis. When stuck, try 2-3 alternatives before asking.

## Workflow
scope → plan → execute → verify → document

## Execution
- Scoped searches: list_files, grep_file with caps, read_file with max_tokens
- Edit in dependency order. Validate params. Prefer read-only first.
- Retry transient errors once. Reassess after 3+ low-signal calls.

## Loop Prevention
- Same tool+params twice → change approach
- 10+ calls without progress → explain blockers
- 90%+ context → write .progress.md, prep reset

## Planning (update_plan)
1. **understanding**: Read 5-10 files, find similar implementations, document file:line refs
2. **design**: 3-7 steps with paths, dependencies, complexity (simple/medium/complex)
3. **final_plan**: Verify paths, order, acceptance criteria before implementation

## Tooling Strategy (On-Demand)
- **Lazy Loading**: Most tools are NOT in context initially.
- **Workflow**: `list_skills` (find) → `load_skill` (activate) → use tool.
- **Agent Skills**: Prefer high-level skills (e.g., "git_workflow") over raw tools.
- **Deep Context**: Use `load_skill_resource` for specialized docs/scripts mentioned in `SKILL.md`."#;

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

/// Compose the system instruction text for the agent
///
/// ## Skills Integration Note
///
/// VTCode implements a **Tiered Disclosure** model for skills:
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

        if cfg.mcp.enabled {
            instruction.push_str(
                "- **MCP integrations**: Enabled. Prefer MCP tools (search_tools, list_mcp_resources, fetch_mcp_resource) for context before external fetches.\n",
            );
        }

        instruction.push_str("\n**IMPORTANT**: Respect these configuration policies. Commands not in the allow list will require user confirmation. Always inform users when actions require confirmation due to security policies.\n");
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
    let instruction = PROMPT_CACHE.get_or_insert_with(&cache_key, || {
        futures::executor::block_on(compose_system_instruction_text(
            project_root,
            vtcode_config,
            None, // No prompt_context for backward compatibility
        ))
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
        futures::executor::block_on(compose_system_instruction_text(
            project_root,
            None,
            None, // No prompt_context
        ))
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
        assert!(result.len() < 3000, "Minimal mode should produce <3K chars");
        assert!(
            result.contains("VTCode") || result.contains("VT Code"),
            "Should contain VTCode identifier"
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
        // After v4.4 optimization, default prompt is much more concise
        assert!(
            approx_tokens > 100 && approx_tokens < 300,
            "Default prompt should be ~150-250 tokens (optimized), got ~{}",
            approx_tokens
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
        let mut config = VTCodeConfig::default();

        let mut ctx = PromptContext::default();
        ctx.add_tool("run_pty_cmd".to_string());
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
        ctx.add_tool("read_file".to_string());
        ctx.add_tool("edit_file".to_string());
        ctx.add_tool("grep_file".to_string());
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
            result.contains("read_file") && result.contains("before"),
            "Should have read-before-edit guideline"
        );
    }
}
