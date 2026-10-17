---
feature: instant-tui-startup
status: delivered
updated: 2026-09-24
branch: fix/instant-tui-startup-regression
commits: 821a9c35a4a6cb35ce49083b8274eb0edb8c57b5..00da2b354103299619569225d96a2a081c7b487e
---

# Instant TUI Startup

## Report

**What was built** — Interactive first paint no longer waits on legacy path migration, harness session-store retention,
or the terminal palette probe. `bootstrap_main` loads `.env` on the critical path but defers
`migrate_legacy_global_paths` to `spawn_blocking` for interactive launches (non-interactive consumers still migrate
synchronously). `initialize_harness` now only opens the canonical emitter; retention and legacy log pruning run in
`run_harness_retention`, spawned after `initialize_session_ui`. The palette probe is never awaited before TUI spawn
(`note_crossterm_raw_mode` makes a late `RawModeGuard` restore a no-op; `await_terminal_palette_probe` drains after
spawn; timeout 50 ms). iTerm2 icon ensure is `spawn_blocking`. Theme/palette still settle before the first model turn;
hydration contracts are unchanged.

### Verification

- `./scripts/check-dev.sh` — PASS (also fixed pre-existing vtcode-ui clippy `let_and_return` / `float_cmp` and a test
  `unreachable!`)
- Targeted `cargo nextest run -p vtcode` (hydrate / bootstrap / startup_policy / harness / migration deferral) — PASS
  (17–18 tests)
- Release PTY first-frame (isolated HOME, `--provider ollama --model llama3`): pre-change median **472.8 ms** →
  post-change **~360–410 ms** (samples in `.vtcode/perf/post-change-*.json`)
- Large-store (120-session copy) first-frame: pre-change **2214 ms** with `dotenv_and_migration=1707 ms` spike →
  post-change **~375 ms**, `dotenv_and_migration≈0.1 ms`
- Residual: warm first-frame still ~360–410 ms (target 150 ms not met). Dominated by `session_setup_critical` ~175–220
  ms (`ToolRegistry` + `discover_controller_subagents`) plus UI spawn. Artifacts: `.vtcode/perf/pre-change-*.json`,
  `.vtcode/perf/post-change-*.json`, `.vtcode/perf/post-change-final-*.json`

### Journey log

- The user-visible “launch takes forever” regression was the large-store `migrate_legacy` spike (1.7 s), not the
  2026-09-18 hydrate split itself.
- Harness retention before `initialize_session_ui` was silent paint-path I/O; splitting emitter vs retention was the
  highest-leverage structural fix.
- Palette probe: overlapping it with critical init still serialized on a silent PTY; only “don’t wait before spawn +
  make RawModeGuard restore cooperative” removed the paint-path gate (timeout now 50 ms).
- `session_setup_critical` remains the floor until a registry-light paint path exists (previous journey already named
  this follow-up).
- Concurrency during measurement (parallel cargo) once produced a fake 1.2 s `session_setup_critical`; always measure on
  an idle machine.

## [S1] Problem

Interactive first-frame latency regressed after the 2026-09-18 critical/hydrate split. Measured on the shipped 0.169.2
release binary with `VTCODE_STARTUP_TRACE=1` (isolated HOME + ollama/llama3, and again against the real HOME/workspace):

| Case                                      | first_ui_render   | Notes                                    |
| ----------------------------------------- | ----------------- | ---------------------------------------- |
| Isolated clean HOME                       | 382–550 ms        | median ~420 ms                           |
| Real HOME + vtcode workspace              | 414–449 ms        | session_setup_critical 146–155 ms        |
| Copied large session store (120 sessions) | up to **2214 ms** | `dotenv_and_migration` spike **1707 ms** |
| Standalone `--version` / `--help`         | 5–6 ms            | no regression                            |
| Standalone `schema tools`                 | 42 ms             | no regression                            |

The previous delivery reported ~468 ms warm first-frame. Current warm first-frame is similar or worse on clean machines
and **much worse** when `.vtcode` has accumulated sessions. Users experience this as “launch takes a long time.”

Phase decomposition (real env run):

- `bootstrap` ≈ 11 ms
- `session_setup_critical` ≈ 150 ms
- `session_setup_ui` ≈ 6 ms (only when probe replies quickly)
- `first_ui_render` ≈ 414–547 ms total wall from process start

So roughly **250–350 ms is untraced work before first paint**, plus an occasional multi-second `dotenv_and_migration`
stall. The prior design’s hydration split is still in place; the regression is work that was added to or left on the
paint path around it.

Bottlenecks (ordered by measured impact):

1. **`dotenv_and_migration` / `VtCodePaths::migrate_legacy`** (`src/main.rs` `migrate_legacy_global_paths`) runs
   synchronously in `bootstrap_main` before dispatch. `LegacyMigrator::run` always touches the marker, parent dir, and
   may scan/copy legacy trees. Observed spike: **1707 ms** with a large store present. Even the fast path is serial with
   everything else before the agent loop.
