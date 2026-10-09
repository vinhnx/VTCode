# Performance Optimization

VT Code uses a local-first performance workflow. Performance checks are measured manually and are not hard CI gates. The
default stance is simple: do not guess, measure first, and only keep complexity that pays for itself.

## Goals

- Keep release artifacts portable.
- Improve runtime without hurting day-to-day iteration speed.
- Optimize only measured hotspots.

## Performance & Simplicity Rules

- Do not guess where time goes. Capture a baseline before changing code that claims a performance win.
- Measure before tuning. Keep before/after numbers from `baseline.sh`, targeted timers, or benchmarks.
- Prefer simple algorithms when input sizes are small or not yet proven large.
- Avoid fancy algorithms and broad refactors unless measurements justify their constant-factor and maintenance cost.
- Start with data structures and layout. In VT Code, the right cache shape, queue boundary, or representation usually
  matters more than clever control flow.

These rules apply to product code and refactors alike. The burden of proof is on the optimization, not on the simpler
baseline.

### Filter before expensive projection or normalization

Establish relevance before performing expensive projection or normalization work. In practice, filter a catalog,
request, or candidate set before serializing schemas, constructing derived views, or normalizing data that will not be
used. This keeps common paths from paying for work that only matters after a policy or relevance check succeeds.

The default documentation gate follows the same principle: `cargo doc --workspace --no-deps` builds the public API
documentation and intentionally excludes private items. Private-item documentation expands the work to internal
implementation details that are not part of the contributor-facing API artifact. Maintainers who need internal API
inspection can opt in with `cargo doc --workspace --no-deps --document-private-items`.

### Bounded I/O on the agent hot path

Prompt checkpoint preparation and dirty-worktree inspection run concurrently on the blocking pool. Hot checkpoint
retention skips live JSON and content garbage collection when there are no retired records; full maintenance retains
orphan collection. See [response progress and latency](response-progress.md) for phase feedback, monotonic diagnostics,
and the distinction between provider activity and visible model text. Preparation remains outside execution-budget
clocks while end-to-end latency observations include it.

Independent code-search backends (literal, declaration, and path search) are started together with `tokio::join!`;
filesystem reads, tree-sitter parsing, and candidate aggregation run in one `spawn_blocking` task. Keep the async
coordinator responsible for ordering and cancellation, not synchronous disk work. For synchronous side-channel APIs such
as progress monitoring, use a bounded coalescing writer so callers replace stale snapshots and never wait on filesystem
latency.

### Agent-loop hot-path invariants

Tool-result cache size is measured from the payload bytes, not the `String` container. Replacing an existing key updates
the byte total in place, so a full cache does not evict an unrelated entry during replacement; zero-capacity caches
reject inserts. Keep these accounting rules intact when changing cache entry representations.

Clean request histories are borrowed and shared with continuation state through `Arc<Vec<Message>>`. Copy only when
persisted editor/few-shot context must be shaped for the route or the provider requires compaction. This keeps the
common no-injection path from allocating multiple equivalent histories while preserving the existing normalization and
continuation boundaries.

Request-history analysis borrows call identifiers from the source messages. A repair clones only the messages retained
in the provider-facing view and creates placeholders only for missing results; it never clones the whole shared source
before rebuilding. Preserve causal matching, duplicate-result rejection, result order, and clean-history Arc reuse.

Request envelopes retain the source tool-catalog `Arc` within a request segment. When model/provider/mode/prompt
identity is unchanged, subsequent turns reuse the frozen ordered catalog without cloning, sorting, or re-hashing its
schema; segment boundaries clear that marker before rebuilding.

Read-only tool calls are batched only after per-call preflight confirms that each call is parallel-safe; duplicate names
are not a safety signal. Batch line ranges still pass through the absolute read cap, so new range-reading paths must
preserve that limit. Unified and runner dispatch both apply `agent.harness.max_parallel_tool_calls`; zero is the
explicit unlimited value. Mutating or otherwise non-parallel calls remain ordered.

Legacy text reads use the same bounded line reader as paged reads. This keeps large files and minified one-line bundles
from creating an unbounded temporary buffer; invalid UTF-8 remains lossily decoded for compatibility. A physical line
that exceeds the bound is reported as `line_truncated` so the agent can switch to byte ranges or targeted inspection
instead of treating the preview as complete. Live command-output spools are never result-cached, and directory-scoped
read caches invalidate on descendant edits. A command-cache miss also invalidates filesystem-derived results before
shell/PTY execution, while completed read-only command cache hits remain reusable.

### Serialization and event-log replay

Avoid combining `#[serde(flatten)]` with `#[serde(untagged)]` on frequent, discriminator-driven protocol payloads. Serde
must buffer the surrounding map to decide which flattened shape applies; direct wire structs can decode the known fields
once and construct the tagged payload afterward. VT Code uses this for OpenResponses and ACP streaming notifications.
Keep flattening when it is the actual contract, such as trace metadata's vendor-extension map.

