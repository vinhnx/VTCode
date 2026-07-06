# vtcode (binary)

[Root AGENTS.md](../AGENTS.md) | CLI entrypoint, session bootstrap, agent runloop wiring.

## Modules

`main.rs` binary entry | `agent/` runloop + subagent dispatch | `cli/` CLI handlers | `startup/` first-run + onboarding | `updater/` self-update | `codex_app_server/` app server bridge | `main_helpers/` tracing + runtime init

## Rules

- Thin binary — all runtime logic in `vtcode-core`. This crate wires CLI args to `Agent::run()`.
- Uses `mimalloc` as global allocator — do not change.
- `vtcode_ui::tui::panic_hook` installs custom panic handler — must run before any output.
- `agent/runloop/` contains the single-agent runloop. Multi-agent is in `vtcode-core::subagents`.

## Gotchas

- `main_helpers` handles runtime relaunch context — do not duplicate init logic in `main.rs`.
- `load_dotenv()` must run before config load to pick up `.env` API keys.
- Provider noise (e.g. MiniMax `]<]minimax[>[`) is stripped centrally in `turn::provider_noise`. Stream-level sanitization (harmony + minimax) lives in `stream_sanitization::StreamSanitizer`. Do not re-implement noise stripping inline.
- Wall-clock budget exhaustion (`run_loop_context::record_wall_clock_exhaustion_notice`) emits the full policy message once, a compact stub for later calls in the batch, then a single "synthesize now" directive via `flush_wall_clock_directive` (called after the tool batch so it is never interleaved between tool responses). Do not push the directive inline in `validate_tool_call`.
