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

Cargo workspace, ~26 crates. `default-members` = root, `vtcode-core`, `vtcode-ui` only.

| Crate | Role |
|---|---|
| `vtcode` (root `src/`) | Binary, CLI, session bootstrap |
| `vtcode-core` | Agent loop, tools, prompts, LLM orchestration, UI |
| `vtcode-ui` | Unified UI: design system, theme registry, TUI framework |
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
| `vtcode-ghostty-core` | Ghostty VT terminal emulator core and runtime bindings |
| `vtcode-macros` | Procedural macros |
| `vtcode-markdown-store` | Markdown storage and rendering |
| `vtcode-terminal-detection` | Terminal detection primitives |
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
| `vtcode-ui` | [vtcode-ui/AGENTS.md](vtcode-ui/AGENTS.md) |
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
| `vtcode-ghostty-core` | [vtcode-ghostty-core/AGENTS.md](vtcode-ghostty-core/AGENTS.md) |
| `vtcode-macros` | [vtcode-macros/AGENTS.md](vtcode-macros/AGENTS.md) |
| `vtcode-markdown-store` | [vtcode-markdown-store/AGENTS.md](vtcode-markdown-store/AGENTS.md) |
| `vtcode-terminal-detection` | [vtcode-terminal-detection/AGENTS.md](vtcode-terminal-detection/AGENTS.md) |
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

Skills are invoked via the Skill tool (slash commands). Subagents are spawned via the Agent tool. Some names (e.g. `coder`, `verifier`) exist as both a skill and an agent -- the skill is the entry point that orchestrates the agent.

### Built-in Skills (Claude Code product)

These ship with Claude Code and are not project-specific:

| Skill | Use when |
|---|---|
| `code-review` | Reviewing diffs for correctness bugs and reuse/simplification cleanups |
| `simplify` | Reviewing changed code for reuse, simplification, efficiency |
| `verify` | Confirming a change works by running the app and observing behavior |
| `security-review` | Security-focused code review |
| `review` | Reviewing a pull request |
| `run` | Launching and driving the app to see a change working |
| `init` | Initializing a new CLAUDE.md with codebase docs |
| `claude-api` | Claude API / Anthropic SDK reference |
| `update-config` | Configuring Claude Code harness settings (hooks, permissions, env vars) |
| `loop` | Running a prompt on a recurring interval |
| `keybindings-help` | Customizing keyboard shortcuts |
| `fewer-permission-prompts` | Adding allowlists to reduce permission prompts |

### Custom Skills (user/global -- private, not in public repo)

These are user-specific and **not checked into this repo**. Document them here for reference only.

| Skill | Source | Use when |
|---|---|---|
| `rust-skills` | `~/.agents/skills/` | 179 Rust coding rules across 14 categories |
| `deep-research` | User-level (private) | Multi-source, fact-checked research on any topic |
| `eval-skill` | `~/.claude/skills/` | Running test cases to validate skill quality |
| `skill-creator` | Plugin (`claude-plugins-official`) | Creating new skills or modifying existing ones |
| `pr-code-review` | Plugin + agent wrapper | PR review entry point (spawns `pr-code-review` agent in `.claude/agents/`) |

### Project Slash Commands (`.claude/commands/`)

These are project-level commands that appear as skills. They live in this repo's `.claude/commands/` directory.

| Skill | Use when |
|---|---|
| `architect` | BDD-TDD workflow (creates spec, Gherkin scenarios, TDD prompts) |
| `coder` | Orchestrated task breakdown and quality gates |
| `run-prompt` | Executing saved prompts from `./prompts/` |
| `debugger` | Forensic root cause analysis (read-only investigation mode) |
| `refactor` | Improving code quality without changing functionality |
| `fix-failing-tests` | Running tests and auto-fixing failures |
| `verifier` | Investigating source code to verify claims |
| `init-explorer` | Gathering project context before other agents run |

### Project Management Skills

These reference docs or workflows in this repo (not slash-command skills):

| Topic | Use when |
|---|---|
| `adding-llm-providers` | Adding new LLM provider integrations |
| `adding-workspace-crate` | Adding new crates to the workspace |
| `audit-module-agents` | Checking if per-module AGENTS.md files need updating |
| `ast-grep` | Structural code search, codemod, and lint via tree-sitter AST patterns |

