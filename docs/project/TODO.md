idea design background task UI/UX and logic, configurable in /config.

similar to subagents system design and UI design

```
⎿  Added 2 lines, removed 2 lines
       1 -#ruby=3.1.7  # Informational only — real version pin is in .r
         -uby-version (read by rbenv)
       1 +#ruby=3.3.11  # Informational only — real version pin is in .
         +ruby-version (read by rbenv)
       2  source "https://rubygems.org"
       3
       4  gem "fastlane"
       5  gem 'prettier'
       6  gem 'dotenv-rails'
       7  gem "xcov"
       8 -gem 'cocoapods'
       8 +gem 'cocoapods', '~> 1.16'
       9  gem 'danger'
      10  gem 'danger-swiftlint'
      11

  Ran 1 shell command

⏺ Task Output bcmvpsney
  ⎿  (No output)

⏺ Task Output bcmvpsney
  ⎿  a44400000-a44800000 rw-
     a44800000-a44b00000 rw-
     a47000000-a47600000 rw-
     … +6 lines

⏺ Background command "Update CocoaPods to ~&gt; 1.16 in lockfile" completed
(exit code 0)

  Read 1 file, ran 3 shell commands

  Task Output br6jdags3
  Update CocoaPods in lockfile with Ruby 3.3.11 active
     Waiting for task (esc to give additional instructions)

* Doing… (23m 6s · ↓ 16.2k tokens)

──────────────────────────────────────────────────────────────────────────────
  Shell details

  Status:   running
  Runtime:  4m 29s
  Command:  eval "$(rbenv init - zsh)" && ruby --version && bundle update
            cocoapods 2>&1

  Output:
  ╭──────────────────────────────────────────────────────────────────────╮
  │ ruby 3.3.11 (2026-03-26 revision 1f2d15125a) [arm64-darwin25]        │
  │ Fetching gem metadata from https://rubygems.org/.......              │
  │ Resolving dependencies...                                            │
  │ Resolving dependencies...                                            │
  │                                                                      │
  │                                                                      │
  │                                                                      │
  │                                                                      │
  │                                                                      │
  │                                                                      │
  ╰──────────────────────────────────────────────────────────────────────╯
  Showing 4 lines

  ← to go back · Esc/Enter/Space to close · x to stop
```

---

fix the agent stop and can't work /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_379.json

---

implement control+r to search all prompts with actual logic, reference.

/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/search_prompts.png

---

critical: check and fix screenshot path

─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────
udpate readme to showcase screesnshot
Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/resources/screenshots/vtcode-01237.png
////////////////////////////////////////////////////// Error //////////////////////////////////////////////////////
LLM request failed: No endpoints found that support image input
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////
────────────────────────────────────────────────────── Info ───────────────────────────────────────────────────────
Hint: Review error details for specific issues; Check tool documentation for known limitations
───────────────────────────────────────────────────────────────────────────────────────────────────────────────────

==> no need to read the screenshot -> if the prompt is asking to update with image path. also refine and make sure image mention file is robust.

logs:

---

implement double escape to rewind, reference.

1. show rewind history, reference. '/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-06-07 at 9.15.02 AM.png'
2. when user select a history. show 4 options. implement actual logic you see from the screenshot. reference. ''/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-06-07 at 9.15.35 AM.png'

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

enhanced vtcode's support for ast-grep. also update this note in agents.md and in relevant subagents.

https://ast-grep.github.io/catalog/rust/

---

https://ast-grep.github.io/guide/tools/json.html

==

should we merge vtcode-design, vtcode-themes, vtcode-tui => vtcode-ui? the centralize crate for all UI components and design system. this way we can have a single source of truth for all UI related code and design decisions, and it will be easier to maintain and evolve the UI as a whole. also, it will reduce the number of crates we have to manage and publish, and it will make it easier for contributors to find and work on UI related code.

===

fix auto mode denied policy for running bash command. maybe suggest user to switch to auto-accept mode??
