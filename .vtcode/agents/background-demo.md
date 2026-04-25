---
name: background-demo
description: Minimal demo subagent for the managed background subprocess flow and script smoke tests.
tools:
  - unified_exec
  - unified_search
background: true
maxTurns: 2
initialPrompt: Run `./scripts/demo-background-subagent.sh` in the workspace root, report one readiness line, then stop.
color: "white #2f6fed"
---

Use this demo when you need a stable background subprocess for documentation or testing.

Use `unified_exec` exactly once to start `./scripts/demo-background-subagent.sh` from the workspace root as a detached background process.
Return a single readiness line that includes the launched PID or a concise confirmation that the process started.
Do not wait for heartbeat output and do not make extra tool calls after reporting readiness.
