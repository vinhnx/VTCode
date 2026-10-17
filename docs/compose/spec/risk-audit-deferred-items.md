---
feature: risk-audit-deferred-items
status: delivered
updated: 2026-09-27
branch: fix/risk-audit-deferred
commits: f10535ad1..0e9155b0b
---

# Risk-Audit Deferred Items

## Report

**What was built** — Closed the four open risk-audit deferred items (A4, C2, C3, model-catalog) on
`fix/risk-audit-deferred`.

1. **A4 env-value injection.** `environment_prefix_has_injection_keys` in `command_args.rs` inspects `env KEY=value` /
   bare `KEY=value` prefixes for a denylist of config/loader/interpreter injection keys (`GIT_CONFIG_*`, `LD_PRELOAD`,
   `BASH_ENV`, `NODE_OPTIONS`, …) plus the `GIT_CONFIG_KEY_` / `GIT_CONFIG_VALUE_` families. Wired into
   `has_unsafe_readonly_options` so every readonly/activity path rejects those commands. Tree-sitter extraction now
   keeps `variable_assignment` and `concatenation` spans so bare `NODE_OPTIONS='…'` prefixes reach the scanner (they
   were previously dropped).
2. **C2 turn tail.** Extracted `complete_turn_persistence_tail` (metrics → session checkpoint → memory envelope) into
   `turn_tail.rs`. All four `select_approved_plan_execution_agent` failure arms run it before `continue`; the post-turn
   arm passes the real `turn_diagnostics` / elapsed / history bytes.
3. **C3 notifications.** Vendored `mac-notification-sys` 0.6.15 under `patches/mac-notification-sys` with an explicit
   `Unset`/`Set`/`Failed` state instead of `Once`: failed setup is retryable, success is idempotent, and
   `ensure_application_set` does not fall through to AppleScript after failure. The vtcode wrapper calls
   `set_application` on every send (no failure cache).
4. **Model catalog.** `supported_models_include_current_reasoning_models` is pinned to the pruned catalog:
   `O3`/`O4_MINI` stay out of `REASONING_MODELS` (remap constants only).

**Verification** —

- `cargo nextest run -p vtcode-core -E 'test(env_value_injection) or test(env_prefix_injection) or test(readonly)'` —
  PASS 45/45
- `cargo nextest run -p vtcode-llm -E 'test(supported_models_include_current_reasoning_models)'` — PASS 1/1
- `cargo nextest run -p vtcode-core --features desktop-notifications -E 'test(macos_notification)'` — PASS 2/2
- `cargo nextest run -p vtcode-safety` — PASS 278/278
- `cargo nextest run -p vtcode -E 'test(session_loop_runner)'` — PASS 81/81
- `cargo test --manifest-path patches/mac-notification-sys/Cargo.toml --lib` — PASS 19/19
- `cargo clippy -p vtcode -p vtcode-core -p vtcode-safety -p vtcode-llm --tests -- -D warnings` — PASS
- Independent review of `f10535ad1..934a79668` found 1 high (MutexGuard held across re-lock in `ensure_application_set`)
  and 1 medium (fresh_context Err arm skipped the tail). Both fixed in `0e9155b0b`; targeted suites re-ran green.

**Journey log** —

- Tree-sitter drops bare `KEY=value` from command words (`variable_assignment` / `concatenation` nodes), so the env
  scanner never saw the A4 vector until extraction kept those spans. `env KEY=value` worked earlier only because `env`
  receives the assignment as an argument word.
- `match *lock_application_state()` keeps the `MutexGuard` temporary alive for the whole match — calling
  `set_application` from the `Unset` arm deadlocks on the same mutex. Drop the guard before re-entering.
- The spec counted three selection-failure sites; the loop has four Err arms because `fresh_context` splits one.
  Enumerate arms, not call sites.
- Model-catalog "flake" was a hard fail: the prune dropped `O3`/`O4_MINI` from `REASONING_MODELS` while the test still
  asserted the old comment.

## [S1] Problem

The 2026-09-27 risk-audit fixes (cf82d6df1, 1b6d8e0be) closed B2/B3/B4/D2/A5 and left four open items plus one
mis-diagnosed flake:

