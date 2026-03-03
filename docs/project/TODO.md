NOTE: use private relay signup codex free

---

NOTE: use deepwiki mcp to reference from codex https://deepwiki.com/openai/codex

---

Analyze the codebase to identify and improve panic-prone code patterns including force unwraps (!), force try, expect() calls, implicitly unwrapped optionals, and fatalError usages. For each occurrence, assess whether it can be replaced with safer alternatives such as guard statements, optional binding, custom error handling, Result types, or default values. Provide refactored code examples for each improvement, explain the potential runtime failures being prevented, and suggest coding standards to minimize future unsafe patterns. Include analysis of any custom panic handlers or recovery mechanisms that could be implemented.

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, context and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

---

Conduct a thorough, end-to-end performance audit and systematic optimization of the vtcode agent harness framework with explicit focus on maximizing execution velocity, achieving superior computational efficiency, and implementing aggressive token and context conservation strategies throughout all operational layers. Execute comprehensive refactoring of the tool invocation and agent communication architecture to eliminate redundant processing, minimize inter-process communication latency, and optimize resource utilization at every stage. Design and implement multilayered error handling protocols including predictive failure detection, graceful degradation mechanisms, automatic recovery procedures, and comprehensive logging to drive error occurrence to near-zero levels. Deliver measurable improvements in reliability, throughput, and operational stability while preserving all existing functionality and maintaining backward compatibility with current integration points.

---

extract and open source more components from vtcode-core

---

Review the unified_exec implementation and vtcode's tool ecosystem to identify token efficiency gaps. Analyze which components waste tokens through redundancy, verbosity, or inefficient patterns, and which are already optimized. Develop optimizations for inefficient tools and propose new tools that consolidate multiple operations into single calls to reduce token consumption in recurring workflows.

Specifically examine these known issues: command payloads for non-diff unified_exec still contain duplicated text (output and stdout fields), which wastes tokens across all command-like tool calls. Address this by ensuring unified_exec normalizes all tool calls to eliminate redundant information.

Identify and address these additional token waste patterns: remove duplicated spool guidance that reaches the model both through spool_hint fields and separate system prompts; trim repeated or unused metadata from model-facing tool payloads such as redundant spool_hint fields, spooled_bytes data, duplicate id==session_id entries, and null working_directory values; shorten high-frequency follow-up prompts for PTY and spool-chunk read operations, and implement compact structured continuation arguments for chunked spool reads.

Review each tool's prompt and response structure to ensure conciseness while maintaining effectiveness, eliminating unnecessary verbosity that increases token usage without adding functional value.

---

Implement a comprehensive solution to resolve the escape key conflict in external editor functionality and expand configurable editor support. First, modify the crossterm event handling system to implement context-aware escape key detection that distinguishes between escape key presses intended for normal mode navigation and those that should trigger rewind functionality. Consider implementing either a configurable double-escape mechanism where a single press exits external editor mode while a double-press triggers rewind, or introduce an alternative keybinding such as Ctrl+Shift+R or F5 for rewind that does not conflict with escape behavior. Ensure VTCode/ANSI escape sequence parsing correctly identifies the source of escape key events. Second, expand the external editor configuration system to include a user-customizable editor preference setting stored in the application configuration. This setting should accept any valid shell command or path to an executable, and the system should parse this command to launch the specified editor when Ctrl+E is triggered. Implement support for launching common editors including Visual Studio Code (code command), Zed (zed command), TextEdit (open command on macOS), Sublime Text (subl command), TextMate (mate command), Emacs (emacsclient or emacs command), Neovim (nvim or vim command), Nano (nano command), and any other editor specified via custom command. The implementation should handle platform-specific editor detection, manage editor process spawning and termination, capture editor output, and properly restore focus to the main application after the external editor session completes. Include error handling for cases where the specified editor is not installed or the command fails to execute.

---

