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

--

keep apply plan https://deepwiki.com/badlogic/pi-mono/3-pi-agent-core:-agent-framework

==

PROMPT CACHING:

Claude Code's entire harness is built around prompt caching. They declare SEVs when cache hit rate drops

It's a prefix match. Order matters enormously

Their prompt layout:

> Static system prompt + tools (globally cached)
> CLAUDE.md (cached per project)
> Session context (cached per session)
> Conversation messages

What kills the cache:

> Timestamps in static prompts
> Shuffling tool order
> Adding/removing tools mid-session
> Switching models mid-conversation

Instead of editing the system prompt, they inject <system-reminder> in the next user message which preserves the cache completely

Plan Mode: the obvious approach is swapping to read-only tools

That breaks the cache, instead they keep ALL tools loaded and use EnterPlanMode/ExitPlanMode as tools themselves

Bonus: the model can enter plan mode on its own when it detects a hard problem

For unused MCP tools: they don't remove them, they send lightweight stubs with defer_loading: true

Full schemas load only when the model discovers them via ToolSearch

Compaction (context overflow): they fork with the exact same prefix so the cache is reused

Only new tokens are the compaction prompt itself

Switching from Opus to Haiku mid-session? Actually MORE expensive because you rebuild the entire cache Use subagents with handoff messages instead

TOOL DESIGN:

The AskUserQuestion tool took 3 attempts:

> Adding questions to ExitPlanTool confused the model > Modified markdown output was inconsistent
> Dedicated tool with structured output worked.

Claude actually liked calling it

Even the best designed tool fails if the model doesn't understand how to call it

Todos got replaced by Tasks as models improved

Early Claude Code needed reminders every 5 turns Smarter models found this limiting and stuck rigidly to the list

Tasks support dependencies, cross-subagent updates, and can be altered/deleted

The takeaway: tools your model once needed might now be constraining it

Search went from RAG → Grep → Skills with progressive disclosure

Over a year Claude went from needing context handed to it to doing nested search across multiple layers on its own

Claude Code has ~20 tools. The bar to add a new one is high Every tool is one more option the model has to think about

SKILLS:

Skills are not markdown files. They're folders with scripts, assets, data, config options, and dynamic hooks

9 categories they've identified:

> Library & API Reference
> Product Verification
> Data & Analysis
> Business Automation
> Code Scaffolding
> Code Quality & Review
> CI/CD & Deployment
> Runbooks
> Infrastructure Ops

What makes a skill great:

> Don't state the obvious. Focus on what pushes Claude out of default patterns
> Gotchas section is the highest-signal content. Built from real failure points over time
> Use the file system for progressive disclosure. Reference docs, templates, scripts
> Don't railroad with rigid steps. Give information and flexibility
> Description field is for the model. It's what Claude scans to decide if a skill matches
> Skills can store memory: logs, JSON, SQLite. Use ${CLAUDE_PLUGIN_DATA} for persistence
> Give Claude scripts so it composes instead of reconstructing boilerplate
> On-demand hooks: /careful blocks destructive commands, /freeze locks edits to a directory

Distribute via repos for small teams, plugin marketplace at scale Let skills prove themselves organically before promoting

FILE SYSTEM + BASH:

Every agent benefits from a file system It represents state the agent reads into context and uses to verify its own work

You don't need to remember everything You need to know how to find it

His advice after dozens of calls with companies building general agents: "Use the bash tool more"

Instead of fetching 100 emails via tool calls and hoping the model figures it out Save to files. Search. Ground in code. Take multiple passes. Verify.

PLAYGROUNDs:

A plugin that generates standalone HTML files for visual, interactive problem-solving Architecture visualisation, design tweaking, game balancing, inline writing critique

The tip: think of a unique way of interacting with the model and ask it to express that

==

1/ check auto-mode keep asking for permission for tools use (HITL) ->
2/ also shift+tab to trust auto mode keep switch back to default edit mode -> hence repeated prompt for tools
3/ implement inline list /mode for full 3 modes and interactive selection 4. check .vtcode/tool-policy.json and improve
5/ maybe regexp and shell command prefix and improve policy allow list for tools usage instead of binary allow/deny.
6/ reference codex: https://deepwiki.com/search/how-does-the-tool-policy-modes_981fc82b-7738-4a30-82cf-0e952627bd9c?mode=fast
