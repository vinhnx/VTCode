# AGENTS.md

## Rules

- Conventional Commits (`type(scope): subject`).
- 4-space indentation, `snake_case` fns, `PascalCase` types, `anyhow::Result<T>` + `.with_context()`.
- CI sets `RUSTFLAGS: "-D warnings"` and uses `--locked`. Match locally with `cargo check --locked`.
- Keep changes surgical. Preserve existing APIs unless the task requires a change.
- `vtcode-exec-events::ThreadEvent` is the authoritative runtime event contract — do not invent parallel types.
- Harness config is split across `agent.harness`, `automation.full_auto`, `context.dynamic` — do not add a new top-level harness subsystem.
- Prefer `compact_str::CompactString` (aliased as `CompactStr` in `vtcode_core::types`) over `String` for small string fields (tool names, status labels, HashMap keys, event IDs). Use `Cow<'static, str>` for functions returning mostly static strings.
- `clippy.toml` allows `unwrap`/`panic`/indexing in tests only.
- Dev profile has `incremental = false` (sccache). Set `CARGO_INCREMENTAL=1` to override.

## Workspace

Cargo workspace, ~26 crates. `default-members` = root, `vtcode-core`, `vtcode-tui` only.

| Crate | Role |
|---|---|
| `vtcode` (root `src/`) | Binary, CLI, session bootstrap |
| `vtcode-core` | Agent loop, tools, prompts, LLM orchestration, UI |
| `vtcode-tui` | Public TUI API surface |
| `vtcode-design` | Centralized design system: color, style, layout, base widgets |
| `vtcode-llm` | LLM provider abstraction (publish=false) |
| `vtcode-tools` | Tool registry and implementations (publish=false) |
| `vtcode-config` | Config loading and schema |
| `vtcode-bash-runner` | Shell execution sandbox |
| `vtcode-acp` | Agent Client Protocol (Zed) |
| `vtcode-auth` | OAuth and credential storage |
| `vtcode-indexer` | Code indexing and search |
| `vtcode-process-hardening` | OS sandboxing (Seatbelt, Landlock) |
| `vtcode-exec-events` | `ThreadEvent` contract and ATIF export |
| `vtcode-commons` | Shared utilities |
| `vtcode-macros` | Procedural macros |
| `vtcode-markdown-store` | Markdown storage and rendering |
| `vtcode-terminal-detection` | Terminal detection primitives |
| `vtcode-theme` | Theme definitions and constants |
| `vtcode-utility-tool-specs` | JSON schemas for utility/file tools |
| `vtcode-collaboration-tool-specs` | JSON schemas for collaboration/HITL tools |
| `vtcode-file-search` | Parallel fuzzy file search |
| `vtcode-vim` | Vim-style prompt editing engine |
| `vtcode-lmstudio` | LM Studio integration (publish=false) |
| `xtask` | Release packaging automation |

New reusable logic: put it in an existing small crate or a new one. Keep it out of `vtcode-core` by default unless tightly coupled to the core runtime.

## Per-Module Guidance

Every crate has its own AGENTS.md with crate-specific conventions:

