# Context Management Strategy Analysis and Plan

This document analyzes the application of advanced context management strategies to `vtcode`.

## Strategy Analysis

| Strategy | Description | Current `vtcode` Status | Plan / Gaps |
| :--- | :--- | :--- | :--- |
| **Context Offloading** | Move parts of context to external storage. | Partial. `TokenBudgetManager` and `ContextManager` exist for trimming. No active "offloading" to external file system for context chunks. | **Low Priority**. Current budget management is decent. Storage offloading is complex. |
| **Context Reduction (Compaction)** | Reversible reduction (paths, IDs). | **Implemented**. `adaptive_trim` and `prune_unified_tool_responses` compact tool outputs. | Continue refining `prune_unified_tool_responses`. |
| **Context Reduction (Summarization)** | Irreversible compression (summaries). | **Implemented**. `adaptive_trim` triggers `SummarizationRecommended` phase, which proactive guards handle by summarizing via LLM. | **Done**. Active loop implemented. |
| **Context Isolation (Sub-agents)** | Split tasks across independent agents. | **No**. Single agent loop structure. | **Future**. implementation of multi-agent/sub-agent architecture is a larger architectural change. |
| **Context Retrieval** | Retrieve offloaded info via search. | **Implemented**. `SessionArchive` now supports `search_sessions` to find keywords in past conversations. | **Done**. Utility implemented. |
| **Context Caching** | Store KV states (provider caching). | **Implemented**. `AnthropicProvider` handles `prompt-caching-2024-07-31` and breakpoints on System Prompt + User messages. | **Maintain**. Ensure `ContextManager` token estimation accounts for cache hits if possible (optimization). |
| **Layered Action Space** | Offload complexity to scripts/sandbox. | **Partial**. `loaded_skills` and `run_pty_cmd` exist. | **Enhance**. Formalize "Skills" as scripts that can be invoked to offload complex multi-step actions. |

## Implementation Plan

### 1. Enable Active Context Summarization (Immediate)

The `RetentionDecision::Summarizable` currently does nothing. We will change this to actively verify and summarize chunks of conversation.

**Steps:**
1.  **Update `vtcode-core`**: Add `Summarize` variant to `RetentionChoice`.
2.  **Update `ContextTrimOutcome`**: Add `SummarizationRecommended` phase.
3.  **Refactor `TurnProcessingContext`**: Inject `provider_client` to allow `run_proactive_guards` to make LLM calls.
4.  **Implement Summarization Logic**:
    *   In `run_proactive_guards`: If `SummarizationRecommended`, call LLM to summarize.
    *   Replace range with a single `Summary` message.

### 2. Verify and Optimize Context Caching

*   `AnthropicProvider` logic is active.

### 3. Layered Action Space (Skills)

*   Enhance "Skills" usage.
