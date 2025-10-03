# Running VTCode with Terminal Benchmark (TBench)

This guide walks through preparing VTCode to run the [Terminal Benchmark (TBench)](https://www.tbench.ai/docs)
terminal evaluation. It covers CLI installation, configuration, and execution using the new
`vtcode benchmark` command.

> **Note:** The TBench CLI distribution may evolve. Always consult the official documentation for
the latest installation instructions, then map the resulting CLI path into the configuration
outlined below.

## Prerequisites

1. A working VTCode installation (Cargo, Homebrew, or npm).
2. Access to at least one supported LLM provider (API keys exported in your shell).
3. The TBench CLI installed locally. Common installation patterns include:

    ```bash
    # Example: install via uvx or pipx (adjust per official docs)
    uvx tbench --help

    # Or using npm/Node distribution
    npm install -g tbench-cli
    ```

    After installation, confirm the binary location and export `TBENCH_CLI` for VTCode:

    ```bash
    export TBENCH_CLI="$(command -v tbench)"
    ```

4. Optional: prepare a benchmark scenario file (YAML or JSON as required by TBench).

## Configure `vtcode.toml`

Add or update the `[benchmark.tbench]` section in `vtcode.toml` to describe how VTCode should launch
TBench:

```toml
[benchmark.tbench]
enabled = true
# command = "/usr/local/bin/tbench"          # Optional when command_env is provided
command_env = "TBENCH_CLI"                    # Fallback environment variable for the CLI
args = ["run", "--config", "benchmarks/vtcode.yaml"]
config_path = "benchmarks/vtcode.yaml"        # Workspace-relative scenario definition
results_dir = "benchmarks/results"            # Directory created before the run
run_log = "benchmarks/logs/latest.log"        # Optional consolidated stdout/stderr log
attach_workspace_env = true                   # Inject VT_CODE_WORKSPACE into the runner
env = { "TBENCH_API_KEY" = "${env:TBENCH_API_KEY}" }
passthrough_env = [
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "TBENCH_API_KEY",
]
```

Key behaviors:

- **Command resolution** – VTCode uses `command` when set; otherwise it reads `command_env` (defaults
  to `TBENCH_CLI`). Empty values trigger a friendly error with remediation guidance.
- **Path resolution** – `config_path`, `results_dir`, and `run_log` are resolved relative to the
  active workspace unless absolute.
- **Environment management** – `env` injects static key/value pairs. `passthrough_env` copies values
  from the current process if defined (ideal for API keys). When `attach_workspace_env` is true,
  VTCode exports `VT_CODE_WORKSPACE` pointing to the workspace root for the TBench runner.

## Running the benchmark

Once configuration is in place, launch the evaluation from the workspace root:

```bash
vtcode benchmark
```

The command prints the resolved CLI, scenario path, and output directories before streaming
TBench output. VTCode mirrors stdout/stderr to your terminal and optionally writes both streams to
`run_log` with `[stdout]`/`[stderr]` prefixes for easier triage.

Successful completion returns exit code `0`. Non-zero exits propagate as errors with the recorded
status code.

## Inspecting artifacts

- **Results directory** – Created automatically when `results_dir` is set. Populate this directory
  path in your TBench configuration to collect transcripts or scores.
- **Run log** – When `run_log` is configured, VTCode rewrites the file at each invocation and appends
  tagged stream entries.
- **Environment variables** – The benchmark process receives:
  - `VT_CODE_WORKSPACE`: workspace root (when `attach_workspace_env = true`).
  - `TBENCH_CONFIG`: resolved scenario path when `config_path` is present.
  - `TBENCH_OUTPUT_DIR`: resolved results directory when `results_dir` is present.
  - `TBENCH_RUN_LOG`: resolved log file when `run_log` is present.

## Troubleshooting

| Symptom | Resolution |
| --- | --- |
| `Unable to determine benchmark CLI command` | Ensure `command` is set or export the environment variable defined in `command_env` (`TBENCH_CLI` by default). |
| `Benchmark working directory does not exist` | Verify the workspace path or set `working_directory` under `[benchmark.tbench]` to a valid location. |
| CLI exits immediately with missing credentials | Add the necessary keys to `env` or `passthrough_env` so they reach the TBench runner. |
| No artifacts produced | Confirm that the scenario file references `TBENCH_OUTPUT_DIR` or the configured results path. |

With these steps, VTCode becomes an orchestrator for the Terminal Benchmark suite, providing a
repeatable way to evaluate agent performance directly from the CLI.
