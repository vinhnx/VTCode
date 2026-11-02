#!/usr/bin/env python3
"""Package a local stdio companion, or an authenticated remote connection."""

import argparse
import json
import os
import re
import struct
import tempfile
import zipfile
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import urlsplit, urlunsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "plugins" / "vtcode-companion"
SCHEMA_ROOT = "https://agent-plugins.org/schemas/1.0.0/"
EXPECTED_TOOLS = {"search", "fetch", "start_task", "get_task", "cancel_task"}
MAX_RESPONSE_BYTES = 1024 * 1024


def package_image(reference, minimum_side, square):
    """Read a contained PNG; icons are square, gallery images may be wide."""
    if (
        not isinstance(reference, str)
        or not reference.startswith("./assets/")
        or "\\" in reference
        or ".." in Path(reference).parts
    ):
        raise ValueError("image paths must stay inside ./assets/")
    source = SOURCE.resolve()
    path = source / reference
    if path.resolve(strict=True) != path or not path.is_file():
        raise ValueError("package images must be regular files without symlinks")
    if path.stat().st_size > 5 * 1024 * 1024:
        raise ValueError("package images must not exceed 5 MiB")
    data = path.read_bytes()
    if (
        len(data) < 33
        or not data.startswith(b"\x89PNG\r\n\x1a\n")
        or data[12:16] != b"IHDR"
    ):
        raise ValueError("package images must be PNG files")
    width, height = struct.unpack(">II", data[16:24])
    if (
        not minimum_side <= width <= 4096
        or not minimum_side <= height <= 4096
        or (square and width != height)
    ):
        raise ValueError("package image dimensions do not match its role")
    return path.relative_to(source).as_posix(), data


class NoRedirect(HTTPRedirectHandler):
    """Never forward bearer credentials through an HTTP redirect."""

    def redirect_request(self, request, fp, code, message, headers, new_url):
        return None


def https_url(value):
    if not isinstance(value, str) or not value:
        raise ValueError("endpoint URL must be a nonempty string")
    parsed = urlsplit(value)
    if (
        parsed.scheme != "https"
        or not parsed.hostname
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
    ):
        raise ValueError(
            "endpoint URLs must be HTTPS without credentials, query, or fragment"
        )
    return parsed


def request_json(url, payload=None, token=None):
    headers = {"Accept": "application/json, text/event-stream"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    data = None
    if payload is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(payload).encode("utf-8")
    request = Request(url, data=data, headers=headers)
    with build_opener(NoRedirect).open(request, timeout=15) as response:
        body = response.read(MAX_RESPONSE_BYTES + 1)
        if len(body) > MAX_RESPONSE_BYTES:
            raise ValueError("endpoint response exceeds byte limit")
        result = json.loads(body)
        if not isinstance(result, dict):
            raise TypeError("endpoint must return a JSON object")
        return result


def rpc(method, params=None):
    result = {"jsonrpc": "2.0", "id": 1, "method": method}
    if params is not None:
        result["params"] = params
    return result


def verify_endpoint(url, token):
    parsed = https_url(url)
    if parsed.path.rstrip("/") != "/mcp":
        raise ValueError("use the VT Code Streamable HTTP /mcp endpoint")
    metadata_url = urlunsplit(
        (parsed.scheme, parsed.netloc, "/.well-known/oauth-protected-resource", "", "")
    )
    metadata = request_json(metadata_url)
    resource = metadata.get("resource", "")
    resource_url = https_url(resource)
    if resource_url.netloc != parsed.netloc:
        raise ValueError("OAuth resource must belong to the MCP endpoint origin")
    servers = metadata.get("authorization_servers")
    if not isinstance(servers, list) or not servers:
        raise ValueError("OAuth authorization server metadata is missing")
    for server in servers:
        https_url(server)
    initialize = rpc(
        "initialize",
        {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "vtcode-companion-package-check",
                "version": "0.1.0",
            },
        },
    )
    try:
        request_json(url, initialize)
    except HTTPError as error:
        if error.code != 401:
            raise ValueError(
                "unauthenticated MCP access must return HTTP 401"
            ) from error
    else:
        raise ValueError("refusing to package an unauthenticated coding service")
    initialized = request_json(url, initialize, token)
    result = initialized.get("result")
    if (
        not isinstance(result, dict)
        or not isinstance(result.get("capabilities"), dict)
        or not isinstance(result["capabilities"].get("tools"), dict)
    ):
        raise TypeError("authenticated MCP initialization failed")
    listing = request_json(url, rpc("tools/list"), token)
    result = listing.get("result")
    tools = result.get("tools") if isinstance(result, dict) else None
    if not isinstance(tools, list):
        raise TypeError("MCP tool discovery failed")
    if any(
        not isinstance(tool, dict) or not isinstance(tool.get("name"), str)
        for tool in tools
    ):
        raise TypeError("MCP tool entries must be objects with string names")
    names = {tool["name"] for tool in tools}
    if names != EXPECTED_TOOLS or len(tools) != len(EXPECTED_TOOLS):
        raise ValueError("endpoint must advertise exactly the five companion tools")
    for tool in tools:
        if tool["name"] in {"start_task", "cancel_task"}:
            hints = tool.get("annotations", {})
            if not isinstance(hints, dict):
                raise TypeError("MCP tool annotations must be an object")
            if (
                hints.get("readOnlyHint") is not False
                or hints.get("destructiveHint") is not True
            ):
                raise ValueError(
                    "task actions require conservative approval annotations"
                )
    return resource


