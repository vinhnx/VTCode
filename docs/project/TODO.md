scan to improve panic and exception handling like force unwrap and expect.

---

implement self update logic for vtcode.

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

--

improve and fix syntax highlighting in raw markdown code
/Users/vinhnguyenxuan/Documents/vtcode-resources/wrong_syntax_highlight.png

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

---

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/docs/BLOATY_ANALYSIS.md

---

extract tui modules to separate files for better organization and maintainability. Each module can have its own file with clear naming conventions, making it easier to navigate and manage the codebase as the project grows. and also to reduce the size of the main tui file, improving readability and reducing cognitive load when working on specific features or components.

also consider open sourcing the tui modules as a standalone library for other projects to use, which would require further refactoring and documentation to ensure it is reusable and adaptable to different contexts.

---

I see the problem now. The codebase has two separate ModelId enums:

     1. `vtcode-config/src/models/model_id.rs` - Used by vtcode-config crate
     2. `vtcode-core/src/config/models/model_id/definition.rs` - Used by
        vtcode-core crate

    These are NOT unified - they're completely separate enums that need to be
    kept in sync manually. This is indeed a design issue, but it's the current
    architecture.

    I've already added GPT53Codex to both enums:
     - ✅ vtcode-config/src/models/model_id.rs (line 41)
     - ✅ vtcode-core/src/config/models/model_id/definition.rs (line 22)

    And all the corresponding implementations in both crates. The build
    succeeded, which means both enums are now in sync.

    Why this architecture exists:
     - vtcode-config handles configuration, constants, and model metadata
     - vtcode-core handles the core agent logic, LLM providers, and runtime
       behavior
     - They're separate crates that can be used independently

    The proper fix would be to have vtcode-core re-export ModelId from
    vtcode-config, but that would be a larger refactoring. For now, both enums
    have been updated with GPT53Codex.