Bounded A2A task queries clone only the requested history suffix and included artifacts while holding the existing
read lock. Keep status, identifiers, metadata, discriminator, suffix order, and full stored tasks unchanged; permission
checks and pagination still belong to the existing server and task-manager paths.

Keep streaming payloads as borrowed SSE text until a consumer needs an owned payload. The normalized Responses adapter
now avoids the old `Value -> JSON -> Value` round trip; common text, reasoning, tool, and lifecycle events use typed
decoding, with full payload materialization reserved for completion and compatibility fallbacks.

Session-log index rebuilds have an even narrower requirement: they need the versioned envelope and `event.type`, not the
full event payload. The rebuild path therefore skips nested payload materialization, while turn reconstruction continues
to use the canonical `VersionedThreadEvent` decoder.

### Privacy-preserving harness trace analysis

Use `vtcode_eval::analyze_jsonl_file` or `analyze_jsonl_reader` for offline DeepSeek/VT Code harness analysis. The file
and reader paths process one JSONL record at a time (with a 1 MiB record limit), retain only aggregate counters, and
never copy prompts, arguments, paths, file contents, output text, or free-form error messages into the returned summary.
Tool names and error values are mapped to bounded known labels; unknown values are grouped under `other_tool` or
`error`.

The analyzer treats `ThreadCompleted` usage as a fallback when no per-turn usage exists, so a normal thread trace does
not double-count its aggregate. Latency count, total, and maximum cover the complete trace; percentile queries use a
bounded 4,096-sample reservoir to keep memory usage stable for large sessions. Use the summary to compare tool
repetition, error categories, token cache usage, and output volume before changing the runtime.

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

### Workspace audit and release-matched comparisons

Record the revision and dirty diff, host CPU/RAM/OS, Rust toolchain, features, allocator, compiler flags, fixture sizes,
and cache state alongside each result. Run builds, tests, benchmarks, and profilers sequentially. Keep build wall time
separate from test execution and operation latency: a timed `cargo bench` command includes compilation, fixture setup,
warm-up, and analysis. Its wall time is not the latency of a tool call.

The baseline script always invokes `cargo build --release --locked --bin vtcode`, even when the executable already
exists, and stops on build/check failures. It records `release_build_ms` separately. The optional `*_bench_ms` fields
remain command wall times for compatibility; use the estimates and raw samples in Criterion's output for runtime
comparisons.

Startup captures use the shared `scripts/perf/startup_env.py` helper to exclude inherited credentials and isolate
workspace, config, XDG and Codex paths. Their `startup_environment` tag lets comparisons flag historical captures with
different isolation. Interactive first-frame samples use a closed loopback Ollama endpoint and stop at the initial
VT Code header or request prompt, without submitting a provider request. Raw terminal bytes are retained; the PTY
is closed before the owned child is reaped so pending output cannot stall cleanup on macOS.

The normal bench profile uses `opt-level = 3`; shipping release uses `"z"`. For release-matched library measurements:

```bash
export CARGO_PROFILE_BENCH_OPT_LEVEL=z
export CARGO_PROFILE_BENCH_DEBUG_ASSERTIONS=true
export CARGO_PROFILE_BENCH_OVERFLOW_CHECKS=true
export CARGO_PROFILE_BENCH_DEBUG=1
export CARGO_PROFILE_BENCH_STRIP=false

cargo bench --locked -p vtcode-core --features a2a-server --bench runtime_paths --no-run
cargo bench --locked -p vtcode-core --features a2a-server --bench runtime_paths -- \
  --sample-size 20 --warm-up-time 0.5 --measurement-time 1 --noplot
```

These overrides retain full LTO, one codegen unit, portable target flags, and release assertions. Document the remaining
differences: benchmark harnesses unwind, symbols are retained, library benchmarks use the system allocator, and this
target enables the optional A2A server. The root executable uses mimalloc by default and disables `vtcode-core` default
features; core's normal benchmark build enables `tui`. Compare identical feature sets, and do not treat a library/PTY
measurement as a measurement of the default executable's alternate backend. `allocator_throughput` explicitly uses
the binary's allocator selection.

`runtime_paths` adds deterministic offline coverage through existing entrypoints. It uses temporary workspaces and
loopback/stdio peers; it requires Python 3 for the synthetic MCP peer and does not call a live provider.

