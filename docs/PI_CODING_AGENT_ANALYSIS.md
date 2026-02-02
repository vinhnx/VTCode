# Pi Coding Agent Analysis & Recommendations for VT Code

**Date**: 2025-12-21
**Source**: https://mariozechner.at/posts/2025-11-30-pi-coding-agent/
**Status**: Analysis Complete

## Executive Summary

Mario Zechner's pi-coding-agent demonstrates that **minimalism works** in coding agent design. Benchmark results (Terminal-Bench 2.0) prove that a ~1,000 token system prompt and 4 core tools perform competitively against feature-rich harnesses.

### Key Finding

> "Modern frontier models are RL-trained enough to understand coding agents without massive prompts."

## Current State: VT Code

### System Prompt Analysis

| Prompt Type                | Lines    | Est. Tokens | Use Case             |
| -------------------------- | -------- | ----------- | -------------------- |
| DEFAULT_SYSTEM_PROMPT      | ~116     | ~6,500      | Production           |
| DEFAULT_LIGHTWEIGHT_PROMPT | ~6       | ~300        | Resource-constrained |
| DEFAULT_SPECIALIZED_PROMPT | ~10      | ~500        | Complex refactoring  |
| + AGENTS.md hierarchy      | Variable | Variable    | Project-specific     |
| + Configuration awareness  | ~60      | ~800        | Runtime policies     |

