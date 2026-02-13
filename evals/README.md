# VTCode Empirical Evaluation Framework

This directory contains the tools and test cases for performing empirical evaluations of the `vtcode` agent. The framework allows you to measure model performance across categories like safety, logic, coding, and instruction following.

## Getting Started

### Prerequisites

1.  **Build vtcode**: Ensure you have a compiled release binary of `vtcode`.
    ```bash
    cargo build --release
    ```
2.  **Python Environment**: The evaluation engine requires Python 3.
3.  **API Keys**: Set the necessary environment variables (e.g., `GEMINI_API_KEY`, `OPENAI_API_KEY`) in a `.env` file in the project root.

### Running Evaluations

The `eval_engine.py` script orchestrates the evaluation process.

**Basic Usage:**
```bash
python3 evals/eval_engine.py --cases evals/test_cases.json --provider gemini --model gemini-3-flash-preview
```

**Arguments:**
- `--cases`: Path to the test cases JSON file (default: `evals/test_cases.json`).
- `--provider`: The LLM provider to evaluate (e.g., `gemini`, `openai`, `anthropic`).
- `--model`: The specific model ID to evaluate (e.g., `gemini-3-flash-preview`, `gpt-4`).

## Directory Structure

- `eval_engine.py`: The main orchestrator that runs test cases and generates reports.
- `metrics.py`: Contains grading logic and metric implementations.
- `test_cases.json`: The primary benchmark suite.
- `test_cases_mini.json`: A smaller suite for quick validation of the framework.
- `reports/`: Automatically created directory where evaluation results are saved as JSON files.

## Test Case Format

Test cases are defined in JSON format:

```json
{
  "id": "logic_fibonacci",
  "category": "logic",
  "task": "Write a python function to calculate the nth fibonacci number.",
  "metric": "code_validity",
  "language": "python"
}
```

### Supported Metrics
- `exact_match`: Checks if the output exactly matches the `expected` string.
- `contains_match`: Checks if the output contains the `expected` string.
- `code_validity`: Checks if the code within markdown blocks is syntactically correct (supports `python`).
- `llm_grader`: Uses the LLM itself to grade the response based on a `rubric`.

## Analyzing Reports

Reports are saved in the `reports/` directory with a timestamp. They include:
- **Summary**: Total tests, passed, and failed counts.
- **Results**: Detailed breakdown for each test case, including:
    - `output`: The raw agent response.
    - `usage`: Token usage metadata.
    - `latency`: Response time in seconds.
    - `grade`: The score or result from the metric.
    - `reasoning`: The agent's thinking process (if supported by the model).
    - `raw_response`: The complete JSON response from `vtcode ask`.

## Grading with LLMs

The `llm_grader` metric uses `vtcode ask` internally to perform evaluations. By default, it uses `gemini-3-flash-preview` for grading to keep costs low and ensure reliability. You can configure this in `evals/metrics.py`.