2. **Terminal palette probe await** (`src/agent/agents.rs` calls `await_terminal_palette_probe()` at the top of
   `run_single_agent_loop`). The probe itself has a **200 ms** timeout (`terminal_color_probe.rs`). It is started early,
   but bootstrap is only ~12–18 ms, so the await commonly still pays most of the 200 ms before any session setup. This
   is a large slice of the untraced gap.
3. **`initialize_harness` sits between critical init and UI spawn** (`session_loop_runner/orchestration.rs` then
   `harness.rs`). It runs `prune_old_harness_logs` + `vtcode_memory::apply_retention_preserving` (walks every session
   dir, may `rmtree` dozens of sessions) **before** `initialize_session_ui`. With 120+ sessions this is real disk I/O on
   the paint path.
4. **`session_setup_critical` still ~140–155 ms** — `ToolRegistry::new*` + `discover_controller_subagents` remain on the
   critical path (known follow-up from the previous delivery).
5. **Session archive setup before paint** — `reserve_session_archive_identifier` / `create_session_archive` /
   `checkpoint_session_archive_start` run before `initialize_session_critical`.

## [S2] Design

User-approved goals (carried forward):

1. **Acceptance metric** — interactive first-frame latency (warm, release).
2. **Scope** — runtime path only; no cargo feature-gating of heavy crates.
3. **Readiness model** — minimal UI first, then ready; first model turn waits for hydration.

New target for this amendment: **warm interactive first-frame ≤ 150 ms** on an isolated clean HOME (and no worse than
~250 ms with a large session store), without changing ThreadEvent / safety / product-surface contracts.

### Contracts

- Hydration failures still abort before a model turn.
- After hydration, tool surface / skills / system prompt / primary agent / MCP / permissions match prior post-setup
  behavior.
- Standalone `--version` / `--help` / `schema tools` must not regress.
- Opt-in trace phases remain: `session_setup_critical`, `session_setup_ui`, `session_setup_hydrate`, `session_setup`,
  `first_ui_render`. New optional sub-phases are allowed if they stay opt-in and silent when unset.
- Retention / legacy migration / harness log pruning are **maintenance**, not first-paint prerequisites. They may run
  after first paint or in background.
- The palette probe must not serialize before first paint when the terminal is slow to answer (timeout reduced to 50 ms;
  never awaited before TUI spawn). Termios race with crossterm must still be avoided via `note_crossterm_raw_mode`.

### Paint-path split (this amendment)

1. **Before first frame (hard minimum)**
   - CLI + dotenv (without legacy migration)
   - Config load + provider client
   - Lightweight `ToolRegistry` + one primary-agent discovery
   - Resume history + cheap bootstrap
   - Seed system prompt + execution-context shells
   - Minimal harness/event emitter **without** retention sweeps
   - TUI spawn / first paint
2. **After first frame, before first model turn (hydration)**
   - Existing deferred hydration (unchanged)
   - Harness retention (`apply_retention_preserving`) + legacy harness-log prune
   - Legacy path migration (or fire-and-forget after paint)
   - Palette probe result application (theme can re-drive after probe)
3. **Background / async allowed**
   - Update preflight (already spawned)
   - Temp-spool cleanup (already spawned)
   - iTerm2 icon ensure (move off the inline pre-TUI path if it still blocks)

### Out of scope

- Feature-gating heavy crates out of the default binary.
- Release profile / LTO / allocator changes.
- Provider network latency and first-run onboarding UX redesign.
- Changing `ThreadEvent` or tool safety policy semantics.
- Binary size / dyld work.

## Tasks

- [x] T1: Record pre-change warm first-frame baselines (isolated + large-store) with `VTCODE_STARTUP_TRACE` —
      acceptance: phase timings saved under `.vtcode/perf/` (covers: S1)
- [x] T2: Move legacy path migration off the interactive first-paint path — acceptance: `dotenv_and_migration` no longer
      includes multi-hundred-ms `migrate_legacy` work on interactive launch; migration still runs and stays idempotent
      (covers: S1, S2)
- [x] T3: Stop blocking session setup on the 200 ms palette probe — acceptance: first-frame no longer waits the probe
      timeout when the terminal is silent; theme/palette still correct after probe completes; no termios race (covers:
      S1, S2)
- [x] T4: Move harness retention + legacy log prune after first paint — acceptance: `initialize_harness` before
      `initialize_session_ui` does not walk/prune the session store; retention still applies before the first model turn
      (covers: S1, S2)
- [x] T5: Slim remaining `session_setup_critical` if T2–T4 leave first-frame above target — acceptance: warm isolated
      first-frame ≤ 150 ms, **or documented residual with evidence** (covers: S2; depends: T2, T3, T4) — residual: warm
      first-frame ~360–410 ms, dominated by `session_setup_critical` ~175–220 ms (ToolRegistry + plugin-aware discovery
      still required before paint per S2) + UI spawn; 150 ms target not met without a registry-light paint path
- [x] T6: Update `docs/development/performance.md` (and binary gotchas if needed) for the new paint-path rules —
      acceptance: docs describe migration/retention/palette deferral (covers: S2; depends: T2, T3, T4)
- [x] T7: Verify + re-measure — acceptance: `./scripts/check-dev.sh` and targeted nextest PASS; before/after first-frame
      recorded in Report (covers: S2; depends: T2, T3, T4, T5, T6)
