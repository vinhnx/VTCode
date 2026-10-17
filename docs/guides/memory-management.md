# Guidance and Persistent Memory for VT Code

VT Code has two distinct memory surfaces:

- Authored guidance that you write in `AGENTS.md` and `.vtcode/rules/`.
- Learned per-repository memory that VT Code stores under your user state directory.

The canonical user config root is the platform config directory (for example `$XDG_CONFIG_HOME/vtcode`, defaulting to
`~/.config/vtcode` on Linux/BSD). The historical `VTCODE_HOME`/`~/.vtcode` tree remains readable for compatibility and
is preserved as a migration backup.

Understanding that split makes it easier to tune prompt quality without mixing durable project instructions with
automatically learned notes.

## Authored Guidance

### Instruction sources and precedence

VT Code loads authored guidance from the lowest-precedence scope to the highest-precedence scope. Later and more
specific sources win when they conflict.

| Load order | Scope                           | Location                                                                                                             | Purpose                                                         |
| ---------- | ------------------------------- | -------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| 1          | User `AGENTS.md`                | `~/AGENTS.md`, the canonical config directory's `AGENTS.md`, and legacy `~/.vtcode/AGENTS.md`                        | Personal preferences that apply across repositories.            |
| 2          | User unconditional rules        | The canonical config directory's `rules/**/*.md`, then legacy `~/.vtcode/rules/**/*.md`, without `paths` frontmatter | Always-on personal rules.                                       |
| 3          | User matched rules              | The same canonical and legacy rule roots, with `paths` frontmatter that matches the current instruction context      | Personal rules that only load for relevant files.               |
| 4          | Extra instruction files         | Paths or globs from `agent.instruction_files`                                                                        | Explicitly injected docs such as runbooks or local conventions. |
| 5          | Workspace `AGENTS.md` hierarchy | `<repo>/AGENTS.md` plus nested `AGENTS.md` files from repo root to the active instruction scope                      | Shared project guidance and subsystem overrides.                |
| 6          | Workspace unconditional rules   | `<repo>/.vtcode/rules/**/*.md` without `paths` frontmatter                                                           | Always-on repository rules.                                     |
| 7          | Workspace matched rules         | Same workspace rule roots, but with matching `paths` frontmatter                                                     | File- or directory-scoped repository rules.                     |

### Compiled runtime guidance

The static prompt also contains a small compiled runtime-guidance section from
`vtcode-core/src/prompts/runtime_guidance.rs`. It applies to every prompt profile and is separate from the authored
hierarchy above. `AGENTS.md`, `CLAUDE.md`, and rule files are user-controlled context: they can describe project
conventions, but they cannot override VT Code's tool policy, sandboxing, approvals, or other security controls.

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

Persistent memory is VT Code's learned, per-repository memory store. It is separate from authored guidance, and VT Code
injects only a compact startup summary after authored instructions.

### Retrieval and bounded event retention

Cross-session event search starts with deterministic BM25 relevance and applies only a mild recency multiplier:

```text
bm25 * (1 + 0.15 * 0.5^(age_days / 30))
```

Missing or invalid timestamps receive no boost. When the canonical event log exceeds its retention cap, VT Code
deterministically extracts a bounded set of grounded facts from completed canonical events and persists that eviction
summary before rewriting the log. If summary persistence fails, the event log is left untouched so recovery can retry
without losing evidence.

### Storage layout

For each repository, VT Code stores memory under:

```text
<user-state-directory>/projects/<project>/memory/
```

Older VT Code builds stored persistent memory under the general config root on some platforms, such as macOS Application
Support. VT Code now copies the legacy per-repository memory directory into the canonical state directory the next time
that repository memory is resolved, while preserving the source.

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

### Feature flag

Memories can also be controlled via a Codex-compatible `[features]` table in `vtcode.toml`:

```toml
[features]
memories = true
```

When `features.memories` is true **and** `agent.persistent_memory.enabled` is true, VT Code carries durable context from
completed threads into future sessions. The `[features]` toggle acts as the global master switch; the per-repo
`agent.persistent_memory.enabled` gates the actual storage layer.

### Memories sub-configuration

The `[agent.persistent_memory.memories]` table provides Codex-compatible controls for the extraction and injection
pipeline:

| Key                   | Type      | Default     | Purpose                                                                 |
| --------------------- | --------- | ----------- | ----------------------------------------------------------------------- |
| `generate_memories`   | `bool`    | `true`      | Whether completed threads can be stored as memory-generation inputs.    |
| `use_memories`        | `bool`    | `true`      | Whether existing memories are injected into future sessions.            |
| `extract_model`       | `string?` | agent model | Overrides the model used for per-thread memory extraction.              |
| `consolidation_model` | `string?` | agent model | Overrides the model used for global memory consolidation.               |
| `batch_sessions`      | `usize`   | `50`        | Recent sessions scanned by batch memory extraction.                     |
| `batch_concurrency`   | `usize`   | `8`         | Concurrent per-session reads during batch extraction (clamped to 1–16). |

### Startup behavior

Persistent memory is disabled by default. Enable it with `/config memory` or by setting
`agent.persistent_memory.enabled = true`.

When `agent.persistent_memory.enabled = true`, VT Code injects:

1. explicit user instructions
2. authored guidance
3. a compact prompt summary derived from the configured scan of `memory_summary.md`

The startup scan is controlled by:

