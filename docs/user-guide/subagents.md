# Subagents

VT Code subagents let the main session delegate bounded work into child threads with their own context, tool restrictions, runtime config overlay, and archived history. They are the closest VT Code feature to Claude Code subagents and Codex subagent workflows, but VT Code keeps its own CLI and runtime semantics.

Subagents help you:

- preserve main-thread context by moving noisy exploration or verification into child threads
- constrain tools for focused reviewers, planners, or debuggers
- reuse project or user agent definitions across repositories
- preload skills, MCP servers, memory, and hooks for specialized work
- inspect delegated child threads with `/agent`
- switch the main session's active primary agent with `Tab`

VT Code ships with built-in primary agents and subagents, and can also load custom agents from `.vtcode`, `.claude`, `.codex`, and enabled plugins.

The same agent specification format can describe delegated child agents and primary agents. A subagent is delegated child work with its own thread. A primary agent controls the main session and changes the request-time instructions, tools, permission notes, model, and reasoning effort that VT Code applies for subsequent turns.

For new `.vtcode/agents/*.md` files, use VT Code tool ids in frontmatter. Claude-style names such as `Read`, `Grep`, `Glob`, `Edit`, `Write`, and `Bash` are compatibility imports for `.claude` files, not the recommended VT Code-native format.

## Built-in primary agents

| Agent | Default model | Mutates files? | Purpose |
| --- | --- | --- | --- |
| `build` | `inherit` | yes | Implementation agent for normal build and repair work |
| `auto` | `inherit` | yes, classifier-reviewed | Build-oriented agent that routes configured tools through `permissions.auto` |
| `plan` | `inherit` | no | Planning workflow agent for repository exploration and proposal drafting |
| `duck` | `inherit` | no | Discussion-first agent for scope, constraints, and trade-offs |

Custom project or user specs with the same name override these built-ins using the normal discovery precedence. Use `mode: primary` for main-session agents, `mode: subagent` for delegated-only definitions, and `mode: all` for definitions that should support both.

Primary agents replace old behaviour labels: choose `duck` for discussion, `plan` for planning workflow, `build` for implementation, and `auto` for classifier-reviewed build work.

## Built-in subagents

| Agent | Default model | Mutates files? | Purpose |
| --- | --- | --- | --- |
| `default` | `inherit` | yes | General delegated work with inherited tools and config |
| `explorer` | `small` | no | Read-only code search, file discovery, and repository understanding |
| `plan` | `inherit` | no | Read-only planning research and constraint gathering |
| `worker` | `inherit` | yes | Bounded implementation and multi-step execution |

Notes:

- `explorer` also matches `explore`
- `worker` also matches `general` and `general-purpose`
- child threads do not spawn more subagents

Completed child threads are expected to return a fixed Markdown handoff that VT Code can merge back into the parent session memory:

```markdown
## Summary
- ...

## Facts
- ...

## Touched Files
- ...

## Verification
- ...

## Open Questions
- ...
```

Use `- None` for empty sections. If a child reply does not follow this contract, VT Code preserves the raw summary as delegation notes but skips structured merge.

## Quickstart

1. Run `/agents`.
2. Choose `Create project agent` or `Create user agent`.
3. VT Code writes a scaffold to `.vtcode/agents/<name>.md` or `~/.vtcode/agents/<name>.md`.
4. Edit the scaffold. A minimal read-only reviewer looks like this:

```markdown
---
name: code-reviewer
description: Read-only reviewer for correctness, regressions, and test gaps. Use proactively after code changes.
tools: [read_file, list_files, unified_search]
permissions:
  default: deny
  allow: [read_file, list_files, unified_search]
  ask: []
  auto: []
  deny: [unified_exec, edit_file, write_file, apply_patch]
model: inherit
color: blue
reasoning_effort: medium
maxTurns: 6
memory: project
---

Review recent code changes. Focus on correctness, risky assumptions, missing tests,
and behavior regressions. Return findings in priority order with file references.
```

5. Invoke it with natural language or an explicit mention:

```text
Use the code-reviewer agent on the auth changes
@agent-code-reviewer inspect the auth changes
Spawn a code-reviewer subagent and summarize only the important findings
```

