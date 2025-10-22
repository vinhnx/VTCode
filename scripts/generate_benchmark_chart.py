#!/usr/bin/env python3
"""
Generate visual benchmark charts from HumanEval results.
Supports both matplotlib (PNG) and ASCII art fallback.
"""
import json
import sys
from pathlib import Path
from datetime import datetime

def generate_ascii_chart(data):
    """Generate ASCII art chart for terminal display."""
    summary = data['summary']
    meta = data['meta']

    pass_rate = summary['pass_at_1']
    passed = sum(1 for r in data['results'] if r['passed'])
    failed = sum(1 for r in data['results'] if not r['passed'])

    # ASCII bar chart
    bar_width = 50
    passed_width = int((passed / summary['n']) * bar_width)
    failed_width = bar_width - passed_width

    chart = f"""
╔══════════════════════════════════════════════════════════════════════════════╗
║                        HUMANEVAL BENCHMARK RESULTS                           ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  Model: {meta['model']:<66} ║
║  Provider: {meta['provider']:<63} ║
║  Date: {datetime.now().strftime('%Y-%m-%d %H:%M:%S'):<66} ║
║                                                                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║  PASS RATE: {pass_rate*100:>5.1f}%                                                         ║
║                                                                              ║
║  [{'█' * passed_width}{'░' * failed_width}]  ║
║   {passed:>3} passed    {failed:>3} failed                                              ║
║                                                                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║  LATENCY METRICS                                                             ║
║                                                                              ║
║    Median (P50):  {summary['latency_p50_s']:>6.3f}s                                            ║
║    P90:           {summary['latency_p90_s']:>6.3f}s                                            ║
║                                                                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║  COST ANALYSIS                                                               ║
║                                                                              ║
║    Input Tokens:  {summary['total_prompt_tokens']:>10,}                                        ║
║    Output Tokens: {summary['total_completion_tokens']:>10,}                                        ║
║    Est. Cost:     ${(summary['est_cost_usd'] or 0):.4f}                                           ║
║                                                                              ║
╚══════════════════════════════════════════════════════════════════════════════╝
"""
    return chart

def generate_matplotlib_chart(data, output_path):
    """Generate PNG chart using matplotlib."""
    try:
        import matplotlib.pyplot as plt
        import matplotlib.patches as mpatches
    except ImportError:
        print("⚠️  matplotlib not installed. Install with: pip install matplotlib")
        return False

    summary = data['summary']
    meta = data['meta']

    passed = sum(1 for r in data['results'] if r['passed'])
    failed = sum(1 for r in data['results'] if not r['passed'])

    # Create figure with subplots
    fig, ((ax1, ax2), (ax3, ax4)) = plt.subplots(2, 2, figsize=(14, 10))
    fig.suptitle(f"HumanEval Benchmark: {meta['model']}", fontsize=16, fontweight='bold')

    # 1. Pass/Fail pie chart
    colors = ['#4CAF50', '#F44336']
    ax1.pie([passed, failed], labels=['Passed', 'Failed'], autopct='%1.1f%%',
            colors=colors, startangle=90)
    ax1.set_title(f"Pass@1: {summary['pass_at_1']*100:.1f}%")

    # 2. Pass/Fail bar chart
    ax2.bar(['Passed', 'Failed'], [passed, failed], color=colors)
    ax2.set_ylabel('Number of Tests')
    ax2.set_title('Test Results Distribution')
    ax2.set_ylim(0, summary['n'])
    for i, v in enumerate([passed, failed]):
        ax2.text(i, v + 2, str(v), ha='center', va='bottom', fontweight='bold')

    # 3. Latency distribution
    latencies = [r['latency_s'] for r in data['results'] if not r.get('gen_timeout')]
    ax3.hist(latencies, bins=30, color='#2196F3', alpha=0.7, edgecolor='black')
    ax3.axvline(summary['latency_p50_s'], color='red', linestyle='--',
                label=f"P50: {summary['latency_p50_s']:.3f}s")
    ax3.axvline(summary['latency_p90_s'], color='orange', linestyle='--',
                label=f"P90: {summary['latency_p90_s']:.3f}s")
    ax3.set_xlabel('Latency (seconds)')
    ax3.set_ylabel('Frequency')
    ax3.set_title('Response Latency Distribution')
    ax3.legend()

    # 4. Metadata table
    ax4.axis('off')
    metadata = [
        ['Model', meta['model']],
        ['Provider', meta['provider']],
        ['Tasks', f"{summary['n']}/{meta['n_requested']}"],
        ['Pass@1', f"{summary['pass_at_1']*100:.1f}%"],
        ['Median Latency', f"{summary['latency_p50_s']:.3f}s"],
        ['P90 Latency', f"{summary['latency_p90_s']:.3f}s"],
        ['Temperature', str(meta['temperature'])],
        ['Max Tokens', str(meta['max_output_tokens'])],
        ['Cost', f"${(summary['est_cost_usd'] or 0):.4f}"],
    ]
    table = ax4.table(cellText=metadata, cellLoc='left', loc='center',
                     colWidths=[0.4, 0.6])
    table.auto_set_font_size(False)
    table.set_fontsize(10)
    table.scale(1, 2)
    ax4.set_title('Benchmark Configuration', pad=20)

    # Style the table
    for i in range(len(metadata)):
        table[(i, 0)].set_facecolor('#E3F2FD')
        table[(i, 0)].set_text_props(weight='bold')

    plt.tight_layout()
    plt.savefig(output_path, dpi=300, bbox_inches='tight')
    print(f" Chart saved to: {output_path}")
    return True

def main():
    if len(sys.argv) < 2:
        print("Usage: python generate_benchmark_chart.py <report_path> [--png]")
        sys.exit(1)

    report_path = Path(sys.argv[1])
    if not report_path.exists():
        print(f"Error: Report file not found: {report_path}")
        sys.exit(1)

    with open(report_path) as f:
        data = json.load(f)

    # Always show ASCII chart
    print(generate_ascii_chart(data))

    # Generate PNG if requested or matplotlib is available
    if '--png' in sys.argv or '--all' in sys.argv:
        png_path = report_path.parent / f"{report_path.stem}_chart.png"
        if generate_matplotlib_chart(data, png_path):
            print(f"\n📊 Visual chart generated: {png_path}")
        else:
            print("\n💡 Tip: Install matplotlib for visual charts: pip install matplotlib")

if __name__ == "__main__":
    main()
