# Subagents

VT Code subagents let the main session delegate bounded work into child threads with their own context, tool restrictions, runtime config overlay, and archived history. They are the closest VT Code feature to Claude Code subagents and Codex subagent workflows, but VT Code keeps its own CLI and runtime semantics.

Subagents help you:

- preserve main-thread context by moving noisy exploration or verification into child threads
- constrain tools for focused reviewers, planners, or debuggers
- reuse project or user agent definitions across repositories
- preload skills, MCP servers, memory, and hooks for specialized work
- switch between the main thread and delegated child threads with `/agent`

VT Code ships with built-in subagents and can also load custom agents from `.vtcode`, `.claude`, `.codex`, and enabled plugins.

For new `.vtcode/agents/*.md` files, use VT Code tool ids in frontmatter. Claude-style names such as `Read`, `Grep`, `Glob`, `Edit`, `Write`, and `Bash` are compatibility imports for `.claude` files, not the recommended VT Code-native format.

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
- child threads cannot spawn more subagents in the current VT Code build

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
permissionMode: plan
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

6. Use `/agent` to switch between the main thread and delegated child threads. Use `/agents threads` to inspect child runs and open completed transcripts.

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
permissionMode = "plan"
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

## Supported Fields

Only `name` and `description` are required.

| Field | Purpose | Notes |
| --- | --- | --- |
| `name` | unique agent identifier | use lowercase letters, digits, and hyphens |
| `description` | delegation hint for VT Code and the model | include phrases like "use proactively" when you want read-only delegation to be attractive |
| `tools` | allowlist of tool names | use VT Code tool ids from `vtcode schema tools`, such as `read_file`, `list_files`, `unified_search`, `unified_exec`, `edit_file`, `write_file`, `apply_patch`, or `unified_file` |
| `disallowedTools` | denylist removed from inherited or allowed tools | use the same VT Code tool ids as `tools`; applied before the runtime child-tool filter |
| `model` | model override | defaults to `inherit`; also accepts `small`, `haiku`, `sonnet`, `opus`, or a full model id |
| `color` | TUI badge color for active subagent indicators | optional; accepts simple color names such as `blue`, hex like `#4f8fd8`, or Git-style fg/bg strings such as `white #4f8fd8` |
| `reasoning_effort` | per-agent reasoning override | `effort` and `model_reasoning_effort` are also accepted |
| `permissionMode` | child permission mode | `default`, `acceptEdits`, `dontAsk`, `bypassPermissions`, `plan`, `auto` |
| `skills` | skills to preload into the child context | uses the same skill loader as the main session |
| `mcpServers` | named or inline MCP servers | inline servers are scoped to the child config overlay |
| `hooks` | child-local lifecycle hooks | use this for `PreToolUse`, `PostToolUse`, and `Stop` behavior inside the child thread |
| `background` | default background execution for this agent | per-call `spawn_agent.background` can also request background mode |
| `maxTurns` | per-agent turn ceiling | can also be overridden per call |
| `nickname_candidates` | preferred thread labels | shown in `/agent` and `/agents` thread lists |
| `initialPrompt` | default task prompt when the spawn request omits one | useful for compatibility imports |
| `memory` | persistent memory scope | `user`, `project`, or `local` |
| `isolation` | compatibility field for future isolation modes | `worktree` is parsed but currently rejected at runtime in VT Code |

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

- Use `tools` for an allowlist and `disallowedTools` for a denylist.
- Prefer the exact VT Code tool ids returned by `vtcode schema tools`.
- For new VT Code-native agent files, do not use Claude-style tool names such as `Read`, `Grep`, `Glob`, `Write`, `Edit`, or `Bash`.
- Use the narrowest VT Code tool set that fits the job instead of granting broad umbrella access by default.
- VT Code always strips child access to `spawn_agent`, `send_input`, `wait_agent`, `resume_agent`, and `close_agent`.
- If the agent is effectively read-only, VT Code strips mutating tools at runtime even if they are listed.

### Permission Modes

Subagents inherit the parent approval context and can only stay at or below the parent's permission strength.

- If the parent is in `auto` or `bypassPermissions`, the parent mode wins.
- Otherwise the child can request a stricter mode such as `plan` or `dontAsk`.

### MCP Servers

Use `mcpServers` to attach named or inline MCP providers to a subagent:

```yaml
---
name: browser-tester
description: Browser-based verification agent for UI regressions.
mcpServers:
  - github
  - playwright:
      type: stdio
      command: npx
      args: ["-y", "@playwright/mcp@latest"]
---
```

Plugin-provided agents are safer by default: VT Code ignores `hooks`, `mcpServers`, and `permissionMode` when those fields come from a plugin agent file.

### Persistent Memory

When `memory` is enabled, VT Code injects a memory appendix into the child prompt and uses these directories:

| Scope | Directory |
| --- | --- |
| `project` | `.vtcode/agent-memory/<agent-name>/` |
| `local` | `.vtcode/agent-memory-local/<agent-name>/` |
| `user` | `~/.vtcode/agent-memory/<agent-name>/` |

VT Code also includes an excerpt of `MEMORY.md` when it exists. Memory does not automatically add file-write tools, so if the agent should update memory files you must let it inherit write-capable file tools or explicitly allow them.

### Hooks

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

VT Code does not currently expose Claude-style session-wide `--agent <name>` or `--agents <json>` flows. In VT Code, `--agent` is already used for model override. Use agent files, natural-language delegation, explicit mentions, or `/agents` instead.

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
- inside the drawer, `Enter` inspects the selected item, `Ctrl+O` opens its transcript or archive, `Ctrl+K` requests a stop, and `Ctrl+X` force-cancels a background subprocess

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

`wait_agent` is a foreground wait on delegated child threads: it blocks the current turn until a child finishes or the timeout expires. Managed background subprocesses are different; keep using `Ctrl+B`, `/subprocesses`, the sidebar, or `Alt+S` for those instead of `wait_agent`.

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
