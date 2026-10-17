"""Offline, line-framed MCP peer; only synthetic benchmark input is accepted."""

import json
import sys
import time

tool_count = int(sys.argv[1])
delay_s = float(sys.argv[2]) / 1000 if len(sys.argv) > 2 else 0
tools = [
    {
        "name": f"widget_{index:04d}",
        "description": f"Synthetic widget tool {index}",
        "inputSchema": {
            "type": "object",
            "properties": {"text": {"type": "string"}},
            "required": ["text"],
        },
    }
    for index in range(tool_count)
]

for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request["method"]
    if method == "initialize":
        result = {
            "protocolVersion": request["params"]["protocolVersion"],
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "offline-fixture", "version": "1"},
        }
    elif method == "tools/list":
        result = {"tools": tools}
    elif method == "tools/call":
        if request["params"]["arguments"]["text"] == "disconnect":
            break
        if delay_s:
            time.sleep(delay_s)
        result = {
            "content": [{"type": "text", "text": request["params"]["arguments"]["text"]}],
            "isError": False,
        }
    elif method == "ping":
        result = {}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "Unknown fixture method"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)
