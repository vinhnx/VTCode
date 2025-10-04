# Terminal-Bench Integration Guide

This guide explains how to evaluate VT Code with
[Terminal-Bench](https://github.com/terminal-bench). It is intentionally focused on the
**VT Code** agent that lives in this repository—no third-party coding assistants (for example,
Claude Code or GitHub Copilot) are required or referenced. The steps below cover local
prerequisites, configuration updates for automated runs, and the workflow for running the
`hello-world` task with the custom agent.

> **Why call out VT Code explicitly?** Terminal-Bench ships harness integrations for many hosted
> agents, but this project is evaluated as a self-contained coding agent. All commands and
> configuration snippets in this guide call `VTCodeTerminalBenchAgent`, ensuring the benchmark runs
> against the first-party VT Code experience.

## 1. Prerequisites

1. **Install Docker** (required by Terminal-Bench tasks).
   - **macOS (Docker Desktop):**
     1. Download the latest **Docker Desktop for Mac** release that matches your hardware
        (Apple Silicon or Intel) from [docker.com](https://docs.docker.com/desktop/install/mac-install/).
     2. Install the application, launch it once, and sign in if prompted so Docker Desktop can
        configure the virtualized runtime.
     3. Open **Settings → Resources → Advanced** and allocate at least 4 CPUs, 8 GB of RAM, and 60 GB
        of disk space to ensure Terminal-Bench tasks have enough headroom.
     4. Enable the **Use Docker Compose V2** setting (it is on by default in current releases).
     5. After installation, confirm Docker is active by running `docker info` from a terminal.

   - **Debian/Ubuntu (Docker Engine):**
     ```bash
     sudo apt-get update
     sudo apt-get install -y ca-certificates curl gnupg
     curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo gpg --dearmor -o /usr/share/keyrings/docker.gpg
     echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/docker.gpg] https://download.docker.com/linux/ubuntu $(lsb_release -cs) stable" \
         | sudo tee /etc/apt/sources.list.d/docker.list > /dev/null
     sudo apt-get update
     sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
     sudo usermod -aG docker "$USER"
     ```
     Log out and back in (or restart your shell) after adding your user to the `docker` group.

   Pull the official Terminal-Bench Ubuntu base image so future runs reuse the cached layers. VT
   Code is tested against the same image, so pinning to it avoids surprises when reproducing
   benchmark results:
   ```bash
   docker pull ghcr.io/laude-institute/terminal-bench/t-bench/ubuntu-24-04:latest
   ```
   Terminal-Bench tasks reference this image when building their client containers, so pre-pulling it
   prevents the harness from compiling the stack from scratch during the first run and guarantees the
   runtime matches the environment we validate in CI.

2. **Install uv** (Python launcher used by the project tooling).
   ```bash
   curl -LsSf https://astral.sh/uv/install.sh | sh
   ```

3. **Install the Terminal-Bench CLI**.
   ```bash
   uv tool install terminal-bench
   ```

4. **Install Rust (stable toolchain)** if it is not already available.
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
   source "$HOME/.cargo/env"
   ```

## 2. Configure VT Code for unattended runs

Terminal-Bench interacts with VT Code through full-auto mode. Ensure the workspace configuration
enables automation, allows the tools required by Terminal-Bench tasks, and renders logs using the
inline UI to avoid TUI escape sequences.

1. Copy the example configuration if you do not already have one:
   ```bash
   mkdir -p .vtcode
   cp vtcode.toml.example .vtcode/vtcode.toml
   ```

2. Edit `.vtcode/vtcode.toml` and enable full-auto execution:
   ```toml
   [automation.full_auto]
   enabled = true
   require_profile_ack = false
   allowed_tools = [
       "run_terminal_cmd",
       "bash",
       "read_file",
       "list_files",
       "write_file",
       "apply_patch"
   ]

   [agent]
   ui_surface = "inline"
   ```
   Adjust the allowlist based on the tasks you plan to evaluate. The inline surface keeps the
   output readable when Terminal-Bench captures the session transcript.

3. Export the API key for your preferred model provider before running the benchmark. For example:
   ```bash
   export GEMINI_API_KEY="your-google-api-key"
   ```
   The agent automatically forwards the standard provider environment variables listed in
   `tools/terminal_bench/vtcode_agent.py`.

## 3. Custom Terminal-Bench agent

The repository ships a reusable agent wrapper at
`tools/terminal_bench/vtcode_agent.py`. It installs VT Code inside the task container and boots the
CLI in full-auto mode with a seeded task description. Key files:

- `tools/terminal_bench/setup.sh`: installation script executed in the task container.
- `tools/terminal_bench/vtcode_agent.py`: `AbstractInstalledAgent` implementation.
- `vtcode-core/src/config/constants.rs`: exposes the
  `VTCODE_AUTOMATION_INPUT_SEQUENCE` environment key so the runloop can seed prompts.
- `src/agent/runloop/unified/turn.rs`: loads the automation sequence and replays each entry as if it
  were typed by the user. Entries are parsed from a JSON array and executed in order. The agent
  appends an `exit` command after the main instruction so the process terminates cleanly once the
  task finishes.

## 4. Running the `hello-world` task

1. Ensure Docker is running locally and that you can execute `docker ps` without sudo.

2. From the VT Code repository root, run the Terminal-Bench harness with the custom agent:
   ```bash
   tb run \
       --dataset terminal-bench-core==head \
       --agent-import-path tools.terminal_bench.vtcode_agent:VTCodeTerminalBenchAgent \
       --agent-kwarg config_path=$PWD/.vtcode/vtcode.toml \
       --task-id hello-world
   ```
   The harness performs the following steps automatically:
   - builds a container for the task,
   - copies `tools/terminal_bench/setup.sh` into the container and installs VT Code,
   - exports provider credentials,
   - sets `VTCODE_AUTOMATION_INPUT_SEQUENCE` to `["<task description>", "exit"]`,
   - runs `vtcode --full-auto --skip-confirmations --no-color chat`.

3. When the run completes, inspect the summary produced by Terminal-Bench. Logs are stored in the
   directory printed by the harness (inside `~/.terminal-bench/runs/`).

## 5. Troubleshooting checklist

- **Docker permissions:** run `docker ps` to verify your shell session is in the `docker` group.
- **API keys:** confirm the expected environment variable is exported before running `tb run`.
- **Model configuration:** adjust the `--provider` and `--model` CLI flags if your workspace
  defaults differ from the desired evaluation setup.
- **Long tasks:** increase `max_timeout_sec` on the agent command inside
  `tools/terminal_bench/vtcode_agent.py` if the benchmark times out before completion.
- **Multiple prompts:** edit the JSON sequence constructed in `_run_agent_commands` if you need more
  scripted turns (for example, add intermediate confirmation strings before the final `exit`).

Following the steps above allows you to evaluate VT Code against Terminal-Bench tasks without manual
intervention.
