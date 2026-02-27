# Benchmark Comparison

This document compares VT Code's performance across different models and configurations.

## Current Results

| Model | Provider | Pass@1 | Latency (P50) | Cost | Date | Tier |
|-------|----------|--------|---------------|------|------|------|
| **gpt-5-nano** | OpenAI | **94.5%** | 10.4s | ~$0.10-0.30/1M | 2025-10-22 | **Frontier** |
| gemini-3-flash-preview | Google | 61.6% | 0.97s | $0.00 (free) | 2025-10-22 | Mid-Range |

**Major Achievement:** gpt-5-nano achieves frontier-tier performance (94.5%), ranking in TOP 5 globally at very affordable pricing.

See [GPT5_NANO_VS_GEMINI.md](GPT5_NANO_VS_GEMINI.md) for detailed comparison.

## Planned Comparisons

### Models to Evaluate

**Completed:**

-   gpt-5-nano (94.5%, ~$0.10-0.30/1M)
-   gemini-3-flash-preview (61.6%, free)
-   ⏳ gemini-2.5-flash
-   ⏳ gpt-4-mini
-   ⏳ claude-3-haiku

**Premium Tier:**

-   ⏳ gpt-5
-   ⏳ claude-sonnet-4-5
-   ⏳ gemini-2.5-pro
-   ⏳ deepseek-reasoner

**Specialized:**

-   gpt-5-codex
-   qwen3-coder

### Configuration Variations

**Temperature:**

-   0.0 (deterministic) - completed
-   0.3 (balanced)
-   0.7 (creative)

**Tool Usage:**

-   Disabled - completed
-   Enabled (with code analysis tools)

**Prompt Formats:**

-   Raw code-only - completed
-   Markdown fenced
-   With examples

## Expected Performance Ranges

Based on published benchmarks and model capabilities:

| Model Class | Expected Pass@1 | Cost per 1M tokens |
| ----------- | --------------- | ------------------ |
| Free tier   | 50-65%          | $0-0.50            |
| Mid-tier    | 65-75%          | $0.50-5.00         |
| Premium     | 75-85%          | $5.00-30.00        |
| Specialized | 80-90%          | $10.00-50.00       |

## How to Add New Results

1. Run benchmark:

    ```bash
    make bench-humaneval PROVIDER=<provider> MODEL='<model>' N_HE=164
    ```

2. Generate visualization:

    ```bash
    python3 scripts/generate_benchmark_chart.py reports/HE_*.json
    ```

3. Compare with existing:

    ```bash
    python3 scripts/compare_benchmarks.py reports/HE_*.json
    ```

4. Document results:
    - Create `HUMANEVAL_YYYY-MM-DD_<model>.md`
    - Update this comparison table
    - Update `SUMMARY.md`

## Analysis Framework

When comparing models, consider:

**Performance:**

-   Pass@1 rate (primary metric)
-   Latency (P50, P90, P99)
-   Consistency (variance across runs)

**Cost:**

-   Token usage (input + output)
-   API pricing
-   Total cost per benchmark run

**Quality:**

-   Types of failures (syntax vs logic)
-   Edge case handling
-   Code style and readability

**Practical Factors:**

-   API availability and reliability
-   Rate limits
-   Free tier quotas
-   Regional availability

## Contributing

To add benchmark results:

1. Run the benchmark with your model
2. Verify results are reproducible (run 2-3 times)
3. Document configuration and environment
4. Submit PR with:
    - Raw JSON report
    - Detailed analysis document
    - Updated comparison tables

## References

-   [HumanEval Dataset](https://github.com/openai/human-eval)
-   [Benchmark Methodology](README.md)
-   [Latest Results](SUMMARY.md)
