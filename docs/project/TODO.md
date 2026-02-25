https://github.com/google/bloaty

--

scan to improve panic and exception handling like force unwrap and expect.

---

implement self update logic for vtcode.

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

--

improve and fix syntax highlighting in raw markdown code
/Users/vinhnguyenxuan/Documents/vtcode-resources/wrong_syntax_highlight.png

---

check error
https://github.com/vinhnx/VTCode/issues/605#issuecomment-3942895952

---

---

idea: maybe align plan_task_tracker mode with task_tracker as subsystem for agent planning and execution flow.

---

improve system prompt "/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/plans/improve-system-prompt-v2.md"

---

## Summary

Goal: improve speed, token efficiency, and tool-call reliability while
minimizing regression risk.
Strategy: fix correctness bugs first, then reduce prompt/tool overhead with
config-compatible, surgical changes.

## Key Findings Driving This Plan

- is_valid_tool does not enforce policy result (it calls policy check and
  ignores return), causing false “valid” decisions and avoidable failures/
  retries.
- Core runner path builds full tool declarations without using cached/mode-
  aware docs (tool_documentation_mode), wasting tokens and startup work.
- Prompt/tool guidance has naming drift (ask_user_question vs ask_questions vs
  request_user_input) and legacy references, increasing tool-call mistakes.
- STRUCTURED_REASONING_INSTRUCTIONS is always appended, including lightweight/
  minimal flows, adding fixed token overhead.
- Loop/retry behavior is split across layers, with some non-retryable failures
  still retried.

--

maybe remove whole tree-sitter

---

check docs/BLOATY_ANALYSIS.md and optimize

```

## Recommendations

### 1. **Tree-sitter Optimization** (Potential savings: ~3-4 MiB)
- Consider loading tree-sitter grammars dynamically instead of embedding all parse tables
- Only include language parsers that are actually needed
- Use `tree-sitter` feature flags more granularly

### 2. **String Optimization** (Potential savings: ~300-500 KiB)
- Review string literals in code (`__cstring` section is 329 KiB in release)
- Consider using `&'static str` constants instead of `String` where possible
- Use string interning for repeated strings

### 3. **Code Size Optimization** (Potential savings: 1-2 MiB)
- Enable more aggressive LTO: `lto = "fat"` (currently `lto = "thin"` in release-fast)
- Use `opt-level = "z"` or `opt-level = "s"` for size optimization in non-critical paths
- Review large constant data structures (like `base_function_declarations` at 192 KiB)

### 4. **Debug Info Management**
- The `bloaty` profile has been added to Cargo.toml for analysis builds
- Consider using `split-debuginfo` for production to reduce binary size while keeping debug info separate

### 5. **Dependency Audit**
- Run `cargo bloat` (different tool) to identify which dependencies contribute most to binary size
- Consider replacing heavy dependencies with lighter alternatives where possible
```
