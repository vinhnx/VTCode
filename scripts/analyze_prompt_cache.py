#!/usr/bin/env python3
"""Summarize cache counters locally; never sends requests or prints prompt content."""

import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import statistics


def counter(value):
    return value if type(value) is int and value >= 0 else None


def reported_counter(record, reported_key, legacy_key):
    if reported_key in record:
        return counter(record[reported_key])
    # Old trajectories defaulted absent counters to zero. Only positive legacy
    # values establish that usage was actually reported.
    value = counter(record.get(legacy_key))
    return value if value else None


def summarize(paths, provider=None, model=None, rates=None, scope=None):
    groups = defaultdict(lambda: {"usage": [], "projections": []})
    malformed_rows = 0
    for path in dict.fromkeys(Path(path).resolve() for path in paths):
        with path.open(encoding="utf-8") as source:
            for line in source:
                if not line.strip():
                    continue
                try:
                    record = json.loads(line)
                except (ValueError, RecursionError):
                    malformed_rows += 1
                    continue
                if not isinstance(record, dict):
                    malformed_rows += 1
                    continue
                if isinstance(record.get("fields"), dict):
                    record = record["fields"]
                kind = record.get("kind")
                if kind not in ("prompt_cache_metrics", "history_projection_metrics"):
                    continue
                route = record.get("provider") or "unknown"
                name = record.get("model") or record.get("requested_model") or "unknown"
                usage_scope = record.get("scope", "foreground")
                if not isinstance(route, str) or not isinstance(name, str):
                    malformed_rows += 1
                    continue
                if usage_scope not in ("foreground", "auxiliary"):
                    malformed_rows += 1
                    continue
                if (provider is not None and provider != route) or (model is not None and model != name):
                    continue
                if scope is not None and scope != usage_scope:
                    continue
                if kind == "history_projection_metrics":
                    fields = ("original_estimated_tokens", "projected_estimated_tokens", "changed_message_count")
                    if all(counter(record.get(key)) is not None for key in fields):
                        groups[route, name, usage_scope]["projections"].append(record)
                    else:
                        malformed_rows += 1
                    continue
                legacy = "input_tokens" not in record
                total = counter(record.get("input_tokens"))
                if legacy:
                    total = counter(record.get("prompt_tokens"))
                reads = reported_counter(record, "cache_read_tokens_reported", "cache_read_tokens")
                writes = reported_counter(record, "cache_write_tokens_reported", "cache_creation_tokens")
                if total is None or (not legacy and (reads or 0) + (writes or 0) > total):
                    malformed_rows += 1
                    continue
                groups[route, name, usage_scope]["usage"].append((total, reads, writes, legacy, counter(record.get("elapsed_ms"))))

    result = []
    for (route, name, usage_scope), group in sorted(groups.items()):
        rows = group["usage"]
        total = sum(row[0] for row in rows if not row[3])
        reads = sum(row[1] or 0 for row in rows)
        writes = sum(row[2] or 0 for row in rows)
        missing_reads = sum(row[1] is None for row in rows)
        missing_writes = sum(row[2] is None for row in rows)
        legacy_rows = sum(row[3] for row in rows)
        latencies = [row[4] for row in rows if row[4] is not None]
        complete = bool(rows) and not (missing_reads or missing_writes or legacy_rows) and route != "unknown"
        projections = group["projections"]
        item = {
            "provider": route, "model": name, "scope": usage_scope, "requests": len(rows),
            "input_tokens": total, "observed_cache_read_tokens": reads,
            "observed_cache_write_tokens": writes,
            "requests_missing_read_counter": missing_reads,
            "requests_missing_write_counter": missing_writes,
            "legacy_input_rows": legacy_rows,
            "legacy_reported_prompt_tokens": sum(row[0] for row in rows if row[3]),
            "weighted_read_fraction": reads / total if total and not (missing_reads or legacy_rows) else None,
            "weighted_write_fraction": writes / total if total and not (missing_writes or legacy_rows) else None,
            "observed_read_fraction_lower_bound": reads / total if total and not legacy_rows else None,
            "observed_write_fraction_lower_bound": writes / total if total and not legacy_rows else None,
            "median_request_elapsed_ms": statistics.median(latencies) if latencies else None,
            "clearing_projections": len(projections),
            "clearing_changed_projections": sum(row["changed_message_count"] > 0 for row in projections),
            "estimated_tokens_removed_by_clearing": sum(
                row["original_estimated_tokens"] - row["projected_estimated_tokens"] for row in projections),
            "estimated_input_cost_usd": None,
        }
        if rates is not None and complete:
            ordinary = total - reads - writes
            cost = (ordinary * rates[0] + reads * rates[1] + writes * rates[2]) / 1_000_000
            if not math.isfinite(cost):
                raise ValueError("input-cost estimate exceeds the finite numeric range")
            item["estimated_input_cost_usd"] = cost
        result.append(item)
    return {"groups": result, "malformed_rows": malformed_rows,
            "pricing_usd_per_million": dict(zip(("input", "cache_read", "cache_write"), rates)) if rates else None,
            "cost_note": "Estimates use supplied USD-per-million rates; verify endpoint, tier, region and context premiums. Missing counters or legacy normalization leave cost unknown."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument("--provider")
    parser.add_argument("--model")
    parser.add_argument("--scope", choices=("foreground", "auxiliary"))
    parser.add_argument("--input-rate", type=float, help="Uncached input USD per million tokens")
    parser.add_argument("--read-rate", type=float, help="Cache-read USD per million tokens")
    parser.add_argument("--write-rate", type=float, help="Cache-write USD per million tokens")
    args = parser.parse_args()
    rates = (args.input_rate, args.read_rate, args.write_rate)
    if any(rate is not None for rate in rates):
        if not args.provider or not args.model or any(rate is None or not math.isfinite(rate) or rate < 0 for rate in rates):
            parser.error("pricing requires an exact provider and model plus three finite non-negative rates")
    else:
        rates = None
    try:
        result = summarize(args.paths, args.provider, args.model, rates, args.scope)
    except (OSError, ValueError, OverflowError) as error:
        parser.error(str(error))
    print(json.dumps(result, indent=2, allow_nan=False))


if __name__ == "__main__":
    main()
