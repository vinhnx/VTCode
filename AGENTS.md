# AGENTS.md

This file provides guidance to agents (both AI and human) working with VT Code.

## Design Philosophy: Desire Paths

When agents intuitively guess wrong about a command, flag, or workflow, we treat it as a signal to **improve the interface** rather than just documenting correct behavior. We "pave the desire paths" by adding aliases, flags, and shortcuts that match intuitive expectations. Over time, this compounds—agents stop making mistakes naturally, and the tool becomes easier to use.

## ExecPlans

When writing complex features or significant refactors, use an ExecPlan (as described in `.vtcode/PLANS.md`) from design to implementation. ExecPlans are self-contained, living design documents that enable a complete novice to implement a feature end-to-end. They include mandatory sections for progress tracking, decision logging, and retrospectives.

## Build & Test Commands

```bash
# Preferred development workflow
cargo check                          # Fast compile check
cargo nextest                       # Run tests
cargo nextest --package vtcode-core    # Test specific package
cargo clippy                         # Lint with strict rules
cargo fmt                           # Format code

# Additional commands
cargo nextest -- --nocapture           # Tests with output
cargo bench                          # Run performance benchmarks
cargo build                          # Build the project
cargo run -- ask "Hello world"      # Run VT Code CLI

# Run a single test
cargo nextest test_name -- --nocapture

# Aliases & Quick Commands
cargo t                              # Run tests (nextest)
cargo tq                             # Quick tests (10s timeout)
cargo tc                             # CI profile tests (with retries)
cargo c                              # Check compilation
cargo ca                             # Check all targets
cargo cl                             # Clippy lint
cargo b                              # Build dev
cargo brf                            # Build release-fast (thin LTO, ~3x faster)
cargo br                             # Build release (full LTO, production)
```

### Build Performance

**sccache** is configured for compilation caching. Current performance:

| Command                              | Cold Build | Incremental |
| ------------------------------------ | ---------- | ----------- |
| `cargo check`                        | ~3:20      | ~3s         |
| `cargo clippy --all-targets`         | ~3:30      | ~36s        |
| `cargo build`                        | ~3:20      | ~3s         |
| `cargo build --profile release-fast` | ~3:10      | ~48s        |
| `cargo build --release`              | ~6:30      | ~6:30       |

**Tips**:

- Use `cargo brf` (release-fast) for quick optimized builds during development
- Use `cargo br` (release) only for final production builds
- sccache provides ~3x speedup on subsequent clean builds

### Release Commands

```bash
# Dry-run release (test first!)
./scripts/release.sh --patch --dry-run

# Actual patch release
./scripts/release.sh --patch

# Minor/major releases
./scripts/release.sh --minor
./scripts/release.sh --major

# Custom version
./scripts/release.sh 1.2.3

# With options
./scripts/release.sh --patch --skip-binaries --skip-crates
```

**Release Process**:

1. Triggers version bump and crate publishing
2. Creates git tags (`v0.58.3`)
3. GitHub Actions automatically:
    - Builds binaries for all platforms
    - Uploads to GitHub Releases
    - Updates Homebrew formula with checksums
    - Commits and pushes formula changes

See: `docs/HOMEBREW_RELEASE_GUIDE.md` for troubleshooting.

### Desire Path Commands (Paved for Agents)

These shortcuts match intuitive agent expectations (defined in `.cargo/config.toml`):

**Testing:**
- `cargo t` → `cargo nextest run`
- `cargo tq` → `cargo nextest run --profile quick`
- `cargo tc` → `cargo nextest run --profile ci`
- `cargo tp <pkg>` → `cargo nextest run --package <pkg>`

**Build & Check:**
- `cargo c` → `cargo check`
- `cargo ca` → `cargo check --all-targets`
- `cargo cb` → `cargo check --all-targets --all-features`
- `cargo cw` → `cargo check --workspace`

**Clippy:**
- `cargo cl` → `cargo clippy --all-targets`
- `cargo cla` → `cargo clippy --all-targets --all-features`
- `cargo clf` → `cargo clippy --fix --allow-dirty --allow-staged`

