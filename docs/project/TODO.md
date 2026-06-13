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

https://mimo.mi.com/llms.txt
Xiaomi provider implement hybrid token plan + API key setup

1. currently we already has Xiaomi API usage key
2. Now add token plan API endpoint and key format so that Xiaomi provider could also use Mimo models
3. When select XiaoMi providers in /model command -> show 2 options continuation + existing setup for user to choose from

https://mimo.mi.com/docs/en-US/tokenplan/Token%20Plan/subscription