| Group                                                  | Workload and measurement boundary                                                                                                                                                                                                          |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `request_history`                                      | Shared clean histories and request-only missing-result repair, 8/128/2,048 turns                                                                                                                                                           |
| `transcript_interaction` (existing `transcript` bench) | Full idle/streaming frames, mouse scrolling, and selection through app events                                                                                                                                                              |
| `admitted_tool_dispatch`                               | Warm internal registry/cache calls for eight fixture file reads, sequential/joined futures/spawned tasks                                                                                                                                   |
| `stream_framing_helper`                                | Exact private SSE/UTF-8 helper source, LF/CRLF bursts and 17-byte fragments; helper attribution only                                                                                                                                       |
| `responses_provider_loopback`                          | Public normalized provider stream consuming 8/256/4,096 recorded text events and completion                                                                                                                                                |
| `responses_boundaries`                                 | Explicit Responses profile through the existing custom-provider router: seven-byte reasoning/tool chunks, GPT summary suppression, malformed/incomplete input and recovery                                                                 |
| `workspace_search`                                     | Live bounded no-follow walker, 8/256/4,096 files, wide/deep trees up to 63 levels, cancellation and visible file mutation                                                                                                                  |
| `file_listing`                                         | Public basic-list tool, 8/256/4,096 mixed file/directory entries with dotfiles and selective globs, directory-cache misses/hits, listing latency and concurrent timer wake delay on current-thread and normal multithreaded Tokio runtimes |
| `session_store`                                        | Canonical event append/flush, reopen, snapshot replay, and invalidated-index rebuild; durability included                                                                                                                                  |
| `session_retention`                                    | Completed nonempty sessions, count-based eviction while preserving one session; durable setup excluded                                                                                                                                     |
| `runner_output`                                        | Pipe bursts, slow consumers, and bounded PTY previews from 1 KiB to 1 MiB fixtures                                                                                                                                                         |
| `runner_cancellation`                                  | Spawn, terminate and reap an owned child, including closed output and exit notification                                                                                                                                                    |
| `mcp_stdio`                                            | Real client handshake, cached discovery/search, and tool requests to a synthetic stdio peer                                                                                                                                                |
| `mcp_boundaries`                                       | Five-millisecond peer delay, 32 outstanding requests, cancelled-call recovery and disconnect notification                                                                                                                                  |
| `a2a_loopback`                                         | Authenticated discovery and bounded-history reads from 8/64/512-message tasks                                                                                                                                                              |
| `a2a_boundaries`                                       | Five-millisecond authenticated request delay and cancelled-call recovery                                                                                                                                                                   |

The helper benches compile the owning private source files directly to avoid widening public APIs. Pair these with the
provider entrypoint measurement before claiming an end-to-end streaming improvement. The older `tool_pipeline`
outcome-clone benches are simulations, not production pipeline measurements. Criterion sample distributions describe
batch-average operation times, not individual request tail latencies.

Set `VTCODE_BENCH_SCOPE` to `harness`, `streaming`, `indexing`, `file_listing`, `memory`, `runner`, or `protocols` to
construct only that subsystem's fixtures. Criterion name filters select timing cases but still run other fixture setup
by default; that setup can dominate whole-process profiles, especially durable memory logs. Tree creation warms
filesystem metadata;
the wide/deep walker cases do not claim a cold OS cache. The older persistent-index benchmark measures a library API
that currently has only benchmark/test callers. Confirm shipping callers before ranking it as a production hotspot.

Capture three paired baseline/candidate runs with identical settings, fixtures, warm-up, and sample counts. Retain
explicitly built baseline executables with hashes, rebuild the release executable before comparisons, and run control
samples to estimate host noise. Keep a change only when its gain exceeds that noise and another required workload has
no repeatable regression beyond noise. Archive Criterion samples and all failures under `.vtcode/perf/`; performance
results remain informational, without machine-dependent CI thresholds.

Keep the benchmark source identical in both builds as well as the production fixtures. Adding cases can change LTO
layout even for unchanged code. When coverage expands during an audit, rebuild the baseline production revision with
the expanded harness before attributing a new regression to the candidate. Preserve both comparisons and their source
and executable hashes.

For directory-list responsiveness, run the retained `runtime_paths` executable in an isolated process:

```bash
VTCODE_BENCH_SCOPE=file_listing VTCODE_FILE_LISTING_SAMPLES=100 \
  target/release/deps/runtime_paths-<hash> --bench --noplot > listing.jsonl
```

This mode emits individual JSON measurements and idle timer controls, rather than Criterion batch averages.
Without `VTCODE_FILE_LISTING_SAMPLES`, the scope exposes Criterion `latency` and `wake_delay` cases.
Fixture setup, cache clearing, priming, and timer arming are excluded from listing latency; normal tool validation,
filtering, formatting, pagination, and cache publication remain included. Each call records its maximum delay past
a one-millisecond timer deadline and its tick count. Short calls still collect one tick, which may occur after the
listing completes, so process wall time exceeds summed listing latency. Tokio timer granularity and OS scheduling
contribute to this proxy; it is not a direct poll-time
or scheduler-latency metric. Compare idle controls and cache hits, retain noisy runs, and report both runtime types.
Metadata is warm, the global ignore matcher has its normal empty default, and no cold-disk, loaded-ignore, live-provider,
or whole-agent latency claim follows from these workloads.

