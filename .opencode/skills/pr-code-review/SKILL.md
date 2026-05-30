---
name: pr-code-review
description: "Spin up a PR code review sub-agent. Ranks bugs critical/high/medium/low, eliminates false positives, writes consolidated report, applies KISS/DRY fixes (main logic only, skip tests), re-reviews after every change, loops until done. Use when the user asks to review a PR, triage bugs, or run a code review fix loop."
metadata:
  slash_alias: "/pr-code-review"
  usage: "/pr-code-review [--branch <ref>|--last-diff|--file <path>|files...]"
  category: "review"
  agent: "pr-code-review"
---

# PR Code Review

Spin up the `pr-code-review` sub-agent to perform a full PR bug review and fix loop.

## Instructions

1. Parse the user input as the argument string that follows `/pr-code-review`.
2. Resolve the diff target:
   - No args or `--last-diff`: use `git diff HEAD~1...HEAD` (last commit) or `git diff` (uncommitted).
   - `--branch <ref>`: use `git diff <ref>...HEAD`.
   - `--file <path>` or positional files: review only those files.
3. Launch the `pr-code-review` agent via the Task tool with the resolved diff target.
4. The agent will: scan -> triage -> eliminate false positives -> report -> fix loop -> re-review -> repeat.

## Agent Delegation

Use the Task tool to delegate to the `pr-code-review` agent:

```
Task tool:
  subagent_type: pr-code-review
  prompt: "Review the PR diff for <target>. Rank bugs critical/high/medium/low. Eliminate false positives. Write consolidated report. Apply KISS/DRY fixes to main logic only (skip tests). Re-review after every change. Loop until done."
```

## Trigger Phrases

- "Review this PR"
- "Code review this diff"
- "Find bugs in this PR"
- "Review and fix this branch"
- "Run code review fix loop"
