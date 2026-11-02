# Agent Guidance, Memory, and Context

_Part of [VT Code Configuration](./config.md)._

## Instruction Guidance and Persistent Memory

VT Code separates authored guidance from learned persistent memory.

- Authored guidance automatically includes the canonical user config `AGENTS.md` (and legacy `~/.vtcode/AGENTS.md`),
  project `AGENTS.md`, project `.vtcode/rules/`, and any `agent.instruction_files` entries you configure. The full
  content of these files is inlined into the prompt up to `agent.instruction_max_bytes` (default 16384); files that
  exceed the budget are truncated with a notice in the prompt.
- Persistent memory is a per-repository memory store summarized into a compact startup section after authored guidance.

### Instruction discovery controls

Use these fields to control how VT Code discovers and expands guidance files:

```toml
[agent]
instruction_files = ["docs/runbooks/**/*.md"]
instruction_excludes = ["**/other-team/.vtcode/rules/**"]
instruction_import_max_depth = 5
```

- `instruction_files` adds explicit files or globs to the authored-guidance bundle.
- `instruction_excludes` removes matching `AGENTS.md` or `.vtcode/rules/` files from discovery.
- `instruction_import_max_depth` limits recursive `@path` imports inside guidance files.

Workspace rules live under project `.vtcode/rules/`. Rules without frontmatter are always loaded. Rules with YAML
`paths` frontmatter are loaded only when the current instruction context matches those paths.

### Persistent memory controls

Persistent memory uses `memory_summary.md` as the source for a compact startup summary and stores the durable registry
under the repository memory directory.

By default, that directory is the user state directory's `projects/<project>/memory/` path. VT Code also migrates older
per-repository memory directories from the legacy `VTCODE_HOME` root into the canonical state directory when it resolves
repository memory.

```toml
[agent.persistent_memory]
enabled = false
auto_write = true
startup_line_limit = 200
startup_byte_limit = 25600

[agent.small_model]
use_for_memory = true
```

- `agent.persistent_memory.enabled` turns per-repository persistent memory on or off. It defaults to `false`.
- `agent.persistent_memory.auto_write` controls whether VT Code stages and consolidates rollout summaries at session
  finalization.
- `startup_line_limit` and `startup_byte_limit` cap the scan VT Code uses to build the compact startup summary from
  `memory_summary.md`.
- `agent.small_model.use_for_memory` enables lightweight-model routing for memory planning, classification, cleanup, and
  summary refresh.

> Note: `[agent.small_model]` is not exposed through the `/model` picker or the `/settings` model-config view — those
> surfaces only edit the main model (`agent.provider` + `agent.default_model`). The lightweight route is auto-selected
> from the main model's provider, or you can set `[agent.small_model]` directly in `vtcode.toml`.

Memory mutation is LLM-assisted only:

- natural-language `remember` / `forget` requests require a valid structured planner response
- session-finalization memory writes use the same LLM-assisted normalization path
- VT Code blocks the mutation instead of falling back to a plain or heuristic-only write when the memory LLM route is
  unavailable

`agent.persistent_memory.directory_override` is supported, but it may only be set from system, user, or project-profile
config layers. A workspace-root `vtcode.toml` cannot redirect memory storage.

### Interactive controls

You can manage this feature without editing TOML directly:

- `/memory` shows loaded `AGENTS.md` sources, matched rules, memory files, pending rollout summaries, and quick actions.
- `/memory` also reports whether one-time legacy cleanup is required and can run that cleanup explicitly.
- `/config memory` jumps directly to the `agent.persistent_memory` settings section.
- `/config agent.persistent_memory` reaches the same section with the full path.

### OpenAI hosted shell skills

For native OpenAI Responses models, VT Code can replace the local `shell` tool with OpenAI's hosted shell environment
and mount hosted skills into that environment. This path is separate from VT Code's local `SKILL.md` discovery system:
VT Code does not upload or manage hosted skills for you in this workflow.

Use a pre-registered hosted skill by ID:

```toml
[provider.openai.hosted_shell]
enabled = true
environment = "container_auto"
file_ids = ["file_123"]

[[provider.openai.hosted_shell.skills]]
type = "skill_reference"
skill_id = "skill_123"
version = 2
```

Or mount an inline zip bundle directly:

```toml
[provider.openai.hosted_shell]
enabled = true
environment = "container_auto"

[[provider.openai.hosted_shell.skills]]
type = "inline"
bundle_b64 = "UEsFBgAAAAAAAA=="
sha256 = "deadbeef"
```

To allow outbound access for trusted domains in the hosted container, configure a request-scoped allowlist and optional
domain secrets:

