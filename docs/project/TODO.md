Analyze the codebase to identify and improve panic-prone code patterns including force unwraps (!), force try, expect() calls, implicitly unwrapped optionals, and fatalError usages. For each occurrence, assess whether it can be replaced with safer alternatives such as guard statements, optional binding, custom error handling, Result types, or default values. Provide refactored code examples for each improvement, explain the potential runtime failures being prevented, and suggest coding standards to minimize future unsafe patterns. Include analysis of any custom panic handlers or recovery mechanisms that could be implemented.

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, context and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

---

Conduct a thorough, end-to-end performance audit and systematic optimization of the vtcode agent harness framework with explicit focus on maximizing execution velocity, achieving superior computational efficiency, and implementing aggressive token and context conservation strategies throughout all operational layers. Execute comprehensive refactoring of the tool invocation and agent communication architecture to eliminate redundant processing, minimize inter-process communication latency, and optimize resource utilization at every stage. Design and implement multilayered error handling protocols including predictive failure detection, graceful degradation mechanisms, automatic recovery procedures, and comprehensive logging to drive error occurrence to near-zero levels. Deliver measurable improvements in reliability, throughput, and operational stability while preserving all existing functionality and maintaining backward compatibility with current integration points.

---

extract and open source more components from vtcode-core

---

Shell integration: respects $SHELL environment variable and supports the ! prefix for direct shell execution.

---

Keyboard-first navigation: Full UNIX keybinding support (Ctrl+A/E/W/U/K, Alt+arrows), suspend/resume with Ctrl+Z, and a quick help overlay with ?.

---

Review the unified_exec implementation and vtcode's tool ecosystem to identify token efficiency gaps. Analyze which components waste tokens through redundancy, verbosity, or inefficient patterns, and which are already optimized. Develop optimizations for inefficient tools and propose new tools that consolidate multiple operations into single calls to reduce token consumption in recurring workflows.

Specifically examine these known issues: command payloads for non-diff unified_exec still contain duplicated text (output and stdout fields), which wastes tokens across all command-like tool calls. Address this by ensuring unified_exec normalizes all tool calls to eliminate redundant information.

Identify and address these additional token waste patterns: remove duplicated spool guidance that reaches the model both through spool_hint fields and separate system prompts; trim repeated or unused metadata from model-facing tool payloads such as redundant spool_hint fields, spooled_bytes data, duplicate id==session_id entries, and null working_directory values; shorten high-frequency follow-up prompts for PTY and spool-chunk read operations, and implement compact structured continuation arguments for chunked spool reads.

Review each tool's prompt and response structure to ensure conciseness while maintaining effectiveness, eliminating unnecessary verbosity that increases token usage without adding functional value.

---

implement realtime status line config, also add /statusline command to show it

```
Configure Status Line
  Select which items to display in the status lin

  Type to search
  >
› [x] current-dir           Current working dire…
  [x] git-branch            Current Git branch (…
  [ ] model-name            Current model name
  [ ] model-with-reasoning  Current model name w…
  [ ] project-root          Project root directo…
  [ ] context-remaining     Percentage of contex…
  [ ] context-used          Percentage of contex…
  [ ] five-hour-limit       Remaining usage on 5…

  ~/project/path · feat/awesome-feature
  Use ↑↓ to navigate, ←→ to move, space to select,
```

---

Implement a comprehensive solution to resolve the escape key conflict in external editor functionality and expand configurable editor support. First, modify the crossterm event handling system to implement context-aware escape key detection that distinguishes between escape key presses intended for normal mode navigation and those that should trigger rewind functionality. Consider implementing either a configurable double-escape mechanism where a single press exits external editor mode while a double-press triggers rewind, or introduce an alternative keybinding such as Ctrl+Shift+R or F5 for rewind that does not conflict with escape behavior. Ensure VTCode/ANSI escape sequence parsing correctly identifies the source of escape key events. Second, expand the external editor configuration system to include a user-customizable editor preference setting stored in the application configuration. This setting should accept any valid shell command or path to an executable, and the system should parse this command to launch the specified editor when Ctrl+E is triggered. Implement support for launching common editors including Visual Studio Code (code command), Zed (zed command), TextEdit (open command on macOS), Sublime Text (subl command), TextMate (mate command), Emacs (emacsclient or emacs command), Neovim (nvim or vim command), Nano (nano command), and any other editor specified via custom command. The implementation should handle platform-specific editor detection, manage editor process spawning and termination, capture editor output, and properly restore focus to the main application after the external editor session completes. Include error handling for cases where the specified editor is not installed or the command fails to execute.

---

