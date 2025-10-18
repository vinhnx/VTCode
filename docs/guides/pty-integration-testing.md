# PTY Integration Testing Guide

This guide shows how to exercise the portable-pty powered terminal path so you can verify command execution, transcript capture, and TUI rendering end-to-end.

## Prerequisites

1. Install the project dependencies:
   ```bash
   rustup show # ensures the pinned toolchain is active
   cargo install cargo-nextest --locked # optional but preferred
   ```
2. Export at least one supported API key before launching the TUI (Gemini, OpenAI, Anthropic). For example:
   ```bash
   export GEMINI_API_KEY="your_api_key"
   ```
3. Make sure you are in the repository root (`vtcode/`).

## Automated Verification

Run the focused PTY smoke tests directly:

```bash
cargo nextest run --test pty_tests
```

If `cargo-nextest` is not installed, fall back to the standard test harness:

```bash
cargo test --package vtcode-core --test pty_tests
```

To execute the same checks plus external tool availability in one pass, use the helper script:

```bash
scripts/test_pty_tools.sh
```

The script automatically prefers `cargo nextest` when available and prints the captured log if any PTY assertion fails.

## Manual TUI Walkthrough

1. Build and launch the interactive client in debug mode (fast incremental rebuilds):
   ```bash
   scripts/run-debug.sh
   ```
   The script compiles the binary and starts `vtcode chat` with debug flags enabled. If you need to override the workspace directory, set `WORKSPACE=/path/to/project` before running the script.

2. Once the TUI loads, open the command palette by typing the slash command:
   ```text
   /command sh -c "printf 'hello from portable-pty' && sleep 1"
   ```
   The agent routes the request through `run_terminal_cmd`, which now uses the shared `PtyManager` backend.

3. Watch the transcript pane: you should see the command summary, streamed PTY output (including ANSI sequences), and the final exit status. Resize the terminal window to confirm `portable-pty` propagates the new dimensions without breaking the screen buffer.

4. To inspect the preserved output after the command completes, open the transcript detail view (`Tab` → select the latest `run_terminal_cmd` entry). The scrollback includes the multi-line PTY output exactly as captured by the parser.

## Troubleshooting

- **Timeouts** – Increase `command_timeout_seconds` in `vtcode.toml` under `[pty]` if long-running commands exceed the default limit.
- **Terminal size issues** – Adjust `[pty]` `default_rows` and `default_cols` in `vtcode.toml`, then relaunch the agent so the PTY environment variables reflect the new size.
- **Windows hosts** – No additional setup is required; `portable-pty` selects the ConPTY backend automatically when available.

Following these steps exercises the entire PTY stack—from command preparation through `portable-pty` execution and transcript rendering—so you can confirm the integration behaves as expected.
