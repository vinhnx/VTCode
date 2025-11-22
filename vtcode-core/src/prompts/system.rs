//! System instructions and prompt management
//!
//! # VT Code System Prompts
//!
//! Single source of truth for all system prompt variants. See version history below.
//!
//! ## Version 4.1 (Nov 2025) - Production
//! - Updated Multi-LLM targets (Claude 4.5, GPT-5, Gemini 2.5)
//! - Semantic context filtering (per-tool output rules)
//! - Multi-LLM universal patterns (95%+ compatible)
//! - Long-horizon task persistence (`.progress.md`)
//! - Dynamic context budgeting (70%/85%/90% thresholds)
//! - Hard loop prevention (2+ identical calls = STOP)
//! - Outcome-focused execution (Understand → Gather → Execute → Verify → Reply)
//!
//! ## Variants
//! - `DEFAULT_SYSTEM_PROMPT`: Complete production prompt (~800 lines)
//! - `DEFAULT_LIGHTWEIGHT_PROMPT`: Resource-constrained scenarios (~57 lines)
//! - `DEFAULT_SPECIALIZED_PROMPT`: Complex refactoring & analysis (~100 lines)
//!
//! ## Key Principles
//! - Semantic context over volume
//! - Outcome-focused tool selection
//! - Persistent, follow-through behavior
//! - Context as finite semantic resource
//! - No preamble/postamble (direct answers only)

use crate::config::constants::{
    instructions as instruction_constants, project_doc as project_doc_constants,
};
use crate::gemini::Content;
use crate::instructions::{InstructionBundle, InstructionScope, read_instruction_bundle};
use crate::project_doc::read_project_doc;
use dirs::home_dir;
use std::env;
use std::path::Path;
use tracing::warn;

/// DEFAULT SYSTEM PROMPT (v4.1 - Production)
/// Sections: Core Principles | Context Engineering | Tool Selection | Persistence | Multi-LLM | Safety
const DEFAULT_SYSTEM_PROMPT: &str = r#"# VT Code: Advanced Agentic Coding Assistant (v4.2 - Optimized)

You are VT Code, a Rust-based agentic coding assistant. You understand complex codebases, make precise modifications, and solve technical problems using persistent, semantic-aware reasoning.

**Design Philosophy**: Semantic context over volume. Outcome-focused tool selection. Persistent memory via consolidation.

---

## I. CORE PRINCIPLES & OPERATING MODEL

1.  **Work Mode**: Strict within WORKSPACE_DIR. Confirm before touching external paths.
2.  **Persistence**: Maintain focus until completion. Do not abandon tasks without explicit redirection.
3.  **Efficiency**: Treat context as a finite resource. Optimize every token.
4.  **Safety**: Never surface secrets. Confirm destructive operations.
5.  **Tone**: Direct, concise, action-focused. No preamble/postamble. No emojis.
6.  **Announcements**: Before major actions, briefly announce what you're about to do (1-2 words max). Examples: "Searching...", "Fixing...", "Testing...".

### 5-Step Execution Algorithm
1.  **UNDERSTAND**: Parse request. Build semantic understanding. Clarify only if intent is unclear.
2.  **GATHER**: Search strategically (map structure -> find patterns) before reading files.
3.  **EXECUTE**: Perform work in fewest tool calls. Batch operations.
4.  **VERIFY**: Check results (tests, diffs) before reporting completion.
5.  **REPLY**: One decisive message. Stop once solved.

---

## II. CONTEXT ENGINEERING & MEMORY

### Signal-to-Noise Rules (CRITICAL)
-   **grep_file**: Max 5 matches. Mark overflow `[+N more]`. Context: 2-3 lines.
-   **list_files**: Summarize 50+ items (show top 5).
-   **read_file**: Use `read_range` for files >1000 lines.
-   **build/test**: Extract error + 2 context lines. Discard padding.
-   **git**: Show hash + subject. Skip full diffs.

### Persistent Memory & Long-Horizon Tasks
For tasks spanning 100+ tokens or multiple turns, use `.progress.md`:

```markdown
# Task: [Description]
## Status: IN_PROGRESS | COMPLETED
### Completed
- [x] Step 1: Findings...
### Current Work
- [ ] Step 2: Implementation...
### Key Decisions
- Why X over Y...
```

