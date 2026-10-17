---
feature: registry-light-critical
status: delivered
updated: 2026-09-24
branch: feat/registry-light-critical
commits: 0fdd25b7e2794cc27db878166821160636921a46..f12c4de0390e6b30808337e947ab4559ca934863
---

# Registry-Light Critical Path

## Report

**What was built** — `initialize_session_critical` no longer constructs `ToolRegistry` or runs
`discover_controller_subagents`. The typeable shell paints first (`session_setup_shell` ≈ 0 ms);
`complete_session_registry` builds the registry and discovers subagents after first paint (trace phase
`session_setup_registry`) and fills `SessionState.tool_registry` (late-filled `Option`). UI wiring that needs the
registry (exec-sessions OnceLock, pty counter, agent palette) runs when it exists; `apply_post_hydration_ui` still
re-drives header/palette. Hydration still gates the first model turn. Structural ratchet
`critical_path_avoids_registry_and_discovery` fails if builders return to critical.

### Verification

- `./scripts/check-dev.sh` — PASS
- Targeted `cargo nextest run -p vtcode` (ratchet, hydrate, initialize_session) — PASS (8–13 tests)
- Release PTY first-frame (isolated HOME, `--provider ollama --model llama3`): static-first-paint **~154 ms** →
  registry-light **~141 ms** median (samples 129–190 ms)
- `session_setup_shell` **0.0 ms**; registry+discovery now after first paint
- Baseline before both passes: **288.5 ms**. Residual ~141 ms is process spawn + TUI worker/crossterm first paint.
- Artifacts: `.vtcode/perf/post-registry-light-isolated-clean.json`

### Journey log

- Late-filled `Option<ToolRegistry>` is workable: turn loop takes the concrete registry after
  `complete_session_registry`; only session-setup/UI needed accessors.
- A `#[cfg(test)]` helper sitting _above_ the critical fn breaks `split("#[cfg(test)]")` ratchets — slice by function
  signatures in the full source instead.
- Hydrate requires the completer first; test helper `initialize_session` must call `complete_session_registry` before
  `hydrate_session_runtime`.
- Structural ratchets on `include_str!` must strip the tests module or slice between function signatures, or the
  forbidden list / test helper self-matches.
- Review caught a real panic: `initialize_session_ui` ran before `complete_session_registry` but called
  `tool_registry()` (`expect`). Early wiring must be Option-tolerant and re-drive OnceLock/pty from stored shell handles
  after the completer. Pair perf gates with a post-paint liveness check.
- Paste-duplication of `complete_session_registry` in a test helper is silent triple/quad discovery — collapse before
  review.

## [S1] Problem

`initialize_session_critical` still builds a full `ToolRegistry` and runs `discover_controller_subagents` **before**
`initialize_session_ui` wires the ready frame (`init.rs` join of `ToolRegistry::new*` +
`discover_controller_subagents`). Those two builders are the residual ~150 ms of `session_setup_critical` after
static-first paint. The TUI shell is already typeable; the user still waits for registry packs and a workspace/plugin
scan before header, palette, and exec wiring settle. Blog lesson applied: anything not required to _paint_ or _type_
must not sit before first frame — stub or defer it and re-drive.

Acceptance metric: **registry-free first frame**. `initialize_session_critical` must not construct `ToolRegistry` or
discover subagents; those complete after first paint and re-drive ready UI before the first model turn. Target: cut
`session_setup_critical` from ~150 ms to ≤ 20 ms (bootstrap-only), so warm first-frame tracks process spawn + TUI paint
(~50–150 ms residual from `static-first-paint`) instead of paying registry cost twice in the user’s path.

## [S2] Design

### Goals

1. **Registry-light critical** — `initialize_session_critical` builds only what the painted shell + typeable input need
   (bootstrap metadata, seed prompt, cheap execution-context shells). No `ToolRegistry`, no
   `discover_controller_subagents`.
2. **Stub then re-drive** — `SessionState.tool_registry` is `Option<ToolRegistry>` (or a late-filled handle). UI wiring
   that needs the registry (exec sessions, pty counter, agent palette) runs after first paint when the registry exists.
   Header primary-agent may show the config/default agent first and re-drive after discovery (`apply_post_hydration_ui`
   already re-drives `set_primary_agent`).
3. **First model turn still waits for hydration** — `hydrate_session_runtime` remains the readiness gate. Discovery
   result feeds primary-agent selection and `SubagentController` as today.

### Contracts

- Hydration failures still abort before a model turn.
- After hydration, tool surface / skills / system prompt / primary agent / MCP / permissions match prior post-setup
  behavior.
- Standalone `--version` / `--help` / `schema tools` must not regress.
- New opt-in trace phase: `session_setup_registry` (post-paint registry + discovery). Existing phases remain.
- Structural ratchet: `initialize_session_critical` production code must not contain `ToolRegistry::new*` or
  `discover_controller_subagents` (same `include_str!` pattern as `shell.rs`).
- `exec_sessions` OnceLock stays unset until the real registry exists; background-shortcut events no-op until then
  (already true).

### Out of scope

- Feature-gating heavy crates / LTO / allocator / binary-size work.
- Changing `ThreadEvent` or tool safety policy semantics.
- Making tools runnable before hydration.

## Tasks

- [x] T1: Capture pre-change warm first-frame baselines with `VTCODE_STARTUP_TRACE` — acceptance: phase timings saved
      under `.vtcode/perf/` (covers: S1)
- [x] T2: Make `SessionState.tool_registry` late-filled (`Option` or equivalent) and strip `ToolRegistry` +
      `discover_controller_subagents` from `initialize_session_critical` — acceptance: critical-path production code
      contains neither builder; compile + existing hydrate/critical tests pass (covers: S2)
- [x] T3: Build registry + discovery after first paint (post-spawn, before/inside hydration) and re-drive UI that
      depends on them (exec-session OnceLock, pty counter, agent palette, primary-agent header) — acceptance: keystrokes
      survive; ready header/primary-agent/palette match prior post-hydrate UI (covers: S2)
- [x] T4: Emit `session_setup_registry` and extend the structural ratchet to `init.rs` critical path — acceptance:
      `VTCODE_STARTUP_TRACE` reports the phase; ratchet fails if builders return to critical (covers: S2)
- [x] T5: Update `docs/development/performance.md` + binary gotchas for registry-light critical path — acceptance: docs
      describe the split and ratchet (covers: S2)
- [x] T6: Verify + re-measure — acceptance: `./scripts/check-dev.sh` and targeted nextest PASS; before/after numbers in
      Report (covers: S2; depends: T2–T5)
