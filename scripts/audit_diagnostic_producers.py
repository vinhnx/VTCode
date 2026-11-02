#!/usr/bin/env python3
"""Inventory production severity and terminal-text producers without reading TODO files.

The runtime adapters own migration; this inventory distinguishes severity-bearing
messages from ordinary progress/help, and records each source location for review.
"""
import argparse
import json
import re
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs/development/diagnostics/producer-audit.json"
SEVERITY = re.compile(r"\b(MessageStyle|InlineMessageKind|DiagnosticSeverity)::(Error|ToolError|Warning|Info)\b")
TERMINAL = re.compile(r"\b(?:eprintln!|println!|styled_eprintln!|styled_println!|runner_println|vtcode_commons::diagnostics::stderr(?:_diagnostic)?)\s*\(")


# Reviewed failure-like Info contexts. New candidates require an explicit rationale.
INFO_CONTEXT_REVIEWS = {
    "crates/codegen/vtcode-core/src/terminal_setup/wizard.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters.",
    "crates/codegen/vtcode-core/src/utils/ansi/output.rs": "Info reference is presentation/style selection; actual failure messages use canonical Error/Warning or explicit Diagnostic payloads.",
    "src/agent/runloop/model_picker/state_machine.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters.",
    "src/agent/runloop/slash_commands/builtins.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters.",
    "src/agent/runloop/slash_commands/rendering.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters.",
    "src/agent/runloop/unified/diagnostics.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/inline_events/context.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters.",
    "src/agent/runloop/unified/mcp_support.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/mcp_support_overview.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/model_selection.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters.",
    "src/agent/runloop/unified/planning_workflow/plan_approval.rs": "Info reference is presentation/style selection; actual failure messages use canonical Error/Warning or explicit Diagnostic payloads.",
    "src/agent/runloop/unified/session_setup/ui/active_settings.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters.",
    "src/agent/runloop/unified/tool_routing/hook_messages.rs": "Typed hook Info remains informational; typed Warning and Error now preserve severity through canonical renderer adapters.",
    "src/agent/runloop/unified/tool_routing/permission_prompt.rs": "User focus/permission navigation instructions; denial and approval remain explicit policy outcomes, never implicit implementation authorization.",
    "src/agent/runloop/unified/turn/finalization.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters.",
    "src/agent/runloop/unified/turn/guards.rs": "Recovery phase progress, retained-state status, or user recovery instructions; paused/blocked/provider failures and failed queue admission use Warning/canonical transitions; budget-exhaustion instructions are help.",
    "src/agent/runloop/unified/turn/session/interaction_loop_runner.rs": "Recovery phase progress, retained-state status, or user recovery instructions; paused/blocked/provider failures and failed queue admission use Warning/canonical transitions; budget-exhaustion instructions are help.",
    "src/agent/runloop/unified/turn/session/interaction_loop_runner/support.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters.",
    "src/agent/runloop/unified/turn/session/slash_commands/apps.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/control.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/explain.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/fast.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/interactive.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/local_server.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/oauth.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/plugins.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/rewind.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/secrets.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/slash_commands/workspace.rs": "User-requested status/configuration/results/help, including negative counts or disabled capabilities; actual operation failures use separate canonical diagnostics.",
    "src/agent/runloop/unified/turn/session/tool_dispatch.rs": "Recovery phase progress, retained-state status, or user recovery instructions; paused/blocked/provider failures and failed queue admission use Warning/canonical transitions; budget-exhaustion instructions are help.",
    "src/agent/runloop/unified/turn/session_loop_runner/blocked_handoff.rs": "Recovery phase progress, retained-state status, or user recovery instructions; paused/blocked/provider failures and failed queue admission use Warning/canonical transitions; budget-exhaustion instructions are help.",
    "src/agent/runloop/unified/turn/session_loop_runner/orchestration/session_iteration.rs": "Recovery phase progress, retained-state status, or user recovery instructions; paused/blocked/provider failures and failed queue admission use Warning/canonical transitions; budget-exhaustion instructions are help.",
    "src/agent/runloop/unified/turn/tool_outcomes/execution_result/failure_diagnosis/presentation.rs": "The only remaining Info diagnosis branch is grep/rg no-match, an ordinary result; true failures now emit one complete canonical diagnostic.",
    "src/agent/runloop/unified/turn/turn_helpers.rs": "Recovery phase progress, retained-state status, or user recovery instructions; paused/blocked/provider failures and failed queue admission use Warning/canonical transitions; budget-exhaustion instructions are help.",
    "src/agent/runloop/unified/turn/turn_loop.rs": "Recovery phase progress, retained-state status, or user recovery instructions; paused/blocked/provider failures and failed queue admission use Warning/canonical transitions; budget-exhaustion instructions are help.",
    "src/agent/runloop/unified/turn/turn_loop/post_tool_recovery.rs": "Recovery phase progress, retained-state status, or user recovery instructions; paused/blocked/provider failures and failed queue admission use Warning/canonical transitions; budget-exhaustion instructions are help.",
    "src/agent/runloop/unified/turn/turn_processing/result_handler.rs": "Recovery phase progress, retained-state status, or user recovery instructions; paused/blocked/provider failures and failed queue admission use Warning/canonical transitions; budget-exhaustion instructions are help.",
    "src/agent/runloop/unified/turn/utils.rs": "Typed hook Info remains informational; typed Warning and Error now preserve severity through canonical renderer adapters.",
    "src/startup/first_run_prompts/trust.rs": "Trust menu choices are help; failure to open the interactive selector now uses Warning and manual-entry instructions.",
    "src/updater/interactive.rs": "Positive operation progress or instructions; adjacent failure branches use Error/Warning canonical adapters."
}