6. Use `/agent` or `/agents threads` to inspect delegated child runs and open completed transcripts. Use `Tab` on an empty idle composer when you want to switch the main session to another primary agent.

## Discovery And Precedence

VT Code loads subagents from highest to lowest precedence:

| Priority | Location | Format | Scope |
| --- | --- | --- | --- |
| 1 | `.vtcode/agents/*.md` | VT Code markdown | current project |
| 2 | `.claude/agents/*.md` | Claude-style markdown | current project |
| 3 | `.codex/agents/*.toml` | Codex TOML | current project |
| 4 | `~/.vtcode/agents/*.md` | VT Code markdown | all projects |
| 5 | `~/.claude/agents/*.md` | Claude-style markdown | all projects |
| 6 | `~/.codex/agents/*.toml` | Codex TOML | all projects |
| 7 | plugin agents | plugin-provided | where the plugin is enabled |
| 8 | built-ins | internal | always available |

If two definitions share the same `name`, the higher-priority source wins. At the same priority, VT Code-native `.vtcode` definitions win over imported `.claude` or `.codex` definitions.

## File Formats

### Markdown Agents

Use Markdown with YAML frontmatter in `.vtcode/agents/` or `.claude/agents/`:

```markdown
---
name: debugger
description: Debugging specialist for failing tests, crashes, and broken behavior. Use proactively when something fails.
model: inherit
color: "#4f8fd8"
maxTurns: 8
tools:
  - unified_search
  - read_file
  - list_files
  - unified_exec
  - unified_file
  - apply_patch
---

Identify the smallest reproducible failure, fix the root cause, verify the result,
and return a concise summary with any remaining risks.
```

### Codex TOML Agents

VT Code also loads Codex-style TOML agents from `.codex/agents/`:

```toml
name = "reviewer"
description = "Read-only code reviewer for recent changes."
developer_instructions = """
Review modified code for correctness, maintainability, and missing tests.
Return findings in priority order with file references.
"""
model = "gpt-5.4-mini"
model_reasoning_effort = "low"
maxTurns = 6
permissions = { default = "deny", allow = ["read_file", "list_files", "unified_search"], ask = [], auto = [], deny = ["unified_exec", "edit_file", "write_file", "apply_patch"] }
```

VT Code reads `developer_instructions` and also accepts `instructions` for compatibility.

Imported Claude-style agent files may still use aliases such as `Read`, `Write`, `Edit`, `Grep`, `Glob`, and `Bash`. VT Code normalizes those aliases to VT Code tool names when it loads the file. For new VT Code-native agent files, write the VT Code tool names directly.

### VT Code Tool Names

For `.vtcode/agents/*.md`, prefer the exact VT Code tool ids returned by `vtcode schema tools`.

```bash
vtcode schema tools
vtcode schema tools --name unified_search --name unified_exec --name unified_file
```

Common starting points:

| Use case | Recommended VT Code tool ids |
| --- | --- |
| Read-only review | `read_file`, `list_files`, `unified_search` |
| Read plus shell execution | `read_file`, `list_files`, `unified_search`, `unified_exec` |
| Write-capable editor | `read_file`, `list_files`, `unified_search`, `edit_file`, `write_file`, `apply_patch` |
| Umbrella file access | `unified_file` |
| Explicit user clarification | `request_user_input` |

Compatibility alias mapping for imported `.claude` agents:

| Claude-style name | VT Code tool id |
| --- | --- |
| `Read` | `read_file` |
| `Grep` | `unified_search` |
| `Glob` | `list_files` |
| `Bash` | `unified_exec` |
| `Edit` | `edit_file` |
| `Write` | `write_file` |
| `Agent` or `Task` | `spawn_agent` |
| Managed background helper | `spawn_background_subprocess` |

## Supported Fields

Only `name` and `description` are required.

