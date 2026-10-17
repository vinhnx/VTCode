"""Credential-free temporary environments for the local startup harness."""

import os
from pathlib import Path


def isolated_environment(directory):
    root = Path(directory).resolve()
    root.mkdir(parents=True, exist_ok=True)
    for name in ("home", "config", "data", "state", "cache", "runtime", "codex", "tmp", "workspace"):
        (root / name).mkdir(mode=0o700, exist_ok=True)
    config = root / "fixture.toml"
    config.write_text("", encoding="utf-8")
    environment = {"PATH": os.environ.get("PATH", os.defpath)}
    environment.update(
        HOME=str(root / "home"),
        TMPDIR=str(root / "tmp"),
        VTCODE_CONFIG=str(root / "config"),
        VTCODE_DATA=str(root / "data"),
        VTCODE_HOME=str(root / "data"),
        VTCODE_CONFIG_PATH=str(config),
        XDG_CONFIG_HOME=str(root / "config"),
        XDG_DATA_HOME=str(root / "data"),
        XDG_STATE_HOME=str(root / "state"),
        XDG_CACHE_HOME=str(root / "cache"),
        XDG_RUNTIME_DIR=str(root / "runtime"),
        CODEX_HOME=str(root / "codex"),
        OLLAMA_BASE_URL="http://127.0.0.1:1",
        VTCODE_STARTUP_TRACE="0",
        NO_COLOR="1",
        TERM="xterm-256color",
        LANG="C",
        LC_ALL="C",
    )
    return environment