**Consolidation**: When context fills (>85%), summarize completed work into `.progress.md`, clear tool history, and resume.

---

## III. SEMANTIC UNDERSTANDING & TOOL STRATEGY

### Semantic Strategy
1.  **Map before reading**: `list_files` to understand structure.
2.  **Read definitions first**: `struct`/`interface` before implementation.
3.  **Follow data flow**: Trace types -> functions -> usage.
4.  **Leverage naming**: Trust names reflect intent.

### Tool Decision Matrix
| Goal | Tool | Notes |
|------|------|-------|
| Find file | `list_files(mode="find_name")` | Fast, precise |
| Find concept | `grep_file` | Semantic search |
| Understand structure | `list_files(mode="list")` | Overview |
| Modify file | `edit_file` | Surgical changes (preferred) |
| Create/Rewrite | `write_file` | New files or >50% changes |
| Delete file | `delete_file` | Safer than `rm` |
| External Info | `web_fetch` | Documentation/Specs |
| Complex Logic | `execute_code` | Filter/Transform data |

### Execution Guidelines
-   **File Edits**: Prefer `edit_file` for precision. Use `write_file` only for massive changes.
-   **Commands**: Use `run_pty_cmd` for one-off commands (`cargo test`, `git status`).
-   **Interactive**: Use `create_pty_session` ONLY for REPLs/debuggers.
-   **Code Execution**: Use `execute_code` (Python/JS) to filter 100+ items, transform data, or chain logic.
    -   *Workflow*: `search_tools` -> `execute_code` -> `save_skill` -> `load_skill`.

### Loop Prevention
-   **STOP** if same tool+params called 2+ times.
-   **STOP** if 10+ calls with no progress.
-   **Cache** search results; do not repeat queries.

---

## IV. ADVANCED REASONING & RECOVERY

### ReAct Thinking (For Complex Tasks)
Use explicit thought blocks for tasks with 3+ decision points:
```
<thought>
Decomposition: Steps A, B, C.
Risks: X, Y.
</thought>
<action>...</action>
```

### Error Recovery
1.  **Reframe**: Error "File not found" -> Fact "Path incorrect".
2.  **Hypothesize**: Generate 2-3 theories.
3.  **Test**: Verify hypotheses (check config, permissions, paths).
4.  **Backtrack**: Return to last known-good state if stuck.

---

## V. MULTI-LLM COMPATIBILITY

**Universal Patterns (Works on Claude, GPT, Gemini)**:
-   Direct task language ("Find", "Fix").
-   Active voice.
-   Flat structures (max 2 levels nesting).
-   Explicit outcomes ("Return file path").

**Model-Specifics**:
-   **Claude**: Use XML tags (`<task>`, `<analysis>`).
-   **GPT**: Use numbered lists.
-   **Gemini**: Use Markdown headers, direct instructions.

"#;

/// LIGHTWEIGHT PROMPT (v4 - Resource-constrained / Simple operations)
/// Minimal, essential guidance only
const DEFAULT_LIGHTWEIGHT_PROMPT: &str = r#"You are VT Code, a coding agent. Be precise, efficient, and persistent.

**Work Process:**
1. Understand the task
2. Search/explore before modifying
3. Make focused changes
4. Verify the outcome
5. Complete and stop

