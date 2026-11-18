"""Replay a credential-free stream against release CLI and Desktop binaries."""

import argparse
import errno
import fcntl
import json
import os
import platform
import re
import resource
import select
import shlex
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
from contextlib import contextmanager, suppress
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

from macos_process_usage import child_pids, process_usage
from startup_env import isolated_environment

MODEL = "gpt-5.6-sol"
SEED_PROMPT = "Generate the fixture's restored history."
STREAM_START = b"Fixture stream begins"
STREAM_END = b"Fixture stream ends"
SEED_END = b"Fixture history ends"
COMMAND_START = ".fixture-command-start"
COMMAND_END = ".fixture-command-end"
COMMAND_PROMPT = "! python3 fixture-output.py"
WORKSPACE_FILE_COUNT = 2048
STREAM_CHUNKS = 280
CHUNK_INTERVAL = 0.05
CONTROL_SEQUENCE = re.compile(rb"\x1b\[[0-?]*[ -/]*[@-~]")
TERMINAL_QUERY = re.compile(rb"\x1b\[(6n|c)|\x1b(?:\[6?)?$")
TERMINAL_REPLY = {b"6n": b"\x1b[1;1R", b"c": b"\x1b[?1;2c"}


class Provider(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/v1/responses":
            self.send_error(404)
            return
        self.rfile.read(int(self.headers["Content-Length"]))
        seed = self.server.seed_lines > 0
        line_count = self.server.seed_lines if seed else STREAM_CHUNKS
        response_id = "resp_fixture"
        if seed:
            self.server.seed_requests += 1
            response_id = f"resp_seed_{self.server.seed_requests}"
        output_tokens = 800 if seed and line_count == 40 else 5000
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.end_headers()
        try:
            parts = [
                ("Fixture history begins" if seed else STREAM_START.decode()) + "\n"
            ]
            self.send_text(0, parts[0])
            if not seed:
                self.server.stream_started.set()
            deadline = time.monotonic()
            for index in range(line_count):
                if not seed:
                    deadline += CHUNK_INTERVAL
                    time.sleep(max(0, deadline - time.monotonic()))
                parts.append(
                    f"Replay line {index:04d} with **markdown** and a long line for resize and reflow.\n"
                )
                self.send_text(index + 1, parts[-1])
            parts.append((SEED_END if seed else STREAM_END).decode())
            self.send_text(line_count + 1, parts[-1])
            self.send_event(
                "response.completed",
                sequence_number=line_count + 2,
                response={
                    "id": response_id,
                    "object": "response",
                    "created_at": 0,
                    "model": MODEL,
                    "status": "completed",
                    "usage": {
                        "input_tokens": 100,
                        "output_tokens": output_tokens,
                        "total_tokens": 100 + output_tokens,
                    },
                    "output": [
                        {
                            "id": "msg_fixture",
                            "type": "message",
                            "role": "assistant",
                            "status": "completed",
                            "content": [
                                {
                                    "type": "output_text",
                                    "text": "".join(parts),
                                    "annotations": [],
                                }
                            ],
                        }
                    ],
                },
            )
        except (BrokenPipeError, ConnectionResetError):
            return
        finally:
            if not seed:
                self.server.stream_finished.set()

    def send_event(self, event_type, **fields):
        event = json.dumps(dict(type=event_type, **fields))
        self.wfile.write(f"data: {event}\n\n".encode())
        self.wfile.flush()

    def send_text(self, sequence_number, delta):
        self.send_event(
            "response.output_text.delta",
            sequence_number=sequence_number,
            item_id="msg_fixture",
            output_index=0,
            content_index=0,
            delta=delta,
        )

    def log_message(self, *_args):
        pass


def resize(fd, columns):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, columns, 0, 0))


