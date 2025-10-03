# Terminal-Bench Integration Guide

This guide documents how to run [Terminal-Bench](https://www.tbench.ai/docs/task-quickstart) against VTCode using the custom
`VTCodeTerminalBenchAgent` adapter.

## Prerequisites

1. Install the Terminal-Bench CLI by following the [official installation guide](https://www.tbench.ai/docs/installation).
2. Build VTCode locally so the benchmark harness can copy the release binary into the task container:

    ```bash
    cargo build --release
    ```

3. Export credentials for your configured provider (for example `OPENROUTER_API_KEY`). The agent reads the same environment
   variables as the regular VTCode CLI.

## Running a Benchmark Trial

Run a single task from the core dataset with:

```bash
OPENROUTER_API_KEY=... tb run \
  --dataset terminal-bench-core==head \
  --agent-import-path benchmarks.terminal_bench.vtcode_agent:VTCodeTerminalBenchAgent \
  --task-id hello-world
```

The adapter performs the following sequence:

1. Ensures a fresh `/workspace` directory exists inside the Terminal-Bench container.
2. Copies `vtcode.toml` and the automation profile into the container.
3. Copies the pre-built `target/release/vtcode` binary, sets execute permissions, and starts VTCode in full-auto mode.
4. Streams benchmark instructions to VTCode and monitors progress until completion or timeout.

## Troubleshooting

- **Binary missing error**: Re-run `cargo build --release` locally; the adapter now aborts early if the binary cannot be found
  after building.
- **Authentication failures**: Confirm the relevant API keys are exported before running `tb run`.
- **Long startup delays**: The harness no longer builds inside the task container, so initialisation should be quick. If boot
  still times out, increase the `boot_timeout_sec` argument when constructing the agent.

For advanced configuration, pass custom paths when instantiating `VTCodeTerminalBenchAgent` (for example a different
`workspace_path` or configuration template). Any supplied paths are expanded relative to the repository root on the host.
