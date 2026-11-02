"""Replace the local plugin launcher with the selected VT Code process."""

import json
import os
import sys
from pathlib import Path


def companion_config_path():
    """Match VT Code's native config directory for companion.json."""
    configured = os.environ.get("VTCODE_CONFIG", "").strip()
    if configured:
        path = Path(configured)
        if not path.is_absolute():
            raise ValueError("VTCODE_CONFIG must be an absolute path")
        return path / "companion.json"

    if os.name == "nt":
        root = os.environ.get("APPDATA")
        if not root:
            raise ValueError("APPDATA is unavailable; run vtcode companion setup")
        return Path(root) / "vinhnx" / "vtcode" / "config" / "companion.json"

    home = Path.home()
    if sys.platform == "darwin":
        config_dir = home / "Library" / "Application Support" / "com.vinhnx.vtcode"
    else:
        xdg = os.environ.get("XDG_CONFIG_HOME", "").strip()
        xdg_path = Path(xdg) if xdg else None
        config_dir = (
            xdg_path / "vtcode"
            if xdg_path is not None and xdg_path.is_absolute()
            else home / ".config" / "vtcode"
        )
    return config_dir / "companion.json"


def read_user_configuration():
    path = companion_config_path()
    try:
        if path.is_symlink() or path.stat().st_size > 16 * 1024:
            raise ValueError(
                "Companion settings are a symlink or exceed the size limit"
            )
        configuration = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as error:
        raise ValueError(
            "Companion is not configured; run `vtcode --workspace <path> companion setup --binary <path>`"
        ) from error
    if not isinstance(configuration, dict) or set(configuration) != {
        "binary",
        "workspace",
    }:
        raise ValueError(
            "Companion settings must contain only binary and workspace paths"
        )
    if not all(isinstance(configuration[key], str) for key in ("binary", "workspace")):
        raise ValueError("Companion binary and workspace settings must be paths")
    if not all(
        Path(configuration[key]).is_absolute() for key in ("binary", "workspace")
    ):
        raise ValueError(
            "Companion binary and workspace settings must be absolute paths"
        )
    binary = Path(configuration["binary"]).resolve(strict=True)
    workspace = Path(configuration["workspace"]).resolve(strict=True)
    return binary, workspace


def configured_target():
    """Return the legacy fixed pair or this user's saved setup."""
    if len(sys.argv) == 3:
        binary = Path(sys.argv[1]).resolve(strict=True)
        workspace = Path(sys.argv[2]).resolve(strict=True)
    elif len(sys.argv) == 1:
        binary, workspace = read_user_configuration()
    else:
        raise ValueError("expected either no arguments or a binary and workspace")

    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("VT Code executable is unavailable")
    if not workspace.is_dir():
        raise ValueError("VT Code workspace is unavailable")

    return binary, workspace


def main():
    binary, workspace = configured_target()
    os.execv(
        binary,
        [
            str(binary),
            "--workspace",
            str(workspace),
            "--quiet",
            "webmcp",
            "stdio",
            "--companion",
        ],
    )


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError):
        sys.exit(
            "Local VT Code Companion could not start; run `vtcode companion setup` and check the saved binary and workspace."
        )