**Build:**
- `cargo b` → `cargo build`
- `cargo brf` → `cargo build --profile release-fast` (thin LTO, ~3x faster)
- `cargo br` → `cargo build --release` (full LTO, production)

**Run:**
- `cargo r` → `cargo run`
- `cargo rrf` → `cargo run --profile release-fast` (fast optimized run)
- `cargo rr` → `cargo run --release` (full optimized run)

**Utility:**
- `cargo f` → `cargo fmt`
- `cargo fc` → `cargo fmt --check`
- `cargo d` → `cargo doc --open`
- `cargo ud` → `cargo update --dry-run`

**Test patterns:**
- `cargo nextest function_name` ✓ (this works)
- `cargo nextest --lib` ✓ (unit tests only)
- `cargo nextest --integration` ✓ (integration tests only)

### Development Scripts

```bash
# Fast debug run (unoptimized, fastest compile)
./scripts/run-debug.sh

# Fast optimized run (release-fast profile, ~3x faster than full release)
cargo rrf -- --show-file-diffs

# Full release run (production quality)
cargo rr -- --show-file-diffs
```

## Workspace Structure

VT Code uses a **11-member workspace** architecture:

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

### Code Intelligence Tool

- **Location**: `vtcode-core/src/tools/code_intelligence.rs`
- **Purpose**: Code navigation using tree-sitter
- **Operations**:
    - `goto_definition`: Find where a symbol is defined
    - `find_references`: Find all references to a symbol
    - `hover`: Get documentation and type info for a symbol
    - `document_symbol`: Get all symbols in a file
    - `workspace_symbol`: Search for symbols across the workspace
- **Languages**: Rust, Python, JavaScript, TypeScript, Go, Java, Bash, Swift
- **Usage**: Call `code_intelligence` tool with operation, file_path, line, and character parameters

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

### Protocol Integrations

- **ACP** (Agent Client Protocol): `vtcode-acp-client/` - Zed IDE integration
- **MCP** (Model Context Protocol): `vtcode-core/src/mcp/` - Extensible tooling via `rmcp`
    - **Location**: `vtcode-core/src/mcp/` (9 modules)
    - **Key Components**:
        - `McpClient`: Main high-level client managing multiple providers
        - `McpProvider`: Individual provider connection and lifecycle
        - `McpToolExecutor`: Trait interface for tool registry integration
        - `rmcp_transport.rs`: Supports stdio, HTTP, and child-process transports
    - **Configuration**:
        - Project: `.mcp.json` (checked into source control)
        - User: `~/.claude.json` (cross-project personal utilities)
        - Runtime: `vtcode.toml` with `[mcp]` section
    - **Features**:
        - Tool discovery and execution with allowlist enforcement
        - Resource and prompt management from MCP providers
        - Security validation (argument size, path traversal, schema)
        - OAuth 2.0 authentication support
        - Event notifications (logging, progress, resource updates)
        - Elicitation handling for user interaction
        - Per-provider concurrency control with semaphores
        - Timeout management (startup and per-tool)
    - **Transports**:
        - **Stdio**: Local tool execution (development, CLI tools)
        - **HTTP**: Remote server integration (requires `experimental_use_rmcp_client = true`)
        - **Child Process**: Managed stdio with lifecycle control
    - **Documentation**:
        - Integration guide: `docs/MCP_INTEGRATION_GUIDE.md` (current implementation)
        - Improvement designs: `docs/MCP_IMPROVEMENTS.md` (planned enhancements)
        - Implementation roadmap: `docs/MCP_ROADMAP.md` (detailed next steps)

### Subagent System

- **Location**: `vtcode-core/src/subagents/`, `vtcode-config/src/subagent.rs`
- **Purpose**: Delegate tasks to specialized agents with isolated context
- **Built-in Subagents**: `explore` (haiku, read-only), `plan` (sonnet, research), `general` (sonnet, full), `code-reviewer`, `debugger`
- **Tool**: `spawn_subagent` with params: `prompt`, `subagent_type`, `resume`, `thoroughness`, `parent_context`
- **Custom Agents**: Define in `.vtcode/agents/` (project) or `~/.vtcode/agents/` (user) as Markdown with YAML frontmatter
- **Documentation**: `docs/subagents/SUBAGENTS.md`

