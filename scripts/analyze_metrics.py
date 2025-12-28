#!/usr/bin/env python3
"""
analyze_metrics.py - Analyze VT Code performance metrics
Usage: python3 scripts/analyze_metrics.py vtcode_performance_metrics.csv
"""

import sys
import csv
from datetime import datetime
from pathlib import Path
from typing import List, Dict, Tuple
import statistics

class PerformanceAnalyzer:
    def __init__(self, csv_file: str):
        self.csv_file = csv_file
        self.metrics: List[Dict] = []
        self.load_metrics()

    def load_metrics(self):
        """Load metrics from CSV file"""
        if not Path(self.csv_file).exists():
            print(f"❌ File not found: {self.csv_file}")
            sys.exit(1)

        try:
            with open(self.csv_file, 'r') as f:
                reader = csv.DictReader(f)
                self.metrics = list(reader)
                
            # Convert numeric fields
            for metric in self.metrics:
                metric['rss_mb'] = float(metric['rss_mb'])
                metric['vsz_mb'] = float(metric['vsz_mb'])
                metric['cpu_percent'] = float(metric['cpu_percent'])
                metric['num_threads'] = int(metric['num_threads'])
                metric['cache_hits'] = int(metric['cache_hits'])
                metric['cache_misses'] = int(metric['cache_misses'])
                metric['tool_calls'] = int(metric['tool_calls'])
                
            print(f"✅ Loaded {len(self.metrics)} metric records")
        except Exception as e:
            print(f"❌ Error loading metrics: {e}")
            sys.exit(1)

    def get_memory_stats(self) -> Dict:
        """Calculate memory statistics"""
        if not self.metrics:
            return {}

        rss_values = [m['rss_mb'] for m in self.metrics]
        vsz_values = [m['vsz_mb'] for m in self.metrics]

        return {
            'rss_min': min(rss_values),
            'rss_max': max(rss_values),
            'rss_avg': statistics.mean(rss_values),
            'rss_median': statistics.median(rss_values),
            'rss_stdev': statistics.stdev(rss_values) if len(rss_values) > 1 else 0,
            'vsz_min': min(vsz_values),
            'vsz_max': max(vsz_values),
            'vsz_avg': statistics.mean(vsz_values),
            'vsz_median': statistics.median(vsz_values),
            'rss_growth': rss_values[-1] - rss_values[0],
            'rss_growth_percent': ((rss_values[-1] - rss_values[0]) / rss_values[0] * 100) if rss_values[0] > 0 else 0,
        }

    def get_cpu_stats(self) -> Dict:
        """Calculate CPU statistics"""
        if not self.metrics:
            return {}

        cpu_values = [m['cpu_percent'] for m in self.metrics]

        return {
            'cpu_min': min(cpu_values),
            'cpu_max': max(cpu_values),
            'cpu_avg': statistics.mean(cpu_values),
            'cpu_median': statistics.median(cpu_values),
            'cpu_stdev': statistics.stdev(cpu_values) if len(cpu_values) > 1 else 0,
        }

    def get_thread_stats(self) -> Dict:
        """Calculate thread statistics"""
        if not self.metrics:
            return {}

        thread_values = [m['num_threads'] for m in self.metrics]

        return {
            'threads_min': min(thread_values),
            'threads_max': max(thread_values),
            'threads_avg': statistics.mean(thread_values),
        }

    def detect_memory_leaks(self, threshold_percent: float = 10.0) -> Tuple[bool, str]:
        """Detect if memory is growing (potential leak)"""
        if len(self.metrics) < 3:
            return False, "Insufficient data for leak detection"

        rss_values = [m['rss_mb'] for m in self.metrics]
        
        # Check if memory consistently grows
        growth_rate = (rss_values[-1] - rss_values[0]) / max(rss_values[0], 1) * 100
        
        if growth_rate > threshold_percent:
            return True, f"Memory growth of {growth_rate:.1f}% detected (threshold: {threshold_percent}%)"
        else:
            return False, f"No significant memory growth ({growth_rate:.1f}%)"

    def detect_cpu_spikes(self, threshold_percent: float = 80.0) -> List[Dict]:
        """Detect CPU usage spikes"""
        spikes = []
        cpu_values = [m['cpu_percent'] for m in self.metrics]
        
        if cpu_values:
            avg_cpu = statistics.mean(cpu_values)
            threshold = avg_cpu * 1.5  # 50% above average
            
            for i, metric in enumerate(self.metrics):
                if metric['cpu_percent'] > threshold_percent:
                    spikes.append({
                        'turn': metric['turn_number'],
                        'timestamp': metric['timestamp_iso'],
                        'cpu_percent': metric['cpu_percent'],
                    })
        
        return spikes

    def print_summary(self):
        """Print comprehensive analysis report"""
        print("\n" + "="*70)
        print("📊 VT CODE PERFORMANCE ANALYSIS REPORT")
        print("="*70)

        print(f"\n📁 Analysis of: {self.csv_file}")
        print(f"   Records analyzed: {len(self.metrics)}")
        if self.metrics:
            print(f"   Time range: {self.metrics[0]['timestamp_iso']} to {self.metrics[-1]['timestamp_iso']}")

        # Memory Analysis
        print("\n" + "─"*70)
        print("💾 MEMORY USAGE ANALYSIS")
        print("─"*70)
        mem_stats = self.get_memory_stats()
        
        if mem_stats:
            print(f"\n  Resident Set Size (RSS - Physical Memory):")
            print(f"    Minimum:        {mem_stats['rss_min']:.1f} MB")
            print(f"    Maximum:        {mem_stats['rss_max']:.1f} MB")
            print(f"    Average:        {mem_stats['rss_avg']:.1f} MB")
            print(f"    Median:         {mem_stats['rss_median']:.1f} MB")
            print(f"    Std Deviation:  {mem_stats['rss_stdev']:.1f} MB")
            print(f"    Total Growth:   {mem_stats['rss_growth']:.1f} MB ({mem_stats['rss_growth_percent']:.1f}%)")

            print(f"\n  Virtual Memory Size (VSZ):")
            print(f"    Minimum:        {mem_stats['vsz_min']:.1f} MB")
            print(f"    Maximum:        {mem_stats['vsz_max']:.1f} MB")
            print(f"    Average:        {mem_stats['vsz_avg']:.1f} MB")

        # Leak Detection
        print(f"\n  Memory Leak Detection:")
        is_leak, leak_msg = self.detect_memory_leaks(threshold_percent=15.0)
        symbol = "⚠️  WARNING" if is_leak else "✅ OK"
        print(f"    {symbol}: {leak_msg}")

        # CPU Analysis
        print("\n" + "─"*70)
        print("⚙️  CPU USAGE ANALYSIS")
        print("─"*70)
        cpu_stats = self.get_cpu_stats()
        
        if cpu_stats:
            print(f"\n  CPU Usage Percentage:")
            print(f"    Minimum:        {cpu_stats['cpu_min']:.1f}%")
            print(f"    Maximum:        {cpu_stats['cpu_max']:.1f}%")
            print(f"    Average:        {cpu_stats['cpu_avg']:.1f}%")
            print(f"    Median:         {cpu_stats['cpu_median']:.1f}%")
            print(f"    Std Deviation:  {cpu_stats['cpu_stdev']:.1f}%")

        # CPU Spikes
        spikes = self.detect_cpu_spikes(threshold_percent=80.0)
        if spikes:
            print(f"\n  ⚠️  CPU Spikes Detected ({len(spikes)} events >80%):")
            for spike in spikes[:5]:  # Show first 5
                print(f"    Turn {spike['turn']}: {spike['cpu_percent']:.1f}% at {spike['timestamp']}")
            if len(spikes) > 5:
                print(f"    ... and {len(spikes) - 5} more")
        else:
            print(f"\n  ✅ No significant CPU spikes detected")

        # Thread Analysis
        print("\n" + "─"*70)
        print("🧵 THREADING ANALYSIS")
        print("─"*70)
        thread_stats = self.get_thread_stats()
        
        if thread_stats:
            print(f"\n  Thread Count:")
            print(f"    Minimum:        {int(thread_stats['threads_min'])} threads")
            print(f"    Maximum:        {int(thread_stats['threads_max'])} threads")
            print(f"    Average:        {thread_stats['threads_avg']:.0f} threads")

        # Recommendations
        print("\n" + "─"*70)
        print("💡 RECOMMENDATIONS")
        print("─"*70)
        
        recommendations = []
        
        if mem_stats and mem_stats['rss_growth_percent'] > 20:
            recommendations.append("  • High memory growth detected - consider implementing cache eviction")
        
        if cpu_stats and cpu_stats['cpu_avg'] > 50:
            recommendations.append("  • High average CPU usage - profile to identify hot spots")
        
        if spikes:
            recommendations.append(f"  • {len(spikes)} CPU spikes detected - investigate during these periods")
        
        if thread_stats and thread_stats['threads_max'] > 100:
            recommendations.append("  • High thread count - review thread pool configuration")
        
        if not recommendations:
            recommendations.append("  • Performance metrics look healthy!")
        
        for rec in recommendations:
            print(rec)

        # Summary metrics
        print("\n" + "="*70)
        print("📈 QUICK METRICS")
        print("="*70)
        
        if mem_stats:
            status = "🟡 WARNING" if mem_stats['rss_growth_percent'] > 15 else "🟢 GOOD"
            print(f"Memory Trend:  {status} ({mem_stats['rss_growth_percent']:+.1f}%)")
        
        if cpu_stats:
            status = "🟡 WARNING" if cpu_stats['cpu_avg'] > 50 else "🟢 GOOD"
            print(f"CPU Usage:     {status} ({cpu_stats['cpu_avg']:.1f}% avg)")

    def export_html_report(self, output_file: str = None):
        """Export detailed HTML report"""
        if not output_file:
            output_file = Path(self.csv_file).stem + "_report.html"

        html_content = f"""
<!DOCTYPE html>
<html>
<head>
    <title>VT Code Performance Report</title>
    <script src="https://cdn.jsdelivr.net/npm/chart.js"></script>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 20px; }}
        h1 {{ color: #333; }}
        .metric {{ display: inline-block; margin: 10px; padding: 15px; background: #f5f5f5; border-radius: 5px; }}
        canvas {{ max-width: 800px; margin: 20px 0; }}
        table {{ width: 100%; border-collapse: collapse; margin: 20px 0; }}
        th, td {{ padding: 10px; border: 1px solid #ddd; text-align: left; }}
        th {{ background: #f5f5f5; }}
    </style>
</head>
<body>
    <h1>VT Code Performance Report</h1>
    <p>Generated: {datetime.now().isoformat()}</p>
    
    <h2>Memory Usage Over Time</h2>
    <canvas id="memoryChart"></canvas>
    
    <h2>CPU Usage Over Time</h2>
    <canvas id="cpuChart"></canvas>

    <script>
        const metrics = {self.metrics};
        
        const ctx = document.getElementById('memoryChart').getContext('2d');
        new Chart(ctx, {{
            type: 'line',
            data: {{
                labels: metrics.map(m => m.turn_number),
                datasets: [{{
                    label: 'RSS (MB)',
                    data: metrics.map(m => m.rss_mb),
                    borderColor: 'rgb(75, 192, 192)',
                    tension: 0.1
                }}, {{
                    label: 'VSZ (MB)',
                    data: metrics.map(m => m.vsz_mb),
                    borderColor: 'rgb(153, 102, 255)',
                    tension: 0.1
                }}]
            }},
            options: {{
                responsive: true,
                plugins: {{ title: {{ display: true, text: 'Memory Usage (MB)' }} }}
            }}
        }});

        const cpuCtx = document.getElementById('cpuChart').getContext('2d');
        new Chart(cpuCtx, {{
            type: 'line',
            data: {{
                labels: metrics.map(m => m.turn_number),
                datasets: [{{
                    label: 'CPU %',
                    data: metrics.map(m => m.cpu_percent),
                    borderColor: 'rgb(255, 99, 132)',
                    tension: 0.1
                }}]
            }},
            options: {{
                responsive: true,
                plugins: {{ title: {{ display: true, text: 'CPU Usage (%)' }} }}
            }}
        }});
    </script>
</body>
</html>
"""
        with open(output_file, 'w') as f:
            f.write(html_content)
        
        print(f"\n✅ HTML report saved to: {output_file}")

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 scripts/analyze_metrics.py <csv_file>")
        sys.exit(1)

    csv_file = sys.argv[1]
    analyzer = PerformanceAnalyzer(csv_file)
    analyzer.print_summary()

if __name__ == "__main__":
    main()