| Field | Purpose | Notes |
| --- | --- | --- |
| `name` | unique agent identifier | use lowercase letters, digits, and hyphens |
| `description` | delegation hint for VT Code and the model | include phrases like "use proactively" when you want read-only delegation to be attractive |
| `mode` | agent availability | `primary`, `subagent`, or `all`; omitted mode defaults to `subagent` |
| `tools` | allowlist of tool names | use VT Code tool ids from `vtcode schema tools`, such as `read_file`, `list_files`, `unified_search`, `unified_exec`, `edit_file`, `write_file`, `apply_patch`, or `unified_file` |
| `disallowedTools` / `disallowed_tools` | denylist removed from inherited or allowed tools | use the same VT Code tool ids as `tools`; applied before runtime filtering |
| `model` | model override | defaults to `inherit`; also accepts `small`, `haiku`, `sonnet`, `opus`, or a full model id |
| `color` | TUI color metadata | optional; accepts simple color names such as `blue`, hex like `#4f8fd8`, or Git-style fg/bg strings such as `white #4f8fd8` |
| `aliases` | alternate names for lookup | exact `name` matches still win over aliases |
| `reasoning_effort` | per-agent reasoning override | `effort` and `model_reasoning_effort` are also accepted |
| `permissions` | granular permission policy | required; set `default` plus optional `allow`, `ask`, `auto`, and `deny` tool lists |
| `skills` | skills to preload | primary agents replace the active skill set while selected; subagents preload skills into the child context |
| `mcpServers` / `mcp_servers` | MCP servers | primary agents can add inline providers while active; subagents can use named or inline providers in the child config overlay |
| `hooks` | lifecycle hooks | primary agents support main-session events; subagents support child-thread events |
| `memory` | persistent memory scope | `user`, `project`, or `local` |
| `background` | marks an agent as eligible for the managed background subprocess flow | launch these agents with `spawn_background_subprocess`; `spawn_agent` stays foreground-only |
| `maxTurns` | per-agent turn ceiling | can also be overridden per call |
| `nickname_candidates` | preferred thread labels | shown in `/agent` and `/agents` thread lists |
| `initialPrompt` | default task prompt when the spawn request omits one | useful for compatibility imports |
| `isolation` | compatibility field for future isolation modes | `worktree` is parsed but currently rejected at runtime in VT Code |

### Field Availability

| Field | Primary agents | Subagents |
| --- | --- | --- |
| `name` | yes | yes |
| `description` | yes | yes |
| Markdown body / instructions | yes | yes |
| `mode` | yes | yes |
| `tools` | yes | yes |
| `disallowedTools` / `disallowed_tools` | yes | yes |
| `permissions` | yes | yes |
| `model` | yes | yes |
| `reasoning_effort` | yes | yes |
| `color` | yes | yes |
| `aliases` | yes | yes |
| `skills` | yes | yes |
| `mcpServers` / `mcp_servers` | yes | yes |
| `hooks` | yes | yes |
| `memory` | yes | yes |
| `background` | no | yes |
| `maxTurns` | no | yes |
| `nickname_candidates` | no | yes |
| `initialPrompt` | no | yes |
| `isolation` | no | yes |

Subagent-only fields describe child-thread launch behaviour. `background` selects the managed background subprocess flow, `maxTurns` limits a delegated run, `nickname_candidates` label child threads, `initialPrompt` fills in a missing delegated task, and `isolation` is reserved for delegated isolation modes. Primary agents run in the main session, so they do not need child launch defaults, child labels, or a separate background or isolation boundary.

## Model Resolution

VT Code resolves a subagent model in this order:

1. `spawn_agent.model` from the current delegated call
2. the agent file's `model`
3. the parent session model

Special model values:

- omitted `spawn_agent.model` falls back to the agent file's `model`, then the parent model
- `inherit` keeps the parent model even when the agent file sets a different model
- `small` uses the configured small-model tier or a lightweight sibling of the parent model
- `haiku`, `sonnet`, and `opus` map to provider-appropriate siblings of the active provider
- VT Code only honors `spawn_agent.model` when the current user turn explicitly asks for that model; opportunistic overrides from the model are ignored
- if `spawn_agent.model` cannot be resolved, VT Code warns, ignores that override, and falls back to the agent file's `model` or the parent session model

## Active Subagent Badges

When one or more child threads are active, VT Code shows each active agent name in the TUI header as a full-background badge.

