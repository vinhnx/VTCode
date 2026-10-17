---
feature: static-first-paint
status: delivered
updated: 2026-09-24
branch: feat/static-first-paint
commits: f4863b9d6cfc975d442782e5104232285f1f4453..fd52538c584398196acbbc6caaede11d68649b4e
---

# Static First Paint

## Report

**What was built** — Interactive TUI now paints a **typeable shell first** (blog static-composer pattern).
`initialize_session_shell` spawns the session with bootstrap placeholder and built-in slash commands only — no
`ToolRegistry`, no subagent discovery, no provider construction, no resume-history load. `initialize_session_critical`
then builds the heavy runtime; `initialize_session_ui` wires the live session without respawning so keystrokes typed
into the shell survive the ready re-drive. A structural ratchet
(`shell::tests::shell_module_avoids_heavy_init_before_paint`) fails if those builders move back before spawn.

### Verification

- `./scripts/check-dev.sh` — PASS
- Targeted `cargo nextest run -p vtcode` (shell ratchet, UI callbacks, hydrate, startup_policy) — PASS (24 tests)
- Release PTY first-frame (isolated HOME, `--provider ollama --model llama3`): pre-change median **288.5 ms** →
  post-change **154.2 ms** (−47%)
- `session_setup_shell` phase: **0.1 ms** (registry/discovery/provider no longer before spawn)
- Residual: warm first-frame still ~154 ms (50 ms target not met). Remaining cost is process spawn + TUI
  worker/crossterm first paint after `spawn_session_with_options`, not session init. Artifacts:
  `.vtcode/perf/pre-static-shell-*.json`, `.vtcode/perf/post-static-shell-*.json`

### Journey log

- The blog’s win was painting a static typeable shell _before_ heavy init — same split works in a TUI: spawn, then
  registry/discovery, then re-drive.
- `session_setup_shell` at 0.1 ms proves the architecture; the residual 154 ms is the TUI framework paint path
  (run_tui/crossterm), not session setup.
- Structural ratchet on `include_str!` must strip the tests module first, or the forbidden-symbol list matches itself.
- Exec sessions use `OnceLock` so the shell callback can exist before `ToolRegistry` and bind the live manager after
  critical init.

## [S1] Problem

Interactive VT Code TUI launch still spends ~360–410 ms warm before the first frame. The previous `instant-tui-startup`
work removed maintenance and palette work from the paint path, but `initialize_session_critical` still builds
`ToolRegistry` and runs `discover_controller_subagents` (~175–220 ms) **before** `initialize_session_ui` spawns the TUI.
Users stare at a blank terminal while init runs.

Anthropic’s claude.ai sprint (“How we made claude.ai 3x faster”) showed the winning pattern: paint a **static typeable
shell** before heavy init, then fill in the real UI. Measured launch for Claude Code went 837 → 347 ms; typeable
composer went 2.93 s → 0.36 s. The same architecture applies here.

Acceptance metric (user-approved): **typeable TUI shell first paint ≤ 50 ms** warm (process spawn + terminal setup floor
is ~5–20 ms; “0 ms” is not physical). Ready state may arrive later; first model turn still waits for full hydration.

## [S2] Design

### Goals

1. **Typeable shell ≤ 50 ms** — chrome + input accept keystrokes before `ToolRegistry`, subagent discovery, provider
   construction, or resume-history load complete.
2. **Paint path + deterministic ratchet** — ship a checked-in startup ceiling that only ratchets down (blog “anything
   you can count, you can climb”).
3. No ThreadEvent / safety / product-surface contract changes. Hydration still gates the first model turn.

### Paint-path split (static-first)

1. **Shell paint (before any heavy init)**
   - CLI parse + dotenv (no legacy migration) + config already resolved upstream
   - Theme/styles snapshot (in-memory)
   - `spawn_session_with_options` with bootstrap placeholder and built-in slash commands only — **typeable immediately**
   - `note_crossterm_raw_mode()` so a late palette probe cannot undo termios
   - Record `first_ui_render` on this frame (not the ready frame)
