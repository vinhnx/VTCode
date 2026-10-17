#!/usr/bin/env python3
"""Build an isolated, local PGO candidate using an explicit training command."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import tomllib

from startup_env import isolated_environment


ROOT = Path(__file__).resolve().parents[2]


def output(command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def base_rustflags(host):
    # Match Cargo's environment precedence; retain the repository's target
    # linker flags when neither environment override is present.
    if "CARGO_ENCODED_RUSTFLAGS" in os.environ:
        flags = os.environ["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
    elif "RUSTFLAGS" in os.environ:
        flags = os.environ["RUSTFLAGS"].split()
    else:
        config = ROOT / ".cargo/config.toml"
        settings = tomllib.loads(config.read_text()) if config.exists() else {}
        target = settings.get("target", {}).get(host, {})
        variable = f"CARGO_TARGET_{host.upper().replace('-', '_')}_RUSTFLAGS"
        if variable in os.environ:
            flags = os.environ[variable].split()
        elif "rustflags" not in target and "CARGO_BUILD_RUSTFLAGS" in os.environ:
            flags = os.environ["CARGO_BUILD_RUSTFLAGS"].split()
        else:
            flags = target.get("rustflags", settings.get("build", {}).get("rustflags", []))
            if isinstance(flags, str):
                flags = flags.split()
    flags = [flag for flag in flags if flag]
    if any(key in flag for flag in flags for key in ("profile-generate", "profile-use", "codegen-units")):
        raise ValueError("remove existing PGO/codegen-units flags before running the experiment")
    return flags


def run_stage(name, command, environment, directory, cwd):
    log = directory / f"{name}.log"
    print(f"[pgo] {name}: {log}", flush=True)
    started = time.perf_counter()
    with log.open("w") as stream:
        subprocess.run(command, cwd=cwd, env=environment, stdout=stream, stderr=subprocess.STDOUT, check=True)
    return round(time.perf_counter() - started, 3)


def binary_record(binary):
    with binary.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return {
        "path": str(binary),
        "size_bytes": binary.stat().st_size,
        "sha256": digest,
    }


def experiment(directory, trainer):
    compiler = os.environ.get("RUSTC", "rustc")
    version = output([compiler, "-vV"])
    host = next(line.removeprefix("host: ") for line in version.splitlines() if line.startswith("host: "))
    sysroot = Path(output([compiler, "--print", "sysroot"]))
    suffix = ".exe" if os.name == "nt" else ""
    profdata = sysroot / "lib/rustlib" / host / "bin" / f"llvm-profdata{suffix}"
    if not profdata.is_file():
        raise ValueError("matching llvm-profdata is missing; run: rustup component add llvm-tools-preview")
    flags = base_rustflags(host)
    # Refuse reuse rather than deleting previous profiles or accepting stale data.
    directory.mkdir(parents=True, exist_ok=False)
    raw = directory / "raw"
    raw.mkdir()
    merged = directory / "merged.profdata"
    environment = os.environ.copy()
    environment.update(RUSTC_WRAPPER="", CARGO_BUILD_RUSTC_WRAPPER="", CARGO_INCREMENTAL="0",
                       CARGO_PROFILE_RELEASE_CODEGEN_UNITS="1")
    environment.pop("RUSTFLAGS", None)
    environment.pop("LLVM_PROFILE_FILE", None)
    summary = {
        "rustc": version,
        "revision": output(["git", "rev-parse", "HEAD"]),
        "worktree_status": output(["git", "status", "--porcelain"]),
        "target": host,
        "profile": "release",
        "base_rustflags": flags,
        "codegen_units": 1,
        "profile_overrides": {key: value for key, value in environment.items() if key.startswith("CARGO_PROFILE_")},
        "trainer": trainer,
        "build_seconds": {},
        "binaries": {},
    }

    def build(stage, extra_flags):
        target = directory / f"{stage}-target"
        stage_environment = environment.copy()
        stage_environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags + extra_flags)
        command = ["cargo", "build", "--locked", "--release", "--bin", "vtcode",
                   "--target", host, "--target-dir", str(target)]
        summary["build_seconds"][stage] = run_stage(stage, command, stage_environment, directory, ROOT)
        binary = target / host / "release" / f"vtcode{suffix}"
        summary["binaries"][stage] = binary_record(binary)
        return binary

    build("baseline", [])
    instrumented = build("generate", [f"-Cprofile-generate={raw}"])
    training_environment = isolated_environment(directory / "training")
    training_environment["LLVM_PROFILE_FILE"] = str(raw / "%m-%p.profraw")
    summary["training_seconds"] = run_stage(
        "train", trainer + [str(instrumented)], training_environment, directory, directory / "training/workspace"
    )
    profiles = list(raw.glob("*.profraw"))
    if not profiles or any(profile.stat().st_size == 0 for profile in profiles):
        raise ValueError(f"training did not produce nonempty profiles in {raw}")
    summary["profile_count"] = len(profiles)
    run_stage("merge", [str(profdata), "merge", "-o", str(merged), str(raw)], environment, directory, ROOT)
    build("use", [f"-Cprofile-use={merged}", "-Cllvm-args=-pgo-warn-missing-function"])
    (directory / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"[pgo] candidate and baseline ready: {directory / 'summary.json'}")
    print("[pgo] compare uninstrumented binaries before adopting PGO; training time is not a runtime benchmark")


def main():
    parser = argparse.ArgumentParser(description=__doc__, epilog="Trainer receives the instrumented binary as its final argument.")
    parser.add_argument("--output", type=Path, help="new artifact directory (default: .vtcode/perf/pgo-<UTC timestamp>)")
    parser.add_argument("trainer", nargs=argparse.REMAINDER, help="-- <trainer command> [arguments]")
    args = parser.parse_args()
    trainer = args.trainer
    if trainer and trainer[0] == "--":
        trainer = trainer[1:]
    if not trainer:
        parser.error("supply an explicit trainer after --")
    # The driver runs in an isolated workspace; resolve path-based commands
    # against the caller's cwd before changing directories. Other arguments
    # (such as a Python script path) must be absolute.
    if "/" in trainer[0] or "\\" in trainer[0]:
        trainer[0] = str(Path(trainer[0]).resolve())
    directory = args.output or ROOT / ".vtcode/perf" / datetime.now(timezone.utc).strftime("pgo-%Y%m%dT%H%M%S%fZ")
    try:
        experiment(directory.resolve(), trainer)
    except subprocess.CalledProcessError as error:
        print(f"[pgo] command failed ({error.returncode}); inspect the stage log above", file=sys.stderr)
        return error.returncode if error.returncode > 0 else 1
    except (OSError, ValueError) as error:
        print(f"[pgo] {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