### Execution Policy System (Codex Patterns)

- **Location**: `vtcode-core/src/exec_policy/`, `vtcode-core/src/sandboxing/`
- **Purpose**: Command authorization and sandboxed execution inspired by OpenAI Codex
- **Key Components**:
    - `ExecPolicyManager`: Central coordinator for policy evaluation
    - `SandboxPolicy`: Isolation levels (ReadOnly, WorkspaceWrite, DangerFullAccess)
    - `SandboxManager`: Platform-specific transforms (macOS Seatbelt, Linux Landlock)
    - `ExecApprovalRequirement`: Skip, NeedsApproval, Forbidden outcomes
- **Policy Features**:
    - Prefix-based rule matching for command authorization
    - Heuristics for unknown commands (safe: ls, cat; dangerous: rm, sudo)
    - Session-scoped approval caching
    - Policy amendments for trusted patterns
- **Turn Diff Tracking**: `TurnDiffTracker` aggregates file changes across patches
- **Tool Trait Extensions**: `is_mutating()`, `is_parallel_safe()`, `kind()`, `matches_kind()`

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
- **Documentation**: `docs/PROCESS_HARDENING.md`
- **Integration**: Called via `#[ctor::ctor]` in `src/main.rs:init()`

### Agent Behavior Configuration (Codex-Inspired)

- **Location**: `vtcode-config/src/core/agent.rs`, `vtcode-config/src/types/mod.rs`
- **Purpose**: Provider-agnostic agent behavior patterns inspired by OpenAI Codex prompting guide
- **Key Components**:
    - `EditingMode`: Enum (`Edit`, `Plan`) - controls file modification
        - `Edit`: Full tool access (default)
        - `Plan`: Read-only exploration, mutating tools blocked
    - `autonomous_mode`: Boolean flag - controls HITL behavior independently
        - When true: auto-approves safe tools with reduced confirmation prompts
    - `require_plan_confirmation`: Human-in-the-loop approval before executing plans
- **Configuration in vtcode.toml**:
    ```toml
    [agent]
    default_editing_mode = "edit"  # "edit" or "plan"
    autonomous_mode = false  # Enable for autonomous operation with reduced HITL
    require_plan_confirmation = true  # Require user approval before executing plans
    ```
- **Design**: These patterns work with all providers (Gemini, Anthropic, OpenAI, xAI, DeepSeek, etc.)
- **System Prompts**: See `vtcode-core/src/prompts/system.rs` for Codex-aligned prompts (v5.2)

### Context Window Management

- **Location**: `src/agent/runloop/unified/context_manager.rs`, `vtcode-config/src/constants.rs`
- **Purpose**: Proactive token budget tracking based on Anthropic context window documentation
- **Key Components**:
    - `ContextManager`: Tracks token usage and manages context limits
    - `TokenBudgetStatus`: Enum (Normal, Warning, High, Critical)
    - `IncrementalSystemPrompt`: Injects context awareness for supported models
- **Context Window Sizes**:
    - Standard: 200K tokens (all models)
    - Enterprise: 500K tokens (Claude.ai Enterprise)
    - Extended: 1M tokens (beta, Claude Sonnet 4/4.5, tier 4 only)
- **Token Budget Thresholds**:
    - 70% (Warning): Start preparing for context handoff
    - 85% (High): Active context management needed
    - 90% (Critical): Force context handoff or summary
- **Context Awareness** (Claude 4.5+):
    - Models track remaining token budget throughout conversation
    - System prompt includes `<budget:token_budget>` and `<system_warning>` tags
    - Supported models: Claude Sonnet 4.5, Claude Haiku 4.5
- **Extended Thinking**:
    - Thinking tokens are stripped from subsequent turns automatically
    - Minimum budget: 1,024 tokens; Recommended: 10,000+ tokens
    - When using with tool use, thinking blocks must be preserved until cycle completes
- **Documentation**: `docs/CONTEXT_WINDOWS.md`

## Communication Style

### Response Guidelines

