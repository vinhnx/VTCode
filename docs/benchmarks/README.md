# VT Code Benchmarks

This directory contains benchmark results and documentation for evaluating VT Code's code generation capabilities.

## Overview

VT Code is evaluated on industry-standard benchmarks to measure:

-   **Code Generation Quality**: Correctness and functionality of generated code
-   **Performance**: Response latency and throughput
-   **Cost Efficiency**: Token usage and API costs across providers

## HumanEval Benchmark

[HumanEval](https://github.com/openai/human-eval) is a benchmark for evaluating code generation models on 164 hand-written programming problems. Each problem includes:

-   Function signature and docstring
-   Unit tests to verify correctness
-   Pass@1 metric (percentage of problems solved on first attempt)

### Latest Results (October 2025)

**MAJOR ACHIEVEMENT: gpt-5-nano achieves frontier-tier performance (94.5%)**

![Comparison Chart](../../docs/benchmarks/reports/comparison_gemini-2.5-flash-lite_vs_gpt-5-nano.png)

**Two models benchmarked:**

| Model                 | Provider | Pass@1    | Passed  | Failed | Latency (P50) | Cost           |
| --------------------- | -------- | --------- | ------- | ------ | ------------- | -------------- |
| **gpt-5-nano**        | OpenAI   | **94.5%** | 155/164 | 9/164  | 10.4s         | ~$0.10-0.30/1M |
| gemini-2.5-flash-lite | Google   | 61.6%     | 101/164 | 63/164 | 0.97s         | $0.00 (free)   |

**Configuration:** `temperature=0.0`, `seed=42`, `timeout=120s`

#### Key Findings

**gpt-5-nano:**

-   Frontier-tier performance (94.5%)
-   TOP 5 globally
-   Very affordable (~$0.10-0.30/1M tokens)
-   10-50x cheaper than premium competitors
-   10.4s median latency

**gemini-2.5-flash-lite:**

-   10x faster (0.97s)
-   Completely FREE (Google free tier)
-   Good for development (61.6%)
-   Perfect for rapid iteration
-   Ideal for high-volume testing

**Strategic Choice:**

-   Use **gpt-5-nano** for production validation and critical tasks
-   Use **gemini-2.5-flash-lite** for development and prototyping

See [GPT5_NANO_VS_GEMINI.md](GPT5_NANO_VS_GEMINI.md) for detailed comparison.
| Estimated Cost | $0.0000 |

> **Note:** Token counts are not currently reported by vtcode. The model is in Google's free tier, so actual cost is $0.

### Comparison with Other Models

| Model                      | Pass@1 | Latency (P50) | Cost (est.) |
| -------------------------- | ------ | ------------- | ----------- |
| gemini-2.5-flash-lite      | 61.6%  | 0.97s         | $0.00       |
| _More results coming soon_ | -      | -             | -           |

### Methodology

1. **Dataset**: Complete HumanEval dataset (164 problems)
2. **Prompt Format**: Raw code-only format optimized for Gemini
3. **Evaluation**: Automated test execution with Python unittest
4. **Reproducibility**: Fixed seed (42) for deterministic sampling
5. **Rate Limiting**: 500ms sleep between tasks to respect API limits

### Running Benchmarks

#### Prerequisites

```bash
# Install Python dependencies
pip install datasets

# Ensure vtcode is built
cargo build --release
```

#### Basic Usage

```bash
# Run full benchmark (164 tasks)
make bench-humaneval PROVIDER=gemini MODEL='gemini-2.5-flash-lite'

# Run subset for quick testing
make bench-humaneval PROVIDER=gemini MODEL='gemini-2.5-flash-lite' N_HE=10

# Run with custom parameters
make bench-humaneval \
  PROVIDER=openai \
  MODEL='gpt-5' \
  N_HE=50 \
  SEED=42 \
  SLEEP_MS=500 \
  RETRY_MAX=3
```

#### Environment Variables

| Variable       | Default                 | Description                                    |
| -------------- | ----------------------- | ---------------------------------------------- |
| `PROVIDER`     | `gemini`                | LLM provider (gemini, openai, anthropic, etc.) |
| `MODEL`        | `gemini-2.5-flash-lite` | Model identifier                               |
| `N_HE`         | `164`                   | Number of tasks to run (max 164)               |
| `SEED`         | `1337`                  | Random seed for reproducibility                |
| `USE_TOOLS`    | `0`                     | Enable tool usage (0=disabled, 1=enabled)      |
| `TEMP`         | `0.0`                   | Temperature for sampling                       |
| `MAX_OUT`      | `1024`                  | Maximum output tokens                          |
| `TIMEOUT_S`    | `120`                   | Timeout per task in seconds                    |
| `SLEEP_MS`     | `0`                     | Sleep between tasks (ms)                       |
| `RETRY_MAX`    | `2`                     | Maximum retry attempts                         |
| `BACKOFF_MS`   | `500`                   | Backoff delay for retries (ms)                 |
| `INPUT_PRICE`  | `0.0`                   | Cost per 1k input tokens (USD)                 |
| `OUTPUT_PRICE` | `0.0`                   | Cost per 1k output tokens (USD)                |

#### Visualization

Generate charts and summaries from results:

```bash
# Generate ASCII chart and markdown summary
python3 scripts/render_benchmark_chart.py reports/HE_*.json

# View latest results
cat reports/HE_*_summary.md
```

### Results Archive

All benchmark results are stored in the `reports/` directory with the naming convention:

```
HE_YYYYMMDD-HHMMSS_<model>_tools-<0|1>_N<count>.json
```

Each report includes:

-   Metadata (model, provider, configuration)
-   Summary statistics (pass@1, latency, cost)
-   Individual task results (passed/failed, errors, timing)

### Known Issues

1. **Token Counting**: vtcode doesn't currently report token usage from the LLM API
2. **Stderr Pollution**: Fixed in v0.30.4 - .env loading message no longer pollutes output
3. **CLI Flags**: `--temperature` and `--max-output-tokens` not supported by `ask` command

### Future Work

-   [ ] Add support for more benchmarks (MBPP, CodeContests)
-   [ ] Multi-model comparison dashboard
-   [ ] Token usage tracking and reporting
-   [ ] Cost optimization analysis
-   [ ] Performance profiling and optimization

## Contributing

To add new benchmarks or improve existing ones:

1. Add benchmark script to `scripts/`
2. Document methodology in this directory
3. Update Makefile with new targets
4. Submit PR with results and analysis

## References

-   [HumanEval Paper](https://arxiv.org/abs/2107.03374) - Original benchmark paper
-   [OpenAI HumanEval](https://github.com/openai/human-eval) - Official implementation
-   [Benchmark Scripts](../../scripts/) - VT Code benchmark implementations