- Set `color` in the agent definition to control that badge color.
- If the color string includes only one color, VT Code uses it as the badge background and chooses a readable foreground automatically.
- If the color string includes both foreground and background, VT Code uses both directly.
- If `color` is omitted, VT Code falls back to a default badge color.

## Control Capabilities

### Tool Allowlists And Denylists

- Use `tools` for an allowlist and `disallowedTools` or `disallowed_tools` for a denylist.
- Prefer the exact VT Code tool ids returned by `vtcode schema tools`.
- For new VT Code-native agent files, do not use Claude-style tool names such as `Read`, `Grep`, `Glob`, `Write`, `Edit`, or `Bash`.
- Use the narrowest VT Code tool set that fits the job instead of granting broad umbrella access by default.
- VT Code always strips child access to `spawn_agent`, `send_input`, `wait_agent`, `resume_agent`, and `close_agent`.
- If the agent is effectively read-only, VT Code strips mutating tools at runtime even if they are listed.

### Permissions

Agents use explicit granular permissions. Set `permissions.default` to the baseline decision and use `allow`, `ask`, `auto`, and `deny` lists for tool-specific policy.

Subagents inherit the parent approval context and cannot broaden it. Primary agents apply their granular policy while active without changing the saved startup posture.

### MCP Servers

Use `mcpServers` to attach MCP providers to an agent. Subagents can use named providers from the active configuration or inline providers scoped to the child config overlay. Primary agents can add inline providers while they are active:

```yaml
---
name: browser-reviewer
description: Browser-based primary agent for UI verification.
mode: primary
mcpServers:
  - playwright:
      type: stdio
      command: npx
      args: ["-y", "@playwright/mcp@latest"]
---
```

Plugin-provided agents are safer by default: VT Code ignores `hooks`, `mcpServers`, and permission policy fields when those fields come from a plugin agent file.

### Persistent Memory

When `memory` is enabled, VT Code uses the same directory scheme for primary agents and subagents:

| Scope | Directory |
| --- | --- |
| `project` | `.vtcode/agent-memory/<agent-name>/` |
| `local` | `.vtcode/agent-memory-local/<agent-name>/` |
| `user` | `~/.vtcode/agent-memory/<agent-name>/` |

The directory key is the canonical `name`, not an alias.

For subagents, VT Code creates the memory directory when needed and injects a compact appendix for `MEMORY.md`. Memory does not automatically add file-write tools, so if the subagent should update memory files you must let it inherit write-capable file tools or explicitly allow them.

For primary agents, memory is read-only. VT Code looks for the existing `MEMORY.md`, does not create the directory or file, and appends memory context only when that file is present. Primary-agent memory does not grant extra tools or permissions.

### Hooks

Agent files can include lifecycle hooks.

Primary agents support these main-session events while selected:

- `UserPromptSubmit`
- `PreToolUse`
- `PostToolUse`
- `PermissionRequest`
- `PreCompact`
- `Stop`
- `Notification`

Primary-agent hooks do not run lifecycle events that belong to the whole session or subagent controller: `SessionStart`, `SessionEnd`, `SubagentStart`, `SubagentStop`, `task_completion`, and `task_completed`. Put those hooks in `vtcode.toml`.

Subagent files are best for child-local hooks such as:

- `PreToolUse`
- `PostToolUse`
- `Stop`

Parent-session subagent lifecycle hooks belong in `vtcode.toml`:

```toml
[hooks.lifecycle]
subagent_start = [
  { matcher = "db-agent", hooks = [{ command = "./scripts/setup-db.sh" }] }
]
subagent_stop = [
  { hooks = [{ command = "./scripts/cleanup-db.sh" }] }
]
```

`SubagentStart` and `SubagentStop` match against the agent name and receive payload fields such as `parent_session_id`, `child_thread_id`, `agent_name`, `display_label`, `background`, `status`, `cwd`, and `transcript_path`.

## Runtime Controls

Subagent runtime behavior lives under `[subagents]` in `vtcode.toml`:

```toml
[subagents]
enabled = true
max_concurrent = 4
max_depth = 1
default_timeout_seconds = 300
auto_delegate_read_only = true
```

Key behaviors:

