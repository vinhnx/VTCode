# AGENTS.md

This file provides guidance to VT Code coding agent, when working with code in this repository. Please make sure the application is "VT Code", with proper capitalization and spacing.

This file is the **entry-point map** for agents working on VT Code. Deep knowledge lives in `docs/` — this file tells you WHERE to look, not WHAT to do.

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

### Core Systems

**LLM** (`vtcode-llm/`, `vtcode-core/src/llm/`): Factory pattern, 10 providers, failover, caching, token budgets.

**Tools** (`vtcode-tools/`, `vtcode-core/src/tools/`): Trait-driven (`Tool`, `ModeTool`, `CacheableTool`). Unified: `unified_exec`, `unified_file`, `unified_search`.

**Config**: Env vars → `vtcode.toml` → constants. Never hardcode.

**PTY** (`vtcode-core/src/exec/`, `vtcode-bash-runner/`): Interactive shell sessions, streaming output.

**Tree-Sitter** (`vtcode-core/src/tree_sitter/`, `vtcode-indexer/`): Rust, Python, JS/TS, Go, Java, Bash. Incremental AST with caching.

**Code Intelligence** (`vtcode-core/src/tools/code_intelligence.rs`): `goto_definition`, `find_references`, `hover`, `document_symbol`, `workspace_symbol`.

### Protocols

**ACP** (`vtcode-acp-client/`): Zed IDE integration.

**A2A**: Agent Card at `/.well-known/agent-card.json`. Task states, SSE, JSON-RPC 2.0. See `docs/a2a/a2a-protocol.md`.

**MCP** (`vtcode-core/src/mcp/`): Extensible tooling via `rmcp`. Config: `.mcp.json` (project) + `vtcode.toml`. Transports: stdio, HTTP, child-process. See `docs/mcp/`.

### Extensions

**Skills** (`.vtcode/skills/` → `~/.vtcode/skills/` → embedded): agentskills.io standard. See `docs/skills/`.

**Subagents** (`vtcode-core/src/subagents/`): `spawn_subagent` tool. Built-in: `explore`, `plan`, `general`, `code-reviewer`, `debugger`. Custom: `.vtcode/agents/`. See `docs/subagents/SUBAGENTS.md`.

**Process Hardening** (`vtcode-process-hardening/`): Pre-main security (ptrace disable, core dump disable, env var removal). Exit codes: 5/6/7. See `docs/development/PROCESS_HARDENING.md`.

## Code Style & Conventions

### Code Style

**Errors**: `anyhow::Result<T>` with `.with_context()`. Never `unwrap()`.

**Unsafe Code**: Do not use `unsafe` blocks or `#[allow(unsafe_code)]` (including tests). Prefer safe std/crate APIs.

**Config**: Never hardcode. Use `vtcode_core::config::constants`, `vtcode.toml`, or `docs/models.json`.

**Naming**: `snake_case` functions/vars, `PascalCase` types. Descriptive names, early returns, 4-space indent.

**Docs**: `.md` in `./docs/` only (`README.md` exception).

## Testing

**Organization**: Unit tests inline (`#[cfg(test)]`), integration in `tests/`, benchmarks in `benches/`, snapshots via [`insta`](https://insta.rs).

```bash
# Build & check
cargo build
cargo check

# Test (nextest is 3-5x faster)
cargo nextest run              # All tests
cargo nextest run -p vtcode-core  # Single package
cargo nextest run --test integration_tests
cargo nextest run -- --nocapture

# Quick profiles (aliases)
cargo t   # nextest run
cargo tq  # no retries
cargo ts  # fallback

# Snapshots (insta)
cargo insta test    # Run + review
cargo insta review  # Interactive
cargo insta accept  # Accept all

# Benchmarks
cargo bench

# Quality gate
cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check && cargo check && cargo nextest run
```

## Development Notes

**Security**: Validate paths (workspace boundaries), command allowlists, tool policies in `vtcode.toml`, human-in-the-loop approval.

**Performance**: Single codegen unit, strict Clippy, no `expect_used`/`unwrap_used`.

**Pitfalls**:
1. Don't assume paths — validate boundaries
2. Don't skip quality gate
3. Don't assume `RwLock` is faster — benchmark; `Mutex` often wins

## Agent Execution Guidelines

### Task Execution

- **Complete autonomously**: Resolve fully before yielding; no intermediate confirmations
- **Root cause fixes**: Fix at the source, not surface-level
- **Verify yourself**: Run `cargo check`, `cargo nextest`, `cargo clippy` after changes
- **Precision over ambition**: Surgical changes respecting existing style
- **Stay in scope**: Don't fix unrelated issues (mention in final message)
- **Iterate proactively**: Fix errors without asking; know when "done" is sufficient

### Responsiveness

**Preambles** (before tool calls): 1-2 sentences showing progress. Group related actions. Skip for trivial reads.

> "Explored repo structure; checking LLM provider factory."  
> "Config loads. Patching tool registry and tests."

**Progress updates** for longer tasks: "Finished tool trait analysis; implementing code_intelligence op."

### Final Answers

**Format**: Lead with outcomes. ≤10 lines. Use bullets (4-6 per section). Monospace for commands/paths.

**Don't**: No inline citations, no repeating plans, no nested bullets, no cramming unrelated items.

### Planning

Use `update_plan` for 4+ step tasks. 5-7 word steps. Mark `in_progress`/`completed`. Update if scope changes.

### Tool Use

**Search**: `unified_search` with `action="grep"` | `list` | `intelligence` | `tools` | `errors` | `agent`. Read files once.

**Edit**: `unified_file` with `edit` | `write` | `create`. `unified_exec` for shell/PTY. Don't re-read to verify.

**Test**: Specific → broad. Run proactively. No tests in codebase? Don't add. Run `cargo clippy` after changes.

## Self-Documentation

Answering questions about VT Code? Check `docs/modules/vtcode_docs_map.md` first.

## Resources

- `docs/ARCHITECTURE.md` — High-level architecture
- `docs/security/SECURITY_MODEL.md` — Security design
- `docs/config/CONFIGURATION_PRECEDENCE.md` — Config loading order
- `docs/providers/PROVIDER_GUIDES.md` — LLM provider setup
- `docs/development/testing.md` — Testing strategy

## Hickey's Core: Simple > Easy

**Simple** = one concern, not complected. **Easy** = familiar/convenient. Never conflate.

**Rules**: Separate what changes from what doesn't. Data over objects. Values over mutation. No temporal coupling. Decomplect first.

**Test**: Can you reason about this without loading the whole system? If no — it's complected.
