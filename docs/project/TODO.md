implement control+r to search all prompts with actual logic, reference.

/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/search_prompts.png

---

implement double escape to rewind, reference.

1. show rewind history, reference. '/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-06-07 at 9.15.02 AM.png'
2. when user select a history. show 4 options. implement actual logic you see from the screenshot. reference. ''/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-06-07 at 9.15.35 AM.png'

===

1. fix vtcode's command ! shell mode doesn't work.

```
!cargo check
 •   cargo check completed with an error.
     Failure details are shown above.
     Suggested next steps:
     • Run tests with cargo nextest run.
     • Check lint warnings with cargo clippy --workspace
       --all-targets -- -D warnings.
```

```
─────────────────────────────────────────────────────────
!cargo fmt
 •   cargo fmt completed with an error.
     Failure details are shown above.
     Suggested next steps:
     • Verify the build with cargo check.
     • Run tests with cargo nextest run.
     • Check lint warnings with cargo clippy --workspace
       --all-targets -- -D warnings.
  ──────────────────────── Info ─────────────────────────
    Shell mode (!): executing command directly.
  ───────────────────────────────────────────────────────
```

3. the error is not shown. "Failure details are shown above."?

---

add this to vtcode's system prompt and design with the idea:

```
## Structural Code: ast-grep
Reach for 'ast-grep" whenever the work is structural - finding, matching, or rewriting code by its shape rather than its text. ast-grep matches the AST through tree-
sitter, so it sees real call expressions, declarations, and JSX nodes where grep only sees characters, and it speaks TypeScript, Python, Rust, Go, and more - one tool across the whole stack. It is installed on every machine (ast-grep -version).
Use it in two modes:
- **Search and codemod.** Before editing many files by hand, write one ast-grep pattern. 'SVAR' matches a single node, '$$$ARGS' matches many. A migration becomes deterministic, auditable, reversible pass instead of forty edits that drift and miss sites.
- find: 'ast-grep -p 'foo($$$ARGS)' -1 ts*
- rewrite: 'ast-grep -p '$A && $A()' -1 ts -r '$A?. () ' (inspect the diff, then add '-U' to apply)
- **Lint gate.** When a repo carries 'sgconfig.yml"
+ a rules/ dir, run ast-grep scan; it enforces the project's banned patterns and exits non-zero on any error-
severity match. When 'ast-grep scan" belongs to the repo's gate, it passes before you commit.
Prefer 'ast-grep' over grep/ripgrep for any code-shape question; keep text grep for prose, logs, and config strings. Always invoke the 'ast-grep' command - the "sg' alias collides with the system
sg •
```

check if vtcode's should have support for ast-grep built in, or just recommend it as a tool to use alongside vtcode. if built in, implement a command like '/astgrep' that takes the same arguments as the CLI and runs it, showing results in the TUI.