- `max_depth = 1` keeps nested delegation off by default
- `max_concurrent` limits simultaneous child threads
- `auto_delegate_read_only` controls whether VT Code may proactively launch read-only agents without an explicit user delegation request

## Work With Subagents

### Automatic Delegation

VT Code keeps delegation rules simple and deterministic:

- read-only agents may be launched proactively only when `subagents.auto_delegate_read_only = true`
- write-capable agents require an explicit signal in the current user turn

Explicit delegation signals include:

- an `@agent-...` mention
- "use the X agent"
- "spawn"
- "delegate"
- "run in parallel"
- "background subagent" or "background agent"

### Invoke Subagents Explicitly

Natural language usually works:

```text
Use the explorer agent to map the auth module
Delegate this test triage to the debugger agent
Run parallel subagents for API, database, and auth
```

An explicit mention guarantees the selection for that turn:

```text
@agent-explorer inspect the auth module
@agent-github:reviewer check the PR changes
```

VT Code treats a single explicit mention as the selected agent for the turn. If the model later tries to spawn a different agent, the call is rejected instead of silently switching.

`/agent` and `/agents` can inspect agent definitions and delegated child runs. `@agent-name` is only for subagent-capable definitions; primary-only agents cannot be invoked with `@`.

### Use A Primary Agent

Press `Tab` on an empty idle composer to cycle the active primary agent. The cycle includes discovered agent specs marked with `mode: primary` or `mode: all`. Project definitions still take precedence over user definitions, imported definitions, plugin definitions, and built-ins according to the discovery order above.

When you select a primary agent, VT Code keeps you in the main session rather than spawning a child thread. The active primary agent is shown in the header.

Primary agents use the same agent definition format:

```markdown
---
name: reviewer
description: Main-session code reviewer for correctness, regressions, and test gaps.
mode: primary
aliases: [review, critic]
color: cyan
tools: [read_file, list_files, unified_search]
disallowedTools: [unified_exec, unified_file]
permissions:
  default: deny
  allow: [read_file, list_files, unified_search]
  ask: []
  auto: []
  deny: [unified_exec, edit_file, write_file, apply_patch]
model: inherit
reasoning_effort: medium
skills: [code-review]
memory: project
mcpServers:
  - docs-search:
      type: stdio
      command: vtcode-docs-mcp
hooks:
  lifecycle:
    user_prompt_submit:
      - hooks:
          - command: "$VT_PROJECT_DIR/.vtcode/hooks/review-prompt.sh"
---

Review code in the main session. Focus on correctness, risky assumptions,
missing tests, and project conventions. Do not edit files directly.
```

Use `mode: primary` for agents that should control the main session, `mode: subagent` for delegated child agents, and `mode: all` for agents that should be available in both places.

Cycling past the last available primary agent wraps back to the first agent.

`@agent-name` remains delegated-child syntax. It does not select a primary agent.

### Create A Custom Primary Agent

To create a custom primary agent:

1. Create a markdown file in `.vtcode/agents/` (project scope) or `~/.vtcode/agents/` (user scope):

```markdown
---
name: reviewer
description: Code review agent that enforces project conventions.
mode: primary
aliases: [review, critic]
color: cyan
tools: [read_file, list_files, unified_search]
permissions:
  default: deny
  allow: [read_file, list_files, unified_search]
  ask: []
  auto: []
  deny: [unified_exec, edit_file, write_file, apply_patch]
model: inherit
reasoning_effort: medium
skills: [code-review]
memory: project
hooks:
  lifecycle:
    pre_tool_use:
      - matcher: unified_search
        hooks:
          - command: "$VT_PROJECT_DIR/.vtcode/hooks/log-review-search.sh"
---

You are a code review specialist.
Focus on correctness, readability, and adherence to project conventions.
Do not edit files directly; provide actionable feedback instead.
```

2. Set `mode: primary` to make it available as a primary agent (or `mode: all` for both primary and subagent use).
3. Use `tools` to restrict which tools the agent can access.
4. Use `permissions.default` and optional tool lists to define the agent's granular permission policy.
5. Press `Tab` on an empty idle composer to cycle to the new agent.

The agent definition body (below the frontmatter) becomes the agent's runtime instructions when selected.

