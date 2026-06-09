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