def package(url, resource, output):
    mcp = {
        "$schema": SCHEMA_ROOT + "mcp.schema.json",
        "mcpServers": {
            "vtcode": {
                "type": "streamable-http",
                "url": url,
                "extensions": {
                    "com.openai": {"auth": {"type": "oauth", "resource": resource}}
                },
            }
        },
    }
    return write_package(mcp, output)


def package_local(binary, workspace, output):
    binary = binary.resolve(strict=True)
    workspace = workspace.resolve(strict=True)
    if not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("local binary must be executable")
    if not workspace.is_dir():
        raise ValueError("local workspace must be a directory")
    mcp = {
        "$schema": SCHEMA_ROOT + "mcp.schema.json",
        "mcpServers": {
            "vtcode": {
                "type": "stdio",
                "command": "python3",
                "args": [
                    "${PLUGIN_ROOT}/server/launch.py",
                    str(binary),
                    str(workspace),
                ],
                "cwd": "${PLUGIN_ROOT}",
            }
        },
    }
    return write_package(mcp, output)


def prepare_marketplace(binary, workspace, destination):
    """Generate a fresh local source; never install or replace a marketplace."""
    destination = destination.absolute()
    if destination.is_symlink() or destination.exists():
        raise ValueError("marketplace already exists; choose a new directory")
    if destination.resolve().is_relative_to(SOURCE.resolve()):
        raise ValueError("write the marketplace outside the plugin source")
    # Validate and assemble through the same archive path as ZIP packaging.
    with tempfile.TemporaryDirectory() as directory:
        archive_path = package_local(binary, workspace, Path(directory) / "local.zip")
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.mkdir(mode=0o700)
        plugin = destination / "plugins" / "vtcode-companion"
        with zipfile.ZipFile(archive_path) as archive:
            for name in archive.namelist():
                path = plugin / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(archive.read(name))
        catalog = {
            "name": "vtcode-local",
            "interface": {"displayName": "VT Code Local"},
            "plugins": [
                {
                    "name": "vtcode-companion",
                    "source": {"source": "local", "path": "./plugins/vtcode-companion"},
                    "policy": {
                        "installation": "AVAILABLE",
                        "authentication": "ON_INSTALL",
                    },
                    "category": "Developer Tools",
                }
            ],
        }
        catalog_path = destination / ".agents" / "plugins" / "marketplace.json"
        catalog_path.parent.mkdir(parents=True)
        catalog_path.write_text(json.dumps(catalog, indent=2) + "\n", encoding="utf-8")
    return destination


