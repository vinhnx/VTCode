---
name: ast-grep
description: Structural search and ast-grep workflow for `sg`. Use when a task depends on syntax-aware matching, `--debug-query` AST inspection, YAML rule authoring, `sg scan`/`sg test`, `sgconfig.yml` project setup, custom or injected languages, or rewrite transformations, and plain-text grep is too imprecise.
when-to-use: Use for ast-grep, sg, structural search, AST-aware matching, selector or strictness tuning, debug-query inspection, reusable rule authoring, `sg scan`, `sg test`, `sg new`, `sgconfig.yml`, custom languages, language injections, or transform/rewriter workflows when syntax matters more than raw text.
when-not-to-use: Do not use for simple keyword search, filenames, logs, stack traces, or broad scans where rg or grep is enough. If sg is unavailable and syntax accuracy is required, stop and report that limitation instead of guessing.
allowed-tools: "Read Search Bash Write"
argument-hint: "[goal] [lang?] [path?]"
metadata:
    short-description: Structural search and rule-authoring workflow for ast-grep
---

# Ast-grep

Use this skill when syntax matters more than literal text.

## Default workflow

1. Translate the task into concrete constraints before running commands:
   - language
   - must-match examples
   - must-not-match examples
   - target node or selector
   - whether punctuation, comments, or text shape matter
   - search root and globs
2. Choose the narrowest path:
   - One-shot structural search or AST inspection: [references/search-and-debug.md](references/search-and-debug.md)
   - Reusable YAML rule plus `sg test` verification: [references/rules-and-tests.md](references/rules-and-tests.md)
   - Project scaffolding and `sgconfig.yml` layout: [references/project-and-config.md](references/project-and-config.md)
   - Rewrite, `fix`, `transform`, and `rewriters`: [references/rewrite-and-transform.md](references/rewrite-and-transform.md)
   - Prompt shaping for fragile patterns: [references/prompting.md](references/prompting.md)
   - Pattern model and tree concepts that explain surprising matches: [references/core-concepts.md](references/core-concepts.md)
   - Unsupported or custom parser setup: [references/custom-languages.md](references/custom-languages.md)
   - Common FAQ gotchas and failure patterns: [references/faq-gotchas.md](references/faq-gotchas.md)
   - Plain-text grep or missing-`sg` fallback: [references/fallbacks.md](references/fallbacks.md)
3. Prefer the smallest query that proves the structure.
4. If the workflow is becoming project-like, scaffold or normalize it with `sg new` and `sgconfig.yml` instead of improvising ad hoc files.
5. Only move to rewrite or bulk-apply flows if the user explicitly asks for code changes.
6. Verify reusable rules with `sg test` before trusting them.

## Operating rules

- Start with examples and counterexamples, not a large YAML rule.
- ast-grep is structural, not textual. If the requirement is really about raw text, file names, or logs, use grep or regex instead of forcing a pattern.
- The pattern must be valid parseable code for the selected language. If the target is only a sub-expression or a kind-sensitive fragment, move to a pattern object with `context` plus `selector`.
- ast-grep matches on Tree-sitter CST, not a simplified AST. Operators, punctuation, and modifiers can matter even when they look trivial.
- Pin `--lang` whenever the language is known or the pattern is ambiguous. `--debug-query` requires it.
- If the file extension is unusual but the parser already exists, prefer `sgconfig.yml` `languageGlobs` before reaching for a custom parser.
- If the target language is not built into ast-grep, do not guess. Register it through workspace-local `sgconfig.yml` `customLanguages` and rerun from the workspace root.
- If the target syntax is embedded inside another language, use `languageInjections` instead of pretending the whole file is the injected language.
- VT Code can install the full optional search bundle with `vtcode dependencies install search-tools`, or just the managed ast-grep copy with `vtcode dependencies install ast-grep`. The managed binary lands in `~/.vtcode/bin`.
- On Linux, prefer the canonical `ast-grep` binary name. VT Code avoids relying on `sg` there because that name can collide with the system `setgroups` command.
- On macOS and Windows, VT Code may also install an `sg` compatibility alias next to the managed `ast-grep` binary.
- Add `--selector` only when the actual match is a subnode inside the pattern.
- Tighten `--strictness` only when the task depends on exact syntax shape.
- `$VAR` matches named nodes only. Use `$$VAR` when unnamed nodes like punctuation or operators must be captured.
- If `$VAR` is not valid syntax in that language, use the custom language's configured `expandoChar` instead of forcing `$`.
- Metavariables match AST/CST nodes, not arbitrary substrings. For prefixes like `useSomething`, move to YAML constraints and regex instead of patterns like `use$HOOK`.
- Use `kind` for the node itself and `field` for the parent-child role. Reach for `has` or `inside` with `field` when the same kind appears in different positions such as object keys vs values.
- `constraints` apply after the main rule and only target single metavariables like `$ARG`, not list captures like `$$$ARGS`.
- Use local `utils` for repeated rule snippets inside one YAML file and global utility rules via `utilDirs` when the same helper should be shared across rule files.
- Prototype reusable checks with `sg scan --inline-rules`; persist them as rule files only after the query stabilizes.
- Use `sg new project|rule|test|util` when starting from scratch instead of hand-creating the file tree.
- When YAML rules depend on previously captured metavariables, force the evaluation order with `all`.
- If a modifier token like `get`, `static`, or an operator matters, spell it out explicitly instead of assuming named nodes preserve that distinction.
- Keep public `unified_search` `action="structural"` read-only. Use the ast-grep CLI path only when the task really needs `scan`, `test`, scaffolding, or rewrite behavior.
- When a query fails, inspect the AST before broadening the rule.
- Do not promise scope, type, control-flow, or data-flow reasoning. ast-grep is syntax-aware, not a static-analysis engine.
- If plain text is sufficient, prefer grep and do not add ast-grep complexity.
- If you need the managed binary available in the shell too, add `export PATH="$HOME/.vtcode/bin:$PATH"` yourself. VT Code does not edit shell profiles for you.