| Crate | AGENTS.md |
|---|---|
| `vtcode` (binary) | [src/AGENTS.md](src/AGENTS.md) |
| `vtcode-core` | [vtcode-core/AGENTS.md](vtcode-core/AGENTS.md) |
| `vtcode-tui` | [vtcode-tui/AGENTS.md](vtcode-tui/AGENTS.md) |
| `vtcode-design` | [vtcode-design/AGENTS.md](vtcode-design/AGENTS.md) |
| `vtcode-config` | [vtcode-config/AGENTS.md](vtcode-config/AGENTS.md) |
| `vtcode-llm` | [vtcode-llm/AGENTS.md](vtcode-llm/AGENTS.md) |
| `vtcode-tools` | [vtcode-tools/AGENTS.md](vtcode-tools/AGENTS.md) |
| `vtcode-bash-runner` | [vtcode-bash-runner/AGENTS.md](vtcode-bash-runner/AGENTS.md) |
| `vtcode-acp` | [vtcode-acp/AGENTS.md](vtcode-acp/AGENTS.md) |
| `vtcode-auth` | [vtcode-auth/AGENTS.md](vtcode-auth/AGENTS.md) |
| `vtcode-indexer` | [vtcode-indexer/AGENTS.md](vtcode-indexer/AGENTS.md) |
| `vtcode-exec-events` | [vtcode-exec-events/AGENTS.md](vtcode-exec-events/AGENTS.md) |
| `vtcode-commons` | [vtcode-commons/AGENTS.md](vtcode-commons/AGENTS.md) |
| `vtcode-process-hardening` | [vtcode-process-hardening/AGENTS.md](vtcode-process-hardening/AGENTS.md) |
| `vtcode-macros` | [vtcode-macros/AGENTS.md](vtcode-macros/AGENTS.md) |
| `vtcode-markdown-store` | [vtcode-markdown-store/AGENTS.md](vtcode-markdown-store/AGENTS.md) |
| `vtcode-terminal-detection` | [vtcode-terminal-detection/AGENTS.md](vtcode-terminal-detection/AGENTS.md) |
| `vtcode-theme` | [vtcode-theme/AGENTS.md](vtcode-theme/AGENTS.md) |
| `vtcode-utility-tool-specs` | [vtcode-utility-tool-specs/AGENTS.md](vtcode-utility-tool-specs/AGENTS.md) |
| `vtcode-collaboration-tool-specs` | [vtcode-collaboration-tool-specs/AGENTS.md](vtcode-collaboration-tool-specs/AGENTS.md) |
| `vtcode-file-search` | [vtcode-file-search/AGENTS.md](vtcode-file-search/AGENTS.md) |
| `vtcode-vim` | [vtcode-vim/AGENTS.md](vtcode-vim/AGENTS.md) |
| `vtcode-lmstudio` | [vtcode-lmstudio/AGENTS.md](vtcode-lmstudio/AGENTS.md) |
| `xtask` | [xtask/AGENTS.md](xtask/AGENTS.md) |

**Maintaining per-module files**: After significant changes (new modules, convention shifts, discovered gotchas), use the `audit-module-agents` skill to check if the affected crate's AGENTS.md needs updating. Keep each file under 30 lines — only document what's unique to that crate. This workflow is also referenced in `CLAUDE.md` for persistent agent memory.

## Project Memory

Session-independent knowledge lives in `.vtcode/memory/` (gitignored). Use it to persist learnings across sessions:

| File | Use for |
|---|---|
| `gotchas.md` | Non-obvious behaviors, pitfalls, workarounds |
| `issues.md` | Recurring problems, flaky tests, env-specific failures |
| `library.md` | Useful patterns, snippets, idioms |
| `decisions.md` | Architecture decisions, trade-offs, rationale |
| `scratch.md` | Ephemeral notes — safe to clear |

Read these files at session start when context is needed. Write to them when you discover something worth persisting. See `.vtcode/memory/README.md` for format rules.

## Skills & Subagents

### Development Workflow Skills

| Skill | Use when |
|---|---|
| `architect` | Starting a new feature with BDD-TDD workflow (creates spec, Gherkin scenarios, TDD prompts) |
| `coder` | Implementing features with orchestrated task breakdown and quality gates |
| `run-prompt` | Executing saved prompts from `./prompts/` (auto-detects TDD/direct/research) |
| `debugger` | Forensic root cause analysis (read-only investigation mode) |
| `refactor` | Improving code quality without changing functionality |
| `fix-failing-tests` | Running tests and auto-fixing failures |
| `verifier` | Investigating source code to verify claims or answer questions |
| `init-explorer` | Gathering project context before other agents run |

### Code Quality Skills

| Skill | Use when |
|---|---|
| `rust-skills` | Writing, reviewing, or refactoring Rust code (179 rules across 14 categories) |
| `pr-code-review` | PR review with bug triage, correctness review, and automated fix loops |

### Project Management Skills

| Skill | Use when |
|---|---|
| `adding-llm-providers` | Adding new LLM provider integrations |
| `adding-workspace-crate` | Adding new crates to the workspace |
| `audit-module-agents` | Checking if per-module AGENTS.md files need updating |
| `deep-research` | Multi-source, fact-checked research on any topic |
| `eval-skill` | Running test cases to validate skill quality |
| `skill-creator` | Creating new skills or modifying existing ones |
| `update-config` | Configuring Claude Code harness settings |

