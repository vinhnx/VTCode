#!/usr/bin/env python3
"""
Generate side-by-side comparison charts for multiple benchmark results.
"""
import json
import sys
from pathlib import Path
from datetime import datetime

def load_report(path):
    """Load a benchmark report."""
    with open(path) as f:
        return json.load(f)

def generate_ascii_comparison(reports):
    """Generate ASCII comparison chart."""
    if len(reports) != 2:
        print("Error: This script compares exactly 2 reports")
        return
    
    r1, r2 = reports
    m1 = r1['meta']
    m2 = r2['meta']
    s1 = r1['summary']
    s2 = r2['summary']
    
    p1 = sum(1 for r in r1['results'] if r['passed'])
    p2 = sum(1 for r in r2['results'] if r['passed'])
    f1 = sum(1 for r in r1['results'] if not r['passed'])
    f2 = sum(1 for r in r2['results'] if not r['passed'])
    
    # Determine winner for each metric
    acc_winner = "🏆" if s1['pass_at_1'] > s2['pass_at_1'] else ""
    acc_winner2 = "🏆" if s2['pass_at_1'] > s1['pass_at_1'] else ""
    lat_winner = "🏆" if s1['latency_p50_s'] < s2['latency_p50_s'] else ""
    lat_winner2 = "🏆" if s2['latency_p50_s'] < s1['latency_p50_s'] else ""
    
    chart = f"""
╔══════════════════════════════════════════════════════════════════════════════╗
║                    HUMANEVAL BENCHMARK COMPARISON                            ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  Model 1: {m1['model']:<62} ║
║  Provider: {m1['provider']:<61} ║
║  Pass@1: {s1['pass_at_1']*100:>5.1f}% {acc_winner:<56} ║
║  Passed: {p1:>3}/{s1['n']:<3}                                                         ║
║  Failed: {f1:>3}/{s1['n']:<3}                                                         ║
║  Latency (P50): {s1['latency_p50_s']:>6.3f}s {lat_winner:<46} ║
║  Latency (P90): {s1['latency_p90_s']:>6.3f}s                                         ║
║                                                                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║                                                                              ║
║  Model 2: {m2['model']:<62} ║
║  Provider: {m2['provider']:<61} ║
║  Pass@1: {s2['pass_at_1']*100:>5.1f}% {acc_winner2:<56} ║
║  Passed: {p2:>3}/{s2['n']:<3}                                                         ║
║  Failed: {f2:>3}/{s2['n']:<3}                                                         ║
║  Latency (P50): {s2['latency_p50_s']:>6.3f}s {lat_winner2:<46} ║
║  Latency (P90): {s2['latency_p90_s']:>6.3f}s                                         ║
║                                                                              ║
╠══════════════════════════════════════════════════════════════════════════════╣
║  COMPARISON SUMMARY                                                          ║
║                                                                              ║
║  Accuracy Difference: {abs(s1['pass_at_1'] - s2['pass_at_1'])*100:>5.1f}% ({('Model 1' if s1['pass_at_1'] > s2['pass_at_1'] else 'Model 2') + ' better':<30})║
║  Speed Difference:    {abs(s1['latency_p50_s'] - s2['latency_p50_s']):.2f}s ({('Model 1' if s1['latency_p50_s'] < s2['latency_p50_s'] else 'Model 2') + ' faster':<30})║
║  Speed Ratio:         {max(s1['latency_p50_s'], s2['latency_p50_s']) / min(s1['latency_p50_s'], s2['latency_p50_s']):.1f}x                                                  ║
║                                                                              ║
╚══════════════════════════════════════════════════════════════════════════════╝
"""
    return chart

