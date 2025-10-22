# Benchmark Summary

## Quick Reference

| Model                    | Pass@1    | Latency (P50) | Cost  | Date       | Tier         |
| ------------------------ | --------- | ------------- | ----- | ---------- | ------------ |
| **gpt-5-nano** 🏆        | **94.5%** | 10.4s         | $0.00 | 2025-10-22 | **Frontier** |
| gemini-2.5-flash-lite ⚡ | 61.6%     | 0.97s         | $0.00 | 2025-10-22 | Mid-Range    |

## Latest Results

### 🏆 gpt-5-nano (2025-10-22) - FRONTIER-TIER PERFORMANCE

![Comparison Chart](../../reports/comparison_gemini-2.5-flash-lite_vs_gpt-5-nano.png)

**Performance:**

-   🏆 155/164 tests passed (94.5%)
-   ⚡ 10.4s median latency
-   💰 $0.00 cost (free tier)
-   🎯 TOP 5 globally

**Key Findings:**

-   Frontier-tier accuracy competitive with $15-60/1M models
-   Only 9 failures out of 164 tasks
-   Comparable to o1, Claude 3.7 Sonnet, GPT-4.5 Turbo

**Full Report:** [GPT5_NANO_VS_GEMINI.md](GPT5_NANO_VS_GEMINI.md)

### gemini-2.5-flash-lite (2025-10-22) - SPEED CHAMPION

![Benchmark Chart](../../reports/HE_20251022-135834_gemini-2.5-flash-lite_tools-0_N164_chart.png)

**Performance:**

-   ✅ 101/164 tests passed (61.6%)
-   ⚡ 0.97s median latency
-   💰 $0.00 cost (free tier)

**Key Findings:**

-   All failures were logic errors (no syntax errors)
-   Consistent sub-second response times
-   Suitable for development and prototyping
-   Consider premium models for >70% accuracy needs

**Full Report:** [HUMANEVAL_2025-10-22.md](HUMANEVAL_2025-10-22.md)

## How to Run

```bash
# Full benchmark
make bench-humaneval PROVIDER=gemini MODEL='gemini-2.5-flash-lite'

# Quick test (10 tasks)
make bench-humaneval PROVIDER=gemini MODEL='gemini-2.5-flash-lite' N_HE=10

# Generate charts
python3 scripts/generate_benchmark_chart.py reports/HE_*.json
```

## Visualization

ASCII chart available via:

```bash
python3 scripts/generate_benchmark_chart.py reports/HE_20251022-135834_gemini-2.5-flash-lite_tools-0_N164.json
```

Output:

```
╔══════════════════════════════════════════════════════════════════════════════╗
║                        HUMANEVAL BENCHMARK RESULTS                           ║
╠══════════════════════════════════════════════════════════════════════════════╣
║  PASS RATE:  61.6%                                                         ║
║                                                                              ║
║  [██████████████████████████████░░░░░░░░░░░░░░░░░░░░]  ║
║   101 passed     63 failed                                              ║
╚══════════════════════════════════════════════════════════════════════════════╝
```

## Files

-   **Detailed Analysis:** [HUMANEVAL_2025-10-22.md](HUMANEVAL_2025-10-22.md)
-   **Methodology:** [README.md](README.md)
-   **Raw Data:** `../../reports/HE_20251022-135834_gemini-2.5-flash-lite_tools-0_N164.json`
-   **Scripts:** `../../scripts/bench_humaneval.py`, `../../scripts/generate_benchmark_chart.py`