Perform a comprehensive analysis of the codebase to identify and eliminate all instances of duplicated code, following the DRY (Don't Repeat Yourself) and KISS (Keep It Simple, Stupid) principles. Conduct a systematic search across all modules, classes, and files to find similar code patterns, duplicate logic, redundant implementations, and opportunities for abstraction. Specifically examine rendering-related code such as diff previews and command output previews to determine if they can share unified rendering logic, styling, and common components. Audit all utility functions scattered throughout different modules and extract them into a centralized shared utility module with proper organization and documentation. Create a detailed report identifying each duplication found, the proposed refactoring strategy, and the expected benefits in terms of maintainability, reduced code complexity, and improved consistency. Ensure all refactored code maintains existing functionality while simplifying the overall architecture. Prioritize changes that provide the greatest reduction in duplication with minimal risk to existing functionality.

---

review any duplicated code in the codebase and refactor to remove duplication. For example, the logic for rendering the diff preview and the command output preview can be unified to use the same rendering logic and styling. This will make the codebase cleaner and easier to maintain. Additionally, any common utility functions that are duplicated across different modules can be extracted into a shared utility module. search across modules for similar code patterns and identify opportunities for refactoring to reduce duplication and improve code reuse.

DRY and KISS

---

Conduct a comprehensive review and enhancement of error handling and recovery mechanisms within the agent loop, with particular emphasis on tool call operations. Implement a multi-layered error handling strategy that includes retry logic with exponential backoff for transient failures such as network timeouts, rate limiting, and temporary service unavailability while implementing fail-fast behavior for non-recoverable errors including authentication failures, invalid parameters, and permission denied scenarios. Develop and integrate a robust state management system that ensures the agent can maintain consistent internal state during and after error occurrences, including proper rollback mechanisms for partial operations and transaction-like semantics where appropriate. Create a comprehensive error categorization system that distinguishes between retryable and non-retryable errors and implements appropriate handling strategies for each category. Enhance user-facing error messages to be clear, actionable, and informative while avoiding technical jargon that may confuse end users. Implement proper logging at multiple levels including debug, info, warning, and error levels to facilitate troubleshooting and monitoring. Conduct a thorough audit of existing error handling implementations to identify gaps, inconsistencies, and potential failure points. Refactor the error handling code to improve modularity, testability, and maintainability while ensuring comprehensive test coverage for error scenarios including edge cases and unexpected inputs. Add appropriate circuit breaker patterns for external service calls to prevent cascading failures and enable graceful degradation when dependent services are unavailable. Implement proper resource cleanup and resource leak prevention throughout the agent loop.

---

check src/agent/runloop/unified/turn module Analyze the agent harness codebase focusing on the runloop, unified, turn, and tool_outcomes components to identify performance bottlenecks, inefficiencies, and optimization opportunities. Perform a comprehensive review of data flow and control flow through these components, examining how tool calls are executed, how outcomes are processed, and how turn execution manages state and sequencing. Evaluate whether the current implementation maximizes parallelism where possible, minimizes blocking operations, and maintains efficient memory usage patterns. Identify any redundant computational steps, unnecessary data transformations, or algorithmic inefficiencies that degrade performance. Assess the current error handling mechanisms for robustness, examining exception propagation paths, retry logic, and failure recovery procedures to ensure they do not introduce excessive latency or create cascading failure scenarios. Examine the design of core data structures used throughout these components for optimal access patterns, memory efficiency, and scalability characteristics. Provide specific, actionable recommendations for refactoring code to reduce complexity, implementing caching where appropriate to avoid redundant computation, optimizing hot path execution, and improving the overall responsiveness and throughput of the agent harness. Your analysis should include concrete code-level suggestions with estimated impact on performance metrics and potential tradeoffs to consider when implementing optimizations.

---

support litellm

https://github.com/majiayu000/litellm-rs

https://docs.litellm.ai/docs/

--

idea: show a info to let user manually switch to edit mode for full tools set, as a fallback if the agent's auto switch from plan -> edit
mode systematically fall.

---

update /status info

example:

```
  Version: {current version here}
  Session ID: {current session id here}
  Directory: {current working directory here}

  Model: {current model here}
  IDE: {determine IDE and version here}
  Terminal: {determine terminal and version here}
  MCP servers: {list MCP servers and versions here}
  Skills: {list agent skills here}
  Memory: project ({AGENTS.md})
  Setting sources: User settings, Project local settings {vtcode.toml}, .vtcode path}
```

---

```

  # Context-Aware LLM Interview Generation for Plan Mode (Codex-Parity)

  ## Summary

  Adopt Codex’s model-driven interview pattern in VT Code: questions/options are primarily LLM-
  generated from exploration context, with deterministic heuristic fallback. Keep strict iterative Plan
  Mode gating so <proposed_plan> is blocked until interview answers close material decisions.

  ## Public API / Interface Changes

  1. New interview synthesis context type (src/agent/runloop/unified/turn/turn_processing/plan_mode.rs
     or sibling module)

  - InterviewResearchContext:
  - discovery_tools_used: Vec<String>
  - recent_files_or_symbols: Vec<String>
  - risk_hints: Vec<String>
  - open_decision_hints: Vec<String>
  - goal_hints: Vec<String>
  - verification_hints: Vec<String>

  2. New async interview synthesis helper in turn processing layer

  - synthesize_plan_mode_interview_args(...) -> Result<Option<serde_json::Value>>
  - Input: InterviewResearchContext, recent assistant text, current user request.
  - Output: request_user_input-compatible payload (questions[1..=3], each with 2-3 options, recommended
    first).

  3. maybe_force_plan_mode_interview signature becomes async and accepts synthesis dependencies

  - Provider client handle + minimal prompt builder input.
  - Returns same TurnProcessingResult, but can now synthesize dynamic args before injecting tool call.

  ## Implementation Plan

  1. Build heuristic research-context extractor.

  - Parse last N user/assistant/tool messages from working_history.
  - Extract structured hints:
  - scope candidates (files/symbols/paths from discovery output),
  - risk/constraint phrases,
  - unresolved-decision markers (Decision needed, Next open decision, explicit “unclear/unknown” cues),
  - validation signal gaps (no concrete command/check).
  - Reuse existing helper families in response_processing.rs (extract_analysis_hints, question
    normalization) instead of duplicating logic.

  2. Add dedicated LLM synthesis pass (hybrid approach).

  - When Plan Mode interview injection is required and model did not already emit valid
    request_user_input, run a compact synthesis call:
  - System instruction: generate 1-3 interview questions, each with 2-3 meaningful mutually exclusive
    options, recommended first, no “Other”.
  - Ground synthesis on InterviewResearchContext.
  - Validate generated JSON against existing request_user_input constraints (snake_case ids, header
    length, one-line question, option count).
  - If synthesis fails or invalid -> fallback to deterministic heuristic templates.

  3. Replace static default interview payload.

  - Remove unconditional default_plan_mode_interview_args() usage in forced injection path.
  - New forced injection precedence:

  1. Use valid model-emitted request_user_input call as-is.
  2. Else use synthesized LLM interview args from research context.
  3. Else use deterministic heuristic fallback payload.

  - Keep current behavior that strips/suppresses assistant text when forcing interview.

  4. Preserve Codex-like model-driven behavior in prompts.

  - Update Plan Mode prompt section (vtcode-core/src/prompts/system.rs) to explicitly require:
  - exploration-first fact gathering,
  - request_user_input for non-discoverable decisions,
  - context-shaped options (tradeoff-oriented, not generic),
  - iterative questioning until decision-complete before final <proposed_plan>.
  - Remove Plan-Mode ambiguity from generic “act without asking” language by adding explicit Plan Mode
    override.

  5. Keep strict iterative gating always-on in Plan Mode.

  - No config toggle.
  - Continue enforcing follow-up interviews when unresolved decision markers remain or interview
    assumption section.
  ## Test Cases and Validation

  1. Unit tests: context extractor

  - Discovery-heavy history yields file/symbol hints.

  2. Unit tests: synthesis pipeline


  3. Unit tests: Plan Mode gating

  - With unresolved decision markers, <proposed_plan> triggers follow-up interview.
  - After answered interview + resolved markers, <proposed_plan> is allowed.
  - Cancelled interview re-prompts on next eligible turn.
  - Recommended option remains first.

  - cargo nextest run -p vtcode-core
  - cargo nextest run -p vtcode-tui
  - cargo clippy --workspace --all-targets -- -D warnings

  ## Assumptions and Defaults

  1. Codex parity means model-driven interview generation first, heuristics second (not static
     defaults).
  2. Additional synthesis pass is acceptable for Plan Mode latency because it runs only when forced-
     interview is needed.
  3. Existing request_user_input schema and UI wizard remain unchanged; only generation/gating logic
     changes.
  4. Enforcement remains always-on in Plan Mode as requested (no feature flag rollout).
```
