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

idea: build a new version release note TUI (use Info tui component). showing only when user first update to a new version, and can be accessed later in the help menu. show a note with top "# Highlight" items on release note, on click. open the latest release tag in github.

example

```

> VT Code  v2.1.170

 ▎ Meet Fable 5, our newest model for complex, long-running work. Try anytime with /model.
 ▎ Included in your plan limits for a limited time, then switch to usage credits to continue.

```

---

fix cargo install

```

error[E0277]: `dyn CellEffect` cannot be shared between threads safely
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/mod.rs:120:5
    |
120 | /     tokio::spawn(async move {
121 | |         if let Err(error) = run_tui(
122 | |             command_rx,
123 | |             event_tx,
...   |
163 | |     });
    | |______^ `dyn CellEffect` cannot be shared between threads safely
    |
    = help: the trait `Sync` is not implemented for `dyn CellEffect`
    = note: required for `Arc<dyn CellEffect>` to implement `std::marker::Send`
note: required because it appears within the type `ratatui_widgets::block::shadow::Effect`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block/shadow.rs:68:6
    |
 68 | enum Effect {
    |      ^^^^^^
note: required because it appears within the type `ratatui_widgets::block::shadow::Shadow`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block/shadow.rs:60:12
    |
 60 | pub struct Shadow {
    |            ^^^^^^
note: required because it appears within the type `Option<Shadow>`
   --> /Users/vinhnguyenxuan/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/core/src/option.rs:600:10
    |
600 | pub enum Option<T> {
    |          ^^^^^^
note: required because it appears within the type `ratatui::widgets::Block<'static>`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block.rs:217:12
    |
217 | pub struct Block<'a> {
    |            ^^^^^
note: required because it appears within the type `std::option::Option<ratatui::widgets::Block<'static>>`
   --> /Users/vinhnguyenxuan/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/core/src/option.rs:600:10
    |
