# AGENTS.md

This file provides guidance to VT Code coding agent, when working with code in this repository. This file is the **entry-point map** for agents working on VT Code. Deep knowledge lives in `docs/` — this file tells you WHERE to look, not WHAT to do.

## Personality & Communication

Default personality and tone when working on VT Code:

- **Concise and direct**: Minimize output; answer specifically without elaboration or flattery
- **Actionable**: Always prioritize next steps, assumptions, and clear guidance
- **Efficient**: Avoid verbose explanations unless explicitly requested
- **Collaborative**: Work like a knowledgeable teammate; share momentum and progress

Example: Instead of "That's a great question about the architecture," jump directly to the analysis or answer.

## Workspace Structure

VT Code uses an **11-member workspace** architecture:

```
vtcode/                          # Binary entrypoint (src/main.rs)
├── vtcode-core/                 # Core library (LLM, tools, config, MCP)
├── vtcode-config/               # Configuration loader & schema
├── vtcode-commons/              # Shared utilities
├── vtcode-llm/                  # LLM provider abstractions
├── vtcode-tools/                # Tool implementations
├── vtcode-bash-runner/          # Shell execution engine
├── vtcode-markdown-store/       # Document storage
├── vtcode-indexer/              # Code indexing
├── vtcode-exec-events/          # Event definitions
├── vtcode-acp-client/           # Agent Client Protocol bridge
└── vtcode-process-hardening/    # Process hardening & security measures
```

**Key separation**:

