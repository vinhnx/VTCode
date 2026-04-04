# Guidance and Persistent Memory for VT Code

VT Code has two distinct memory surfaces:

- Authored guidance that you write in `AGENTS.md` and `.vtcode/rules/`.
- Learned per-repository memory that VT Code stores under your user config directory.

Understanding that split makes it easier to tune prompt quality without mixing durable project instructions with automatically learned notes.

## Authored Guidance

### Instruction sources and precedence

VT Code loads authored guidance from the lowest-precedence scope to the highest-precedence scope. Later and more specific sources win when they conflict.

| Load order | Scope | Location | Purpose |
| --- | --- | --- | --- |
| 1 | User `AGENTS.md` | `~/AGENTS.md`, `~/.vtcode/AGENTS.md`, `~/.config/vtcode/AGENTS.md` | Personal preferences that apply across repositories. |
| 2 | User unconditional rules | `~/.vtcode/rules/**/*.md` or `~/.config/vtcode/rules/**/*.md` without `paths` frontmatter | Always-on personal rules. |
| 3 | User matched rules | Same rule roots, but with `paths` frontmatter that matches the current instruction context | Personal rules that only load for relevant files. |
| 4 | Extra instruction files | Paths or globs from `agent.instruction_files` | Explicitly injected docs such as runbooks or local conventions. |
| 5 | Workspace `AGENTS.md` hierarchy | `<repo>/AGENTS.md` plus nested `AGENTS.md` files from repo root to the active instruction scope | Shared project guidance and subsystem overrides. |
| 6 | Workspace unconditional rules | `<repo>/.vtcode/rules/**/*.md` without `paths` frontmatter | Always-on repository rules. |
| 7 | Workspace matched rules | Same workspace rule roots, but with matching `paths` frontmatter | File- or directory-scoped repository rules. |

### Path-scoped rules

Rules inside `.vtcode/rules/` can use YAML frontmatter with a `paths` field:

```md
---
paths:
  - "src/**/*.rs"
  - "tests/**/*.rs"
---

# Rust Rules
- Keep changes surgical.
```

VT Code activates matched rules from the next prompt rebuild when any of these contexts include a matching path:

- the active editor file
- visible editor files
- the active instruction directory
- tracked session file activity such as reads, searches, and edits

### Imports and excludes

Authored guidance files can import other files inline with `@path/to/file.md`.

- Imports expand at the location of the `@path` line, not at the end of the file.
- Relative imports resolve from the containing file.
- The default recursive import limit is `5`, controlled by `agent.instruction_import_max_depth`.
- Imports are limited to the workspace or VT Code user-config roots.
- Use `agent.instruction_excludes` to skip specific `AGENTS.md` or `.vtcode/rules/` paths by glob.

## Persistent Memory

Persistent memory is VT Code's learned, per-repository memory store. It is separate from authored guidance, and VT Code injects only a compact startup summary after authored instructions.

### Storage layout

For each repository, VT Code stores memory under:

```text
~/.vtcode/projects/<project>/memory/
```

Older VT Code builds stored persistent memory under the general config root on some platforms, such as macOS Application Support. VT Code now migrates the legacy per-repository memory directory into `~/.vtcode/projects/<project>/memory/` the next time that repository memory is resolved.

The directory contains:

```text
memory/
├── memory_summary.md
├── MEMORY.md
├── preferences.md
├── repository-facts.md
└── rollout_summaries/
```

- `memory_summary.md` is the source file for the compact startup summary.
- `MEMORY.md` is the durable registry and index.
- `preferences.md` stores stable user and workflow preferences.
- `repository-facts.md` stores grounded repository and tooling facts.
- `rollout_summaries/` stores per-session evidence summaries before and after consolidation.

### Startup behavior

Persistent memory is disabled by default. Enable it with `/config memory` or by setting `agent.persistent_memory.enabled = true`.

When `agent.persistent_memory.enabled = true`, VT Code injects:

1. explicit user instructions
2. authored guidance
3. a compact prompt summary derived from the configured scan of `memory_summary.md`

