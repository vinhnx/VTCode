# Benchmark Chart Quick Reference

## Current Chart

![HumanEval Benchmark - gemini-2.5-flash-lite](../../docs/benchmarks/reports/HE_20251022-135834_gemini-2.5-flash-lite_tools-0_N164_chart.png)

## Chart Breakdown

### Top Left: Pass/Fail Distribution

-   **Green (61.6%)**: Tests that passed
-   **Red (38.4%)**: Tests that failed
-   Shows overall Pass@1 metric

### Top Right: Absolute Counts

-   **101 Passed**: Successfully generated correct code
-   **63 Failed**: Logic errors or incorrect implementations
-   Total: 164 tasks (complete HumanEval dataset)

### Bottom Left: Latency Distribution

-   **Histogram**: Shows response time frequency
-   **Red Line (P50)**: 0.973s - Half of responses faster than this
-   **Orange Line (P90)**: 1.363s - 90% of responses faster than this
-   Most responses cluster around 1 second

### Bottom Right: Configuration Summary

-   Model and provider details
-   Key performance metrics
-   Configuration parameters
-   Cost analysis

## Key Insights

### Performance

✅ **61.6% Pass@1** - Good for a free-tier model
✅ **Sub-second median latency** - Fast enough for interactive use
✅ **Zero cost** - Ideal for development and prototyping

### Quality

✅ **No syntax errors** - All generated code was valid Python
✅ **Consistent performance** - Tight latency distribution
⚠️ **Logic errors only** - Failed tests had incorrect implementations

### Use Cases

-   ✅ Development and prototyping
-   ✅ Code suggestions and completions
-   ✅ Learning and experimentation
-   ⚠️ Production code (consider premium models for >70% accuracy)

## Generating Your Own Charts

```bash
# After running a benchmark
python3 scripts/generate_benchmark_chart.py reports/HE_*.json --png

# View the chart
open reports/HE_*_chart.png  # macOS
xdg-open reports/HE_*_chart.png  # Linux
```

## Comparing Models

When you run benchmarks with different models, compare:

1. **Pass@1 Rate**: Higher is better (aim for >70% for production)
2. **Latency**: Lower is better (sub-second ideal)
3. **Cost**: Balance accuracy vs. cost for your use case
4. **Consistency**: Tight latency distribution = predictable performance

## Next Steps

-   Run benchmarks with other models: [README.md](README.md)
-   Understand methodology: [HUMANEVAL_2025-10-22.md](HUMANEVAL_2025-10-22.md)
-   Compare results: [COMPARISON.md](COMPARISON.md)
-   Learn visualization: [VISUALIZATION.md](VISUALIZATION.md)
