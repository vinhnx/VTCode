---
name: cmd-command
description: "Run a terminal command (usage: /command <program> [args...])"
disable-model-invocation: true
metadata:
  slash_alias: "/command"
  usage: "/command <program> [args...]"
  category: "tools"
  backend: "traditional_skill"
---

# Run Terminal Command

Interpret the user input as the raw command string that follows `/command`.

- Execute that command in the current workspace.
- Do not rewrite the command unless quoting or escaping is required by the shell.
- Report the important result briefly, including failures.
- If the command produces large output, summarize the relevant lines instead of dumping everything.
- If the command is empty, respond with the expected usage instead of guessing.
