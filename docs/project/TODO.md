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

implement control+r to search all prompts with actual logic, reference.

/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/search_prompts.png

---

implement double escape to rewind, reference.

1. show rewind history, reference. '/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-06-07 at 9.15.02 AM.png'
2. when user select a history. show 4 options. implement actual logic you see from the screenshot. reference. ''/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-06-07 at 9.15.35 AM.png'

--

1/ Yank vtcode-tui, vtcode-design, vtcode-theme 0.123.x from crates.io (now merged into vtcode-ui)
2/ Update release scripts to remove stale crate references
3/ File upstream issue on ratatui to add Send + Sync bounds to CellEffect trait

--

idea: build a new version release note TUI (use Info tui component). showing only when user first update to a new version, and can be accessed later in the help menu.

example

```

> VT Code  v2.1.170

 ▎ Meet Fable 5, our newest model for complex, long-running work. Try anytime with /model.
 ▎ Included in your plan limits for a limited time, then switch to usage credits to continue.

```