600 | pub enum Option<T> {
    |          ^^^^^^
note: required because it appears within the type `TextArea<'static>`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-textarea-0.9.1/src/textarea.rs:106:12
    |
106 | pub struct TextArea<'a> {
    |            ^^^^^^^^
note: required because it appears within the type `input_manager::InputManager`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/session/input_manager.rs:73:12
    |
 73 | pub struct InputManager {
    |            ^^^^^^^^^^^^
note: required because it appears within the type `core_tui::session::Session`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/session.rs:198:12
    |
198 | pub struct Session {
    |            ^^^^^^^
note: required because it's used within this `async` fn body
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/runner/mod.rs:257:1
    |
257 | / {
258 | |     // Create a guard to mark TUI as initialized during the session
259 | |     // This ensures the panic hook knows to restore terminal state
260 | |     let _panic_guard = crate::tui::ui::tui::panic_hook::TuiPanicGuard::new();
...   |
384 | |     Ok(())
385 | | }
    | |_^
note: required because it's used within this `async` block
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/mod.rs:120:18
    |
120 |     tokio::spawn(async move {
    |                  ^^^^^^^^^^
note: required by a bound in `tokio::spawn`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.3/src/task/spawn.rs:176:21
    |
174 |     pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
    |            ----- required by a bound in this function
175 |     where
176 |         F: Future + Send + 'static,
    |                     ^^^^ required by this bound in `spawn`
    = note: the full name for the type has been written to '/var/folders/bw/b3wqv2xj57s853ypn022f87w0000gp/T/cargo-installt4d9m0/release/deps/vtcode_ui-36f53ea7829d06ad.long-type-12361127572278058028.txt'
    = note: consider using `--verbose` to print the full type name to the console

error[E0277]: `dyn CellEffect` cannot be sent between threads safely
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/mod.rs:120:5
    |
120 | /     tokio::spawn(async move {
121 | |         if let Err(error) = run_tui(
122 | |             command_rx,
123 | |             event_tx,
...   |
163 | |     });
    | |______^ `dyn CellEffect` cannot be sent between threads safely
    |
    = help: the trait `std::marker::Send` is not implemented for `dyn CellEffect`
    = note: required for `Arc<dyn CellEffect>` to implement `std::marker::Send`
note: required because it appears within the type `ratatui_widgets::block::shadow::Effect`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block/shadow.rs:68:6
    |
 68 | enum Effect {
    |      ^^^^^^
note: required because it appears within the type `ratatui_widgets::block::shadow::Shadow`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block/shadow.rs:60:12
    |
 60 | pub struct Shadow {
    |            ^^^^^^
note: required because it appears within the type `Option<Shadow>`
   --> /Users/vinhnguyenxuan/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/core/src/option.rs:600:10
    |
600 | pub enum Option<T> {
    |          ^^^^^^
note: required because it appears within the type `ratatui::widgets::Block<'static>`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block.rs:217:12
    |
217 | pub struct Block<'a> {
    |            ^^^^^
note: required because it appears within the type `std::option::Option<ratatui::widgets::Block<'static>>`
   --> /Users/vinhnguyenxuan/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/core/src/option.rs:600:10
    |
600 | pub enum Option<T> {
    |          ^^^^^^
note: required because it appears within the type `TextArea<'static>`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-textarea-0.9.1/src/textarea.rs:106:12
    |
106 | pub struct TextArea<'a> {
    |            ^^^^^^^^
note: required because it appears within the type `input_manager::InputManager`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/session/input_manager.rs:73:12
    |
 73 | pub struct InputManager {
    |            ^^^^^^^^^^^^
note: required because it appears within the type `core_tui::session::Session`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/session.rs:198:12
    |
198 | pub struct Session {
    |            ^^^^^^^
note: required because it's used within this `async` fn body
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/runner/mod.rs:257:1
    |
257 | / {
258 | |     // Create a guard to mark TUI as initialized during the session
259 | |     // This ensures the panic hook knows to restore terminal state
260 | |     let _panic_guard = crate::tui::ui::tui::panic_hook::TuiPanicGuard::new();
...   |
384 | |     Ok(())
385 | | }
    | |_^
note: required because it's used within this `async` block
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/mod.rs:120:18
    |
120 |     tokio::spawn(async move {
    |                  ^^^^^^^^^^
note: required by a bound in `tokio::spawn`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.3/src/task/spawn.rs:176:21
    |
174 |     pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
    |            ----- required by a bound in this function
175 |     where
176 |         F: Future + Send + 'static,
    |                     ^^^^ required by this bound in `spawn`
    = note: the full name for the type has been written to '/var/folders/bw/b3wqv2xj57s853ypn022f87w0000gp/T/cargo-installt4d9m0/release/deps/vtcode_ui-36f53ea7829d06ad.long-type-12361127572278058028.txt'
    = note: consider using `--verbose` to print the full type name to the console

error[E0277]: `dyn CellEffect` cannot be shared between threads safely
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/session_options.rs:100:5
    |
100 | /     tokio::spawn(async move {
101 | |         if let Err(error) = run_tui(
102 | |             command_rx,
103 | |             event_tx,
...   |
146 | |     });
    | |______^ `dyn CellEffect` cannot be shared between threads safely
    |
    = help: the trait `Sync` is not implemented for `dyn CellEffect`
    = note: required for `Arc<dyn CellEffect>` to implement `std::marker::Send`
note: required because it appears within the type `ratatui_widgets::block::shadow::Effect`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block/shadow.rs:68:6
    |
 68 | enum Effect {
    |      ^^^^^^
note: required because it appears within the type `ratatui_widgets::block::shadow::Shadow`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block/shadow.rs:60:12
    |
 60 | pub struct Shadow {
    |            ^^^^^^
note: required because it appears within the type `Option<Shadow>`
   --> /Users/vinhnguyenxuan/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/core/src/option.rs:600:10
    |
600 | pub enum Option<T> {
    |          ^^^^^^
note: required because it appears within the type `ratatui::widgets::Block<'static>`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block.rs:217:12
    |
217 | pub struct Block<'a> {
    |            ^^^^^
note: required because it appears within the type `std::option::Option<ratatui::widgets::Block<'static>>`
   --> /Users/vinhnguyenxuan/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/core/src/option.rs:600:10
    |
600 | pub enum Option<T> {
    |          ^^^^^^
note: required because it appears within the type `TextArea<'static>`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-textarea-0.9.1/src/textarea.rs:106:12
    |
106 | pub struct TextArea<'a> {
    |            ^^^^^^^^
note: required because it appears within the type `input_manager::InputManager`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/session/input_manager.rs:73:12
    |
 73 | pub struct InputManager {
    |            ^^^^^^^^^^^^
note: required because it appears within the type `core_tui::session::Session`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/session.rs:198:12
    |
198 | pub struct Session {
    |            ^^^^^^^
note: required because it appears within the type `AppSession`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/app/session/mod.rs:47:12
    |
 47 | pub struct AppSession {
    |            ^^^^^^^^^^
note: required because it's used within this `async` fn body
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/runner/mod.rs:257:1
    |
257 | / {
258 | |     // Create a guard to mark TUI as initialized during the session
259 | |     // This ensures the panic hook knows to restore terminal state
260 | |     let _panic_guard = crate::tui::ui::tui::panic_hook::TuiPanicGuard::new();
...   |
384 | |     Ok(())
385 | | }
    | |_^
note: required because it's used within this `async` block
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/session_options.rs:100:18
    |
100 |     tokio::spawn(async move {
    |                  ^^^^^^^^^^
note: required by a bound in `tokio::spawn`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.3/src/task/spawn.rs:176:21
    |
174 |     pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
    |            ----- required by a bound in this function
175 |     where
176 |         F: Future + Send + 'static,
    |                     ^^^^ required by this bound in `spawn`
    = note: the full name for the type has been written to '/var/folders/bw/b3wqv2xj57s853ypn022f87w0000gp/T/cargo-installt4d9m0/release/deps/vtcode_ui-36f53ea7829d06ad.long-type-12361127572278058028.txt'
    = note: consider using `--verbose` to print the full type name to the console

error[E0277]: `dyn CellEffect` cannot be sent between threads safely
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/session_options.rs:100:5
    |
100 | /     tokio::spawn(async move {
101 | |         if let Err(error) = run_tui(
102 | |             command_rx,
103 | |             event_tx,
...   |
146 | |     });
    | |______^ `dyn CellEffect` cannot be sent between threads safely
    |
    = help: the trait `std::marker::Send` is not implemented for `dyn CellEffect`
    = note: required for `Arc<dyn CellEffect>` to implement `std::marker::Send`
note: required because it appears within the type `ratatui_widgets::block::shadow::Effect`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block/shadow.rs:68:6
    |
 68 | enum Effect {
    |      ^^^^^^
note: required because it appears within the type `ratatui_widgets::block::shadow::Shadow`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block/shadow.rs:60:12
    |
 60 | pub struct Shadow {
    |            ^^^^^^
note: required because it appears within the type `Option<Shadow>`
   --> /Users/vinhnguyenxuan/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/core/src/option.rs:600:10
    |
600 | pub enum Option<T> {
    |          ^^^^^^
note: required because it appears within the type `ratatui::widgets::Block<'static>`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-widgets-0.3.1/src/block.rs:217:12
    |
217 | pub struct Block<'a> {
    |            ^^^^^
note: required because it appears within the type `std::option::Option<ratatui::widgets::Block<'static>>`
   --> /Users/vinhnguyenxuan/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/src/rust/library/core/src/option.rs:600:10
    |
600 | pub enum Option<T> {
    |          ^^^^^^
note: required because it appears within the type `TextArea<'static>`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/ratatui-textarea-0.9.1/src/textarea.rs:106:12
    |
106 | pub struct TextArea<'a> {
    |            ^^^^^^^^
note: required because it appears within the type `input_manager::InputManager`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/session/input_manager.rs:73:12
    |
 73 | pub struct InputManager {
    |            ^^^^^^^^^^^^
note: required because it appears within the type `core_tui::session::Session`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/session.rs:198:12
    |
198 | pub struct Session {
    |            ^^^^^^^
note: required because it appears within the type `AppSession`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/app/session/mod.rs:47:12
    |
 47 | pub struct AppSession {
    |            ^^^^^^^^^^
note: required because it's used within this `async` fn body
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/core_tui/runner/mod.rs:257:1
    |
257 | / {
258 | |     // Create a guard to mark TUI as initialized during the session
259 | |     // This ensures the panic hook knows to restore terminal state
260 | |     let _panic_guard = crate::tui::ui::tui::panic_hook::TuiPanicGuard::new();
...   |
384 | |     Ok(())
385 | | }
    | |_^
note: required because it's used within this `async` block
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/vtcode-ui-0.125.0/src/tui/session_options.rs:100:18
    |
100 |     tokio::spawn(async move {
    |                  ^^^^^^^^^^
note: required by a bound in `tokio::spawn`
   --> /Users/vinhnguyenxuan/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.52.3/src/task/spawn.rs:176:21
    |
174 |     pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
    |            ----- required by a bound in this function
175 |     where
176 |         F: Future + Send + 'static,
    |                     ^^^^ required by this bound in `spawn`
    = note: the full name for the type has been written to '/var/folders/bw/b3wqv2xj57s853ypn022f87w0000gp/T/cargo-installt4d9m0/release/deps/vtcode_ui-36f53ea7829d06ad.long-type-12361127572278058028.txt'
    = note: consider using `--verbose` to print the full type name to the console

For more information about this error, try `rustc --explain E0277`.
error: could not compile `vtcode-ui` (lib) due to 4 previous errors
warning: build failed, waiting for other jobs to finish...
error: failed to compile `vtcode v0.125.0`, intermediate artifacts can be found at `/var/folders/bw/b3wqv2xj57s853ypn022f87w0000gp/T/cargo-installt4d9m0`.
To reuse those artifacts with a future compilation, set the environment variable `CARGO_TARGET_DIR` to that path.
```
