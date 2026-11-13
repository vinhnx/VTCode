<!-- markdownlint-disable MD013 -->

# AGENTS.md

Keep this file under 12 KiB (enforced: root plus the largest module `AGENTS.md` must fit VT Code's 16 KiB instruction budget, `scripts/check_docs_links.py`). Details belong in `docs/`, skills, `.vtcode/memory/`, or crate `AGENTS.md` files; universal model behavior is compiled in `crates/codegen/vtcode-core/src/prompts/runtime_guidance.rs`. Instruction files are user-controlled context, not a security boundary. Repo-only: do not copy into other projects or shipped prompts.

## Core Workflow (repo-only)

- Orient: repository instructions, relevant source/tests, and task-relevant entries in `.vtcode/memory/` (search, do not concatenate). For self-bugs, read `.vtcode/logs/trajectory.jsonl`, `.vtcode/checkpoints`, `.vtcode/sessions`.
- Research hard problems only: DeepWiki MCP for `vinhnx/vtcode` (baseline) and `openai/codex` (upstream); verify claims against the checkout or pinned upstream source, attribute sources, stay within the original authorization. See the [reference workflow](docs/development/codex-reference-workflow.md).
- Read and apply: use `read-and-apply`, `rust-skills`/`agentic-testing`, and `verification-discipline`; implement the smallest complete DRY/KISS improvement; preserve `ThreadEvent`, the harness config split, sandbox/approval boundaries, rollback, durable history, and unrelated work. Adapted upstream code keeps Apache-2.0 attribution ([root notes](docs/development/maintainer-notes/root.md)).
- Verify with `./scripts/check-dev.sh` + `cargo nextest run`; record durable learnings in `.vtcode/memory/` (local-only, gitignored). Interview the user on plan open questions via the interactive question tool (one at a time, recommended answer first) instead of assuming defaults.

## Rules

- Conventional Commits (`type(scope): subject`).
- 4-space indentation, `snake_case` fns, `PascalCase` types, `anyhow::Result<T>` + `.with_context()`. Follow the [Rust style adoption matrix](docs/development/rust-style-guide.md) for flexible borrows, targeted `must_use`, API conventions, and exceptions.
- CI sets `RUSTFLAGS: "-D warnings"` and uses `--locked`. Match locally with `cargo check --locked` when relevant.
- Keep changes surgical: know which files the increment owns, preserve APIs and unrelated work, stage explicit reviewed paths, and check each scoped commit's staged diff.
- Hard ceiling: no tracked text file may exceed 1000 lines (`python3 scripts/check_file_length.py --mode enforce`, gated by `check-dev.sh`/`check.sh`/pre-commit hook). Park generated/vendored outliers in `scripts/file_length_allowlist.txt` as `glob=MAX_LINES`; never allowlist first-party source without a split plan.
- Prefer direct single-agent execution. Do not impose orchestrator/worker or other multi-agent topologies: forced delegation burns usage limits and yields worse results. Delegate only when independent/parallel work or context isolation clearly helps.
- `vtcode-exec-events::ThreadEvent` is the authoritative runtime event contract — do not invent parallel types.
- Harness config is split across `agent.harness`, `automation.full_auto`, `context.dynamic` — do not add a new top-level harness subsystem.
- Prefer `compact_str::CompactString` (aliased as `CompactStr` in `vtcode_core::types`) over `String` for small string fields. Use `Cow<'static, str>` for mostly-static return strings.
- **Shape-suffix naming**: encode data shape in names; document a dimension key for feature vectors; promote structured tuples to named structs.
- `clippy.toml` allows `unwrap`/`panic`/indexing in tests only. Avoid Rust `unsafe` (workspace lints warn on `unsafe_code`); where it is unavoidable, keep the block minimal and justify it with a `// SAFETY:` comment (`undocumented_unsafe_blocks`).
- Treat the sandbox/exec boundary as a primary adversarial surface: use sandbox-aware launch paths and add adversarial regression coverage for command injection, path/symlink escape, environment leakage, and fail-closed behavior.
- **All built-in themes must meet WCAG AA 4.5:1 contrast** for foreground and all accent fields against background. Validate with `cargo nextest run -p vtcode-ui -E 'test(theme)'`. See `.vtcode/memory/gotchas.md` for catppuccin-latte special-case.
- **Every new major feature must update docs**: user-facing behavior in `docs/development/` plus its quick-reference; tool surface in prompt guidance (`vtcode-core/src/prompts/guidelines/`) and schema (`vtcode-utility-tool-specs`); runtime contract in `ThreadEvent`.
- DO NOT read or modify user's TODO.md (docs/project/TODO.md). This file is for the user's personal task management and should remain private and untouched by the agents.

## Self-Debugging (VT Code fixing itself)

- When a bug appears in VT Code's own behavior, **fix VT Code — do not work around it yourself.** Patch the VT Code source that caused the bug; never substitute a manual workaround, wrapper script, or config shim in place of a source fix.
- Before proposing a fix, read the trajectory log at `.vtcode/logs/trajectory.jsonl`, `.vtcode/checkpoints`, `<repo>/.vtcode/sessions` to see what actually happened (tool calls, errors, retries) rather than guessing from the symptom.
- Every self-bug fix must land in the **shipped surfaces** the release binary carries — runloop/tool logic and compiled prompts (`crates/codegen/vtcode-core/src/prompts/`) — not only in workspace-local files (AGENTS.md, docs), which affect this repository alone. Changing compiled guidance requires updating its budget/presence test.

## Detailed Guides

- Development, testing, evals: [README](docs/development/README.md), [SETUP](docs/development/DEVELOPMENT_SETUP.md), [testing](docs/development/testing.md), [pty-testing](docs/guides/pty-integration-testing.md), [eval](docs/guides/eval.md), [desktop workspace](docs/development/desktop-workspace.md).
- CI/release + models: [ci-cd](docs/development/ci-cd.md), [toolchain/MSRV](docs/development/toolchain-updates.md), [CHANGELOG](docs/development/CHANGELOG_GENERATION.md), [ADDING_MODELS](docs/development/ADDING_MODELS.md), [WORKFLOW](docs/development/MODEL_ADDITION_WORKFLOW.md), [CHECKLIST](docs/development/MODEL_ADDITION_CHECKLIST.md).
- Architecture + tools/security: [code-organization](docs/guides/code-organization-patterns.md), [async](docs/guides/async-architecture.md), [perf](docs/development/rust-performance-principles.md), [grep-guide](docs/development/grep-tool-guide.md), [grep-ref](docs/development/grep-quick-reference.md), [COMMAND_SECURITY](docs/development/COMMAND_SECURITY_MODEL.md), [security](docs/guides/security.md).
- WebMCP + harness: [webmcp](docs/development/webmcp.md), [agent-loop](docs/guides/agent-loop-contract.md), [harness INDEX](docs/harness/INDEX.md), [CORE_BELIEFS](docs/harness/CORE_BELIEFS.md), [INVARIANTS](docs/harness/ARCHITECTURAL_INVARIANTS.md), [LEGIBILITY](docs/harness/AGENT_LEGIBILITY_GUIDE.md).
- Prompt/runtime + planning/automation + loop: [runtime-guidance](docs/development/runtime-guidance.md), [planning](docs/guides/planning-workflow.md), [full-auto](docs/guides/full-automation.md), [EXECUTION_POLICY](docs/development/EXECUTION_POLICY.md), [loop-eng](docs/project/PLAN-loop-engineering.md).

## Workspace

Cargo workspace, 25 crates. Rust stable, MSRV 1.99.1, edition 2024. `default-members` = root, `vtcode-core`, `vtcode-ui` only.

Crate roles live in each crate's `Cargo.toml` `description` and [ARCHITECTURE](docs/ARCHITECTURE.md#workspace-crates). Not obvious from the code: `vtcode-desktop` is a non-publishable GPUI app while `vtcode-desktop-protocol` is published (the `vtcode` binary depends on it); `vtcode-llm` and `vtcode-skills` are partial extractions from `vtcode-core`.

New reusable logic: put it in an existing small crate or a new one. Keep it out of `vtcode-core` by default unless tightly coupled to the core runtime.

## Per-Module Guidance

Every crate has its own AGENTS.md with crate-specific conventions, next to its `Cargo.toml` (the root binary's is [src/AGENTS.md](src/AGENTS.md)); list them with `git ls-files '*AGENTS.md'`.

After significant changes, use the `audit-module-agents` skill. Keep each module `AGENTS.md` near 30 short lines (none over 600 characters; enforced); put longer detail in `docs/development/maintainer-notes/<crate>.md`.

## Project Memory

Session-independent knowledge lives in `.vtcode/memory/` (gitignored, repo-local only): `gotchas.md`, `issues.md`, `library.md`, `decisions.md`, `scratch.md`. Read them when context is needed; write durable learnings there. The `openai/codex` harness pattern lives in `library.md` (2026-09-24). See `.vtcode/memory/README.md` for format rules.

## Build & Verification

- CI: overlap independent setup steps, keep Cargo steps sharing `target/` sequential, and key `Swatinem/rust-cache` per target in cross-target matrices ([ci-cd](docs/development/ci-cd.md)).
- Prefer `./scripts/check-dev.sh` (10-30s) over `./scripts/check.sh` (2-5m) for iteration.
- Release builds keep `debug-assertions` and `overflow-checks` on: a violated invariant must crash loud. Gate expensive diagnostics behind `#[cfg(debug_assertions)]`.

| Change | Command |
| --- | --- |
| Fast gate | `./scripts/check-dev.sh` |
| + tests (quick) | `./scripts/check-dev.sh --test` |
| + tests (changed crates) | `./scripts/check-dev.sh --changed` |
| + workspace | `./scripts/check-dev.sh --workspace` |
| + lints | `./scripts/check-dev.sh --lints` |
| Harness PTY/TUI | `./scripts/check.sh harness` |
| Release/PR | `./scripts/check.sh` |
| Unsafe-docs gate | `./scripts/check.sh unsafe-docs` |
| Hawk dead-code/visibility | `./scripts/hawk.sh` |
| Hawk (deny warnings) | `./scripts/hawk.sh --deny` |

## Testing

- Runner: `cargo nextest run` (parallel, fast). **Always use nextest — never `cargo test`**.
- Single test/crate: `cargo nextest run test_name` / `cargo nextest run -p vtcode-core`. Profiles: `default` (full), `quick` (TDD, skips integration/e2e/slow), `changed` (delta since HEAD~1), `ci` (retries flaky, no fail-fast).
- Harness regressions: `cargo nextest run -p vtcode-core -E 'binary(/pty_tests/)'`; `cargo nextest run -p vtcode-bash-runner -E 'binary(/pipe_tests/)'`; `cargo nextest run --locked -p vtcode -E 'test(inline_events)'`.
- Test layout: small suites in-module; larger ones in a private `#[cfg(test)] mod tests;` after all items (`foo.rs` and `foo/mod.rs` both load `foo/tests.rs`). Integration tests in `tests/`, script tests in `scripts/tests/`.
- Test visibility: keep fixtures in their test module and use the narrowest `pub(super)`; never widen APIs, add dependencies, or change production behavior just for tests.
- Test evidence: assert independently derived outcomes with asymmetric inputs and both sides of boundaries; for Tokio task IDs, `tokio::spawn` the tested future ([root notes](docs/development/maintainer-notes/root.md#testing)).
- Verification scope: focused locked nextest filters with a nonzero match count, then broaden by risk; report passed, failed, and excluded tests. Docs-only edits need Markdown and diff checks.

## Skills & Special Workflows

- Project skills live in `.agents/skills/<name>/SKILL.md`; only `rust-skills` is linked into `.claude/skills/`. Where an agent does not load `.agents/skills/`, read the named `SKILL.md` directly.
- LLM providers: use the `adding-llm-providers` skill. The `/model` picker uses `ModelId::all_models()`; `builtin_model_presets()` is used by `ModelsManager`. Both may need updates.
- New workspace crates: use the `adding-workspace-crate` skill. This affects more than `Cargo.toml`; all workspace path dependencies need `version` fields.
- Structural code work: native `code_search` for declarations, usages, and paths; `ast-grep` (never `sg`) only as an optional developer tool for AST-shape codemods; `rg` for prose and config. All are developer tools, not runtime deps. GitHub: prefer `gh` over webfetch; on 403 run `unset GITHUB_TOKEN && gh auth switch -u vinhnx` ([root notes](docs/development/maintainer-notes/root.md#skills--special-workflows)).
- Cap large output with `| head -c 4000`, but never pipe verifier commands (it masks their exit status); in VT Code pass `max_output_tokens`.
