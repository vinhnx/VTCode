"""Terminal-Bench adapter for running VTCode in benchmark scenarios."""

from __future__ import annotations

import platform
import re
import time
import subprocess
from pathlib import Path
from typing import Optional

from terminal_bench.agents.base_agent import AgentResult, BaseAgent
from terminal_bench.agents.failure_mode import FailureMode
from terminal_bench.terminal.tmux_session import TmuxSession
from terminal_bench.utils.logger import logger


class VTCodeTerminalBenchAgent(BaseAgent):
    """Terminal-Bench agent that orchestrates a VTCode session."""

    _HOST_REPO_ROOT = Path(__file__).resolve().parents[2]
    _DEFAULT_CONFIG_PATH = _HOST_REPO_ROOT / "vtcode.toml"
    _DEFAULT_PROFILE_PATH = _HOST_REPO_ROOT / "automation" / "full_auto_profile.toml"
    _DEFAULT_WORKSPACE = Path("/workspace")
    _COMPLETION_KEYWORDS = (
        "session complete",
        "plan complete",
        "all steps completed",
        "no outstanding tasks",
    )
    _PROGRESS_PATTERN = re.compile(r"Progress:\s*(\\d+)/(\\d+) completed", re.IGNORECASE)

    def __init__(
        self,
        *,
        vtcode_binary: str = "vtcode",
        workspace_path: Optional[str | Path] = None,
        config_template: Optional[str | Path] = None,
        profile_template: Optional[str | Path] = None,
        boot_timeout_sec: float = 15.0,
        run_timeout_sec: float = 900.0,
        poll_interval_sec: float = 5.0,
        exit_command: str = "/exit",
        **kwargs,
    ) -> None:
        """Initialise the VTCode Terminal-Bench agent.

        Args:
            vtcode_binary: Name or absolute path of the VTCode executable inside the container.
            workspace_path: Optional workspace path for the benchmark container.
            config_template: Optional override for the VTCode configuration template.
            profile_template: Optional override for the full-auto acknowledgement profile.
            boot_timeout_sec: Time to wait for VTCode to become interactive.
            run_timeout_sec: Maximum time to allow VTCode to work on a task.
            poll_interval_sec: Interval for polling terminal output during monitoring.
            exit_command: Command sent to terminate the VTCode session at shutdown.
            **kwargs: Additional keyword arguments forwarded to :class:`BaseAgent`.
        """
        super().__init__(**kwargs)
        # Always use /workspace inside the container, ignore host paths
        self._workspace_path = (
            Path(workspace_path).expanduser() if workspace_path else self._DEFAULT_WORKSPACE
        )
        self._host_vtcode_binary = self._determine_host_binary_path(vtcode_binary)
        self._vtcode_binary = str(
            self._workspace_path / "target" / "release" / self._host_vtcode_binary.name
        )
        self._config_template = self._resolve_optional_path(
            config_template, self._DEFAULT_CONFIG_PATH
        )
        self._profile_template = self._resolve_optional_path(
            profile_template, self._DEFAULT_PROFILE_PATH
        )
        self._boot_timeout_sec = boot_timeout_sec
        self._run_timeout_sec = run_timeout_sec
        self._poll_interval_sec = poll_interval_sec
        self._exit_command = exit_command
        self._logger = logger.getChild(__name__)

    @staticmethod
    def name() -> str:
        """Return the unique identifier for this agent."""
        return "vtcode-openrouter-grok4fast"

    def perform_task(
        self,
        instruction: str,
        session: TmuxSession,
        logging_dir: Optional[Path] = None,
    ) -> AgentResult:
        """Execute the provided benchmark instruction using VTCode.

        Args:
            instruction: Natural-language description of the benchmark task.
            session: Active :class:`TmuxSession` instance provided by Terminal-Bench.
            logging_dir: Optional directory for persisting debug logs.

        Returns:
            An :class:`AgentResult` describing the outcome of the run.
        """
        rendered_instruction = self._render_instruction(instruction)
        aggregated_output: list[str] = []
        failure_mode = FailureMode.NONE

        try:
            self._prepare_workspace(session)
            time.sleep(1.0)
            self._launch_agent(session)
            self._send_instruction(session, rendered_instruction)
            outputs, timed_out = self._monitor_session(session)
            aggregated_output.extend(outputs)
            if timed_out:
                failure_mode = FailureMode.AGENT_TIMEOUT
        except TimeoutError:
            failure_mode = FailureMode.AGENT_TIMEOUT
        except Exception as exc:  # pylint: disable=broad-except
            self._logger.exception("VTCode agent execution failed: %s", exc)
            failure_mode = FailureMode.UNKNOWN_AGENT_ERROR
        finally:
            self._shutdown(session)
            if logging_dir is not None:
                self._persist_logs(logging_dir, aggregated_output)

        return AgentResult(
            total_input_tokens=0,
            total_output_tokens=0,
            failure_mode=failure_mode,
            timestamped_markers=[],
        )

    def _resolve_optional_path(
        self,
        candidate: Optional[str | Path],
        default: Path,
    ) -> Optional[Path]:
        """Resolve an optional path, falling back to a default when absent."""
        if candidate is not None:
            resolved = Path(candidate).resolve()
            return resolved if resolved.exists() else None

        return default if default.exists() else None

    def _determine_host_binary_path(self, binary: str) -> Path:
        """Determine the host-side VTCode binary path, building a default when needed."""
        candidate = Path(binary)
        if candidate.is_absolute():
            return candidate

        repo_candidate = (self._HOST_REPO_ROOT / candidate).resolve()
        if repo_candidate.exists():
            return repo_candidate

        fallback = (self._HOST_REPO_ROOT / "target" / "release" / candidate.name).resolve()
        return fallback

    def _ensure_host_binary(self) -> None:
        """Build the VTCode binary locally if it is missing."""
        if self._host_vtcode_binary.exists():
            return

        build_cmd = ["cargo", "build", "--release"]
        self._logger.info(
            "Building VTCode binary at %s using command: %s",
            self._host_vtcode_binary,
            " ".join(build_cmd),
        )
        subprocess.run(build_cmd, cwd=self._HOST_REPO_ROOT, check=True, capture_output=False)
        if not self._host_vtcode_binary.exists():
            raise FileNotFoundError(
                f"VTCode binary not found after build: {self._host_vtcode_binary}"
            )

    def _should_use_host_binary(self) -> bool:
        """Return True when the host binary can run inside the Linux container."""
        if platform.system().lower() != "linux":
            return False

        if not self._host_vtcode_binary.exists():
            return True

        try:
            with self._host_vtcode_binary.open("rb") as handle:
                signature = handle.read(4)
        except OSError:
            return False

        return signature == b"\x7fELF"

    def _build_binary_in_container(self, session: TmuxSession) -> None:
        """Compile VTCode inside the benchmark container to ensure compatibility."""
        build_cmd = "cargo build --release --bin vtcode"
        self._logger.info("Building VTCode inside container using command: %s", build_cmd)
        session.send_keys([build_cmd, "Enter"], block=True, max_timeout_sec=900.0)

    def _prepare_workspace(self, session: TmuxSession) -> None:
        """Copy configuration assets into the benchmark workspace."""
        workspace = self._workspace_path
        automation_dir = workspace / "automation"
        binary_dir = workspace / "target" / "release"

        use_host_binary = self._should_use_host_binary()
        if use_host_binary:
            self._ensure_host_binary()
        else:
            self._logger.info(
                "Skipping host-built binary: host platform is incompatible with Linux container"
            )

        session.send_keys([f"mkdir -p {workspace}", "Enter"], block=True, max_timeout_sec=10.0)
        session.send_keys([f"cd {workspace}", "Enter"], block=True, max_timeout_sec=10.0)
        session.send_keys(
            [f"mkdir -p {automation_dir}", "Enter"], block=True, max_timeout_sec=10.0
        )
        session.send_keys(
            [f"mkdir -p {binary_dir}", "Enter"], block=True, max_timeout_sec=10.0
        )

        if self._config_template is not None:
            session.copy_to_container(
                paths=[self._config_template],
                container_dir=str(workspace),
                container_filename="vtcode.toml",
            )

        if self._profile_template is not None:
            session.copy_to_container(
                paths=[self._profile_template],
                container_dir=str(automation_dir),
                container_filename=self._profile_template.name,
            )

        if use_host_binary:
            session.copy_to_container(
                paths=[self._host_vtcode_binary],
                container_dir=str(binary_dir),
                container_filename=self._host_vtcode_binary.name,
            )
        else:
            self._build_binary_in_container(session)
        session.send_keys([f"chmod +x {self._vtcode_binary}", "Enter"], block=True, max_timeout_sec=10.0)

    def _launch_agent(self, session: TmuxSession) -> None:
        """Start the VTCode process inside the tmux session."""
        command = (
            f"{self._vtcode_binary} "
            f"--workspace {self._workspace_path} "
            "--provider openrouter "
            "--model x-ai/grok-4-fast:free "
            "--full-auto --skip-confirmations --log-level info"
        )
        session.send_keys([command, "Enter"], block=False, min_timeout_sec=1.0)
        self._logger.info("Launched VTCode with command: %s", command)
        self._wait_for_boot()

    def _wait_for_boot(self) -> None:
        """Wait for VTCode to finish its startup sequence."""
        deadline = time.monotonic() + self._boot_timeout_sec
        while time.monotonic() < deadline:
            time.sleep(0.5)

    def _send_instruction(self, session: TmuxSession, instruction: str) -> None:
        """Send the benchmark instruction to the VTCode session."""
        normalised = " ".join(instruction.strip().split())
        if not normalised:
            return

        for chunk in self._chunk_text(normalised):
            session.send_keys([chunk], block=False, min_timeout_sec=0.05)
        session.send_keys(["Enter"], block=False, min_timeout_sec=0.1)

    def _monitor_session(self, session: TmuxSession) -> tuple[list[str], bool]:
        """Monitor the VTCode session output until completion or timeout."""
        outputs: list[str] = []
        deadline = time.monotonic() + self._run_timeout_sec
        while time.monotonic() < deadline:
            time.sleep(self._poll_interval_sec)
            try:
                chunk = session.get_incremental_output()
            except Exception as exc:  # pylint: disable=broad-except
                self._logger.debug("Failed to fetch terminal output: %s", exc)
                continue

            if not chunk:
                continue

            outputs.append(chunk)
            if self._detect_completion(chunk):
                return outputs, False

        return outputs, True

    def _detect_completion(self, output: str) -> bool:
        """Determine whether VTCode signalled task completion."""
        lowered = output.lower()
        if any(keyword in lowered for keyword in self._COMPLETION_KEYWORDS):
            return True

        progress_match = self._PROGRESS_PATTERN.search(output)
        if progress_match is not None:
            done, total = progress_match.groups()
            if done == total and done != "0":
                return True

        return False

    def _shutdown(self, session: TmuxSession) -> None:
        """Gracefully shut down the VTCode process."""
        session.send_keys([self._exit_command, "Enter"], block=False, min_timeout_sec=0.5)
        time.sleep(1.0)
        session.send_keys(["C-c"], block=False, min_timeout_sec=0.1)

    def _persist_logs(self, logging_dir: Path, outputs: list[str]) -> None:
        """Persist captured VTCode output to disk for later inspection."""
        logging_dir.mkdir(parents=True, exist_ok=True)
        log_path = logging_dir / "vtcode-session.log"
        log_path.write_text("\n".join(outputs), encoding="utf-8")

    @staticmethod
    def _chunk_text(text: str, chunk_size: int = 256) -> list[str]:
        """Break a string into tmux-friendly segments."""
        return [text[i : i + chunk_size] for i in range(0, len(text), chunk_size)]
