# Local Performance Scripts

These scripts provide a repeatable local performance workflow for VT Code.

## Commands

```bash
# Capture metrics + raw logs
./scripts/perf/baseline.sh baseline
./scripts/perf/baseline.sh latest

# Compare two captured runs
./scripts/perf/compare.sh \
  .vtcode/perf/baseline.json \
  .vtcode/perf/latest.json

# Build release binary for profiling (line tables + frame pointers)
./scripts/perf/profile.sh

# Local-only host-tuned build/run
./scripts/perf/native-build.sh
./scripts/perf/native-run.sh -- --version

# Experimental local PGO build (Python 3.11+ and matching LLVM tools)
rustup component add llvm-tools-preview
python3 scripts/perf/pgo.py -- python3 "$PWD/scripts/perf/pgo_train_startup.py"
```

The baseline builds and measures `target/release/vtcode`. It captures release binary size, cold launch from fresh `/tmp`
copies, warm `--version`, a credential-free `tool-policy status`, and interactive first-render latency through a PTY
that answers terminal capability queries. It also retains the `vtcode-core` pipeline and harness benchmarks for local
comparison; none of these measurements is a CI performance gate.

Criterion workloads are skipped by default so launch measurements do not wait for a full benchmark-profile rebuild. Set
`PERF_RUN_BENCHMARKS=1` to include them in the JSON capture.

## Responsiveness replay

Build both applications with the same release profile and keep the matching
`vtcode` executable beside each Desktop build.

```bash
cargo build --locked --release -p vtcode -p vtcode-desktop
python3 scripts/perf/responsiveness.py --rounds 3 /path/to/baseline/vtcode /path/to/candidate/vtcode
python3 scripts/perf/responsiveness.py --restored --rounds 3 target/release/vtcode
python3 scripts/perf/responsiveness.py --fixed-size --rounds 3 target/release/vtcode
python3 scripts/perf/responsiveness.py --command-output target/release/vtcode
python3 scripts/perf/responsiveness.py --workspace-search target/release/vtcode
python3 scripts/perf/responsiveness.py --native desktop target/release/vtcode-desktop
python3 scripts/perf/responsiveness.py --native terminal target/release/vtcode
```

The automated CLI driver submits one prompt to a loopback fixture, injects
typing, navigation, scroll and resize input during the assistant stream, and
reports PTY bytes for idle, streaming and recovery plus the existing frame
sampler summaries. Standard output is JSONL for the automated runs. The sampler's slow
counters and maxima are cumulative within a run. Compare deltas and distributions across
alternating runs. The child CPU total includes startup and cleanup. PTY bytes reflect
application output, and visible feedback requires a native terminal capture. The fixture
disables TUI copy-on-select to keep automated selection from modifying the host clipboard.
On macOS, the automated run also samples CPU time, current and peak physical footprint,
and cumulative disk bytes at its phase boundaries. Native Desktop logs the same counters
once per second for the app and its immediate children, including the backend.

`--restored` creates a session through the normal CLI, verifies its completed turns on disk, and resumes
it before the measured run. The CLI fixture uses one 2,000-line response. Desktop uses 32 responses of
40 lines each to stay below its per-message history preview limit.
`--fixed-size` removes width changes and `--keyboard-only` removes mouse events. `--command-output`
runs a local script using Bash mode in the CLI and the embedded terminal in Desktop.
`--workspace-search` creates 2,048 small files and types a query during the automated
replay. Open the file picker and type `fixture-files/unit-0` for the native search workload.

To isolate transcript work from terminal output, run the existing transcript benchmark
under the same release profile and record the width-change, scroll, selection and
streaming cases separately.

```bash
cargo bench --locked --profile release -p vtcode-ui --bench transcript -- 'transcript_(reflow|interaction)'
```

For a native capture, submit `Replay the responsiveness fixture.` in each app.
Open the printed workspace first in Desktop, or use `--restored` to launch it
with the seeded session selected. With `--command-output`, open the Desktop terminal
with Command+J or enter the printed command in the CLI. Record the display refresh rate, terminal and OS,
release revision, and the sequence of actions. Repeat transcript scrolling,
selection, typing and resize during the stream. Include click and keyboard
actions, terminal scrolling, and zoom in, out and reset in Desktop. Use the same
idle and recovery windows and record native input-to-visible latency and missed
refresh intervals separately. Instruments System Trace can attribute main-thread
work, and Activity Monitor and File Activity can measure the app and its
backend child. Capture allocations or Metal activity separately to avoid
combining profiler overhead with the comparison.