1. **A4 plan-mode env-value injection.** `command_words_after_environment_prefix` strips `env VAR=value` / leading
   `VAR=value` prefixes without inspecting values.
   `env GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0='…' git status` reproduces the blocked
   `-c` config-injection vector in plan mode. The same hole admits loader/interpreter injection (`LD_PRELOAD`,
   `BASH_ENV`, `NODE_OPTIONS`, …) in front of an otherwise read-only program.
2. **C2 approved-plan selection failure skips the turn tail.** Three `select_approved_plan_execution_agent` failure
   paths `continue` and skip metrics/checkpoint. Two of them run after a real turn's work (plan-approval handoff and
   fresh-execution setup), so a failure can drop that turn's checkpoint even though history changed.
3. **C3 macOS notification setup is non-retryable.** `notify_rust::set_application` is wrapped in `OnceLock<Result<…>>`
   in `notifications/mod.rs`, and `mac-notification-sys` marks `INIT_APPLICATION_SET` with `Once::call_once` even when
   `setApplication` fails. A single failed setup disables desktop notifications for the process lifetime; later
   `set_application` calls return `AlreadySet` without retrying the native call.
4. **Model-catalog test is a hard fail, not a flake.** `supported_models_include_current_reasoning_models` asserts
   `O3`/`O4_MINI` remain in `REASONING_MODELS` after the catalog prune (684bdcb90); the const no longer lists them.

## [S2] Design

### A. A4 — inspect env assignments for injection keys

Extend the shared env-prefix scanner in `vtcode-core::tools::command_args` without changing the strip-and-classify
contract of `command_words_after_environment_prefix` (intent and activity must keep seeing the same executable).

New shared helper `environment_prefix_has_injection_keys(words: &[String]) -> bool` (true when any assignment before the
executable sets a known injection key):

- Collect `KEY=value` words that appear before the executable, including those under `env` (after
  `-u`/`--unset`/`-C`/`--chdir`/`--` handling). `-u KEY` / `--unset KEY` does **not** count as an assignment.
- Match keys case-sensitively against a denylist of process/config injection names and two families:
  - exact: `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`, `GIT_CONFIG_NOSYSTEM`, `GIT_EXTERNAL_DIFF`, `GIT_TEXTCONV`,
    `GIT_DIFF_OPTS`, `GIT_EDITOR`, `GIT_SEQUENCE_EDITOR`, `GIT_SSH`, `GIT_SSH_COMMAND`, `GIT_EXEC_PATH`, `LD_PRELOAD`,
    `LD_AUDIT`, `LD_LIBRARY_PATH`, `DYLD_INSERT_LIBRARIES`, `DYLD_LIBRARY_PATH`, `DYLD_FRAMEWORK_PATH`,
    `DYLD_FALLBACK_LIBRARY_PATH`, `BASH_ENV`, `ENV`, `SHELLOPTS`, `PERL5OPT`, `PYTHONSTARTUP`, `NODE_OPTIONS`,
    `RUBYOPT`, `EDITOR`, `VISUAL`, `PAGER`
  - prefix: `GIT_CONFIG_KEY_`, `GIT_CONFIG_VALUE_` (covers `GIT_CONFIG_COUNT` companion keys). `GIT_CONFIG_COUNT` itself
    is exact-denylisted.
- Unknown keys (`LANG`, `FOO=bar`) stay allowed — plan mode keeps working.
- `env -S` / `--split-string` remains fail-closed (already returns an empty command slice).

Wire the helper next to `has_unsafe_readonly_options` so every readonly / activity classification path rejects a command
whose env prefix carries an injection key. Adversarial regression tests live in `tool_intent/readonly.rs`.

### B. C2 — one shared turn-tail helper

Extract the metrics + checkpoint + memory-envelope block of the turn loop (`orchestration.rs` emit →
`persist_session_checkpoint` → `refresh_session_memory_envelope_async`) into a helper (e.g.
`complete_turn_persistence_tail`) taking a small context struct.

Call sites:

1. The existing success path after a real turn outcome (keeps tracker auto-continue and stall bookkeeping **after** the
   helper returns).
