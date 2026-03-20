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

Use this loop for any non-trivial performance change. Change one thing at a time so the comparison stays attributable.

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
cargo bench -p vtcode-tools --bench cache_bench
```

Use benches when a hotspot is stable and repeatable. Use the baseline/profile scripts when the question is broader end-to-end behavior.

## Optimization Rules

- Change one thing at a time.
- Keep changes surgical and behavior-preserving.
- Prefer simple, safe single-pass reductions over broad refactors.
- Revisit data structures before introducing algorithmic sophistication.
- Keep the simplest implementation until measured workload data proves it insufficient.
- For hashers, follow the selective policy in `performance-hasher-policy.md`.
