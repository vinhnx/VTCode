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

VT Code uses a **13-member Cargo workspace**:

```
vtcode/                          # Binary entrypoint (src/main.rs)
├── vtcode-core/                 # Core library (LLM, tools, config, MCP)
├── vtcode-tui/                  # TUI surface and session runtime
├── vtcode-config/               # Configuration loader & schema
├── vtcode-commons/              # Shared utilities
├── vtcode-llm/                  # LLM provider abstractions
├── vtcode-lmstudio/             # LM Studio provider integration
├── vtcode-tools/                # Tool implementations
├── vtcode-bash-runner/          # Shell execution engine
├── vtcode-markdown-store/       # Document storage
├── vtcode-indexer/              # Code indexing
├── vtcode-exec-events/          # Event definitions
├── vtcode-file-search/          # Parallel fuzzy file search
└── vtcode-acp-client/           # Agent Client Protocol bridge
```

`vtcode-process-hardening` is intentionally excluded from `[workspace].members` and remains an isolated pre-main hardening crate.

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

**Process Hardening** (`vtcode-process-hardening/`): Pre-main security (ptrace disable, core dump disable, env var removal). Exit codes: 5/6/7. See `docs/development/PROCESS_HARDENING.md`.

## Code Style & Conventions

### Code Style

**Errors**: `anyhow::Result<T>` with `.with_context()`. Never `unwrap()`.

**Unsafe Code**: Do not use `unsafe` blocks or `#[allow(unsafe_code)]` (including tests). Prefer safe std/crate APIs.

**Config**: Never hardcode. Use `vtcode_core::config::constants`, `vtcode.toml`, or `docs/models.json`.

**Naming**: `snake_case` functions/vars, `PascalCase` types. Descriptive names, early returns, 4-space indent.

**Docs**: `.md` in `./docs/` only (`README.md` exception).

### Rust Style (epage)

