NOTE: use private relay signup codex free

---

idea: wrap a 'vtcode update' cli command to replace curl brew cargo install

---

NOTE: use deepwiki mcp to reference from codex https://deepwiki.com/openai/codex

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

Perform a comprehensive analysis of the codebase to identify and eliminate all instances of duplicated code, following the DRY (Don't Repeat Yourself) and KISS (Keep It Simple, Stupid) principles. Conduct a systematic search across all modules, classes, and files to find similar code patterns, duplicate logic, redundant implementations, and opportunities for abstraction. Specifically examine rendering-related code such as diff previews and command output previews to determine if they can share unified rendering logic, styling, and common components. Audit all utility functions scattered throughout different modules and extract them into a centralized shared utility module with proper organization and documentation. Create a detailed report identifying each duplication found, the proposed refactoring strategy, and the expected benefits in terms of maintainability, reduced code complexity, and improved consistency. Ensure all refactored code maintains existing functionality while simplifying the overall architecture. Prioritize changes that provide the greatest reduction in duplication with minimal risk to existing functionality.

---

review any duplicated code in the codebase and refactor to remove duplication. For example, the logic for rendering the diff preview and the command output preview can be unified to use the same rendering logic and styling. This will make the codebase cleaner and easier to maintain. Additionally, any common utility functions that are duplicated across different modules can be extracted into a shared utility module. search across modules for similar code patterns and identify opportunities for refactoring to reduce duplication and improve code reuse.

DRY and KISS

---

Conduct a comprehensive review and enhancement of error handling and recovery mechanisms within the agent loop, with particular emphasis on tool call operations. Implement a multi-layered error handling strategy that includes retry logic with exponential backoff for transient failures such as network timeouts, rate limiting, and temporary service unavailability while implementing fail-fast behavior for non-recoverable errors including authentication failures, invalid parameters, and permission denied scenarios. Develop and integrate a robust state management system that ensures the agent can maintain consistent internal state during and after error occurrences, including proper rollback mechanisms for partial operations and transaction-like semantics where appropriate. Create a comprehensive error categorization system that distinguishes between retryable and non-retryable errors and implements appropriate handling strategies for each category. Enhance user-facing error messages to be clear, actionable, and informative while avoiding technical jargon that may confuse end users. Implement proper logging at multiple levels including debug, info, warning, and error levels to facilitate troubleshooting and monitoring. Conduct a thorough audit of existing error handling implementations to identify gaps, inconsistencies, and potential failure points. Refactor the error handling code to improve modularity, testability, and maintainability while ensuring comprehensive test coverage for error scenarios including edge cases and unexpected inputs. Add appropriate circuit breaker patterns for external service calls to prevent cascading failures and enable graceful degradation when dependent services are unavailable. Implement proper resource cleanup and resource leak prevention throughout the agent loop.

---

CODEX plus

main account
kiweuro
writedownapp
humidapp
vtchat.io

---

merge

vtcode-acp
vtcode-acp-client

module

---

---

try copilot github oauth to use anthropic's claude

https://claude.ai/chat/37d94c2c-a521-44d2-85f3-e15c38864286

===

https://deepwiki.com/search/how-does-codex-read-agentsmd_b2f6e918-cb8a-4f83-99dd-38c7c11e77fd?mode=fast

For example, here’s how pi-mono loads AGENTS.md:

// Append project context files
if (contextFiles.length > 0) {
prompt += "\n\n# Project Context\n\n";
prompt += "Project-specific instructions and guidelines:\n\n";
for (const { path: filePath, content } of contextFiles) {
prompt += `## ${filePath}\n\n${content}\n\n`;
}
}

Claude Code does the same with CLAUDE.md. The pattern is simple: read the file, append it right after the system prompt.

==

Improve /model loading performance. models.json are gettings large and loading them is becoming a bottleneck. Consider lazy loading model metadata, caching results, or optimizing the data structure for faster access. Additionally, review the model selection logic to ensure it is efficient and does not require unnecessary processing of the entire models.json file on each load.

==

handle double click text to highlight text in tui and copy to clipboard.

---

idea: file change detection for better context management. Implement a file watcher that detects changes in project files and automatically updates the agent's context with the latest content. This will ensure that the agent always has access to the most up-to-date information without requiring manual refreshes. The file watcher can be implemented using libraries such as chokidar for Node.js or fs.watch for Python, and can be configured to monitor specific directories or file types relevant to the project.

Show a human in the loop UI asking user for next step.

```
* Asking user I found an unrelated modified file in the worktree: "docs...

VT Code is requesting information

I found an unrelated modified file in the worktree: "docs/project/TODO.md". I didn't change it as part of this
task. How would you like me to proceed?

Choose an action:
› {leave_as_is} text
  {inspect_changes} text
  {stop_here} text

Other (type your answer)

th select • Enter accept • ctrl+d decline • Esc cancel
```

/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/file_change_detection.png

===

bug:

1. currently when selecting the text in the TUI, it being automatically coppy the highlight text to clip board -> remove it. Don't want to have it automatically copy to clipboard when selecting text in the TUI, as it can be disruptive and may not always be desired. Instead, implement a more intentional action for copying to clipboard, such as a specific keyboard shortcut (e.g., Ctrl+C) or a context menu option that appears when right-clicking on the selected text. This way, users can choose when they want to copy the highlighted text without it happening automatically every time they select something in the TUI.
2. When selecting the user's chat box' input text, and then press Ctrl+C to copy, current the highlight text is not copied to clipboard. Fix the issue so that when the user selects text in the chat input box and presses Ctrl+C, the selected text is properly copied to the clipboard as expected. This will allow users to easily copy and share their input without any issues.
