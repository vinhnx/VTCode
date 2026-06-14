---
name: cmd-review
description: "Review the current diff or selected files (usage: /review [--last-diff|--target <expr>|--file <path>|files...] [--style <style>])"
disable-model-invocation: true
metadata:
  slash_alias: "/review"
  usage: "/review [--last-diff|--target <expr>|--file <path>|files...] [--style <style>]"
  category: "tools"
  backend: "traditional_skill"
---

# Review Changes

Interpret the user input as the raw argument string that follows `/review`.

- Support the existing slash-style review inputs such as `--last-diff`, `--target <expr>`, `--file <path>`, positional file paths, and `--style <style>`.
- If the input is empty, review the current diff.
- Focus on bugs, regressions, correctness risks, and missing tests before any summary.
- Use concrete file and line references when you identify a finding.
- Keep findings ordered by severity and keep the high-level summary brief.