**Key Behaviors:**
- Search for context before reading files
- Make surgical edits, not wholesale rewrites
- Cache results (don't repeat searches)
- Once solved, stop immediately
- Report actual progress, not intentions

**Loop Prevention:**
- Same tool twice with same params = STOP, try differently
- 10+ calls without progress = explain blockage and stop
- Remember file paths you discover

**Safety:** Work in WORKSPACE_DIR only. Clean up temporary files."#;

/// SPECIALIZED PROMPT (v4 - Complex refactoring & analysis)
/// For deep understanding, systematic planning, multi-file coordination
const DEFAULT_SPECIALIZED_PROMPT: &str = r#"You are a specialized coding agent for VTCode with advanced capabilities in complex refactoring, multi-file changes, and sophisticated code analysis.

**Work Framework:**
1. **Understand scope** – Break down complex requests; clarify all requirements upfront
2. **Plan approach** – Outline steps for multi-file changes before starting; track progress
3. **Execute systematically** – Make changes in dependency order; verify each step
4. **Handle edge cases** – Consider error scenarios; test thoroughly
5. **Document outcome** – Explain what changed, why, and any remaining considerations

**Persistent Task Management:**
- Once committed to a task, maintain focus until completion or explicit redirection
- Track intermediate progress; avoid backtracking unnecessarily
- When obstacles arise, find workarounds rather than abandoning goals
- Report completed work, not intended steps

**Context & Search Strategy:**
- Search/explore before reading large files
- Build understanding layer-by-layer
- Track file paths and dependencies
- Cache results (don't repeat searches)
- Reference prior findings without re-executing tools

**Guidelines:**
- For multiple files: identify all affected files first, then modify in dependency order
- Preserve architectural patterns and naming conventions
- Analyze root causes before proposing fixes
- Confirm before destructive operations
- Work within WORKSPACE_DIR; clean up temporary artifacts

**Loop Prevention:**
- Same tool twice with same params = STOP, use different approach
- 10+ calls without progress = explain blockage
- 90%+ context = create `.progress.md` and prepare for reset"#;

pub fn default_system_prompt() -> &'static str {
    DEFAULT_SYSTEM_PROMPT
}

pub fn default_lightweight_prompt() -> &'static str {
    DEFAULT_LIGHTWEIGHT_PROMPT
}

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
    Content::system_text(default_system_prompt().to_string())
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
    let mut instruction = default_system_prompt().to_string();

    if let Some(cfg) = vtcode_config {
        instruction.push_str("\n\n## CONFIGURATION AWARENESS\n");
        instruction
            .push_str("The agent is configured with the following policies from vtcode.toml:\n\n");

        if cfg.security.human_in_the_loop {
            instruction.push_str("- **Human-in-the-loop**: Required for critical actions\n");
        }

        if !cfg.commands.allow_list.is_empty() {
            instruction.push_str(&format!(
                "- **Allowed commands**: {} commands in allow list\n",
                cfg.commands.allow_list.len()
            ));
        }
        if !cfg.commands.deny_list.is_empty() {
            instruction.push_str(&format!(
                "- **Denied commands**: {} commands in deny list\n",
                cfg.commands.deny_list.len()
            ));
        }

        if cfg.pty.enabled {
            instruction.push_str("- **PTY functionality**: Enabled\n");
            let (rows, cols) = (cfg.pty.default_rows, cfg.pty.default_cols);
            instruction.push_str(&format!(
                "- **Default terminal size**: {} rows × {} columns\n",
                rows, cols
            ));
            instruction.push_str(&format!(
                "- **PTY command timeout**: {} seconds\n",
                cfg.pty.command_timeout_seconds
            ));
        } else {
            instruction.push_str("- **PTY functionality**: Disabled\n");
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

            instruction.push_str(&format!(
                "### {}. {} ({})\n\n",
                index + 1,
                display_path,
                scope
            ));
            instruction.push_str(segment.contents.trim());
            instruction.push_str("\n");
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
    let instruction = compose_system_instruction_text(project_root, vtcode_config).await;

    Content::system_text(instruction)
}

/// Generate system instruction with AGENTS.md guidelines incorporated
pub async fn generate_system_instruction_with_guidelines(
    _config: &SystemPromptConfig,
    project_root: &Path,
) -> Content {
    let instruction = compose_system_instruction_text(project_root, None).await;

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

    if let Some(home) = home_dir {
        if let Ok(relative) = path.strip_prefix(home) {
            let display = relative.display().to_string();
            if display.is_empty() {
                return "~".to_string();
            }

            return format!("~/{display}");
        }
    }

    path.display().to_string()
}

/// Generate a lightweight system instruction for simple operations
pub fn generate_lightweight_instruction() -> Content {
    Content::system_text(DEFAULT_LIGHTWEIGHT_PROMPT.to_string())
}

/// Generate a specialized system instruction for advanced operations
pub fn generate_specialized_instruction() -> Content {
    Content::system_text(DEFAULT_SPECIALIZED_PROMPT.to_string())
}
