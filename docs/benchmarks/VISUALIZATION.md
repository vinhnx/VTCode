# Benchmark Visualization Guide

This guide explains how to generate and interpret benchmark visualizations.

## Chart Components

The benchmark chart includes four key visualizations:

### 1. Pass/Fail Pie Chart (Top Left)

-   Shows the overall pass rate as a percentage
-   Green: Passed tests
-   Red: Failed tests
-   Displays Pass@1 metric prominently

### 2. Test Results Bar Chart (Top Right)

-   Absolute numbers of passed vs failed tests
-   Useful for understanding the scale of results
-   Shows exact counts above each bar

### 3. Latency Distribution (Bottom Left)

-   Histogram of response times across all tasks
-   Red dashed line: P50 (median) latency
-   Orange dashed line: P90 latency
-   Helps identify performance outliers

### 4. Configuration Table (Bottom Right)

-   Model and provider information
-   Key metrics summary
-   Configuration parameters used
-   Cost analysis

## Generating Charts

### Prerequisites

```bash
# Install matplotlib (optional, for PNG charts)
pip install matplotlib

# Or use ASCII-only mode (no dependencies)
```

### Basic Usage

```bash
# Generate ASCII chart (always works)
python3 scripts/generate_benchmark_chart.py reports/HE_*.json

# Generate PNG chart (requires matplotlib)
python3 scripts/generate_benchmark_chart.py reports/HE_*.json --png

# Generate both
python3 scripts/generate_benchmark_chart.py reports/HE_*.json --all
```

### Output Files

The script generates:

-   **ASCII chart**: Displayed in terminal
-   **PNG chart**: `reports/HE_*_chart.png` (if matplotlib available)
-   **Markdown summary**: `reports/HE_*_summary.md`

## Chart Interpretation

### Pass Rate Analysis

| Pass@1 | Interpretation                                |
| ------ | --------------------------------------------- |
| 90%+   | Excellent - Production ready for most tasks   |
| 75-90% | Very Good - Suitable for most coding tasks    |
| 60-75% | Good - Useful for development and prototyping |
| 45-60% | Fair - May need human review                  |
| <45%   | Poor - Not recommended for production         |

### Latency Analysis

| P50 Latency | Interpretation                       |
| ----------- | ------------------------------------ |
| <0.5s       | Excellent - Real-time interaction    |
| 0.5-1.0s    | Very Good - Smooth user experience   |
| 1.0-2.0s    | Good - Acceptable for most use cases |
| 2.0-5.0s    | Fair - Noticeable delay              |
| >5.0s       | Poor - May impact productivity       |

### Cost Analysis

| Cost per 164 tasks | Interpretation                    |
| ------------------ | --------------------------------- |
| $0.00              | Free tier - Ideal for development |
| $0.01-0.10         | Very Low - Cost-effective         |
| $0.10-1.00         | Low - Reasonable for production   |
| $1.00-10.00        | Medium - Consider optimization    |
| >$10.00            | High - Evaluate alternatives      |

## Example Charts

### Model Comparison Chart

![Comparison Chart](../../docs/benchmarks/reports/comparison_gemini-2.5-flash-lite_vs_gpt-5-nano.png)

**Key Observations:**

1. **Pass Rate**: gpt-5-nano achieves 94.5% (frontier-tier), gemini achieves 61.6% (mid-range)
2. **Distribution**: gpt-5-nano passes 155/164 tests, gemini passes 101/164
3. **Latency**: gemini is 10x faster (0.97s vs 10.4s)
4. **Cost**: Both are $0.00 (completely free)
5. **Strategic Choice**: Use gpt-5-nano for accuracy, gemini for speed

### Individual Model Chart

![Gemini Chart](../../docs/benchmarks/reports/HE_20251022-135834_gemini-2.5-flash-lite_tools-0_N164_chart.png)

**gemini-2.5-flash-lite Observations:**

1. **Pass Rate**: 61.6% - good for development and rapid iteration
2. **Speed**: Sub-second latency (0.97s) - 10x faster than gpt-5-nano
3. **Cost**: $0.00 - ideal for high-volume testing

## Comparing Multiple Models

To compare multiple benchmark runs:

```bash
# Generate comparison table
python3 scripts/compare_benchmarks.py reports/HE_*.json

# Generate individual charts for each
for report in reports/HE_*.json; do
    python3 scripts/generate_benchmark_chart.py "$report" --png
done
```

## Customization

### Modifying Chart Appearance

Edit `scripts/generate_benchmark_chart.py`:

```python
# Change colors
colors = ['#4CAF50', '#F44336']  # Green, Red

# Adjust figure size
fig, axes = plt.subplots(2, 2, figsize=(14, 10))

# Change DPI (resolution)
plt.savefig(output_path, dpi=300)
```

### Adding New Metrics

To add custom metrics to the chart:

1. Extract data from the report JSON
2. Add a new subplot or modify existing ones
3. Update the metadata table with new fields

## Troubleshooting

### matplotlib Not Found

```bash
# Install matplotlib
pip install matplotlib

# Or use ASCII-only mode
python3 scripts/generate_benchmark_chart.py reports/HE_*.json
# (omit --png flag)
```

### Chart Not Displaying

```bash
# Check file was created
ls -lh reports/*_chart.png

# View with system default
open reports/HE_*_chart.png  # macOS
xdg-open reports/HE_*_chart.png  # Linux
```

### Low Resolution

Increase DPI in the script:

```python
plt.savefig(output_path, dpi=600)  # Higher quality
```

## Best Practices

1. **Always generate charts** after running benchmarks
2. **Include charts in documentation** for visual reference
3. **Compare across runs** to track improvements
4. **Archive charts** with their corresponding reports
5. **Use consistent settings** for fair comparisons

## References

-   [matplotlib Documentation](https://matplotlib.org/stable/contents.html)
-   [Benchmark Methodology](README.md)
-   [Results Archive](SUMMARY.md)