## Outputs

All artifacts are written to `.vtcode/perf/`:

- `baseline.json` / `latest.json`: captured metrics
- `*-cargo_check.log`: cargo check output
- `*-release_build.log` / `*-release_build.time`: explicit release rebuild output and command timing
- `*-bench_tool_pipeline.log`: `vtcode-core` tool-pipeline bench output
- `*-bench_agent_harness.log`: `vtcode-core` interactive harness and optimization bench output
- `*-cold_startup.json`, `*-warm_startup.json`, `*-first_user_io.json`: raw release launch samples
- `*-interactive_first_render.json`: raw PTY first-frame samples
- `*-interactive_first_render.log`: captured terminal bytes for each PTY sample
- `diff.md`: markdown comparison report

## Notes

- Cargo steps clear `RUSTC_WRAPPER` and `CARGO_BUILD_RUSTC_WRAPPER` by default so the scripts still work when the
  environment or `.cargo/config.toml` points at a blocked `sccache`.
- Set `PERF_KEEP_RUSTC_WRAPPER=1` if you explicitly want the perf run to keep the configured wrapper.
- `startup_ms` is retained as an alias for `warm_startup_ms` for compatibility with older reports.
- The release executable is rebuilt on every capture. Build/check failures abort capture instead of measuring a stale
  executable. `release_build_ms`, `cargo_check_ms`, and `*_bench_ms` are command wall times; Criterion operation estimates
  are in the benchmark logs. See [workspace audit methodology](../../docs/development/performance.md#workspace-audit-and-release-matched-comparisons).
- `cold_startup_ms` measures three launches of fresh copies in `/tmp`; it is a fresh-copy loader/process signal, not a
  page-cache eviction benchmark.
- `interactive_first_render_ms` ends when the PTY sees the initial VT Code header or `Type a request` prompt and
  terminates the isolated sample. Cleanup closes the PTY before reaping the child, including when output is pending.
- Startup routes share a credential-free environment with temporary workspace, `HOME`, XDG, Codex, and config paths.
  Provider credentials are excluded and the Ollama endpoint is a closed loopback port. The comparison warns when the
  environment differs from an older capture.

## PGO experiments

`pgo.py` builds a release baseline, instruments the same target, runs an explicit trainer, merges its profiles with the
active Rust toolchain's `llvm-profdata`, and builds a candidate. All builds use `--locked`, an explicit host target, the
same base flags, and one codegen unit. Explicit targeting keeps PGO flags out of build scripts and proc macros. Each
stage has a separate target directory and log; failures stop the experiment without a success summary. Existing output
directories are refused, so old profiles and normal release artifacts are preserved.

The trainer receives the instrumented binary as its final argument. It runs in an isolated workspace with the same
credential-free environment as startup measurements, plus `LLVM_PROFILE_FILE`. Use absolute paths for trainer script
arguments and fixtures. `pgo_train_startup.py` covers `--version`, `--help`, tool schema export, and policy status only;
it is a pipeline smoke test, not representative agent-loop training. Supply a deterministic trainer for the workload
you intend to optimize:

```bash
python3 scripts/perf/pgo.py --output .vtcode/perf/pgo-custom -- /absolute/path/to/trainer
```

`summary.json` records the Rust version, revision and dirty status, base flags, binary sizes and hashes, and separate
build/training times. `CARGO_ENCODED_RUSTFLAGS` takes precedence over `RUSTFLAGS`; otherwise the script retains the
repository's host-target flags (including its linker settings). Export custom flags from external Cargo configurations
explicitly through either variable. Existing PGO or codegen-unit flags are rejected. No `target-cpu=native` is added.
Keep source files and build configuration unchanged throughout an experiment.
Review missing-function diagnostics in `use.log`; startup smoke profiles do not establish complete profile coverage.

Compare the **baseline** and **use** binaries from the summary in three paired runs with identical fixtures. Measure
cold/warm launch, interactive latency, binary size, and the relevant CPU workload; assess build cost separately. The
instrumented binary and training duration are not runtime benchmarks. PGO remains local and opt-in until gains exceed
noise without regressions; CI and distribution builds do not consume these profiles. See the
[PGO guide](../../docs/development/performance.md#local-pgo-experiments).
