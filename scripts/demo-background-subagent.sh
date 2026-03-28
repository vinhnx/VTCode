#!/usr/bin/env bash

set -euo pipefail

shutdown() {
  printf '[demo-background-subagent] shutting down\n'
  exit 0
}

trap shutdown INT TERM

printf '[demo-background-subagent] ready pid=%s started_at=%s\n' "$$" "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

while true; do
  printf '[demo-background-subagent] heartbeat pid=%s ts=%s\n' "$$" "$(date -u +%H:%M:%S)"
  sleep 5
done