class Terminal:
    def __init__(self, fd):
        self.fd = fd
        self.bytes_read = 0
        self.tail = b""
        self.recent_text = b""
        self.query_tail = b""

    def pump(self, duration, marker=None):
        deadline = time.monotonic() + duration
        self.tail = b""
        while (remaining := deadline - time.monotonic()) > 0:
            if not select.select([self.fd], [], [], remaining)[0]:
                break
            try:
                chunk = os.read(self.fd, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise
                break
            if not chunk:
                break
            self.bytes_read += len(chunk)
            self.recent_text = (self.recent_text + chunk)[-8192:]
            query = self.query_tail + chunk
            self.query_tail = b""
            for match in TERMINAL_QUERY.finditer(query):
                if match[1]:
                    os.write(self.fd, TERMINAL_REPLY[match[1]])
                else:
                    self.query_tail = match[0]
            self.tail = (self.tail + chunk)[-16384:]
            if marker and marker in CONTROL_SEQUENCE.sub(b"", self.tail):
                return
        if marker:
            raise RuntimeError(
                f"missing terminal marker {marker!r}. Tail: {CONTROL_SEQUENCE.sub(b'', self.tail)[-2000:]!r}"
            )


def fixture_environment(root, base_url, command_output, workspace_search):
    env = isolated_environment(root)
    env.update(
        OPENAI_API_KEY="fixture-key",
        OPENAI_BASE_URL=base_url,
        VTCODE_TRUST_WORKSPACE="full-auto",
        VTCODE_TRUST_WORKSPACE_QUIET="1",
    )
    config = f"""[agent]
provider = "openai"
default_model = "{MODEL}"
api_key_env = "OPENAI_API_KEY"
[agent.persistent_memory]
enabled = false
[tui]
alternate_screen = "always"
[ui.fullscreen]
copy_on_select = false
"""
    workspace = root / "workspace"
    for path in [
        Path(env["VTCODE_CONFIG_PATH"]),
        root / "config/vtcode.toml",
        workspace / "vtcode.toml",
    ]:
        path.write_text(config, encoding="utf-8")
    (root / "config/update.toml").write_text(
        "check_interval_hours = 0\n", encoding="utf-8"
    )
    if command_output:
        (workspace / "fixture-output.py").write_text(
            f"""import time
from pathlib import Path

Path({COMMAND_START!r}).touch()
deadline = time.monotonic()
for index in range(140):
    deadline += 0.1
    time.sleep(max(0, deadline - time.monotonic()))
    print(f"Fixture command line {{index:04d}} with sustained terminal output", flush=True)
Path({COMMAND_END!r}).touch()
""",
            encoding="utf-8",
        )
    if workspace_search:
        files = workspace / "fixture-files"
        files.mkdir()
        for index in range(WORKSPACE_FILE_COUNT):
            (files / f"unit-{index:04d}.txt").write_text(
                f"Fixture file {index:04d}\n", encoding="utf-8"
            )
    return env, workspace


def frame_metrics(root):
    return [
        {key: int(value) for key, value in re.findall(r"(\w+)=(\d+)", line)}
        for path in sorted(root.rglob("debug-*.log"))
        for line in path.read_text(errors="replace").splitlines()
        if "tui frame metrics window" in line
    ]


@contextmanager
def launch_pty(command, workspace, env):
    master, slave = os.openpty()
    terminal = Terminal(master)

    child = None
    try:
        resize(slave, 100)
        # Acquire the controlling terminal after starting a new session, so
        # SIGWINCH reaches VT Code. The provider runs on another thread.
        attach_terminal = (
            "import fcntl, os, sys, termios\n"
            "fcntl.ioctl(0, termios.TIOCSCTTY, 0)\n"
            "os.execv(sys.argv[1], sys.argv[1:])"
        )
        child = subprocess.Popen(
            [sys.executable, "-c", attach_terminal, *command],
            cwd=workspace,
            env=env,
            stdin=slave,
            stdout=slave,
            stderr=slave,
            start_new_session=True,
        )
        yield child, terminal, slave
    finally:
        try:
            if child is not None and child.poll() is None:
                with suppress(ProcessLookupError):
                    os.killpg(child.pid, signal.SIGTERM)
                # Let the TUI restore the terminal and flush buffered tracing before
                # closing the master, which can hang up the controlling terminal.
                terminal.pump(2)
        finally:
            os.close(master)
            os.close(slave)
            if child is not None:
                try:
                    child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    with suppress(ProcessLookupError):
                        os.killpg(child.pid, signal.SIGKILL)
                    child.wait()


def chat_command(binary):
    return [str(binary), "chat", "--provider", "openai", "--model", MODEL]


def submit(terminal, prompt):
    os.write(terminal.fd, prompt.encode())
    terminal.pump(0.25)
    os.write(terminal.fd, b"\r")


def seed_history(binary, workspace, env, server, desktop=False):
    # Desktop clips individual restored messages. Seed separate items there
    # so a large conversation exercises its history projection and scrolling.
    turns = 32 if desktop else 1
    server.seed_lines = 40 if desktop else 2000
    server.seed_requests = 0
    try:
        return record_history(binary, workspace, env, turns)
    finally:
        server.seed_lines = 0


def record_history(binary, workspace, env, turns):
    with launch_pty(chat_command(binary), workspace, env) as (child, terminal, _):
        terminal.pump(20, b"Type a request")
        terminal.pump(0.5)
        for turn in range(turns):
            submit(
                terminal,
                f"{SEED_PROMPT} Turn {turn + 1}." if turns > 1 else SEED_PROMPT,
            )
            terminal.pump(30, b";state=done:")
            terminal.pump(0.25)
        submit(terminal, "/exit")
        terminal.pump(8)
        if child.wait(timeout=2) != 0:
            raise RuntimeError(f"history seed exited with status {child.returncode}")
    sessions = list((workspace / ".vtcode/sessions").glob("*/manifest.json"))
    if len(sessions) != 1:
        raise RuntimeError(f"expected one seeded session, found {len(sessions)}")
    session = sessions[0].parent
    events = (session / "events.jsonl").read_bytes()
    if SEED_END not in events or events.count(b'"type":"turn.completed"') != turns:
        raise RuntimeError(f"seeded session did not persist {turns} completed turns")
    return session.name


def run(binary, root, base_url, server, args):
    env, workspace = fixture_environment(
        root, base_url, args.command_output, args.workspace_search
    )
    command = chat_command(binary)
    if args.restored:
        command = [
            str(binary),
            "--resume",
            seed_history(binary, workspace, env, server),
        ]
    server.stream_started.clear()
    server.stream_finished.clear()
    env.update(VTCODE_TUI_FRAME_METRICS="1", RUST_LOG="vtcode.tui.latency=debug")
    phases = {
        "restored": args.restored,
        "resize": not args.fixed_size,
        "mouse": not args.keyboard_only,
        "output": "command" if args.command_output else "assistant",
        "workspace_files": WORKSPACE_FILE_COUNT if args.workspace_search else 0,
    }
    process_samples = {}

    def record_process(phase, pid):
        if sys.platform == "darwin":
            process_samples[phase] = process_usage(pid)

    def output_started():
        return (
            (workspace / COMMAND_START).exists()
            if args.command_output
            else server.stream_started.is_set()
        )

    def output_finished():
        return (
            (workspace / COMMAND_END).exists()
            if args.command_output
            else server.stream_finished.is_set()
        )

    usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
    with launch_pty(command, workspace, env) as (child, terminal, slave):
        terminal.pump(20, b"Type a request")
        terminal.pump(1)
        record_process("idle_start", child.pid)
        start = terminal.bytes_read
        terminal.pump(5.5)
        phases["idle_bytes"] = terminal.bytes_read - start
        record_process("idle_end", child.pid)
        submit(
            terminal,
            COMMAND_PROMPT
            if args.command_output
            else "Replay the responsiveness fixture.",
        )
        if args.command_output:
            terminal.pump(10, b"1. Approve Once")
            if b"fixture-output.py" not in CONTROL_SEQUENCE.sub(b"", terminal.tail):
                raise RuntimeError("approval dialog did not show the fixture command")
            os.write(terminal.fd, b"\r")
        deadline = time.monotonic() + 20
        while not output_started():
            terminal.pump(0.05)
            if time.monotonic() >= deadline or child.poll() is not None:
                raise RuntimeError(
                    "fixture did not begin streaming. Terminal tail: "
                    f"{CONTROL_SEQUENCE.sub(b'', terminal.recent_text)[-2000:]!r}"
                )
        start = terminal.bytes_read
        for cycle in range(24):
            keys = b"abc\x7f\x1b[D\x1b[C"
            if not args.keyboard_only:
                keys += b"\x1b[<64;15;10M\x1b[<65;15;10M"
            os.write(terminal.fd, keys)
            if not args.fixed_size and cycle % 4 == 0:
                resize(slave, 76 if cycle % 8 == 0 else 100)
            if not args.keyboard_only and cycle % 4 == 2:
                os.write(terminal.fd, b"\x1b[<0;15;10M\x1b[<32;28;10M\x1b[<0;28;10m")
            if args.workspace_search and cycle == 4:
                os.write(terminal.fd, b" @fixture-files/unit-0")
            if args.workspace_search and cycle == 5:
                os.write(terminal.fd, b"\x1b")
            terminal.pump(0.5)
            if output_finished():
                raise RuntimeError("fixture stream ended before the input workload")
        if args.command_output:
            terminal.pump(5)
            if not output_finished():
                raise RuntimeError("fixture command did not finish")
        else:
            terminal.pump(5, b";state=done:")
        phases["stream_bytes"] = terminal.bytes_read - start
        record_process("output_end", child.pid)
        start = terminal.bytes_read
        terminal.pump(5.5)
        phases["recovery_bytes"] = terminal.bytes_read - start
        record_process("recovery_end", child.pid)
        if child.poll() is not None:
            raise RuntimeError(
                f"CLI exited during recovery with status {child.returncode}"
            )
    usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
    phases["child_cpu_seconds"] = round(
        (usage_after.ru_utime - usage_before.ru_utime)
        + (usage_after.ru_stime - usage_before.ru_stime),
        4,
    )
    phases["tui_frame_metrics"] = frame_metrics(root)
    if process_samples:
        phases["macos_process_usage"] = process_samples
    if len(phases["tui_frame_metrics"]) < 2 or not any(
        window.get("input_to_draw_max_us") for window in phases["tui_frame_metrics"]
    ):
        raise RuntimeError(
            "expected multiple frame metric windows and a nonzero input latency sample"
        )
    return phases


def run_native(binary, root, base_url, server, args):
    desktop = args.native == "desktop"
    env, workspace = fixture_environment(
        root, base_url, args.command_output, args.workspace_search
    )
    if args.command_output:
        if desktop:
            shell = root / "fixture-shell"
            shell.write_text(
                "#!/bin/sh\n"
                f"cd {shlex.quote(str(workspace))} || exit 1\n"
                "exec python3 fixture-output.py\n",
                encoding="utf-8",
            )
            shell.chmod(0o700)
            env["SHELL"] = str(shell)
            print(
                "Open Desktop's embedded terminal with Command+J to start output.",
                flush=True,
            )
        else:
            print(f"In the composer, run {COMMAND_PROMPT}", flush=True)
    if desktop:
        backend = (
            binary.parent.parent / "Helpers/vtcode"
            if binary.parent.name == "MacOS"
            else binary.with_name("vtcode")
        )
        if not backend.is_file():
            raise RuntimeError(f"Desktop requires the matching backend at {backend}")
    command = [str(binary)]
    if desktop:
        if args.restored:
            command.extend(
                [
                    "--session",
                    seed_history(backend, workspace, env, server, desktop=True),
                    "--workspace",
                    str(workspace),
                ]
            )
        else:
            print(f"Open the fixture workspace in Desktop: {workspace}", flush=True)
    else:
        env.update(
            TERM=os.environ.get("TERM", "xterm-256color"),
            VTCODE_TUI_FRAME_METRICS="1",
            RUST_LOG="vtcode.tui.latency=debug",
        )
        if args.restored:
            command.extend(["--resume", seed_history(binary, workspace, env, server)])
        else:
            command = chat_command(binary)
    if desktop and sys.platform == "darwin":
        with subprocess.Popen(command, cwd=workspace, env=env) as app:
            started = time.monotonic()
            while app.poll() is None:
                processes = {}
                for pid in [app.pid, *child_pids(app.pid)]:
                    # A backend can finish between the list and counter reads.
                    with suppress(ProcessLookupError):
                        processes[pid] = process_usage(pid)
                print(
                    json.dumps(
                        {
                            "elapsed_s": round(time.monotonic() - started, 3),
                            "processes": processes,
                        }
                    ),
                    flush=True,
                )
                try:
                    app.wait(timeout=1)
                except subprocess.TimeoutExpired:
                    continue
            if app.returncode:
                raise subprocess.CalledProcessError(app.returncode, command)
    else:
        subprocess.run(command, cwd=workspace, env=env, check=True)
    if not desktop:
        print(json.dumps({"tui_frame_metrics": frame_metrics(root)}))


@contextmanager
def native_fixture_directory():
    directory = tempfile.TemporaryDirectory(prefix="vtcode-responsive-")
    try:
        yield Path(directory.name)
    finally:
        # The Desktop backend can finish writing its archive after the GUI has
        # exited. Retry the isolated directory cleanup if a file appears mid-walk.
        deadline = time.monotonic() + 3
        while True:
            try:
                directory.cleanup()
                break
            except OSError as error:
                if error.errno != errno.ENOTEMPTY or time.monotonic() >= deadline:
                    raise
                time.sleep(0.05)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "binary",
        type=Path,
        nargs="+",
        help="release binaries, in alternating run order",
    )
    parser.add_argument("--rounds", type=int, default=1)
    parser.add_argument(
        "--restored", action="store_true", help="seed and reopen a long session"
    )
    parser.add_argument(
        "--fixed-size", action="store_true", help="omit size changes for a control run"
    )
    parser.add_argument(
        "--keyboard-only",
        action="store_true",
        help="omit mouse events for a control run",
    )
    parser.add_argument(
        "--command-output",
        action="store_true",
        help="exercise sustained local command output",
    )
    parser.add_argument(
        "--workspace-search",
        action="store_true",
        help="populate the workspace for file search",
    )
    parser.add_argument(
        "--native",
        choices=("desktop", "terminal"),
        help="launch one fixture for native interaction",
    )
    args = parser.parse_args()
    if args.rounds < 1:
        parser.error("--rounds must be positive")
    if args.native and (
        len(args.binary) != 1
        or args.rounds != 1
        or args.fixed_size
        or args.keyboard_only
    ):
        parser.error(
            "--native requires one binary and one round. Drive resize manually."
        )
    # Bind before starting the driver. The provider never contacts an upstream service.
    with ThreadingHTTPServer(("127.0.0.1", 0), Provider) as server:
        server.seed_lines = 0
        server.seed_requests = 0
        server.stream_started = threading.Event()
        server.stream_finished = threading.Event()
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            base_url = f"http://127.0.0.1:{server.server_port}/v1"
            if args.native:
                with native_fixture_directory() as directory:
                    run_native(
                        args.binary[0].resolve(strict=True),
                        directory,
                        base_url,
                        server,
                        args,
                    )
                return
            print(
                json.dumps(
                    {"os": platform.platform(), "terminal": "xterm-256color PTY"}
                ),
                flush=True,
            )
            for round_index in range(args.rounds):
                for binary in args.binary:
                    with tempfile.TemporaryDirectory(
                        prefix="vtcode-responsive-"
                    ) as directory:
                        result = run(
                            binary.resolve(strict=True),
                            Path(directory),
                            base_url,
                            server,
                            args,
                        )
                    print(
                        json.dumps(
                            dict(round=round_index + 1, binary=str(binary), **result)
                        ),
                        flush=True,
                    )
        finally:
            server.shutdown()
            worker.join()


if __name__ == "__main__":
    main()