```toml
[provider.openai.hosted_shell]
enabled = true
environment = "container_auto"

[provider.openai.hosted_shell.network_policy]
type = "allowlist"
allowed_domains = ["httpbin.org"]

[[provider.openai.hosted_shell.network_policy.domain_secrets]]
domain = "httpbin.org"
name = "API_KEY"
value = "debug-secret-123"
```

Notes:

- `provider.openai.hosted_shell` is only used for OpenAI Responses-capable models on the native OpenAI endpoint.
- `environment = "container_reference"` reuses an existing OpenAI container and ignores `file_ids` and `skills`.
- `provider.openai.hosted_shell.network_policy` currently applies only to `container_auto`.
- `type = "allowlist"` requires at least one `allowed_domains` entry. Each `domain_secrets[*].domain` must also appear
  in `allowed_domains`.
- `version` may be omitted for the default `"latest"` behavior, or set to a pinned integer/string version when your
  hosted skill deployment requires it.

## Context compaction and session history

VT Code has two compaction paths:

- provider-native compaction for providers that support Responses/API-managed compaction
- local fallback compaction for other providers

Local fallback compaction preserves a continuity tail of approximately 20,000 estimated tokens. The tail is made of
complete user/assistant/tool protocol groups; an incomplete trailing tool call is dropped. VT Code summarizes only the
older history prefix and rebuilds the preserved history as:

1. one structured summary message
2. retained recent real user messages and the continuity tail
3. the session memory envelope

Summarized session forks reuse that same handoff shape when you choose a summarized fork from `/fork` or pass
`--summarize` on a forked CLI flow.

### Relevant settings

```toml
[agent.harness]
auto_compaction_enabled = true
auto_compaction_threshold_tokens = 120000

[context]
# Optional session safety ceiling; 0 follows the resolved model/provider capacity.
max_context_tokens = 0

[context.dynamic]
enabled = true
persist_history = true
retained_user_messages = 4
```

Notes:

- `agent.harness.auto_compaction_enabled` enables automatic compaction when prompt-side token pressure crosses the
  configured threshold.
- Disabling `agent.harness.auto_compaction_enabled` skips normal threshold-triggered compaction but does not disable the
  single bounded post-tool recovery compaction used to recover from a provider failure after tool output.
- `agent.harness.auto_compaction_threshold_tokens` applies to both provider-native compaction and VT Code's local
  fallback compaction. It remains authoritative when set, but never exceeds the provider's hard context capacity.
- When the harness threshold is unset, VT Code derives the effective prompt budget from the resolved model capacity, the
  provider route, and a positive `context.max_context_tokens` ceiling. It reserves the next response before deciding
  whether the request fits. The default trigger is 75% of that prompt budget (`DEFAULT_COMPACTION_TRIGGER_RATIO`), so
  long runs compact before the expensive near-full zone; an explicit `auto_compaction_threshold_tokens` still wins,
  capped at the budget.
- `context.max_context_tokens = 0` preserves provider-only threshold resolution for compatibility; a known provider
  capacity is still a hard upper bound.
- `context.dynamic.persist_history = true` lets VT Code persist compaction artifacts and the session memory envelope so
  later resumes and summarized forks can reuse that context.
- `context.dynamic.retained_user_messages` controls how many recent real user messages VT Code preserves verbatim on the
  local fallback compaction path and in summarized forks. The default is `4`.
- The session memory envelope is VT Code's durable working-memory artifact. It is refreshed at turn boundaries and after
  completed child-agent results, then persisted beside history artifacts as `.memory.json`.
- A soft boundary may mark compaction for the next outer turn boundary; the effective prompt threshold still reserves
  output before the next model request. Compaction does not issue a hidden summary request from inside an active tool
  loop.
- Steering follow-ups are stored in schema-version 3 envelopes as UUID-tagged intents: at most 16 pending intents and
  the most recent 64 applied IDs are retained for restart recovery.

## Participant system

### participants

Controls the behavior of the participant system that provides context augmentation:

```toml
[participants]
# Enable participant system for @mention support
enabled = true

# Default participants to always include
default_participants = ["@workspace", "@code"]

# Timeout for participant context resolution (in seconds)
timeout = 15

# Whether to cache participant context between messages
cache_context = true

# Maximum size of context that each participant can provide
max_context_size = 524288  # 512KB
```

### participant.settings

Individual settings for different participants:

```toml
[participants.workspace]
# Include file statistics in workspace context
include_file_stats = true

# Include git status in workspace context
include_git_status = true

# Maximum number of files to list
max_files_to_list = 100

[participants.code]
# Include syntax highlighting information
include_syntax_info = true

# Maximum file size to send for code context (in bytes)
max_file_size = 262144  # 256KB

[participants.terminal]
# Include recent terminal commands
include_recent_commands = true

# Number of recent commands to include
recent_commands_limit = 10

[participants.git]
# Include git repository information
include_repo_info = true

# Include git diff information
include_diff = false
```
