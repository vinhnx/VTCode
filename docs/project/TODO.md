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

try copilot github oauth in vtcode system. reference OpenAI and OpenRouter OAUTH flows and vtcode-auth module implementation.

deepwiki reference (use mcp and web search to explore if possible):

- opencode: https://deepwiki.com/search/how-does-github-copilot-subscr_f92939bf-7608-4e34-b694-f318941b019c?mode=fast
- pi-mono: https://deepwiki.com/search/how-does-github-copilot-subscr_1387ab95-3e25-408b-a6e9-880712afe35d?mode=fast

NOTE:

```
Clear answer: **it depends entirely on which integration path you use.** Here's the breakdown:

---

**TOS violation: using `copilot_internal` API directly**

Using `api.github.com/copilot_internal` outside of officially supported clients violates GitHub's ToS and can result in account suspension — third-party implementations that reverse-engineer or proxy Copilot's internal API are not officially supported.

---

**Sanctioned path: `@github/copilot-language-server` SDK**

GitHub released the Copilot Language Server SDK publicly in February 2025, explicitly enabling any editor or IDE to integrate Copilot via the Language Server Protocol standard. This is GitHub's official blessed route — the same one used by VS Code, JetBrains, Neovim, and Xcode.

The Copilot Language Server also supports the Agent Client Protocol (ACP), enabling integration with ACP-compatible editors. Communication happens over stdin/stdout.

---

**Integration architecture for VT Code**

Since VT Code is a terminal agent (Rust), the wiring is:

```

VT Code (LSP client, Rust) <--JSON-RPC 2.0 over stdio--> @github/copilot-language-server (Node.js subprocess)

````

Bootstrap flow:
```json
// 1. Send initialize
{ "editorInfo": { "name": "VT Code", "version": "x.y.z" }, "editorPluginInfo": { "name": "vtcode-copilot", "version": "1.0.0" } }

// 2. Auth via device flow — server returns userCode, user visits URL once
{ "userCode": "ABCD-EFGH", "command": { "command": "github.copilot.finishDeviceFlow" } }

// 3. Inline completions
{ "method": "textDocument/inlineCompletion", "params": { "textDocument": { "uri": "file:///...", "version": 0 }, "position": {"line": N, "character": M} } }
````

---

**Key ToS constraints to stay compliant**

- Each user authenticates with their own Copilot subscription via the device flow — VT Code does not proxy or pool credentials
- You must pass telemetry back (accept/reject events) via `github.copilot.didAcceptCompletionItem` — this is required by the protocol, not optional
- GitHub Copilot Extensions are being deprecated on November 10, 2025, in favor of MCP — so don't build on the Extensions platform, LSP is the right surface

---

**Bottom line**: using `@github/copilot-language-server` is fully compliant and architecturally sound for VT Code. It's a Node.js subprocess that VT Code spawns and speaks LSP JSON-RPC to over stdio — clean separation, no ToS risk.

Package: `npm:@github/copilot-language-server` — native binaries for `darwin-arm64` (M4), `linux-x64`, etc. are bundled, so no Node runtime dependency on the user side if you ship the binary.

```

===


https://gist.github.com/cedws/3a24b2c7569bb610e24aa90dd217d9f2

===
```

===

Fix OAUTH info display broken in TUI

'/Users/vinhnguyenxuan/Desktop/Screenshot 2026-03-20 at 4.13.12 PM.png'

===

check Pi implementation again, it seems Pi is using GitHUb Copilot plugin, instead of GitHub Copilot CLI (https://deepwiki.com/search/how-does-github-copilot-subscr_1387ab95-3e25-408b-a6e9-880712afe35d?mode=fast). While Open Code use a Github app (should we register a VT Code Github app for better integration and compliance?).

Pi: '/Users/vinhnguyenxuan/Desktop/Screenshot 2026-03-20 at 4.18.16 PM.png'
Ours: '/Users/vinhnguyenxuan/Desktop/Screenshot 2026-03-20 at 4.13.50 PM.png'
OpenCode: ''/Users/vinhnguyenxuan/Desktop/Screenshot 2026-03-20 at 4.21.49 PM.png'

===

Also add full list of supports GitHUb copilot models.