def generate_matplotlib_comparison(reports, output_path):
    """Generate comparison chart using matplotlib."""
    try:
        import matplotlib.pyplot as plt
        import numpy as np
    except ImportError:
        print("⚠️  matplotlib not installed. Install with: pip install matplotlib")
        return False
    
    if len(reports) != 2:
        print("Error: This script compares exactly 2 reports")
        return False
    
    r1, r2 = reports
    m1 = r1['meta']
    m2 = r2['meta']
    s1 = r1['summary']
    s2 = r2['summary']
    
    p1 = sum(1 for r in r1['results'] if r['passed'])
    p2 = sum(1 for r in r2['results'] if r['passed'])
    f1 = sum(1 for r in r1['results'] if not r['passed'])
    f2 = sum(1 for r in r2['results'] if not r['passed'])
    
    # Create figure with subplots
    fig = plt.figure(figsize=(16, 10))
    gs = fig.add_gridspec(3, 3, hspace=0.3, wspace=0.3)
    
    # Title
    fig.suptitle(f"HumanEval Benchmark Comparison\n{m1['model']} vs {m2['model']}", 
                 fontsize=16, fontweight='bold')
    
    # 1. Pass Rate Comparison (bar chart)
    ax1 = fig.add_subplot(gs[0, 0])
    models = [m1['model'][:20], m2['model'][:20]]
    pass_rates = [s1['pass_at_1'] * 100, s2['pass_at_1'] * 100]
    colors = ['#4CAF50' if s1['pass_at_1'] > s2['pass_at_1'] else '#2196F3',
              '#4CAF50' if s2['pass_at_1'] > s1['pass_at_1'] else '#2196F3']
    bars = ax1.bar(models, pass_rates, color=colors, alpha=0.8, edgecolor='black')
    ax1.set_ylabel('Pass@1 (%)')
    ax1.set_title('Pass@1 Comparison')
    ax1.set_ylim(0, 100)
    for i, (bar, val) in enumerate(zip(bars, pass_rates)):
        ax1.text(bar.get_x() + bar.get_width()/2, val + 2, f'{val:.1f}%',
                ha='center', va='bottom', fontweight='bold', fontsize=12)
    
    # 2. Passed vs Failed (stacked bar)
    ax2 = fig.add_subplot(gs[0, 1])
    passed = [p1, p2]
    failed = [f1, f2]
    x = np.arange(len(models))
    width = 0.6
    ax2.bar(x, passed, width, label='Passed', color='#4CAF50', alpha=0.8)
    ax2.bar(x, failed, width, bottom=passed, label='Failed', color='#F44336', alpha=0.8)
    ax2.set_ylabel('Number of Tests')
    ax2.set_title('Test Results Distribution')
    ax2.set_xticks(x)
    ax2.set_xticklabels(models)
    ax2.legend()
    for i, (p, f) in enumerate(zip(passed, failed)):
        ax2.text(i, p/2, str(p), ha='center', va='center', fontweight='bold', color='white')
        ax2.text(i, p + f/2, str(f), ha='center', va='center', fontweight='bold', color='white')
    
    # 3. Latency Comparison (bar chart)
    ax3 = fig.add_subplot(gs[0, 2])
    latencies_p50 = [s1['latency_p50_s'], s2['latency_p50_s']]
    latencies_p90 = [s1['latency_p90_s'], s2['latency_p90_s']]
    x = np.arange(len(models))
    width = 0.35
    ax3.bar(x - width/2, latencies_p50, width, label='P50', color='#2196F3', alpha=0.8)
    ax3.bar(x + width/2, latencies_p90, width, label='P90', color='#FF9800', alpha=0.8)
    ax3.set_ylabel('Latency (seconds)')
    ax3.set_title('Latency Comparison')
    ax3.set_xticks(x)
    ax3.set_xticklabels(models)
    ax3.legend()
    
    # 4. Latency Distribution Model 1
    ax4 = fig.add_subplot(gs[1, :2])
    lat1 = [r['latency_s'] for r in r1['results'] if not r.get('gen_timeout')]
    ax4.hist(lat1, bins=30, color='#2196F3', alpha=0.7, edgecolor='black', label=m1['model'][:20])
    ax4.axvline(s1['latency_p50_s'], color='red', linestyle='--', linewidth=2,
                label=f"P50: {s1['latency_p50_s']:.2f}s")
    ax4.axvline(s1['latency_p90_s'], color='orange', linestyle='--', linewidth=2,
                label=f"P90: {s1['latency_p90_s']:.2f}s")
    ax4.set_xlabel('Latency (seconds)')
    ax4.set_ylabel('Frequency')
    ax4.set_title(f'Latency Distribution: {m1["model"]}')
    ax4.legend()
    ax4.grid(True, alpha=0.3)
    
    # 5. Latency Distribution Model 2
    ax5 = fig.add_subplot(gs[2, :2])
    lat2 = [r['latency_s'] for r in r2['results'] if not r.get('gen_timeout')]
    ax5.hist(lat2, bins=30, color='#4CAF50', alpha=0.7, edgecolor='black', label=m2['model'][:20])
    ax5.axvline(s2['latency_p50_s'], color='red', linestyle='--', linewidth=2,
                label=f"P50: {s2['latency_p50_s']:.2f}s")
    ax5.axvline(s2['latency_p90_s'], color='orange', linestyle='--', linewidth=2,
                label=f"P90: {s2['latency_p90_s']:.2f}s")
    ax5.set_xlabel('Latency (seconds)')
    ax5.set_ylabel('Frequency')
    ax5.set_title(f'Latency Distribution: {m2["model"]}')
    ax5.legend()
    ax5.grid(True, alpha=0.3)
    
    # 6. Summary Table
    ax6 = fig.add_subplot(gs[1:, 2])
    ax6.axis('off')
    
    # Calculate differences
    acc_diff = abs(s1['pass_at_1'] - s2['pass_at_1']) * 100
    acc_better = m1['model'][:15] if s1['pass_at_1'] > s2['pass_at_1'] else m2['model'][:15]
    speed_diff = abs(s1['latency_p50_s'] - s2['latency_p50_s'])
    speed_faster = m1['model'][:15] if s1['latency_p50_s'] < s2['latency_p50_s'] else m2['model'][:15]
    speed_ratio = max(s1['latency_p50_s'], s2['latency_p50_s']) / min(s1['latency_p50_s'], s2['latency_p50_s'])
    
    summary_data = [
        ['Metric', m1['model'][:15], m2['model'][:15]],
        ['', '', ''],
        ['Pass@1', f"{s1['pass_at_1']*100:.1f}%", f"{s2['pass_at_1']*100:.1f}%"],
        ['Passed', f"{p1}/{s1['n']}", f"{p2}/{s2['n']}"],
        ['Failed', f"{f1}/{s1['n']}", f"{f2}/{s2['n']}"],
        ['P50 Latency', f"{s1['latency_p50_s']:.2f}s", f"{s2['latency_p50_s']:.2f}s"],
        ['P90 Latency', f"{s1['latency_p90_s']:.2f}s", f"{s2['latency_p90_s']:.2f}s"],
        ['', '', ''],
        ['Comparison', '', ''],
        ['Acc. Diff', f"{acc_diff:.1f}%", f"({acc_better} better)"],
        ['Speed Diff', f"{speed_diff:.2f}s", f"({speed_faster} faster)"],
        ['Speed Ratio', f"{speed_ratio:.1f}x", ''],
    ]
    
    table = ax6.table(cellText=summary_data, cellLoc='left', loc='center',
                     colWidths=[0.35, 0.325, 0.325])
    table.auto_set_font_size(False)
    table.set_fontsize(9)
    table.scale(1, 2)
    
    # Style header row
    for i in range(3):
        table[(0, i)].set_facecolor('#E3F2FD')
        table[(0, i)].set_text_props(weight='bold')
    
    # Style comparison section
    for i in range(8, 12):
        table[(i, 0)].set_facecolor('#FFF3E0')
    
    ax6.set_title('Comparison Summary', pad=20, fontweight='bold')
    
    plt.tight_layout()
    plt.savefig(output_path, dpi=300, bbox_inches='tight')
    print(f"✅ Comparison chart saved to: {output_path}")
    return True

