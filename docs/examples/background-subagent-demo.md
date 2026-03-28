---
name: background-demo
description: Minimal demo agent for the managed background subprocess flow.
tools:
  - unified_exec
color: "white #2f6fed"
background: true
maxTurns: 1
---

Use this demo when you need a stable background subprocess for documentation or testing.

This file is a subagent definition example, not a shell command. Use the discoverable workspace copy at `.vtcode/agents/background-demo.md`, or copy this file there before asking VT Code to delegate to `@agent-background-demo`.

Suggested task:

```text
Run `./scripts/demo-background-subagent.sh` in the workspace, report readiness once, then stay idle until VT Code stops the subprocess.
```
