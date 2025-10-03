# Terminal-Bench Setup for VTCode

This guide outlines how to run [Terminal-Bench](https://www.tbench.ai/docs/first-steps) against the VTCode agent using the OpenRouter `x-ai/grok-4-fast:free` model. It walks through the required environment variables, configuration, and the custom agent adapter provided in this repository.

## Prerequisites

1. **Install VTCode** (any of the supported distributions):
    - `cargo install vtcode`
    - `brew install vinhnx/tap/vtcode`
    - `npm install -g vtcode`
2. **Install Terminal-Bench CLI** in your Python environment:
    ```bash
    pip install terminal-bench
    ```
3. **Configure the OpenRouter API key** for Grok-4-Fast access:
    ```bash
    export OPENROUTER_API_KEY="sk-..."
    ```
4. **Clone this repository** and ensure it is mounted or available inside the benchmark container/workspace.

## Configuration Files

The repository now ships with a benchmark-ready configuration:

-   `vtcode.toml`
    -   Sets `provider = "openrouter"` and `default_model = "x-ai/grok-4-fast:free"`.
    -   Forces the inline UI surface so VTCode renders cleanly in tmux.
    -   Enables full-auto mode with an explicit tool allow-list for Terminal-Bench.
-   `automation/full_auto_profile.toml`
    -   Acknowledges automated execution. Required because full-auto mode is enabled.

The custom Terminal-Bench adapter copies both files into the workspace before launching VTCode, so no manual steps are needed as long as the repository is present.

## Custom Agent Adapter

The adapter lives at `benchmarks/terminal_bench/vtcode_agent.py` and implements `VTCodeTerminalBenchAgent`. It:

-   Builds VTCode from source in the container during workspace preparation
-   Launches VTCode with `--full-auto --provider openrouter --model x-ai/grok-4-fast:free`
-   Sends the benchmark instruction into the session
-   Streams terminal output to `logging_dir/vtcode-session.log` for later inspection
-   Shuts down the session gracefully after either detecting completion or hitting the timeout window

You can customise behaviour via `--agent-kwarg` parameters:

| Kwarg              | Description                                         | Default  |
| ------------------ | --------------------------------------------------- | -------- |
| `run_timeout_sec`  | Maximum time (seconds) to let VTCode work on a task | `900`    |
| `boot_timeout_sec` | Startup grace period after launching VTCode         | `15`     |
| `vtcode_binary`    | Name/path of the VTCode executable                  | `vtcode` |

Example override: `--agent-kwarg run_timeout_sec=1200`.

## Running the Benchmark

Once prerequisites are ready, run Terminal-Bench with the custom adapter:

```bash
TB_DATASET="terminal-bench-core==head"
TB_AGENT="benchmarks.terminal_bench.vtcode_agent:VTCodeTerminalBenchAgent"
TB_TASK="hello-world"

cd /path/to/vtcode

tb run \
    --dataset "$TB_DATASET" \
    --agent-import-path "$TB_AGENT" \
    --task-id "$TB_TASK"
```

Notes:

-   The adapter automatically builds VTCode from source in the container and uses `/workspace` as the working directory.
-   Additional `tb run` flags (e.g., `--record`, `--dry-run`) are supported as usual.

## Logs and Debugging

-   VTCode terminal output is appended to the configured `logging_dir` (`vtcode-session.log`).
-   Terminal-Bench also records tmux sessions, so you can replay the `.cast` files to study the run.
-   If the agent times out, the `FailureMode` in the Terminal-Bench summary will be `agent_timeout`. Increase `run_timeout_sec` if additional time is required.

## Safety Checklist

-   Use dedicated, disposable workspaces when running in full-auto mode.
-   Confirm `OPENROUTER_API_KEY` is scoped for Grok-4-Fast and the expected rate limits.
-   After benchmarking, reset `vtcode.toml` or switch `ui_surface` back to `auto` if you prefer the interactive TUI for manual sessions.

With these steps you can reproduce benchmarks from the Terminal-Bench guide while exercising the VTCode agent and Grok-4-Fast model on OpenRouter.
