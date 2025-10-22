# VT Code Benchmarks

This directory contains benchmark results and documentation for evaluating VT Code's code generation capabilities.

## Overview

VT Code is evaluated on industry-standard benchmarks to measure:
- **Code Generation Quality**: Correctness and functionality of generated code
- **Performance**: Response latency and throughput
- **Cost Efficiency**: Token usage and API costs across providers

## HumanEval Benchmark

[HumanEval](https://github.com/openai/human-eval) is a benchmark for evaluating code generation models on 164 hand-written programming problems. Each problem includes:
- Function signature and docstring
- Unit tests to verify correctness
- Pass@1 metric (percentage of problems solved on first attempt)

### Latest Results (2025-10-22)

**Model:** `gemini-2.5-flash-lite`  
**Provider:** Google Gemini  
**Configuration:** `temperature=0.0`, `max_output_tokens=1024`, `seed=42`

![Benchmark Results Chart](../../reports/HE_20251022-135834_gemini-2.5-flash-lite_tools-0_N164_chart.png)

#### Performance Metrics

| Metric | Value |
|--------|-------|
| **Pass@1** | **61.6%** |
| Tasks Completed | 164/164 |
| Tests Passed | 101 |
| Tests Failed | 63 |
| Median Latency (P50) | 0.973s |
| P90 Latency | 1.363s |

#### Cost Analysis

| Metric | Value |
|--------|-------|
| Input Tokens | 0* |
| Output Tokens | 0* |
| Estimated Cost | $0.0000 |

> **Note:** Token counts are not currently reported by vtcode. The model is in Google's free tier, so actual cost is $0.

### Comparison with Other Models

| Model | Pass@1 | Latency (P50) | Cost (est.) |
|-------|--------|---------------|-------------|
| gemini-2.5-flash-lite | 61.6% | 0.97s | $0.00 |
| *More results coming soon* | - | - | - |

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

| Variable | Default | Description |
|----------|---------|-------------|
| `PROVIDER` | `gemini` | LLM provider (gemini, openai, anthropic, etc.) |
| `MODEL` | `gemini-2.5-flash-lite` | Model identifier |
| `N_HE` | `164` | Number of tasks to run (max 164) |
| `SEED` | `1337` | Random seed for reproducibility |
| `USE_TOOLS` | `0` | Enable tool usage (0=disabled, 1=enabled) |
| `TEMP` | `0.0` | Temperature for sampling |
| `MAX_OUT` | `1024` | Maximum output tokens |
| `TIMEOUT_S` | `120` | Timeout per task in seconds |
| `SLEEP_MS` | `0` | Sleep between tasks (ms) |
| `RETRY_MAX` | `2` | Maximum retry attempts |
| `BACKOFF_MS` | `500` | Backoff delay for retries (ms) |
| `INPUT_PRICE` | `0.0` | Cost per 1k input tokens (USD) |
| `OUTPUT_PRICE` | `0.0` | Cost per 1k output tokens (USD) |

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
- Metadata (model, provider, configuration)
- Summary statistics (pass@1, latency, cost)
- Individual task results (passed/failed, errors, timing)

### Known Issues

1. **Token Counting**: vtcode doesn't currently report token usage from the LLM API
2. **Stderr Pollution**: Fixed in v0.30.4 - .env loading message no longer pollutes output
3. **CLI Flags**: `--temperature` and `--max-output-tokens` not supported by `ask` command

### Future Work

- [ ] Add support for more benchmarks (MBPP, CodeContests)
- [ ] Multi-model comparison dashboard
- [ ] Token usage tracking and reporting
- [ ] Cost optimization analysis
- [ ] Performance profiling and optimization

## Contributing

To add new benchmarks or improve existing ones:

1. Add benchmark script to `scripts/`
2. Document methodology in this directory
3. Update Makefile with new targets
4. Submit PR with results and analysis

## References

- [HumanEval Paper](https://arxiv.org/abs/2107.03374) - Original benchmark paper
- [OpenAI HumanEval](https://github.com/openai/human-eval) - Official implementation
- [Benchmark Scripts](../../scripts/) - VT Code benchmark implementations
