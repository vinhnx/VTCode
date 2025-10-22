# Terminal-Bench Hello-World Setup for VTCode

This guide walks through preparing [Terminal-Bench](https://www.tbench.ai/docs) to run the `hello-world`
benchmark task with the VTCode agent. It covers container engine configuration (Docker or Podman),
installing the Terminal-Bench CLI, packaging VTCode for autonomous execution, and invoking the
benchmark harness with a custom agent wrapper.

> **Scope.** The commands below assume a Unix-like host (Linux or macOS) with administrative access.
Windows users can follow the same steps inside WSL2. Replace paths with values appropriate for your
environment.

## Prerequisites

- Python 3.10 or newer (3.12+ recommended) and `pip` or [`uv`](https://docs.astral.sh/uv/).
- Git and Rust (for building VTCode).
- Network access to pull container images and model providers used by VTCode.
- API keys exported for the model provider you intend to use (for example `export GEMINI_API_KEY=...`).

## Step&nbsp;1 – Install and validate a container engine

Terminal-Bench orchestrates each task inside a container. Choose either Docker or Podman and
complete the matching setup.

### Option A – Docker

```bash
# Ubuntu / Debian
sudo apt-get update
sudo apt-get install -y ca-certificates curl gnupg
sudo install -m 0755 -d /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/ubuntu/gpg | sudo gpg --dearmor -o /etc/apt/keyrings/docker.gpg
sudo chmod a+r /etc/apt/keyrings/docker.gpg
echo \
  "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
  https://download.docker.com/linux/ubuntu $(. /etc/os-release && echo "$VERSION_CODENAME") stable" \
  | sudo tee /etc/apt/sources.list.d/docker.list >/dev/null
sudo apt-get update
sudo apt-get install -y docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin

# Allow your user to run the daemon without sudo (log out/in afterwards)
sudo usermod -aG docker "$USER"

# Start and validate the daemon
sudo systemctl enable --now docker
sudo docker info
```

```bash
# macOS (Homebrew)
brew install colima docker
colima start --cpu 4 --memory 8

# Verify the Docker CLI talks to the Colima VM
docker info
```

### Option B – Podman

```bash
# Fedora / RHEL / CentOS
sudo dnf install -y podman podman-docker

# Ubuntu / Debian
sudo apt-get update
sudo apt-get install -y podman podman-docker
```

Enable the Podman service socket (rootless is recommended) and expose a Docker-compatible API:

```bash
systemctl --user enable --now podman.socket
export DOCKER_HOST=unix://$XDG_RUNTIME_DIR/podman/podman.sock
podman system service --time=0 &   # keep-alive background service
podman info
```

If you prefer rootful Podman, replace `systemctl --user` with `sudo systemctl` and set
`DOCKER_HOST=unix:///run/podman/podman.sock`. Terminal-Bench will connect through the Docker API, so
keeping the compatibility shim (`podman-docker`) installed is essential.

## Step&nbsp;2 – Install the Terminal-Bench CLI

Terminal-Bench distributes a Python CLI named `tb`. Install it globally or in a virtual environment:

```bash
# Using uv (preferred)
uv tool install terminal-bench

# Or with pip for the current user
pip install --user terminal-bench
```

Confirm the installation and review the available commands:

```bash
~/.local/bin/tb --help
~/.local/bin/tb datasets list
```

Add `~/.local/bin` to your `PATH` if the `tb` command is not found.

## Step&nbsp;3 – Clone VTCode and enable full-auto mode

```bash
# Clone and enter the workspace
git clone https://github.com/vtcode/vtcode.git
cd vtcode

# Create a workspace-local configuration
cp vtcode.toml.example vtcode.toml
```

Edit `vtcode.toml` so the automation section allows unattended execution:

```toml
[automation.full_auto]
enabled = true
require_profile_ack = true
max_turns = 30
allowed_tools = ["read_file", "list_files", "grep_file", "simple_search", "write_file", "run_terminal_cmd"]
profile_path = "automation/full_auto_profile.toml"
```

Create the acknowledgement profile referenced above:

```bash
mkdir -p automation
cat <<'PROFILE' > automation/full_auto_profile.toml
operator = "vtcode-tbench"
reviewed_at = "$(date --iso-8601=seconds)"
notes = "Autonomous runs limited to Terminal-Bench containers."
PROFILE
```

Build the VTCode binary that will run inside the benchmark container:

```bash
cargo build --release --bin vtcode
```

The compiled binary will be available at `target/release/vtcode`.

## Step&nbsp;4 – Stage VTCode artifacts for injection

Terminal-Bench starts from a clean container for each task. Stage the pieces the agent must copy in
at runtime:

```bash
mkdir -p ~/.cache/vtcode-tbench
cp target/release/vtcode ~/.cache/vtcode-tbench/vtcode
cp vtcode.toml ~/.cache/vtcode-tbench/vtcode.toml
cp automation/full_auto_profile.toml ~/.cache/vtcode-tbench/full_auto_profile.toml
```

Keep this directory updated whenever you rebuild VTCode or tweak the automation profile. The custom
agent will use it as the source of truth.

## Step&nbsp;5 – Implement a VTCode agent wrapper for Terminal-Bench

Create a small Python module that subclasses `BaseAgent` and boots VTCode inside the task container.
The example below expects the staged files from the previous step and keeps the interface flexible by
accepting keyword arguments.

```
# File: tbench_vtcode_agent/__init__.py
from __future__ import annotations

import shlex
import textwrap
from pathlib import Path

from terminal_bench.agents.base_agent import BaseAgent, AgentResult
from terminal_bench.agents.failure_mode import FailureMode
from terminal_bench.terminal.tmux_session import TmuxSession


class VTCodeAgent(BaseAgent):
    """Launches the vtcode binary inside a Terminal-Bench tmux session."""

    def __init__(
        self,
        vtcode_binary: str,
        vtcode_config: str,
        vtcode_profile: str,
        workspace_subdir: str = ".",
        **kwargs,
    ) -> None:
        super().__init__(**kwargs)
        self._binary = Path(vtcode_binary).expanduser().resolve()
        self._config = Path(vtcode_config).expanduser().resolve()
        self._profile = Path(vtcode_profile).expanduser().resolve()
        self._workspace_subdir = workspace_subdir

    @staticmethod
    def name() -> str:
        return "vtcode"

    def _push_file(
        self,
        session: TmuxSession,
        host_path: Path,
        container_path: str,
        mode: int,
    ) -> None:
        session.copy_to_container(paths=host_path, container_dir=str(Path(container_path).parent))
        session.send_keys(
            [
                f"install -m {mode:o} {shlex.quote(str(Path('/tmp') / host_path.name))} {shlex.quote(container_path)}",
                "Enter",
            ],
            block=True,
        )

    def perform_task(
        self,
        instruction: str,
        session: TmuxSession,
        logging_dir=None,
    ) -> AgentResult:
        try:
            stage_dir = Path("/tmp/vtcode-bootstrap")
            session.send_keys([f"mkdir -p {stage_dir}", "Enter"], block=True)

            # Copy binary and configuration
            session.copy_to_container(paths=self._binary, container_dir=str(stage_dir))
            session.copy_to_container(paths=self._config, container_dir=str(stage_dir))
            session.copy_to_container(paths=self._profile, container_dir=str(stage_dir))

            session.send_keys(
                [
                    "bash -lc \"\n"
                    "  install -Dm755 /tmp/vtcode-bootstrap/vtcode ~/.local/bin/vtcode\n"
                    "  install -Dm644 /tmp/vtcode-bootstrap/vtcode.toml ~/vtcode.toml\n"
                    "  install -Dm644 /tmp/vtcode-bootstrap/full_auto_profile.toml ~/automation/full_auto_profile.toml\n"
                    "\"",
                    "Enter",
                ],
                block=True,
                max_timeout_sec=120,
            )

            prompt = self._render_instruction(instruction)
            escaped_prompt = shlex.quote(prompt)
            workspace = shlex.quote(str(Path("/workspace") / self._workspace_subdir))

            run_script = textwrap.dedent(
                f"""
                bash -lc '\n"
                "  export PATH="$HOME/.local/bin:$PATH"\n"
                "  export VTCODE_AUTOMATION_ACK=1\n"
                "  mkdir -p automation\n"
                "  vtcode --workspace {workspace} --full-auto {escaped_prompt}\n"
                "'
                """
            ).strip()

            session.send_keys([run_script, "Enter"], block=True, max_timeout_sec=900)
            return AgentResult()
        except Exception as exc:  # noqa: BLE001
            session.send_keys(["pkill -f vtcode", "Enter"], block=False)
            return AgentResult(failure_mode=FailureMode.UNKNOWN_AGENT_ERROR)
```

Ensure the module is importable by the `tb` CLI. For quick experiments, set
`export PYTHONPATH=$PWD/tbench_vtcode_agent` (if the directory contains an `__init__.py`) or install
the package in editable mode with `pip install -e .`.

## Step&nbsp;6 – Run the hello-world benchmark

Execute the Terminal-Bench run with your custom agent. Replace the paths with the staged directory
from Step&nbsp;4.

```bash
export PYTHONPATH=$PYTHONPATH:$PWD

TB_VTCODE_CACHE="$HOME/.cache/vtcode-tbench"
tb run \
  --dataset terminal-bench-core==head \
  --task-id hello-world \
  --agent-import-path tbench_vtcode_agent:VTCodeAgent \
  --agent-kwarg vtcode_binary="$TB_VTCODE_CACHE/vtcode" \
  --agent-kwarg vtcode_config="$TB_VTCODE_CACHE/vtcode.toml" \
  --agent-kwarg vtcode_profile="$TB_VTCODE_CACHE/full_auto_profile.toml" \
  --run-id vtcode-hello-world
```

For Podman, keep `DOCKER_HOST` pointed at the Podman socket (`export DOCKER_HOST=unix://$XDG_RUNTIME_DIR/podman/podman.sock`) before
invoking `tb run`.

During execution, Terminal-Bench launches a tmux session inside the task container. The wrapper above
copies the VTCode binary plus configuration, runs the agent in full-auto mode against the
`/workspace` mount provided by the dataset, and returns control once VTCode exits.

## Step&nbsp;7 – Review the results

Terminal-Bench writes artefacts under the `runs/` directory (or the location given by
`--output-path`). Inspect the outcome and logs with:

```bash
ls runs/vtcode-hello-world
cat runs/vtcode-hello-world/results.json | jq
less runs/vtcode-hello-world/logs/*.log
```

The `results.json` file reports success/failure for each task along with metadata such as execution
hashes, timestamps, and container IDs. Terminal-Bench also saves an asciinema recording and the raw
tmux transcript, which are valuable for debugging agent behaviour.

## Troubleshooting

- **Docker/Podman connection errors.** Confirm `docker info` or `podman info` succeeds on the host.
  For Podman, the compatibility socket must be running and `DOCKER_HOST` must reference it.
- **Missing VTCode binary.** Re-run `cargo build --release` after pulling new commits, then refresh the
  staged cache in Step&nbsp;4.
- **API key failures.** VTCode reads API keys from the container environment. Export variables (or drop a
  `.env` file) before starting the run so the wrapper script inherits them.
- **Full-auto guardrails triggered.** If VTCode exits complaining about the acknowledgement profile,
  verify the profile exists in the staged cache and the automation settings in `vtcode.toml` match the
  values expected by the agent wrapper.

## Cleaning up

After experimenting, remove images and containers created by Terminal-Bench:

```bash
tb runs cleanup  # removes cached container images

# Docker
docker system prune -f

# Podman
podman system prune -f
```

You can also delete the `runs/` directory once you have captured the logs you need.