### Subagents (`.claude/agents/`)

Specialized agents spawned via the Agent tool for orchestrated workflows:

| Agent | Role |
|---|---|
| `acceptance-qa` | User Acceptance Testing -- validates final implementation against requirements |
| `analyst` | RCA synthesis specialist |
| `architect` | Greenfield spec designer |
| `bdd-agent` | BDD specialist -- generates Gherkin scenarios |
| `bdd-test-runner` | Test infrastructure validator (Dockerfile.test, Makefile, `make test`) |
| `codebase-analyst` | Finds reuse opportunities |
| `coder` | Implementation specialist |
| `coding-standards-checker` | Code quality verifier |
| `debugger` | CRASH-RCA orchestrator |
| `fix-failing-tests` | Fix failing tests specialist |
| `forensic` | Investigation specialist for CRASH sessions |
| `gherkin-to-test` | Converts Gherkin to TDD prompts |
| `init-explorer` | Initializer -- explores codebase and sets up context |
| `pr-code-review` | PR review with bug triage and automated fix loops |
| `refactor-decision-engine` | Decides if refactoring is needed |
| `refactorer` | Code refactoring specialist |
| `requirements-qa` | Validates BDD features against original requirements |
| `rust-engineer` | Rust systems specialist (memory safety, zero-cost abstractions) |
| `scope-manager` | Complexity gatekeeper for BDD features |
| `strict-coder` | Implementation specialist following architectural constraints |
| `stuck` | Human escalation agent |
| `test-creator` | TDD specialist -- writes tests first |
| `tester` | Functionality verification |
| `tester-backend` | Backend testing specialist |
| `tester-frontend` | Frontend visual testing specialist |
| `verifier` | Code investigation specialist |

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
| Ast-grep scan   | `ast-grep scan` (requires `sgconfig.yml` + `rules/`) |

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

## Structural Code Tools

### ast-grep

Reach for `ast-grep` whenever the work is structural — finding, matching, or rewriting code by its shape rather than its text. ast-grep matches the AST through tree-sitter, so it sees real call expressions, declarations, and JSX nodes where grep only sees characters, and it speaks TypeScript, Python, Rust, Go, and more — one tool across the whole stack.

**Use it in two modes:**

- **Search and codemod.** Before editing many files by hand, write one ast-grep pattern. `$A` matches a single node, `$$$ARGS` matches many. A migration becomes a deterministic, auditable, reversible pass instead of forty edits that drift and miss sites.
  - find: `ast-grep -p 'foo($$$ARGS)' -l ts`
  - rewrite: `ast-grep -p '$A && $A()' -l ts -r '$A?.()'` (inspect the diff, then add `-U` to apply)
- **Lint gate.** When a repo carries `sgconfig.yml` + a `rules/` dir, run `ast-grep scan`; it enforces the project's banned patterns and exits non-zero on any error-severity match.

**When to prefer ast-grep over text grep:**

| Task | Tool |
|---|---|
| Find all calls to a function | `ast-grep -p 'function_name($$$)' -l rs` |
| Find all impl blocks for a trait | `ast-grep -p 'impl $TRAIT for $TYPE { $$$ }' -l rs` |
| Rename a function across call sites | `ast-grep -p 'old_name($$$)' -r 'new_name($$$)' -l rs` |
| Find deprecated macro usage | `ast-grep -p 'deprecated_macro!($$$)' -l rs` |
| Find all `unwrap()` calls | `ast-grep -p '$A.unwrap()' -l rs` |
| Search log messages, comments, config | text `Grep` (prose, not code shape) |

**Rust catalog**: https://ast-grep.github.io/catalog/rust/

**Project rules**: `rules/` directory at workspace root. Run `ast-grep scan` to enforce. Run `vtcode check ast-grep` via the project's check script.

Prefer `ast-grep` over grep/ripgrep for any code-shape question; keep text grep for prose, logs, and config strings. Always invoke the `ast-grep` command — the `sg` alias collides with the system `sg`.
