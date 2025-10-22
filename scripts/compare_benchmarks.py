#!/usr/bin/env python3
"""
Compare multiple HumanEval benchmark results.
"""
import json
import sys
from pathlib import Path
from datetime import datetime

def load_report(path):
    """Load a benchmark report."""
    with open(path) as f:
        return json.load(f)

def compare_reports(reports):
    """Generate comparison table."""
    print("\n" + "=" * 100)
    print("HUMANEVAL BENCHMARK COMPARISON".center(100))
    print("=" * 100)
    print()
    
    # Header
    print(f"{'Model':<30} {'Pass@1':<10} {'P50 Lat':<10} {'P90 Lat':<10} {'Cost':<10} {'Date':<15}")
    print("-" * 100)
    
    # Sort by pass rate
    sorted_reports = sorted(reports, key=lambda r: r['summary']['pass_at_1'], reverse=True)
    
    for report in sorted_reports:
        meta = report['meta']
        summary = report['summary']
        
        model = meta['model'][:28]
        pass_rate = f"{summary['pass_at_1']*100:.1f}%"
        p50 = f"{summary['latency_p50_s']:.3f}s"
        p90 = f"{summary['latency_p90_s']:.3f}s"
        cost = f"${summary['est_cost_usd'] or 0:.4f}"
        
        # Extract date from filename or use current
        date = "2025-10-22"  # Default
        
        print(f"{model:<30} {pass_rate:<10} {p50:<10} {p90:<10} {cost:<10} {date:<15}")
    
    print("=" * 100)
    print()

def main():
    if len(sys.argv) < 2:
        print("Usage: python compare_benchmarks.py <report1.json> [report2.json] ...")
        print("\nExample:")
        print("  python compare_benchmarks.py reports/HE_*.json")
        sys.exit(1)
    
    reports = []
    for path_str in sys.argv[1:]:
        path = Path(path_str)
        if path.exists():
            reports.append(load_report(path))
        else:
            print(f"Warning: File not found: {path}")
    
    if not reports:
        print("Error: No valid report files found")
        sys.exit(1)
    
    compare_reports(reports)
    
    # Summary statistics
    if len(reports) > 1:
        pass_rates = [r['summary']['pass_at_1'] for r in reports]
        print(f"Best Pass@1:    {max(pass_rates)*100:.1f}%")
        print(f"Average Pass@1: {sum(pass_rates)/len(pass_rates)*100:.1f}%")
        print(f"Worst Pass@1:   {min(pass_rates)*100:.1f}%")
        print()

if __name__ == "__main__":
    main()