STATUS_RESULT_REVIEWS = {
    "crates/codegen/vtcode-core/src/commands/stats.rs": "Requested metrics and HTML export include historical error/recovery counts as data.",
    "crates/codegen/vtcode-core/src/core/agent/core.rs": "Task outcome statistics include historical failures and recovery counts as data.",
    "crates/codegen/vtcode-core/src/tool_policy/manager.rs": "Requested policy status lists allowed/prompt/denied rules; does not itself deny an action.",
    "crates/codegen/vtcode-core/src/tools/cached_executor.rs": "Requested execution/cache statistics include historical failed-call counts as data.",
    "src/cli/auth.rs": "Explicit auth-status projection: authenticated, unauthenticated, unavailable, and failed-flow are reported data, not a new retry incident.",
    "src/cli/auto.rs": "Returned task results and already-recorded warning summaries; not a second incident emission.",
    "src/cli/dependencies.rs": "Explicit dependency health check results; installer warnings use the sanitized diagnostic adapter.",
    "src/cli/init_project.rs": "User-requested project existence/overwrite validation and force-option help.",
    "src/cli/secret.rs": "User-requested migration totals; individual migration errors use the diagnostic adapter.",
    "src/cli/session_store.rs": "User-requested audit verification report and file mismatch data; failure exit remains authoritative.",
    "src/cli/sessions.rs": "Saved blocked-session state and recovery instructions, not a fresh failure transition.",
    "src/cli/skills/render.rs": "Compatibility notes and available alternatives are requested report/help data.",
    "src/cli/skills/validate.rs": "Validation report data; failure to generate the report uses the diagnostic adapter."
}
RISK_WORDS = re.compile(r"fail|error|blocked|exhaust|unable|not available|cannot|denied|recovery|retry", re.IGNORECASE)


