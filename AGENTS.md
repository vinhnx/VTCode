This file is the **entry-point map** for agents working on VT Code.
Deep knowledge lives in `docs/` — this file tells you WHERE to look, not WHAT to do.

## Personality & Communication

- **Concise and direct**: Minimize output; answer specifically without elaboration
- **Actionable**: Prioritize next steps and clear guidance over explanation
- **Efficient**: Avoid verbose explanations unless explicitly requested
- **Collaborative**: Work like a knowledgeable teammate sharing momentum

## Workspace Structure

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
├── vtcode-file-search/          # File search utilities
├── vtcode-exec-events/          # Event definitions
├── vtcode-acp-client/           # Agent Client Protocol bridge
└── vtcode-process-hardening/    # Process hardening & security
```

**Key separation**: `vtcode-core/` is the reusable library; `src/` is the CLI executable (Ratatui TUI, PTY, slash commands).

## Quick Reference Commands

```bash
cargo check                  # Type-check
cargo clippy                 # Lint
cargo fmt --check            # Format check
cargo nextest run            # Run tests
cargo nextest run --test integration_tests  # Integration only
cargo bench                  # Benchmarks

# Before committing — run all:
cargo clippy && cargo fmt --check && cargo check && cargo nextest run
```

## Rulebook (CRITICAL)

### Code Quality
- **No `any` types** unless absolutely necessary.
- **NEVER use inline imports** (e.g., `let x = await import(...)`). Always use standard top-level imports.
- **Upgrade dependencies** instead of downgrading code to fix type errors.
- **Always ask** before removing functionality or code that appears intentional.
- **Never hardcode model IDs** (use `docs/models.json`) or keybindings.

### Critical Git Rules for Parallel Agents
Multiple agents work in this tree. **NEVER** use destructive commands:
- **NO** `git reset --hard`, `git checkout .`, `git clean -fd`, or `git stash`.
- **NO** `git add .` or `git add -A`. These sweep up other agents' work.
- **ALWAYS** use `git add <specific-file-paths>` for only YOUR changes.
- **ALWAYS** `git pull --rebase` before pushing. If conflicts occur in files you didn't touch, **ABORT** and ask the user.

### Tool Usage
- **Read every file in full** before editing.
- **NEVER use `sed` or `cat`** for reading; use the provided `read` tools.

### Communication & Style
- **Technical prose only**: No fluff, emojis, or cheerful filler.
- **Direct attribution**: Cite issues/PRs as `Fixed #123`.
- **Changelog**: Append to `## [Unreleased]` sections; never modify released versions.

## Key Files — Never Hardcode

- **Model IDs** → `docs/models.json`
- **Constants** → `vtcode-core/src/config/constants.rs`
- **Config values** → `vtcode.toml` (runtime), `vtcode-core/src/config/constants.rs` (defaults)

## Common Pitfalls (Top 5)

1. **Don't hardcode model names** — they change frequently; use constants
2. **Don't use `unwrap()`** — use `anyhow::Result` with `.with_context()`
3. **Don't create .md files in root** — docs belong in `./docs/`
4. **Don't skip quality checks** — always run clippy + fmt + check + nextest
5. **Don't ignore Clippy warnings** — fix or explain in code comments

## Deep Dives — Where to Look

### Architecture & Design
- Architecture overview → [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Configuration precedence → [`docs/config/CONFIGURATION_PRECEDENCE.md`](docs/config/CONFIGURATION_PRECEDENCE.md)
- Process hardening → [`docs/PROCESS_HARDENING.md`](docs/PROCESS_HARDENING.md)
- Security model → [`docs/security/SECURITY_MODEL.md`](docs/security/SECURITY_MODEL.md)

### Protocol Integrations
- MCP (Model Context Protocol) → [`docs/mcp/`](docs/mcp/) (start at `docs/mcp/00_START_HERE.md`)
- ACP (Agent Client Protocol) → [`docs/ACP_INTEGRATION.md`](docs/ACP_INTEGRATION.md)

### Agent Behavior & Harness Engineering
- Core beliefs & execution philosophy → [`docs/harness/CORE_BELIEFS.md`](docs/harness/CORE_BELIEFS.md)
- Humans Steer, Agents Execute → [`vtcode-core/src/prompts/system.rs`](vtcode-core/src/prompts/system.rs)
- Agent legibility standards → [`docs/harness/AGENT_LEGIBILITY_GUIDE.md`](docs/harness/AGENT_LEGIBILITY_GUIDE.md)
- Quality scoring rubric → [`docs/harness/QUALITY_SCORE.md`](docs/harness/QUALITY_SCORE.md)
- Execution plans & workflow → [`docs/harness/EXEC_PLANS.md`](docs/harness/EXEC_PLANS.md)
- Architectural invariants → [`docs/harness/ARCHITECTURAL_INVARIANTS.md`](docs/harness/ARCHITECTURAL_INVARIANTS.md)
- Tech debt tracker → [`docs/harness/TECH_DEBT_TRACKER.md`](docs/harness/TECH_DEBT_TRACKER.md)

### Code Style & Tools
- Code style & conventions → [`.github/copilot-instructions.md`](.github/copilot-instructions.md)
- Available tools reference → [`docs/AVAILABLE_TOOLS.md`](docs/AVAILABLE_TOOLS.md)
- Subagent system → [`docs/subagents/SUBAGENTS.md`](docs/subagents/SUBAGENTS.md)

### Testing & Development
- Testing guide → [`docs/development/testing.md`](docs/development/testing.md)
- Provider setup → [`docs/PROVIDER_GUIDES.md`](docs/PROVIDER_GUIDES.md)

### Contributing
- Contributing guide → [`CONTRIBUTING.md`](CONTRIBUTING.md)
