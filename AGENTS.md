<!-- markdownlint-disable MD013 -->

# AGENTS.md

Keep this file concise and under 150 lines. Root guidance belongs here; detailed explanations belong in `docs/`, skills, `.vtcode/memory/`, or crate-local `AGENTS.md` files. Universal model-facing behavior is compiled in `crates/codegen/vtcode-core/src/prompts/runtime_guidance.rs`. Keep this file and module `AGENTS.md` files focused on project and maintainer guidance; dynamically loaded instruction files are user-controlled context, not a security boundary. This workflow is repo-only: do not copy it to other projects or into shipped prompts.

## Core Workflow (repo-only)

- Orient: start with repository instructions, relevant local source/tests, and `.vtcode/memory/` (`gotchas.md`, `issues.md`, `library.md`, `decisions.md`). Search for task-relevant entries and read matching sections; avoid concatenating the full memory corpus. Routine or self-contained tasks need only this local grounding. For self-bugs inspect relevant events in `.vtcode/logs/trajectory.jsonl`, `.vtcode/checkpoints`, and `.vtcode/sessions`.
- Research complex/hard problems: consult DeepWiki MCP (`deepwiki` in `.mcp.json` / `vtcode.toml [mcp.providers]`) for `vinhnx/vtcode` (the local baseline) and `openai/codex` (upstream comparison) only when a problem is complex or hard; routine work uses local source and tests. Reuse prior research, attribute sources, and verify local claims against the current checkout and upstream claims against source/tests pinned to a commit; indexes are discovery aids. Apply findings only within the original authorization. If research is unavailable, use local evidence and state the gap. See the [reference workflow](docs/development/codex-reference-workflow.md) for pinned-source fallbacks and the full decision loop.
- Read and apply: use relevant skills, especially `read-and-apply` for upstream resources, `rust-skills`/`agentic-testing` for implementation/regressions, and `verification-discipline` for contracts, invariants, overflow and `unsafe` hardening. Compare upstream patterns with existing VT Code code and tests, identify confirmed gaps, and implement the smallest complete DRY/KISS improvement. Reuse existing patterns; preserve `ThreadEvent`, the harness config split, sandbox/approval boundaries, conflict checks, rollback, durable history, and unrelated work. Cite the upstream source path + commit when adapting a pattern (`codex-rs/...`, Apache-2.0 © 2025 OpenAI, <https://github.com/openai/codex>); preserve notices and document substantive changes per Apache-2.0 §4, extending `scripts/templates/third-party-header.txt` and regenerating notices for substantive copies.
- Verify with `./scripts/check-dev.sh` + `cargo nextest run`; record durable learnings in `.vtcode/memory/` (local-only, gitignored). Interview the user on plan open questions via the interactive question tool (one at a time, recommended answer first) instead of assuming defaults.

## Rules

- Conventional Commits (`type(scope): subject`).
- 4-space indentation, `snake_case` fns, `PascalCase` types, `anyhow::Result<T>` + `.with_context()`. Follow the [Rust style adoption matrix](docs/development/rust-style-guide.md) for flexible borrows, targeted `must_use`, API conventions, and exceptions.
- CI sets `RUSTFLAGS: "-D warnings"` and uses `--locked`. Match locally with `cargo check --locked` when relevant.
- Keep changes surgical: identify the production, test, and documentation files owned by the current increment before editing. Preserve existing APIs and unrelated work; inspect generated/hook-added changes, stage explicit reviewed paths rather than the whole worktree, and check the staged diff and final status for each scoped commit.
- Hard ceiling: no tracked text file may exceed 1000 lines (`python3 scripts/check_file_length.py --mode enforce`, gated by `check-dev.sh`/`check.sh`/pre-commit hook). Park generated/vendored outliers in `scripts/file_length_allowlist.txt` as `glob=MAX_LINES`; never allowlist first-party source without a split plan.
- Prefer direct single-agent execution. Do not impose orchestrator/worker or other multi-agent topologies: forced delegation burns usage limits and usually yields worse results. Delegate only when independent/parallel work or context isolation clearly helps.
- `vtcode-exec-events::ThreadEvent` is the authoritative runtime event contract — do not invent parallel types.
- Harness config is split across `agent.harness`, `automation.full_auto`, `context.dynamic` — do not add a new top-level harness subsystem.
- Prefer `compact_str::CompactString` (aliased as `CompactStr` in `vtcode_core::types`) over `String` for small string fields. Use `Cow<'static, str>` for mostly-static return strings.
- **Shape-suffix naming**: encode the dimensional structure of data in variable/type names. For feature vectors, document a **dimension key** (table of index → name → meaning). For bare tuples holding structured data, promote to named structs so the shape is explicit in the type system (inspired by Noam Shazeer's shape-suffix convention).
- `clippy.toml` allows `unwrap`/`panic`/indexing in tests only. Avoid Rust `unsafe` (workspace lints warn on `unsafe_code`); where it is unavoidable, keep the block minimal and justify it with a `// SAFETY:` comment (`undocumented_unsafe_blocks`).
- Treat the sandbox/exec boundary as a primary adversarial surface: use sandbox-aware launch paths and add adversarial regression coverage for command injection, path/symlink escape, environment leakage, and fail-closed behavior.
- **All built-in themes must meet WCAG AA 4.5:1 contrast** for foreground and all accent fields against background. Validate with `cargo nextest run -p vtcode-ui -E 'test(theme)'`. See `.vtcode/memory/gotchas.md` for catppuccin-latte special-case.
- **Every new major feature must update docs**: user-facing behavior → `docs/development/` guide + a table row/section in the relevant quick-reference; agent-facing tool surface → prompt guidance (`crates/codegen/vtcode-core/src/prompts/guidelines/`) + schema (`crates/common/vtcode-utility-tool-specs`); runtime contract → `vtcode-exec-events::ThreadEvent`. No feature is "done" until the docs it changes are updated and the AGENTS.md detailed-guides links still resolve.
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

After significant changes (new modules, convention shifts, discovered gotchas, public API changes), use the `audit-module-agents` skill to check if the affected crate's AGENTS.md needs updating. Keep each local AGENTS.md to about 30 short lines (none over 600 characters, enforced by `scripts/check_docs_links.py`) holding the module map, rules, and gotchas a session cannot derive from the code; put longer contracts and incident detail in `docs/development/maintainer-notes/<crate>.md` and link it.

## Project Memory

Session-independent knowledge lives in `.vtcode/memory/` (gitignored, repo-local only): `gotchas.md`, `issues.md`, `library.md`, `decisions.md`, and `scratch.md`. Read these files when context is needed. Write durable learnings there. Harness/agent-logic-only `openai/codex` pattern lives in `library.md` (2026-09-24). See `.vtcode/memory/README.md` for format rules.

## Build & Verification

- CI parallelism: overlap independent setup/I/O steps with `parallel` after toolchain setup; use `cache-bin: false` when cache restoration overlaps tool installers. Keep Cargo commands sharing `target/` sequential (`webmcp.yml` runs its typecheck and test steps in parallel). Verify barriers, failure propagation, and hosted timings; see [ci-cd](docs/development/ci-cd.md). CI build caching: always namespace `Swatinem/rust-cache` per target (`with: { key: ${{ matrix.target }} }`) in cross-target matrix jobs; see "Cross-target cache keys" in [ci-cd](docs/development/ci-cd.md).
- Prefer `./scripts/check-dev.sh` (10-30s) over `./scripts/check.sh` (2-5m) for iteration.
- Release builds keep `debug-assertions = true` and `overflow-checks = true` in `[profile.release]`. `debug_assert!` and overflow checks are NOT disabled in prod: a violated invariant must crash loud, not let the program run under wrong assumptions (see kristoff.it/blog/fix-your-asserts). Use `assert!`/`debug_assert!` for invariants that always hold; gate expensive diagnostics behind `#[cfg(debug_assertions)]` since that branch still compiles out of release when the flag is off elsewhere.

| Change                    | Command                                              |
| ------------------------- | ---------------------------------------------------- |
| Fast gate                 | `./scripts/check-dev.sh`                             |
| + tests (quick)           | `./scripts/check-dev.sh --test`                      |
| + tests (changed crates)  | `./scripts/check-dev.sh --changed`                   |
| + workspace               | `./scripts/check-dev.sh --workspace`                 |
| + lints                   | `./scripts/check-dev.sh --lints`                     |
| Harness PTY/TUI           | `./scripts/check.sh harness`                         |
| Release/PR                | `./scripts/check.sh`                                 |
| Unsafe-docs gate          | `./scripts/check.sh unsafe-docs`                     |
| Ast-grep rules            | `vtcode check ast-grep`                              |
| Hawk dead-code/visibility | `./scripts/hawk.sh`                                  |
| Hawk (deny warnings)      | `./scripts/hawk.sh --deny`                           |
| Ast-grep scan             | `ast-grep scan` (requires `sgconfig.yml` + `rules/`) |

## Testing

- Runner: `cargo nextest run` (parallel, fast). **Always use nextest — never `cargo test`**.
- Single test/crate: `cargo nextest run test_name` / `cargo nextest run -p vtcode-core`. Profiles: `default` (full), `quick` (TDD, skips integration/e2e/slow), `changed` (delta since HEAD~1), `ci` (retries flaky, no fail-fast).
- Harness regressions: `cargo nextest run -p vtcode-core -E 'binary(/pty_tests/)'`; `cargo nextest run -p vtcode-bash-runner -E 'binary(/pipe_tests/)'`; `cargo nextest run --locked -p vtcode -E 'test(inline_events)'`.
- Test layout: keep small unit suites in-module; move substantial suites to a private `#[cfg(test)] mod tests;` declared after all production items (avoids `clippy::items-after-test-module`). Inspect the actual module tree before choosing paths: both `foo.rs` and `foo/mod.rs` load `mod tests;` from `foo/tests.rs`. Integration tests use the crate's existing `tests/`; shell/script tests use `scripts/tests/`.
- Test visibility: keep helpers and fixtures within their owning test module; use private child-module access or the narrowest required `pub(super)` for production siblings. Do not widen public APIs, add dependencies, or alter production behavior solely to make tests convenient. During extraction, retain public-entrypoint regressions and verify the moved logic and remaining facade preserve the original contract.
- Test evidence: assert independently derived outcomes with asymmetric inputs and both sides of boundaries; cover ordering, cleanup, cancellation, and rejected-call recovery when relevant. For Tokio task-ID behavior, explicitly `tokio::spawn` the tested future; the `#[tokio::test]` main future does not itself establish a spawned-task ID. Keep thread-fallback coverage separate, and never clear shared global state while other tests may own entries.
- Verification scope: start with focused locked nextest filters, confirm a nonzero matched test count, then broaden to the affected module/crate in proportion to risk. Run final checks after edits stabilize; report passed, failed, and excluded tests plus untested host/platform behavior. Documentation-only edits need Markdown and diff checks rather than an unrelated Rust rebuild.

## Skills & Special Workflows

- Project skills live in `.agents/skills/<name>/SKILL.md`; only `rust-skills` is linked into `.claude/skills/`. Where an agent does not load `.agents/skills/`, read the named `SKILL.md` directly.
- LLM providers: use the `adding-llm-providers` skill. The `/model` picker uses `ModelId::all_models()`; `builtin_model_presets()` is used by `ModelsManager`. Both may need updates.
- New workspace crates: use the `adding-workspace-crate` skill. This affects more than `Cargo.toml`; all workspace path dependencies need `version` fields.
- Structural code work: prefer `ast-grep` over text grep for code shape, calls, impls, and codemods. Use `rg` for prose, logs, and config strings. Always invoke `ast-grep`, not the `sg` alias. In VT Code, use `exec_command` or the ast-grep skill for arbitrary structural patterns; its advanced `code_search` accepts one literal query and bounded filters. Rust pattern catalog: <https://ast-grep.github.io/catalog/rust/>. GitHub refs: prefer `gh` CLI over `webfetch` (`gh` authenticates as the exported-token account — on 403/admin-rights failures, run `unset GITHUB_TOKEN && gh auth switch -u vinhnx` before retrying); for internal improvements, follow the Codex research workflow in Core Workflow.
- Cap large command output: `COMMAND 2>&1 | head -c 4000` — but never pipe verifier commands (`cargo check --locked`, `cargo fmt --all -- --check`, `cargo nextest run`); in VT Code pass `max_output_tokens` instead, since a pipe masks the verifier's exit status and never clears the verification gate.
