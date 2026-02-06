# Agent Teams (Experimental)

VT Code agent teams let you coordinate multiple subagents as a lightweight “team” in a single session. The MVP implementation is intentionally minimal: teammates run in-process, tasks execute sequentially, and all state lives in memory for the current session only.

## Enablement

Agent teams are disabled by default. Enable them in `vtcode.toml`:

```toml
[agent_teams]
enabled = true
# default_model = ""
```

You must also enable subagents:

```toml
[subagents]
enabled = true
```

Optional override:

```bash
export VTCODE_EXPERIMENTAL_AGENT_TEAMS=1
```

## Commands

- `/team start [name] [count] [subagent_type]`
- `/team add <name> [subagent_type]`
- `/team remove <name>`
- `/team task add <description>`
- `/team assign <task_id> <teammate>`
- `/team tasks`
- `/team teammates`
- `/team model`
- `/team stop`

`/team model` opens the interactive model picker and saves the selection to
`[agent_teams] default_model` in `vtcode.toml`.

## How It Works (MVP)

- **In-process only**: no split panes, no separate windows.
- **Sequential execution**: one teammate task at a time.
- **Lazy spawn**: teammate subagent sessions start on first assignment.
- **Session scope**: no persistence or resumption across sessions.
- **Lead-only messaging**: you interact through the lead session only.
- **Task summaries**: `/team tasks` shows a summary for completed and failed tasks.

## When to Use

- Parallel exploration with small, independent tasks.
- Delegating focused research or review to separate subagents.

If you need richer orchestration, use VT Code subagents directly or run multiple sessions manually.
