#!/usr/bin/env python3
"""Measure rebuilt binaries in an isolated PTY; no provider calls are made."""
import argparse
import errno
import fcntl
import json
import os
from pathlib import Path
import select
import signal
import shlex
import struct
import subprocess
import tempfile
import termios
import time


def run(binary, scenario, enforce=True):
    with tempfile.TemporaryDirectory(prefix="vtcode-exit-") as directory:
        workspace = Path(directory)
        config_dir = workspace / "config"
        config_dir.mkdir()
        config = '''[agent]
provider = "openai"
default_model = "gpt-5"
api_key_env = "OPENAI_API_KEY"
[agent.persistent_memory]
enabled = false
[tui]
alternate_screen = "always"
'''
        base_config = config
        if scenario in ("stalled_cleanup", "initialization"):
            event = "session_end" if scenario == "stalled_cleanup" else "session_start"
            config += f'''\n[[hooks.lifecycle.{event}]]
hooks = [{{ command = "echo $$ > hook.pid; exec sleep 30", timeout_seconds = 60 }}]
'''
        (config_dir / "vtcode.toml").write_text(config)
        (workspace / "vtcode.toml").write_text(base_config)
        env = os.environ.copy()
        env.update(TERM="xterm-256color", OPENAI_API_KEY="fixture-key", OPENAI_BASE_URL="http://127.0.0.1:9/v1",
                   VTCODE_CONFIG=str(config_dir), VTCODE_DATA=str(workspace / "data"),
                   XDG_CONFIG_HOME=str(config_dir), VTCODE_TRUST_WORKSPACE="full-auto",
                   VTCODE_TRUST_WORKSPACE_QUIET="1", NO_COLOR="1")
        master, slave = os.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
        cooked = termios.tcgetattr(slave)

        def terminal_session():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

        command = shlex.join([str(binary), "chat", "--provider", "openai", "--model", "gpt-5", "--dangerously-skip-permissions"])
        # Keep the controlling shell alive while checking termios. macOS revokes
        # the slave when its session leader exits, making late ioctls invalid.
        child = subprocess.Popen(["sh", "-c", command + '; fixture_code=$?; printf "\\n__VTCODE_EXIT__:%s\\n" "$fixture_code"; sleep 1; exit "$fixture_code"'],
                                 cwd=workspace, env=env, stdin=slave, stdout=slave, stderr=slave, preexec_fn=terminal_session)
        output = bytearray()

        def pump(duration):
            until = time.monotonic() + duration
            while time.monotonic() < until:
                ready, _, _ = select.select([master], [], [], min(0.01, max(0, until - time.monotonic())))
                if ready:
                    try:
                        chunk = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            return
                        raise
                    output.extend(chunk)
                    if b"\x1b[6n" in chunk:
                        os.write(master, b"\x1b[1;1R")
                    if b"\x1b[c" in chunk:
                        os.write(master, b"\x1b[?1;2c")
                if child.poll() is not None:
                    return

        try:
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                pump(0.05)
                if child.poll() is not None:
                    raise AssertionError("binary exited before PTY cancellation: " + output.decode(errors="replace")[-1500:])
                raw = not termios.tcgetattr(slave)[3] & termios.ICANON
                if scenario == "initialization":
                    if (workspace / "hook.pid").exists():
                        break
                elif raw:
                    pump(1)
                    break
            else:
                raise AssertionError("initialization fixture never entered its stalled hook: " + output.decode(errors="replace")[-1500:])
            os.write(master, b"\x03")
            pump(0.15)
            assert child.poll() is None, "first Ctrl+C must cancel, leaving the session open"
            accepted = time.monotonic()
            os.write(master, b"\x03")
            while b"__VTCODE_EXIT__:" not in output and time.monotonic() - accepted < 4:
                if scenario == "events":
                    os.write(master, b"\x1b[I\x1b[O")
                pump(0.01)
            elapsed = time.monotonic() - accepted
            if b"__VTCODE_EXIT__:" not in output:
                raise AssertionError(f"exit did not terminate within 4s ({scenario})")
            pump(0.05)
            restored = termios.tcgetattr(slave)
            for bit in (termios.ICANON, termios.ECHO, termios.ISIG):
                assert restored[3] & bit == cooked[3] & bit, "terminal was not restored to cooked mode"
            text = output.decode(errors="replace")
            # The postamble has one complete stats line after the TUI leaves.
            postamble = text.rsplit("\x1b[?1049l", 1)[-1]
            assert postamble.count("VT Code") == 1, "one exit summary must survive terminal restoration: " + postamble[-1000:]
            if scenario in ("stalled_cleanup", "initialization"):
                marker = workspace / "hook.pid"
                assert marker.exists(), "stalled hook must run to exercise cleanup: " + text[-2500:]
                pid = int(marker.read_text())
                try:
                    os.kill(pid, 0)
                except ProcessLookupError:
                    pass
                else:
                    raise AssertionError("hook child survived process termination")
            if enforce:
                assert elapsed < 2, f"accepted exit to shell return took {elapsed:.3f}s"
            return {"scenario": scenario, "exit_ms": round(elapsed * 1000, 1), "cooked": True, "postamble": 1}
        finally:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait()
            os.close(master)
            os.close(slave)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--scenario", choices=["idle", "events", "stalled_cleanup", "initialization"], action="append")
    args = parser.parse_args()
    for scenario in args.scenario or ["idle", "events", "stalled_cleanup", "initialization"]:
        for pair in range(args.runs):
            for label, binary in (("baseline", args.baseline), ("candidate", args.candidate)):
                if binary is not None:
                    try:
                        result = run(binary.resolve(), scenario, enforce=label == "candidate")
                    except AssertionError as error:
                        if label == "candidate":
                            raise
                        result = {"scenario": scenario, "failure": str(error)}
                    print(json.dumps({"binary": label, "pair": pair + 1, **result}), flush=True)


if __name__ == "__main__":
    main()