### Inspect Active Agents In Place

- `/agent` opens the active-agent inspector for delegated child runs in the current session
- `/agents threads` and `/agents active` are aliases for the same in-place inspector flow
- selecting an active child opens a read-only inspector modal; `Esc` closes it and returns to the main VT Code session immediately
- `Ctrl+R` reloads the selected inspector, `Ctrl+K` cancels the selected agent, and completed transcripts can still be opened in your editor

Approval prompts raised by child runs still include `Source: <label>`. Use `/agent` to inspect that delegated run without switching the session itself.

### Background Subagents And Subprocesses

VT Code can also run a background subagent as a managed child `vtcode` subprocess. This path is separate from foreground child threads and is now explicitly opt-in:

- background subprocesses are disabled by default
- no default background agent is preconfigured
- `Ctrl+B` only starts or stops a background subagent after you enable background mode and set `default_agent`
- if background mode is disabled or unconfigured, `Ctrl+B` opens the Local Agents drawer and shows the setup guidance instead of launching anything
- `/subprocesses` opens the Local Agents drawer
- `Alt+S` focuses the same Local Agents drawer quickly from the main session
- when local agents exist, the footer shows a compact badge such as `1 local agent | ↓ explore`
- in wide layouts, the sidebar shows a single `Local Agents` section instead of separate live-agent and subprocess sections
- with an empty composer, `Down` opens the Local Agents drawer; otherwise `Down` keeps its normal history behavior
- inside the drawer, `Enter` inspects the selected item, `Alt+O` opens its transcript or archive, `Ctrl+K` requests a stop, and `Ctrl+X` force-cancels a background subprocess

Background subprocess state is persisted under `.vtcode/state/background_subagents.json`. On restart, VT Code only respawns enabled background agents when both `subagents.background.enabled = true` and `subagents.background.auto_restore = true`.

Add background runtime controls under `[subagents.background]`:

```toml
[subagents.background]
enabled = true
default_agent = "rust-engineer"
refresh_interval_ms = 2000
auto_restore = false
toggle_shortcut = "ctrl+b"
```

Equivalent examples:

```yaml
subagents:
  background:
    enabled: true
    default_agent: rust-engineer
    refresh_interval_ms: 2000
    auto_restore: false
    toggle_shortcut: ctrl+b
```

```json
{
  "subagents": {
    "background": {
      "enabled": true,
      "default_agent": "rust-engineer",
      "refresh_interval_ms": 2000,
      "auto_restore": false,
      "toggle_shortcut": "ctrl+b"
    }
  }
}
```

If you omit `default_agent`, VT Code keeps background mode disabled for launch purposes and treats `Ctrl+B` and `/subprocesses toggle` as setup entrypoints only.

For a minimal demo pair, see [background-subagent-demo.md](../examples/background-subagent-demo.md) and [demo-background-subagent.sh](../../scripts/demo-background-subagent.sh).

### Continue Existing Child Work

Delegated child threads keep their own history. VT Code can continue them with follow-up input instead of starting from scratch. The runtime exposes `send_input`, `resume_agent`, `wait_agent`, and `close_agent` to the model for this purpose.

`wait_agent` is a foreground wait on delegated child threads: it blocks the current turn until a child finishes or the timeout expires. Managed background subprocesses are different; launch them with `spawn_background_subprocess` and manage them with `Ctrl+B`, `/subprocesses`, the sidebar, or `Alt+S` instead of `wait_agent`.

## Choose Between Main Thread And Subagents

Prefer the main thread when:

- the task needs frequent back-and-forth
- several phases share heavy context
- the change is quick and local

Prefer subagents when:

- the work is self-contained
- the task produces noisy logs or exploratory output
- you want a tighter tool or permission boundary
- you want to inspect delegated runs or keep a long-lived helper available without leaving the main session

If you need nested delegation or long-lived parallel teams, VT Code subagents are not the right primitive yet. The current build keeps child depth at one level by default.

## Related Guides

- [Interactive Mode Reference](./interactive-mode.md)
- [Configuration Guide](../config/config.md)
- [Lifecycle Hooks](../guides/lifecycle-hooks.md)
- [Command Reference](./commands.md)