### Directory listing experiment (2026-10-07)

A private basic-list candidate moved the full directory scan into one blocking task per cache miss, following
[Tokio filesystem batching guidance](https://dial9-rs.github.io/blog/principles-for-fast-tokio-applications/)
and [uv's blocking extraction change](https://github.com/astral-sh/uv/pull/21372). Path validation, metadata lookup,
cache lookup, pagination, and cache publication stayed on their existing paths; formatting and ignore helpers were
unchanged. The candidate was **rejected and removed** under the recorded acceptance rules.

On an Apple Silicon macOS host, both retained binaries used the identical frozen harness and the release-matched
profile above. Three pairs alternated baseline/candidate, candidate/baseline, baseline/candidate. Each binary run
collected 100 individual observations for each of 24 listing cases plus two idle controls: 15,600 observations total.
Builds and checks completed before timing. No instrumented profiler ran alongside these measurements.

For 4,096-entry cache misses, current-thread p95 maximum timer wake delay improved in every pair: visible listings
fell from 2.224–2.399 ms to 1.412–1.435 ms; selective listings fell from 2.219–2.272 ms to 1.422–1.436 ms.
These paired reductions were approximately 35–40%, beyond deterministic 95% bootstrap intervals with 5,000 resamples.
Normal multithreaded wake delays stayed near the timer floor and failed to improve beyond those intervals in five
of six large-scan comparisons. Large-scan mean listing latency changed by approximately -3.5% to +1.3%.

The latency guard also failed: the current-thread 4,096-entry visible cache-hit case became 17.0%, 17.3%, and 36.2%
slower across the three pairs. Baseline/candidate means were 187/219, 183/215, and 144/196 microseconds; even the lower
95% ratio bounds exceeded a 5% regression. This is an observation about these artifacts, not proof that the blocking
scan itself caused the cache-hit cost. Other cache controls and idle samples also varied, particularly in pair three;
all measurements were retained rather than selecting favorable cases. The evidence did not meet the complete gate.

Raw observations, both executables, source snapshots and hashes, profile settings, bootstrap analysis, failed checks,
and the excluded compilation-overlapping smoke run remain in the local, gitignored
`.vtcode/perf/2026-10-07-file-listing/` directory (`provenance.json`, `acceptance.json`, `runs.json`, `analysis.json`,
and `results.md`). The benchmark and behavior regressions remain available for future measurements; no production
directory-listing optimization was retained. These local warm-metadata results do not establish cross-platform or
whole-agent performance.

The `vtcode-ui` `markdown_render` bench exercises the public renderer with short responses, mixed content, nested
lists, fenced code, Unicode prose, and plan wrappers containing multiline inline code. Build it with the same profile
overrides above, then retain the executable printed by Cargo and verify its cases with `--list` before comparing:

```bash
cargo bench --locked -p vtcode-ui --bench markdown_render --no-run
cargo bench --locked -p vtcode-ui --bench markdown_render -- \
  --sample-size 30 --warm-up-time 0.5 --measurement-time 2 --noplot
```

Fixture construction happens outside the timed loop. Use the existing `transcript` bench as an interaction control;
Markdown renderer results do not measure the headless ANSI fallback or whole-session latency. Documents without `<`
can bypass plan cleanup while retaining line-ending normalization. Documents that may contain wrappers must still
update inline-code state across line boundaries. Preserve literal tags inside code, case-insensitive wrapper removal,
fence handling, and CRLF normalization when changing it.

For CPU attribution, use a symbolized release build and `samply record --save-only`; keep profiler runs separate from
production timing. Use the existing hotpath harness for allocation attribution and future polling, and platform
RSS/resource sampling for memory. Record unavailable counters explicitly: process RSS is not bytes allocated,
filesystem block-operation counts are not bytes read/written, and a fresh executable copy does not evict the OS cache.
Process-scoped filesystem tracing may require host privileges. Never weaken durability or sandbox/permission checks
to improve a number.

The perf harness builds and measures `target/release/vtcode`, not `cargo run` or the debug binary. It clears
`RUSTC_WRAPPER` and `CARGO_BUILD_RUSTC_WRAPPER` by default for its cargo steps so local measurements still work when
`sccache` is configured but unavailable. Set `PERF_KEEP_RUSTC_WRAPPER=1` only when you explicitly want to keep the
wrapper.

Use this loop for any non-trivial performance change. Change one thing at a time so the comparison stays attributable.

## TUI hotpath profiling

The `vtcode-ui` crate is instrumented with [hotpath](https://hotpath.rs/) for function timing and allocation
attribution. Profile the TUI hot paths **from this workspace** (the `profiling` feature lives on `vtcode-ui`):

```bash
# From the repo root of this branch (not a checkout without the feature):
cargo run -p vtcode-ui --features vtcode-ui/profiling --example tui_hotpath
```

If you see `the package 'vtcode-ui' does not contain this feature: profiling`, you are in a tree that predates the
feature — switch to the branch/worktree that has `profiling` in `crates/codegen/vtcode-ui/Cargo.toml`.

This prints a timing + alloc report on exit (reflow, wrap, render, input). Root `--features profiling` also forwards
`vtcode-ui/profiling` so the full binary runloop and TUI can be profiled together.

TUI invariants found via hotpath (keep these):

- Bottom-follow (`offset == 0`) appends never force a transcript reflow for scroll adjust; scrolled-up views still
  recompute to keep the view stable.
- Predecessor reflow is limited to Tool/Pty block edges and Info/Warning/Error group heads — plain Agent↔Agent
  streaming must not double-reflow.
- Link projection fast-rejects when `may_contain_link_candidate_text` is false (no path chars and no `.`); bare
  workspace filenames still match via `.`.
- Eviction uses `TranscriptReflowCache::evict_prefix`, never a full wipe.
- Single-style ASCII prose uses `wrap_ascii_word_boundaries` (no grapheme clustering / `clip_line`); other lines keep
  the full wrapper.
- Transcript lines are pre-wrapped to `content_width`; `TranscriptWidget` must not re-wrap in `Paragraph` every frame.
- Common transcript frames (no queue overlay, no links, no indicator shimmer) paint via `paint_pre_wrapped_lines`
  (`Buffer::set_span`) — do not clone `Line`s into `Paragraph`. Bottom padding is empty space, not a reason to clone/pad
  the line list.
- Header content paints via `Buffer::set_span`; the block title is cached (`header_block_title_cache`) and invalidated
  with `header_lines_cache`. Do not rebuild `Paragraph` + `Block::title(...)` every frame.
- `build_input_render` is fingerprint-cached (size/content_len/cursor/flags).

## TUI frame metrics

Steady-state TUI jank (frame drops, input lag under streaming tool/PTY load) is diagnosed with an opt-in sampler in
`vtcode-ui` (`tui/frame_metrics.rs`):

```bash
VTCODE_TUI_FRAME_METRICS=1 vtcode
```

When enabled, `render_if_dirty` records draw and input-to-draw durations into a 256-sample ring and logs a windowed
summary (p50/p95/max, slow counts, frames drawn/skipped) every 5s on the `vtcode.tui.latency` target. When the flag is
unset the sampler is a no-op.

Slow thresholds match the existing debug logs: draw ≥ 8ms, input-to-draw ≥ 16ms.

Related TUI invariants to preserve when optimizing the render path:

- Streaming appends use `mark_transcript_line_dirty` (not `mark_dirty`) so header and sidebar caches survive every
  chunk.
- Transcript eviction drops only the evicted prefix of the reflow cache (`TranscriptReflowCache::evict_prefix`); do not
  call full `invalidate_transcript_cache` on eviction.
- Hover and scroll use `mark_visual_dirty`; only content changes drop caches.
- Capture retention is bounded (`TUI_TOOL_OUTPUT_CAPTURE_MAX_LINES`, `TUI_TOOL_OUTPUT_BLOCKS_MAX`,
  `TUI_COMPACT_ACTIVITY_MAX_ENTRIES`).

## Standalone startup benchmark

Startup policy is defined by the command case and launch state, not by one generic `startup_ms` number. Measure the
release executable directly:

```bash
cargo build --release --locked --bin vtcode
VTCODE_BIN="$PWD/target/release/vtcode" \
  VTCODE_BENCH_RUNS=10 \
  cargo bench --locked --bench startup -- --noplot
```

The standalone case matrix is:

```text
vtcode --version
vtcode --help
vtcode schema tools --format ndjson --name code_search
```

Run every case as both a cold and warm sample. Cold means copying the executable to a new temporary path for each
launch, then timing the launch of that freshly copied executable; this approximates fresh executable loader/relocation
work. Warm means timing repeated launches of the same executable after warm-up. Cold does not mean flushing the
operating system's page cache: the harness never flushes or evicts OS page caches, so label and compare the result as a
fresh-copy cold proxy.

Every child receives an isolated temporary `HOME`, config root, data root, explicit config-file path, and workspace.
This prevents credentials, user configuration, persistent data, and repository contents from affecting the startup
result. The schema case is still useful because it exercises tool registry construction while remaining standalone and
provider-free.

For each case and launch mode, retain the raw millisecond samples and report the median and p95. The median is the
primary central result; p95 exposes startup tail behavior. Use the same binary, machine, environment, sample count, and
isolation layout for before/after comparisons. Keep `VTCODE_STARTUP_TRACE=0` (or unset) during timed runs; enable
`VTCODE_STARTUP_TRACE=1` only for a separate diagnostic run.

For a quick A/B check of two release binaries, `scripts/bench-startup.sh` uses [hyperfine](https://github.com/sharkdp/hyperfine)
when installed (`brew install hyperfine` or `cargo install --locked hyperfine`). It runs without an intermediate shell
(`-N`) after 3 warmup runs and reports mean, standard deviation, and outliers across `--version`, `--help`, and
`schema tools`. It isolates only `HOME` (not the config-file path, data root, or workspace), so use
`cargo bench --bench startup` for recorded results.

```bash
VTCODE_BIN="$PWD/target/release/vtcode" VTCODE_BASELINE_BIN=/path/to/previous/vtcode \
  VTCODE_BENCH_RUNS=30 VTCODE_BENCH_JSON=/tmp/vtcode-startup.json ./scripts/bench-startup.sh
```

Without hyperfine the script falls back to a plain timing loop; `VTCODE_BASELINE_BIN` requires hyperfine.

The broader capture remains available when its additional workloads are needed:

```bash
./scripts/perf/baseline.sh baseline
./scripts/perf/baseline.sh latest
./scripts/perf/compare.sh \
  .vtcode/perf/baseline.json .vtcode/perf/latest.json
```

It records separate cold fresh-copy, warm, first-user-I/O, and interactive first-render artifacts. Those metrics must
not be treated as substitutes for the three-case standalone matrix above.

For phase-level diagnostics, set the opt-in trace before launching the binary:

```bash
VTCODE_STARTUP_TRACE=1 target/release/vtcode --provider ollama --model llama3
```

The trace is silent when unset and reports only duration records for bootstrap, CLI parsing, runtime creation, config,
validation, authentication, session setup, and first UI render. It is initialized before tracing is configured so early
startup work is observable without adding work to normal launches.

### Patterns that pay off on the startup path

- **Join independent disk I/O.** `initialize_dot_folder`, `init_global_guardian`, `determine_theme`, and
  `resolve_runtime_provider_auth` only depend on config that is already resolved; run them through `tokio::join!` so
  their disk reads overlap instead of running serially.
- **Gate inits behind `command_skips_provider_auth`.** Commands that never run tools (Login, Logout, Auth, ToolPolicy,
  AppServer, Notify, Pods, Schedule) do not need the guardian, file/command caches, gatekeeper, session-archive, or
  perf-telemetry init — skip them entirely.
- **Keep file reads bounded.** The dotfile audit log (`audit.rs::read_last_hash`) is append-only and grows unbounded;
  read only the tail window so startup cost stays `O(window)`, not `O(file size)`.
- **Defer non-critical background work.** Temp-spool cleanup (`cleanup_old_temp_spools`) runs in `spawn_blocking` so a
  cold user-cache `large-output/` directory never blocks first user I/O.
- **Registry-light critical path.** `initialize_session_critical` no longer constructs `ToolRegistry` or runs
  `discover_controller_subagents`. Those run in `complete_session_registry` after first paint (trace phase
  `session_setup_registry`) and re-drive the ready UI before hydration. Ratchet:
  `critical_path_avoids_registry_and_discovery`.
- **Static-first typeable shell.** `initialize_session_shell` paints a typeable TUI (bootstrap placeholder + built-in
  slash commands) _before_ `initialize_session_critical` builds `ToolRegistry`, discovers subagents, or constructs the
  provider client. Keystrokes typed into the shell survive the ready re-drive (no respawn). Structural ratchet:
  `shell::tests::shell_module_avoids_heavy_init_before_paint`.
- **Interactive first frame uses a critical/hydrate split.** `initialize_session_critical` builds the heavy session
  runtime (provider client, primary-agent discovery, lightweight tool registry, resume history, cheap bootstrap)
  **after** the typeable shell is painted. `initialize_session_ui` spawns the session; `hydrate_session_runtime` then
  finishes tool-catalog projection, system-prompt composition, CGP wiring, subagent controller creation, trajectory,
  dynamic context, and MCP reconfigure before the interaction loop dispatches the first model turn. Opt-in phases:
  `session_setup_critical`, `session_setup_ui`, `session_setup_hydrate`, `session_setup`, `first_ui_render`.
- **Keep update/release I/O off first paint.** The critical path consults only the in-memory preflight notice;
  `Updater::new` + cache reads and release-notes reads run in hydration and merge header highlights there.
- **Maintenance never sits on the first-paint path.** Interactive legacy path migration (`VtCodePaths::migrate_legacy`)
  is fire-and-forget `spawn_blocking` scheduled from `run()` before dispatch; non-interactive consumers still migrate
  synchronously before dispatch. Harness session-store retention (`apply_retention_preserving`) and legacy harness-log
  pruning run in `run_harness_retention`, spawned only after `initialize_session_ui` returns (first paint is available),
  never inside `initialize_harness`. iTerm2 icon ensure is `spawn_blocking`.
- **Palette probe is awaited before TUI spawn (bounded, usually instant).** The OSC probe (50 ms timeout) is started in
  bootstrap and overlaps startup-context resolution. `initialize_session_shell` awaits it before
  `spawn_session_with_options` so late `OSC 10/11/4` replies cannot race the TUI event loop for `/dev/tty` bytes and
  leak as `10;rgb:...` input; `note_crossterm_raw_mode()` is still called before spawn so a late `RawModeGuard` restore
  cannot undo crossterm raw mode. `await_terminal_palette_probe()` after spawn settles theme before the first model
  turn. Stragglers from slow terminals are swallowed by the vendored crossterm `parse_osc` backstop.
- **Reuse the loaded session config.** `ToolRegistry::new_with_loaded_config` reuses the merged `VTCodeConfig` snapshot
  instead of a second `ConfigManager::load_from_workspace` parse; `ToolRegistry::new` remains for paths without a loaded
  config.
- **Join pre-paint builders.** `ToolRegistry` construction overlaps `discover_controller_subagents`, and prompt-template
  discovery overlaps the dot-config load, via `tokio::join!`.
- **Lazy `--version` diagnostics.** `build_augmented_cli_command` only formats `long_version()` (storage paths + env
  dump) when argv contains a version flag; interactive launches use the static crate version.
- **Reuse subagent discovery.** Critical path caches `DiscoveredSubagents` in `SessionState`; hydration builds via
  `SubagentController::new_with_discovered` instead of a second workspace/plugin scan.
- **Skeleton-first slash palette.** TUI spawns with built-ins only; workspace templates merge post-paint via app-level
  `InlineCommand::SetSlashCommands`.
- **Defer approval-pattern I/O.** Critical path builds the recorder via `ApprovalRecorder::new_deferred` (path
  resolution only, no `mkdir`, no pattern file reads); hydration ensures the cache dir and calls `reload()` before the
  first turn. Writes were already self-ensuring.
- **Defer policy file I/O.** First paint builds the registry via `new_for_first_paint_with_loaded_config` (no policy
  file read/create); hydration attaches it with `ensure_workspace_policy_manager` before any tool runs. Evaluation fails
  open to metadata defaults until then.

### Release artifact assumptions

The shipped `release` profile remains tuned for launch size and dead-code removal: `opt-level = "z"`, fat LTO,
`codegen-units = 1`, stripping, and an abort-on-panic runtime. Keep `codegen-units = 1` for fat-LTO builds:
uv#22303 showed higher values increase total LTO/IR work and peak RSS despite more backend parallelism.
macOS release scripts and `.cargo/config.toml` also apply
`-Wl,-dead_strip`. Verify the effective profile and the measured binary size before attributing a result to Rust startup
code; a debug binary is not a valid proxy for the shipped launch path.

Cold and warm results answer different questions. Warm results isolate process and loader overhead after the binary is
resident. Fresh-copy results expose the size and relocation cost paid by a newly spawned process, which is the relevant
signal for subprocess-heavy workflows. Interactive results additionally include configuration, authentication, terminal
initialization, and session setup through the first usable frame.

The default binary links heavy subsystems that most invocations never use:

- `vtcode-eval` — eval framework (only `vtcode eval` commands).
- `vtcode-acp` — Agent Client Protocol (only `vtcode acp`).
- transitively via `vtcode-core`: `vtcode-indexer`, `vtcode-mcp`, `vtcode-a2a`, `vtcode-skills`.

These are potential binary-size levers. Cutting them requires feature-gating them out of the default binary (and behind
an opt-in feature for the commands that need them). That is a product decision, so it is intentionally not done
silently. Measure cold-start impact with:

```bash
./scripts/perf/baseline.sh latest
```

## Profiling Build

Use this when collecting profiler traces:

```bash
./scripts/perf/profile.sh
```

This builds release with:

- `-C force-frame-pointers=yes`
- `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`
- `CARGO_PROFILE_RELEASE_STRIP=false` so the release profile does not remove those symbols

Then profile `target/release/vtcode` with your preferred tool.

## Local Native Tuning

For local experiments only:

```bash
./scripts/perf/native-build.sh
./scripts/perf/native-run.sh -- --version
```

These scripts append `-C target-cpu=native` for local runs only. They do not change portable release defaults.

## Benchmarks

Current benches (`criterion` except standalone `startup`):

```bash
cargo bench --bench allocator_throughput
cargo bench -p vtcode-core --bench tool_pipeline
cargo bench -p vtcode-core --bench agent_harness
cargo bench -p vtcode-ui --bench markdown_render
cargo bench -p vtcode-ui --bench transcript
```

Standalone process startup (needs a release binary) stays separate; see
[Standalone startup benchmark](#standalone-startup-benchmark).

Use benches when a hotspot is stable and repeatable. Use the baseline/profile scripts when the question is broader
end-to-end behavior.

### Benchmark discipline (benchmaxxing guardrails)

Follow this loop for any claimed speedup; it adapts iterative `criterion` benchmaxxing to an I/O-bound agent loop where
1.2-1.5x per converged pass is a strong result:

- Capture a True Performance Baseline first: run the relevant bench without library changes, sequentially, on a fixed
  machine/binary/env.
- Optimize library code only. Do not modify existing bench measurement logic to hit a goal; a speedup claim that edits
  timed code paths is invalid. Adding new coverage benches in a separate change is allowed.
- Run benches sequentially. Never run two benches in parallel; they compete for resources and invalidate results.
- Keep comparisons portable. Never use `RUSTFLAGS` or `-C target-cpu=native` for before/after numbers; native builds are
  local-only via `scripts/perf/native-*.sh`.
- Keep iterations independent. Use `iter_batched` with fresh setup per iteration so no cache built in one iteration
  leaks into the next, except explicit `cache_hit` benches where a shared warm cache is the point being measured.
  Filesystem setup stays outside the timed section.
- Use `criterion` directly with `black_box` on outputs. Do not invent custom timing harnesses.
- Cover small and large inputs. A win on one size only is not a win; report median + statistical significance from
  `criterion`.
- Gate on correctness. Compare output against independently expected results (golden tests, `size_of` guards,
  catalog-hash stability assertions), including malformed input, cancellation, ordering, and invalidation.
- Keep gains that exceed measured control-run variation in three paired runs, with no repeatable regression beyond
  noise in another required workload. Re-rank remaining candidates after each retained change. Prefer single-agent
  iteration; delegate only when independent work or context isolation clearly helps.
- No `unsafe` for speed. VT Code prohibits `unsafe` in product code; use iterators, `memchr`, `with_capacity`, `Arc`
  sharing, and enum footprint reduction per `rust-performance-principles.md`.

### Interactive latency workloads

The `agent_harness` target measures the repeated work that affects interactive requests: warm prompt-resource cache
hits, few-shot tag selection, tool definition sorting during catalog refresh, scoring against a warm indexed file list,
and cold-cache tool-catalog assembly under hosted, client-local, and disabled deferred-loading policies. Its fixtures
are deterministic and keep filesystem setup outside the timed iterations.

Prompt resources use canonical source paths, a five-minute bounded cache, and a two-second metadata polling interval.
Cache misses perform scans, reads, and parsing on Tokio's blocking pool; warm prompt assembly does not reread or reparse
unchanged resources. Indexed searches read an immutable path-text table by `StringId`; incremental updates publish a new
table so searches holding an older index remain safe.

Workspace tool registration also retains the effective persistent-memory configuration from its startup parse. Memory
tool calls no longer reload and reparse `vtcode.toml`; this follows the same snapshot policy as the web-tool and
output-spooler settings while preserving the disabled-memory guard.

Basic directory-list cache keys include the canonical workspace and every response-shaping filter and pagination value.
A cached listing therefore stays local to its workspace and cannot satisfy a request with a different list shape; the
async path reuses its single metadata result for directory checks.

The same target also includes uncached filesystem workloads: `agent_harness_file_search_uncached` measures parallel
traversal and bounded candidate aggregation, while `agent_harness_file_index_build` measures a full index construction
per iteration. `agent_harness_tool_catalog_projection_repeat` measures repeated schema/model-tool projection after the
catalog is warm; its projection cache is private to an immutable catalog and keyed by documentation mode. These
benchmarks expose repeated work and synchronization cost rather than serving as universal CI thresholds.

Code-search changes should be checked for both backend overlap and blocking pool behavior; progress-ledger changes
should be checked for bounded queue growth and latest-snapshot semantics before comparing end-to-end medians.

Compare repeated local medians rather than adding a noisy hard gate:

```bash
./scripts/perf/baseline.sh baseline
./scripts/perf/baseline.sh latest
./scripts/perf/compare.sh
```

Rustc-specific AST shrinking, compiler incremental-cache changes, and PGO are outside this runtime-focused wave. Revisit
them only with a confirmed VT Code profile hotspot and a separate build-performance budget. If PGO is adopted,
use `codegen-units = 1` for both instrumented and final builds (uv#22303: instrumented stage held 87-92% of
savings; profiling keeps function bodies alive, so 1 CGU enables earlier cleanup before fat LTO).

## Optimization Rules

- Change one thing at a time.
- Keep changes surgical and behavior-preserving.
- Prefer simple, safe single-pass reductions over broad refactors.
- Revisit data structures before introducing algorithmic sophistication.
- Keep the simplest implementation until measured workload data proves it insufficient.
- For hashers, follow the selective policy in `performance-hasher-policy.md`.
