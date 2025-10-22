#!/usr/bin/env python3
"""
Render HumanEval benchmark results as charts and tables.
"""
import json
import sys
from pathlib import Path
from datetime import datetime

def render_ascii_bar(value, max_value=1.0, width=40):
    """Render an ASCII progress bar."""
    filled = int((value / max_value) * width)
    bar = "█" * filled + "░" * (width - filled)
    return f"{bar} {value*100:.1f}%"

def render_results_table(report_path):
    """Render benchmark results as a formatted table."""
    with open(report_path) as f:
        data = json.load(f)

    meta = data['meta']
    summary = data['summary']

    # Header
    print("\n" + "=" * 80)
    print("HUMANEVAL BENCHMARK RESULTS".center(80))
    print("=" * 80)

    # Model info
    print(f"\n📊 Model: {meta['model']}")
    print(f"🔧 Provider: {meta['provider']}")
    print(f"📅 Date: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print(f"🎯 Tasks: {summary['n']}/{meta['n_requested']}")

    # Performance metrics
    print(f"\n{'Metric':<30} {'Value':<50}")
    print("-" * 80)
    print(f"{'Pass@1':<30} {render_ascii_bar(summary['pass_at_1'])}")
    print(f"{'Passed Tests':<30} {sum(1 for r in data['results'] if r['passed'])}/{summary['n']}")
    print(f"{'Failed Tests':<30} {sum(1 for r in data['results'] if not r['passed'])}/{summary['n']}")

    # Latency metrics
    print(f"\n{'Latency Metrics':<30} {'Value':<50}")
    print("-" * 80)
    print(f"{'Median (P50)':<30} {summary['latency_p50_s']:.3f}s")
    print(f"{'P90':<30} {summary['latency_p90_s']:.3f}s")

    # Cost metrics
    print(f"\n{'Cost Metrics':<30} {'Value':<50}")
    print("-" * 80)
    print(f"{'Input Tokens':<30} {summary['total_prompt_tokens']:,}")
    print(f"{'Output Tokens':<30} {summary['total_completion_tokens']:,}")
    cost = summary['est_cost_usd'] or 0.0
    print(f"{'Estimated Cost':<30} ${cost:.4f}")

    # Configuration
    print(f"\n{'Configuration':<30} {'Value':<50}")
    print("-" * 80)
    print(f"{'Temperature':<30} {meta['temperature']}")
    print(f"{'Max Output Tokens':<30} {meta['max_output_tokens']}")
    print(f"{'Timeout':<30} {meta['timeout_s']}s")
    print(f"{'Use Tools':<30} {meta['use_tools']}")
    print(f"{'Seed':<30} {meta['seed']}")

    print("\n" + "=" * 80 + "\n")

def render_markdown_table(report_path):
    """Render benchmark results as a Markdown table."""
    with open(report_path) as f:
        data = json.load(f)

    meta = data['meta']
    summary = data['summary']
    passed = sum(1 for r in data['results'] if r['passed'])
    failed = sum(1 for r in data['results'] if not r['passed'])

    md = []
    md.append("## HumanEval Benchmark Results")
    md.append("")
    md.append(f"**Model:** `{meta['model']}`  ")
    md.append(f"**Provider:** `{meta['provider']}`  ")
    md.append(f"**Date:** {datetime.now().strftime('%Y-%m-%d')}")
    md.append("")
    md.append("### Performance Metrics")
    md.append("")
    md.append("| Metric | Value |")
    md.append("|--------|-------|")
    md.append(f"| **Pass@1** | **{summary['pass_at_1']*100:.1f}%** |")
    md.append(f"| Tasks Completed | {summary['n']}/{meta['n_requested']} |")
    md.append(f"| Tests Passed | {passed} |")
    md.append(f"| Tests Failed | {failed} |")
    md.append(f"| Median Latency (P50) | {summary['latency_p50_s']:.3f}s |")
    md.append(f"| P90 Latency | {summary['latency_p90_s']:.3f}s |")
    md.append("")
    md.append("### Cost Analysis")
    md.append("")
    md.append("| Metric | Value |")
    md.append("|--------|-------|")
    md.append(f"| Input Tokens | {summary['total_prompt_tokens']:,} |")
    md.append(f"| Output Tokens | {summary['total_completion_tokens']:,} |")
    cost = summary['est_cost_usd'] or 0.0
    md.append(f"| Estimated Cost | ${cost:.4f} |")
    md.append("")
    md.append("> **Note:** `gemini-2.5-flash-lite` is in Google's free tier, so actual cost is $0.")
    md.append("")

    return "\n".join(md)

def main():
    if len(sys.argv) < 2:
        print("Usage: python render_benchmark_chart.py <report_path>")
        sys.exit(1)

    report_path = Path(sys.argv[1])
    if not report_path.exists():
        print(f"Error: Report file not found: {report_path}")
        sys.exit(1)

    # Render ASCII table to console
    render_results_table(report_path)

    # Generate markdown
    md_content = render_markdown_table(report_path)

    # Save markdown to file
    md_path = report_path.parent / f"{report_path.stem}_summary.md"
    with open(md_path, 'w') as f:
        f.write(md_content)

    print(f" Markdown summary saved to: {md_path}")
    print(f" Full report available at: {report_path}")

if __name__ == "__main__":
    main()