The startup scan is controlled by:

- `agent.persistent_memory.startup_line_limit`
- `agent.persistent_memory.startup_byte_limit`

### Write flow

When `agent.persistent_memory.auto_write = true`, VT Code writes memory in two phases:

1. Session finalization writes one rollout summary into `rollout_summaries/`.
2. Consolidation merges pending rollout summaries into `preferences.md`, `repository-facts.md`, `MEMORY.md`, and `memory_summary.md`.

VT Code now treats LLM assistance as a hard requirement for memory mutation:

- natural-language `remember` and `forget` requests are planned through a structured LLM response
- session-finalization memory writes use the same LLM-assisted normalization path
- VT Code writes the files itself after validating the structured output
- if no memory LLM route is available, or the structured response is invalid, VT Code blocks the mutation and leaves memory unchanged

If `agent.small_model.use_for_memory = true`, VT Code prefers the configured lightweight-model route for memory planning, classification, cleanup, and summary refresh. Otherwise it uses the active session model/provider.

## Interactive Controls

### `/memory`

Use `/memory` as the memory-focused control surface.

- In inline UI, it shows loaded `AGENTS.md` sources, matched rules, memory status, file paths, and quick actions.
- Quick actions include toggling memory, toggling auto-write, toggling lightweight-memory routing, picking the memory triage model, scaffolding memory files, running one-time legacy cleanup, rebuilding the summary, opening the memory directory, and jumping to `/config memory`.
- In non-inline UI, `/memory` prints status plus exact follow-up commands such as `/config memory` and `/edit <target>`.
- `/memory` also shows whether cleanup is required because legacy raw prompts or serialized tool payloads were found in the memory store.

### Natural-language memory prompts

VT Code also detects explicit memory-management prompts before they go to the model.

- Prompts like `remember that I prefer pnpm`, `save to memory: use cargo nextest`, and `forget my pnpm preference` open a human-in-the-loop confirmation dialog in inline UI.
- VT Code sends the raw request through the memory planner first, then shows the normalized fact or exact deletion candidates before applying the change.
- If the request is underspecified, such as `save to memory and remember my name`, VT Code asks for the missing detail before it writes anything.
- Prompts like `show memory` or `what do you remember` route to the existing `/memory` surface instead of sending the request to the model.
- If cleanup is required, VT Code asks you to run the one-time cleanup before any memory mutation.
- If inline selection UI is unavailable, VT Code does not mutate memory and points you back to `/memory`.

### `/config memory`

Use `/config memory` to jump directly to the persistent-memory settings section. The same section is also reachable through `/config agent.persistent_memory`.

The focused controls cover:

- `agent.persistent_memory.enabled`
- `agent.persistent_memory.auto_write`
- `agent.persistent_memory.startup_line_limit`
- `agent.persistent_memory.startup_byte_limit`
- `agent.persistent_memory.directory_override`
- `agent.instruction_import_max_depth`
- `agent.instruction_excludes`
- `agent.small_model.use_for_memory`

`directory_override` is intentionally restricted to system, user, or project-profile config layers. A workspace-root `vtcode.toml` cannot redirect persistent memory storage.

For current-value fields such as startup line limits, byte limits, and import depth, pressing `Enter` on an empty inline input keeps the displayed value.

## `/init` and Scaffolding

`/init` still generates the root `AGENTS.md`, and now also scaffolds:

- `.vtcode/README.md`
- the per-repository memory directory layout

Use `/init --force` when you want to regenerate the root guidance file and refresh workspace scaffolding in one pass.

## Recommended Practices

- Keep authored guidance concise, reviewable, and intentionally human-written.
- Use `.vtcode/rules/` for modular project rules instead of growing one large `AGENTS.md`.
- Reserve persistent memory for reusable learned facts, not policy or mandatory coding standards.
- Prefer `/memory` for day-to-day memory inspection and quick actions.
- Prefer `/config memory` when you need to tune limits, excludes, or the storage location.
