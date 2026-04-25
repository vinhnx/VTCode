# AGENTS.md

## Use This File For

- Repo-wide workflow and placement decisions.
- Open `docs/ARCHITECTURE.md` only when the task spans crates, touches runtime boundaries, or you need repo orientation.
- Prefer module-local docs over broad repo exploration when working in one area.

## Core Rules

- Keep changes surgical and behavior-preserving.
- Use Conventional Commits (`type(scope): subject`).
- Match CI expectations in `.github/workflows/`.
- Rust uses 4-space indentation, snake_case functions, PascalCase types, and `anyhow::Result<T>` with `.with_context()` on fallible paths.
- Measure before optimizing.
- When ownership or lifetimes get tangled, prefer explicit handles/IDs plus an owning context.
- Do not reach for raw pointers, custom `Send`/`Sync`, or lifetime-branding patterns unless simpler handle-based designs are insufficient; document the invariant if you do.
- If this repo includes or adds C/C++ surfaces, follow [`docs/development/CPP_CORE_GUIDELINES_ADOPTION.md`](docs/development/CPP_CORE_GUIDELINES_ADOPTION.md).

## Verification

Use `./scripts/check-dev.sh` during development. Do not use `./scripts/check.sh` for routine iteration.

| If you changed... | Run |
| --- | --- |
| A focused code path and you want the default fast gate | `./scripts/check-dev.sh` |
| Logic covered by tests or you added tests | `./scripts/check-dev.sh --test` |
| Multiple crates or shared code | `./scripts/check-dev.sh --workspace` |
| Extra lint-sensitive code paths | `./scripts/check-dev.sh --lints` |
| GitHub workflows or workflow-security-sensitive scripts | `./scripts/check.sh workflow-security` |
| Ast-grep rules or scan scaffolding (`sgconfig.yml`, `rules/`) | `vtcode check ast-grep` |
| PTY/TUI harness paths called out in `docs/harness/QUALITY_SCORE.md` | `./scripts/check.sh harness` |
| Release validation, final PR validation, or reviewer/CI explicitly asked | `./scripts/check.sh` |

Use `cargo check`, `cargo nextest run`, `cargo fmt`, and `cargo clippy` when you need a narrower command for a specific crate or faster debugging loop.

## Code Placement

Repository shape:
- Main code lives in `src/`, `vtcode-core/`, `vtcode-tui/`, and `tests/`.

Choose placement before adding code:

| Situation | Preferred location |
| --- | --- |
| Reusable logic that does not need to live in the core runtime | An existing smaller crate, or a new small crate |
| Code tightly coupled to existing `vtcode-core` runtime responsibilities | `vtcode-core` |
| Unsure whether new reusable logic belongs in `vtcode-core` | Keep it out of `vtcode-core` by default |

## Implementation Notes

- Prefer simple algorithms and control flow until the workload justifies extra complexity.
- Keep new abstractions proportional to current use; do not generalize single-use code.
- Preserve existing APIs and behavior unless the task requires a change.

<!-- codemod-skill-discovery:begin -->
## Codemod Skill Discovery
This section is managed by `codemod` CLI.

- Core skill: `.agents/skills/codemod/SKILL.md`
- Package skills: `.agents/skills/<package-skill>/SKILL.md`
- Codemod MCP: use it for JSSG authoring guidance, CLI/workflow guidance, import-helper guidance, and semantic-analysis-aware codemod work.
- List installed Codemod skills: `npx codemod ai list --harness codex --format json`

<!-- codemod-skill-discovery:end -->