- **vtcode-core/**: Reusable library with 77% complexity reduction through mode-based execution
- **src/**: CLI executable (Ratatui TUI, PTY, slash commands)

## Architecture Highlights

Understanding these patterns requires reading multiple files across the codebase:

### Multi-Provider LLM System

- **Location**: `vtcode-llm/`, `vtcode-core/src/llm/`
- **Pattern**: Factory pattern with provider-specific request shaping
- **Providers**: OpenAI, Anthropic, Gemini, xAI, DeepSeek, OpenRouter, Ollama, Z.AI, Moonshot AI, MiniMax
- **Features**: Automatic failover, prompt caching, token budget tracking

### Trait-Based Tool System

- **Location**: `vtcode-tools/`, `vtcode-core/src/tools/`
- **Pattern**: Trait-driven composition (`Tool`, `ModeTool`, `CacheableTool`)
- **Key**: Single source of truth for content search (grep_file) and file operations
- **Tools**: 54+ specialized handlers with workspace-scoped isolation
- **Unified Tools**: `unified_exec` (shell), `unified_file` (file ops), `unified_search` (discovery) provide a minimal, high-efficiency interface for agents.

### Configuration Precedence

**Critical**: Configuration flows in this order:

1. Environment variables (API keys, overrides)
2. `vtcode.toml` (runtime configuration)
3. `vtcode-core/src/config/constants.rs` (code constants)

### PTY Session Management

- **Location**: `vtcode-core/src/exec/`, `vtcode-bash-runner/`
- **Pattern**: Interactive shell sessions with streaming output
- **Use**: Long-running commands, interactive workflows, real-time feedback

### Tree-Sitter Integration

- **Location**: `vtcode-core/src/tree_sitter/`, `vtcode-indexer/`
- **Languages**: Rust, Python, JavaScript/TypeScript, Go, Java, Bash (+ optional Swift)
- **Pattern**: Incremental AST building with caching for semantic code analysis

### Code Intelligence Tool

- **Location**: `vtcode-core/src/tools/code_intelligence.rs`
- **Purpose**: Code navigation using tree-sitter
- **Operations**: `goto_definition`, `find_references`, `hover`, `document_symbol`, `workspace_symbol`
- **Usage**: `{"operation": "goto_definition", "file_path": "src/main.rs", "line": 42, "character": 15}`

### Protocol Integrations

**ACP** (Agent Client Protocol):

- **Location**: `vtcode-acp-client/` - Zed IDE integration
- **Purpose**: Enable VT Code to run as agent within Zed editor

**A2A** (Agent2Agent Protocol):

- **Discovery**: Agent Card at `/.well-known/agent-card.json`
- **Features**: Task lifecycle states (`submitted`/`working`/`completed`/`failed`), SSE streaming, JSON-RPC 2.0 over HTTP, push notifications
- **Documentation**: `docs/a2a/a2a-protocol.md`

**MCP** (Model Context Protocol):

- **Location**: `vtcode-core/src/mcp/` (9 modules) - Extensible tooling via `rmcp`
- **Key Components**:
    - `McpClient`: Main high-level client managing multiple providers
    - `McpProvider`: Individual provider connection and lifecycle
    - `McpToolExecutor`: Trait interface for tool registry integration
    - `rmcp_transport.rs`: Supports stdio, HTTP, and child-process transports
- **Configuration**:
    - Project: `.mcp.json` (checked into source control)
    - Runtime: `vtcode.toml` with `[mcp]` section
- **Features**:
    - Tool discovery and execution with allowlist enforcement
    - Resource and prompt management from MCP providers
    - Security validation (argument size, path traversal, schema)
    - OAuth 2.0 authentication support
    - Event notifications (logging, progress, resource updates)
    - Per-provider concurrency control with semaphores
    - Timeout management (startup and per-tool)
- **Transports**:
    - **Stdio**: Local tool execution (development, CLI tools)
    - **HTTP**: Remote server integration (requires `experimental_use_rmcp_client = true`)
    - **Child Process**: Managed stdio with lifecycle control
- **Documentation**:
    - Integration guide: `docs/mcp/MCP_INTEGRATION_GUIDE.md`
    - Implementation roadmap: `docs/mcp/MCP_ROADMAP.md`

### Agent Skills

- **Pattern**: Multi-location discovery with precedence — project `.vtcode/skills/` → user `~/.vtcode/skills/` → embedded resources
- **Standard**: Compliant with the [agentskills.io](http://agentskills.io/) open standard for interoperability
- **Documentation**: `docs/skills/SKILLS_GUIDE.md`, `docs/skills/AGENT_SKILLS_SPEC_IMPLEMENTATION.md`

### Subagent System

- **Location**: `vtcode-core/src/subagents/`, `vtcode-config/src/subagent.rs`
- **Purpose**: Delegate tasks to specialized agents with isolated context
- **Built-in**: `explore` (haiku, read-only), `plan` (sonnet, research), `general` (sonnet, full), `code-reviewer`, `debugger`
- **Tool**: `spawn_subagent` - params: `prompt`, `subagent_type`, `resume`, `thoroughness`, `parent_context`
- **Custom Agents**: Define in `.vtcode/agents/` (project) or `~/.vtcode/agents/` (user) as Markdown with YAML frontmatter
- **Documentation**: `docs/subagents/SUBAGENTS.md`

### Process Hardening

- **Location**: `vtcode-process-hardening/` (dedicated crate)
- **Purpose**: Apply security hardening measures before the main binary executes
- **Pattern**: Pre-main execution using `#[ctor::ctor]` constructor decorator
- **Features**:
    - **Linux/Android**: `PR_SET_DUMPABLE` (ptrace disable), `RLIMIT_CORE` (disable core dumps), `LD_*` env var removal
    - **macOS**: `PT_DENY_ATTACH` (debugger prevention), `RLIMIT_CORE`, `DYLD_*` env var removal
    - **BSD**: `RLIMIT_CORE`, `LD_*` env var removal
    - **Windows**: Placeholder for future mitigation policies
- **Key Detail**: Uses `std::env::vars_os()` to handle non-UTF-8 environment variables correctly
- **Exit Codes**: 5 (prctl), 6 (ptrace), 7 (setrlimit) indicate hardening failures
- **Documentation**: `docs/development/PROCESS_HARDENING.md`
- **Integration**: Called via `#[ctor::ctor]` in `src/main.rs:init()`

**Working Directory Context**:

- Explicit workspace path in system prompt
- Configuration: `agent.include_working_directory = true` (default)
- Overhead: ~10 tokens

**Implementation**: System prompt composition happens in `vtcode-core/src/prompts/system.rs` with PromptContext built in `src/agent/runloop/unified/prompts.rs`. See unit tests in `vtcode-core/src/prompts/system.rs` (tests module) for usage examples.

## Code Style & Conventions

### Critical Standards from .github/copilot-instructions.md

**Error Handling**:

```rust
// ALWAYS use anyhow::Result<T> with context
use anyhow::{Context, Result};

pub async fn read_config(path: &str) -> Result<Config> {
    tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read config at {}", path))?;
    // Process...
}

// NEVER use unwrap()
// ❌ let data = result.unwrap();
// ✅ let data = result.with_context(|| "Description")?;
```

**Constants & Configuration**:

```rust
// NEVER hardcode values (especially model IDs)
// ❌ let model = "gpt-4";
// ❌ let timeout = 30;

// ✅ Use constants from vtcode-core/src/config/constants.rs
use vtcode_core::config::constants::DEFAULT_TIMEOUT;

// ✅ Read from vtcode.toml at runtime
let config = Config::load("vtcode.toml")?;

// ✅ Model metadata in docs/models.json
```

**Naming Conventions**:

- `snake_case` for functions and variables
- `PascalCase` for types and structs
- Descriptive names, early returns
- 4-space indentation

**Documentation**:

- All `.md` files go in `./docs/` directory (NOT in root)
- `README.md` is the only exception (stays in root)
- Use `docs/models.json` for latest LLM model metadata

### Documents Orgranization

- `docs/ARCHITECTURE.md`: High-level architecture overview
- `docs/security/SECURITY_MODEL.md`: Security design and threat model
- `docs/config/CONFIGURATION_PRECEDENCE.md`: Configuration loading order and best practices
- `docs/providers/PROVIDER_GUIDES.md`: Setup guides for LLM providers
- `docs/development/testing.md`: Testing strategy and infrastructure
- All docs should be in a respective subdirectory under `docs/` for organization and discoverability.

## Testing Infrastructure

### Test Organization

- **Unit tests**: Inline with source in `#[cfg(test)]` modules
- **Integration tests**: `tests/` directory (20+ test files)
- **Benchmarks**: `benches/` directory
- **Mock data**: `tests/mock_data.rs` for realistic scenarios

### Build & Test Commands

```bash
# Build
cargo build                         # Full build
cargo check                         # Fast compile check (no codegen)

# Testing (preferred: cargo-nextest is 3-5x faster)
cargo nextest run                   # All tests
cargo nextest run -p vtcode-core    # Single package
cargo t                             # Alias for nextest run
cargo tq                            # Quick profile (no retries)
cargo ts                            # Fallback if nextest unavailable

# Integration tests
cargo nextest run --test integration_tests

# Benchmarks
cargo bench
cargo bench -- search_benchmark

# With output
cargo nextest run -- --nocapture

# Quality gate (run before committing)
cargo clippy && cargo fmt --check && cargo check && cargo nextest run
```

## Important Development Notes

### Security & Safety

- Validate all file paths (workspace boundary enforcement)
- Command allowlist with per-argument validation
- Tool policies (allow/deny/prompt) in `vtcode.toml`
- Human-in-the-loop approval system

### Memory & Performance

- Single codegen unit for better optimization
- Strict Clippy linting rules (see `Cargo.toml` workspace.lints)
- No `expect_used`, `unwrap_used`, or manual implementations when stdlib has them

### Key Files Never to Hardcode

- **Model IDs**: Use `docs/models.json`
- **Constants**: Use `vtcode-core/src/config/constants.rs`
- **Config values**: Read from `vtcode.toml`

### Common Pitfalls

1. **Don't hardcode model names** - they change frequently, use constants
2. **Don't use `unwrap()`** - use `.with_context()` for error context
3. **Don't create .md files in root** - they belong in `./docs/`
4. **Don't modify constants directly** - consider if it should be in `vtcode.toml` instead
5. **Don't ignore Clippy warnings** - fix them or explain why in code comments
6. **Don't repeat yourself** - extract common logic into functions or modules
7. **Don't assume workspace paths** - always validate and sanitize file paths
8. **Don't skip tests** - add tests for new features and bug fixes
9. **Don't overcomplicate** - prefer simple, clear solutions over clever ones
10. **Don't forget to run quality checks** - always run `cargo clippy`, `cargo fmt`, `cargo check`, and `cargo nextest` before committing
11. **Don't assume `RwLock` is faster** - for tiny cache/read paths, `Mutex` can outperform `RwLock` due to reader-counter atomic contention; benchmark under realistic concurrency before choosing

## Agent Execution Guidelines

### Task Execution Philosophy

When working on VT Code features:

- **Complete autonomously**: Resolve tasks fully before yielding back to user—do not ask for confirmation on intermediate steps
- **Root cause fixes**: Fix problems at their source rather than applying surface-level patches
- **Verification responsibility**: Run `cargo check`, `cargo nexttest`, `cargo clippy` after making changes—don't ask the user to run these
- **Precision over ambition**: In existing codebases, make surgical changes that respect surrounding code style; avoid unnecessary refactoring
- **Avoid unrelated fixes**: Do not fix bugs or test failures outside your task scope (you may mention them in final output)
- **Keep working**: Do not terminate or ask permission to continue on valid tasks. Proactively resolve issues within constraints.

### Responsiveness & Momentum

**Preamble Messages** (before tool calls):

Concise, action-oriented notes that show progress and build momentum:

- **Length**: 1–2 sentences max (8–12 words ideal for quick updates)
- **Grouping**: Logically group related actions—don't send a note for every individual command
- **Context building**: Show what you've done so far and what's next (creates continuity)
- **Tone**: Friendly and collaborative; add small touches of personality
- **Exception**: Skip preambles for trivial single-file reads unless part of larger work

Examples:

- "I've explored the repo structure; now checking the LLM provider factory."
- "Config loads correctly. Next, patching the tool registry and related tests."
- "Spotted the issue in the cache layer; now hunting where it gets used."

**Progress Updates** (for longer tasks):

For work spanning multiple tool calls or complex planning, send brief progress updates (1–2 sentences, 8–10 words) at reasonable intervals:

- "Finished analyzing tool trait definitions; implementing new code_intelligence operation."
- "Tests passing for core logic; now verifying integration points."

### Final Answers

**Structure and Format**:

- **Lead with outcomes**: State what was accomplished before describing how
- **Assume accessibility**: User has direct access to your changes—no need to repeat full file contents
- **Brevity first**: Aim for 10 lines or fewer; expand only when critical for understanding
- **Let content speak**: Avoid unnecessary explanations or summaries

**Formatting Guidelines**:

- **Headers**: Use only when they improve clarity (1–3 words, Title Case). Leave no blank line before first bullet.
- **Bullets**: Use `-` prefix; keep one-line where possible; group related items; order by importance (4–6 bullets max per section)
- **Monospace**: Wrap commands, file paths, env vars, code identifiers in backticks (`` ` ``)
- **File references**: Include paths with optional line numbers (e.g., `src/main.rs:42`, `vtcode-core/src/llm/mod.rs:10`); no ranges or URIs
- **Tone**: Natural and conversational—like a teammate handing off completed work

**Don't**:

- Don't output inline citations (broken in CLI rendering)
- Don't repeat the plan after calling `update_plan` (already displayed)
- Don't use nested bullets or deep hierarchies
- Don't cram unrelated keywords into single bullets—split for clarity

### Planning (update_plan tool)

Use plans for non-trivial, multi-step work:

- **When to use**: Tasks requiring 4+ steps, logical dependencies, ambiguity that benefits from outlining
- **When to skip**: Simple queries, single-step changes, or anything you can resolve immediately
- **Quality**: Avoid filler steps; don't state the obvious; structure as 5–7 word descriptive steps
- **During execution**: Mark steps `completed` as you finish them; keep exactly one step `in_progress`
- **Updates**: If scope changes mid-task, call `update_plan` with rationale explaining why
- **Final step**: Once complete, mark all steps as `completed` and do not repeat the plan in your output

High-quality plan example:

1. Read existing tool trait definitions
2. Add new operation to code_intelligence
3. Update tool registry and tests
4. Verify with end-to-end test
5. Update docs/ARCHITECTURE.md

### Work Completion

- **Autonomy**: Complete tasks fully before yielding; do not ask for confirmation on intermediate steps
- **Iteration**: If feedback or errors arise, fix them proactively and iterate up to reasonable limits
- **Scope boundary**: Don't overstep into unrelated work, but do resolve all aspects of the requested task
- **Timeboxing**: For ambiguous or open-ended tasks, use judgment to decide when "done" is sufficient
- **User context**: The user is working on the same machine—no need for setup instructions or file content restatement

### Tool Use Guidelines

**Search and File Exploration**:

- Prefer `unified_search` with `action="grep"` for fast, focused searches
- Use `unified_search` with `action="list"` for directory exploration
- Use `unified_search` with `action="intelligence"` for code navigation (definitions, references)
- Use `unified_search` with `action="tools"` to discover available tools and skills
- Use `unified_search` with `action="errors"` for diagnostic information
- Use `unified_search` with `action="agent"` for system state and available tools
- When reading files, read the complete file once; don't re-invoke `Read` on the same file

**Code Modification**:

- Use `unified_file` with `action="edit"` for surgical changes to existing code
- Use `unified_file` with `action="write"` to replace entire file contents
- Use `unified_file` with `action="create"` for new files
- Use `unified_exec` for shell commands and interactive PTY sessions
- After applying patches or creating files, don't re-read to verify—the tool will fail if it didn't work
- Never use `git commit` or `git push` unless explicitly requested
- Use `git log` and `git blame` for code history when additional context is needed

**Testing and Validation**:

- Run specific tests first (`cargo nexttest function_name`), then broaden to related suites
- When test infrastructure exists, use it proactively; don't ask the user to run tests
- For non-interactive modes (approval never), run tests and validation yourself before yielding
- For interactive modes, suggest what to validate next rather than running lengthy test suites proactively

### Validation & Testing

- **Test strategy**: Start specific to code you changed, then broaden to related tests
- **When test infrastructure exists**: Use it proactively to verify your work
- **When no tests exist**: Don't add tests to codebases with no test patterns
- **Formatting**: If codebase has a formatter, use it; if issues persist after 3 iterations, present the correct solution and note formatting in final message
- **Linting**: Run `cargo clippy` after changes; address warnings in scope of your task

## Development Workflow

### Before Committing

```bash
# Run all quality checks
cargo clippy && cargo fmt --check && cargo check && cargo nexttest
```

### Adding New Features

1. Read existing code patterns first
2. Use configuration from `vtcode.toml` when possible
3. Add constants to `vtcode-core/src/config/constants.rs` if needed
4. Write tests (unit + integration)
5. Update documentation in `./docs/` if needed

## Self-Documentation

When answering questions about VT Code itself, consult `docs/modules/vtcode_docs_map.md` first to locate canonical references before answering.

## Additional Resources

- **Architecture**: See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- **Security**: See [docs/security/SECURITY_MODEL.md](docs/security/SECURITY_MODEL.md)
- **Contributing**: See [CONTRIBUTING.md](docs/CONTRIBUTING.md)
- **Configuration**: See [docs/config/CONFIGURATION_PRECEDENCE.md](docs/config/CONFIGURATION_PRECEDENCE.md)
- **Provider Setup**: See [docs/providers/PROVIDER_GUIDES.md](docs/providers/PROVIDER_GUIDES.md)
- **Testing Guide**: See [docs/development/testing.md](docs/development/testing.md)