Perform a comprehensive analysis of the codebase to identify and eliminate all instances of duplicated code, following the DRY (Don't Repeat Yourself) and KISS (Keep It Simple, Stupid) principles. Conduct a systematic search across all modules, classes, and files to find similar code patterns, duplicate logic, redundant implementations, and opportunities for abstraction. Specifically examine rendering-related code such as diff previews and command output previews to determine if they can share unified rendering logic, styling, and common components. Audit all utility functions scattered throughout different modules and extract them into a centralized shared utility module with proper organization and documentation. Create a detailed report identifying each duplication found, the proposed refactoring strategy, and the expected benefits in terms of maintainability, reduced code complexity, and improved consistency. Ensure all refactored code maintains existing functionality while simplifying the overall architecture. Prioritize changes that provide the greatest reduction in duplication with minimal risk to existing functionality.

---

IMPLEMENT POSTAMBLE CROSSTERM EXIT SUMMARIAZATION

REF:

```
 Total usage est:        5 Premium
 requests
 API time spent:         14m 55s
 Total session time:     1h 13m 43s
 Total code changes:     +137 -13
 Breakdown by AI model:
  gpt-5.3-codex           10.0m in,
  40.1k out, 8.9m cached (Est. 5
  Premium requests)

 Resume this session with copilot
 --resume=ce712ef0-6605-4cd4-82bf-fad39
 bd55bca
```

---

review any duplicated code in the codebase and refactor to remove duplication. For example, the logic for rendering the diff preview and the command output preview can be unified to use the same rendering logic and styling. This will make the codebase cleaner and easier to maintain. Additionally, any common utility functions that are duplicated across different modules can be extracted into a shared utility module. search across modules for similar code patterns and identify opportunities for refactoring to reduce duplication and improve code reuse.

DRY and KISS

---

improve agent's postamlbe summarization. reference and also suggest next actions based on the summary. This will help users quickly understand the outcome of the agent's actions and what they can do next. And for the agent itself.

```
• Implemented. I tightened theme color distribution and readability logic across
  both vtcode-tui and vtcode-core.

  - Added luminance balancing for rendered text/accent styles (foreground,
    primary, secondary, user, response, reasoning, alert) relative to background:
      - vtcode-tui/src/ui/theme.rs:118:118
      - vtcode-core/src/ui/theme.rs:118:118
  - Added explicit luminance adjustment helpers:
      - vtcode-tui/src/ui/theme.rs:1414:1414
      - vtcode-core/src/ui/theme.rs:1373:1373
  - Strengthened ensure_contrast with final black/white high-contrast fallback so
    accessibility floor is always met:
      - vtcode-tui/src/ui/theme.rs:1450:1450
      - vtcode-core/src/ui/theme.rs:1409:1409
  - Added full-theme regression tests to enforce contrast + readability bounds
    across all registered themes:
      - vtcode-tui/src/ui/theme.rs:1781:1781
      - vtcode-core/src/ui/theme.rs:1726:1726

  Validation run:
  - cargo check -p vtcode-core passed
  - cargo test -p vtcode-tui
    ui::theme::tests::test_all_themes_have_readable_foreground_and_accents --
    --nocapture passed

  Note:

  - I also noticed an unrelated local config change in vtcode.toml (theme changed
    to ciapre). I left it untouched.
    ``
```

the goal is to provide users with a clear and concise summary of the agent's actions, including any relevant metrics such as API usage, time spent, and code changes. Additionally, the summary should suggest next steps for the user to take based on the outcome of the agent's actions. This could include options to resume the session, view detailed logs, or take specific actions based on the results of the agent's work. The postamble summarization should be designed to help users quickly understand the results of their interactions with the agent and guide them towards meaningful next steps. The tone should be informative and actionable, providing users with the information they need to make informed decisions about how to proceed after the agent has completed its tasks. Not too verbose, but comprehensive enough to cover the key outcomes and next steps. Not too blunt, but clear and concise.

---

---

improve file brower selection -> user input, it should show @file alias but full path context for agent. the agent should not confusedd by the @file alias and should be able to understand the full path context. This will help the agent to accurately identify the file and perform the necessary actions on it. Additionally, the UI can display the @file alias for better readability and user experience, while still providing the full path context in the background for the agent's processing.

---

Conduct a comprehensive review and enhancement of error handling and recovery mechanisms within the agent loop, with particular emphasis on tool call operations. Implement a multi-layered error handling strategy that includes retry logic with exponential backoff for transient failures such as network timeouts, rate limiting, and temporary service unavailability while implementing fail-fast behavior for non-recoverable errors including authentication failures, invalid parameters, and permission denied scenarios. Develop and integrate a robust state management system that ensures the agent can maintain consistent internal state during and after error occurrences, including proper rollback mechanisms for partial operations and transaction-like semantics where appropriate. Create a comprehensive error categorization system that distinguishes between retryable and non-retryable errors and implements appropriate handling strategies for each category. Enhance user-facing error messages to be clear, actionable, and informative while avoiding technical jargon that may confuse end users. Implement proper logging at multiple levels including debug, info, warning, and error levels to facilitate troubleshooting and monitoring. Conduct a thorough audit of existing error handling implementations to identify gaps, inconsistencies, and potential failure points. Refactor the error handling code to improve modularity, testability, and maintainability while ensuring comprehensive test coverage for error scenarios including edge cases and unexpected inputs. Add appropriate circuit breaker patterns for external service calls to prevent cascading failures and enable graceful degradation when dependent services are unavailable. Implement proper resource cleanup and resource leak prevention throughout the agent loop.

---

check src/agent/runloop/unified/turn module Analyze the agent harness codebase focusing on the runloop, unified, turn, and tool_outcomes components to identify performance bottlenecks, inefficiencies, and optimization opportunities. Perform a comprehensive review of data flow and control flow through these components, examining how tool calls are executed, how outcomes are processed, and how turn execution manages state and sequencing. Evaluate whether the current implementation maximizes parallelism where possible, minimizes blocking operations, and maintains efficient memory usage patterns. Identify any redundant computational steps, unnecessary data transformations, or algorithmic inefficiencies that degrade performance. Assess the current error handling mechanisms for robustness, examining exception propagation paths, retry logic, and failure recovery procedures to ensure they do not introduce excessive latency or create cascading failure scenarios. Examine the design of core data structures used throughout these components for optimal access patterns, memory efficiency, and scalability characteristics. Provide specific, actionable recommendations for refactoring code to reduce complexity, implementing caching where appropriate to avoid redundant computation, optimizing hot path execution, and improving the overall responsiveness and throughput of the agent harness. Your analysis should include concrete code-level suggestions with estimated impact on performance metrics and potential tradeoffs to consider when implementing optimizations.

---

update color scheme for unify diff bg and +/ gutter colors to improve readability and reduce eye strain during code reviews. Suggested colors:

green: 223a2b
red: A63333
