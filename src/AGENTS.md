# vtcode (binary)

[Root AGENTS.md](../AGENTS.md) | CLI entrypoint, session bootstrap, agent runloop wiring.

## Modules

`main.rs` binary entry | `agent/` runloop + subagent dispatch | `cli/` CLI handlers | `startup/` first-run + onboarding | `updater/` self-update | `codex_app_server/` app server bridge | `main_helpers/` tracing + runtime init

## Rules

- Thin binary — all runtime logic in `vtcode-core`. This crate wires CLI args to `Agent::run()`.
- Global allocator is `mimalloc` by default; `allocator-jemalloc` opts into
  `tikv-jemalloc` (background-thread purging, recommended for Linux servers).
  Selected in `src/allocator.rs` (a `mod allocator;` in `src/main.rs`). See
  `docs/development/ALLOCATOR_MEMORY.md`.
- `vtcode bench-allocator` measures RSS under a bursty/sparse Tokio workload to detect
  allocator memory pinning; use it before changing the default allocator (see Gotchas).
- `vtcode_ui::tui::panic_hook` installs custom panic handler — must run before any output.
- `agent/runloop/` contains the single-agent runloop. Multi-agent is in `vtcode-core::subagents`.

## Gotchas

- `main_helpers` handles runtime relaunch context — do not duplicate init logic in `main.rs`.
- Allocator memory pinning: vtcode's bursty/sparse Tokio workload (semaphore-capped
  `JoinSet` fans-out, then workers idle) makes both `mimalloc` and `glibc` hold RSS flat
  after a burst (frees stranded on cross-thread lists, only reconciled by future allocation
  activity). `jemalloc` only returns memory on idle when its `background_thread` is active —
  on macOS the jemalloc build prints `background_thread currently supports pthread only` and
  behaves like mimalloc, so switching the allocator does NOT help on macOS dev. On Linux
  containers (the article's target) `background_thread` works and jemalloc reclaims memory.
  Measure with `vtcode bench-allocator` before changing the default; do not switch blindly.
- `load_dotenv()` must run before config load to pick up `.env` API keys.
- Provider noise (e.g. MiniMax `]<]minimax[>[`) is stripped centrally in `turn::provider_noise`. Stream-level sanitization (harmony + minimax) lives in `stream_sanitization::StreamSanitizer`. Do not re-implement noise stripping inline.
- Wall-clock budget exhaustion (`run_loop_context::record_wall_clock_exhaustion_notice`) emits the full policy message once, a compact stub for later calls in the batch, then a single "synthesize now" directive via `flush_wall_clock_directive` (called after the tool batch so it is never interleaved between tool responses). Do not push the directive inline in `validate_tool_call`.
- `turn_processing/llm_request/` is split into contract-carrying submodules (`snapshot`, `tool_shaping`, `context_management`, `response_chain`, `prompt_assembly`) with `pub(super)` visibility; go through `mod.rs`'s exports. Prompt section order in `prompt_assembly::build_prompt_output` is prompt-cache-sensitive — reordering sections invalidates provider caches. `tool_shaping.rs` carries the wire invariant: hosted Anthropic/OpenAI payloads keep full deferred tool definitions; only the ClientLocal policy omits them (pinned by tests in `request_builder.rs`).
