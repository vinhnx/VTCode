#!/usr/bin/env python3
"""Minimal newline-delimited JSON-RPC MCP mock server for integration tests."""

from __future__ import annotations

import json
import sys
from typing import Any


def send(message: dict[str, Any]) -> None:
    sys.stdout.write(json.dumps(message))
    sys.stdout.write("\n")
    sys.stdout.flush()


def initialize_result(protocol_version: str) -> dict[str, Any]:
    return {
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {
                "listChanged": False,
            }
        },
        "serverInfo": {
            "name": "vtcode-mock-mcp",
            "version": "0.1.0",
        },
    }


def list_tools_result() -> dict[str, Any]:
    return {
        "tools": [
            {
                "name": "echo",
                "description": "Echoes back the provided message",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                        }
                    },
                    "required": ["message"],
                },
            }
        ]
    }


def call_tool_result(method_params: dict[str, Any]) -> dict[str, Any]:
    tool_name = method_params.get("name")
    arguments = method_params.get("arguments") or {}

    if tool_name != "echo":
        return {
            "isError": True,
            "content": [
                {
                    "type": "text",
                    "text": f"unknown tool: {tool_name}",
                }
            ],
        }

    message = str(arguments.get("message", ""))
    return {
        "content": [
            {
                "type": "text",
                "text": f"echo:{message}",
            }
        ]
    }


for raw_line in sys.stdin:
    line = raw_line.strip()
    if not line:
        continue

    try:
        request = json.loads(line)
    except json.JSONDecodeError:
        continue

    method = request.get("method")
    request_id = request.get("id")
    params = request.get("params") or {}

    # Notifications have no id and require no response.
    if request_id is None:
        continue

    if method == "initialize":
        protocol = str(params.get("protocolVersion", "2024-11-05"))
        send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": initialize_result(protocol),
            }
        )
    elif method == "tools/list":
        send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": list_tools_result(),
            }
        )
    elif method == "tools/call":
        send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "result": call_tool_result(params),
            }
        )
    elif method == "ping":
        send({"jsonrpc": "2.0", "id": request_id, "result": {}})
    else:
        send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "error": {
                    "code": -32601,
                    "message": f"Method not found: {method}",
                },
            }
        )
