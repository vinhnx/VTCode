---
name: cmd-analyze
description: "Perform comprehensive codebase analysis and generate reports (usage: /analyze [full|security|performance])"
disable-model-invocation: true
metadata:
  slash_alias: "/analyze"
  usage: "/analyze [full|security|performance]"
  category: "tools"
  backend: "traditional_skill"
---

# Analyze Workspace

Interpret the user input as the raw argument string that follows `/analyze`.

- If the input is empty, perform a full workspace analysis.
- If the input is `full`, `security`, or `performance`, focus on that scope.
- Base the analysis on the current workspace contents, not generic advice.
- Call out concrete findings, risks, and prioritized next actions.
- Keep the response concise and actionable.