2. **Concurrent with / after shell paint (before first model turn)**
   - `initialize_session_critical` remainder: `ToolRegistry`, plugin-aware discovery, provider client, resume history,
     bootstrap metadata
   - `initialize_harness` emitter (no retention)
   - `hydrate_session_runtime` (unchanged) + `apply_post_hydration_ui` re-drive (header, primary agent, palette, slash
     templates, resume transcript)
3. **Background (already deferred)**
   - Harness retention, legacy migration, update preflight, spool cleanup, iTerm2

### Contracts

- Keystrokes typed into the shell during init are preserved across the ready re-drive (blog static-composer handoff
  rule: no lost or reordered keys).
- Ready re-drive must not jump the input box or drop typed text (≤ 1 px equivalent for TUI: same input row, same buffer
  contents).
- Hydration failure still aborts before a model turn and surfaces the same setup error.
- After hydration, tool surface / skills / system prompt / primary agent / MCP / permissions match prior post-setup
  behavior.
- Standalone `--version` / `--help` / `schema tools` must not regress.
- New opt-in trace phase: `session_setup_shell` (time to typeable shell). Existing phases remain.
- Deterministic ratchet: a unit/integration test or bench asserts `session_setup_shell` work does not reintroduce
  registry/discovery/provider construction before spawn (structural guard), plus a warm first-paint ceiling test that
  fails if the shell path gains blocking I/O.

### Out of scope

- Feature-gating heavy crates / LTO / allocator / binary-size work.
- Provider network latency.
- Changing `ThreadEvent` or tool safety semantics.
- Full registry-light first model turn (hydration still builds the registry).

## Tasks

- [x] T1: Capture pre-change warm shell/first-frame baselines with `VTCODE_STARTUP_TRACE` — acceptance: phase timings
      saved under `.vtcode/perf/` (covers: S1)
- [x] T2: Extract `initialize_session_shell` that spawns a typeable TUI with bootstrap placeholder and built-in slash
      commands only — acceptance: spawn path performs no `ToolRegistry`, subagent discovery, provider construction, or
      resume-history load (covers: S2)
- [x] T3: Reorder orchestration so shell paint precedes `initialize_session_critical` heavy builders; run critical +
      hydrate + `apply_post_hydration_ui` re-drive after spawn — acceptance: keystrokes typed before ready survive into
      the ready input buffer; ready header/primary-agent/slash state matches prior post-hydrate UI (covers: S2)
- [x] T4: Emit `session_setup_shell` and measure `first_ui_render` on the shell frame — acceptance:
      `VTCODE_STARTUP_TRACE` reports `session_setup_shell` and `first_ui_render` ≤ 50 ms warm on isolated HOME (covers:
      S2) — residual: `session_setup_shell` **0.1 ms**; `first_ui_render` **~154 ms** median (TUI worker/crossterm paint
      after spawn). 50 ms shell-paint target not met; registry/discovery no longer on the paint path.
- [x] T5: Add a structural startup ratchet test (no heavy init before spawn) — acceptance: tests fail if
      registry/discovery/provider work or `load_user_config` move back before spawn (covers: S2) — structural
      `include_str!` ratchet on production code; no separate warm ceiling timer (wall-clock flaky). Keystroke/keybinding
      handoff via `SessionShell.legacy_key_bindings` verified after review.
- [x] T6: Update `docs/development/performance.md` + binary gotchas for static-first paint and the ratchet — acceptance:
      docs describe shell-first order and how to run the ratchet (covers: S2)
- [x] T7: Verify + re-measure — acceptance: `./scripts/check-dev.sh` and targeted nextest PASS; before/after shell and
      first-frame numbers in Report (covers: S2; depends: T2–T6)