def inventory():
    entries = []
    for base in (ROOT / "src", ROOT / "crates"):
        for path in sorted(base.rglob("*.rs")):
            relative = path.relative_to(ROOT).as_posix()
            if path.name == "tests.rs" or "/tests/" in relative or "/benches/" in relative or "/examples/" in relative:
                continue
            # Build scripts and release tooling are maintainer tools, not shipped runtime producers.
            if path.name == "build.rs" or relative.startswith("crates/codegen/xtask/"):
                continue
            source = path.read_text()
            # Project convention places in-module test suites after production.
            source = re.split(r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+tests\b", source, maxsplit=1)[0]
            source_lines = source.splitlines()
            for number, line in enumerate(source_lines, 1):
                stripped = line.strip()
                if stripped.startswith(("//", "*")):
                    continue
                for match in SEVERITY.finditer(line):
                    severity = match[2]
                    if match[1] == "DiagnosticSeverity":
                        disposition = "canonical_diagnostic"
                        boundary = "Diagnostic payload -> canonical renderer sink and ThreadEvent"
                    elif severity == "Info":
                        disposition = "ordinary_progress_or_help"
                        boundary = "existing severity style; explicit diagnostics use Diagnostic"
                    elif match[1] == "MessageStyle":
                        disposition = "canonical_adapter"
                        boundary = "AnsiRenderer::line/flush/line_with_override_style"
                    elif relative.startswith("src/") or "/types/message_commands.rs" in relative or relative.endswith("/ui/tui.rs"):
                        disposition = "canonical_ui_adapter"
                        boundary = "InlineHandle Error/Warning append observer -> canonical Diagnostic"
                    else:
                        disposition = "presentation_style"
                        boundary = "UI severity rendering; canonical payload enters InlineCommand::Diagnostic"
                    entry = dict(path=relative, line=number, surface=match[0], classification=disposition, boundary=boundary, source=stripped)
                    if match[1] == "MessageStyle" and severity == "Info":
                        context = " ".join(source_lines[number - 1:number + 4])
                        if RISK_WORDS.search(context):
                            entry["review_rationale"] = INFO_CONTEXT_REVIEWS.get(relative, "REVIEW REQUIRED: failure-like Info context")
                            if relative not in INFO_CONTEXT_REVIEWS:
                                entry["classification"] = "needs_review"
                    entries.append(entry)
                if re.search(r"\.(?:diagnostic|record_diagnostic|emit_diagnostic)\s*\(", line):
                    entries.append(dict(path=relative, line=number, surface="diagnostic", classification="canonical_diagnostic", boundary="canonical renderer sink and ThreadEvent", source=stripped))
                if TERMINAL.search(line):
                    migrated = "diagnostics::stderr" in line or relative == "crates/common/vtcode-commons/src/diagnostics.rs"
                    entry = dict(path=relative, line=number, surface="stderr" if migrated or "eprintln!" in line else "stdout", classification="canonical_early_startup_adapter" if migrated else "ordinary_progress_or_help", boundary="diagnostics::stderr (no thread exists yet)" if migrated else "CLI output/help/progress", source=stripped)
                    if not migrated and relative in STATUS_RESULT_REVIEWS:
                        entry["classification"] = "status_result"
                        entry["review_rationale"] = STATUS_RESULT_REVIEWS[relative]
                    elif "runner_println" in line:
                        entry["review_rationale"] = "Headless progress; incidents migrated to record_diagnostic/emit_diagnostic; shared progress printer redacts before display."
                    elif relative.endswith("/ui/styled.rs"):
                        entry["review_rationale"] = "Shared terminal styles: Error/Warning now adapt canonical payloads before stderr; remaining styled stdout is progress/help."
                    entries.append(entry)
    entries.sort(key=lambda entry: (entry["path"], entry["line"], entry["surface"]))
    return {"schema_version": 1, "scope": "production Rust severity references and terminal output; tests/examples/doc comments excluded", "migration": "shared adapters preserve legacy call sites; canonical structured incidents retain correlation and actions", "classifications": dict(sorted(Counter(entry["classification"] for entry in entries).items())), "producers": entries}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    report = inventory()
    producers = report.pop("producers")
    unreviewed = [entry for entry in producers if entry["classification"] == "needs_review"]
    if unreviewed:
        locations = ", ".join(f"{entry['path']}:{entry['line']}" for entry in unreviewed)
        raise SystemExit("Diagnostic Info review required: " + locations)
    header = json.dumps(report, indent=2, ensure_ascii=False).rstrip()
    content = header[:-2] + ',\n  "producers": [\n    '
    content += ',\n    '.join(json.dumps(entry, ensure_ascii=False) for entry in producers)
    content += "\n  ]\n}\n"
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text() != content:
            raise SystemExit("Diagnostic inventory is stale; run python3 scripts/audit_diagnostic_producers.py")
    else:
        OUTPUT.write_text(content)
    print(f"Diagnostic inventory: {len(producers)} entries")


if __name__ == "__main__":
    main()