2. Each of the three `select_approved_plan_execution_agent` failure paths, with outcome `"aborted"`, **before**
   `continue`.

Failure paths must still recover (no hard session abort) and must not claim a completed turn. Selection failure happens
before request send on one site and after a finished turn on the others; the helper only persists whatever history is
already in `runtime.state.messages`, so both shapes are safe.

### C. C3 — vendor a retryable `mac-notification-sys`

Vendor `mac-notification-sys` 0.6.15 under `third-party/mac-notification-sys` and patch via `[patch.crates-io]`:

- Replace `INIT_APPLICATION_SET: Once` with an explicit state (`Unset` / `Set` / `Failed`) so a failed `setApplication`
  can be retried and a successful set is idempotent (`Ok` on later calls instead of `AlreadySet`).
- `ensure_application_set` treats only `Set` as done. `Failed` returns an error (no AppleScript fallthrough); callers
  retry `set_application` until success. The state lock is released before any re-entrant call.
- Preserve the AppleScript-skip behavior: callers still `set_application` first.

Update `vtcode-core::notifications::ensure_macos_notification_application`:

- Stop caching a failed setup in `OnceLock` forever; store success only, or call the now-idempotent `set_application` on
  each send.
- Treat an already-configured application as success.

Add a third-party header note and keep `scripts/generate-notices.sh` in sync when the lockfile source for
`mac-notification-sys` changes to the path patch.

### D. Model catalog

Pin `supported_models_include_current_reasoning_models` to the pruned catalog:

- `O3` / `O4_MINI` stay **out** of `REASONING_MODELS` (deprecated remap constants only).
- Keep picker exclusions (`!supported.contains(O3/O4_MINI)`).
- Keep current-reasoning membership asserts (`gpt-5.6-sol`, default model).
- Replace the stale "retained in REASONING_MODELS" comment.

## [S3] Out of Scope

- A3 name-mistake preflight streaks and A6 assistant-text-response cap — deliberate trade-offs already documented in
  handlers.
- Broader env allowlisting (option c) and failing closed on every env prefix (option b).
- Upstream `mac-notification-sys` / `notify-rust` releases (the vendor patch may be offered upstream later; not required
  here).
- Restructuring the entire orchestration loop beyond the extracted tail helper.
- Unrelated uncommitted TUI work sitting on `main`.

## Tasks

- [x] T1: Add `environment_prefix_has_injection_keys` and deny readonly/activity classification when it fires —
      acceptance: `env GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0=x git status` and
      `LD_PRELOAD=evil.so ls` are not readonly; `LANG=C rg foo` and `FOO=bar git status` remain readonly (covers: S2.A)
- [x] T2: Adversarial regression tests for env-value injection in `tool_intent/readonly.rs` — acceptance: tests cover
      exact keys, `GIT_CONFIG_KEY_*`/`VALUE_*` prefixes, `env` vs bare prefixes, `env -u` non-assignment, and the
      benign-allow cases (covers: S2.A; depends: T1)
- [x] T3: Extract `complete_turn_persistence_tail` and call it from the selection-failure sites with outcome `aborted`
      before `continue` — acceptance: a selection failure still emits turn metrics and persists the session checkpoint;
      no hard session abort (covers: S2.B)
- [x] T4: Vendor patched `mac-notification-sys` under `patches/mac-notification-sys` with retryable `set_application`
      and `[patch.crates-io]` — acceptance: unit test in the vendored crate shows failed setup can retry and second
      success returns Ok; workspace still builds with `desktop-notifications` (covers: S2.C)
- [x] T5: Point `ensure_macos_notification_application` at the retryable API (no permanent failure cache) — acceptance:
      macOS notification setup failure no longer poisons later sends; wrapper treats already-set as Ok (covers: S2.C;
      depends: T4)
- [x] T6: Pin `supported_models_include_current_reasoning_models` to the pruned catalog — acceptance:
      `cargo nextest run -p vtcode-llm -E 'test(supported_models_include_current_reasoning_models)'` passes (covers:
      S2.D)
- [x] T7: Record verification and journey in this document's Report — acceptance: Report has What was built /
      Verification / Journey log (covers: S1)