def write_package(mcp, output):
    output = output.resolve()
    if output.is_relative_to(SOURCE.resolve()):
        raise ValueError("write the ZIP outside the plugin source directory")
    if output.exists():
        raise ValueError("output already exists; choose a new archive path")
    manifest = json.loads((SOURCE / "plugin.json").read_text(encoding="utf-8"))
    if (
        manifest["$schema"] != SCHEMA_ROOT + "plugin.schema.json"
        or manifest["name"] != "vtcode-companion"
        or re.fullmatch(r"(0|[1-9][0-9]*)(\.(0|[1-9][0-9]*)){2}", manifest["version"])
        is None
    ):
        raise ValueError("invalid companion manifest identity or schema")
    interface = manifest["extensions"]["com.openai"]["interface"]
    if len(interface["shortDescription"]) > 30:
        raise ValueError("plugin subtitle exceeds 30 characters")
    skill_path = SOURCE / "skills" / "vtcode-companion" / "SKILL.md"
    if skill_path.is_symlink() or (SOURCE / "plugin.json").is_symlink():
        raise ValueError("package sources must not be symlinks")
    skill = skill_path.read_text(encoding="utf-8")
    if not skill.startswith("---\nname: vtcode-companion\ndescription: "):
        raise ValueError("invalid skill frontmatter")
    files = {
        "plugin.json": (json.dumps(manifest, indent=2) + "\n").encode("utf-8"),
        "mcp.json": (json.dumps(mcp, indent=2) + "\n").encode("utf-8"),
        "skills/vtcode-companion/SKILL.md": skill.encode("utf-8"),
    }
    for field in ("logo", "logoDark", "composerIcon", "composerIconDark"):
        if field in interface:
            name, data = package_image(
                interface[field], 256 if field.startswith("logo") else 48, True
            )
            files[name] = data
    for reference in interface.get("screenshots", []):
        name, data = package_image(reference, 1, False)
        files[name] = data
    if mcp["mcpServers"]["vtcode"]["type"] == "stdio":
        launcher = SOURCE / "server" / "launch.py"
        if launcher.is_symlink():
            raise ValueError("package sources must not be symlinks")
        files["server/launch.py"] = launcher.read_bytes()
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=output.parent) as directory:
        archive_path = Path(directory) / "companion.zip"
        with zipfile.ZipFile(archive_path, "w", zipfile.ZIP_DEFLATED) as archive:
            for name, content in files.items():
                archive.writestr(name, content)
        with zipfile.ZipFile(archive_path) as archive:
            if set(archive.namelist()) != set(files) or archive.testzip() is not None:
                raise ValueError("archive contents failed validation")
            for name, content in files.items():
                if archive.read(name) != content:
                    raise ValueError("archive round-trip validation failed")
        # Exclusive creation preserves an existing user artifact even under a race.
        with output.open("xb") as destination:
            destination.write(archive_path.read_bytes())
    return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    transport = parser.add_mutually_exclusive_group(required=True)
    transport.add_argument("--local", action="store_true")
    transport.add_argument("--mcp-url")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--workspace", type=Path)
    destination = parser.add_mutually_exclusive_group(required=True)
    destination.add_argument("--output", type=Path, help="write a ZIP archive")
    destination.add_argument(
        "--marketplace", type=Path, help="create a fresh local marketplace directory"
    )
    args = parser.parse_args()
    if args.local and (args.binary is None or args.workspace is None):
        parser.error("--local requires --binary and --workspace")
    if not args.local and (args.binary is not None or args.workspace is not None):
        parser.error("--binary and --workspace apply only to --local")
    if args.marketplace is not None and not args.local:
        parser.error("--marketplace applies only to --local")
    token = os.environ.get("VTCODE_COMPANION_ACCESS_TOKEN")
    if not args.local and not token:
        parser.error(
            "set VTCODE_COMPANION_ACCESS_TOKEN through your secure environment; never pass it as an argument"
        )
    try:
        if args.local:
            if args.marketplace is not None:
                output = prepare_marketplace(
                    args.binary, args.workspace, args.marketplace
                )
            else:
                output = package_local(args.binary, args.workspace, args.output)
        else:
            resource = verify_endpoint(args.mcp_url, token)
            output = package(args.mcp_url, resource, args.output)
    except (OSError, ValueError, TypeError, KeyError):
        parser.exit(
            1,
            "Endpoint or package validation failed; no verified package was produced. Check connection and metadata locally.\n",
        )
    print(output)


if __name__ == "__main__":
    main()
