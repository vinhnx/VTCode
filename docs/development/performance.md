# Performance Optimization

VT Code uses a local-first performance workflow. Performance checks are measured manually and are not hard CI gates. The default stance is simple: do not guess, measure first, and only keep complexity that pays for itself.

## Goals

- Keep release artifacts portable.
- Improve runtime without hurting day-to-day iteration speed.
- Optimize only measured hotspots.

## Performance & Simplicity Rules

- Do not guess where time goes. Capture a baseline before changing code that claims a performance win.
- Measure before tuning. Keep before/after numbers from `baseline.sh`, targeted timers, or benchmarks.
- Prefer simple algorithms when input sizes are small or not yet proven large.
- Avoid fancy algorithms and broad refactors unless measurements justify their constant-factor and maintenance cost.
- Start with data structures and layout. In VT Code, the right cache shape, queue boundary, or representation usually matters more than clever control flow.

These rules apply to product code and refactors alike. The burden of proof is on the optimization, not on the simpler baseline.

## Local Workflow

```bash
# 1) Capture baseline
./scripts/perf/baseline.sh baseline

# 2) Make a targeted change

# 3) Capture latest
./scripts/perf/baseline.sh latest

# 4) Compare results
./scripts/perf/compare.sh
```

Artifacts are written to `.vtcode/perf/` and include JSON metrics plus raw logs.

The perf harness clears `RUSTC_WRAPPER` and `CARGO_BUILD_RUSTC_WRAPPER` by default for its cargo steps so local measurements still work when `sccache` is configured but unavailable. Set `PERF_KEEP_RUSTC_WRAPPER=1` only when you explicitly want to keep the wrapper. `startup_ms` measures the built `target/debug/vtcode` binary rather than `cargo run`, which keeps compile time out of the startup number.

Use this loop for any non-trivial performance change. Change one thing at a time so the comparison stays attributable.

## Startup budget

`vtcode`'s startup-critical work lives in `StartupContext::from_cli_args`
(`src/startup/mod.rs`). The perf harness captures two distinct startup metrics:

- **`startup_ms`** — `vtcode --version`. This is **clap-only**: the `--version`
  flag short-circuits during argument parsing and `from_cli_args` never runs.
  Use it as a stable signal for binary/loader cost, **not** as a proxy for
  startup optimization work.
- **`first_user_io_ms`** — `vtcode auth openai`. This actually exercises
  `from_cli_args` (config load, dotfolder init, guardian init, theme
  resolution, auth resolution) without any network round-trip, so it is the
  right metric for startup-critical changes. `baseline.sh` measures both via 8
  warm runs and diffs them in `compare.sh`.

### Patterns that pay off on the startup path

- **Join independent disk I/O.** `initialize_dot_folder`, `init_global_guardian`,
  `determine_theme`, and `resolve_runtime_provider_auth` only depend on config
  that is already resolved; run them through `tokio::join!` so their disk reads
  overlap instead of running serially.
- **Gate inits behind `command_skips_provider_auth`.** Commands that never run
  tools (Login, Logout, Auth, ToolPolicy, AppServer, Notify, Pods, Schedule)
  do not need the guardian, file/command caches, gatekeeper, session-archive,
  or perf-telemetry init — skip them entirely.
- **Keep file reads bounded.** The dotfile audit log (`audit.rs::read_last_hash`)
  is append-only and grows unbounded; read only the tail window so startup cost
  stays `O(window)`, not `O(file size)`.
- **Defer non-critical background work.** Temp-spool cleanup
  (`cleanup_old_temp_spools`) runs in `spawn_blocking` so a cold `~/.vtcode/tmp`
  never blocks first user I/O.

### Cold vs warm — what actually costs time

Warm startup (binary already in the OS page cache) is **effectively free**:

| metric | release (62 MB) | debug (176 MB) |
|---|---|---|
| `vtcode --version`, warm | < 1 ms | ~5–8 ms |
| `vtcode auth openai`, warm | < 1 ms | ~10–20 ms |

The **only** meaningful launch cost is **cold binary page-in** (first run after
the page cache evicts the binary). Measured release cold-start of
`vtcode --version` ≈ **1.2 s** for the 62 MB release binary; the 176 MB debug
binary is proportionally ~3 s. Every run after that is sub-millisecond because
the binary stays resident in the page cache.

This matters most when `vtcode` is spawned as a **subprocess** (sub-agent
dispatch, background agents): each fresh process pays cold page-in until the
cache warms.

### Remaining lever: binary size, not `from_cli_args`

The `[profile.release]` is already maxed for load speed — `lto = true`,
`strip = true`, `panic = "abort"`, `codegen-units = 1`, `opt-level = 3`. There
is no further safe profile knob. `from_cli_args` is also already parallelized
and gated. So the launch-time lever that moves the cold range is **reducing the
binary's on-disk size**, which shrinks page-in time linearly.

The default binary links heavy subsystems that most invocations never use:

- `vtcode-eval` — eval framework (only `vtcode eval` commands).
- `vtcode-acp` — Agent Client Protocol (only `vtcode acp`).
- transitively via `vtcode-core`: `vtcode-indexer`, `vtcode-mcp`, `vtcode-a2a`,
  `vtcode-skills`.

These are the binary-size lever. Cutting them requires **feature-gating them out
of the default binary** (and behind an opt-in feature for the commands that need
them). That is a product decision — dropping them from `default` makes those
subcommands unavailable unless the binary is built with the feature — so it is
intentionally **not** done silently. Measure cold-start impact with:

```bash
# cold (first run after cache eviction) vs warm
/usr/bin/time -p target/release/vtcode --version   # repeat; first = cold
```

## Profiling Build

Use this when collecting profiler traces:

```bash
./scripts/perf/profile.sh
```

This builds release with:

- `-C force-frame-pointers=yes`
- `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`

Then profile `target/release/vtcode` with your preferred tool.

## Local Native Tuning

For local experiments only:

```bash
./scripts/perf/native-build.sh
./scripts/perf/native-run.sh -- --version
```

These scripts append `-C target-cpu=native` for local runs only. They do not change portable release defaults.

## Benchmarks

Current Criterion benches:

```bash
cargo bench -p vtcode-core --bench tool_pipeline
```

Use benches when a hotspot is stable and repeatable. Use the baseline/profile scripts when the question is broader end-to-end behavior.

## Optimization Rules

- Change one thing at a time.
- Keep changes surgical and behavior-preserving.
- Prefer simple, safe single-pass reductions over broad refactors.
- Revisit data structures before introducing algorithmic sophistication.
- Keep the simplest implementation until measured workload data proves it insufficient.
- For hashers, follow the selective policy in `performance-hasher-policy.md`.
