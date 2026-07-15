# AGENTS.md

Keep this file concise and under 150 lines. Root guidance belongs here; detailed explanations belong in `docs/`, skills, `.vtcode/memory/`, or crate-local `AGENTS.md` files.

## Rules

- Conventional Commits (`type(scope): subject`).
- 4-space indentation, `snake_case` fns, `PascalCase` types, `anyhow::Result<T>` + `.with_context()`.
- CI sets `RUSTFLAGS: "-D warnings"` and uses `--locked`. Match locally with `cargo check --locked` when relevant.
- Keep changes surgical. Preserve existing APIs unless the task requires a change.
- `vtcode-exec-events::ThreadEvent` is the authoritative runtime event contract — do not invent parallel types.
- Harness config is split across `agent.harness`, `automation.full_auto`, `context.dynamic` — do not add a new top-level harness subsystem.
- Prefer `compact_str::CompactString` (aliased as `CompactStr` in `vtcode_core::types`) over `String` for small string fields. Use `Cow<'static, str>` for mostly-static return strings.
- **Shape-suffix naming**: encode the dimensional structure of data in variable/type names. For feature vectors, document a **dimension key** (table of index → name → meaning). For bare tuples holding structured data, promote to named structs so the shape is explicit in the type system (inspired by Noam Shazeer's shape-suffix convention).
- `clippy.toml` allows `unwrap`/`panic`/indexing in tests only.
- Dev profile has `incremental = false` (sccache). Set `CARGO_INCREMENTAL=1` to override.
- **All built-in themes must meet WCAG AA 4.5:1 contrast** for foreground and all accent fields against background. Validate with `cargo nextest run -p vtcode-ui -E 'test(theme)'`. See `.vtcode/memory/gotchas.md` for catppuccin-latte special-case.
- **Every new major feature must update docs**: user-facing behavior → `docs/development/` guide + a table row/section in the relevant quick-reference; agent-facing tool surface → prompt guidance (`vtcode-core/src/prompts/guidelines.rs`) + schema (`vtcode-utility-tool-specs`); runtime contract → `vtcode-exec-events::ThreadEvent`. No feature is "done" until the docs it changes are updated and the AGENTS.md detailed-guides links still resolve.

## Detailed Guides

- Development overview and setup: [docs/development/README.md](docs/development/README.md), [docs/development/DEVELOPMENT_SETUP.md](docs/development/DEVELOPMENT_SETUP.md).
- Testing: [docs/development/testing.md](docs/development/testing.md), [docs/guides/pty-integration-testing.md](docs/guides/pty-integration-testing.md).
- CI/release: [docs/development/ci-cd.md](docs/development/ci-cd.md), [docs/development/CHANGELOG_GENERATION.md](docs/development/CHANGELOG_GENERATION.md).
- Architecture/conventions: [docs/guides/code-organization-patterns.md](docs/guides/code-organization-patterns.md), [docs/guides/async-architecture.md](docs/guides/async-architecture.md), [docs/development/rust-performance-principles.md](docs/development/rust-performance-principles.md).
- Tools/security: [docs/development/grep-tool-guide.md](docs/development/grep-tool-guide.md), [docs/development/grep-quick-reference.md](docs/development/grep-quick-reference.md), [docs/development/COMMAND_SECURITY_MODEL.md](docs/development/COMMAND_SECURITY_MODEL.md), [docs/guides/security.md](docs/guides/security.md).
- Harness/agent behavior: [docs/guides/agent-loop-contract.md](docs/guides/agent-loop-contract.md), [docs/harness/INDEX.md](docs/harness/INDEX.md), [docs/harness/CORE_BELIEFS.md](docs/harness/CORE_BELIEFS.md), [docs/harness/ARCHITECTURAL_INVARIANTS.md](docs/harness/ARCHITECTURAL_INVARIANTS.md), [docs/harness/AGENT_LEGIBILITY_GUIDE.md](docs/harness/AGENT_LEGIBILITY_GUIDE.md).
- Planning and automation: [docs/guides/planning-workflow.md](docs/guides/planning-workflow.md), [docs/guides/full-automation.md](docs/guides/full-automation.md), [docs/development/EXECUTION_POLICY.md](docs/development/EXECUTION_POLICY.md).
- Loop engineering: [docs/project/PLAN-loop-engineering.md](docs/project/PLAN-loop-engineering.md) — worktree isolation, propose/verify sub-agents, loop state persistence, cost guardrails.
- Models/providers: [docs/development/ADDING_MODELS.md](docs/development/ADDING_MODELS.md), [docs/development/MODEL_ADDITION_WORKFLOW.md](docs/development/MODEL_ADDITION_WORKFLOW.md), [docs/development/MODEL_ADDITION_CHECKLIST.md](docs/development/MODEL_ADDITION_CHECKLIST.md).

## Workspace

Cargo workspace, ~30 crates. Rust stable, MSRV 1.88, edition 2024. `default-members` = root, `vtcode-core`, `vtcode-ui` only.

| Crate | Role |
|---|---|
| `vtcode` (root `src/`) | Binary, CLI, session bootstrap |
| `vtcode-core` | Agent loop, tools, prompts, LLM orchestration, UI |
| `vtcode-ui` | Unified UI: design system, theme registry, TUI framework |
| `vtcode-config` | Config loading and schema |
| `vtcode-bash-runner` | Shell execution sandbox |
| `vtcode-acp` | Agent Client Protocol (Zed) |
| `vtcode-auth` | OAuth and credential storage |
| `vtcode-indexer` | Code indexing and search |
| `vtcode-exec-events` | `ThreadEvent` contract and ATIF export |
| `vtcode-commons` | Shared utilities |
| `vtcode-macros` | Procedural macros |
| `vtcode-utility-tool-specs` | JSON schemas for utility, file, and collaboration/HITL tools |
| `vtcode-llm` | LLM provider abstraction, client implementations, streaming (partial extraction) |
| `vtcode-skills` | Skill types, discovery, loading, and validation (partial extraction) |
| `vtcode-session-store` | Unified per-session state store: append-only `ThreadEvent` log, derived views, retention, cross-session query (single source of truth) |
| `vtcode-eval` | Agent evaluation framework: pass@k/pass^k metrics, capability/regression evals, environment-based outcome verification |
| `vtcode-safety` | Command safety detection, execution policies, sandboxing |
| `vtcode-a2a` | Agent2Agent (A2A) protocol client and server |
| `vtcode-mcp` | Model Context Protocol client, connection pooling, tool discovery |
| `xtask` | Release packaging automation |

New reusable logic: put it in an existing small crate or a new one. Keep it out of `vtcode-core` by default unless tightly coupled to the core runtime.

## Per-Module Guidance

Every crate has its own AGENTS.md with crate-specific conventions:

| Crate | AGENTS.md |
|---|---|
| `vtcode` (binary) | [src/AGENTS.md](src/AGENTS.md) |
| `vtcode-core` | [vtcode-core/AGENTS.md](vtcode-core/AGENTS.md) |
| `vtcode-ui` | [vtcode-ui/AGENTS.md](vtcode-ui/AGENTS.md) |
| `vtcode-config` | [vtcode-config/AGENTS.md](vtcode-config/AGENTS.md) |
| `vtcode-bash-runner` | [vtcode-bash-runner/AGENTS.md](vtcode-bash-runner/AGENTS.md) |
| `vtcode-acp` | [vtcode-acp/AGENTS.md](vtcode-acp/AGENTS.md) |
| `vtcode-auth` | [vtcode-auth/AGENTS.md](vtcode-auth/AGENTS.md) |
| `vtcode-indexer` | [vtcode-indexer/AGENTS.md](vtcode-indexer/AGENTS.md) |
| `vtcode-exec-events` | [vtcode-exec-events/AGENTS.md](vtcode-exec-events/AGENTS.md) |
| `vtcode-commons` | [vtcode-commons/AGENTS.md](vtcode-commons/AGENTS.md) |
| `vtcode-macros` | [vtcode-macros/AGENTS.md](vtcode-macros/AGENTS.md) |
| `vtcode-utility-tool-specs` | [vtcode-utility-tool-specs/AGENTS.md](vtcode-utility-tool-specs/AGENTS.md) |
| `vtcode-llm` | [vtcode-llm/AGENTS.md](vtcode-llm/AGENTS.md) |
| `vtcode-skills` | [vtcode-skills/AGENTS.md](vtcode-skills/AGENTS.md) |
| `vtcode-session-store` | [vtcode-session-store/AGENTS.md](vtcode-session-store/AGENTS.md) |
| `vtcode-eval` | [vtcode-eval/AGENTS.md](vtcode-eval/AGENTS.md) |
| `vtcode-safety` | [vtcode-safety/AGENTS.md](vtcode-safety/AGENTS.md) |
| `vtcode-a2a` | [vtcode-a2a/AGENTS.md](vtcode-a2a/AGENTS.md) |
| `vtcode-mcp` | [vtcode-mcp/AGENTS.md](vtcode-mcp/AGENTS.md) |
| `xtask` | [xtask/AGENTS.md](xtask/AGENTS.md) |

After significant changes (new modules, convention shifts, discovered gotchas, public API changes), use the `audit-module-agents` skill to check if the affected crate's AGENTS.md needs updating. Keep each local AGENTS.md under 30 lines.

## Project Memory

Session-independent knowledge lives in `.vtcode/memory/` (gitignored): `gotchas.md`, `issues.md`, `library.md`, `decisions.md`, and `scratch.md`. Read these files when context is needed. Write durable learnings there. See `.vtcode/memory/README.md` for format rules.

## Build & Verification

- CI build caching: `Swatinem/rust-cache` keys off the target triple only when `CARGO_BUILD_TARGET` is set or you pass `key:`. Builds that pass `--target` via the CLI (e.g. `cross build --target`) share ONE cache key across matrix jobs on the same runner OS, causing colliding/failing saves and every target restoring a mismatched cache. Always namespace the cache per target (`with: { key: ${{ matrix.target }} }`) in cross-target matrix jobs.
- Prefer `./scripts/check-dev.sh` (10-30s) over `./scripts/check.sh` (2-5m) for iteration.
- Release builds keep `debug-assertions = true` and `overflow-checks = true` in `[profile.release]`. `debug_assert!` and overflow checks are NOT disabled in prod: a violated invariant must crash loud, not let the program run under wrong assumptions (see kristoff.it/blog/fix-your-asserts). Use `assert!`/`debug_assert!` for invariants that always hold; gate expensive diagnostics behind `#[cfg(debug_assertions)]` since that branch still compiles out of release when the flag is off elsewhere.

| Change | Command |
|---|---|
| Fast gate | `./scripts/check-dev.sh` |
| + tests (quick) | `./scripts/check-dev.sh --test` |
| + tests (changed crates) | `./scripts/check-dev.sh --changed` |
| + workspace | `./scripts/check-dev.sh --workspace` |
| + lints | `./scripts/check-dev.sh --lints` |
| Harness PTY/TUI | `./scripts/check.sh harness` |
| Release/PR | `./scripts/check.sh` |
| Ast-grep rules | `vtcode check ast-grep` |
| Ast-grep scan | `ast-grep scan` (requires `sgconfig.yml` + `rules/`) |

Narrow commands: `cargo check`, `cargo nextest run`, `cargo nextest run --profile quick`, `cargo fmt`, `cargo clippy`. **Never use `cargo test` — always use `cargo nextest run`.**

## Testing

- Runner: `cargo nextest run` (parallel, fast). **Always use nextest — never `cargo test`**.
- Single test: `cargo nextest run test_name`.
- Single crate: `cargo nextest run -p vtcode-core`.
- Profiles: `default` (full), `quick` (TDD, skips integration/e2e/slow), `changed` (delta since HEAD~1), `ci` (retries flaky, no fail-fast).
- Harness regressions: `cargo nextest run -p vtcode-core -E 'binary(/pty_tests/)'`; `cargo nextest run -p vtcode-bash-runner -E 'binary(/pipe_tests/)'`; `cargo nextest run -p vtcode -E 'binary(/inline_events/)'`.
- Integration tests: `tests/` at workspace root. Unit tests: in-module.

## Skills & Special Workflows

- Skills are invoked via the Skill tool; subagents are spawned via the Agent tool. Project slash commands live in `.claude/commands/`; agents live in `.claude/agents/`.
- LLM providers: use the `adding-llm-providers` skill. The `/model` picker uses `ModelId::all_models()`; `builtin_model_presets()` is used by `ModelsManager`. Both may need updates.
- New workspace crates: use the `adding-workspace-crate` skill. This affects more than `Cargo.toml`; all workspace path dependencies need `version` fields.
- Structural code work: prefer `ast-grep` over text grep for code shape, calls, impls, and codemods. Use `rg` for prose, logs, and config strings. Always invoke `ast-grep`, not the `sg` alias. Use `exec_command` or the ast-grep skill for arbitrary structural patterns. Advanced `code_search` accepts one literal query and bounded filters.
- Cap large command output: `COMMAND 2>&1 | head -c 4000`.
