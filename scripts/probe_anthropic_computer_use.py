#!/usr/bin/env python3
"""Live acceptance probe for the Anthropic computer-use route (release gate).

P1: does the route accept tools=[{type:computer_toolset_20260801}] and emit
    tool_use blocks with toolset_name=computer?
P2: does a replay of the assistant tool_use blocks plus a real screenshot
    tool_result continue the conversation?
Prints statuses, block types, and usage only. Never prints credentials.

Usage:
    python3 scripts/probe_anthropic_computer_use.py <messages_url> <model> <key_env> <shot_b64>

Example (direct Anthropic; key must already be exported in this shell):
    python3 scripts/probe_anthropic_computer_use.py \\
        https://api.anthropic.com/v1/messages claude-sonnet-5 ANTHROPIC_API_KEY /tmp/shot.b64

On success the observed (provider, model, endpoint, protocol) tuple is the
candidate for `VERIFIED_MATRIX` in crates/codegen/vtcode-computer-use/src/gate.rs;
record the probe date beside the entry. The merge gateway mistranslates
provider-native tool declarations, so probe direct provider endpoints only.
"""

import json
import os
import sys
import urllib.request

PROTOCOL = "computer_toolset_20260801"
API_VERSION = "2026-10-21"

if len(sys.argv) != 5:
    raise SystemExit("usage: probe_anthropic_computer_use.py <messages_url> <model> <key_env> <shot_b64>")

BASE = sys.argv[1]
MODEL = sys.argv[2]
SHOT_B64 = sys.argv[4]


def load_key(name):
    key = os.environ.get(name)
    if key:
        return key
    try:
        with open(".env") as fh:
            for line in fh:
                line = line.strip()
                if line.startswith(name + "="):
                    return line.split("=", 1)[1].strip().strip("'\"")
    except OSError:
        pass
    raise SystemExit(f"missing {name}; export it or add it to the gitignored workspace .env")


def post(payload, key):
    data = json.dumps(payload).encode()
    req = urllib.request.Request(
        BASE,
        data=data,
        headers={
            "x-api-key": key,
            "anthropic-version": API_VERSION,
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=90) as resp:
            return resp.status, json.loads(resp.read().decode())
    except urllib.error.HTTPError as err:
        return err.code, {"_error_body": err.read().decode()[:1200]}


def tool_use_blocks(body):
    if not isinstance(body, dict):
        return []
    return [
        block
        for block in body.get("content", [])
        if isinstance(block, dict) and block.get("type") == "tool_use" and block.get("toolset_name") == "computer"
    ]


def main():
    key = load_key(sys.argv[3])
    with open(SHOT_B64) as fh:
        shot = fh.read().strip()

    print("=== P1: declare computer toolset ===", flush=True)
    status, body = post(
        {
            "model": MODEL,
            "max_tokens": 1024,
            "tools": [{"type": PROTOCOL}],
            "messages": [{"role": "user", "content": "Take a screenshot of the current screen."}],
        },
        key,
    )
    print("status:", status)
    if isinstance(body, dict):
        print("block types:", [b.get("type") for b in body.get("content", []) if isinstance(b, dict)])
        print("stop_reason:", body.get("stop_reason"))
        print("usage:", body.get("usage"))
    calls = tool_use_blocks(body)
    if status != 200 or not calls:
        print("P1 body:", json.dumps(body)[:2000])
        print("RESULT: route does not yield computer toolset blocks; stopping before P2")
        return 1
    print("ids:", [c.get("id") for c in calls], "| actions:", [c.get("name") for c in calls])

    print("=== P2: replay tool_use blocks with a real screenshot ===", flush=True)
    status2, body2 = post(
        {
            "model": MODEL,
            "max_tokens": 1024,
            "tools": [{"type": PROTOCOL}],
            "messages": [
                {"role": "user", "content": "Take a screenshot of the current screen."},
                {"role": "assistant", "content": calls},
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": calls[0].get("id"),
                            "toolset_name": "computer",
                            "is_error": False,
                            "content": [
                                {
                                    "type": "image",
                                    "source": {"type": "base64", "media_type": "image/png", "data": shot},
                                }
                            ],
                        }
                    ],
                },
            ],
        },
        key,
    )
    print("status:", status2)
    if isinstance(body2, dict):
        print("block types:", [b.get("type") for b in body2.get("content", []) if isinstance(b, dict)])
        print("stop_reason:", body2.get("stop_reason"))
        print("usage:", body2.get("usage"))
    if status2 != 200:
        print("P2 body:", json.dumps(body2)[:2000])
        print("RESULT: screenshot replay rejected")
        return 1
    print("RESULT: screenshot replay accepted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
