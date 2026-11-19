#!/usr/bin/env python3
"""Live acceptance probe for the OpenAI computer-use route (release gate).

P1: does the route accept tools=[{type:computer}] and emit computer_call?
P2: does stateless replay (no previous_response_id, store=false) continue
    with a real screenshot captured from the isolated browser container?
Prints statuses, item types, and usage only. Never prints credentials.

Usage:
    printf '{"kind":"screenshot"}\\n' | docker run ... (see crate README) \\
        | python3 -c '...'  # save base64 screenshot to SHOT_B64
    python3 scripts/probe_openai_computer_use.py <base_url> <model> <key_env> <shot_b64>

Example (direct OpenAI; key must already be exported in this shell):
    python3 scripts/probe_openai_computer_use.py \\
        https://api.openai.com/v1/responses gpt-6-luna OPENAI_API_KEY /tmp/shot.b64

Known result (2026-11-19): Merge Gateway mistranslates {type:computer}
into a Bedrock toolSpec and rejects it, so the gateway is unsuitable;
probe direct provider endpoints only.
"""

import json
import os
import sys
import urllib.request

if len(sys.argv) != 5:
    raise SystemExit("usage: probe_openai_computer_use.py <base_url> <model> <key_env> <shot_b64>")

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
        headers={"Authorization": f"Bearer {key}", "Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=90) as resp:
            return resp.status, json.loads(resp.read().decode())
    except urllib.error.HTTPError as err:
        return err.code, {"_error_body": err.read().decode()[:1200]}


def main():
    key = load_key(sys.argv[3])
    with open(SHOT_B64) as fh:
        shot = fh.read().strip()

    print("=== P1: declare computer tool ===", flush=True)
    status, body = post(
        {
            "model": MODEL,
            "store": False,
            "tools": [{"type": "computer"}],
            "input": "Take a screenshot of the current screen.",
        },
        key,
    )
    print("status:", status)
    items = body.get("output", []) if isinstance(body, dict) else []
    print("output types:", [i.get("type") for i in items if isinstance(i, dict)])
    print("usage:", body.get("usage") if isinstance(body, dict) else None)
    calls = [i for i in items if isinstance(i, dict) and i.get("type") == "computer_call"]
    if status != 200 or not calls:
        print("P1 body:", json.dumps(body)[:2000])
        print("RESULT: route does not yield computer_call; stopping before P2")
        return 1
    call = calls[0]
    print("call_id:", call.get("call_id"), "| actions:", [a.get("type") for a in call.get("actions", [])])

    print("=== P2: stateless replay without previous_response_id ===", flush=True)
    status2, body2 = post(
        {
            "model": MODEL,
            "store": False,
            "tools": [{"type": "computer"}],
            "input": [
                call,
                {
                    "type": "computer_call_output",
                    "call_id": call.get("call_id"),
                    "output": {
                        "type": "computer_screenshot",
                        "image_url": f"data:image/png;base64,{shot}",
                    },
                },
            ],
        },
        key,
    )
    print("status:", status2)
    items2 = body2.get("output", []) if isinstance(body2, dict) else []
    print("output types:", [i.get("type") for i in items2 if isinstance(i, dict)])
    print("usage:", body2.get("usage") if isinstance(body2, dict) else None)
    if status2 != 200:
        print("P2 body:", json.dumps(body2)[:2000])
        print("RESULT: stateless replay rejected")
        return 1
    print("RESULT: stateless replay accepted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
