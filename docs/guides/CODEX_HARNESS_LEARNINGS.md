# Codex Model Harness Learnings

This document summarizes key learnings from Cursor's blog post on [improving their agent harness for OpenAI Codex models](https://cursor.com/blog/codex-model-harness) and how VT Code has applied them.

## Key Insights from Cursor

### 1. Shell-Forward Approach
Codex models are trained on shell-oriented workflows. They favor tool names that mirror shell equivalents like `rg` (ripgrep).

**VT Code Implementation**:
- Tool naming aligned with shell equivalents (`grep_file`, `list_files`, `rg` preference)
- Instructions encourage `rg` over `grep` for pattern matching
- Unified tools (`unified_search`, `unified_file`, `unified_exec`) map to familiar operations

### 2. Preambles / Reasoning Summaries
Codex models use reasoning summaries instead of standard responses during tool calling. Balance is key—allow users to follow progress without spam.

**VT Code Implementation**:
- System prompt limits preambles to 1-2 sentences max
- Guidelines to note discoveries and tactic changes
- Explicit instruction: "Do NOT comment on your own communication patterns"

### 3. Reading Lints (CRITICAL)
Providing Codex with tool definitions alone is insufficient. Clear, literal instructions are needed.

**VT Code Implementation**:
```
**Error checking (CRITICAL for Codex models)**:
- AFTER editing files, run the appropriate linter/type-checker for the language:
  - Rust: `cargo check` or `cargo clippy`
  - TypeScript/JavaScript: `npx tsc --noEmit` or `npm run lint`
  - Python: `ruff check` or `mypy`
  - Go: `go build ./...` or `golangci-lint run`
- Do NOT wait for user to report errors—proactively catch them
- If linter returns issues in files you edited, fix them immediately
```

### 4. Preserving Reasoning Traces (30% Performance Impact)
Codex models experience severe performance degradation (~30%) when reasoning traces are dropped between tool calls. This is far worse than standard GPT-5 (~3% degradation).

**VT Code Implementation**:
- `reasoning_details` field preserved on `LLMResponse` and `LLMMessage`
- `build_standard_responses_payload()` and `build_codex_responses_payload()` both inject `reasoning_details` from previous assistant messages
- Documentation in `responses_api.rs` explains the criticality of trace preservation

### 5. Biasing for Action
Models should autonomously proceed rather than waiting for permission.

**VT Code Implementation**:
```
**Bias for action** (CRITICAL for autonomous operation):
- Do NOT ask "would you like me to..." or "should I proceed?"—just do it
- Do NOT ask for permission to read files, run tests, or make edits
- If you have the tools and context to complete a task, complete it
```

### 6. Message Ordering
OpenAI models prioritize system prompts over user messages. Avoid system instructions that could accidentally contradict user requests.

**VT Code Implementation**:
- System prompts focus on behavior patterns, not restrictive rules
- Avoid instructions like "don't waste tokens" that could impede ambitious tasks
- User prompts take precedence via configuration precedence chain

## Files Modified

1. **`vtcode-core/src/prompts/system.rs`**
   - Added explicit `get_errors` instructions for Codex
   - Strengthened "bias for action" language
   - Enhanced preamble/reasoning summary guidelines

2. **`vtcode-core/src/llm/providers/openai/responses_api.rs`**
   - Added module documentation about reasoning trace criticality
   - Fixed `build_codex_responses_payload()` to inject reasoning traces (was missing)

## Verification

To verify reasoning trace preservation:
```bash
# Run tests
cargo test --package vtcode-core reasoning

# Check for reasoning_details handling
rg "reasoning_details" vtcode-core/src/llm/
```

## References

- [Cursor Blog: Improving agent for OpenAI Codex models](https://cursor.com/blog/codex-model-harness)
- [OpenAI Responses API Cookbook](https://cookbook.openai.com/examples/responses_api/reasoning_items)
- [OpenAI Migration Guide](https://platform.openai.com/docs/guides/migrate-to-responses)
