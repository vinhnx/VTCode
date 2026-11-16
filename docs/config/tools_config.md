# Tools Configuration

This document describes the tools-related configuration in `vtcode.toml`.

- `max_tool_loops` was removed. Turns are no longer capped by a tool-loop count; the runloop converges through the
  behavioral guards (`max_repeated_tool_calls`, navigation/low-signal/read caps, blocked-tool fuse) and the resource
  bounds (`max_tool_wall_clock_secs`, `max_budget_usd`, context compaction). Stale `[tools].max_tool_loops` keys parse
  but do nothing and are pruned on the next config save.

Example:

```toml
[tools]
default_policy = "prompt"
```

## Blocked-call limits and blocked handoffs

`max_consecutive_blocked_tool_calls_per_turn` controls the consecutive blocked-call cap. The runtime also applies a
bounded total fuse: normal mode allows two times the configured cap, Plan Mode allows four times the cap, and recovery
mode retains the tighter configured cap — unless `max_total_blocked_tool_calls_per_turn` sets an explicit total.
Per-tool overrides in `blocked_tool_thresholds` (e.g. `code_search = 6`) replace the consecutive cap for that tool;
`code_search` gets 2x headroom by default. These limits apply consistently to policy/preflight denials and blocked
execution failures; the consecutive streak resets after an allowed call, while the total count remains per turn. One
attempt before the fuse, the runtime emits a warning advisory with the streak/total counters and a per-tool remedy hint.

```toml
[tools]
max_consecutive_blocked_tool_calls_per_turn = 8
max_total_blocked_tool_calls_per_turn = 16

[tools.blocked_tool_thresholds]
exec_command = 3
code_search = 6
```

When the fuse stops a turn, VT Code emits `turn.blocked` (streak/total/caps/last tool) alongside `turn.failed`, forces a
session-history checkpoint, pins referenced spool outputs so resume can still read them, and creates the blocked
handoff. A persisted archive produces a verified `vtcode --resume <archive-id>` command. Disabled history or a failed
checkpoint produces a handoff without a resume command and states why resume is unavailable. Interactive sessions remain
available for the next user input; the TUI shows a `Blocked` header badge, `Blocked • continue to retry…` footer hint,
and a transcript banner with resume guidance.

Runner paths without session-archive support likewise omit the resume command instead of treating a runtime session ID
as an archive identifier.

Tool outputs are rendered with ANSI styles in the chat interface. Tools should return plain text.
