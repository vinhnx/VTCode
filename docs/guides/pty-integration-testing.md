# PTY Integration Testing Guide

This guide shows how to exercise the portable-pty powered terminal path so you can verify command execution, transcript
capture, and TUI rendering end-to-end.

## Prerequisites

1. Install the project dependencies:

   ```bash
   rustup show # ensures the pinned toolchain is active
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

The regression surface includes ANSI and carriage-return output, large output, command errors, labeled stdout/stderr
pipe results, callback pressure, and the complete session-local Transcript Review. Complete output remains available to
the configured Transcript Review shortcut (default `Ctrl+T`) while the compact live view keeps only its bounded preview.
Contiguous successful PTY calls may share one compact activity row, but failures, warnings, stderr, diffs, and artifacts
remain visible inline. The review must preserve conversation order and must not show duplicate output aliases. When the
live preview queue is full, tests should observe a bounded coalesced/drop notice instead of silent loss. Use the same
environment workaround as CI-local iteration when needed:

```bash
RUSTC_WRAPPER= cargo nextest run --locked --test pty_tests
```

To execute the same checks plus external tool availability in one pass, use the helper script:

```bash
scripts/test_pty_tools.sh
```

The script runs the PTY tests and prints the captured log if any PTY assertion fails.

## Manual TUI Walkthrough

1. Build and launch the interactive client in debug mode (fast incremental rebuilds):

   ```bash
   scripts/run-debug.sh
   ```

   The script compiles the binary and starts `vtcode chat` with debug flags enabled. If you need to override the
   workspace directory, set `WORKSPACE=/path/to/project` before running the script.

2. Once the TUI loads, run a shell command with the `!` shell-mode prefix:

   ```text
   !sh -c "printf 'hello from portable-pty' && sleep 1"
   ```

   The agent routes the request through `run_pty_cmd`, which now uses the shared `PtyManager` backend.

3. Watch the transcript pane: you should see the command summary, streamed PTY output (including ANSI sequences), and
   the final exit status. Resize the terminal window to confirm `portable-pty` propagates the new dimensions without
   breaking the screen buffer.

4. To inspect the preserved output after the command completes, press the configured Transcript Review shortcut (default
   `Ctrl+T`) or click the visible review hint on a compact activity row. Transcript Review includes the complete
   multi-line PTY output in its original conversation position, while the live transcript keeps only the configured
   bounded preview. Press the configured render-mode shortcut (default `R`) to verify the ANSI-free raw view, and use
   `v` or `[` to exercise complete-output handoffs.

## Troubleshooting

- **Timeouts** – Increase `command_timeout_seconds` in `vtcode.toml` under `[pty]` if long-running commands exceed the
  default limit.
- **Terminal size issues** – Adjust `[pty]` `default_rows` and `default_cols` in `vtcode.toml`, then relaunch the agent
  so the PTY environment variables reflect the new size.
- **Windows hosts** – No additional setup is required; `portable-pty` selects the ConPTY backend automatically when
  available.

Following these steps exercises the entire PTY stack—from command preparation through `portable-pty` execution and
transcript rendering—so you can confirm the integration behaves as expected.