def main():
    if len(sys.argv) < 3:
        print("Usage: python generate_comparison_chart.py <report1.json> <report2.json> [--png]")
        print("\nExample:")
        print("  python generate_comparison_chart.py reports/model1.json reports/model2.json --png")
        sys.exit(1)
    
    report1_path = Path(sys.argv[1])
    report2_path = Path(sys.argv[2])
    
    if not report1_path.exists():
        print(f"Error: Report file not found: {report1_path}")
        sys.exit(1)
    
    if not report2_path.exists():
        print(f"Error: Report file not found: {report2_path}")
        sys.exit(1)
    
    reports = [load_report(report1_path), load_report(report2_path)]
    
    # Always show ASCII comparison
    print(generate_ascii_comparison(reports))
    
    # Generate PNG if requested
    if '--png' in sys.argv or '--all' in sys.argv:
        # Create output filename
        m1 = reports[0]['meta']['model']
        m2 = reports[1]['meta']['model']
        safe_m1 = m1.replace('/', '_').replace(' ', '_')
        safe_m2 = m2.replace('/', '_').replace(' ', '_')
        png_path = report1_path.parent / f"comparison_{safe_m1}_vs_{safe_m2}.png"
        
        if generate_matplotlib_comparison(reports, png_path):
            print(f"\n📊 Visual comparison chart generated: {png_path}")
        else:
            print("\n💡 Tip: Install matplotlib for visual charts: pip install matplotlib")

if __name__ == "__main__":
    main()