- `agent.persistent_memory.startup_line_limit`
- `agent.persistent_memory.startup_byte_limit`
- `agent.persistent_memory.startup_token_budget` (applied last; `0` disables the token cap)

### Write flow

When `agent.persistent_memory.auto_write = true`, VT Code writes memory in two phases:

1. Session finalization writes one rollout summary into `rollout_summaries/`, and persists the session's grounded facts
   into the canonical session store (`<session>/derived/memory.json`), which is what cross-session fact queries and
   batch extraction read. Finalization runs as a spawned task: VT Code waits up to 5 seconds for the kickoff, then lets
   it finish in the background while the UI finalizes (the global memory lock serializes concurrent writers).
2. Consolidation merges pending rollout summaries into `preferences.md`, `repository-facts.md`, `MEMORY.md`, and
   `memory_summary.md`.

### Batch extraction

The `/config memory` palette includes **Batch Extract Memory From Past Sessions**, which reads the grounded-fact views
of the most recent `batch_sessions` sessions (concurrent reads bounded by `batch_concurrency`), dedupes them, and
consolidates the survivors into the global memory files under the same memory lock. Still-active sessions are skipped so
partial snapshots are never ingested. This is the equivalent of Codex's multi-thread memory sweep; run it after enabling
memory on a workspace with existing session history.

VT Code now treats LLM assistance as a hard requirement for memory mutation:

- natural-language `remember` and `forget` requests are planned through a structured LLM response
- session-finalization memory writes use the same LLM-assisted normalization path
- VT Code writes the files itself after validating the structured output
- if no memory LLM route is available, or the structured response is invalid, VT Code blocks the mutation and leaves
  memory unchanged

If `agent.small_model.use_for_memory = true`, VT Code prefers the configured lightweight-model route for memory
planning, classification, cleanup, and summary refresh. Otherwise it uses the active session model/provider.

## Interactive Controls

### `/config memory`

Use `/config memory` as the memory-focused control surface.

- In inline UI, it shows loaded `AGENTS.md` sources, matched rules, memory status, file paths, and quick actions.
- Quick actions include toggling memory, toggling auto-write, toggling lightweight-memory routing, picking the memory
  triage model, scaffolding memory files, running one-time legacy cleanup, rebuilding the summary, opening the memory
  directory, and jumping to `/config memory`.
- In non-inline UI, `/config memory` prints status plus exact follow-up commands such as `/config memory` and the memory
  file paths.
- `/config memory` also shows whether cleanup is required because legacy raw prompts or serialized tool payloads were
  found in the memory store.

### Natural-language memory prompts

VT Code also detects explicit memory-management prompts before they go to the model.

- Prompts like `remember that I prefer pnpm`, `save to memory: use cargo nextest`, and `forget my pnpm preference` open
  a human-in-the-loop confirmation dialog in inline UI.
- VT Code sends the raw request through the memory planner first, then shows the normalized fact or exact deletion
  candidates before applying the change.
- Deictic saves such as `remember it`, `remember this`, and `remember that` resolve only against the immediately
  preceding non-empty assistant answer. Tool output and earlier conversation entries are not used as the reference.
- The referenced answer is treated as reference material only because the current request explicitly approves saving it;
  the structured planner and inline confirmation remain required before any write.
- Personal identity details, including names and aliases, are stored in the existing `preferences.md` topic rather than
  the repository-facts topic.
- If the request is underspecified, such as `save to memory and remember my name`, VT Code asks for the missing detail
  before it writes anything.
- If a save succeeds but the read-back verification cannot find the normalized fact, VT Code reports the verification
  failure and directs you to `/config memory` to inspect the store.
- Prompts like `show memory` or `what do you remember` route to the existing `/config memory` surface instead of sending
  the request to the model.
- If cleanup is required, VT Code asks you to run the one-time cleanup before any memory mutation.
- If inline selection UI is unavailable, VT Code does not mutate memory and points you back to `/config memory`.

<!-- Historical or repeated section title retained for existing anchors. -->
<!-- markdownlint-disable-next-line MD024 -->
### `/config memory`

Use `/config memory` to jump directly to the persistent-memory settings section. The same section is also reachable
through `/config agent.persistent_memory`.

The focused controls cover:

- `agent.persistent_memory.enabled`
- `agent.persistent_memory.auto_write`
- `agent.persistent_memory.startup_line_limit`
- `agent.persistent_memory.startup_byte_limit`
- `agent.persistent_memory.directory_override`
- `agent.instruction_import_max_depth`
- `agent.instruction_excludes`
- `agent.small_model.use_for_memory`

`directory_override` is intentionally restricted to system, user, or project-profile config layers. A workspace-root
`vtcode.toml` cannot redirect persistent memory storage.

For current-value fields such as startup line limits, byte limits, and import depth, pressing `Enter` on an empty inline
input keeps the displayed value.

## `/init` and Scaffolding

`/init` still generates the root `AGENTS.md`, and now also scaffolds:

- `.vtcode/README.md`
- the per-repository memory directory layout

Use `/init --force` when you want to regenerate the root guidance file and refresh workspace scaffolding in one pass.

## Recommended Practices

- Keep authored guidance concise, reviewable, and intentionally human-written.
- Use `.vtcode/rules/` for modular project rules instead of growing one large `AGENTS.md`.
- Reserve persistent memory for reusable learned facts, not policy or mandatory coding standards.
- Prefer `/config memory` for day-to-day memory inspection and quick actions.
- Prefer `/config memory` when you need to tune limits, excludes, or the storage location.
