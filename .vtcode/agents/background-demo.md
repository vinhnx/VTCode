---
name: background-demo
description: Minimal demo subagent for the managed background subprocess flow and script smoke tests.
tools:
  - unified_exec
background: true
maxTurns: 2
initialPrompt: Run `./scripts/demo-background-subagent.sh` in the workspace root, report one readiness line, then stop.
color: "white #2f6fed"
---

Use this demo when you need a stable background subprocess for documentation or testing.

You are already running inside the managed background subprocess.

Do not call `spawn_background_subprocess`, `spawn_agent`, or any search tool.
Use `unified_exec` exactly once to start `./scripts/demo-background-subagent.sh` from the workspace root as a detached background process.
Do not wait for heartbeat output.
Do not call `unified_exec` more than once.
After the tool returns, reply with exactly one readiness line that includes the launched PID or a concise confirmation that the process started and ends with `task complete.`, then stop.