**Total base overhead**: ~7,800 tokens (vs pi's <1,000)

### Tool Inventory

**Built-in tools** (22):

1. grep_file
2. list_files
3. run_pty_cmd
4. update_plan
5. create_pty_session
6. list_pty_sessions
7. close_pty_session
8. send_pty_input
9. read_pty_session
10. resize_pty_session
11. web_fetch
12. read_file
13. create_file
14. delete_file
15. write_file
16. edit_file
17. apply_patch
18. search_replace
19. search_tools
20. skill
21. execute_code
22. debug_agent
23. analyze_agent

**MCP tools**: Variable (13-18k tokens per server according to pi's analysis)

**Tool result structure**: Currently unified (no LLM vs UI split)

## Pi's Philosophy vs VT Code

| Aspect              | Pi                          | VT Code                          | Gap                                 |
| ------------------- | --------------------------- | -------------------------------- | ----------------------------------- |
| **System Prompt**   | <1,000 tokens               | ~7,800 tokens                    | 7.8x overhead                       |
| **Core Tools**      | 4 (read, write, edit, bash) | 22 built-in + MCP                | 5.5x+ tools                         |
| **MCP Support**     | No (use CLI tools)          | Yes                              | Context overhead                    |
| **Security**        | YOLO by default             | Workspace boundaries, allowlists | Friction vs theater?                |
| **Todos**           | External files (TODO.md)    | Built-in `update_plan` tool      | Internal state tracking             |
| **Plan Mode**       | Files (PLAN.md)             | Read-only mode via flags         | Similar approach                    |
| **Background Bash** | Use tmux                    | PTY session management           | Better integration vs observability |
| **Sub-agents**      | Spawn via bash              | Task tool with agents            | Observability concern               |
| **Observability**   | Full session visibility     | Event system + logging           | Good baseline                       |

## Recommendations by Priority

### IMMEDIATE (High Impact, Low Effort)

#### 1. System Prompt Diet (Target: <3,000 tokens)

**Current bloat areas**:

- Verbose anti-giving-up policy (lines 54-66): ~500 tokens
- Redundant tool picker guidance (line 95): ~300 tokens
- Loop prevention duplication (lines 102-103): ~150 tokens
- Final response rules (lines 108-113): ~200 tokens

**Recommendation**: Create `MINIMAL_SYSTEM_PROMPT` variant:

```rust
const MINIMAL_SYSTEM_PROMPT: &str = r#"# VT Code: Agentic Coding Assistant

You are a coding agent. Act until done, >85% budget, or blocked.

## Core
- Stay in WORKSPACE_DIR; confirm destructive ops
- Read before editing; prefer scoped tools
- JSON params only; quote paths
- Never give up: try 3 alternatives before asking

## Loop (UNDERSTAND → GATHER → EXECUTE → VERIFY)
- GATHER: list_files (scoped), grep_file (≤5), read_file (max_tokens)
- EXECUTE: edit_file, create_file, run_pty_cmd (quoted)
- VERIFY: cargo check/test; confirm changes

## Budget
- <75%: normal | 75-85%: trim | 85-90%: summarize | >90%: checkpoint
- Max tool output: 25K tokens

## Tools
See tool descriptions. Prefer MCP first when enabled.
"#;
```

**Action**: Add configuration flag in `vtcode.toml`:

```toml
[agent]
system_prompt_mode = "minimal"  # vs "default" vs "specialized"
```

#### 2. Progressive Tool Documentation Loading

**Problem**: All 22+ tool descriptions load upfront (~3-4k tokens)

**Pi's approach**: Tools know their own usage; models trained on standard patterns

**Solution**: Lazy load tool documentation

```rust
pub struct ToolDocumentation {
    /// Minimal signature (always loaded)
    pub signature: &'static str,
    /// Full docs (loaded on-demand via ask_user_question)
    pub full_docs: Option<String>,
}

// vtcode-core/src/tools/registry/inventory.rs
impl ToolInventory {
    pub fn get_tool_signatures(&self) -> Vec<&str> {
        self.tools.values()
            .map(|t| t.signature)
            .collect()
    }

    pub fn get_full_docs(&self, tool_name: &str) -> Option<String> {
        self.tools.get(tool_name)
            .and_then(|t| t.full_docs.clone())
    }
}
```

**Estimated savings**: 2,000-3,000 tokens per request

#### 3. Configuration: YOLO Mode

Add explicit security mode choice:

```toml
[security]
mode = "yolo"  # vs "workspace" vs "paranoid"

# YOLO: No prompts, full filesystem access (pi's approach)
# WORKSPACE: Current behavior (workspace boundaries)
# PARANOID: Confirm all file writes, bash commands
```

Document clearly in `docs/SECURITY_MODEL.md`:

> **YOLO Mode Warning**: If an agent can write and execute code with network access,
> data exfiltration is always possible. Security measures are largely theater.
> Use containers for untrusted workloads.

### SHORT-TERM (High Impact, Medium Effort)

#### 4. Split Tool Results (LLM vs UI)

Implement pi's structured split pattern:

```rust
// vtcode-core/src/tools/traits.rs
pub struct ToolResult {
    /// Content for LLM context (text summary)
    pub llm_content: Vec<ContentBlock>,

    /// Rich data for TUI rendering (optional)
    pub ui_details: Option<serde_json::Value>,

    /// Attachments (images, diffs, etc)
    pub attachments: Vec<Attachment>,
}

// Example: grep_file
impl Tool for GrepFileTool {
    async fn execute(&self, args: Args) -> Result<ToolResult> {
        let matches = perform_grep(args)?;

        Ok(ToolResult {
            // LLM sees summary
            llm_content: vec![ContentBlock::Text(
                format!("Found {} matches in {} files",
                    matches.len(), unique_files)
            )],

            // TUI shows full results with syntax highlighting
            ui_details: Some(json!({
                "matches": matches,
                "context_lines": 2,
                "highlight": true
            })),

            attachments: vec![],
        })
    }
}
```

**Benefits**:

- Reduced context pollution
- Richer TUI display
- Better token efficiency

**Estimated impact**: 20-30% context savings on tool-heavy sessions

#### 5. MCP Cost Analysis Tool

Create diagnostic tool to measure MCP overhead:

```bash
cargo run -- mcp analyze

# Output:
# MCP Server: playwright
#   Tools: 21
#   Token overhead: 13,700
#   Usage last 10 sessions: 2 tool calls
#   Recommendation: Convert to CLI tool
#
# MCP Server: filesystem
#   Tools: 8
#   Token overhead: 4,200
#   Usage last 10 sessions: 347 tool calls
#   Recommendation: Keep enabled
```

#### 6. Session Export Format

Implement clean JSON export for post-processing:

```rust
// vtcode-core/src/utils/session_archive.rs
pub struct SessionExport {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub model: String,
    pub turns: Vec<Turn>,
    pub token_usage: TokenUsage,
    pub tool_calls: Vec<ToolCallRecord>,
}

pub fn export_session(session: &Session) -> Result<SessionExport> {
    // Clean, parseable format for analysis
}
```

### MEDIUM-TERM (Medium Impact, High Effort)

#### 7. Differential Rendering for TUI

Apply pi's approach to Ratatui-based TUI:

```rust
// src/ui/tui/differential_renderer.rs
pub struct DifferentialRenderer {
    /// Previous frame's lines
    backbuffer: Vec<String>,
    /// Synchronized output support
    supports_sync: bool,
}

impl DifferentialRenderer {
    pub fn render(&mut self, components: &[Component]) -> Result<()> {
        let new_lines = self.collect_lines(components);
        let first_changed = self.find_first_diff(&new_lines);

        if self.supports_sync {
            // CSI ?2026h
            print!("\x1b[?2026h");
        }

        // Only redraw from first_changed onwards
        self.redraw_from(first_changed, &new_lines)?;

        if self.supports_sync {
            // CSI ?2026l
            print!("\x1b[?2026l");
        }

        self.backbuffer = new_lines;
        Ok(())
    }
}
```

**Benefit**: Reduce flicker in VS Code terminal

#### 8. Context Handoff Testing

Ensure mid-session model switching works:

```rust
#[cfg(test)]
mod cross_provider_tests {
    #[tokio::test]
    async fn claude_to_gpt_handoff() {
        let mut session = Session::new();

        // Start with Claude
        session.set_model("anthropic", "claude-sonnet-4-5");
        session.add_message(user("What is 25 * 18?"));
        let response = session.run().await?;
        assert!(response.content.contains("450"));

        // Switch to GPT mid-session
        session.set_model("openai", "gpt-5");
        session.add_message(user("Is that correct?"));
        let response = session.run().await?;

        // GPT should see Claude's thinking as <thinking> tags
        assert!(session.messages[1].content.contains("<thinking>"));
    }
}
```

#### 9. Tool Consolidation Analysis

Audit if some tools can merge:

- `create_file` + `write_file` → `write_file` (creates if missing)
- PTY tools (6) → Can some be consolidated?
- `debug_agent` + `analyze_agent` → Different enough?

**Guiding principle** (from pi):

> "Bash is all you need" - Can the agent achieve this via `run_pty_cmd`?

### LONG-TERM (Strategic)

#### 10. Terminal-Bench Integration

Run VT Code through Terminal-Bench 2.0:

```bash
git clone https://github.com/laude-institute/terminal-bench
cd terminal-bench

# Create vtcode agent adapter
# Compare against:
# - Pi (minimal)
# - Claude Code (feature-rich)
# - Terminus 2 (tmux-only)
```

**Goal**: Validate that minimalism doesn't sacrifice performance

#### 11. CLI Tool Ecosystem (MCP Alternative)

Create `~/.vtcode/tools/` with README-based tools:

```
~/.vtcode/tools/
  ├── websearch/
  │   ├── README.md (tool docs)
  │   └── search.sh
  ├── screenshot/
  │   ├── README.md
  │   └── capture.py
  └── ...
```

Agent workflow:

1. User asks for web search
2. Agent: `run_pty_cmd("cat ~/.vtcode/tools/websearch/README.md")`
3. Agent learns usage (progressive disclosure)
4. Agent: `run_pty_cmd("~/.vtcode/tools/websearch/search.sh 'rust async')`

**Benefits**:

- Pay token cost only when needed
- Composable via pipes
- No MCP server overhead
- Easy to extend (just add scripts)

## Configuration Proposal

New `vtcode.toml` section:

```toml
[agent.philosophy]
# Minimalism mode (inspired by pi-coding-agent)
mode = "balanced"  # "minimal" | "balanced" | "full-featured"

# System prompt strategy
system_prompt = "default"  # "minimal" | "default" | "specialized"

# Tool loading strategy
tool_loading = "lazy"  # "eager" | "lazy" | "minimal"

# Security model
security = "workspace"  # "yolo" | "workspace" | "paranoid"

[agent.context_optimization]
# Progressive tool documentation
progressive_tool_docs = true

# Split tool results (LLM vs UI)
split_tool_results = true

# Max tool output tokens (per pi's 25K limit)
max_tool_output_tokens = 25000

[agent.mcp]
# MCP overhead warnings
warn_on_high_token_servers = true
token_threshold = 10000

# Alternative: CLI tool ecosystem
prefer_cli_tools = false
cli_tools_path = "~/.vtcode/tools"
```

## Measured Impact Projections

| Change                        | Token Savings         | Complexity | Priority |
| ----------------------------- | --------------------- | ---------- | -------- |
| Minimal system prompt         | -4,800 tokens         | Low        | P0       |
| Progressive tool docs         | -2,500 tokens         | Medium     | P0       |
| Split tool results            | -20-30% on tool calls | Medium     | P1       |
| Remove verbose anti-giving-up | -500 tokens           | Low        | P0       |
| MCP overhead warnings         | N/A (awareness)       | Low        | P1       |
| Differential rendering        | N/A (UX)              | High       | P2       |

**Total potential savings**: ~7,800 tokens/request in minimal mode (50% reduction)

## The Core Question

VT Code must decide: **Feature-rich or minimalist?**

**Pi's answer**: Minimalism via configurability. Let users choose their complexity.

**Recommendation for VT Code**:

```toml
# Preset configurations
[presets.minimal]  # Pi-inspired
system_prompt = "minimal"
tools = ["read_file", "write_file", "edit_file", "run_pty_cmd"]
security = "yolo"
mcp_enabled = false

[presets.balanced]  # Current VT Code (improved)
system_prompt = "default"
tools = "all_builtin"
security = "workspace"
mcp_enabled = true
progressive_loading = true

[presets.paranoid]  # Maximum safety
system_prompt = "specialized"
tools = "all_with_confirmations"
security = "paranoid"
human_in_the_loop = true
```

## Implementation Roadmap

**Week 1**: P0 items (minimal prompt, progressive loading)
**Week 2**: P1 items (split results, MCP analysis)
**Week 3**: Testing & benchmarking
**Week 4**: Documentation & rollout

## References

- Pi mono repo: https://github.com/badlogic/pi-mono
- Terminal-Bench: https://github.com/laude-institute/terminal-bench
- Blog post: https://mariozechner.at/posts/2025-11-30-pi-coding-agent/
- Armin Ronacher on agents: https://lucumr.pocoo.org/2025/11/21/agents-are-hard/
- Simon Willison on dual LLM: https://simonwillison.net/2023/Apr/25/dual-llm-pattern/

## Conclusion

Pi proves that **less is more**. VT Code's Rust architecture and trait system position it perfectly to offer **user choice**: minimal for power users, full-featured for newcomers.

The path forward: **Configurable minimalism** > forced complexity.
