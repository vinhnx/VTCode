#!/usr/bin/env python3
"""Offline startup smoke training; does not cover interactive agent workloads."""

import subprocess
import sys


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: pgo_train_startup.py <instrumented-vtcode>")
    for arguments in (["--version"], ["--help"], ["schema", "tools"], ["tool-policy", "status"]):
        subprocess.run([sys.argv[1], *arguments], check=True, timeout=60)


if __name__ == "__main__":
    main()
