---
name: ast-grep
description: Use when the task involves ast-grep project workflows such as `sg scan`, `sg test`, `sg new`, `sgconfig.yml`, `customLanguages`, `languageGlobs`, `languageInjections`, `transform`, `rewriters`, or ast-grep rule authoring and debugging.
metadata:
    short-description: Ast-grep project workflows
---

# Ast-Grep

Use this skill for ast-grep project setup, rule authoring, rule debugging, and CLI workflows that go beyond a single structural query.

## Routing

- Prefer `unified_search` with `action="structural"` and `workflow="scan"` for read-only project scans.
- Prefer `unified_search` with `action="structural"` and `workflow="test"` for read-only ast-grep rule tests.
- Stay on the public structural surface first when the task is only running project checks and reporting findings.
- Use `unified_exec` only when the public structural surface cannot express the requested ast-grep flow.

## Use `unified_exec` For

- `sg new`
- Rewrite or apply flows
- Interactive ast-grep flags
- `transform` or `rewriters`
- Non-trivial `sgconfig.yml` authoring or debugging
- Rule authoring tasks that need direct ast-grep CLI iteration beyond public scan/test

## Read More

- Read [references/project-workflows.md](references/project-workflows.md) when you need the boundary between public scan/test support and skill-driven CLI work.