Based on [epage's Rust Style Guide](https://epage.github.io/dev/rust-style/). Code is technical writing — lead with salient details (inverted pyramid), guide the reader through structure.

#### Project Structure

- **P-MOD**: Prefer `mod.rs` over `name.rs` when splitting into directories. Keeps modules atomic for browsing/renaming. Enforced via `clippy.self_named_module_files`.
- **P-DIR-MOD**: Directory root modules (`mod.rs`, `lib.rs`) only re-export. All definitions live in topically named files.
- **P-PRELUDE-MOD**: Prelude modules only re-export.
- **P-PATH-MOD**: Avoid `#[path]`; use standard module lookup.
- **P-API**: API should be a subset of file layout — a reader should navigate from API to source by path.
- **P-VISIBILITY**: Limit visibility to module-scope (no `pub(_)`), `pub(crate)`, or `pub`. If not fully abstracted within a module, use `pub(crate)`.

#### File Structure

- **M-PRIV-PUB-USE**: Private imports first, then public re-exports (group `pub use` with the public API).
- **M-PRIV-USE**: Limit private imports to heavily-used items where intent is clear from the name. Import traits anonymously (`use Trait as _`).
- **M-SINGLE-USE**: Import items individually, not compound (`use a::B; use a::C;` not `use a::{B, C};`).
- **M-ITEM-TOC**: Central/titular item first in a module — it provides context and a TOC for the rest.
- **M-TYPE-ASSOC**: Type definition immediately followed by its `impl` block, before the next type.
- **M-ASSOC-TRAIT**: Associated functions (`impl Foo {}`) before trait impls (`impl Display for Foo {}`).
- **M-CALLER-CALLEE**: Caller before callee. The weaker the callee's abstraction, the closer it should follow.
- **M-PUB-PRIV**: Public items before private items in modules, structs, and impl blocks.

#### Function Structure

- **F-GROUP**: Use blank lines to group related logic ("paragraphs" of a function).
- **F-OUT**: Open blocks with output variable declarations to announce intent.
- **F-VISUAL**: Blocks (`if`/`else`/`match`) should reflect business logic. Use early returns for bookkeeping, not business paths. Prefer combinators for non-business transformations.
- **F-PURE-MUT**: Pure expressions xor mutable statements — don't mix mutation with expression-based logic.
- **F-COMBINATOR**: Closures in combinators (`map`, `filter`, etc.) must be pure. Use `for` loops for side effects — `for_each`/`try_for_each` are disallowed via `clippy.toml`.

### Logging & Tracing Guidelines

**Architecture**: Shared buffered writer in `vtcode-core/src/utils/trace_writer.rs` (`FlushableWriter`) with flush hook in `vtcode-commons/src/trace_flush.rs`. Tracing setup is centralized in `src/main_helpers/tracing.rs` via `install_tracing_stack()`.

**Level discipline** — choose the right level for each log:
- `error!` — Unrecoverable failures that need operator attention
- `warn!` — Degraded behavior (rate limits, circuit breakers, loop detection, config issues)
- `info!` — Key lifecycle milestones (session start/end, approval decisions, feature activation)
- `debug!` — Useful for troubleshooting (provider registration, cache hits, config loading)
- `trace!` — Per-invocation internals (tool routing, policy checks, safety gateway, CGP lifecycle, payload diagnostics)

**Rules**:
- Never log at `info!` for per-tool-invocation events — use `trace!`
- Never log at `debug!` for routine happy-path checks (read-only classification, preapproval) — use `trace!`
- Keep `info!` for events that appear ≤ once per session phase (init, shutdown, mode change)
- Actionable logs only: if the reader can't act on it, downgrade or remove it
- No-op paths should be silent: early-return without logging when there's nothing to do (e.g., empty custom providers list)

**Flush on exit**: Always call `vtcode_commons::trace_flush::flush_trace_log()` (or `vtcode_core::utils::trace_writer::flush_trace_log()`) before `process::exit()` or at shutdown. Signal handlers and TUI runner already do this.

**Session file retention**: All session artifacts (`.json`, `.jsonl`, `.log`) are pruned by `session_archive` retention (default: 100 files, 14 days, 100 MB). Debug logs have separate rotation at 50 MB / 7 days.

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

# Memory Safety (Miri)
./scripts/check.sh miri  # Run local Miri check to detect Undefined Behavior
```

## Development Notes

**Security**: Validate paths (workspace boundaries), command allowlists, tool policies in `vtcode.toml`, human-in-the-loop approval.

**Performance**: Single codegen unit, strict Clippy, no `expect_used`/`unwrap_used`.

### Performance & Simplicity

- Don't speculate about bottlenecks; measure with VT Code benchmarks, traces, or targeted timings before optimizing.
- Benchmark before tuning and keep before/after evidence for non-trivial performance claims.
- Prefer simple or brute-force approaches when scale is unproven or inputs are usually small.
- Avoid fancy algorithms unless measurements show they matter more than the simpler alternative.
- Choose data structures and layout first; once the data is right, the algorithm should usually become obvious.

**Blocked Handoff Recovery**: When a run writes a blocked handoff, check `.vtcode/tasks/current_blocked.md` (and `.vtcode/tasks/blockers/`) for context, then resume with `vtcode --resume <session_id>`.

**Hooks Noise Control**: Set `hooks.lifecycle.quiet_success_output = true` in `vtcode.toml` to suppress plain stdout for successful lifecycle hooks while retaining structured/failed output.

**Interactive Session Commands**: See `docs/user-guide/interactive-mode.md` for full details.

- `/terminal-setup` runs the guided terminal setup flow for multiline bindings (see `docs/guides/terminal-optimization.md`).
- `/vim`, `/vim on`, `/vim off` toggle Vim prompt editing; set `ui.vim_mode = true` in `vtcode.toml` to enable by default.
- `/suggest`, `/tasks`, `/jobs` open prompt suggestions, the TODO panel, and the jobs picker.

**Update System**: See `docs/guides/UPDATE_SYSTEM.md` for the full workflow.

- `vtcode update` installs updates; `vtcode update --check` checks only.
- `vtcode update --list` lists available versions; `vtcode update --list --limit <N>` lists more.
- `vtcode update --pin <version>` pins a release; `vtcode update --unpin` clears the pin.
- `vtcode update --channel <stable|beta|nightly>` switches release channels.

## Agent-First CLI Invariants

- Prefer machine-readable output for automation (`vtcode ask --output-format json`, `vtcode exec --json`).
- Introspect tool schemas at runtime before generating tool calls (`vtcode schema tools`).
- Use `vtcode exec --dry-run` before mutation-oriented autonomous tasks.
- Assume agent-generated input can be adversarial; keep input validation strict for CLI/MCP fields.

**Pitfalls**:

1. Don't assume paths — validate boundaries
2. Don't skip quality gate
3. Don't assume `RwLock` is faster — benchmark; `Mutex` often wins

### Slash Commands & Inline List UI

- Register new slash command metadata in `vtcode-core/src/ui/slash.rs` (`SLASH_COMMANDS`) so it appears in the `/` suggestion list.
- Wire command parsing and outcome routing end-to-end: `src/agent/runloop/slash_commands.rs` -> `src/agent/runloop/unified/turn/session/slash_commands/mod.rs` -> concrete handler in `.../slash_commands/ui.rs` (or relevant handler module).
- For picker/selection UX, use shared inline list flows (`ShowListModal` / `ShowWizardModal` / shared inline events) instead of introducing new popup/overlay widget implementations.
- Keep slash behavior consistent: if a command should execute immediately from slash selection, update the immediate-submit matcher in `vtcode-tui/src/core_tui/session/slash.rs`.
- Add focused tests when touching this path (at minimum: `vtcode-core` slash suggestion tests and `vtcode-tui` slash/session tests).

## Agent Execution Guidelines

## Summary Instructions

When VT Code compacts or summarizes a conversation, preserve:

- The current task objective and acceptance criteria
- File paths that were read or modified
- Test results and error messages
- Decisions made and the reasoning behind them

### Task Execution

- **Complete autonomously**: Resolve fully before yielding; no intermediate confirmations
- **Root cause fixes**: Fix at the source, not surface-level
- **Verify yourself**: Run `cargo check`, `cargo nextest`, `cargo clippy` after changes.
- **No scope creep**: Stick to the task; don't fix unrelated issues (mention in final message).
- **Testing**: Prefer cargo nextest for speed. Add tests if none exist, but don't add if it requires significant scope increase. Also run cargo insta snapshots if applicable for UI tests.
- **Safety gate**: No `unsafe` code (including tests) and enforce `cargo clippy --workspace --all-targets -- -D warnings`
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

## Model Management

Adding a new LLM model requires updates across three layers (constants, configuration, runtime):

- **Quick start**: `./scripts/add_model.sh` - Interactive prompts + code generation
- **Full guide**: `docs/development/ADDING_MODELS.md` - 10-step detailed workflow
- **Checklist**: `docs/development/MODEL_ADDITION_CHECKLIST.md` - 70-point verification
- **Why partial automation**: `docs/development/MODEL_ADDITION_WORKFLOW.md` - Architecture & approach

Files to update: openai.rs, models.json, model_id.rs, as_str.rs, display.rs, description.rs, parse.rs, provider.rs, collection.rs, capabilities.rs.

## Resources

- `docs/ARCHITECTURE.md` — High-level architecture
- `docs/security/SECURITY_MODEL.md` — Security design
- `docs/config/CONFIGURATION_PRECEDENCE.md` — Config loading order
- `docs/providers/PROVIDER_GUIDES.md` — LLM provider setup
- `docs/development/testing.md` — Testing strategy
- `docs/development/ADDING_MODELS.md` — Model addition workflow

## Hickey's Core: Simple > Easy

**Simple** = one concern, not complected. **Easy** = familiar/convenient. Never conflate.

**Rules**: Separate what changes from what doesn't. Data over objects. Values over mutation. No temporal coupling. Decomplect first.

**Test**: Can you reason about this without loading the whole system? If no — it's complected.
