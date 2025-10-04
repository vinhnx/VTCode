from __future__ import annotations

import json
import os
import shlex
from enum import Enum
from pathlib import Path
from typing import Final

from terminal_bench.agents.installed_agents.abstract_installed_agent import (
    AbstractInstalledAgent,
)
from terminal_bench.terminal.models import TerminalCommand


class EnvVar(Enum):
    AUTOMATION_SEQUENCE = 'VTCODE_AUTOMATION_INPUT_SEQUENCE'
    CONFIG_PATH = 'VTCODE_CONFIG_PATH'


class ApiKeyEnv(Enum):
    ANTHROPIC = 'ANTHROPIC_API_KEY'
    GEMINI = 'GEMINI_API_KEY'
    OPENAI = 'OPENAI_API_KEY'
    DEEPSEEK = 'DEEPSEEK_API_KEY'
    OPENROUTER = 'OPENROUTER_API_KEY'
    XAI = 'XAI_API_KEY'


class ExecutionFlag(Enum):
    FULL_AUTO = '--full-auto'
    NO_COLOR = '--no-color'
    SKIP_CONFIRMATIONS = '--skip-confirmations'


class Subcommand(Enum):
    CHAT = 'chat'


class CliOption(Enum):
    CONFIG = '--config'


class Binary(Enum):
    VTCODE = 'vtcode'


class AutomationToken(Enum):
    EXIT = 'exit'


class VTCodeTerminalBenchAgent(AbstractInstalledAgent):
    """Terminal-Bench integration that runs vtcode in full-auto mode."""

    INSTALL_SCRIPT_NAME: Final[str] = 'setup.sh'

    def __init__(self, config_path: str | None = None, *args, **kwargs) -> None:
        super().__init__(*args, **kwargs)
        self._config_path = Path(config_path).expanduser() if config_path else None

    @staticmethod
    def name() -> str:
        return 'VT Code Terminal-Bench Agent'

    @property
    def _env(self) -> dict[str, str]:
        env: dict[str, str] = {}
        for api_key in ApiKeyEnv:
            value = os.environ.get(api_key.value)
            if value:
                env[api_key.value] = value
        if self._config_path:
            env[EnvVar.CONFIG_PATH.value] = str(self._config_path)
        return env

    @property
    def _install_agent_script_path(self) -> Path:
        return Path(__file__).parent / self.INSTALL_SCRIPT_NAME

    def _run_agent_commands(self, instruction: str) -> list[TerminalCommand]:
        command_sequence = json.dumps([instruction, AutomationToken.EXIT.value])
        export_sequence = TerminalCommand(
            command=self._format_export(EnvVar.AUTOMATION_SEQUENCE.value, command_sequence),
            block=True,
            min_timeout_sec=0.1,
        )
        vtcode_command = TerminalCommand(
            command=self._build_vtcode_command(),
            block=True,
            min_timeout_sec=0.1,
            max_timeout_sec=float('inf'),
        )
        return [export_sequence, vtcode_command]

    @staticmethod
    def _format_export(key: str, value: str) -> str:
        return f"export {key}={shlex.quote(value)}"

    def _build_vtcode_command(self) -> str:
        parts: list[str] = [Binary.VTCODE.value]
        if self._config_path:
            parts.extend([CliOption.CONFIG.value, shlex.quote(str(self._config_path))])
        parts.extend(flag.value for flag in ExecutionFlag)
        parts.append(Subcommand.CHAT.value)
        return ' '.join(parts)