- **No emoji**: Never use emojis in responses. Keep output professional and text-based.
- **Minimize exclamation points**: Use them sparingly; let the content speak for itself.
- **Be concise**: Answer directly without unnecessary preamble, elaboration, or summaries.
- **Avoid flattery**: Don't call ideas "good," "great," "interesting," or other positive adjectives. Respond directly to the request.
- **Focus on the task**: Only address the user's specific query. Skip tangential information unless critical.
- **One-to-three sentences**: Aim for brevity whenever possible. One-word answers are preferred for simple questions.
- **No long introductions**: Get to the point immediately.
- **Clean markdown**: Format responses with GitHub-flavored Markdown where appropriate.

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

**TUI Logging (Critical)**:

- **NEVER use `println!` or `eprintln!` in TUI-active code paths**
- The TUI uses `CrosstermBackend` on stderr—direct prints corrupt the display
- Use `tracing::debug!`, `tracing::info!`, `tracing::warn!`, or `tracing::error!` instead
- Logs go to `/tmp/vtcode-debug.log` when TUI is active
- Acceptable exceptions:
    - Pre-TUI startup errors in `main.rs` (before TUI initializes)
    - `vtcode-process-hardening` (runs before main, can't use tracing)
    - CLI-only commands in `src/cli/` (run without TUI)

```rust
// ❌ NEVER in TUI code
eprintln!("Warning: something went wrong");
println!("Debug: value = {}", x);

// ✅ ALWAYS use tracing
tracing::warn!("Something went wrong");
tracing::debug!("value = {}", x);
```

## Testing Infrastructure

### Test Organization

- **Unit tests**: Inline with source in `#[cfg(test)]` modules
- **Integration tests**: `tests/` directory (20+ test files)
- **Benchmarks**: `benches/` directory
- **Mock data**: `tests/mock_data.rs` for realistic scenarios

### Running Specific Tests

```bash
# All tests
cargo nextest

# Integration tests
cargo nextest --test integration_tests

# Benchmarks
cargo bench
cargo bench -- search_benchmark

# With output
cargo nextest -- --nocapture
```

## Agent UX & Desire Paths

This section tracks UX friction points that agents intuitively expect vs. how the system actually works.

### Philosophy: Pave the Desire Paths

When agents guess wrong about commands, flags, or workflows, we treat that as a signal to **improve the interface** rather than just documenting the correct behavior. Over time, small UX improvements compound—agents stop making "mistakes" naturally.

### Active Patterns

**Cargo alias expectations**:

- Agents try `cargo t` expecting `cargo nextest` → ✓ Paved with cargo aliases
- Agents try `cargo c` expecting `cargo check` → ✓ Paved with cargo aliases
- Agents try `cargo r` expecting `cargo run` → ✓ Paved with cargo aliases

**Tool operation patterns**:

- Agents expect `code_intelligence goto_definition` → Unified under `unified_search` with `action="intelligence"`
- Agents expect `spawn_subagent --name explore` → Currently requires positional `subagent_type` param
- Agents expect `exec_command` and `write_stdin` (Unified Exec) → ✓ Paved with `unified_exec` (compatible with OpenAI Codex pattern)
- Agents expect `read_file`, `write_file`, `edit_file` → ✓ Unified under `unified_file` with `action` param
- Agents expect `grep_file`, `list_files`, `search_tools`, `get_errors`, `agent_info` → ✓ Unified under `unified_search` with `action` param (`grep`, `list`, `intelligence`, `tools`, `errors`, `agent`)

### Reporting Friction

If you (agent or human) notice a repeated "wrong" guess that makes intuitive sense, document it here before implementing the improvement.

## Important Development Notes

### Git Operations

**Agent Must Not Perform Git Operations Automatically**

- Do NOT run `git commit`, `git push`, `git merge`, or any destructive git operations on behalf of the user
- Inform the user when changes need to be committed and let them handle git operations
- Use `git status`, `git diff`, and `git log` only for diagnostic/informational purposes
- Always inform the user before making changes that would affect git history

### Security & Safety

- Validate all file paths (workspace boundary enforcement)
- Command allowlist with per-argument validation
- Tool policies (allow/deny/prompt) in `vtcode.toml`
- Human-in-the-loop approval system

### Autonomy & Verification

- **Verification Autonomy**: The agent MUST run verification commands (`cargo check`, `cargo nextest`, etc.) itself using `run_pty_cmd` after making changes. Do NOT ask the user to run these commands.
- **Planning**: Use `update_plan` for any task requiring 4+ steps to maintain state and provide visibility to the user.

### Memory & Performance

- LTO enabled even in dev profile (optimized for M4 Apple Silicon)
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

## Development Workflow

### Before Committing

```bash
# Run all quality checks
cargo clippy && cargo fmt --check && cargo check && cargo nextest
```

### Adding New Features

1. Read existing code patterns first
2. Use configuration from `vtcode.toml` when possible
3. Add constants to `vtcode-core/src/config/constants.rs` if needed
4. Write tests (unit + integration)
5. Update documentation in `./docs/` if needed

### Environment Setup

```bash
# Set API key for your preferred provider
export OPENAI_API_KEY="sk-..."      # OpenAI
export ANTHROPIC_API_KEY="sk-..."   # Anthropic
export GEMINI_API_KEY="..."         # Google Gemini
# See docs/installation/ for all provider options

# Run VT Code
cargo run
```

## Additional Resources

- **Architecture**: See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- **Security**: See [docs/SECURITY_MODEL.md](docs/SECURITY_MODEL.md)
- **Contributing**: See [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md)
- **Configuration**: See [docs/config/CONFIGURATION_PRECEDENCE.md](docs/config/CONFIGURATION_PRECEDENCE.md)
- **Provider Setup**: See [docs/PROVIDER_GUIDES.md](docs/PROVIDER_GUIDES.md)
- **Testing Guide**: See [docs/development/testing.md](docs/development/testing.md)

## Agentic Patterns (From The Agentic AI Handbook)

VT Code implements production-ready agentic patterns from [The Agentic AI Handbook](https://www.nibzard.com/agentic-handbook/) by Nikola Balić. These patterns are battle-tested solutions for building reliable AI agents.

### Core Patterns Implemented

#### 1. Plan-Then-Execute Pattern

VT Code's Plan Mode implements this pattern with two phases:

1. **Plan phase**: Agent generates a fixed sequence of actions before seeing untrusted data
2. **Execute phase**: Controller runs the exact sequence. Tool outputs shape parameters, not which tools run

**Configuration**:

```toml
[agent]
default_editing_mode = "plan"     # Start in plan mode
require_plan_confirmation = true  # HITL before execution
```

**Usage**: Enter plan mode with `/plan` or `Shift+Tab`. Agent explores read-only and writes plan to `.vtcode/plans/`. User approves before execution.

#### 2. Inversion of Control

VT Code gives agents **tools + goals** rather than step-by-step instructions. This is the foundation of the subagent system.

**Example**:

```markdown
Instead of: "Read file X, extract Y, check Z, then update..."

Use: "Refactor UploadService to use async patterns. You have tools to read files, run tests, and make edits."
```

**Implementation**: See `vtcode-core/src/subagents/` for the subagent registry and runner.

#### 3. Spectrum of Control (Blended Initiative)

VT Code supports fluid control transfer between human and agent:

- **Human-led**: Human directs, agent executes (`/ask` mode)
- **Agent-led**: Agent proposes, human approves (Plan Mode confirmation)
- **Blended**: Dynamic flow based on confidence (`autonomous_mode`)

**Configuration**:

```toml
[agent]
autonomous_mode = true    # Auto-approve safe tools
human_in_the_loop = true  # Require approval for critical actions
```

#### 4. Reflection Loop

VT Code implements self-review for quality improvement:

```toml
[agent]
enable_self_review = true
max_review_passes = 3
```

The agent generates a draft, evaluates it against quality metrics, and refines until threshold is met.

#### 5. Skill Library Evolution

VT Code persists working solutions as reusable skills:

1. Agent writes code to solve immediate problem
2. If solution works, save to `.vtcode/skills/`
3. Refactor for generalization (parameterize hard-coded values)
4. Add documentation (purpose, parameters, returns, examples)
5. Future agents discover and reuse via `list_skills`/`load_skill`

**Progressive disclosure**: Skills are lazy-loaded to save context (91% token reduction achieved).

**Directory**: `.vtcode/skills/` with `INDEX.md` for discovery.

### Advanced Patterns

#### 6. Chain-of-Thought Monitoring & Interruption

VT Code surfaces agent reasoning in real-time for human oversight:

- **Reasoning visibility**: Agent's intermediate thinking is displayed
- **Early interruption**: First tool call reveals understanding—monitor closely
- **Circuit breaker**: Pauses after repeated failures for human guidance

**Configuration**:

```toml
[agent.circuit_breaker]
enabled = true
failure_threshold = 5
pause_on_open = true
```

#### 7. Context Window Anxiety Management

Models like Claude Sonnet 4.5 exhibit "context anxiety"—they rush decisions when approaching limits. VT Code counteracts this:

1. Enable large context (1M tokens) but cap actual usage at 200k
2. Counter-prompting: "You have plenty of context remaining—do not rush"
3. Explicit token budget transparency in prompts

**Implementation**: See `src/agent/runloop/unified/context_manager.rs` for token tracking.

#### 8. Abstracted Code Representation for Review

Instead of raw diffs, VT Code generates higher-level representations:

- **Intent descriptions**: "Refactors X to enable Y"
- **Architectural rationales**: "Reorganizing to separate concerns"
- **Behavior descriptions**: Before/after behavior summaries

This makes human review scalable for multi-file changes.

#### 9. Lethal Trifecta Threat Model

VT Code enforces security by ensuring at least one circle is missing:

1. **Access to private data** (secrets, user data)
2. **Exposure to untrusted content** (user input, web)
3. **Ability to externally communicate** (API calls)

**Implementation**: See `vtcode-core/src/exec_policy/` and `vtcode-process-hardening/`.

### Multi-Agent Patterns

#### 10. Swarm Migration Pattern

For large-scale migrations, VT Code's subagent system can orchestrate parallel work:

1. Main agent creates migration plan (enumerate all files)
2. Break into parallelizable chunks
3. Spawn subagent swarm (concurrent agents per chunk)
4. Map-reduce execution and verification

**Implementation**: See `vtcode-core/src/subagents/registry.rs` for subagent types.

#### 11. Oracle/Worker Pattern

VT Code's `small_model` configuration implements this pattern:

- **Oracle**: High-end model (Sonnet) for planning, review, error correction
- **Workers**: Smaller model (Haiku) for execution (large reads, parsing, summarization)

**Configuration**:

```toml
[agent.small_model]
enabled = true
model = "claude-3-5-haiku"  # Leave empty for auto-select
use_for_large_reads = true
use_for_git_history = true
```

### Pattern Maturity

| Pattern                 | Status        | Notes                               |
| ----------------------- | ------------- | ----------------------------------- |
| Plan-Then-Execute       | Best Practice | Plan Mode with confirmation         |
| Inversion of Control    | Best Practice | Subagent system                     |
| Spectrum of Control     | Validated     | autonomous_mode + HITL              |
| Reflection Loop         | Established   | self_review config                  |
| Skill Library Evolution | Established   | Skills in `.vtcode/skills/`         |
| CoT Monitoring          | Established   | Reasoning display + circuit breaker |
| Context Anxiety         | Established   | Token budget transparency           |
| Abstracted Review       | Emerging      | Intent descriptions                 |
| Lethal Trifecta         | Best Practice | Exec policy + sandboxing            |
| Swarm Migration         | Experimental  | Subagent orchestration              |
| Oracle/Worker           | Validated     | small_model tier                    |

### References

- [The Agentic AI Handbook](https://www.nibzard.com/agentic-handbook/) by Nikola Balić
- [Awesome Agentic Patterns](https://github.com/nibzard/awesome-agentic-patterns) (113+ patterns)
- [agentic-patterns.com](https://agentic-patterns.com/) - Pattern explorer

## Agent Workflows

This CLAUDE.md focuses on the **VT Code codebase itself**.

## IMPORTANT:

- When working on VT Code features, ALWAYS follow the guidelines in this document to ensure code quality, maintainability, and security.

- Make sure the name is "VT Code" not "VTCODE" or "vtcode" in user-facing text.
