#!/usr/bin/env python3
"""Generate docs/config/CONFIG_FIELD_REFERENCE.md from vtcode-config schema."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_OUTPUT = REPO_ROOT / "docs" / "config" / "CONFIG_FIELD_REFERENCE.md"
CARGO_SCHEMA_CMD = [
    "cargo",
    "run",
    "-q",
    "-p",
    "vtcode-config",
    "--features",
    "schema",
    "--example",
    "schema_dump",
]


@dataclass
class FieldEntry:
    path: str
    type_name: str
    required: bool
    default: str
    description: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate config field reference docs from vtcode-config schema."
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"Output markdown file path (default: {DEFAULT_OUTPUT}).",
    )
    return parser.parse_args()


def load_schema_from_cargo() -> dict[str, Any]:
    result = subprocess.run(
        CARGO_SCHEMA_CMD,
        cwd=REPO_ROOT,
        env={**os.environ, "RUSTC_WRAPPER": ""},
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        stderr = result.stderr.strip()
        raise RuntimeError(
            "Failed to export configuration schema.\n"
            "Remediation:\n"
            "1. Ensure Rust toolchain is installed and `cargo` is available.\n"
            "2. Run: cargo run -p vtcode-config --features schema --example schema_dump\n"
            f"3. Cargo stderr:\n{stderr}"
        )

    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(
            "Schema export output was not valid JSON.\n"
            "Remediation:\n"
            "1. Re-run cargo command directly to inspect output.\n"
            "2. Ensure vtcode-config example `schema_dump` prints JSON only."
        ) from exc


def decode_json_pointer_segment(segment: str) -> str:
    return segment.replace("~1", "/").replace("~0", "~")


def resolve_pointer(document: dict[str, Any], pointer: str) -> Any:
    if not pointer.startswith("#/"):
        raise KeyError(f"Unsupported schema reference pointer: {pointer}")
    current: Any = document
    for segment in pointer[2:].split("/"):
        key = decode_json_pointer_segment(segment)
        current = current[key]
    return current


def deep_copy(value: Any) -> Any:
    return json.loads(json.dumps(value))


def merge_schema(base: dict[str, Any], overlay: dict[str, Any]) -> dict[str, Any]:
    merged = deep_copy(base)
    for key, value in overlay.items():
        if key == "$ref":
            continue
        if key == "required":
            existing = set(merged.get("required", []))
            existing.update(value)
            merged["required"] = sorted(existing)
            continue
        if key == "properties":
            props = merged.setdefault("properties", {})
            props.update(value)
            continue
        merged[key] = deep_copy(value)
    return merged


def normalize_schema_node(
    node: dict[str, Any],
    root_schema: dict[str, Any],
    ref_stack: tuple[str, ...] = (),
) -> dict[str, Any]:
    normalized = deep_copy(node)

    while "$ref" in normalized:
        ref = normalized["$ref"]
        if ref in ref_stack:
            # Recursive schema branch; stop expansion here.
            return {k: v for k, v in normalized.items() if k != "$ref"}
        target = resolve_pointer(root_schema, ref)
        normalized = merge_schema(target, normalized)
        ref_stack = (*ref_stack, ref)

    if "allOf" in normalized:
        all_of = normalized.pop("allOf")
        base = normalized
        for sub in all_of:
            sub_normalized = normalize_schema_node(sub, root_schema, ref_stack)
            base = merge_schema(base, sub_normalized)
        normalized = base

    return normalized


def format_default(value: Any) -> str:
    text = json.dumps(value, ensure_ascii=True)
    if len(text) > 120:
        return f"{text[:117]}..."
    return text


def format_type_name(node: dict[str, Any]) -> str:
    type_value = node.get("type")
    if isinstance(type_value, list):
        return " | ".join(sorted(str(item) for item in type_value))
    if isinstance(type_value, str):
        return type_value

    if "enum" in node:
        values = [json.dumps(value, ensure_ascii=True) for value in node["enum"]]
        preview = ", ".join(values[:4])
        if len(values) > 4:
            preview = f"{preview}, ..."
        return f"enum({preview})"

    variants: list[str] = []
    for key in ("oneOf", "anyOf"):
        for option in node.get(key, []):
            option_type = option.get("type")
            if isinstance(option_type, str):
                variants.append(option_type)
            elif "enum" in option:
                variants.append("enum")
            elif "$ref" in option:
                variants.append(option["$ref"].split("/")[-1])
    if variants:
        return " | ".join(sorted(set(variants)))

    if "properties" in node:
        return "object"
    if "items" in node:
        return "array"
    if "additionalProperties" in node:
        return "map"

    return "unknown"


def normalize_description(text: str | None) -> str:
    if not text:
        return ""
    return " ".join(text.strip().split())


def collect_fields(root_schema: dict[str, Any]) -> list[FieldEntry]:
    field_map: dict[str, FieldEntry] = {}

    def upsert(entry: FieldEntry) -> None:
        existing = field_map.get(entry.path)
        if existing is None:
            field_map[entry.path] = entry
            return

        description = existing.description
        if not description and entry.description:
            description = entry.description
        default = existing.default
        if default == "" and entry.default:
            default = entry.default
        type_name = existing.type_name
        if type_name == "unknown" and entry.type_name != "unknown":
            type_name = entry.type_name
        field_map[entry.path] = FieldEntry(
            path=existing.path,
            type_name=type_name,
            required=existing.required or entry.required,
            default=default,
            description=description,
        )

    def walk(node: dict[str, Any], path: str, required: bool) -> None:
        normalized = normalize_schema_node(node, root_schema)
        description = normalize_description(normalized.get("description"))
        default = format_default(normalized["default"]) if "default" in normalized else ""
        type_name = format_type_name(normalized)

        if "oneOf" in normalized or "anyOf" in normalized:
            upsert(
                FieldEntry(
                    path=path,
                    type_name=type_name,
                    required=required,
                    default=default,
                    description=description,
                )
            )
            return

        if normalized.get("type") == "object" or "properties" in normalized:
            properties = normalized.get("properties", {})
            required_set = set(normalized.get("required", []))
            if not properties:
                upsert(
                    FieldEntry(
                        path=path,
                        type_name=type_name,
                        required=required,
                        default=default,
                        description=description,
                    )
                )
            for prop_name in sorted(properties):
                child_path = f"{path}.{prop_name}" if path else prop_name
                walk(properties[prop_name], child_path, prop_name in required_set)

            additional = normalized.get("additionalProperties")
            if isinstance(additional, dict):
                map_path = f"{path}.*" if path else "*"
                walk(additional, map_path, required=False)
            elif additional is True and path:
                upsert(
                    FieldEntry(
                        path=f"{path}.*",
                        type_name="any",
                        required=False,
                        default="",
                        description="Additional map entries.",
                    )
                )
            return

        if normalized.get("type") == "array" or "items" in normalized:
            upsert(
                FieldEntry(
                    path=path,
                    type_name=type_name,
                    required=required,
                    default=default,
                    description=description,
                )
            )
            items = normalized.get("items")
            if isinstance(items, dict):
                walk(items, f"{path}[]", required=False)
            return

        upsert(
            FieldEntry(
                path=path,
                type_name=type_name,
                required=required,
                default=default,
                description=description,
            )
        )

    walk(root_schema, "", required=False)
    entries = [entry for entry in field_map.values() if entry.path]
    entries.sort(key=lambda item: item.path)
    return entries


def escape_cell(value: str) -> str:
    return value.replace("|", "\\|").replace("\n", " ").strip()


def render_markdown(entries: list[FieldEntry]) -> str:
    lines = [
        "# Config Field Reference",
        "",
        "Generated from `vtcode-config` schema (`VTCodeConfig`) for complete field coverage.",
        "",
        "Regenerate:",
        "",
        "```bash",
        "python3 scripts/generate_config_field_reference.py",
        "```",
        "",
        "| Field | Type | Required | Default | Description |",
        "|-------|------|----------|---------|-------------|",
    ]
    for entry in entries:
        required = "yes" if entry.required else "no"
        default = entry.default or "-"
        description = entry.description or "-"
        lines.append(
            f"| `{escape_cell(entry.path)}` | `{escape_cell(entry.type_name)}` | "
            f"{required} | `{escape_cell(default)}` | {escape_cell(description)} |"
        )
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    args = parse_args()
    output_path = args.output
    if not output_path.is_absolute():
        output_path = REPO_ROOT / output_path

    try:
        schema = load_schema_from_cargo()
        entries = collect_fields(schema)
        markdown = render_markdown(entries)
    except RuntimeError as exc:
        print(str(exc), file=sys.stderr)
        return 1

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(markdown, encoding="utf-8")
    print(f"Wrote {len(entries)} config fields to {output_path.relative_to(REPO_ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