### Subagents (`.claude/agents/`)

Specialized agents for orchestrated workflows:

| Agent | Role |
|---|---|
| `init-explorer` | Initializer - explores codebase and sets up context |
| `architect` | Greenfield spec designer |
| `bdd-agent` | BDD specialist - generates Gherkin scenarios |
| `scope-manager` | Complexity gatekeeper for BDD features |
| `gherkin-to-test` | Converts Gherkin to TDD prompts |
| `codebase-analyst` | Finds reuse opportunities |
| `refactor-decision-engine` | Decides if refactoring is needed |
| `test-creator` | TDD specialist - writes tests first |
| `coder` | Implementation specialist |
| `coding-standards-checker` | Code quality verifier |
| `tester` | Functionality verification |
| `bdd-test-runner` | Test infrastructure validator |
| `refactorer` | Code refactoring specialist |
| `fix-failing-tests` | Fix failing tests specialist |
| `verifier` | Code investigation specialist |
| `stuck` | Human escalation agent |
| `debugger` | CRASH-RCA orchestrator |
| `forensic` | Investigation specialist for CRASH sessions |
| `analyst` | RCA synthesis specialist |

### Workflow Chains

**BDD-TDD (`/architect`)**: `init-explorer` -> `architect` -> `bdd-agent` -> `gherkin-to-test` -> `codebase-analyst` -> `refactor-decision` -> `test-creator` -> `coder` -> `standards` -> `tester` -> `bdd-test-runner`

**Direct Implementation (`/coder`)**: Orchestrator -> `coder` (per todo) -> `coding-standards-checker` -> `tester`

**Forensic Debugging (`/debugger`)**: `init-explorer` -> `crash.py start` -> hypothesis loop (read-only tools + `crash.py step`) -> `crash.py diagnose`

## Build & Verification

Rust stable, MSRV 1.88, edition 2024.

Prefer `./scripts/check-dev.sh` (10-30s) over `./scripts/check.sh` (2-5m) for iteration.

| Change          | Command                              |
| --------------- | ------------------------------------ |
| Fast gate       | `./scripts/check-dev.sh`             |
| + tests         | `./scripts/check-dev.sh --test`      |
| + workspace     | `./scripts/check-dev.sh --workspace` |
| + lints         | `./scripts/check-dev.sh --lints`     |
| Harness PTY/TUI | `./scripts/check.sh harness`         |
| Release/PR      | `./scripts/check.sh`                 |
| Ast-grep rules  | `vtcode check ast-grep`              |

Narrow commands: `cargo check`, `cargo nextest run`, `cargo fmt`, `cargo clippy`.

### Dev Run

```bash
./scripts/run-debug.sh   # debug build + launch
./scripts/run.sh         # release build + launch
```

Both auto-bootstrap Ghostty VT runtime.

## Testing

- **Runner**: `cargo nextest run` (parallel, fast). Fallback: `cargo test --workspace`.
- **Single test**: `cargo nextest run test_name` or `cargo test test_name`.
- **Single crate**: `cargo nextest run -p vtcode-core`.
- **Profiles**: `default` (local), `ci` (no fail-fast, 2 retries, 60s timeout), `quick` (TDD, skips integration/e2e).
- **Harness regressions** (run separately):
    ```
    cargo test -p vtcode-core --test pty_tests
    cargo test -p vtcode-bash-runner --test pipe_tests
    cargo test -p vtcode --bin vtcode inline_events::tests
    ```
- Integration tests: `tests/` at workspace root. Unit tests: in-module.

## LLM Provider

Use the `adding-llm-providers` skill. The `/model` picker uses `ModelId::all_models()` — `builtin_model_presets()` is a separate system used by `ModelsManager`. Both must be updated.

## Adding a New Workspace Crate

Use the `adding-workspace-crate` skill. Adding a crate touches more than just `Cargo.toml` — the release pipeline, documentation, and agent guidance all need updating. All path dependencies on workspace crates must include a `version` field for crates.io compatibility.

## Output

Cap large command output: `COMMAND 2>&1 | head -c 4000`
