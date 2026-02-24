# Changelog

All notable changes to vtcode will be documented in this file.
## v0.73.2 - 2026-01-29
## [unreleased]


### Bug Fixes
- Update versioning format to remove 'v' prefix in tags and URLs (@vinhnx)

- Resolve critical scrolling issue and remove unused slash command handlers (@vinhnx)

- Correct exec_code policy and update TODO for markdown rendering issue (@vinhnx)

- Update default model in configuration to glm-5:cloud (@vinhnx)

- Correct changelog generation to use the previous release tag instead of a fixed version. (@vinhnx)

- Update chat input placeholders for clarity and improved user guidance (Vinh Nguyen)

- Update chat input placeholders for clarity and improved user guidance (Vinh Nguyen)

- Disable scroll indicator in status bar (Vinh Nguyen)



### Documentation
- Update changelog for 0.79.2 [skip ci] (vtcode-release-bot)

- Update changelog for 0.79.3 [skip ci] (vtcode-release-bot)

- Update changelog for 0.79.4 [skip ci] (vtcode-release-bot)

- Update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (@vinhnx)

- Update changelog for 0.80.0 [skip ci] (vtcode-release-bot)

- Update documentation for TECH_DEBT_TRACKER and QUALITY_SCORE; add tests for subagent loading and file operations (@vinhnx)

- Update changelog for 0.80.1 [skip ci] (vtcode-release-bot)

- Update changelog for 0.81.0 [skip ci] (vtcode-release-bot)

- Update changelog for 0.81.1 [skip ci] (vtcode-release-bot)

- Update changelog for 0.81.2 [skip ci] (vtcode-release-bot)

- Update changelog for 0.81.3 [skip ci] (vtcode-release-bot)

- Add a guide for adding new models to AGENTS.md. (@vinhnx)

- Update changelog for 0.82.0 [skip ci] (vtcode-release-bot)

- Update changelog for 0.82.1 [skip ci] (vtcode-release-bot)

- Update TODO.md with additional PTY truncate display information and test references (@vinhnx)

- Update changelog for 0.82.2 [skip ci] (vtcode-release-bot)

- Update changelog for 0.83.0 [skip ci] (vtcode-release-bot)

- Update changelog for 0.82.3 [skip ci] (vtcode-release-bot)

- Update TODO.md with new tasks and references (Vinh Nguyen)

- Update TODO.md with examples and improve TUI display for truncated outputs (Vinh Nguyen)



### Features
- Add MiniMax M2.5 model support across various providers and update related constants (@vinhnx)

- Add Qwen3 Coder Next model support and update related constants (@vinhnx)

- Add skill bundle import/export functionality with zip support (@vinhnx)

- Implement plan mode toggle and strip proposed plan blocks in rendering (@vinhnx)

- Implement in-process teammate runner and enhance team protocol messaging (@vinhnx)

- Add /share-log command to export session log as JSON for debugging (@vinhnx)

- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (@vinhnx)

- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (@vinhnx)

- Use configurable constants for agent session limits and expose the default max context tokens function. (@vinhnx)

- Introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (@vinhnx)

- Add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (@vinhnx)

- Add sanitizer module for secret redaction and integrate into output handling (@vinhnx)

- Implement credential storage using OS keyring and file fallback (@vinhnx)

- Add timeout handling for turn metadata collection (@vinhnx)

- Implement mouse scroll handling for improved navigation (@vinhnx)

- Add Qwen3.5-397B-A17B model with hybrid architecture and update configuration (@vinhnx)

- Implement secure storage for custom API keys using OS keyring (@vinhnx)

- Add CI workflows for building Linux and Windows binaries; optimize release process (@vinhnx)

- Add full CI mode to release script for all platforms (@vinhnx)

- Refactor build process to use conditional cross compilation for Linux and Windows (@vinhnx)

- Implement mouse scroll support for TUI session and history picker, and update default agent configuration to Ollama. (@vinhnx)

- Render GFM tables inside markdown code blocks as tables and prevent word-wrapping for table lines in the TUI. (@vinhnx)

- Implement mouse text selection in the TUI and add a new `vtcode.toml` configuration file. (@vinhnx)

- Add Claude Sonnet 4.6 model support and integrate it across model definitions, parsing, catalog, and documentation. (@vinhnx)

- Implement Gemini 3.1 Pro Preview models with updated token limits and system prompt handling. (@vinhnx)

- Implement Gemini prompt caching with TTL using a new `CacheControl` part and add support for Gemini 3.1 Pro preview models. (@vinhnx)

- Add `prompt_cache_key` to OpenAI requests for improved cache locality and simplify Responses API usage logic. (@vinhnx)

- Add top-level cache control to Anthropic requests, with TTL determined by breakpoint consumption. (@vinhnx)

- Standardize MiniMax-M2.5 model identifier, promote it as the default, and update configuration defaults. (@vinhnx)

- Introduce CI cost optimization strategies, add a new `--ci-only` release mode, and document release workflow details. (@vinhnx)

- Add prompt cache key to LLM requests and enhance unified_file tool execution diagnostics. (@vinhnx)

- Refactor Ollama non-streaming request handling and add a fallback to non-streaming for initial stream failures. (@vinhnx)

- Improve spooled tool output handling by verifying file existence and add a mechanism to suppress agent follow-up prompt detection for auto-generated prompts. (@vinhnx)

- Enhance error handling and recovery mechanisms across various components (@vinhnx)

- Implement tool reentrancy guard to prevent recursive execution and improve panic reporting with `better-panic`. (@vinhnx)

- Implement chunked reading for spooled tool outputs with improved agent messaging and update default LLM provider configuration. (@vinhnx)

- Add chunked file read spool progress tracking and refine token usage calculation for context management. (@vinhnx)

- Generate consolidated checksums.txt for releases and centralize script utilities into common.sh. (@vinhnx)

- Implement TaskTracker tool and enhance agent guards and documentation based on NL2Repo-Bench insights. (@vinhnx)

- Integrate AI agent best practices into system prompts and loop detection for improved planning, root cause analysis, and uncertainty recognition. (@vinhnx)

- Enhance documentation on grounding, uncertainty, and regression verification; improve loop detection guidance (@vinhnx)

- Enhance `AskUserChoice` with freeform input, custom labels, placeholders, and default selections. (@vinhnx)

- Implement freeform text input for wizard modals, guided by system prompt and toggled by the Tab key. (@vinhnx)

- Refine plan mode transitions by adding more aliases, enabling contextual exit confirmations, and providing user guidance. (Vinh Nguyen)

- Set custom terminal title for VT Code TUI (Vinh Nguyen)

- Migrate changelog generation to git-cliff and update related documentation (Vinh Nguyen)



### Other
- Prevent footer panic when hint is absent, refactor path argument to `&Path`, and optimize sidebar string truncation. (@vinhnx)



### Refactors
- Remove unused ReasoningSegment import from turn_processing.rs (@vinhnx)

- Reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (@vinhnx)

- Extract large event handler modules into smaller files for improved navigation (@vinhnx)

- Streamline file operations and enhance workspace path handling (@vinhnx)

- Replace manual file operations with shared utility functions for consistency (@vinhnx)

- Replace manual file operations with shared utility functions for consistency (@vinhnx)

- Replace manual file operations with shared utility functions for consistency (@vinhnx)

- Consolidate duplicated logic across workspace crates into shared utility functions (@vinhnx)

- Streamline MCP tool management and indexing in ToolRegistry (@vinhnx)

- Remove Rust cache step from release workflow (@vinhnx)

- Clean up code formatting and improve readability across multiple files (@vinhnx)

- Remove unused imports and enhance configuration for credential storage (@vinhnx)

- Make TUI signal cleanup and dotfile permission backup UNIX-specific. (@vinhnx)

- Improve previous SemVer tag identification by searching commit history in release scripts (@vinhnx)

- Remove unified tool resolver module to streamline codebase (Vinh Nguyen)

- Remove unused TUI components and improve event handling for focus changes (Vinh Nguyen)

- Standardize continuation prefix handling in wrap_block_lines functions (Vinh Nguyen)
## 0.82.3 - 2026-02-24

### Features

- Implement freeform text input for wizard modals, guided by system prompt and toggled by the Tab key. (9b54cdd6) (@vinhnx)
- Enhance `AskUserChoice` with freeform input, custom labels, placeholders, and default selections. (53e0e111) (@vinhnx)
- Enhance documentation on grounding, uncertainty, and regression verification; improve loop detection guidance (064ea630) (@vinhnx)
- Integrate AI agent best practices into system prompts and loop detection for improved planning, root cause analysis, and uncertainty recognition. (91e5e295) (@vinhnx)
- Implement TaskTracker tool and enhance agent guards and documentation based on NL2Repo-Bench insights. (45a9a159) (@vinhnx)

### Refactors

- improve previous SemVer tag identification by searching commit history in release scripts (0fcdce3d) (@vinhnx)

### Other

- Update commit (925f355d) (@vinhnx)
- Add reference to git-cliff for changelog generation (8dd703f1) (@vinhnx)
- Refactor user input option generation and enhance markdown rendering in modals (a72a63c0) (@vinhnx)
- Add navigation loop guidance and improve plan mode handling (5d162ae6) (@vinhnx)
- Add plan-mode task tracker with CRUD functionality and integrate with existing tools (56c7e3b8) (@vinhnx)
- Rename UpdatePlanCommand to TaskTrackerCommand and refactor related files and documentation (b4520273) (@vinhnx)
- Update commit (0642ff3b) (@vinhnx)

## 0.83.0 - 2026-02-24

### Features

- Implement freeform text input for wizard modals, guided by system prompt and toggled by the Tab key. (9b54cdd6) (@vinhnx)
- Enhance `AskUserChoice` with freeform input, custom labels, placeholders, and default selections. (53e0e111) (@vinhnx)
- Enhance documentation on grounding, uncertainty, and regression verification; improve loop detection guidance (064ea630) (@vinhnx)
- Integrate AI agent best practices into system prompts and loop detection for improved planning, root cause analysis, and uncertainty recognition. (91e5e295) (@vinhnx)
- Implement TaskTracker tool and enhance agent guards and documentation based on NL2Repo-Bench insights. (45a9a159) (@vinhnx)

### Refactors

- improve previous SemVer tag identification by searching commit history in release scripts (0fcdce3d) (@vinhnx)

### Other

- Refactor user input option generation and enhance markdown rendering in modals (a72a63c0) (@vinhnx)
- Add navigation loop guidance and improve plan mode handling (5d162ae6) (@vinhnx)
- Add plan-mode task tracker with CRUD functionality and integrate with existing tools (56c7e3b8) (@vinhnx)
- Rename UpdatePlanCommand to TaskTrackerCommand and refactor related files and documentation (b4520273) (@vinhnx)
- Update commit (0642ff3b) (@vinhnx)

## 0.82.2 - 2026-02-23

### Features

- Add chunked file read spool progress tracking and refine token usage calculation for context management. (ef8f162d) (@vinhnx)
- Implement chunked reading for spooled tool outputs with improved agent messaging and update default LLM provider configuration. (b5b0c230) (@vinhnx)
- Implement tool reentrancy guard to prevent recursive execution and improve panic reporting with `better-panic`. (44351bf5) (@vinhnx)
- enhance error handling and recovery mechanisms across various components (7902206c) (@vinhnx)
- improve spooled tool output handling by verifying file existence and add a mechanism to suppress agent follow-up prompt detection for auto-generated prompts. (083ae71a) (@vinhnx)
- refactor Ollama non-streaming request handling and add a fallback to non-streaming for initial stream failures. (30683331) (@vinhnx)
- Add prompt cache key to LLM requests and enhance unified_file tool execution diagnostics. (4073aed6) (@vinhnx)
- introduce CI cost optimization strategies, add a new `--ci-only` release mode, and document release workflow details. (dd2f3168) (@vinhnx)
- standardize MiniMax-M2.5 model identifier, promote it as the default, and update configuration defaults. (ff6dcef6) (@vinhnx)
- Add top-level cache control to Anthropic requests, with TTL determined by breakpoint consumption. (91c0c9e4) (@vinhnx)
- Add `prompt_cache_key` to OpenAI requests for improved cache locality and simplify Responses API usage logic. (45c9002e) (@vinhnx)
- Implement Gemini prompt caching with TTL using a new `CacheControl` part and add support for Gemini 3.1 Pro preview models. (8b5b42a1) (@vinhnx)
- Implement Gemini 3.1 Pro Preview models with updated token limits and system prompt handling. (dc0742c0) (@vinhnx)
- add Claude Sonnet 4.6 model support and integrate it across model definitions, parsing, catalog, and documentation. (d460c56d) (@vinhnx)
- Implement mouse text selection in the TUI and add a new `vtcode.toml` configuration file. (83567152) (@vinhnx)
- Render GFM tables inside markdown code blocks as tables and prevent word-wrapping for table lines in the TUI. (c90f06e3) (@vinhnx)
- Implement mouse scroll support for TUI session and history picker, and update default agent configuration to Ollama. (db99f4db) (@vinhnx)
- refactor build process to use conditional cross compilation for Linux and Windows (d15bb558) (@vinhnx)
- add full CI mode to release script for all platforms (326a2c8c) (@vinhnx)
- add CI workflows for building Linux and Windows binaries; optimize release process (090bebb4) (@vinhnx)
- implement secure storage for custom API keys using OS keyring (3da5a60a) (@vinhnx)
- add Qwen3.5-397B-A17B model with hybrid architecture and update configuration (26a9a7ee) (@vinhnx)
- implement mouse scroll handling for improved navigation (24a2d640) (@vinhnx)
- add timeout handling for turn metadata collection (1b1f91d4) (@vinhnx)
- implement credential storage using OS keyring and file fallback (1e94c71a) (@vinhnx)
- add sanitizer module for secret redaction and integrate into output handling (4263808d) (@vinhnx)
- add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (fddc4887) (@vinhnx)
- introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (52b13dd1) (@vinhnx)
- Use configurable constants for agent session limits and expose the default max context tokens function. (21d1183f) (@vinhnx)
- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (68d07c91) (@vinhnx)
- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (ac806e0e) (@vinhnx)
- add /share-log command to export session log as JSON for debugging (64305820) (@vinhnx)
- implement in-process teammate runner and enhance team protocol messaging (4aff9318) (@vinhnx)
- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Bug Fixes

- correct changelog generation to use the previous release tag instead of a fixed version. (b0437d27) (@vinhnx)
- Update default model in configuration to glm-5:cloud (1700a7e4) (@vinhnx)
- correct exec_code policy and update TODO for markdown rendering issue (9b87f88b) (@vinhnx)
- resolve critical scrolling issue and remove unused slash command handlers (bcb81434) (@vinhnx)

### Refactors

- Make TUI signal cleanup and dotfile permission backup UNIX-specific. (14a4f2d2) (@vinhnx)
- remove unused imports and enhance configuration for credential storage (b79f2bd2) (@vinhnx)
- clean up code formatting and improve readability across multiple files (b1ae3ad9) (@vinhnx)
- remove Rust cache step from release workflow (e36c5f55) (@vinhnx)
- streamline MCP tool management and indexing in ToolRegistry (a5c3677b) (@vinhnx)
- consolidate duplicated logic across workspace crates into shared utility functions (a9df41fd) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (15c45f9a) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (7fcbe5f2) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (54c447af) (@vinhnx)
- streamline file operations and enhance workspace path handling (f7ebb78d) (@vinhnx)
- extract large event handler modules into smaller files for improved navigation (9eda39e4) (@vinhnx)
- reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (95bcd08f) (@vinhnx)
- remove unused ReasoningSegment import from turn_processing.rs (9f4672d7) (@vinhnx)

### Documentation

- Update TODO.md with additional PTY truncate display information and test references (cd3a3850) (@vinhnx)
- Add a guide for adding new models to AGENTS.md. (8648b584) (@vinhnx)
- update documentation for TECH_DEBT_TRACKER and QUALITY_SCORE; add tests for subagent loading and file operations (fb7c0944) (@vinhnx)
- update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (b4a1ef7a) (@vinhnx)
- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- Release (cf5d5f7e) (@vinhnx)
- Release (28a8476b) (@vinhnx)
- Release (77422bee) (@vinhnx)
- Release (14027810) (@vinhnx)
- Release (6c982a35) (@vinhnx)
- clean up configuration file by removing unused custom API keys and simplifying array formatting (ad594e41) (@vinhnx)
- Release (4bd94bdb) (@vinhnx)
- Release (800c7069) (@vinhnx)
- Release (a08f765d) (@vinhnx)
- Release (accdcc25) (@vinhnx)
- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Increase max tool calls per turn to 48 and implement budget warning system in harness (7b6cade0) (@vinhnx)
- Add spool chunk read tracking and enforce limits per turn (c09ef6d3) (@vinhnx)
- Fix: Prevent footer panic when hint is absent, refactor path argument to `&Path`, and optimize sidebar string truncation. (59525d4b) (@vinhnx)
- Update TODO.md (3391d6b9) (@1097578+vinhnx)
- Update commit (9d05e9de) (@vinhnx)
- Enhance model behavior configuration for LLM providers (0fa12334) (@vinhnx)
- Add scripts for documentation link validation and markdown location checks (7290fc5b) (@vinhnx)
- Update default model in vtcode.toml to minimax-m2:cloud; format code for consistency (e024a48d) (@vinhnx)
- Add additional_agent_dirs configuration option to vtcode.toml (0f6d1747) (@vinhnx)
- Remove custom prompts feature and related code; update error handling for agent actions; refine UI shortcuts and command handling (cfcc9765) (@vinhnx)
- Remove custom prompts feature and related code (dfc3ec5d) (@vinhnx)
- Update LLM provider configuration to use Ollama and remove unused typos.toml file (ca83bdcd) (@vinhnx)
- Enhance agent runner settings and improve loop detection; update CLI commands for reasoning effort and verbosity; refine patch handling in file operations; adjust markdown diff summary; modify configuration for LLM provider. (ff221d1d) (@vinhnx)
- Refactor agent runner and tool registry for improved error handling and normalization; enhance loop detection and add tests for new functionality. (101a2b07) (@vinhnx)
- Refactor LLM request handling, improve reasoning processing, and enhance tool command parsing (896f6c69) (@vinhnx)
- Implement streaming response rendering with reasoning support and helper functions (b3ba347b) (@vinhnx)
- Refactor file operations to use utility functions for directory creation and file reading/writing (7f800b3d) (@vinhnx)
- Implement unified session loop for agent execution and remove plugin marketplace commands (17d9597f) (@vinhnx)
- Add tests for model picker and prompt refinement functionality (0305390e) (@vinhnx)
- Enhance configuration and logging for agent and hooks; add mock MCP server for integration tests (59a48db6) (@vinhnx)
- Refactor agent runner to use AgentSessionState for session management and update related components (083fcac6) (@vinhnx)
- Refactor apply_patch handler for improved output handling; streamline sandbox policy mapping (67c0e05b) (@vinhnx)
- Refactor caching logic for improved performance; enhance error context handling; update tests for accurate cache statistics (5a10f0d7) (@vinhnx)
- Refactor exec_policy and command validation; consolidate HTTP client utilities; enhance error handling; improve file operations; update middleware to async; clean up validation cache; and adjust rate limiting implementation. (e6245af6) (@vinhnx)
- Adjust provider configurations and logging. (a649d023) (@vinhnx)
- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.82.1 - 2026-02-20

### Features

- introduce CI cost optimization strategies, add a new `--ci-only` release mode, and document release workflow details. (dd2f3168) (@vinhnx)
- standardize MiniMax-M2.5 model identifier, promote it as the default, and update configuration defaults. (ff6dcef6) (@vinhnx)
- Add top-level cache control to Anthropic requests, with TTL determined by breakpoint consumption. (91c0c9e4) (@vinhnx)
- Add `prompt_cache_key` to OpenAI requests for improved cache locality and simplify Responses API usage logic. (45c9002e) (@vinhnx)
- Implement Gemini prompt caching with TTL using a new `CacheControl` part and add support for Gemini 3.1 Pro preview models. (8b5b42a1) (@vinhnx)
- Implement Gemini 3.1 Pro Preview models with updated token limits and system prompt handling. (dc0742c0) (@vinhnx)
- add Claude Sonnet 4.6 model support and integrate it across model definitions, parsing, catalog, and documentation. (d460c56d) (@vinhnx)
- Implement mouse text selection in the TUI and add a new `vtcode.toml` configuration file. (83567152) (@vinhnx)
- Render GFM tables inside markdown code blocks as tables and prevent word-wrapping for table lines in the TUI. (c90f06e3) (@vinhnx)
- Implement mouse scroll support for TUI session and history picker, and update default agent configuration to Ollama. (db99f4db) (@vinhnx)
- refactor build process to use conditional cross compilation for Linux and Windows (d15bb558) (@vinhnx)
- add full CI mode to release script for all platforms (326a2c8c) (@vinhnx)
- add CI workflows for building Linux and Windows binaries; optimize release process (090bebb4) (@vinhnx)
- implement secure storage for custom API keys using OS keyring (3da5a60a) (@vinhnx)
- add Qwen3.5-397B-A17B model with hybrid architecture and update configuration (26a9a7ee) (@vinhnx)
- implement mouse scroll handling for improved navigation (24a2d640) (@vinhnx)
- add timeout handling for turn metadata collection (1b1f91d4) (@vinhnx)
- implement credential storage using OS keyring and file fallback (1e94c71a) (@vinhnx)
- add sanitizer module for secret redaction and integrate into output handling (4263808d) (@vinhnx)
- add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (fddc4887) (@vinhnx)
- introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (52b13dd1) (@vinhnx)
- Use configurable constants for agent session limits and expose the default max context tokens function. (21d1183f) (@vinhnx)
- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (68d07c91) (@vinhnx)
- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (ac806e0e) (@vinhnx)
- add /share-log command to export session log as JSON for debugging (64305820) (@vinhnx)
- implement in-process teammate runner and enhance team protocol messaging (4aff9318) (@vinhnx)
- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Bug Fixes

- correct exec_code policy and update TODO for markdown rendering issue (9b87f88b) (@vinhnx)
- resolve critical scrolling issue and remove unused slash command handlers (bcb81434) (@vinhnx)

### Refactors

- Make TUI signal cleanup and dotfile permission backup UNIX-specific. (14a4f2d2) (@vinhnx)
- remove unused imports and enhance configuration for credential storage (b79f2bd2) (@vinhnx)
- clean up code formatting and improve readability across multiple files (b1ae3ad9) (@vinhnx)
- remove Rust cache step from release workflow (e36c5f55) (@vinhnx)
- streamline MCP tool management and indexing in ToolRegistry (a5c3677b) (@vinhnx)
- consolidate duplicated logic across workspace crates into shared utility functions (a9df41fd) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (15c45f9a) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (7fcbe5f2) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (54c447af) (@vinhnx)
- streamline file operations and enhance workspace path handling (f7ebb78d) (@vinhnx)
- extract large event handler modules into smaller files for improved navigation (9eda39e4) (@vinhnx)
- reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (95bcd08f) (@vinhnx)
- remove unused ReasoningSegment import from turn_processing.rs (9f4672d7) (@vinhnx)

### Documentation

- Add a guide for adding new models to AGENTS.md. (8648b584) (@vinhnx)
- update documentation for TECH_DEBT_TRACKER and QUALITY_SCORE; add tests for subagent loading and file operations (fb7c0944) (@vinhnx)
- update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (b4a1ef7a) (@vinhnx)
- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- Release (28a8476b) (@vinhnx)
- Release (77422bee) (@vinhnx)
- Release (14027810) (@vinhnx)
- Release (6c982a35) (@vinhnx)
- clean up configuration file by removing unused custom API keys and simplifying array formatting (ad594e41) (@vinhnx)
- Release (4bd94bdb) (@vinhnx)
- Release (800c7069) (@vinhnx)
- Release (a08f765d) (@vinhnx)
- Release (accdcc25) (@vinhnx)
- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Update commit (9d05e9de) (@vinhnx)
- Enhance model behavior configuration for LLM providers (0fa12334) (@vinhnx)
- Add scripts for documentation link validation and markdown location checks (7290fc5b) (@vinhnx)
- Update default model in vtcode.toml to minimax-m2:cloud; format code for consistency (e024a48d) (@vinhnx)
- Add additional_agent_dirs configuration option to vtcode.toml (0f6d1747) (@vinhnx)
- Remove custom prompts feature and related code; update error handling for agent actions; refine UI shortcuts and command handling (cfcc9765) (@vinhnx)
- Remove custom prompts feature and related code (dfc3ec5d) (@vinhnx)
- Update LLM provider configuration to use Ollama and remove unused typos.toml file (ca83bdcd) (@vinhnx)
- Enhance agent runner settings and improve loop detection; update CLI commands for reasoning effort and verbosity; refine patch handling in file operations; adjust markdown diff summary; modify configuration for LLM provider. (ff221d1d) (@vinhnx)
- Refactor agent runner and tool registry for improved error handling and normalization; enhance loop detection and add tests for new functionality. (101a2b07) (@vinhnx)
- Refactor LLM request handling, improve reasoning processing, and enhance tool command parsing (896f6c69) (@vinhnx)
- Implement streaming response rendering with reasoning support and helper functions (b3ba347b) (@vinhnx)
- Refactor file operations to use utility functions for directory creation and file reading/writing (7f800b3d) (@vinhnx)
- Implement unified session loop for agent execution and remove plugin marketplace commands (17d9597f) (@vinhnx)
- Add tests for model picker and prompt refinement functionality (0305390e) (@vinhnx)
- Enhance configuration and logging for agent and hooks; add mock MCP server for integration tests (59a48db6) (@vinhnx)
- Refactor agent runner to use AgentSessionState for session management and update related components (083fcac6) (@vinhnx)
- Refactor apply_patch handler for improved output handling; streamline sandbox policy mapping (67c0e05b) (@vinhnx)
- Refactor caching logic for improved performance; enhance error context handling; update tests for accurate cache statistics (5a10f0d7) (@vinhnx)
- Refactor exec_policy and command validation; consolidate HTTP client utilities; enhance error handling; improve file operations; update middleware to async; clean up validation cache; and adjust rate limiting implementation. (e6245af6) (@vinhnx)
- Adjust provider configurations and logging. (a649d023) (@vinhnx)
- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.82.0 - 2026-02-20

### Features

- Implement Gemini prompt caching with TTL using a new `CacheControl` part and add support for Gemini 3.1 Pro preview models. (8b5b42a1) (@vinhnx)
- Implement Gemini 3.1 Pro Preview models with updated token limits and system prompt handling. (dc0742c0) (@vinhnx)
- add Claude Sonnet 4.6 model support and integrate it across model definitions, parsing, catalog, and documentation. (d460c56d) (@vinhnx)
- Implement mouse text selection in the TUI and add a new `vtcode.toml` configuration file. (83567152) (@vinhnx)
- Render GFM tables inside markdown code blocks as tables and prevent word-wrapping for table lines in the TUI. (c90f06e3) (@vinhnx)
- Implement mouse scroll support for TUI session and history picker, and update default agent configuration to Ollama. (db99f4db) (@vinhnx)
- refactor build process to use conditional cross compilation for Linux and Windows (d15bb558) (@vinhnx)
- add full CI mode to release script for all platforms (326a2c8c) (@vinhnx)
- add CI workflows for building Linux and Windows binaries; optimize release process (090bebb4) (@vinhnx)
- implement secure storage for custom API keys using OS keyring (3da5a60a) (@vinhnx)
- add Qwen3.5-397B-A17B model with hybrid architecture and update configuration (26a9a7ee) (@vinhnx)
- implement mouse scroll handling for improved navigation (24a2d640) (@vinhnx)
- add timeout handling for turn metadata collection (1b1f91d4) (@vinhnx)
- implement credential storage using OS keyring and file fallback (1e94c71a) (@vinhnx)
- add sanitizer module for secret redaction and integrate into output handling (4263808d) (@vinhnx)
- add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (fddc4887) (@vinhnx)
- introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (52b13dd1) (@vinhnx)
- Use configurable constants for agent session limits and expose the default max context tokens function. (21d1183f) (@vinhnx)
- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (68d07c91) (@vinhnx)
- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (ac806e0e) (@vinhnx)
- add /share-log command to export session log as JSON for debugging (64305820) (@vinhnx)
- implement in-process teammate runner and enhance team protocol messaging (4aff9318) (@vinhnx)
- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Bug Fixes

- correct exec_code policy and update TODO for markdown rendering issue (9b87f88b) (@vinhnx)
- resolve critical scrolling issue and remove unused slash command handlers (bcb81434) (@vinhnx)

### Refactors

- remove unused imports and enhance configuration for credential storage (b79f2bd2) (@vinhnx)
- clean up code formatting and improve readability across multiple files (b1ae3ad9) (@vinhnx)
- remove Rust cache step from release workflow (e36c5f55) (@vinhnx)
- streamline MCP tool management and indexing in ToolRegistry (a5c3677b) (@vinhnx)
- consolidate duplicated logic across workspace crates into shared utility functions (a9df41fd) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (15c45f9a) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (7fcbe5f2) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (54c447af) (@vinhnx)
- streamline file operations and enhance workspace path handling (f7ebb78d) (@vinhnx)
- extract large event handler modules into smaller files for improved navigation (9eda39e4) (@vinhnx)
- reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (95bcd08f) (@vinhnx)
- remove unused ReasoningSegment import from turn_processing.rs (9f4672d7) (@vinhnx)

### Documentation

- Add a guide for adding new models to AGENTS.md. (8648b584) (@vinhnx)
- update documentation for TECH_DEBT_TRACKER and QUALITY_SCORE; add tests for subagent loading and file operations (fb7c0944) (@vinhnx)
- update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (b4a1ef7a) (@vinhnx)
- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- Release (77422bee) (@vinhnx)
- Release (14027810) (@vinhnx)
- Release (6c982a35) (@vinhnx)
- clean up configuration file by removing unused custom API keys and simplifying array formatting (ad594e41) (@vinhnx)
- Release (4bd94bdb) (@vinhnx)
- Release (800c7069) (@vinhnx)
- Release (a08f765d) (@vinhnx)
- Release (accdcc25) (@vinhnx)
- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Update commit (9d05e9de) (@vinhnx)
- Enhance model behavior configuration for LLM providers (0fa12334) (@vinhnx)
- Add scripts for documentation link validation and markdown location checks (7290fc5b) (@vinhnx)
- Update default model in vtcode.toml to minimax-m2:cloud; format code for consistency (e024a48d) (@vinhnx)
- Add additional_agent_dirs configuration option to vtcode.toml (0f6d1747) (@vinhnx)
- Remove custom prompts feature and related code; update error handling for agent actions; refine UI shortcuts and command handling (cfcc9765) (@vinhnx)
- Remove custom prompts feature and related code (dfc3ec5d) (@vinhnx)
- Update LLM provider configuration to use Ollama and remove unused typos.toml file (ca83bdcd) (@vinhnx)
- Enhance agent runner settings and improve loop detection; update CLI commands for reasoning effort and verbosity; refine patch handling in file operations; adjust markdown diff summary; modify configuration for LLM provider. (ff221d1d) (@vinhnx)
- Refactor agent runner and tool registry for improved error handling and normalization; enhance loop detection and add tests for new functionality. (101a2b07) (@vinhnx)
- Refactor LLM request handling, improve reasoning processing, and enhance tool command parsing (896f6c69) (@vinhnx)
- Implement streaming response rendering with reasoning support and helper functions (b3ba347b) (@vinhnx)
- Refactor file operations to use utility functions for directory creation and file reading/writing (7f800b3d) (@vinhnx)
- Implement unified session loop for agent execution and remove plugin marketplace commands (17d9597f) (@vinhnx)
- Add tests for model picker and prompt refinement functionality (0305390e) (@vinhnx)
- Enhance configuration and logging for agent and hooks; add mock MCP server for integration tests (59a48db6) (@vinhnx)
- Refactor agent runner to use AgentSessionState for session management and update related components (083fcac6) (@vinhnx)
- Refactor apply_patch handler for improved output handling; streamline sandbox policy mapping (67c0e05b) (@vinhnx)
- Refactor caching logic for improved performance; enhance error context handling; update tests for accurate cache statistics (5a10f0d7) (@vinhnx)
- Refactor exec_policy and command validation; consolidate HTTP client utilities; enhance error handling; improve file operations; update middleware to async; clean up validation cache; and adjust rate limiting implementation. (e6245af6) (@vinhnx)
- Adjust provider configurations and logging. (a649d023) (@vinhnx)
- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.81.3 - 2026-02-20

### Features

- refactor build process to use conditional cross compilation for Linux and Windows (d15bb558) (@vinhnx)
- add full CI mode to release script for all platforms (326a2c8c) (@vinhnx)
- add CI workflows for building Linux and Windows binaries; optimize release process (090bebb4) (@vinhnx)
- implement secure storage for custom API keys using OS keyring (3da5a60a) (@vinhnx)
- add Qwen3.5-397B-A17B model with hybrid architecture and update configuration (26a9a7ee) (@vinhnx)
- implement mouse scroll handling for improved navigation (24a2d640) (@vinhnx)
- add timeout handling for turn metadata collection (1b1f91d4) (@vinhnx)
- implement credential storage using OS keyring and file fallback (1e94c71a) (@vinhnx)
- add sanitizer module for secret redaction and integrate into output handling (4263808d) (@vinhnx)
- add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (fddc4887) (@vinhnx)
- introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (52b13dd1) (@vinhnx)
- Use configurable constants for agent session limits and expose the default max context tokens function. (21d1183f) (@vinhnx)
- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (68d07c91) (@vinhnx)
- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (ac806e0e) (@vinhnx)
- add /share-log command to export session log as JSON for debugging (64305820) (@vinhnx)
- implement in-process teammate runner and enhance team protocol messaging (4aff9318) (@vinhnx)
- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Bug Fixes

- correct exec_code policy and update TODO for markdown rendering issue (9b87f88b) (@vinhnx)
- resolve critical scrolling issue and remove unused slash command handlers (bcb81434) (@vinhnx)

### Refactors

- remove unused imports and enhance configuration for credential storage (b79f2bd2) (@vinhnx)
- clean up code formatting and improve readability across multiple files (b1ae3ad9) (@vinhnx)
- remove Rust cache step from release workflow (e36c5f55) (@vinhnx)
- streamline MCP tool management and indexing in ToolRegistry (a5c3677b) (@vinhnx)
- consolidate duplicated logic across workspace crates into shared utility functions (a9df41fd) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (15c45f9a) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (7fcbe5f2) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (54c447af) (@vinhnx)
- streamline file operations and enhance workspace path handling (f7ebb78d) (@vinhnx)
- extract large event handler modules into smaller files for improved navigation (9eda39e4) (@vinhnx)
- reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (95bcd08f) (@vinhnx)
- remove unused ReasoningSegment import from turn_processing.rs (9f4672d7) (@vinhnx)

### Documentation

- update documentation for TECH_DEBT_TRACKER and QUALITY_SCORE; add tests for subagent loading and file operations (fb7c0944) (@vinhnx)
- update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (b4a1ef7a) (@vinhnx)
- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- Release (14027810) (@vinhnx)
- Release (6c982a35) (@vinhnx)
- clean up configuration file by removing unused custom API keys and simplifying array formatting (ad594e41) (@vinhnx)
- Release (4bd94bdb) (@vinhnx)
- Release (800c7069) (@vinhnx)
- Release (a08f765d) (@vinhnx)
- Release (accdcc25) (@vinhnx)
- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Enhance model behavior configuration for LLM providers (0fa12334) (@vinhnx)
- Add scripts for documentation link validation and markdown location checks (7290fc5b) (@vinhnx)
- Update default model in vtcode.toml to minimax-m2:cloud; format code for consistency (e024a48d) (@vinhnx)
- Add additional_agent_dirs configuration option to vtcode.toml (0f6d1747) (@vinhnx)
- Remove custom prompts feature and related code; update error handling for agent actions; refine UI shortcuts and command handling (cfcc9765) (@vinhnx)
- Remove custom prompts feature and related code (dfc3ec5d) (@vinhnx)
- Update LLM provider configuration to use Ollama and remove unused typos.toml file (ca83bdcd) (@vinhnx)
- Enhance agent runner settings and improve loop detection; update CLI commands for reasoning effort and verbosity; refine patch handling in file operations; adjust markdown diff summary; modify configuration for LLM provider. (ff221d1d) (@vinhnx)
- Refactor agent runner and tool registry for improved error handling and normalization; enhance loop detection and add tests for new functionality. (101a2b07) (@vinhnx)
- Refactor LLM request handling, improve reasoning processing, and enhance tool command parsing (896f6c69) (@vinhnx)
- Implement streaming response rendering with reasoning support and helper functions (b3ba347b) (@vinhnx)
- Refactor file operations to use utility functions for directory creation and file reading/writing (7f800b3d) (@vinhnx)
- Implement unified session loop for agent execution and remove plugin marketplace commands (17d9597f) (@vinhnx)
- Add tests for model picker and prompt refinement functionality (0305390e) (@vinhnx)
- Enhance configuration and logging for agent and hooks; add mock MCP server for integration tests (59a48db6) (@vinhnx)
- Refactor agent runner to use AgentSessionState for session management and update related components (083fcac6) (@vinhnx)
- Refactor apply_patch handler for improved output handling; streamline sandbox policy mapping (67c0e05b) (@vinhnx)
- Refactor caching logic for improved performance; enhance error context handling; update tests for accurate cache statistics (5a10f0d7) (@vinhnx)
- Refactor exec_policy and command validation; consolidate HTTP client utilities; enhance error handling; improve file operations; update middleware to async; clean up validation cache; and adjust rate limiting implementation. (e6245af6) (@vinhnx)
- Adjust provider configurations and logging. (a649d023) (@vinhnx)
- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.81.2 - 2026-02-19

### Features

- add full CI mode to release script for all platforms (326a2c8c) (@vinhnx)
- add CI workflows for building Linux and Windows binaries; optimize release process (090bebb4) (@vinhnx)
- implement secure storage for custom API keys using OS keyring (3da5a60a) (@vinhnx)
- add Qwen3.5-397B-A17B model with hybrid architecture and update configuration (26a9a7ee) (@vinhnx)
- implement mouse scroll handling for improved navigation (24a2d640) (@vinhnx)
- add timeout handling for turn metadata collection (1b1f91d4) (@vinhnx)
- implement credential storage using OS keyring and file fallback (1e94c71a) (@vinhnx)
- add sanitizer module for secret redaction and integrate into output handling (4263808d) (@vinhnx)
- add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (fddc4887) (@vinhnx)
- introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (52b13dd1) (@vinhnx)
- Use configurable constants for agent session limits and expose the default max context tokens function. (21d1183f) (@vinhnx)
- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (68d07c91) (@vinhnx)
- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (ac806e0e) (@vinhnx)
- add /share-log command to export session log as JSON for debugging (64305820) (@vinhnx)
- implement in-process teammate runner and enhance team protocol messaging (4aff9318) (@vinhnx)
- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Bug Fixes

- correct exec_code policy and update TODO for markdown rendering issue (9b87f88b) (@vinhnx)
- resolve critical scrolling issue and remove unused slash command handlers (bcb81434) (@vinhnx)

### Refactors

- remove unused imports and enhance configuration for credential storage (b79f2bd2) (@vinhnx)
- clean up code formatting and improve readability across multiple files (b1ae3ad9) (@vinhnx)
- remove Rust cache step from release workflow (e36c5f55) (@vinhnx)
- streamline MCP tool management and indexing in ToolRegistry (a5c3677b) (@vinhnx)
- consolidate duplicated logic across workspace crates into shared utility functions (a9df41fd) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (15c45f9a) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (7fcbe5f2) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (54c447af) (@vinhnx)
- streamline file operations and enhance workspace path handling (f7ebb78d) (@vinhnx)
- extract large event handler modules into smaller files for improved navigation (9eda39e4) (@vinhnx)
- reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (95bcd08f) (@vinhnx)
- remove unused ReasoningSegment import from turn_processing.rs (9f4672d7) (@vinhnx)

### Documentation

- update documentation for TECH_DEBT_TRACKER and QUALITY_SCORE; add tests for subagent loading and file operations (fb7c0944) (@vinhnx)
- update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (b4a1ef7a) (@vinhnx)
- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- Release (6c982a35) (@vinhnx)
- clean up configuration file by removing unused custom API keys and simplifying array formatting (ad594e41) (@vinhnx)
- Release (4bd94bdb) (@vinhnx)
- Release (800c7069) (@vinhnx)
- Release (a08f765d) (@vinhnx)
- Release (accdcc25) (@vinhnx)
- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Enhance model behavior configuration for LLM providers (0fa12334) (@vinhnx)
- Add scripts for documentation link validation and markdown location checks (7290fc5b) (@vinhnx)
- Update default model in vtcode.toml to minimax-m2:cloud; format code for consistency (e024a48d) (@vinhnx)
- Add additional_agent_dirs configuration option to vtcode.toml (0f6d1747) (@vinhnx)
- Remove custom prompts feature and related code; update error handling for agent actions; refine UI shortcuts and command handling (cfcc9765) (@vinhnx)
- Remove custom prompts feature and related code (dfc3ec5d) (@vinhnx)
- Update LLM provider configuration to use Ollama and remove unused typos.toml file (ca83bdcd) (@vinhnx)
- Enhance agent runner settings and improve loop detection; update CLI commands for reasoning effort and verbosity; refine patch handling in file operations; adjust markdown diff summary; modify configuration for LLM provider. (ff221d1d) (@vinhnx)
- Refactor agent runner and tool registry for improved error handling and normalization; enhance loop detection and add tests for new functionality. (101a2b07) (@vinhnx)
- Refactor LLM request handling, improve reasoning processing, and enhance tool command parsing (896f6c69) (@vinhnx)
- Implement streaming response rendering with reasoning support and helper functions (b3ba347b) (@vinhnx)
- Refactor file operations to use utility functions for directory creation and file reading/writing (7f800b3d) (@vinhnx)
- Implement unified session loop for agent execution and remove plugin marketplace commands (17d9597f) (@vinhnx)
- Add tests for model picker and prompt refinement functionality (0305390e) (@vinhnx)
- Enhance configuration and logging for agent and hooks; add mock MCP server for integration tests (59a48db6) (@vinhnx)
- Refactor agent runner to use AgentSessionState for session management and update related components (083fcac6) (@vinhnx)
- Refactor apply_patch handler for improved output handling; streamline sandbox policy mapping (67c0e05b) (@vinhnx)
- Refactor caching logic for improved performance; enhance error context handling; update tests for accurate cache statistics (5a10f0d7) (@vinhnx)
- Refactor exec_policy and command validation; consolidate HTTP client utilities; enhance error handling; improve file operations; update middleware to async; clean up validation cache; and adjust rate limiting implementation. (e6245af6) (@vinhnx)
- Adjust provider configurations and logging. (a649d023) (@vinhnx)
- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.81.1 - 2026-02-17

### Features

- implement secure storage for custom API keys using OS keyring (3da5a60a) (@vinhnx)
- add Qwen3.5-397B-A17B model with hybrid architecture and update configuration (26a9a7ee) (@vinhnx)
- implement mouse scroll handling for improved navigation (24a2d640) (@vinhnx)
- add timeout handling for turn metadata collection (1b1f91d4) (@vinhnx)
- implement credential storage using OS keyring and file fallback (1e94c71a) (@vinhnx)
- add sanitizer module for secret redaction and integrate into output handling (4263808d) (@vinhnx)
- add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (fddc4887) (@vinhnx)
- introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (52b13dd1) (@vinhnx)
- Use configurable constants for agent session limits and expose the default max context tokens function. (21d1183f) (@vinhnx)
- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (68d07c91) (@vinhnx)
- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (ac806e0e) (@vinhnx)
- add /share-log command to export session log as JSON for debugging (64305820) (@vinhnx)
- implement in-process teammate runner and enhance team protocol messaging (4aff9318) (@vinhnx)
- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Bug Fixes

- correct exec_code policy and update TODO for markdown rendering issue (9b87f88b) (@vinhnx)
- resolve critical scrolling issue and remove unused slash command handlers (bcb81434) (@vinhnx)

### Refactors

- remove unused imports and enhance configuration for credential storage (b79f2bd2) (@vinhnx)
- clean up code formatting and improve readability across multiple files (b1ae3ad9) (@vinhnx)
- remove Rust cache step from release workflow (e36c5f55) (@vinhnx)
- streamline MCP tool management and indexing in ToolRegistry (a5c3677b) (@vinhnx)
- consolidate duplicated logic across workspace crates into shared utility functions (a9df41fd) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (15c45f9a) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (7fcbe5f2) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (54c447af) (@vinhnx)
- streamline file operations and enhance workspace path handling (f7ebb78d) (@vinhnx)
- extract large event handler modules into smaller files for improved navigation (9eda39e4) (@vinhnx)
- reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (95bcd08f) (@vinhnx)
- remove unused ReasoningSegment import from turn_processing.rs (9f4672d7) (@vinhnx)

### Documentation

- update documentation for TECH_DEBT_TRACKER and QUALITY_SCORE; add tests for subagent loading and file operations (fb7c0944) (@vinhnx)
- update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (b4a1ef7a) (@vinhnx)
- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- clean up configuration file by removing unused custom API keys and simplifying array formatting (ad594e41) (@vinhnx)
- Release (4bd94bdb) (@vinhnx)
- Release (800c7069) (@vinhnx)
- Release (a08f765d) (@vinhnx)
- Release (accdcc25) (@vinhnx)
- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Enhance model behavior configuration for LLM providers (0fa12334) (@vinhnx)
- Add scripts for documentation link validation and markdown location checks (7290fc5b) (@vinhnx)
- Update default model in vtcode.toml to minimax-m2:cloud; format code for consistency (e024a48d) (@vinhnx)
- Add additional_agent_dirs configuration option to vtcode.toml (0f6d1747) (@vinhnx)
- Remove custom prompts feature and related code; update error handling for agent actions; refine UI shortcuts and command handling (cfcc9765) (@vinhnx)
- Remove custom prompts feature and related code (dfc3ec5d) (@vinhnx)
- Update LLM provider configuration to use Ollama and remove unused typos.toml file (ca83bdcd) (@vinhnx)
- Enhance agent runner settings and improve loop detection; update CLI commands for reasoning effort and verbosity; refine patch handling in file operations; adjust markdown diff summary; modify configuration for LLM provider. (ff221d1d) (@vinhnx)
- Refactor agent runner and tool registry for improved error handling and normalization; enhance loop detection and add tests for new functionality. (101a2b07) (@vinhnx)
- Refactor LLM request handling, improve reasoning processing, and enhance tool command parsing (896f6c69) (@vinhnx)
- Implement streaming response rendering with reasoning support and helper functions (b3ba347b) (@vinhnx)
- Refactor file operations to use utility functions for directory creation and file reading/writing (7f800b3d) (@vinhnx)
- Implement unified session loop for agent execution and remove plugin marketplace commands (17d9597f) (@vinhnx)
- Add tests for model picker and prompt refinement functionality (0305390e) (@vinhnx)
- Enhance configuration and logging for agent and hooks; add mock MCP server for integration tests (59a48db6) (@vinhnx)
- Refactor agent runner to use AgentSessionState for session management and update related components (083fcac6) (@vinhnx)
- Refactor apply_patch handler for improved output handling; streamline sandbox policy mapping (67c0e05b) (@vinhnx)
- Refactor caching logic for improved performance; enhance error context handling; update tests for accurate cache statistics (5a10f0d7) (@vinhnx)
- Refactor exec_policy and command validation; consolidate HTTP client utilities; enhance error handling; improve file operations; update middleware to async; clean up validation cache; and adjust rate limiting implementation. (e6245af6) (@vinhnx)
- Adjust provider configurations and logging. (a649d023) (@vinhnx)
- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.81.0 - 2026-02-16

### Features

- add Qwen3.5-397B-A17B model with hybrid architecture and update configuration (26a9a7ee) (@vinhnx)
- implement mouse scroll handling for improved navigation (24a2d640) (@vinhnx)
- add timeout handling for turn metadata collection (1b1f91d4) (@vinhnx)
- implement credential storage using OS keyring and file fallback (1e94c71a) (@vinhnx)
- add sanitizer module for secret redaction and integrate into output handling (4263808d) (@vinhnx)
- add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (fddc4887) (@vinhnx)
- introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (52b13dd1) (@vinhnx)
- Use configurable constants for agent session limits and expose the default max context tokens function. (21d1183f) (@vinhnx)
- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (68d07c91) (@vinhnx)
- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (ac806e0e) (@vinhnx)
- add /share-log command to export session log as JSON for debugging (64305820) (@vinhnx)
- implement in-process teammate runner and enhance team protocol messaging (4aff9318) (@vinhnx)
- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Bug Fixes

- correct exec_code policy and update TODO for markdown rendering issue (9b87f88b) (@vinhnx)
- resolve critical scrolling issue and remove unused slash command handlers (bcb81434) (@vinhnx)

### Refactors

- remove unused imports and enhance configuration for credential storage (b79f2bd2) (@vinhnx)
- clean up code formatting and improve readability across multiple files (b1ae3ad9) (@vinhnx)
- remove Rust cache step from release workflow (e36c5f55) (@vinhnx)
- streamline MCP tool management and indexing in ToolRegistry (a5c3677b) (@vinhnx)
- consolidate duplicated logic across workspace crates into shared utility functions (a9df41fd) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (15c45f9a) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (7fcbe5f2) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (54c447af) (@vinhnx)
- streamline file operations and enhance workspace path handling (f7ebb78d) (@vinhnx)
- extract large event handler modules into smaller files for improved navigation (9eda39e4) (@vinhnx)
- reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (95bcd08f) (@vinhnx)
- remove unused ReasoningSegment import from turn_processing.rs (9f4672d7) (@vinhnx)

### Documentation

- update documentation for TECH_DEBT_TRACKER and QUALITY_SCORE; add tests for subagent loading and file operations (fb7c0944) (@vinhnx)
- update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (b4a1ef7a) (@vinhnx)
- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- Release (800c7069) (@vinhnx)
- Release (a08f765d) (@vinhnx)
- Release (accdcc25) (@vinhnx)
- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Enhance model behavior configuration for LLM providers (0fa12334) (@vinhnx)
- Add scripts for documentation link validation and markdown location checks (7290fc5b) (@vinhnx)
- Update default model in vtcode.toml to minimax-m2:cloud; format code for consistency (e024a48d) (@vinhnx)
- Add additional_agent_dirs configuration option to vtcode.toml (0f6d1747) (@vinhnx)
- Remove custom prompts feature and related code; update error handling for agent actions; refine UI shortcuts and command handling (cfcc9765) (@vinhnx)
- Remove custom prompts feature and related code (dfc3ec5d) (@vinhnx)
- Update LLM provider configuration to use Ollama and remove unused typos.toml file (ca83bdcd) (@vinhnx)
- Enhance agent runner settings and improve loop detection; update CLI commands for reasoning effort and verbosity; refine patch handling in file operations; adjust markdown diff summary; modify configuration for LLM provider. (ff221d1d) (@vinhnx)
- Refactor agent runner and tool registry for improved error handling and normalization; enhance loop detection and add tests for new functionality. (101a2b07) (@vinhnx)
- Refactor LLM request handling, improve reasoning processing, and enhance tool command parsing (896f6c69) (@vinhnx)
- Implement streaming response rendering with reasoning support and helper functions (b3ba347b) (@vinhnx)
- Refactor file operations to use utility functions for directory creation and file reading/writing (7f800b3d) (@vinhnx)
- Implement unified session loop for agent execution and remove plugin marketplace commands (17d9597f) (@vinhnx)
- Add tests for model picker and prompt refinement functionality (0305390e) (@vinhnx)
- Enhance configuration and logging for agent and hooks; add mock MCP server for integration tests (59a48db6) (@vinhnx)
- Refactor agent runner to use AgentSessionState for session management and update related components (083fcac6) (@vinhnx)
- Refactor apply_patch handler for improved output handling; streamline sandbox policy mapping (67c0e05b) (@vinhnx)
- Refactor caching logic for improved performance; enhance error context handling; update tests for accurate cache statistics (5a10f0d7) (@vinhnx)
- Refactor exec_policy and command validation; consolidate HTTP client utilities; enhance error handling; improve file operations; update middleware to async; clean up validation cache; and adjust rate limiting implementation. (e6245af6) (@vinhnx)
- Adjust provider configurations and logging. (a649d023) (@vinhnx)
- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.80.1 - 2026-02-16

### Features

- implement mouse scroll handling for improved navigation (24a2d640) (@vinhnx)
- add timeout handling for turn metadata collection (1b1f91d4) (@vinhnx)
- implement credential storage using OS keyring and file fallback (1e94c71a) (@vinhnx)
- add sanitizer module for secret redaction and integrate into output handling (4263808d) (@vinhnx)
- add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (fddc4887) (@vinhnx)
- introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (52b13dd1) (@vinhnx)
- Use configurable constants for agent session limits and expose the default max context tokens function. (21d1183f) (@vinhnx)
- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (68d07c91) (@vinhnx)
- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (ac806e0e) (@vinhnx)
- add /share-log command to export session log as JSON for debugging (64305820) (@vinhnx)
- implement in-process teammate runner and enhance team protocol messaging (4aff9318) (@vinhnx)
- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Bug Fixes

- correct exec_code policy and update TODO for markdown rendering issue (9b87f88b) (@vinhnx)
- resolve critical scrolling issue and remove unused slash command handlers (bcb81434) (@vinhnx)

### Refactors

- remove unused imports and enhance configuration for credential storage (b79f2bd2) (@vinhnx)
- clean up code formatting and improve readability across multiple files (b1ae3ad9) (@vinhnx)
- remove Rust cache step from release workflow (e36c5f55) (@vinhnx)
- streamline MCP tool management and indexing in ToolRegistry (a5c3677b) (@vinhnx)
- consolidate duplicated logic across workspace crates into shared utility functions (a9df41fd) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (15c45f9a) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (7fcbe5f2) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (54c447af) (@vinhnx)
- streamline file operations and enhance workspace path handling (f7ebb78d) (@vinhnx)
- extract large event handler modules into smaller files for improved navigation (9eda39e4) (@vinhnx)
- reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (95bcd08f) (@vinhnx)
- remove unused ReasoningSegment import from turn_processing.rs (9f4672d7) (@vinhnx)

### Documentation

- update documentation for TECH_DEBT_TRACKER and QUALITY_SCORE; add tests for subagent loading and file operations (fb7c0944) (@vinhnx)
- update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (b4a1ef7a) (@vinhnx)
- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- Release (a08f765d) (@vinhnx)
- Release (accdcc25) (@vinhnx)
- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Enhance model behavior configuration for LLM providers (0fa12334) (@vinhnx)
- Add scripts for documentation link validation and markdown location checks (7290fc5b) (@vinhnx)
- Update default model in vtcode.toml to minimax-m2:cloud; format code for consistency (e024a48d) (@vinhnx)
- Add additional_agent_dirs configuration option to vtcode.toml (0f6d1747) (@vinhnx)
- Remove custom prompts feature and related code; update error handling for agent actions; refine UI shortcuts and command handling (cfcc9765) (@vinhnx)
- Remove custom prompts feature and related code (dfc3ec5d) (@vinhnx)
- Update LLM provider configuration to use Ollama and remove unused typos.toml file (ca83bdcd) (@vinhnx)
- Enhance agent runner settings and improve loop detection; update CLI commands for reasoning effort and verbosity; refine patch handling in file operations; adjust markdown diff summary; modify configuration for LLM provider. (ff221d1d) (@vinhnx)
- Refactor agent runner and tool registry for improved error handling and normalization; enhance loop detection and add tests for new functionality. (101a2b07) (@vinhnx)
- Refactor LLM request handling, improve reasoning processing, and enhance tool command parsing (896f6c69) (@vinhnx)
- Implement streaming response rendering with reasoning support and helper functions (b3ba347b) (@vinhnx)
- Refactor file operations to use utility functions for directory creation and file reading/writing (7f800b3d) (@vinhnx)
- Implement unified session loop for agent execution and remove plugin marketplace commands (17d9597f) (@vinhnx)
- Add tests for model picker and prompt refinement functionality (0305390e) (@vinhnx)
- Enhance configuration and logging for agent and hooks; add mock MCP server for integration tests (59a48db6) (@vinhnx)
- Refactor agent runner to use AgentSessionState for session management and update related components (083fcac6) (@vinhnx)
- Refactor apply_patch handler for improved output handling; streamline sandbox policy mapping (67c0e05b) (@vinhnx)
- Refactor caching logic for improved performance; enhance error context handling; update tests for accurate cache statistics (5a10f0d7) (@vinhnx)
- Refactor exec_policy and command validation; consolidate HTTP client utilities; enhance error handling; improve file operations; update middleware to async; clean up validation cache; and adjust rate limiting implementation. (e6245af6) (@vinhnx)
- Adjust provider configurations and logging. (a649d023) (@vinhnx)
- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.80.0 - 2026-02-16

### Features

- add Kimi K2.5 model support across OpenRouter, Ollama, and HuggingFace. (fddc4887) (@vinhnx)
- introduce agent legibility guidelines and refine steering message variants for clarity and structured output. (52b13dd1) (@vinhnx)
- Use configurable constants for agent session limits and expose the default max context tokens function. (21d1183f) (@vinhnx)
- Enhance reasoning display by introducing structured `ReasoningSegment` with stages and improved rendering in the TUI. (68d07c91) (@vinhnx)
- Implement agent steering mechanism to control runloop execution, including stop, pause, resume, and input injection capabilities. (ac806e0e) (@vinhnx)
- add /share-log command to export session log as JSON for debugging (64305820) (@vinhnx)
- implement in-process teammate runner and enhance team protocol messaging (4aff9318) (@vinhnx)
- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Refactors

- consolidate duplicated logic across workspace crates into shared utility functions (a9df41fd) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (15c45f9a) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (7fcbe5f2) (@vinhnx)
- replace manual file operations with shared utility functions for consistency (54c447af) (@vinhnx)
- streamline file operations and enhance workspace path handling (f7ebb78d) (@vinhnx)
- extract large event handler modules into smaller files for improved navigation (9eda39e4) (@vinhnx)
- reimplement LLM streaming and event handling using AgentSessionController and its event sink mechanism. (95bcd08f) (@vinhnx)
- remove unused ReasoningSegment import from turn_processing.rs (9f4672d7) (@vinhnx)

### Documentation

- update contributing guidelines to reference CONTRIBUTING.md and AGENTS.md. (b4a1ef7a) (@vinhnx)
- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- Release (accdcc25) (@vinhnx)
- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Update default model in vtcode.toml to minimax-m2:cloud; format code for consistency (e024a48d) (@vinhnx)
- Add additional_agent_dirs configuration option to vtcode.toml (0f6d1747) (@vinhnx)
- Remove custom prompts feature and related code; update error handling for agent actions; refine UI shortcuts and command handling (cfcc9765) (@vinhnx)
- Remove custom prompts feature and related code (dfc3ec5d) (@vinhnx)
- Update LLM provider configuration to use Ollama and remove unused typos.toml file (ca83bdcd) (@vinhnx)
- Enhance agent runner settings and improve loop detection; update CLI commands for reasoning effort and verbosity; refine patch handling in file operations; adjust markdown diff summary; modify configuration for LLM provider. (ff221d1d) (@vinhnx)
- Refactor agent runner and tool registry for improved error handling and normalization; enhance loop detection and add tests for new functionality. (101a2b07) (@vinhnx)
- Refactor LLM request handling, improve reasoning processing, and enhance tool command parsing (896f6c69) (@vinhnx)
- Implement streaming response rendering with reasoning support and helper functions (b3ba347b) (@vinhnx)
- Refactor file operations to use utility functions for directory creation and file reading/writing (7f800b3d) (@vinhnx)
- Implement unified session loop for agent execution and remove plugin marketplace commands (17d9597f) (@vinhnx)
- Add tests for model picker and prompt refinement functionality (0305390e) (@vinhnx)
- Enhance configuration and logging for agent and hooks; add mock MCP server for integration tests (59a48db6) (@vinhnx)
- Refactor agent runner to use AgentSessionState for session management and update related components (083fcac6) (@vinhnx)
- Refactor apply_patch handler for improved output handling; streamline sandbox policy mapping (67c0e05b) (@vinhnx)
- Refactor caching logic for improved performance; enhance error context handling; update tests for accurate cache statistics (5a10f0d7) (@vinhnx)
- Refactor exec_policy and command validation; consolidate HTTP client utilities; enhance error handling; improve file operations; update middleware to async; clean up validation cache; and adjust rate limiting implementation. (e6245af6) (@vinhnx)
- Adjust provider configurations and logging. (a649d023) (@vinhnx)
- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.79.4 - 2026-02-14

### Features

- implement plan mode toggle and strip proposed plan blocks in rendering (79f0327d) (@vinhnx)
- add skill bundle import/export functionality with zip support (dea5b5b7) (@vinhnx)
- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Documentation

- update documentation and improve clarity on execution plans, architectural invariants, and quality scores feat: enhance system instruction generation to replace placeholders with unified tool guidance (27c61ef7) (@vinhnx)

### Chores

- Release (69df0e20) (@vinhnx)
- Release (10e4f284) (@vinhnx)

### Other

- Add support for inline skill bundles and network policies in skills (ddb9d280) (@vinhnx)
- Add architectural invariants, core beliefs, execution plans, quality score, and tech debt tracker documentation (80cf6d68) (@vinhnx)

## 0.79.3 - 2026-02-13

### Features

- add Qwen3 Coder Next model support and update related constants (5a4303e0) (@vinhnx)
- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

### Chores

- Release (10e4f284) (@vinhnx)

## 0.79.2 - 2026-02-13

### Features

- add MiniMax M2.5 model support across various providers and update related constants (968963f1) (@vinhnx)

## v0.79.1 - 2026-02-13

### Features

- add support for MoonshotAI Kimi K2 models in ModelId (90e18ff2) (@vinhnx)
- complete model migration, fix test failures, and enhance UI stability (32f252ec) (@vinhnx)
- add pty_stream module and integrate it into tool pipeline execution (17b0c9d6) (@vinhnx)

### Refactors

- increase spooling thresholds and improve output handling for large tool outputs (dbae38d2) (@vinhnx)
- enhance file output handling and add no_spool flag for read operations (a3c134c4) (@vinhnx)
- enhance path validation logic and add lexical workspace check (605ea1ec) (@vinhnx)
- modularize tool output handling and enhance command safety validation (88146309) (@vinhnx)
- enhance tool validation and error messaging, modularize execution logic (2f6d20f0) (@vinhnx)
- streamline MCP event handling and enhance error content construction (9510ef79) (@vinhnx)
- remove dead code and streamline path handling functions (35289283) (@vinhnx)
- replace hardcoded Plan Mode strings with constants for consistency and maintainability (5d4d2407) (@vinhnx)
- update TODO with comprehensive code audit and optimization guidelines (4f96ff24) (@vinhnx)
- streamline error handling and validation logic; enhance retry safety checks (754ce484) (@vinhnx)
- enhance IDE context flushing and user confirmation handling; improve command auditing (b00d3b7f) (@vinhnx)
- improve tool validation and error handling; enhance test coverage for non-interactive environments (4b47208b) (@vinhnx)
- enhance error handling for tool arguments and improve rate limiting logic (386ce6aa) (@vinhnx)
- remove unused imports in turn_loop.rs for cleaner code (936e1885) (@vinhnx)
- improve code formatting and readability across multiple files (393f63e9) (@vinhnx)
- replace FxHashMap with LoopTracker for tool attempt tracking; optimize loop detection and history management (bb831f19) (@vinhnx)
- add token tracking validation in ContextManager; optimize turn balancer check intervals (4dd087c9) (@vinhnx)
- optimize tool signature handling and caching; enhance turn configuration extraction and prompt caching (b42c331a) (@vinhnx)
- comprehensive optimization of agent loop and tool execution pipeline (e97e91c0) (@vinhnx)
- improve code readability and structure across multiple files (2b5a2895) (@vinhnx)
- optimize line truncation logic in summarizers (4996c6ad) (@vinhnx)
- consolidate path resolution logic and remove redundant functions (d0a10bad) (@vinhnx)

### Tests

- skip TUI-dependent tests in non-interactive environments (4559d31e) (@vinhnx)

### Chores

- Release (4000421c) (@vinhnx)

### Other

- Update models and configurations for Gemini 3 and GLM-5; adjust tool capabilities and user confirmations (7f05b778) (@vinhnx)
- Add GLM-5 model support and remove deprecated GLM-4.5/4.6 models (29d0992a) (@vinhnx)
- Implement tool catalog state management and integrate into MCP tool lifecycle (18c73b54) (@vinhnx)
- Enhance plan mode handling and tool safety validation; refactor prompt management and session loop logic (63a40249) (@vinhnx)
- Add prompt assembly mode and enhance tool validation (480ed33a) (@vinhnx)
- Add safety validation and transition functions for plan mode handling (9db46b25) (@vinhnx)
- Refactor optimizer and tool result handling; enhance turn duration recording; update validation and state management; optimize loop detection; improve LLM request handling; remove fallback chains module; streamline tool execution checks; fix TUI modal search handling; adjust integration tests for tool usage. (9df107c3) (@vinhnx)

## v0.79.0 - 2026-02-13

### Features

- complete model migration, fix test failures, and enhance UI stability (32f252ec) (@vinhnx)
- add pty_stream module and integrate it into tool pipeline execution (17b0c9d6) (@vinhnx)

### Refactors

- increase spooling thresholds and improve output handling for large tool outputs (dbae38d2) (@vinhnx)
- enhance file output handling and add no_spool flag for read operations (a3c134c4) (@vinhnx)
- enhance path validation logic and add lexical workspace check (605ea1ec) (@vinhnx)
- modularize tool output handling and enhance command safety validation (88146309) (@vinhnx)
- enhance tool validation and error messaging, modularize execution logic (2f6d20f0) (@vinhnx)
- streamline MCP event handling and enhance error content construction (9510ef79) (@vinhnx)
- remove dead code and streamline path handling functions (35289283) (@vinhnx)
- replace hardcoded Plan Mode strings with constants for consistency and maintainability (5d4d2407) (@vinhnx)
- update TODO with comprehensive code audit and optimization guidelines (4f96ff24) (@vinhnx)
- streamline error handling and validation logic; enhance retry safety checks (754ce484) (@vinhnx)
- enhance IDE context flushing and user confirmation handling; improve command auditing (b00d3b7f) (@vinhnx)
- improve tool validation and error handling; enhance test coverage for non-interactive environments (4b47208b) (@vinhnx)
- enhance error handling for tool arguments and improve rate limiting logic (386ce6aa) (@vinhnx)
- remove unused imports in turn_loop.rs for cleaner code (936e1885) (@vinhnx)
- improve code formatting and readability across multiple files (393f63e9) (@vinhnx)
- replace FxHashMap with LoopTracker for tool attempt tracking; optimize loop detection and history management (bb831f19) (@vinhnx)
- add token tracking validation in ContextManager; optimize turn balancer check intervals (4dd087c9) (@vinhnx)
- optimize tool signature handling and caching; enhance turn configuration extraction and prompt caching (b42c331a) (@vinhnx)
- comprehensive optimization of agent loop and tool execution pipeline (e97e91c0) (@vinhnx)
- improve code readability and structure across multiple files (2b5a2895) (@vinhnx)
- optimize line truncation logic in summarizers (4996c6ad) (@vinhnx)
- consolidate path resolution logic and remove redundant functions (d0a10bad) (@vinhnx)

### Tests

- skip TUI-dependent tests in non-interactive environments (4559d31e) (@vinhnx)

### Other

- Update models and configurations for Gemini 3 and GLM-5; adjust tool capabilities and user confirmations (7f05b778) (@vinhnx)
- Add GLM-5 model support and remove deprecated GLM-4.5/4.6 models (29d0992a) (@vinhnx)
- Implement tool catalog state management and integrate into MCP tool lifecycle (18c73b54) (@vinhnx)
- Enhance plan mode handling and tool safety validation; refactor prompt management and session loop logic (63a40249) (@vinhnx)
- Add prompt assembly mode and enhance tool validation (480ed33a) (@vinhnx)
- Add safety validation and transition functions for plan mode handling (9db46b25) (@vinhnx)
- Refactor optimizer and tool result handling; enhance turn duration recording; update validation and state management; optimize loop detection; improve LLM request handling; remove fallback chains module; streamline tool execution checks; fix TUI modal search handling; adjust integration tests for tool usage. (9df107c3) (@vinhnx)

## v0.78.8 - 2026-02-09

### Refactors

- streamline release process and remove deprecated crate waiting logic (4436c3cc) (@vinhnx)

## v0.78.7 - 2026-02-09

*No significant changes*

## v0.78.6 - 2026-02-09

### Features

- add wait_for_crates_io function to ensure crate availability on crates.io (8b4ac577) (@vinhnx)

## v0.78.5 - 2026-02-09

### Other

- Remove outdated optimization notes and focus on DRY opportunities in the codebase (c9ac418b) (@vinhnx)
- Refactor to use rustc_hash::FxHashMap for improved performance and memory efficiency; update related structures and configurations. (3b166144) (@vinhnx)

## v0.78.4 - 2026-02-08

*No significant changes*

## v0.78.3 - 2026-02-08

### Features

- enhance crate publishing process with reliable version parsing and no-verify option (4fb5612c) (@vinhnx)
- implement delete_word_forward method in Session (5936d8bc) (@vinhnx)
- add delete_word_forward method to InputManager and update LayoutMode footer behavior (29e2dd09) (@vinhnx)

### Other

- Revert "feat: integrate `tui_input` crate for enhanced input management and modal search functionality" (b6e27465) (@vinhnx)

## v0.78.2 - 2026-02-08

### Features

- Display a scroll indicator in the TUI footer and adjust status height calculation based on layout mode. (6c5efc03) (@vinhnx)
- Introduce compile-time optimization guide and profiling script, and add general performance principles to TODO. (f1257ba9) (@vinhnx)
- add 'mono' theme and improve TUI modal search input handling. (7fd6334c) (@vinhnx)
- integrate `tui_input` crate for enhanced input management and modal search functionality (69a24ed3) (@vinhnx)
- improve plugin validation and enhance path resolution in PTY manager (a2ea12c1) (@vinhnx)
- add path utilities and normalize ASCII identifiers for improved path handling (5a888daf) (@vinhnx)
- implement command blocking during running tasks and update configuration for LLM provider (da55d7cd) (@vinhnx)
- enhance Plan Mode with reminders and execution prompts (ba69b139) (@vinhnx)

### Performance

- Cache session header lines and queued input previews to optimize TUI rendering performance and remove outdated content from TODO.md. (cf87bc80) (@vinhnx)

### Refactors

- Improve string truncation logic to ensure character boundaries are respected (83ebed35) (@vinhnx)
- Optimize I/O operations with buffered writes and simplify `ToolCallRecord`'s `tool_name` ownership. (a66a3a9d) (@vinhnx)
- remove scroll indicator from footer widget and associated UI logic. (4f060bed) (@vinhnx)

### Other

- Immprove (e3fc8d93) (@vinhnx)
- Add team context and teammate management features (b322e6cd) (@vinhnx)

## v0.78.1 - 2026-02-07

### Features

- enhance input handling with queue overlay and update input placeholders (8394895f) (@vinhnx)
- implement queue editing functionality and update input handling (6c0d373b) (@vinhnx)
- add support for inline data URLs and images in message content (90254442) (@vinhnx)

### Bug Fixes

- add exit_plan_mode tool to planner agent and update tests (63a6835e) (@vinhnx)

### Other

- Implement collapsible pasted message handling and improve image path parsing (5efac36e) (@vinhnx)

## v0.78.0 - 2026-02-06

### Documentation

- add task summaries feature to agent teams and enhance subagent matching logic (5fa919c1) (@vinhnx)

### Other

- Refactor CLI argument documentation for clarity and consistency (087be239) (@vinhnx)
- Enhance user input tools to restrict usage to Plan mode only (93675177) (@vinhnx)
- Improve output spooler and system prompt handling (255e0a44) (@vinhnx)
- Add experimental agent teams feature with slash commands and configuration (b70ce06e) (@vinhnx)
- Update script (35c8d01f) (@vinhnx)
- Remove code repetition and special casing of local providers (ec4b2099) (@gzsombor)

## v0.77.1 - 2026-02-06

### Chores

- Release (210e6503) (@vinhnx)
- update tool policy and improve tool registration descriptions (09e87101) (@vinhnx)
- update config - enable list_skills tool and adjust settings (401cc2f9) (@vinhnx)

### Other

- Update commit (717601ea) (@vinhnx)
- Add jq dependency check and improve cargo release process (5c9a2c82) (@vinhnx)
- Add support for effort parameter in Anthropic API and related validation (9519a720) (@vinhnx)
- Add adaptive thinking support for Claude Opus 4.6 model and update related configurations (19515dee) (@vinhnx)
- Add context management support to LLM requests and related components (e893cee1) (@vinhnx)
- Add support for Claude Opus 4.6 model with adaptive thinking and update related configurations (77d0d485) (@vinhnx)
- Implement tool safety checks, enhance wizard modal, and update configuration (574d60de) (@vinhnx)
- Add skills-ref commands for skill validation, listing, and prompt generation; update skill discovery paths and manifest structure (df2919d9) (@vinhnx)
- Enhance input widget styling with padding and background; update configuration theme and editing mode (958f7c38) (@vinhnx)
- Cleanup TODO.md by removing outdated tasks and enhancing UI transition notes (21d1d5c6) (@vinhnx)
- Refactor spinner implementation and enhance cursor behavior during status updates (0b6828ac) (@vinhnx)
- Refactor test assertion for compact_title method in MCP event (4f44b68a) (@vinhnx)
- Implement command caching and gatekeeper policy; enhance file reading with async logging and performance tracking (e464ee31) (@vinhnx)

## v0.77.0 - 2026-02-06

### Chores

- update tool policy and improve tool registration descriptions (09e87101) (@vinhnx)
- update config - enable list_skills tool and adjust settings (401cc2f9) (@vinhnx)

### Other

- Add support for effort parameter in Anthropic API and related validation (9519a720) (@vinhnx)
- Add adaptive thinking support for Claude Opus 4.6 model and update related configurations (19515dee) (@vinhnx)
- Add context management support to LLM requests and related components (e893cee1) (@vinhnx)
- Add support for Claude Opus 4.6 model with adaptive thinking and update related configurations (77d0d485) (@vinhnx)
- Implement tool safety checks, enhance wizard modal, and update configuration (574d60de) (@vinhnx)
- Add skills-ref commands for skill validation, listing, and prompt generation; update skill discovery paths and manifest structure (df2919d9) (@vinhnx)
- Enhance input widget styling with padding and background; update configuration theme and editing mode (958f7c38) (@vinhnx)
- Cleanup TODO.md by removing outdated tasks and enhancing UI transition notes (21d1d5c6) (@vinhnx)
- Refactor spinner implementation and enhance cursor behavior during status updates (0b6828ac) (@vinhnx)
- Refactor test assertion for compact_title method in MCP event (4f44b68a) (@vinhnx)
- Implement command caching and gatekeeper policy; enhance file reading with async logging and performance tracking (e464ee31) (@vinhnx)

## v0.76.2 - 2026-02-05

### Other

- Improve CI CD (79589790) (@vinhnx)
- Refactor MCP integration and update dependencies (b31b5407) (@vinhnx)

## v0.76.1 - 2026-02-05

### Other

- Refactor MCP integration and update dependencies (b31b5407) (@vinhnx)

## v0.76.0 - 2026-02-05

### Features

- add turn metadata support for LLM requests with git context (46a57d6d) (@vinhnx)
- enhance command safety checks for git subcommands and improve branch operation validation (ca9833f4) (@vinhnx)
- improve git changelog generator to group by commit types (9d2b46d1) (@vinhnx)
- implement shell snapshot feature to optimize command execution (e5d9d7fe) (@vinhnx)
- add git diff guidance to tool guidelines (934d723e) (@vinhnx)
- enhance agent message rendering with left padding and improved line handling (dc8025a6) (@vinhnx)
- update text deletion commands for improved line handling (7c003cae) (@vinhnx)
- clarify patch input parameters and remove 'diff' alias to prevent confusion (466549b7) (@vinhnx)
- enhance diff rendering with summary formatting and colorization (bbed557c) (@vinhnx)
- add support for inline streaming and recent spooled output retrieval (eee866c7) (@vinhnx)
- enhance diff view with changed lines count summary and line numbers (808464e9) (@vinhnx)

### Refactors

- simplify workspace directory creation in first run setup (6a35774d) (@vinhnx)

### Other

- Implement no_spool functionality for tool output and enhance cursor behavior during scrolling and shimmer states (477209fd) (@vinhnx)
- Refactor diff handling and rendering for improved clarity and summary display (f8dbf9e1) (@vinhnx)

## v0.75.2 - 2026-02-04

- Update TODO (fb065df0) (@vinhnx)
- feat: add Qwen3 Coder Next model with enhanced reasoning capabilities (26a65840) (@vinhnx)
- feat: enhance file output rendering to display diff content when applicable (0c05c762) (@vinhnx)
- feat: simplify debug script by removing sccache handling; enhance markdown diff rendering (cd8cdd28) (@vinhnx)
- feat: improve sccache error handling in debug script; retry without sccache on permission errors (29bff726) (@vinhnx)
- feat: enhance debug script to handle sccache permission errors during build and run (0beccb9e) (@vinhnx)
- feat: enhance message rendering for info boxes; group consecutive info messages and improve styling (3557bd25) (@vinhnx)
- feat: enhance UI styling and message rendering; improve error and info message handling (fa61cea9) (@vinhnx)
- feat: update tool policies, enhance message rendering, and modify default model configuration (436ac6cb) (@vinhnx)
- feat: reject hooks in skill definitions and update validation logic (6a4105e9) (@vinhnx)
- feat: update tool policies and enhance message handling; modify configuration for LLM provider (1c093ab7) (@vinhnx)
- Add webapp-testing skill with Playwright scripts and examples; introduce xlsx skill for spreadsheet handling (e2dfd86a) (@vinhnx)
- chore: update homebrew formula to v0.75.1 (b52a37dd)


## v0.75.1 - 2026-02-03

- refactor: reorganize release steps and update Homebrew process (e3d99f5a)


## v0.75.0 - 2026-02-03

- fix: resolve tool call ID mapping issue and update JSON handling in messages (9af9d34b) (@vinhnx)
- chore: update homebrew formula to v0.74.17 (2af3f3ff)


## v0.74.17 - 2026-02-03

- feat: add Step 3.5 Flash model and update configuration for OpenRouter (c50770ab) (@vinhnx)
- chore: update homebrew formula to v0.74.16 (a92e5a23)


## v0.74.16 - 2026-02-03

- Improve release (ff4ae644) (@vinhnx)
- chore(release): bump version to 0.74.15 [skip ci] (6a88018d) (@vinhnx)
- docs: update changelog for v0.74.15 [skip ci] (0b859919) (@vtcode-release-bot)
- Revert "refactor: enhance base URL resolution and improve JSON handling in request builder" (4cb8f2d9) (@vinhnx)
- chore: update homebrew formula to v0.74.14 (03234509)


## v0.74.15 - 2026-02-03

- Revert "refactor: enhance base URL resolution and improve JSON handling in request builder" (4cb8f2d9) (@vinhnx)
- chore: update homebrew formula to v0.74.14 (03234509)


## v0.74.14 - 2026-02-02

- chore(release): bump version to 0.74.13 [skip ci] (e49f412f) (@vinhnx)
- docs: update changelog for v0.74.13 [skip ci] (d64665d0) (@vtcode-release-bot)
- Update commit (171a2aa6) (@vinhnx)
- Refactor agent guidelines, improve spacing in TODO, and enhance model picker logic; update Anthropic provider tests and configuration (0dece6ac) (@vinhnx)
- refactor: enhance base URL resolution and improve JSON handling in request builder (584a82ca) (@vinhnx)
- docs: add behavioral guidelines to reduce common LLM coding mistakes (75ca745d) (@vinhnx)
- refactor: simplify conditional checks and remove unused imports (2602cbc8) (@vinhnx)
- refactor: update tool policies to allow all actions and improve terminal cleanup logic (0e54e2ff) (@vinhnx)
- chore: update homebrew formula to v0.74.12 (0e743b32)


## v0.74.13 - 2026-02-02

- Update commit (171a2aa6) (@vinhnx)
- Refactor agent guidelines, improve spacing in TODO, and enhance model picker logic; update Anthropic provider tests and configuration (0dece6ac) (@vinhnx)
- refactor: enhance base URL resolution and improve JSON handling in request builder (584a82ca) (@vinhnx)
- docs: add behavioral guidelines to reduce common LLM coding mistakes (75ca745d) (@vinhnx)
- refactor: simplify conditional checks and remove unused imports (2602cbc8) (@vinhnx)
- refactor: update tool policies to allow all actions and improve terminal cleanup logic (0e54e2ff) (@vinhnx)
- chore: update homebrew formula to v0.74.12 (0e743b32)


## v0.74.12 - 2026-02-02

- chore: update vtcode.gif resource (68cdf67b) (@vinhnx)
- docs: add compliance testing section and request object for Open Responses (ee2bcf14) (@vinhnx)
- chore(release): bump version to 0.74.11 [skip ci] (528d8846) (@vinhnx)
- docs: update changelog for v0.74.11 [skip ci] (dcdaef82) (@vtcode-release-bot)
- fix: update spinner finish behavior for cancellation handling (73a7d72f) (@vinhnx)
- refactor: remove unused set_defer_rendering method from StreamingReasoningState (e429daf0) (@vinhnx)
- refactor: remove deprecated model constants and clean up supported models list (eb9b6ff9) (@vinhnx)
- fix: resolve duplicate model entries and correct legacy model references (a712a191) (@vinhnx)
- Update model references to "claude-haiku-4-5" across configuration and tests (b06501f4) (@vinhnx)
- feat: add signal handling for graceful termination in TUI (f4de0101) (@vinhnx)
- Update model references from gpt-4 to gpt-5 across documentation and codebase (cec3d7c9) (@vinhnx)
- refactor: improve reasoning content comparison and suppress duplication in response rendering (87b066cf) (@vinhnx)
- chore: update default model and workspace trust settings in configuration fix: suppress duplicated content rendering in response handling refactor: clean up spacing logic in reasoning buffer add: implement streaming payload decoding helpers for OpenRouter (49a403fd) (@vinhnx)
- Fix release note (f7562c14) (@vinhnx)
- chore: update changelog header and release script title format (8f804b80) (@vinhnx)
- Duplicate badge links for Agent Skills and protocols (b9216314) (@1097578+vinhnx)
- Add star history section to README (3de92245) (@1097578+vinhnx)
- chore: update homebrew formula to v0.74.10 (c08a491c)


## v0.74.11 - 2026-02-02

- fix: update spinner finish behavior for cancellation handling (36d1f577) (@vinhnx)
- refactor: remove unused set_defer_rendering method from StreamingReasoningState (756c86ba) (@vinhnx)
- refactor: remove deprecated model constants and clean up supported models list (621b7373) (@vinhnx)
- fix: resolve duplicate model entries and correct legacy model references (7928cb44) (@vinhnx)
- Update model references to "claude-haiku-4-5" across configuration and tests (84a8d4ab) (@vinhnx)
- feat: add signal handling for graceful termination in TUI (a3ca378b) (@vinhnx)
- Update model references from gpt-4 to gpt-5 across documentation and codebase (2520e9fa) (@vinhnx)
- refactor: improve reasoning content comparison and suppress duplication in response rendering (bcce1fb1) (@vinhnx)
- chore: update default model and workspace trust settings in configuration fix: suppress duplicated content rendering in response handling refactor: clean up spacing logic in reasoning buffer add: implement streaming payload decoding helpers for OpenRouter (049663b0) (@vinhnx)
- Fix release note (d75a3f0d) (@vinhnx)
- chore: update changelog header and release script title format (744db41d) (@vinhnx)
- Add star history section to README (3de92245) (@1097578+vinhnx)
- chore: update homebrew formula to v0.74.10 (c08a491c)


## v0.74.10 - 2026-02-02

- Update commit (cbde5c0b)
- chore: update homebrew formula to v0.74.9 (6b604f22)


## v0.74.9 - 2026-02-02


### Documentation



##### [View changes on GitHub](https://github.com/vinhnx/vtcode/compare/v0.74.8...v0.74.9)

## v0.74.9 - 2026-02-02


*No significant changes*

##### [View changes on GitHub](https://github.com/vinhnx/vtcode/compare/v0.74.8...v0.74.9)

## v0.74.8 - 2026-02-02


### Refactors




- **commons**:

- **llm**:



### Documentation



##### [View changes on GitHub](https://github.com/vinhnx/vtcode/compare/v0.74.7...v0.74.8)

## v - 2026-02-02


### Refactors




- **commons**:

- **llm**:



##### [View changes on GitHub](https://github.com/vinhnx/vtcode/compare/v0.74.7...v)

## v0.74.7 - 2026-02-01

* Update commit (3edfdb95)
* fix: rename directory with colon to be Windows-compatible (f533addc)
* chore: update homebrew formula to v0.74.6 (dad20e9c)

## v0.74.6 - 2026-02-01

* Migrate LM Studio 0.4 REST API (4d12e993)
* Update release (bb930a4e)
* chore: update homebrew formula to v0.74.5 (dd7bdd0a)

## v0.74.5 - 2026-02-01

* Update CI (c9cf0a74)
* chore: update homebrew formula to v0.74.4 (1dbb0e27)

## v0.74.4 - 2026-02-01

* Fix: Skip hanging GitHub CLI refresh in build script (c77bb4aa)
* Fix: Skip hanging GitHub CLI refresh in release script (5bf61747)
* automation: add CI trigger and comprehensive release flow guide (d071618c)
* automation: add automatic gh auth switch and scope refresh (1b5f38ec)
* chore(release): bump version to 0.74.3 [skip ci] (6f365d16)
* docs: update changelog for v0.74.3 [skip ci] (d5b8110e)
* fix: skip gh auth checks in dry-run mode (8f4f8dae)
* automation: enhance release.sh with direct GitHub binary upload via gh CLI (9168134d)
* chore: add Windows cross-platform builds to release workflow (d497cf6d)
* feat: enhance AGENTS.md with new cargo commands and build performance tips (4df82383)
* feat: update imports in harness.rs and tests.rs for improved clarity (6e2b9432)
* chore: remove unnecessary blank line in run-tests.sh (c9422726)
* feat: update test commands to prefer cargo-nextest for faster execution (db00e155)
* feat: enhance Turn Diff Tracker with Agent Trace support and backward compatibility (a81074bd)
* feat: enhance Agent Trace support with async storage and serialization improvements (e777f259)
* Implement Agent Trace storage and specification for AI code attribution (d6748fc4)
* refactor: update process group management documentation for clarity (aff7157a)
* Implement process group management and graceful termination for child processes (d7aac98b)
* feat: implement wire API detection and version handling for Ollama (0cc27230)
* refactor: optimize development profile settings in Cargo.toml (cac65958)
* refactor: optimize development and test profiles in Cargo.toml (7f4cc397)
* fix: correct file path conversion in log_access method (ec7e3dc4)
* refactor: improve code formatting and readability across multiple files (d4ba430d)
* Fix badge links in README.md (e098b997)
* chore: update homebrew formula to v0.74.2 (744e7902)

## v0.74.3 - 2026-02-01

* fix: skip gh auth checks in dry-run mode (8f4f8dae)
* automation: enhance release.sh with direct GitHub binary upload via gh CLI (9168134d)
* chore: add Windows cross-platform builds to release workflow (d497cf6d)
* feat: enhance AGENTS.md with new cargo commands and build performance tips (4df82383)
* feat: update imports in harness.rs and tests.rs for improved clarity (6e2b9432)
* chore: remove unnecessary blank line in run-tests.sh (c9422726)
* feat: update test commands to prefer cargo-nextest for faster execution (db00e155)
* feat: enhance Turn Diff Tracker with Agent Trace support and backward compatibility (a81074bd)
* feat: enhance Agent Trace support with async storage and serialization improvements (e777f259)
* Implement Agent Trace storage and specification for AI code attribution (d6748fc4)
* refactor: update process group management documentation for clarity (aff7157a)
* Implement process group management and graceful termination for child processes (d7aac98b)
* feat: implement wire API detection and version handling for Ollama (0cc27230)
* refactor: optimize development profile settings in Cargo.toml (cac65958)
* refactor: optimize development and test profiles in Cargo.toml (7f4cc397)
* fix: correct file path conversion in log_access method (ec7e3dc4)
* refactor: improve code formatting and readability across multiple files (d4ba430d)
* Fix badge links in README.md (e098b997)
* chore: update homebrew formula to v0.74.2 (744e7902)

## v0.74.2 - 2026-01-31

* feat: add ACP authentication methods and configuration support (e51a5658)
* docs: update changelog for v0.74.2 [skip ci] (6d29ab5b)
* refactor: add dead code allowance for search_position and start_search method (39dc545b)
* Implement OpenRouter OAuth PKCE authentication flow and related utilities (c338e631)
* feat: add ANSI escape sequence parsing constants and improve handling in text utilities (d5641f54)
* refactor: remove unused Wrap widget import from history picker (67d5cc2c)
* feat: implement history picker for fuzzy command search (Ctrl+R) (e529e7a0)
* refactor: reorder use statements for clarity in theme module (63495bea)
* Update README.md to fix badge links (7b44cfa4)
* chore: update homebrew formula to v0.74.1 (73b0bf06)

## v0.74.2 - 2026-01-31

* refactor: add dead code allowance for search_position and start_search method (39dc545b)
* Implement OpenRouter OAuth PKCE authentication flow and related utilities (c338e631)
* feat: add ANSI escape sequence parsing constants and improve handling in text utilities (d5641f54)
* refactor: remove unused Wrap widget import from history picker (67d5cc2c)
* feat: implement history picker for fuzzy command search (Ctrl+R) (e529e7a0)
* refactor: reorder use statements for clarity in theme module (63495bea)
* Update README.md to fix badge links (7b44cfa4)
* chore: update homebrew formula to v0.74.1 (73b0bf06)

## v0.74.1 - 2026-01-31

* refactor: update terminal theme to ciapre-dark (157b61cd)
* refactor: improve formatting of model pull commands in Ollama provider documentation (252114fc)
* refactor: add Kimi K2.5 and GLM 4.7 models to Ollama provider documentation and tests (84b6c722)
* refactor: add Kimi K2.5 model support and update related configurations (6e83202d)
* refactor: improve table formatting in color guidelines documentation (d33b7afa)
* refactor: implement color accessibility features and update configuration options (6422c103)
* refactor: enhance cursor visibility logic by adding status spinner check (910506f9)
* refactor: update malloc warning suppression in debug script (839dbd68)
* refactor: improve malloc warning suppression and enhance spinner behavior in UI interactions (241c53fe)
* refactor: enhance local build process for macOS and Linux in release scripts (98cb01fb)

## v0.74.0 - 2026-01-31

* Fix permission (e2511d7e)
* docs: update AGENTS.md for improved clarity and formatting (775a0db7)
* refactor: update logging in TUI code to use tracing instead of println and eprintln (3393f2e0)
* refactor: replace eprintln with tracing for improved logging consistency (1d8f11d2)
* refactor: replace println with tracing for improved logging consistency (4ad8b586)
* fix: replace println with tracing debug for git repository check (c6c11053)
* feat: add skip_model_validation option to AnthropicConfig and update validation logic (f19fdb56)
* Refactor code for improved readability and consistency across multiple files (fec995fc)
* fix: preserve tool_exists when MCP tool check returns false (999f4a85)
* refactor: update tool execution methods and enhance context handling (e592f983)
* Refactor session loop and tool outcomes; remove unused code and improve context handling (016ef738)
* refactor: unify direct tool execution and expand interaction loop context with new tool-related services. (d7953097)
* refactor: refine `ToolOutcomeContext` lifetimes to improve mutable borrowing patterns and simplify context access. (a81254ef)
* refactor: adjust tool outcome context passing and borrowing in turn processing. (a197876a)
* refactor: centralize tool outcome handling parameters into a new `ToolOutcomeContext` struct. (73856006)
* Refactor tool call handling by centralizing execution, permission, and safety validation logic into dedicated outcome handlers and removing the execution module. (cfb8e1d7)
* refactor: extract metric recording and remove auto-exit plan mode logic from tool execution result handling. (5e83a3a2)
* Refactor tool outcome handling in the agent runloop by introducing tool-specific retry limits, centralizing repetition tracking, and enhancing context conversion. (ce765244)
* Refactor tool outcome handling by consolidating success, failure, and timeout handlers, and updating tool repetition tracking to only count successful calls. (33a9b664)
* fix: enable `unified_file` tool in the sandbox and refactor diff preview styling to use a color palette. (15715b9e)
* feat: prevent duplicate LLM reasoning output and prioritize visible alias targets for hidden tools during lookup. (c434de61)
* feat: Implement dotfile protection with audit, backup, and guardian modules, and enhance tool registry alias resolution to prioritize LLM-visible tools. (62689a2c)
* feat: Implement session loading and mode switching, refreshing available commands on mode change and using constants for mode IDs. (9004549c)
* feat: Add `switch_mode` tool and update `agent-client-protocol` dependency to 0.9.3, adapting API usage. (1bdf7a7f)
* feat: Introduce a standard Agent Client Protocol adapter and generalize ACP implementation details and tooling. (121184b5)
* fix: refine markdown styling logic for strong, heading, and inline code elements, and enhance theme-based accent application (04278e00)
* fix: update default theme, enable todo planning, refine tool output and display settings, and adjust tool policies for streamlined configuration (3308c57e)
* Update commit (db294299)

## v0.73.6 - 2026-01-30

* fix: update default theme, enable todo planning, refine tool output and display settings, and adjust tool policies for streamlined configuration (dcc7043e)
* fix: add persistence for editing and autonomous mode settings, and align theme with active configuration (ecacbc19)
* fix: add persistence for editing and autonomous mode settings, and align theme with active configuration (bfea2f52)
* fix: update default theme and reasoning effort, improve config overrides, adjust workspace trust mode, and enhance contribution docs (85c1ba7f)
* fix: add support for VTCODE_CONFIG_PATH, enhance configuration loading logic, and remove unused `.aiignore` file (bf6547aa)
* fix: improve shimmer animation handling, refactor spinner updates, and enhance status rendering logic (0a40ad46)
* fix: adjust tool policies, refactor text styling logic, and optimize message rendering indentation (8123ac34)
* fix: adjust color mappings, enhance markdown rendering logic, and add tests for new edge cases in tool policies and UI interactions (8309ffc1)
* fix: enforce tools_policy prompts, refactor workspace trust application, and enhance command safety checks (394aa59b)
* fix: enhance checksum verification logic across scripts, add fallback for individual sha256 files, and improve error handling (5abd7c60)
* fix: refactor ask command output handling, enhance pipeline detection, and centralize code extraction logic (b2aee725)
* fix: add spinner for long-running tasks, improve cursor handling, and streamline release fetching logic (484bfef1)
* fix: streamline platform-specific binary builds, refactor `ask` command implementation, and enhance local release workflow (d3bca9c2)
* chore: update homebrew formula to v0.73.5 (fd22abb9)

## v0.73.5 - 2026-01-29

* fix: improve release fetching with fallback for older versions, enhance platform-specific binary handling (5d1344a8)
* Improve deploy release (6b931dd7)

## v0.73.4 - 2026-01-29

* Fix vtcode-file-search build error (8437a13d)
* chore(release): bump version to 0.73.3 [skip ci] (48dd4c9b)
* docs: update changelog for v0.73.3 [skip ci] (d1c5e175)
* fix: streamline output handling in ask command and improve code extraction logic (8de09e16)
* chore: switch LLM provider to Ollama and update related configs, fix minor lint issues in release script (dc2e637d)
* chore: update homebrew formula to v0.73.2 (bf537280)

## v0.73.3 - 2026-01-29

* fix: streamline output handling in ask command and improve code extraction logic (8de09e16)
* chore: switch LLM provider to Ollama and update related configs, fix minor lint issues in release script (dc2e637d)
* chore: update homebrew formula to v0.73.2 (bf537280)


* fix: update GitHub release title format and improve changelog generation (97571aa2)
* chore: update homebrew formula to v0.73.1 (4ca75038)

## [Unreleased] - 2025-12-14
# [Version 0.73.1] - 2026-01-28


### Chores
    - chore: update homebrew formula to v0.73.0 and fix update script
# [Version 0.73.0] - 2026-01-28


### Features
    - feat: add GitHub Actions release workflow and update release script for better error handling


### Chores
    - chore: enhance GitHub CLI authentication checks in release scripts
# [Version 0.72.4] - 2026-01-28


### Documentation
    - docs: update changelog for v0.72.3 [skip ci]
    - docs: update changelog for v0.73.0 [skip ci]
    - docs: center align VT Code GIF in README


### Chores
    - chore: fix README paths, benchmark inclusion, and release config
    - chore(release): bump version to {{version}}
    - chore: fix Cross.toml warnings and sync vtcode.toml version
    - chore: update Cargo.toml to exclude resources directory and add VT Code GIF to README
    - chore: update npm package.json to v0.72.2 version =  [skip ci]


### Other Changes
    - Fix docker build
    - Refactor Cross.toml to consolidate Docker configuration for cross-compilation
    - Enhance Open Responses specification conformance and update documentation
    - Add technical whitepapers on security architecture and modular design principles
    - Enhance Open Responses with sequenced events and improved item serialization
    - Refactor code structure for improved readability and maintainability
    - Implement Open Responses integration and configuration options
    - Implement Open Responses specification with streaming events, output items, and response handling
# [Version 0.72.3] - 2026-01-28


### Documentation
    - docs: update changelog for v0.73.0 [skip ci]
    - docs: center align VT Code GIF in README


### Chores
    - chore: fix Cross.toml warnings and sync vtcode.toml version
    - chore: update Cargo.toml to exclude resources directory and add VT Code GIF to README
    - chore: update npm package.json to v0.72.2 version =  [skip ci]


### Other Changes
    - Refactor Cross.toml to consolidate Docker configuration for cross-compilation
    - Enhance Open Responses specification conformance and update documentation
    - Add technical whitepapers on security architecture and modular design principles
    - Enhance Open Responses with sequenced events and improved item serialization
    - Refactor code structure for improved readability and maintainability
    - Implement Open Responses integration and configuration options
    - Implement Open Responses specification with streaming events, output items, and response handling
# [Version 0.73.0] - 2026-01-28


### Documentation
    - docs: center align VT Code GIF in README


### Chores
    - chore: update Cargo.toml to exclude resources directory and add VT Code GIF to README
    - chore: update npm package.json to v0.72.2 version =  [skip ci]


### Other Changes
    - Refactor Cross.toml to consolidate Docker configuration for cross-compilation
    - Enhance Open Responses specification conformance and update documentation
    - Add technical whitepapers on security architecture and modular design principles
    - Enhance Open Responses with sequenced events and improved item serialization
    - Refactor code structure for improved readability and maintainability
    - Implement Open Responses integration and configuration options
    - Implement Open Responses specification with streaming events, output items, and response handling
# [Version 0.72.2] - 2026-01-28


### Documentation
    - docs: update changelog for v0.72.1 [skip ci]


### Chores
    - chore: update npm package.json to v0.72.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.72.0 version =  [skip ci]
# [Version 0.72.1] - 2026-01-28


### Refactors
    - refactor: enhance rendering logic for consistent tool output styling, simplify message spans, and update spinner handling for better readability
    - refactor: remove DESIGN_SYSTEM.md, update default model to minimax-m2.5:cloud, and improve TUI spinner handling with `is_spinner_frame` function
    - refactor: update authorship information across multiple crates, enhance TUI performance with increased tick rates, and integrate new tui-shimmer dependency for improved UI effects
    - refactor: add follow-up prompts for truncated outputs and improve spooled file handling messages for enhanced user guidance
    - refactor: enhance command status handling, improve loop detection logic, and update tool execution messages for clarity
    - refactor: enhance rendering logic with dimming style, standardize long-running command locks, and update tooling policies for improved usability
    - refactor: add cargo command serialization to prevent file lock contention, improve PTY tool timeout handling, and enhance error recovery logic
    - refactor: add cargo command serialization to prevent file lock contention, improve PTY tool timeout handling, and enhance error recovery logic
    - refactor: standardize color palette, update UI feedback styles, and improve markdown spacing configuration
    - refactor: improve reasoning rendering logic, add deferred rendering support, and optimize duplicate content handling
    - refactor: optimize markdown rendering with conditional line numbering, add diff language detection, and improve
    - refactor: enhance line numbering in markdown code blocks, improve text trimming logic, and add support for "Reasoning" style rendering
    - refactor: update default model to GPT-OSS, improve markdown rendering, and apply conditional/indentation optimizations
    - refactor: switch default provider to Ollama, update model and API key configurations, and apply "if-let" refactoring for cleaner conditionals


### Documentation
    - docs: update changelog for v0.72.0 [skip ci]


### Chores
    - chore: update npm package.json to v0.72.0 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.71.7 version =  [skip ci]
# [Version 0.72.0] - 2026-01-28


### Features
    - feat: add new logo assets and update existing images for branding consistency


### Refactors
    - refactor: enhance rendering logic for consistent tool output styling, simplify message spans, and update spinner handling for better readability
    - refactor: remove DESIGN_SYSTEM.md, update default model to minimax-m2.5:cloud, and improve TUI spinner handling with `is_spinner_frame` function
    - refactor: update authorship information across multiple crates, enhance TUI performance with increased tick rates, and integrate new tui-shimmer dependency for improved UI effects
    - refactor: add follow-up prompts for truncated outputs and improve spooled file handling messages for enhanced user guidance
    - refactor: enhance command status handling, improve loop detection logic, and update tool execution messages for clarity
    - refactor: enhance rendering logic with dimming style, standardize long-running command locks, and update tooling policies for improved usability
    - refactor: add cargo command serialization to prevent file lock contention, improve PTY tool timeout handling, and enhance error recovery logic
    - refactor: add cargo command serialization to prevent file lock contention, improve PTY tool timeout handling, and enhance error recovery logic
    - refactor: standardize color palette, update UI feedback styles, and improve markdown spacing configuration
    - refactor: improve reasoning rendering logic, add deferred rendering support, and optimize duplicate content handling
    - refactor: optimize markdown rendering with conditional line numbering, add diff language detection, and improve
    - refactor: enhance line numbering in markdown code blocks, improve text trimming logic, and add support for "Reasoning" style rendering
    - refactor: update default model to GPT-OSS, improve markdown rendering, and apply conditional/indentation optimizations
    - refactor: switch default provider to Ollama, update model and API key configurations, and apply "if-let" refactoring for cleaner conditionals
    - refactor: switch default provider to Hugging Face and add Moonshot Kimi K2.5 model support
    - refactor: switch default LLM provider to Anthropics, enhance reasoning deduplication, and apply Codex-inspired output limits
    - refactor: update default model and tool policy permissions, add Codex harness learnings documentation


### Documentation
    - docs: update changelog for v0.71.7 [skip ci]
    - docs: update changelog for v0.71.6 [skip ci]
    - docs: update changelog for v0.71.5 [skip ci]
    - docs: add comprehensive AgentSkills support section to README


### Chores
    - chore: update npm package.json to v0.71.7 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: remove obsolete demo files and update README to reflect changes
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.71.4 version =  [skip ci]
# [Version 0.71.7] - 2026-01-27


### Features
    - feat: add new logo assets and update existing images for branding consistency


### Refactors
    - refactor: switch default provider to Hugging Face and add Moonshot Kimi K2.5 model support
    - refactor: switch default LLM provider to Anthropics, enhance reasoning deduplication, and apply Codex-inspired output limits
    - refactor: update default model and tool policy permissions, add Codex harness learnings documentation
    - refactor: update tool policy to allow file editing and enhance output spooling for PTY-related tools


### Documentation
    - docs: update changelog for v0.71.6 [skip ci]
    - docs: update changelog for v0.71.5 [skip ci]
    - docs: add comprehensive AgentSkills support section to README
    - docs: update changelog for v0.71.4 [skip ci]


### Chores
    - chore: remove obsolete demo files and update README to reflect changes
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.71.4 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.71.3 version =  [skip ci]
# [Version 0.71.6] - 2026-01-27


### Features
    - feat: add new logo assets and update existing images for branding consistency


### Refactors
    - refactor: switch default LLM provider to Anthropics, enhance reasoning deduplication, and apply Codex-inspired output limits
    - refactor: update default model and tool policy permissions, add Codex harness learnings documentation
    - refactor: update tool policy to allow file editing and enhance output spooling for PTY-related tools


### Documentation
    - docs: update changelog for v0.71.5 [skip ci]
    - docs: add comprehensive AgentSkills support section to README
    - docs: update changelog for v0.71.4 [skip ci]


### Chores
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.71.4 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.71.3 version =  [skip ci]
# [Version 0.71.5] - 2026-01-27


### Features
    - feat: add new logo assets and update existing images for branding consistency


### Refactors
    - refactor: switch default LLM provider to Anthropics, enhance reasoning deduplication, and apply Codex-inspired output limits
    - refactor: update default model and tool policy permissions, add Codex harness learnings documentation
    - refactor: update tool policy to allow file editing and enhance output spooling for PTY-related tools


### Documentation
    - docs: add comprehensive AgentSkills support section to README
    - docs: update changelog for v0.71.4 [skip ci]


### Chores
    - chore: update npm package.json to v0.71.4 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.71.3 version =  [skip ci]
# [Version 0.71.4] - 2026-01-27


### Refactors
    - refactor: update tool policy to allow file editing and enhance output spooling for PTY-related tools


### Documentation
    - docs: update changelog for v0.71.3 [skip ci]


### Chores
    - chore: update npm package.json to v0.71.3 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.71.2 version =  [skip ci]
# [Version 0.71.3] - 2026-01-26


### Documentation
    - docs: update changelog for v0.71.2 [skip ci]


### Chores
    - chore: update npm package.json to v0.71.2 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.71.1 version =  [skip ci]
# [Version 0.71.2] - 2026-01-26


### Features
    - feat: add UI support for modal layouts and wizard states in TUI session
    - feat: add enhanced caching logic and tool execution pipeline improvements
    - feat: streamline `file_ops` by removing legacy recursive search methods and enhance OpenAI provider with streaming logic
    - feat: implement AgentRunner modularization for summarization, telemetry, tool access, and execution
    - feat: modularize tool outcome handlers into separate files (failure, success, timeout, apply) and refactor implementation for better readability and maintainability
    - feat: add `ZedAgent` implementation to support session management, tool execution, and client interaction
    - feat: integrate `MCP client` with `ToolRegistry` and add functions for tool management
    - feat: add `parse_openai_tool_calls` function to handle OpenAI tool call parsing logic
    - feat: add OpenAI provider support for chat message parsing, request building, response parsing, and streaming decoder implementations


### Bug Fixes
    - fix: remove `check_output.txt` to clean up outdated and obsolete error logs
    - fix: address unresolved imports and modules in tests across multiple components


### Refactors
    - refactor: adjust formatting, imports, and re-exports for improved consistency
    - refactor: remove `read_file_handler.rs`, `bash_runner.rs`, and unused code
    - refactor: remove `read_file_handler.rs`, `bash_runner.rs`, and unused code
    - refactor: remove `text_tools.rs` to simplify codebase and eliminate unused functions
    - refactor: remove unused LLM request structures and related configurations
    - refactor: reorder imports across modules for consistency and readability
    - refactor: reorder imports across modules for consistency and readability
    - refactor: expand visibility for `parse_terminal_command` and `run_list_files` functions to improve module accessibility
    - refactor: remove `models.rs` to simplify configuration and reduce redundancy in model management
    - refactor: remove Anthropic provider and OpenRouter implementation for codebase simplification
    - refactor: remove Anthropic provider and OpenRouter implementation for codebase simplification
    - refactor: remove obsolete `src/acp/zed.rs` file and related references to streamline the codebase
    - refactor: remove `AnthropicProvider` and related implementations from the codebase to clean up unused functionality
    - refactor: remove `OpenAIPromptCacheSettings` import from `xai.rs` to clean up unused dependencies


### Documentation
    - docs: update changelog for v0.71.1 [skip ci]
    - docs: update changelog for v0.71.0 [skip ci]


### Tests
    - test: add environment variable handling and cleanup in `test_get_gemini_api_key_from_config`


### Chores
    - chore: update npm package.json to v0.71.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.70.1 version =  [skip ci]
# [Version 0.71.1] - 2026-01-26


### Features
    - feat: add UI support for modal layouts and wizard states in TUI session
    - feat: add enhanced caching logic and tool execution pipeline improvements
    - feat: streamline `file_ops` by removing legacy recursive search methods and enhance OpenAI provider with streaming logic
    - feat: implement AgentRunner modularization for summarization, telemetry, tool access, and execution
    - feat: modularize tool outcome handlers into separate files (failure, success, timeout, apply) and refactor implementation for better readability and maintainability
    - feat: add `ZedAgent` implementation to support session management, tool execution, and client interaction
    - feat: integrate `MCP client` with `ToolRegistry` and add functions for tool management
    - feat: add `parse_openai_tool_calls` function to handle OpenAI tool call parsing logic
    - feat: add OpenAI provider support for chat message parsing, request building, response parsing, and streaming decoder implementations
    - feat: add human-readable slug generator for plan file naming, update TUI header editing mode handling
    - feat: migrate `XAIProvider` to use the new `Responses API`, improve support for tools, caching, and error handling
    - feat: migrate `XAIProvider` to use the new `Responses API`, improve support for tools, caching, and error handling


### Bug Fixes
    - fix: remove `check_output.txt` to clean up outdated and obsolete error logs
    - fix: address unresolved imports and modules in tests across multiple components


### Refactors
    - refactor: adjust formatting, imports, and re-exports for improved consistency
    - refactor: remove `read_file_handler.rs`, `bash_runner.rs`, and unused code
    - refactor: remove `read_file_handler.rs`, `bash_runner.rs`, and unused code
    - refactor: remove `text_tools.rs` to simplify codebase and eliminate unused functions
    - refactor: remove unused LLM request structures and related configurations
    - refactor: reorder imports across modules for consistency and readability
    - refactor: reorder imports across modules for consistency and readability
    - refactor: expand visibility for `parse_terminal_command` and `run_list_files` functions to improve module accessibility
    - refactor: remove `models.rs` to simplify configuration and reduce redundancy in model management
    - refactor: remove Anthropic provider and OpenRouter implementation for codebase simplification
    - refactor: remove Anthropic provider and OpenRouter implementation for codebase simplification
    - refactor: remove obsolete `src/acp/zed.rs` file and related references to streamline the codebase
    - refactor: remove `AnthropicProvider` and related implementations from the codebase to clean up unused functionality
    - refactor: remove `OpenAIPromptCacheSettings` import from `xai.rs` to clean up unused dependencies
    - refactor: optimize memory usage and runtime efficiency, improve error handling, and enhance circuit breaker logic


### Documentation
    - docs: update changelog for v0.71.0 [skip ci]
    - docs: update changelog for v0.70.1 [skip ci]


### Tests
    - test: add environment variable handling and cleanup in `test_get_gemini_api_key_from_config`


### Chores
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.70.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.70.0 version =  [skip ci]
# [Version 0.71.0] - 2026-01-26


### Features
    - feat: add UI support for modal layouts and wizard states in TUI session
    - feat: add enhanced caching logic and tool execution pipeline improvements
    - feat: streamline `file_ops` by removing legacy recursive search methods and enhance OpenAI provider with streaming logic
    - feat: implement AgentRunner modularization for summarization, telemetry, tool access, and execution
    - feat: modularize tool outcome handlers into separate files (failure, success, timeout, apply) and refactor implementation for better readability and maintainability
    - feat: add `ZedAgent` implementation to support session management, tool execution, and client interaction
    - feat: integrate `MCP client` with `ToolRegistry` and add functions for tool management
    - feat: add `parse_openai_tool_calls` function to handle OpenAI tool call parsing logic
    - feat: add OpenAI provider support for chat message parsing, request building, response parsing, and streaming decoder implementations
    - feat: add human-readable slug generator for plan file naming, update TUI header editing mode handling
    - feat: migrate `XAIProvider` to use the new `Responses API`, improve support for tools, caching, and error handling
    - feat: migrate `XAIProvider` to use the new `Responses API`, improve support for tools, caching, and error handling


### Bug Fixes
    - fix: remove `check_output.txt` to clean up outdated and obsolete error logs
    - fix: address unresolved imports and modules in tests across multiple components


### Refactors
    - refactor: adjust formatting, imports, and re-exports for improved consistency
    - refactor: remove `read_file_handler.rs`, `bash_runner.rs`, and unused code
    - refactor: remove `read_file_handler.rs`, `bash_runner.rs`, and unused code
    - refactor: remove `text_tools.rs` to simplify codebase and eliminate unused functions
    - refactor: remove unused LLM request structures and related configurations
    - refactor: reorder imports across modules for consistency and readability
    - refactor: reorder imports across modules for consistency and readability
    - refactor: expand visibility for `parse_terminal_command` and `run_list_files` functions to improve module accessibility
    - refactor: remove `models.rs` to simplify configuration and reduce redundancy in model management
    - refactor: remove Anthropic provider and OpenRouter implementation for codebase simplification
    - refactor: remove Anthropic provider and OpenRouter implementation for codebase simplification
    - refactor: remove obsolete `src/acp/zed.rs` file and related references to streamline the codebase
    - refactor: remove `AnthropicProvider` and related implementations from the codebase to clean up unused functionality
    - refactor: remove `OpenAIPromptCacheSettings` import from `xai.rs` to clean up unused dependencies
    - refactor: optimize memory usage and runtime efficiency, improve error handling, and enhance circuit breaker logic


### Documentation
    - docs: update changelog for v0.70.1 [skip ci]


### Tests
    - test: add environment variable handling and cleanup in `test_get_gemini_api_key_from_config`


### Chores
    - chore: update npm package.json to v0.70.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.70.0 version =  [skip ci]
# [Version 0.70.1] - 2026-01-25


### Features
    - feat: add human-readable slug generator for plan file naming, update TUI header editing mode handling
    - feat: migrate `XAIProvider` to use the new `Responses API`, improve support for tools, caching, and error handling
    - feat: migrate `XAIProvider` to use the new `Responses API`, improve support for tools, caching, and error handling
    - feat: introduce `InputHistoryEntry` to manage input with attachments, enhance reverse search and history navigation
    - feat: introduce `InputHistoryEntry` to manage input with attachments, enhance reverse search and history navigation
    - feat: refine tool output styling, and enhance agent configuration
    - feat: refine tool output styling, and enhance agent configuration
    - feat: add nested discovery for Claude skills, enhance SKILL.md parsing with default values, and update validation rules
    - feat: add subagent system with optional enablement, commands, and configuration updates


### Refactors
    - refactor: optimize memory usage and runtime efficiency, improve error handling, and enhance circuit breaker logic
    - refactor: remove `ui.show_message_dividers` config, simplify divider logic, and enhance tool summary rendering
    - refactor: transition `UnifiedCache` to use `RwLock` for interior mutability, enhance test coverage, and simplify cache operations
    - refactor: disable subagents by default and update documentation with usage and configuration details
    - refactor: remove unused fields and path handling from `HarnessEventEmitter` and `HarnessTurnState`


### Documentation
    - docs: update changelog for v0.70.0 [skip ci]


### Chores
    - chore: update npm package.json to v0.70.0 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.69.1 version =  [skip ci]
# [Version 0.70.0] - 2026-01-24


### Features
    - feat: introduce `InputHistoryEntry` to manage input with attachments, enhance reverse search and history navigation
    - feat: introduce `InputHistoryEntry` to manage input with attachments, enhance reverse search and history navigation
    - feat: refine tool output styling, and enhance agent configuration
    - feat: refine tool output styling, and enhance agent configuration
    - feat: add nested discovery for Claude skills, enhance SKILL.md parsing with default values, and update validation rules
    - feat: add subagent system with optional enablement, commands, and configuration updates
    - feat: add harness event emitter and session persistence for enhanced logging and state management
    - feat: add adaptive logo SVGs for different color schemes


### Bug Fixes
    - fix: update allowed tools list to include request_user_input
    - fix: enhance output spooling logic for PTY commands and handle double-serialized JSON


### Refactors
    - refactor: remove `ui.show_message_dividers` config, simplify divider logic, and enhance tool summary rendering
    - refactor: transition `UnifiedCache` to use `RwLock` for interior mutability, enhance test coverage, and simplify cache operations
    - refactor: disable subagents by default and update documentation with usage and configuration details
    - refactor: remove unused fields and path handling from `HarnessEventEmitter` and `HarnessTurnState`
    - refactor: update queue display to show follow-ups and improve styling


### Documentation
    - docs: update changelog for v0.69.1 [skip ci]
    - docs: update changelog for v0.69.0 [skip ci]
    - docs: add note to check amp in vscode session


### Chores
    - chore: update npm package.json to v0.69.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.68.2 version =  [skip ci]
# [Version 0.69.1] - 2026-01-24


### Features
    - feat: add harness event emitter and session persistence for enhanced logging and state management
    - feat: add adaptive logo SVGs for different color schemes
    - feat: implement batch file reading with token-efficient command transformation
    - feat: add support for additional programming languages including swift in syntax highlighting and configuration


### Bug Fixes
    - fix: update allowed tools list to include request_user_input
    - fix: enhance output spooling logic for PTY commands and handle double-serialized JSON


### Refactors
    - refactor: update queue display to show follow-ups and improve styling
    - refactor: enhance token-efficient output handling and command parsing in executors
    - refactor: update exit_plan_mode policy to prompt; enhance OpenResponsesProvider with version handling and reasoning content support


### Documentation
    - docs: update changelog for v0.69.0 [skip ci]
    - docs: add note to check amp in vscode session
    - docs: update changelog for v0.68.2 [skip ci]


### Chores
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.68.2 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.68.1 version =  [skip ci]
# [Version 0.69.0] - 2026-01-24


### Features
    - feat: add harness event emitter and session persistence for enhanced logging and state management
    - feat: add adaptive logo SVGs for different color schemes
    - feat: implement batch file reading with token-efficient command transformation
    - feat: add support for additional programming languages including swift in syntax highlighting and configuration


### Bug Fixes
    - fix: update allowed tools list to include request_user_input
    - fix: enhance output spooling logic for PTY commands and handle double-serialized JSON


### Refactors
    - refactor: update queue display to show follow-ups and improve styling
    - refactor: enhance token-efficient output handling and command parsing in executors
    - refactor: update exit_plan_mode policy to prompt; enhance OpenResponsesProvider with version handling and reasoning content support


### Documentation
    - docs: add note to check amp in vscode session
    - docs: update changelog for v0.68.2 [skip ci]


### Chores
    - chore: update npm package.json to v0.68.2 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.68.1 version =  [skip ci]
# [Version 0.68.2] - 2026-01-24


### Features
    - feat: implement batch file reading with token-efficient command transformation
    - feat: add support for additional programming languages including swift in syntax highlighting and configuration


### Bug Fixes
    - fix: remove unused methods is_planner_active and is_coder_active


### Refactors
    - refactor: enhance token-efficient output handling and command parsing in executors
    - refactor: update exit_plan_mode policy to prompt; enhance OpenResponsesProvider with version handling and reasoning content support
    - refactor: increase max_conversation_turns to 150 and streamline allowed_tools format; add auto_exit_plan_mode_attempted to context for improved plan mode handling
    - refactor: integrate clean_reasoning_text function to streamline reasoning text handling across multiple modules
    - refactor: rename default method to default_cache and update default implementations for various structs


### Documentation
    - docs: update changelog for v0.68.1 [skip ci]


### Chores
    - chore: update npm package.json to v0.68.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: clean up Cargo.toml and Cargo.lock by removing unused dependencies and updating package versions
    - chore: update Rust version to 1.93.0 in documentation and templates
    - chore: update npm package.json to v0.68.0 version =  [skip ci]
# [Version 0.68.1] - 2026-01-23


### Bug Fixes
    - fix: remove unused methods is_planner_active and is_coder_active


### Refactors
    - refactor: increase max_conversation_turns to 150 and streamline allowed_tools format; add auto_exit_plan_mode_attempted to context for improved plan mode handling
    - refactor: integrate clean_reasoning_text function to streamline reasoning text handling across multiple modules
    - refactor: rename default method to default_cache and update default implementations for various structs


### Documentation
    - docs: update changelog for v0.68.0 [skip ci]


### Chores
    - chore: clean up Cargo.toml and Cargo.lock by removing unused dependencies and updating package versions
    - chore: update Rust version to 1.93.0 in documentation and templates
    - chore: update npm package.json to v0.68.0 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.67.0 version =  [skip ci]
# [Version 0.68.0] - 2026-01-22


### Refactors
    - refactor: simplify codebase by auditing markdown.rs, removing unused tests, and standardizing effort parameter in tool calls
    - refactor: remove tui-syntax-highlight dependency and streamline syntax highlighting implementation
    - refactor: adjust output thresholds and preview line counts for improved token efficiency


### Documentation
    - docs: update changelog for v0.67.0 [skip ci]


### Chores
    - chore: update npm package.json to v0.67.0 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.66.8 version =  [skip ci]
# [Version 0.67.0] - 2026-01-22


### Features
    - feat: enhance CLI with quick start guidance and slash command notes


### Refactors
    - refactor: simplify codebase by auditing markdown.rs, removing unused tests, and standardizing effort parameter in tool calls
    - refactor: remove tui-syntax-highlight dependency and streamline syntax highlighting implementation
    - refactor: adjust output thresholds and preview line counts for improved token efficiency


### Documentation
    - docs: update changelog for v0.66.8 [skip ci]


### Chores
    - chore: update npm package.json to v0.66.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.66.7 version =  [skip ci]
# [Version 0.66.8] - 2026-01-22


### Features
    - feat: enhance CLI with quick start guidance and slash command notes


### Refactors
    - refactor: use AsRef trait for string conversion in command rendering


### Documentation
    - docs: update changelog for v0.66.7 [skip ci]


### Chores
    - chore: update npm package.json to v0.66.7 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.66.6 version =  [skip ci]
# [Version 0.66.7] - 2026-01-22


### Bug Fixes
    - fix: resolve ambiguous AsRef trait for Cow in zed.rs
    - fix: resolve ambiguous AsRef trait for Cow<'_, str>


### Refactors
    - refactor: use AsRef trait for string conversion in command rendering


### Documentation
    - docs: update changelog for v0.66.6 [skip ci]
    - docs: update changelog for v0.66.5 [skip ci]
    - docs: update changelog for v0.66.4 [skip ci]


### Chores
    - chore: update npm package.json to v0.66.6 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.66.3 version =  [skip ci]
# [Version 0.66.6] - 2026-01-22


### Bug Fixes
    - fix: resolve ambiguous AsRef trait for Cow in zed.rs
    - fix: resolve ambiguous AsRef trait for Cow<'_, str>


### Refactors
    - refactor: optimize string handling with dereferencing in multiple files


### Documentation
    - docs: update changelog for v0.66.5 [skip ci]
    - docs: update changelog for v0.66.4 [skip ci]
    - docs: update changelog for v0.66.3 [skip ci]


### Chores
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.66.3 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.66.2 version =  [skip ci]
# [Version 0.66.5] - 2026-01-22


### Bug Fixes
    - fix: resolve ambiguous AsRef trait for Cow in zed.rs
    - fix: resolve ambiguous AsRef trait for Cow<'_, str>


### Refactors
    - refactor: optimize string handling with dereferencing in multiple files


### Documentation
    - docs: update changelog for v0.66.4 [skip ci]
    - docs: update changelog for v0.66.3 [skip ci]


### Chores
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.66.3 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.66.2 version =  [skip ci]
# [Version 0.66.4] - 2026-01-22


### Bug Fixes
    - fix: resolve ambiguous AsRef trait for Cow<'_, str>


### Refactors
    - refactor: optimize string handling with dereferencing in multiple files


### Documentation
    - docs: update changelog for v0.66.3 [skip ci]


### Chores
    - chore: update npm package.json to v0.66.3 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.66.2 version =  [skip ci]
# [Version 0.66.3] - 2026-01-22


### Features
    - feat: add support for image URLs in @ pattern parsing and implement vision support for LLM providers


### Refactors
    - refactor: optimize string handling with dereferencing in multiple files
    - refactor: enhance log event filtering and improve user message styling in TUI
    - refactor: optimize inline event handling and improve command safety checks
    - refactor: streamline syntax highlighting by introducing a dedicated module and optimizing theme management
    - refactor: update thinking budget constants and enhance extended thinking configuration
    - refactor: implement Chain-of-Thought monitoring and context anxiety management patterns
    - refactor: improve history navigation and update input handling
    - refactor: enhance history navigation and update inline event handling
    - refactor: update test assertions for clarity and accuracy
    - refactor: update TODO list with improved queue messages UI and handling
    - refactor: update path parameter types from PathBuf to Path for consistency
    - refactor: simplify conditional checks for context awareness in prompt building


### Documentation
    - docs: update changelog for v0.66.2 [skip ci]
    - docs: update changelog for v0.67.0 [skip ci]
    - docs: update changelog for v0.67.0 [skip ci]


### Tests
    - test: add streaming event deserialization tests


### Chores
    - chore: update npm package.json to v0.66.2 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: add #[allow(dead_code)] annotations to unused items across multiple files
    - chore: update npm package.json to v0.66.1 version =  [skip ci]
# [Version 0.66.2] - 2026-01-22


### Features
    - feat: add support for image URLs in @ pattern parsing and implement vision support for LLM providers
    - feat: Add Z.AI GLM-4.7-Flash model support and update configuration
    - feat: Add extended thinking configuration for Anthropic models
    - feat: Implement Anthropic token counting, allowing estimation of input tokens via a new configurable option.


### Refactors
    - refactor: enhance log event filtering and improve user message styling in TUI
    - refactor: optimize inline event handling and improve command safety checks
    - refactor: streamline syntax highlighting by introducing a dedicated module and optimizing theme management
    - refactor: update thinking budget constants and enhance extended thinking configuration
    - refactor: implement Chain-of-Thought monitoring and context anxiety management patterns
    - refactor: improve history navigation and update input handling
    - refactor: enhance history navigation and update inline event handling
    - refactor: update test assertions for clarity and accuracy
    - refactor: update TODO list with improved queue messages UI and handling
    - refactor: update path parameter types from PathBuf to Path for consistency
    - refactor: simplify conditional checks for context awareness in prompt building


### Documentation
    - docs: update changelog for v0.67.0 [skip ci]
    - docs: update changelog for v0.67.0 [skip ci]
    - docs: update changelog for v0.66.1 [skip ci]
    - docs: update changelog for v0.66.0 [skip ci]


### Tests
    - test: add streaming event deserialization tests


### Chores
    - chore: add #[allow(dead_code)] annotations to unused items across multiple files
    - chore: update npm package.json to v0.66.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
# [Version 0.67.0] - 2026-01-20


### Features
    - feat: add support for image URLs in @ pattern parsing and implement vision support for LLM providers
    - feat: Add Z.AI GLM-4.7-Flash model support and update configuration
    - feat: Add extended thinking configuration for Anthropic models
    - feat: Implement Anthropic token counting, allowing estimation of input tokens via a new configurable option.


### Refactors
    - refactor: update thinking budget constants and enhance extended thinking configuration
    - refactor: implement Chain-of-Thought monitoring and context anxiety management patterns
    - refactor: improve history navigation and update input handling
    - refactor: enhance history navigation and update inline event handling
    - refactor: update test assertions for clarity and accuracy
    - refactor: update TODO list with improved queue messages UI and handling
    - refactor: update path parameter types from PathBuf to Path for consistency
    - refactor: simplify conditional checks for context awareness in prompt building


### Documentation
    - docs: update changelog for v0.67.0 [skip ci]
    - docs: update changelog for v0.66.1 [skip ci]
    - docs: update changelog for v0.66.0 [skip ci]


### Tests
    - test: add streaming event deserialization tests


### Chores
    - chore: add #[allow(dead_code)] annotations to unused items across multiple files
    - chore: update npm package.json to v0.66.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
# [Version 0.67.0] - 2026-01-20


### Features
    - feat: add support for image URLs in @ pattern parsing and implement vision support for LLM providers
    - feat: Add Z.AI GLM-4.7-Flash model support and update configuration
    - feat: Add extended thinking configuration for Anthropic models
    - feat: Implement Anthropic token counting, allowing estimation of input tokens via a new configurable option.


### Refactors
    - refactor: update thinking budget constants and enhance extended thinking configuration
    - refactor: implement Chain-of-Thought monitoring and context anxiety management patterns
    - refactor: improve history navigation and update input handling
    - refactor: enhance history navigation and update inline event handling
    - refactor: update test assertions for clarity and accuracy
    - refactor: update TODO list with improved queue messages UI and handling
    - refactor: update path parameter types from PathBuf to Path for consistency
    - refactor: simplify conditional checks for context awareness in prompt building


### Documentation
    - docs: update changelog for v0.66.1 [skip ci]
    - docs: update changelog for v0.66.0 [skip ci]


### Tests
    - test: add streaming event deserialization tests


### Chores
    - chore: add #[allow(dead_code)] annotations to unused items across multiple files
    - chore: update npm package.json to v0.66.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
# [Version 0.66.1] - 2026-01-19


### Features
    - feat: Add Z.AI GLM-4.7-Flash model support and update configuration
    - feat: Add extended thinking configuration for Anthropic models
    - feat: Implement Anthropic token counting, allowing estimation of input tokens via a new configurable option.


### Documentation
    - docs: update changelog for v0.66.0 [skip ci]


### Chores
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.65.5 version =  [skip ci]
# [Version 0.66.0] - 2026-01-19


### Features
    - feat: Add Z.AI GLM-4.7-Flash model support and update configuration
    - feat: Add extended thinking configuration for Anthropic models
    - feat: Implement Anthropic token counting, allowing estimation of input tokens via a new configurable option.
    - feat: add effort parameter for Claude Opus 4.5 to control token usage


### Documentation
    - docs: update changelog for v0.65.5 [skip ci]


### Chores
    - chore: update npm package.json to v0.65.5 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.65.4 version =  [skip ci]
# [Version 0.65.5] - 2026-01-19


### Features
    - feat: add effort parameter for Claude Opus 4.5 to control token usage
    - feat: implement autonomous mode with reduced HITL prompts and update related configurations
    - feat: enhance output spooling for read_file and unified_file with raw content extraction
    - feat: enhance context awareness with token usage tracking and context window size


### Bug Fixes
    - fix: prevent duplicate reasoning output during finalization


### Documentation
    - docs: update changelog for v0.65.4 [skip ci]


### Chores
    - chore: update npm package.json to v0.65.4 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.65.3 version =  [skip ci]
# [Version 0.65.4] - 2026-01-18


### Features
    - feat: implement autonomous mode with reduced HITL prompts and update related configurations
    - feat: enhance output spooling for read_file and unified_file with raw content extraction
    - feat: enhance context awareness with token usage tracking and context window size
    - feat: Add max_conversation_turns configuration to various components and update tests
    - feat: Implement autonomous loop detection with TUI warnings, integrate into agent runloop, and add project TODO documentation.
    - feat: Enhance agent robustness with exponential backoff for circuit breakers, custom tool loop limits, and conversation turn limits.
    - feat: Implement priority-based adaptive rate limiting, tiered cache eviction, and sliding window tool health tracking.
    - feat: Update session limit messages to recommend persisting progress via artifacts like task.md/docs.
    - feat: Dynamically configure conversation message and session turn limits, and remove telemetry from interaction loop parameters.
    - feat: Implement adaptive rate limiting with priority-based scaling and integrate telemetry for tool usage tracking.
    - feat: Enhance tool execution with circuit breakers, adaptive rate limiting, and health-based delegation, and introduce session telemetry and dynamic cache capacity management.
    - feat: introduce dedicated modules for MCP lifecycle, slash command handling, and tool dispatch, and parallelize tool batch execution.
    - feat: improve code block indentation normalization to handle mixed whitespace and refine markdown table rendering separators.


### Bug Fixes
    - fix: prevent duplicate reasoning output during finalization


### Refactors
    - refactor: Simplify tracing initialization with unwrap_or_default
    - refactor: Replace map_or with is_none_or for improved clarity in MCP tool filtering
    - refactor: Remove unnecessary cloning and assignment of `_updated_snapshot`.
    - refactor: Inline table row rendering logic, remove duplicate parameters, and clean up the TODO list.


### Documentation
    - docs: update changelog for v0.65.3 [skip ci]


### Chores
    - chore: update npm package.json to v0.65.3 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.65.2 version =  [skip ci]
# [Version 0.65.3] - 2026-01-18


### Features
    - feat: Add max_conversation_turns configuration to various components and update tests
    - feat: Implement autonomous loop detection with TUI warnings, integrate into agent runloop, and add project TODO documentation.
    - feat: Enhance agent robustness with exponential backoff for circuit breakers, custom tool loop limits, and conversation turn limits.
    - feat: Implement priority-based adaptive rate limiting, tiered cache eviction, and sliding window tool health tracking.
    - feat: Update session limit messages to recommend persisting progress via artifacts like task.md/docs.
    - feat: Dynamically configure conversation message and session turn limits, and remove telemetry from interaction loop parameters.
    - feat: Implement adaptive rate limiting with priority-based scaling and integrate telemetry for tool usage tracking.
    - feat: Enhance tool execution with circuit breakers, adaptive rate limiting, and health-based delegation, and introduce session telemetry and dynamic cache capacity management.
    - feat: introduce dedicated modules for MCP lifecycle, slash command handling, and tool dispatch, and parallelize tool batch execution.
    - feat: improve code block indentation normalization to handle mixed whitespace and refine markdown table rendering separators.
    - feat: Add an empirical evaluation framework for measuring LLM performance and link it in the main README.
    - feat: implement a new evaluation framework with test cases, metrics, and report generation, and update LLM provider integrations to support it.
    - feat: Introduce coding agent settings to LLM requests to refine model behavior, implementing their application in the Anthropic provider for system prompt adjustments, prefill, message reordering, and XML document handling.
    - feat: Add prefill and character reinforcement options to LLMRequest, implement Anthropic-specific handling, safety screening, and leak protection.
    - feat: Add `thinking_budget` to `LLMRequest` and implement Anthropic extended thinking logic and validation.
    - feat: Implement request and organization IDs for LLM responses and error metadata, enhance Anthropic error handling, and add `Refusal` finish reason.
    - feat: Implement support for request-specific Anthropic beta headers and update structured output model list.
    - feat: Add support for new Anthropic Claude 4 and 3.x models, enable new beta features, and refine reasoning parameter validation.


### Refactors
    - refactor: Simplify tracing initialization with unwrap_or_default
    - refactor: Replace map_or with is_none_or for improved clarity in MCP tool filtering
    - refactor: Remove unnecessary cloning and assignment of `_updated_snapshot`.
    - refactor: Inline table row rendering logic, remove duplicate parameters, and clean up the TODO list.
    - refactor: update reasoning color and style for improved readability and placeholder effect


### Documentation
    - docs: update changelog for v0.65.2 [skip ci]
    - docs: Add a new document detailing strategies for reducing Anthropic latency and link it from the Anthropic API overview.


### Chores
    - chore: update npm package.json to v0.65.2 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.65.1 version =  [skip ci]
# [Version 0.65.2] - 2026-01-18


### Features
    - feat: Add an empirical evaluation framework for measuring LLM performance and link it in the main README.
    - feat: implement a new evaluation framework with test cases, metrics, and report generation, and update LLM provider integrations to support it.
    - feat: Introduce coding agent settings to LLM requests to refine model behavior, implementing their application in the Anthropic provider for system prompt adjustments, prefill, message reordering, and XML document handling.
    - feat: Add prefill and character reinforcement options to LLMRequest, implement Anthropic-specific handling, safety screening, and leak protection.
    - feat: Add `thinking_budget` to `LLMRequest` and implement Anthropic extended thinking logic and validation.
    - feat: Implement request and organization IDs for LLM responses and error metadata, enhance Anthropic error handling, and add `Refusal` finish reason.
    - feat: Implement support for request-specific Anthropic beta headers and update structured output model list.
    - feat: Add support for new Anthropic Claude 4 and 3.x models, enable new beta features, and refine reasoning parameter validation.
    - feat: add tool search configuration and integration for Anthropic provider


### Refactors
    - refactor: update reasoning color and style for improved readability and placeholder effect


### Documentation
    - docs: Add a new document detailing strategies for reducing Anthropic latency and link it from the Anthropic API overview.
    - docs: update changelog for v0.65.1 [skip ci]
    - docs: update changelog for v0.65.1 [skip ci]


### Chores
    - chore: update npm package.json to v0.65.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.65.0 version =  [skip ci]
# [Version 0.65.1] - 2026-01-17


### Features
    - feat: add tool search configuration and integration for Anthropic provider
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.65.1 [skip ci]
    - docs: update changelog for v0.65.0 [skip ci]
    - docs: update changelog for v0.64.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.9 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]


### Chores
    - chore: update npm package.json to v0.65.0 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update Cargo.lock
    - chore: update remaining crate versions to 0.62.0
    - chore: bump version to 0.62.0
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
# [Version 0.65.1] - 2026-01-17


### Features
    - feat: add tool search configuration and integration for Anthropic provider
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.65.0 [skip ci]
    - docs: update changelog for v0.64.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.9 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]


### Chores
    - chore: update npm package.json to v0.65.0 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update Cargo.lock
    - chore: update remaining crate versions to 0.62.0
    - chore: bump version to 0.62.0
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
# [Version 0.65.0] - 2026-01-17


### Features
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.64.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.9 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.8 [skip ci]


### Chores
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update Cargo.lock
    - chore: update remaining crate versions to 0.62.0
    - chore: bump version to 0.62.0
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
# [Version 0.64.0] - 2026-01-17


### Features
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.9 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.8 [skip ci]


### Chores
    - chore(release): bump version to {{version}}
    - chore: update Cargo.lock
    - chore: update remaining crate versions to 0.62.0
    - chore: bump version to 0.62.0
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
# [Version 0.63.0] - 2026-01-17


### Features
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.9 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.8 [skip ci]


### Chores
    - chore: update Cargo.lock
    - chore: update remaining crate versions to 0.62.0
    - chore: bump version to 0.62.0
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
# [Version 0.63.0] - 2026-01-17


### Features
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.63.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.9 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.8 [skip ci]


### Chores
    - chore: update remaining crate versions to 0.62.0
    - chore: bump version to 0.62.0
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
# [Version 0.63.0] - 2026-01-17


### Features
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.9 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.8 [skip ci]


### Chores
    - chore: bump version to 0.62.0
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
# [Version 0.62.0] - 2026-01-17


### Features
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.9 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.8 [skip ci]


### Chores
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
# [Version 0.61.0] - 2026-01-17


### Features
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.60.9 [skip ci]
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.8 [skip ci]


### Chores
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
# [Version 0.60.9] - 2026-01-17


### Features
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.61.0 [skip ci]
    - docs: update changelog for v0.60.8 [skip ci]


### Chores
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
# [Version 0.61.0] - 2026-01-17


### Features
    - feat: Refactor API response handling and file search parameters, add conditional Anthropic API compilation, and enable schema generation for core types.
    - feat: add Anthropic API compatibility server and documentation


### Documentation
    - docs: update changelog for v0.60.8 [skip ci]


### Chores
    - chore: update npm package.json to v0.60.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
# [Version 0.60.8] - 2026-01-17


### Documentation
    - docs: update changelog for v0.60.7 [skip ci]


### Chores
    - chore: update logo image to higher resolution
    - chore: update README layout and add new logo assets
    - chore: update npm package.json to v0.60.7 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: enable contributors in changelog configuration
    - chore: update npm package.json to v0.60.6 version =  [skip ci]
# [Version 0.60.7] - 2026-01-17


### Features
    - feat: add debug logging for subagent parsing and loading; update error messages for context and segment not found


### Refactors
    - refactor: remove unused LLM provider implementations


### Documentation
    - docs: update changelog for v0.60.6 [skip ci]


### Chores
    - chore: enable contributors in changelog configuration
    - chore: update npm package.json to v0.60.6 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.60.5 version =  [skip ci]
# [Version 0.60.6] - 2026-01-17


### Features
    - feat: add debug logging for subagent parsing and loading; update error messages for context and segment not found
    - feat: update tool policies, enhance file handling, and modify agent configuration for improved functionality


### Refactors
    - refactor: remove unused LLM provider implementations


### Documentation
    - docs: update changelog for v0.60.5 [skip ci]


### Chores
    - chore: update npm package.json to v0.60.5 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.60.4 version =  [skip ci]
# [Version 0.60.5] - 2026-01-16


### Features
    - feat: update tool policies, enhance file handling, and modify agent configuration for improved functionality
    - feat: update tool policies and agent configuration for improved execution control and user confirmation
    - feat: enhance agent behavior configuration with Codex-inspired patterns and update tool response truncation settings
    - feat: add GPT-5.2 Codex model and improve code formatting across multiple files


### Documentation
    - docs: update changelog for v0.60.4 [skip ci]


### Chores
    - chore: update npm package.json to v0.60.4 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.60.3 version =  [skip ci]
# [Version 0.60.4] - 2026-01-16


### Features
    - feat: update tool policies and agent configuration for improved execution control and user confirmation
    - feat: enhance agent behavior configuration with Codex-inspired patterns and update tool response truncation settings
    - feat: add GPT-5.2 Codex model and improve code formatting across multiple files
    - feat: enhance sandboxing with new documentation and environment handling


### Refactors
    - refactor: streamline code by simplifying conditional checks and improving output handling
    - refactor: clean up code formatting and improve readability in multiple files


### Documentation
    - docs: update changelog for v0.60.3 [skip ci]


### Chores
    - chore: update npm package.json to v0.60.3 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.60.2 version =  [skip ci]
# [Version 0.60.3] - 2026-01-14


### Features
    - feat: enhance sandboxing with new documentation and environment handling
    - feat: update tool policies and enhance session limit handling for tool loops


### Refactors
    - refactor: streamline code by simplifying conditional checks and improving output handling
    - refactor: clean up code formatting and improve readability in multiple files
    - refactor: update tool policies to allow write_file and unified_file actions; remove redundant error logging


### Documentation
    - docs: update changelog for v0.60.2 [skip ci]


### Chores
    - chore: update npm package.json to v0.60.2 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update .gitignore and tool policies; change provider and API key in vtcode.toml
    - chore: update npm package.json to v0.60.1 version =  [skip ci]
# [Version 0.60.2] - 2026-01-10


### Features
    - feat: update tool policies and enhance session limit handling for tool loops
    - feat: implement plan mode tools for managing planning workflow and enhance code block indentation normalization
    - feat: implement session limit increase prompt and safety validation enhancements


### Refactors
    - refactor: update tool policies to allow write_file and unified_file actions; remove redundant error logging


### Documentation
    - docs: update changelog for v0.60.1 [skip ci]


### Chores
    - chore: update .gitignore and tool policies; change provider and API key in vtcode.toml
    - chore: update npm package.json to v0.60.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.60.0 version =  [skip ci]
# [Version 0.60.1] - 2026-01-10


### Features
    - feat: implement plan mode tools for managing planning workflow and enhance code block indentation normalization
    - feat: implement session limit increase prompt and safety validation enhancements
    - feat: update tool policies to allow apply_patch, unified_exec, and unified_file actions
    - feat: add editing modes and commands for toggling between Edit, Plan, and Agent modes
    - feat: implement Plan Mode for read-only exploration and planning


### Documentation
    - docs: update changelog for v0.60.0 [skip ci]


### Chores
    - chore: update npm package.json to v0.60.0 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update tool policies and remove unused dependencies
    - chore: update npm package.json to v0.59.2 version =  [skip ci]
# [Version 0.60.0] - 2026-01-10


### Features
    - feat: update tool policies to allow apply_patch, unified_exec, and unified_file actions
    - feat: add editing modes and commands for toggling between Edit, Plan, and Agent modes
    - feat: implement Plan Mode for read-only exploration and planning
    - feat: implement sandboxing configuration and policies
    - feat: enhance dynamic context discovery and update configuration
    - feat: implement dynamic context discovery with file spooling for large outputs


### Documentation
    - docs: update changelog for v0.59.2 [skip ci]


### Chores
    - chore: update tool policies and remove unused dependencies
    - chore: update npm package.json to v0.59.2 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.59.1 version =  [skip ci]
# [Version 0.59.2] - 2026-01-08


### Features
    - feat: implement sandboxing configuration and policies
    - feat: enhance dynamic context discovery and update configuration
    - feat: implement dynamic context discovery with file spooling for large outputs


### Refactors
    - refactor: update tool policies, enhance subagent cleanup, and improve documentation


### Documentation
    - docs: update changelog for v0.59.1 [skip ci]


### Chores
    - chore: update npm package.json to v0.59.1 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.59.0 version =  [skip ci]
# [Version 0.59.1] - 2026-01-07


### Refactors
    - refactor: update tool policies, enhance subagent cleanup, and improve documentation
    - refactor: clean up code formatting and improve readability across multiple files


### Documentation
    - docs: update changelog for v0.59.0 [skip ci]
    - docs: update changelog for v0.58.26 [skip ci]
    - docs: update ACP V2 Migration Guide for improved clarity and formatting


### Chores
    - chore: update npm package.json to v0.59.0 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.25 version =  [skip ci]
# [Version 0.59.0] - 2026-01-06


### Features
    - feat: add LRU cache for canonicalized paths and optimize vector allocations
    - feat: restore Kitty keyboard protocol support and update session handling


### Refactors
    - refactor: clean up code formatting and improve readability across multiple files


### Documentation
    - docs: update changelog for v0.58.26 [skip ci]
    - docs: update ACP V2 Migration Guide for improved clarity and formatting
    - docs: update changelog for v0.58.25 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.25 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.24 version =  [skip ci]
# [Version 0.58.26] - 2026-01-06


### Features
    - feat: add LRU cache for canonicalized paths and optimize vector allocations
    - feat: restore Kitty keyboard protocol support and update session handling


### Refactors
    - refactor: clean up code formatting and improve readability across multiple files


### Documentation
    - docs: update ACP V2 Migration Guide for improved clarity and formatting
    - docs: update changelog for v0.58.25 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.25 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.24 version =  [skip ci]
# [Version 0.58.25] - 2026-01-06


### Features
    - feat: add LRU cache for canonicalized paths and optimize vector allocations
    - feat: restore Kitty keyboard protocol support and update session handling


### Documentation
    - docs: update changelog for v0.58.24 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.24 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.23 version =  [skip ci]
# [Version 0.58.24] - 2026-01-05


### Documentation
    - docs: update changelog for v0.58.23 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.23 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.22 version =  [skip ci]
# [Version 0.58.23] - 2026-01-05


### Documentation
    - docs: update changelog for v0.58.22 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.22 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.21 version =  [skip ci]
# [Version 0.58.22] - 2026-01-04


### Documentation
    - docs: update changelog for v0.58.21 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.21 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.20 version =  [skip ci]
# [Version 0.58.21] - 2026-01-04


### Bug Fixes
    - fix: suppress dead code warnings for unused UI and agent functions


### Documentation
    - docs: update changelog for v0.58.20 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.20 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.19 version =  [skip ci]
# [Version 0.58.20] - 2026-01-04


### Bug Fixes
    - fix: suppress dead code warnings for unused UI and agent functions
    - fix: prefix unused variable with underscore in config_watcher


### Documentation
    - docs: update changelog for v0.58.19 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.19 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.18 version =  [skip ci]
# [Version 0.58.19] - 2026-01-04


### Features
    - feat(core): enhance tool caching and UI redraw optimization


### Bug Fixes
    - fix: prefix unused variable with underscore in config_watcher
    - fix: remove dead code and fix compilation errors


### Documentation
    - docs: update changelog for v0.58.18 [skip ci]
    - docs: update changelog for v0.58.17 [skip ci]
    - docs: update changelog for v0.58.16 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.18 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.15 version =  [skip ci]
# [Version 0.58.18] - 2026-01-04


### Features
    - feat(core): enhance tool caching and UI redraw optimization
    - feat(core): add file system watcher and performance optimization infrastructure
    - feat(core): integrate real performance optimizations into tool registry


### Bug Fixes
    - fix: remove dead code and fix compilation errors


### Documentation
    - docs: update changelog for v0.58.17 [skip ci]
    - docs: update changelog for v0.58.16 [skip ci]
    - docs: update changelog for v0.58.15 [skip ci]
    - docs: reorganize documentation and integrate skill tools into registry


### Chores
    - chore(release): bump version to {{version}}
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.15 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.14 version =  [skip ci]
# [Version 0.58.17] - 2026-01-04


### Features
    - feat(core): enhance tool caching and UI redraw optimization
    - feat(core): add file system watcher and performance optimization infrastructure
    - feat(core): integrate real performance optimizations into tool registry


### Documentation
    - docs: update changelog for v0.58.16 [skip ci]
    - docs: update changelog for v0.58.15 [skip ci]
    - docs: reorganize documentation and integrate skill tools into registry


### Chores
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.15 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.14 version =  [skip ci]
# [Version 0.58.16] - 2026-01-04


### Features
    - feat(core): enhance tool caching and UI redraw optimization
    - feat(core): add file system watcher and performance optimization infrastructure
    - feat(core): integrate real performance optimizations into tool registry


### Documentation
    - docs: update changelog for v0.58.15 [skip ci]
    - docs: reorganize documentation and integrate skill tools into registry


### Chores
    - chore: update npm package.json to v0.58.15 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.14 version =  [skip ci]
# [Version 0.58.15] - 2026-01-04


### Features
    - feat(core): add file system watcher and performance optimization infrastructure
    - feat(core): integrate real performance optimizations into tool registry


### Documentation
    - docs: reorganize documentation and integrate skill tools into registry
    - docs: update changelog for v0.58.14 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.14 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: remove unused GeminiPromptCacheMode import.
    - chore: update npm package.json to v0.58.13 version =  [skip ci]
# [Version 0.58.14] - 2026-01-03


### Features
    - feat: enhance LLM provider initialization with client injection and refine prompt caching
    - feat: Refactor tool registry to use MCP tool index cache and update mutability of inventory and tool policy access.
    - feat: Implement parallel tool execution for agent actions and update tool registry operations to be asynchronous.
    - feat: Refactor tool permission context and enhance command safety validation with new progress updates for tool execution.


### Refactors
    - refactor: simplify nested conditional logic with chained `&& let` patterns
    - refactor: update symbol name extraction to use `ChildByField` and add Rust language test.
    - refactor: Extract progress update guard and elapsed time updater to `progress.rs` and add `PlaceholderSpinner::force_refresh`.


### Documentation
    - docs: update changelog for v0.58.13 [skip ci]


### Style Changes
    - style: apply consistent formatting and whitespace adjustments


### Chores
    - chore: remove unused GeminiPromptCacheMode import.
    - chore: update npm package.json to v0.58.13 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.12 version =  [skip ci]
# [Version 0.58.13] - 2026-01-03


### Features
    - feat: enhance LLM provider initialization with client injection and refine prompt caching
    - feat: Refactor tool registry to use MCP tool index cache and update mutability of inventory and tool policy access.
    - feat: Implement parallel tool execution for agent actions and update tool registry operations to be asynchronous.
    - feat: Refactor tool permission context and enhance command safety validation with new progress updates for tool execution.


### Bug Fixes
    - fix: resolve Windows build errors in vtcode-core


### Refactors
    - refactor: simplify nested conditional logic with chained `&& let` patterns
    - refactor: update symbol name extraction to use `ChildByField` and add Rust language test.
    - refactor: Extract progress update guard and elapsed time updater to `progress.rs` and add `PlaceholderSpinner::force_refresh`.


### Documentation
    - docs: update changelog for v0.58.12 [skip ci]
    - docs: add Windows build fixes documentation


### Style Changes
    - style: apply consistent formatting and whitespace adjustments


### Chores
    - chore: update npm package.json to v0.58.12 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.11 version =  [skip ci]
# [Version 0.58.12] - 2026-01-02


### Bug Fixes
    - fix: resolve Windows build errors in vtcode-core
    - fix: suppress dead_code warnings for planned/stub functions


### Documentation
    - docs: add Windows build fixes documentation
    - docs: update changelog for v0.58.11 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.11 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.10 version =  [skip ci]
# [Version 0.58.11] - 2026-01-02


### Bug Fixes
    - fix: suppress dead_code warnings for planned/stub functions
    - fix: build-release workflow now triggers on tag push events


### Documentation
    - docs: update changelog for v0.58.10 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.10 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.9 version =  [skip ci]
# [Version 0.58.10] - 2026-01-02


### Bug Fixes
    - fix: build-release workflow now triggers on tag push events


### Documentation
    - docs: update changelog for v0.58.9 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.9 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.8 version =  [skip ci]
# [Version 0.58.9] - 2026-01-02


### Features
    - feat: auto-trigger build-release workflow on GitHub release creation


### Documentation
    - docs: update changelog for v0.58.8 [skip ci]


### Chores
    - chore: update npm package.json to v0.58.8 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.7 version =  [skip ci]
# [Version 0.58.8] - 2026-01-02


### Features
    - feat: auto-trigger build-release workflow on GitHub release creation


### Bug Fixes
    - fix: suppress unused_imports warning in openai.rs for CI compatibility
    - fix: install OpenSSL dependencies for Linux builds in CI
    - fix: conditionally import debug-only items to fix release build


### Documentation
    - docs: update changelog for v0.58.7 [skip ci]
    - docs: add instructions for manually triggering release build
    - docs: add quick reference for monitoring and auto-install
    - docs: add release monitoring guide with auto-install instructions
    - docs: add native installer readme - central documentation hub
    - docs: deployment complete - v0.58.6 release ready
    - docs: add release v0.58.6 and installer test guide


### Chores
    - chore: update npm package.json to v0.58.7 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.6 version =  [skip ci]
# [Version 0.58.7] - 2026-01-02


### Bug Fixes
    - fix: suppress unused_imports warning in openai.rs for CI compatibility
    - fix: install OpenSSL dependencies for Linux builds in CI
    - fix: conditionally import debug-only items to fix release build
    - fix: ensure get_download_url outputs only URL to stdout
    - fix: redirect all logging to stderr in installer script


### Documentation
    - docs: add instructions for manually triggering release build
    - docs: add quick reference for monitoring and auto-install
    - docs: add release monitoring guide with auto-install instructions
    - docs: add native installer readme - central documentation hub
    - docs: deployment complete - v0.58.6 release ready
    - docs: add release v0.58.6 and installer test guide
    - docs: update changelog for v0.58.6 [skip ci]
    - docs: add native installer implementation status report


### Chores
    - chore: update npm package.json to v0.58.6 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: update npm package.json to v0.58.5 version =  [skip ci]
# [Version 0.58.6] - 2026-01-02


### Features
    - feat: add native installer with auto-updater module


### Bug Fixes
    - fix: ensure get_download_url outputs only URL to stdout
    - fix: redirect all logging to stderr in installer script
    - fix: correct Python variable substitution in Homebrew workflow
    - fix: improve release.toml commit message template for consistency


### Documentation
    - docs: add native installer implementation status report
    - docs: update changelog for v0.58.5 [skip ci]
    - docs: add .nojekyll to bypass Jekyll processing
    - docs: remove HTML index, use Jekyll markdown
    - docs: add HTML landing page for GitHub Pages
    - docs: add Jekyll config and documentation index
    - docs: update Homebrew documentation - simplified architecture
    - docs: add Homebrew verification checklist - release automation complete
    - docs: add actionable next steps for completing homebrew distribution setup
    - docs: add comprehensive homebrew fix summary with all solutions applied
    - docs: explain why homebrew updates stopped and root cause analysis
    - docs: add guide for setting up custom homebrew tap repository


### Chores
    - chore: update npm package.json to v0.58.5 version =  [skip ci]
    - chore(release): bump version to {{version}}
    - chore: remove Pages workflow - using simpler direct branch deployment
    - chore: remove redundant release-on-tag.yml workflow
    - chore: update npm package.json to v0.58.4 version =  [skip ci]
# [Version 0.58.5] - 2026-01-02


### Features
    - feat: add native installer with auto-updater module


### Bug Fixes
    - fix: correct Python variable substitution in Homebrew workflow
    - fix: improve release.toml commit message template for consistency
    - fix: resolve python string interpolation in homebrew formula updates
    - fix: improve homebrew formula regex patterns for reliable checksum updates
    - fix: homebrew release automation and YAML workflow indentation


### Documentation
    - docs: add .nojekyll to bypass Jekyll processing
    - docs: remove HTML index, use Jekyll markdown
    - docs: add HTML landing page for GitHub Pages
    - docs: add Jekyll config and documentation index
    - docs: update Homebrew documentation - simplified architecture
    - docs: add Homebrew verification checklist - release automation complete
    - docs: add actionable next steps for completing homebrew distribution setup
    - docs: add comprehensive homebrew fix summary with all solutions applied
    - docs: explain why homebrew updates stopped and root cause analysis
    - docs: add guide for setting up custom homebrew tap repository
    - docs: update changelog for v0.58.4 [skip ci]
    - docs: add verification summary for homebrew release fixes


### Chores
    - chore: remove Pages workflow - using simpler direct branch deployment
    - chore: remove redundant release-on-tag.yml workflow
    - chore: update npm package.json to v0.58.4 version =  [skip ci]
    - chore: release v{{version}}
    - chore: update npm package.json to v0.58.3 version =  [skip ci]
# [Version 0.58.4] - 2026-01-02


### Features
    - feat(runner): add keyboard protocol environment check and enhance logging
    - feat(tools): Add move and copy file operations with error handling
    - feat(command-safety): Enhance command safety module with comprehensive integration and documentation
    - feat(prompts): Revise system prompts for clarity, autonomy, and Codex alignment
    - feat(dependencies): Update Cargo.lock and Cargo.toml with new packages and versions
    - feat(ollama-integration): Implement comprehensive Ollama client and progress reporting system
    - feat(process-hardening): Introduce vtcode-process-hardening crate for enhanced security measures
    - feat(file-search): Implement vtcode-file-search crate and integration with extensions
    - feat: Unify system prompt instruction generation and skill rendering via `get_user_instructions` and `AgentConfig`.
    - feat: Improve context manager performance with incremental stats and bolster tool security with path and command validation.
    - feat: Implement the Desire Paths philosophy by updating agent prompts and documentation to improve agent UX.


### Bug Fixes
    - fix: resolve python string interpolation in homebrew formula updates
    - fix: improve homebrew formula regex patterns for reliable checksum updates
    - fix: homebrew release automation and YAML workflow indentation
    - fix: update dependencies and improve error handling in terminal functions
    - fix(models): Correct ClaudeOpus41 version and update related assertions refactor(loader): Adjust config loading order and clarify comments feat(output-styles): Add kebab-case renaming for OutputStyleFileConfig


### Refactors
    - refactor(config): Remove ConfigOptimizer and implement ConfigBuilder for streamlined configuration management


### Documentation
    - docs: add verification summary for homebrew release fixes
    - docs: update changelog for v0.58.3 [skip ci]
    - docs: update changelog for v0.58.2 [skip ci]
    - docs: update changelog for v0.58.1 [skip ci]
    - docs: update changelog for v0.58.0 [skip ci]
    - docs: update changelog for v0.57.0 [skip ci]
    - docs: update changelog for v0.56.0 [skip ci]
    - docs: Add Phase 3 extension integration planning and file search documentation
    - docs: Update implementation summary and configuration for file search and system prompt enhancements


### Chores
    - chore: update npm package.json to v0.58.3 version =  [skip ci]
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: fix release metadata for vtcode-file-search and vtcode-process-hardening, add version constraint
    - chore: add missing metadata to vtcode-file-search and vtcode-process-hardening
    - chore(deps): bump the all-rust-deps group with 14 updates
# [Version 0.58.3] - 2026-01-02


### Features
    - feat(runner): add keyboard protocol environment check and enhance logging
    - feat(tools): Add move and copy file operations with error handling
    - feat(command-safety): Enhance command safety module with comprehensive integration and documentation
    - feat(prompts): Revise system prompts for clarity, autonomy, and Codex alignment
    - feat(dependencies): Update Cargo.lock and Cargo.toml with new packages and versions
    - feat(ollama-integration): Implement comprehensive Ollama client and progress reporting system
    - feat(process-hardening): Introduce vtcode-process-hardening crate for enhanced security measures
    - feat(file-search): Implement vtcode-file-search crate and integration with extensions
    - feat: Unify system prompt instruction generation and skill rendering via `get_user_instructions` and `AgentConfig`.
    - feat: Improve context manager performance with incremental stats and bolster tool security with path and command validation.
    - feat: Implement the Desire Paths philosophy by updating agent prompts and documentation to improve agent UX.


### Bug Fixes
    - fix: update dependencies and improve error handling in terminal functions
    - fix(models): Correct ClaudeOpus41 version and update related assertions refactor(loader): Adjust config loading order and clarify comments feat(output-styles): Add kebab-case renaming for OutputStyleFileConfig


### Refactors
    - refactor(config): Remove ConfigOptimizer and implement ConfigBuilder for streamlined configuration management


### Documentation
    - docs: update changelog for v0.58.2 [skip ci]
    - docs: update changelog for v0.58.1 [skip ci]
    - docs: update changelog for v0.58.0 [skip ci]
    - docs: update changelog for v0.57.0 [skip ci]
    - docs: update changelog for v0.56.0 [skip ci]
    - docs: Add Phase 3 extension integration planning and file search documentation
    - docs: Update implementation summary and configuration for file search and system prompt enhancements


### Chores
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: fix release metadata for vtcode-file-search and vtcode-process-hardening, add version constraint
    - chore: add missing metadata to vtcode-file-search and vtcode-process-hardening
    - chore(deps): bump the all-rust-deps group with 14 updates
    - chore: update npm package.json to v0.55.1 [skip ci]
# [Version 0.58.2] - 2026-01-02


### Features
    - feat(runner): add keyboard protocol environment check and enhance logging
    - feat(tools): Add move and copy file operations with error handling
    - feat(command-safety): Enhance command safety module with comprehensive integration and documentation
    - feat(prompts): Revise system prompts for clarity, autonomy, and Codex alignment
    - feat(dependencies): Update Cargo.lock and Cargo.toml with new packages and versions
    - feat(ollama-integration): Implement comprehensive Ollama client and progress reporting system
    - feat(process-hardening): Introduce vtcode-process-hardening crate for enhanced security measures
    - feat(file-search): Implement vtcode-file-search crate and integration with extensions
    - feat: Unify system prompt instruction generation and skill rendering via `get_user_instructions` and `AgentConfig`.
    - feat: Improve context manager performance with incremental stats and bolster tool security with path and command validation.
    - feat: Implement the Desire Paths philosophy by updating agent prompts and documentation to improve agent UX.


### Bug Fixes
    - fix(models): Correct ClaudeOpus41 version and update related assertions refactor(loader): Adjust config loading order and clarify comments feat(output-styles): Add kebab-case renaming for OutputStyleFileConfig


### Refactors
    - refactor(config): Remove ConfigOptimizer and implement ConfigBuilder for streamlined configuration management


### Documentation
    - docs: update changelog for v0.58.1 [skip ci]
    - docs: update changelog for v0.58.0 [skip ci]
    - docs: update changelog for v0.57.0 [skip ci]
    - docs: update changelog for v0.56.0 [skip ci]
    - docs: Add Phase 3 extension integration planning and file search documentation
    - docs: Update implementation summary and configuration for file search and system prompt enhancements


### Chores
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: fix release metadata for vtcode-file-search and vtcode-process-hardening, add version constraint
    - chore: add missing metadata to vtcode-file-search and vtcode-process-hardening
    - chore(deps): bump the all-rust-deps group with 14 updates
    - chore: update npm package.json to v0.55.1 [skip ci]
# [Version 0.58.1] - 2026-01-02


### Features
    - feat(runner): add keyboard protocol environment check and enhance logging
    - feat(tools): Add move and copy file operations with error handling
    - feat(command-safety): Enhance command safety module with comprehensive integration and documentation
    - feat(prompts): Revise system prompts for clarity, autonomy, and Codex alignment
    - feat(dependencies): Update Cargo.lock and Cargo.toml with new packages and versions
    - feat(ollama-integration): Implement comprehensive Ollama client and progress reporting system
    - feat(process-hardening): Introduce vtcode-process-hardening crate for enhanced security measures
    - feat(file-search): Implement vtcode-file-search crate and integration with extensions
    - feat: Unify system prompt instruction generation and skill rendering via `get_user_instructions` and `AgentConfig`.
    - feat: Improve context manager performance with incremental stats and bolster tool security with path and command validation.
    - feat: Implement the Desire Paths philosophy by updating agent prompts and documentation to improve agent UX.
    - feat: add tokio dependency and improve error handling in background task execution
    - feat(subagents): document subagent system and built-in agents; update README and changelog
    - feat(statusline): add custom status line scripts and JSON input handling
    - feat: add reverse search and background operation handling in TUI
    - feat(output-styles): implement output styles feature with customizable behavior and response formats
    - feat(hooks): add lifecycle hooks for file protection, command logging, code formatting, markdown formatting, and notifications
    - feat(marketplace): add marketplace and plugin management commands


### Bug Fixes
    - fix(models): Correct ClaudeOpus41 version and update related assertions refactor(loader): Adjust config loading order and clarify comments feat(output-styles): Add kebab-case renaming for OutputStyleFileConfig


### Refactors
    - refactor(config): Remove ConfigOptimizer and implement ConfigBuilder for streamlined configuration management
    - refactor: improve Linux checksum handling and release verification in scripts


### Documentation
    - docs: update changelog for v0.58.0 [skip ci]
    - docs: update changelog for v0.57.0 [skip ci]
    - docs: update changelog for v0.56.0 [skip ci]
    - docs: Add Phase 3 extension integration planning and file search documentation
    - docs: Update implementation summary and configuration for file search and system prompt enhancements
    - docs: update changelog for v0.55.1 [skip ci]
    - docs: update changelog for v0.55.0 [skip ci]
    - docs: update changelog for v0.54.4 [skip ci]


### Chores
    - chore: release v{{version}}
    - chore: release v{{version}}
    - chore: fix release metadata for vtcode-file-search and vtcode-process-hardening, add version constraint
    - chore: add missing metadata to vtcode-file-search and vtcode-process-hardening
    - chore: update npm package.json to v0.55.1 [skip ci]
    - chore: release v0.55.1
    - chore: release v0.55.0
    - chore: release v0.54.4
    - chore: update npm package.json to v0.54.3 [skip ci]
# [Version 0.58.0] - 2026-01-02


### Features
    - feat(runner): add keyboard protocol environment check and enhance logging
    - feat(tools): Add move and copy file operations with error handling
    - feat(command-safety): Enhance command safety module with comprehensive integration and documentation
    - feat(prompts): Revise system prompts for clarity, autonomy, and Codex alignment
    - feat(dependencies): Update Cargo.lock and Cargo.toml with new packages and versions
    - feat(ollama-integration): Implement comprehensive Ollama client and progress reporting system
    - feat(process-hardening): Introduce vtcode-process-hardening crate for enhanced security measures
    - feat(file-search): Implement vtcode-file-search crate and integration with extensions
    - feat: Unify system prompt instruction generation and skill rendering via `get_user_instructions` and `AgentConfig`.
    - feat: Improve context manager performance with incremental stats and bolster tool security with path and command validation.
    - feat: Implement the Desire Paths philosophy by updating agent prompts and documentation to improve agent UX.
    - feat: add tokio dependency and improve error handling in background task execution
    - feat(subagents): document subagent system and built-in agents; update README and changelog
    - feat(statusline): add custom status line scripts and JSON input handling
    - feat: add reverse search and background operation handling in TUI
    - feat(output-styles): implement output styles feature with customizable behavior and response formats
    - feat(hooks): add lifecycle hooks for file protection, command logging, code formatting, markdown formatting, and notifications
    - feat(marketplace): add marketplace and plugin management commands


### Bug Fixes
    - fix(models): Correct ClaudeOpus41 version and update related assertions refactor(loader): Adjust config loading order and clarify comments feat(output-styles): Add kebab-case renaming for OutputStyleFileConfig


### Refactors
    - refactor(config): Remove ConfigOptimizer and implement ConfigBuilder for streamlined configuration management
    - refactor: improve Linux checksum handling and release verification in scripts


### Documentation
    - docs: update changelog for v0.57.0 [skip ci]
    - docs: update changelog for v0.56.0 [skip ci]
    - docs: Add Phase 3 extension integration planning and file search documentation
    - docs: Update implementation summary and configuration for file search and system prompt enhancements
    - docs: update changelog for v0.55.1 [skip ci]
    - docs: update changelog for v0.55.0 [skip ci]
    - docs: update changelog for v0.54.4 [skip ci]


### Chores
    - chore: release v{{version}}
    - chore: fix release metadata for vtcode-file-search and vtcode-process-hardening, add version constraint
    - chore: add missing metadata to vtcode-file-search and vtcode-process-hardening
    - chore: update npm package.json to v0.55.1 [skip ci]
    - chore: release v0.55.1
    - chore: release v0.55.0
    - chore: release v0.54.4
    - chore: update npm package.json to v0.54.3 [skip ci]
# [Version 0.57.0] - 2026-01-02


### Features
    - feat(runner): add keyboard protocol environment check and enhance logging
    - feat(tools): Add move and copy file operations with error handling
    - feat(command-safety): Enhance command safety module with comprehensive integration and documentation
    - feat(prompts): Revise system prompts for clarity, autonomy, and Codex alignment
    - feat(dependencies): Update Cargo.lock and Cargo.toml with new packages and versions
    - feat(ollama-integration): Implement comprehensive Ollama client and progress reporting system
    - feat(process-hardening): Introduce vtcode-process-hardening crate for enhanced security measures
    - feat(file-search): Implement vtcode-file-search crate and integration with extensions
    - feat: Unify system prompt instruction generation and skill rendering via `get_user_instructions` and `AgentConfig`.
    - feat: Improve context manager performance with incremental stats and bolster tool security with path and command validation.
    - feat: Implement the Desire Paths philosophy by updating agent prompts and documentation to improve agent UX.
    - feat: add tokio dependency and improve error handling in background task execution
    - feat(subagents): document subagent system and built-in agents; update README and changelog
    - feat(statusline): add custom status line scripts and JSON input handling
    - feat: add reverse search and background operation handling in TUI
    - feat(output-styles): implement output styles feature with customizable behavior and response formats
    - feat(hooks): add lifecycle hooks for file protection, command logging, code formatting, markdown formatting, and notifications
    - feat(marketplace): add marketplace and plugin management commands


### Bug Fixes
    - fix(models): Correct ClaudeOpus41 version and update related assertions refactor(loader): Adjust config loading order and clarify comments feat(output-styles): Add kebab-case renaming for OutputStyleFileConfig


### Refactors
    - refactor(config): Remove ConfigOptimizer and implement ConfigBuilder for streamlined configuration management
    - refactor: improve Linux checksum handling and release verification in scripts


### Documentation
    - docs: update changelog for v0.56.0 [skip ci]
    - docs: Add Phase 3 extension integration planning and file search documentation
    - docs: Update implementation summary and configuration for file search and system prompt enhancements
    - docs: update changelog for v0.55.1 [skip ci]
    - docs: update changelog for v0.55.0 [skip ci]
    - docs: update changelog for v0.54.4 [skip ci]


### Chores
    - chore: fix release metadata for vtcode-file-search and vtcode-process-hardening, add version constraint
    - chore: add missing metadata to vtcode-file-search and vtcode-process-hardening
    - chore: update npm package.json to v0.55.1 [skip ci]
    - chore: release v0.55.1
    - chore: release v0.55.0
    - chore: release v0.54.4
    - chore: update npm package.json to v0.54.3 [skip ci]
# [Version 0.56.0] - 2026-01-02


### Features
    - feat(runner): add keyboard protocol environment check and enhance logging
    - feat(tools): Add move and copy file operations with error handling
    - feat(command-safety): Enhance command safety module with comprehensive integration and documentation
    - feat(prompts): Revise system prompts for clarity, autonomy, and Codex alignment
    - feat(dependencies): Update Cargo.lock and Cargo.toml with new packages and versions
    - feat(ollama-integration): Implement comprehensive Ollama client and progress reporting system
    - feat(process-hardening): Introduce vtcode-process-hardening crate for enhanced security measures
    - feat(file-search): Implement vtcode-file-search crate and integration with extensions
    - feat: Unify system prompt instruction generation and skill rendering via `get_user_instructions` and `AgentConfig`.
    - feat: Improve context manager performance with incremental stats and bolster tool security with path and command validation.
    - feat: Implement the Desire Paths philosophy by updating agent prompts and documentation to improve agent UX.
    - feat: add tokio dependency and improve error handling in background task execution
    - feat(subagents): document subagent system and built-in agents; update README and changelog
    - feat(statusline): add custom status line scripts and JSON input handling
    - feat: add reverse search and background operation handling in TUI
    - feat(output-styles): implement output styles feature with customizable behavior and response formats
    - feat(hooks): add lifecycle hooks for file protection, command logging, code formatting, markdown formatting, and notifications
    - feat(marketplace): add marketplace and plugin management commands


### Bug Fixes
    - fix(models): Correct ClaudeOpus41 version and update related assertions refactor(loader): Adjust config loading order and clarify comments feat(output-styles): Add kebab-case renaming for OutputStyleFileConfig


### Refactors
    - refactor(config): Remove ConfigOptimizer and implement ConfigBuilder for streamlined configuration management
    - refactor: improve Linux checksum handling and release verification in scripts


### Documentation
    - docs: update changelog for v0.56.0 [skip ci]
    - docs: Add Phase 3 extension integration planning and file search documentation
    - docs: Update implementation summary and configuration for file search and system prompt enhancements
    - docs: update changelog for v0.55.1 [skip ci]
    - docs: update changelog for v0.55.0 [skip ci]
    - docs: update changelog for v0.54.4 [skip ci]


### Chores
    - chore: add missing metadata to vtcode-file-search and vtcode-process-hardening
    - chore: update npm package.json to v0.55.1 [skip ci]
    - chore: release v0.55.1
    - chore: release v0.55.0
    - chore: release v0.54.4
    - chore: update npm package.json to v0.54.3 [skip ci]
# [Version 0.56.0] - 2026-01-02


### Features
    - feat(runner): add keyboard protocol environment check and enhance logging
    - feat(tools): Add move and copy file operations with error handling
    - feat(command-safety): Enhance command safety module with comprehensive integration and documentation
    - feat(prompts): Revise system prompts for clarity, autonomy, and Codex alignment
    - feat(dependencies): Update Cargo.lock and Cargo.toml with new packages and versions
    - feat(ollama-integration): Implement comprehensive Ollama client and progress reporting system
    - feat(process-hardening): Introduce vtcode-process-hardening crate for enhanced security measures
    - feat(file-search): Implement vtcode-file-search crate and integration with extensions
    - feat: Unify system prompt instruction generation and skill rendering via `get_user_instructions` and `AgentConfig`.
    - feat: Improve context manager performance with incremental stats and bolster tool security with path and command validation.
    - feat: Implement the Desire Paths philosophy by updating agent prompts and documentation to improve agent UX.
    - feat: add tokio dependency and improve error handling in background task execution
    - feat(subagents): document subagent system and built-in agents; update README and changelog
    - feat(statusline): add custom status line scripts and JSON input handling
    - feat: add reverse search and background operation handling in TUI
    - feat(output-styles): implement output styles feature with customizable behavior and response formats
    - feat(hooks): add lifecycle hooks for file protection, command logging, code formatting, markdown formatting, and notifications
    - feat(marketplace): add marketplace and plugin management commands


### Bug Fixes
    - fix(models): Correct ClaudeOpus41 version and update related assertions refactor(loader): Adjust config loading order and clarify comments feat(output-styles): Add kebab-case renaming for OutputStyleFileConfig


### Refactors
    - refactor(config): Remove ConfigOptimizer and implement ConfigBuilder for streamlined configuration management
    - refactor: improve Linux checksum handling and release verification in scripts


### Documentation
    - docs: Add Phase 3 extension integration planning and file search documentation
    - docs: Update implementation summary and configuration for file search and system prompt enhancements
    - docs: update changelog for v0.55.1 [skip ci]
    - docs: update changelog for v0.55.0 [skip ci]
    - docs: update changelog for v0.54.4 [skip ci]


### Chores
    - chore: update npm package.json to v0.55.1 [skip ci]
    - chore: release v0.55.1
    - chore: release v0.55.0
    - chore: release v0.54.4
    - chore: update npm package.json to v0.54.3 [skip ci]
# [Version 0.55.1] - 2025-12-29


### Features
    - feat: add tokio dependency and improve error handling in background task execution
    - feat(subagents): document subagent system and built-in agents; update README and changelog
    - feat(statusline): add custom status line scripts and JSON input handling
    - feat: add reverse search and background operation handling in TUI
    - feat(output-styles): implement output styles feature with customizable behavior and response formats
    - feat(hooks): add lifecycle hooks for file protection, command logging, code formatting, markdown formatting, and notifications
    - feat(marketplace): add marketplace and plugin management commands
    - feat(notifications): add toggle for terminal notifications in config
    - feat(cli): add support for multiple workspaces and enhanced security controls
    - feat(release): enhance GitHub account handling for CI environments


### Refactors
    - refactor: improve Linux checksum handling and release verification in scripts


### Documentation
    - docs: update changelog for v0.55.0 [skip ci]
    - docs: update changelog for v0.54.4 [skip ci]
    - docs: update changelog for v0.54.3 [skip ci]


### Chores
    - chore: release v0.55.0
    - chore: release v0.54.4
    - chore: update npm package.json to v0.54.3 [skip ci]
    - chore: release v0.54.3
    - chore: update npm package.json to v0.54.2 [skip ci]
# [Version 0.55.0] - 2025-12-29


### Features
    - feat(subagents): document subagent system and built-in agents; update README and changelog
    - feat(statusline): add custom status line scripts and JSON input handling
    - feat: add reverse search and background operation handling in TUI
    - feat(output-styles): implement output styles feature with customizable behavior and response formats
    - feat(hooks): add lifecycle hooks for file protection, command logging, code formatting, markdown formatting, and notifications
    - feat(marketplace): add marketplace and plugin management commands
    - feat(notifications): add toggle for terminal notifications in config
    - feat(cli): add support for multiple workspaces and enhanced security controls
    - feat(release): enhance GitHub account handling for CI environments


### Refactors
    - refactor: improve Linux checksum handling and release verification in scripts


### Documentation
    - docs: update changelog for v0.54.4 [skip ci]
    - docs: update changelog for v0.54.3 [skip ci]


### Chores
    - chore: release v0.54.4
    - chore: update npm package.json to v0.54.3 [skip ci]
    - chore: release v0.54.3
    - chore: update npm package.json to v0.54.2 [skip ci]
# [Version 0.54.4] - 2025-12-29


### Features
    - feat(subagents): document subagent system and built-in agents; update README and changelog
    - feat(statusline): add custom status line scripts and JSON input handling
    - feat: add reverse search and background operation handling in TUI
    - feat(output-styles): implement output styles feature with customizable behavior and response formats
    - feat(hooks): add lifecycle hooks for file protection, command logging, code formatting, markdown formatting, and notifications
    - feat(marketplace): add marketplace and plugin management commands
    - feat(notifications): add toggle for terminal notifications in config
    - feat(cli): add support for multiple workspaces and enhanced security controls
    - feat(release): enhance GitHub account handling for CI environments


### Refactors
    - refactor: improve Linux checksum handling and release verification in scripts


### Documentation
    - docs: update changelog for v0.54.3 [skip ci]


### Chores
    - chore: update npm package.json to v0.54.3 [skip ci]
    - chore: release v0.54.3
    - chore: update npm package.json to v0.54.2 [skip ci]

# [Version 0.54.3] - 2025-12-28

### Features

    - feat(subagents): add subagent system for delegating tasks to specialized agents
        - Built-in subagents: explore (haiku, read-only), plan (sonnet, research), general (sonnet, full), code-reviewer, debugger
        - `spawn_subagent` tool with resume, thoroughness, parent_context params
        - Custom agents via Markdown with YAML frontmatter in `.vtcode/agents/` or `~/.vtcode/agents/`
        - System prompts updated to guide orchestrator delegation
        - Documentation: `docs/subagents/SUBAGENTS.md`
    - feat(notifications): add toggle for terminal notifications in config
    - feat(cli): add support for multiple workspaces and enhanced security controls
    - feat(release): enhance GitHub account handling for CI environments
    - feat(a2a): complete CLI integration and documentation\n\n- Add full A2A CLI with serve, discover, send-task, list-tasks, get-task, cancel-task commands\n- Create comprehensive CLI handlers for all A2A operations\n- Fix streaming event handling with proper pinning\n- Update server.rs Box<dyn Stream> return type for axum compatibility\n- Add completion summary document\n- All checks pass: cargo check --package vtcode-core\n\nImplements: A2A Protocol Phase 4 - CLI integration and user-facing features
    - feat(a2a): add A2A client with streaming support\n\n- New A2aClient for discovery, task ops, push config, and streaming\n- SSE client parses streaming events without extra deps\n- Simple incremental request IDs and HTTPS agent card fetch\n- Tests added for SSE parsing helpers\n\nTests: cargo test --package vtcode-core --lib a2a (39/39)
    - feat(a2a): trigger webhooks on streaming events\n\n- Add webhook_notifier to server state and wire into streaming pipeline\n- Fire webhooks for status updates and messages when broadcasted\n- Fix SSRF-safe config retrieval and avoid Option to_string() error\n- Clean up unused tracing import in webhook module\n- Tests: all A2A suites pass (37/37)
    - feat(a2a): finish push notification config storage and RPC wiring\n\n- Add webhook config storage to TaskManager (set/get/remove) with SSRF validation\n- Wire JSON-RPC handlers for pushNotificationConfig set/get\n- Fix server dispatch and imports\n- All A2A tests pass (37/37) including server + webhook
    - feat(a2a): add webhook notifier for push notifications (Phase 3.2 partial)
    - feat(a2a): implement full SSE streaming support (Phase 3.1)
    - feat: implement Agent2Agent (A2A) Protocol support (Phase 1 & 2)
    - feat: Add async method to InlineSession for receiving next event
    - feat: Remove the `plan` tool and associated components, and update related tool and skill management logic.
    - feat: Refactor tool permission handling for TUI-only mode and update default LLM provider configuration.
    - feat: Add GitHub account switching and cleanup functionality in release script

### Refactors

    - refactor(a2a): clean up unused imports and improve webhook handling
    - refactor: Enhance analysis command to support multiple analysis types and improve error handling
    - refactor: Update tool policies to prompt-based for MCP time functions and improve session handling with cancellation support
    - refactor: Remove unused agent diagnostic tools from TODO documentation
    - refactor: Refine tool policies by removing unused tools, changing several to prompt-based, and making `wrap_text` test-only.
    - refactor: overhaul TUI, tool policy, and context management, adding new documentation and tests.
    - refactor: Remove token budget management and related token estimation/truncation components, and add associated documentation and verification scripts.
    - refactor: improve error message for missing MCP tools with installation instructions

### Documentation

    - docs: update changelog for v0.54.2 [skip ci]
    - docs: update changelog for v0.55.0 [skip ci]
    - docs(a2a): add comprehensive documentation for A2A Protocol implementation
    - docs(a2a): add Phase 3 implementation status tracker

### Chores

    - chore: update npm package.json to v0.54.2 [skip ci]
    - chore: release v0.54.2
    - chore: update npm package.json to v0.54.1 [skip ci]

# [Version 0.54.2] - 2025-12-28

### Features

    - feat(a2a): complete CLI integration and documentation\n\n- Add full A2A CLI with serve, discover, send-task, list-tasks, get-task, cancel-task commands\n- Create comprehensive CLI handlers for all A2A operations\n- Fix streaming event handling with proper pinning\n- Update server.rs Box<dyn Stream> return type for axum compatibility\n- Add completion summary document\n- All checks pass: cargo check --package vtcode-core\n\nImplements: A2A Protocol Phase 4 - CLI integration and user-facing features
    - feat(a2a): add A2A client with streaming support\n\n- New A2aClient for discovery, task ops, push config, and streaming\n- SSE client parses streaming events without extra deps\n- Simple incremental request IDs and HTTPS agent card fetch\n- Tests added for SSE parsing helpers\n\nTests: cargo test --package vtcode-core --lib a2a (39/39)
    - feat(a2a): trigger webhooks on streaming events\n\n- Add webhook_notifier to server state and wire into streaming pipeline\n- Fire webhooks for status updates and messages when broadcasted\n- Fix SSRF-safe config retrieval and avoid Option to_string() error\n- Clean up unused tracing import in webhook module\n- Tests: all A2A suites pass (37/37)
    - feat(a2a): finish push notification config storage and RPC wiring\n\n- Add webhook config storage to TaskManager (set/get/remove) with SSRF validation\n- Wire JSON-RPC handlers for pushNotificationConfig set/get\n- Fix server dispatch and imports\n- All A2A tests pass (37/37) including server + webhook
    - feat(a2a): add webhook notifier for push notifications (Phase 3.2 partial)
    - feat(a2a): implement full SSE streaming support (Phase 3.1)
    - feat: implement Agent2Agent (A2A) Protocol support (Phase 1 & 2)
    - feat: Add async method to InlineSession for receiving next event
    - feat: Remove the `plan` tool and associated components, and update related tool and skill management logic.
    - feat: Refactor tool permission handling for TUI-only mode and update default LLM provider configuration.
    - feat: Add GitHub account switching and cleanup functionality in release script
    - feat: Introduce `EnhancedSkillLoader` and `EnhancedSkill` for unified skill and tool management, and refactor skill discovery results across the agent and CLI.
    - feat: add support for loading skill `references/` and `assets/` directories and introduce `ResourceType::Asset`
    - feat: Reimplement skill management with a new skill model and dedicated modules.
    - feat: Refine tool policies and skill loading for lazy-loaded capabilities, updating system prompts to reflect on-demand activation.
    - feat: Implement lazy-loading and tiered disclosure for agent skills and tools, reducing default available tools and updating system prompts.
    - feat: Introduce skill varieties and enhance skill listing/loading with filtering and dormant tool support
    - feat: Implement `Tool` trait for `CliToolBridge` and integrate skill-based tool registration with `ToolRegistry`.
    - feat: Add new Ollama cloud models and update reasoning model detection.
    - feat: Implement on-demand skill loading with `LoadSkillTool` and `LoadSkillResourceTool`, and enable skill restoration from previous sessions.
    - feat: Add `ListSkillsTool` for programmatic skill discovery, replacing direct skill prompt integration.
    - feat: Add `LoadSkillTool` for progressive skill instruction loading, enhance skill context with path storage, and integrate skill discovery into agent setup.
    - feat: introduce context summarization with adaptive trimming integration and new `Summarize` retention choice.
    - feat: calculate context usage from history and add a final pre-request safety check after trimming.
    - feat: Add `mcp::fetch` and `mcp::time` tools, simplify LSP client message handling, and remove outdated agent system analysis from TODO documentation.
    - feat: Implement PTY session termination on Ctrl+C cancellation with debounced signal handling and status line feedback.
    - feat: Implement timed double Ctrl+C for agent exit, deferring shutdown, and update the default model.
    - feat: Add `--skip-release` option and enhance GitHub release verification logic with CI environment detection.

### Refactors

    - refactor(a2a): clean up unused imports and improve webhook handling
    - refactor: Enhance analysis command to support multiple analysis types and improve error handling
    - refactor: Update tool policies to prompt-based for MCP time functions and improve session handling with cancellation support
    - refactor: Remove unused agent diagnostic tools from TODO documentation
    - refactor: Refine tool policies by removing unused tools, changing several to prompt-based, and making `wrap_text` test-only.
    - refactor: overhaul TUI, tool policy, and context management, adding new documentation and tests.
    - refactor: Remove token budget management and related token estimation/truncation components, and add associated documentation and verification scripts.
    - refactor: improve error message for missing MCP tools with installation instructions
    - refactor: Introduce a dedicated interaction loop for centralized user input and turn flow, updating session and context management.
    - refactor: consistently use `adaptive_trim` with `pruning_ledger` across all proactive token budget guards.
    - refactor: Restructure agent turn execution with new guard, context, and tool outcome modules, removing old loop detection, and updating LSP tools.

### Documentation

    - docs: update changelog for v0.55.0 [skip ci]
    - docs(a2a): add comprehensive documentation for A2A Protocol implementation
    - docs(a2a): add Phase 3 implementation status tracker
    - docs: update changelog for v0.54.1 [skip ci]
    - docs: Streamline TODO by removing verbose system skill enumeration and adding a focused task.

### Chores

    - chore: update npm package.json to v0.54.1 [skip ci]
    - chore: release v0.54.1
    - chore: update npm package.json to v0.54.0 [skip ci]

# [Version 0.55.0] - 2025-12-28

### Features

    - feat(a2a): complete CLI integration and documentation\n\n- Add full A2A CLI with serve, discover, send-task, list-tasks, get-task, cancel-task commands\n- Create comprehensive CLI handlers for all A2A operations\n- Fix streaming event handling with proper pinning\n- Update server.rs Box<dyn Stream> return type for axum compatibility\n- Add completion summary document\n- All checks pass: cargo check --package vtcode-core\n\nImplements: A2A Protocol Phase 4 - CLI integration and user-facing features
    - feat(a2a): add A2A client with streaming support\n\n- New A2aClient for discovery, task ops, push config, and streaming\n- SSE client parses streaming events without extra deps\n- Simple incremental request IDs and HTTPS agent card fetch\n- Tests added for SSE parsing helpers\n\nTests: cargo test --package vtcode-core --lib a2a (39/39)
    - feat(a2a): trigger webhooks on streaming events\n\n- Add webhook_notifier to server state and wire into streaming pipeline\n- Fire webhooks for status updates and messages when broadcasted\n- Fix SSRF-safe config retrieval and avoid Option to_string() error\n- Clean up unused tracing import in webhook module\n- Tests: all A2A suites pass (37/37)
    - feat(a2a): finish push notification config storage and RPC wiring\n\n- Add webhook config storage to TaskManager (set/get/remove) with SSRF validation\n- Wire JSON-RPC handlers for pushNotificationConfig set/get\n- Fix server dispatch and imports\n- All A2A tests pass (37/37) including server + webhook
    - feat(a2a): add webhook notifier for push notifications (Phase 3.2 partial)
    - feat(a2a): implement full SSE streaming support (Phase 3.1)
    - feat: implement Agent2Agent (A2A) Protocol support (Phase 1 & 2)
    - feat: Add async method to InlineSession for receiving next event
    - feat: Remove the `plan` tool and associated components, and update related tool and skill management logic.
    - feat: Refactor tool permission handling for TUI-only mode and update default LLM provider configuration.
    - feat: Add GitHub account switching and cleanup functionality in release script
    - feat: Introduce `EnhancedSkillLoader` and `EnhancedSkill` for unified skill and tool management, and refactor skill discovery results across the agent and CLI.
    - feat: add support for loading skill `references/` and `assets/` directories and introduce `ResourceType::Asset`
    - feat: Reimplement skill management with a new skill model and dedicated modules.
    - feat: Refine tool policies and skill loading for lazy-loaded capabilities, updating system prompts to reflect on-demand activation.
    - feat: Implement lazy-loading and tiered disclosure for agent skills and tools, reducing default available tools and updating system prompts.
    - feat: Introduce skill varieties and enhance skill listing/loading with filtering and dormant tool support
    - feat: Implement `Tool` trait for `CliToolBridge` and integrate skill-based tool registration with `ToolRegistry`.
    - feat: Add new Ollama cloud models and update reasoning model detection.
    - feat: Implement on-demand skill loading with `LoadSkillTool` and `LoadSkillResourceTool`, and enable skill restoration from previous sessions.
    - feat: Add `ListSkillsTool` for programmatic skill discovery, replacing direct skill prompt integration.
    - feat: Add `LoadSkillTool` for progressive skill instruction loading, enhance skill context with path storage, and integrate skill discovery into agent setup.
    - feat: introduce context summarization with adaptive trimming integration and new `Summarize` retention choice.
    - feat: calculate context usage from history and add a final pre-request safety check after trimming.
    - feat: Add `mcp::fetch` and `mcp::time` tools, simplify LSP client message handling, and remove outdated agent system analysis from TODO documentation.
    - feat: Implement PTY session termination on Ctrl+C cancellation with debounced signal handling and status line feedback.
    - feat: Implement timed double Ctrl+C for agent exit, deferring shutdown, and update the default model.
    - feat: Add `--skip-release` option and enhance GitHub release verification logic with CI environment detection.

### Refactors

    - refactor(a2a): clean up unused imports and improve webhook handling
    - refactor: Enhance analysis command to support multiple analysis types and improve error handling
    - refactor: Update tool policies to prompt-based for MCP time functions and improve session handling with cancellation support
    - refactor: Remove unused agent diagnostic tools from TODO documentation
    - refactor: Refine tool policies by removing unused tools, changing several to prompt-based, and making `wrap_text` test-only.
    - refactor: overhaul TUI, tool policy, and context management, adding new documentation and tests.
    - refactor: Remove token budget management and related token estimation/truncation components, and add associated documentation and verification scripts.
    - refactor: improve error message for missing MCP tools with installation instructions
    - refactor: Introduce a dedicated interaction loop for centralized user input and turn flow, updating session and context management.
    - refactor: consistently use `adaptive_trim` with `pruning_ledger` across all proactive token budget guards.
    - refactor: Restructure agent turn execution with new guard, context, and tool outcome modules, removing old loop detection, and updating LSP tools.

### Documentation

    - docs(a2a): add comprehensive documentation for A2A Protocol implementation
    - docs(a2a): add Phase 3 implementation status tracker
    - docs: update changelog for v0.54.1 [skip ci]
    - docs: Streamline TODO by removing verbose system skill enumeration and adding a focused task.

### Chores

    - chore: update npm package.json to v0.54.1 [skip ci]
    - chore: release v0.54.1
    - chore: update npm package.json to v0.54.0 [skip ci]

# [Version 0.54.1] - 2025-12-27

### Features

    - feat: Introduce `EnhancedSkillLoader` and `EnhancedSkill` for unified skill and tool management, and refactor skill discovery results across the agent and CLI.
    - feat: add support for loading skill `references/` and `assets/` directories and introduce `ResourceType::Asset`
    - feat: Reimplement skill management with a new skill model and dedicated modules.
    - feat: Refine tool policies and skill loading for lazy-loaded capabilities, updating system prompts to reflect on-demand activation.
    - feat: Implement lazy-loading and tiered disclosure for agent skills and tools, reducing default available tools and updating system prompts.
    - feat: Introduce skill varieties and enhance skill listing/loading with filtering and dormant tool support
    - feat: Implement `Tool` trait for `CliToolBridge` and integrate skill-based tool registration with `ToolRegistry`.
    - feat: Add new Ollama cloud models and update reasoning model detection.
    - feat: Implement on-demand skill loading with `LoadSkillTool` and `LoadSkillResourceTool`, and enable skill restoration from previous sessions.
    - feat: Add `ListSkillsTool` for programmatic skill discovery, replacing direct skill prompt integration.
    - feat: Add `LoadSkillTool` for progressive skill instruction loading, enhance skill context with path storage, and integrate skill discovery into agent setup.
    - feat: introduce context summarization with adaptive trimming integration and new `Summarize` retention choice.
    - feat: calculate context usage from history and add a final pre-request safety check after trimming.
    - feat: Add `mcp::fetch` and `mcp::time` tools, simplify LSP client message handling, and remove outdated agent system analysis from TODO documentation.
    - feat: Implement PTY session termination on Ctrl+C cancellation with debounced signal handling and status line feedback.
    - feat: Implement timed double Ctrl+C for agent exit, deferring shutdown, and update the default model.
    - feat: Add `--skip-release` option and enhance GitHub release verification logic with CI environment detection.
    - feat: Implement LSP client and manager with agent slash commands, and add LLM provider caching tests.

### Refactors

    - refactor: Introduce a dedicated interaction loop for centralized user input and turn flow, updating session and context management.
    - refactor: consistently use `adaptive_trim` with `pruning_ledger` across all proactive token budget guards.
    - refactor: Restructure agent turn execution with new guard, context, and tool outcome modules, removing old loop detection, and updating LSP tools.

### Documentation

    - docs: Streamline TODO by removing verbose system skill enumeration and adding a focused task.
    - docs: update changelog for v0.54.0 [skip ci]

### Chores

    - chore: update npm package.json to v0.54.0 [skip ci]
    - chore: release v0.54.0
    - chore: update npm package.json to v0.53.2 [skip ci]

# [Version 0.54.0] - 2025-12-27

### Features

    - feat: Implement LSP client and manager with agent slash commands, and add LLM provider caching tests.
    - feat: Update default agent configuration to HuggingFace and refine tool schemas and prompt generation logic.
    - feat: Introduce dynamic system prompt enhancements including temporal context and working directory awareness, along with refined tool usage guidelines for improved agent performance.
    - feat: Enhance textual tool call parsing, pre-validate arguments, and refine tool failure detection to improve agent robustness.

### Documentation

    - docs: update changelog for v0.53.2 [skip ci]

### Chores

    - chore: update npm package.json to v0.53.2 [skip ci]
    - chore: release v0.53.2
    - chore: update npm package.json to v0.53.1 [skip ci]

# [Version 0.53.2] - 2025-12-26

### Features

    - feat: Update default agent configuration to HuggingFace and refine tool schemas and prompt generation logic.
    - feat: Introduce dynamic system prompt enhancements including temporal context and working directory awareness, along with refined tool usage guidelines for improved agent performance.
    - feat: Enhance textual tool call parsing, pre-validate arguments, and refine tool failure detection to improve agent robustness.
    - feat: Enhance session resume/fork logic and improve conversation history display during session startup.
    - feat: add session resumption functionality and update related actions
    - feat: implement session forking with custom session ID support
    - feat: enhance documentation and prompts for clarity, consistency, and performance improvements
    - feat: optimize ANSI syntax highlighting in diff renderer for improved performance

### Refactors

    - refactor: update reasoning labels for clarity in justification and session headers
    - refactor: streamline toolset by merging agent diagnostics and removing deprecated tools
    - refactor: simplify error handling in dotenv loading

### Documentation

    - docs: update changelog for v0.53.1 [skip ci]

### Chores

    - chore: update npm package.json to v0.53.1 [skip ci]
    - chore: release v0.53.1
    - chore: remove completed tasks from TODO.md and improve memory usage for large conversations
    - chore: update npm package.json to v0.53.0 [skip ci]

# [Version 0.53.1] - 2025-12-26

### Features

    - feat: Enhance session resume/fork logic and improve conversation history display during session startup.
    - feat: add session resumption functionality and update related actions
    - feat: implement session forking with custom session ID support
    - feat: enhance documentation and prompts for clarity, consistency, and performance improvements
    - feat: optimize ANSI syntax highlighting in diff renderer for improved performance
    - feat: add agent option to CLI for temporary model override
    - feat: enhance planning tool with quality validation and detailed descriptions for task phases
    - feat: enhance input history navigation and improve session input handling
    - feat: enhance tool execution logging and improve diff preview generation
    - feat: improve error handling for create_file and update_plan methods, enhance logging for theme loading failures
    - feat: implement adaptive TUI tick rate, coalesce scroll events, and enhance session management
    - feat: implement adaptive TUI tick rate and coalesce scroll events
    - feat: Add alias for /config command as /settings, enhance slash command descriptions, and introduce quiet mode in configuration
    - feat: Improve terminal detection and configuration path resolution across operating systems, update LLM provider integrations, and refine agent slash commands and welcome flow.
    - feat: Integrate `TimeoutsConfig` into LLM provider HTTP clients and refactor OpenRouter error handling.
    - feat(llm): Introduce a centralized HTTP client factory, refactor providers to use it for consistent timeout configuration, and enhance API error parsing.
    - feat: Improve tool input deserialization to handle quoted values, enhance `grep` path validation, and update tool policies.
    - feat: Introduce GLM-4.7 Novita model, prepend system prompts in HuggingFace provider, skip GLM thinking parameter, and update tool policies.
    - feat: Introduce `--quiet` flag and separate `stdout` for data and `stderr` for logs to improve CLI piping.
    - feat: Refactor and expand slash command handling with new diagnostics, skills, tools, workspace, and context commands.
    - feat: Implement terminal setup wizard with support for multiple terminals and features, and update LLM provider models.
    - feat: add code intelligence tool with LSP-like navigation features

### Bug Fixes

    - fix: Disable JSON object output and Responses API for GLM models and refine streaming completion event content handling.
    - fix: disable npm publishing in release.sh
    - fix: remove npm installation due to GitHub Actions costs

### Refactors

    - refactor: update reasoning labels for clarity in justification and session headers
    - refactor: streamline toolset by merging agent diagnostics and removing deprecated tools
    - refactor: simplify error handling in dotenv loading

### Documentation

    - docs: update changelog for v0.53.0 [skip ci]
    - docs: Update README with new sections for Keyboard Shortcuts and macOS Alt Shortcut Troubleshooting; refine TODO list entries for clarity and consistency.
    - docs: Add a comprehensive list of new features, bug fixes, and performance improvements to the project TODO list.
    - docs: update changelog for v0.52.10 [skip ci]
    - docs: update changelog for v0.52.9 [skip ci]
    - docs: update installation instructions and scripts for npm package

### Chores

    - chore: remove completed tasks from TODO.md and improve memory usage for large conversations
    - chore: update npm package.json to v0.53.0 [skip ci]
    - chore: release v0.53.0
    - chore: release v0.52.10
    - chore: release v0.52.9
    - chore(deps): bump the all-rust-deps group with 21 updates
    - chore(deps): bump DavidAnson/markdownlint-cli2-action from 21 to 22
    - chore(deps): bump actions/cache from 4 to 5
    - chore(deps): bump actions/upload-artifact from 5 to 6

# [Version 0.53.0] - 2025-12-25

### Features

    - feat: add agent option to CLI for temporary model override
    - feat: enhance planning tool with quality validation and detailed descriptions for task phases
    - feat: enhance input history navigation and improve session input handling
    - feat: enhance tool execution logging and improve diff preview generation
    - feat: improve error handling for create_file and update_plan methods, enhance logging for theme loading failures
    - feat: implement adaptive TUI tick rate, coalesce scroll events, and enhance session management
    - feat: implement adaptive TUI tick rate and coalesce scroll events
    - feat: Add alias for /config command as /settings, enhance slash command descriptions, and introduce quiet mode in configuration
    - feat: Improve terminal detection and configuration path resolution across operating systems, update LLM provider integrations, and refine agent slash commands and welcome flow.
    - feat: Integrate `TimeoutsConfig` into LLM provider HTTP clients and refactor OpenRouter error handling.
    - feat(llm): Introduce a centralized HTTP client factory, refactor providers to use it for consistent timeout configuration, and enhance API error parsing.
    - feat: Improve tool input deserialization to handle quoted values, enhance `grep` path validation, and update tool policies.
    - feat: Introduce GLM-4.7 Novita model, prepend system prompts in HuggingFace provider, skip GLM thinking parameter, and update tool policies.
    - feat: Introduce `--quiet` flag and separate `stdout` for data and `stderr` for logs to improve CLI piping.
    - feat: Refactor and expand slash command handling with new diagnostics, skills, tools, workspace, and context commands.
    - feat: Implement terminal setup wizard with support for multiple terminals and features, and update LLM provider models.
    - feat: add code intelligence tool with LSP-like navigation features

### Bug Fixes

    - fix: Disable JSON object output and Responses API for GLM models and refine streaming completion event content handling.
    - fix: disable npm publishing in release.sh
    - fix: remove npm installation due to GitHub Actions costs
    - fix: rename npm package from vtcode-bin to vtcode

### Documentation

    - docs: Update README with new sections for Keyboard Shortcuts and macOS Alt Shortcut Troubleshooting; refine TODO list entries for clarity and consistency.
    - docs: Add a comprehensive list of new features, bug fixes, and performance improvements to the project TODO list.
    - docs: update changelog for v0.52.10 [skip ci]
    - docs: update changelog for v0.52.9 [skip ci]
    - docs: update installation instructions and scripts for npm package

### Chores

    - chore: release v0.52.10
    - chore: release v0.52.9
    - chore(deps): bump the all-rust-deps group with 21 updates
    - chore: release v0.52.8
    - chore: update npm version to 0.52.8
    - chore: release v0.52.7
    - chore(deps): bump DavidAnson/markdownlint-cli2-action from 21 to 22
    - chore(deps): bump actions/cache from 4 to 5
    - chore(deps): bump actions/upload-artifact from 5 to 6

# [Version 0.52.10] - 2025-12-25

### Features

    - feat: Refactor and expand slash command handling with new diagnostics, skills, tools, workspace, and context commands.
    - feat: Implement terminal setup wizard with support for multiple terminals and features, and update LLM provider models.
    - feat: add code intelligence tool with LSP-like navigation features

### Bug Fixes

    - fix: Disable JSON object output and Responses API for GLM models and refine streaming completion event content handling.
    - fix: disable npm publishing in release.sh
    - fix: remove npm installation due to GitHub Actions costs
    - fix: rename npm package from vtcode-bin to vtcode

### Documentation

    - docs: update changelog for v0.52.9 [skip ci]
    - docs: update installation instructions and scripts for npm package

### Chores

    - chore: release v0.52.9
    - chore(deps): bump the all-rust-deps group with 21 updates
    - chore: release v0.52.8
    - chore: update npm version to 0.52.8
    - chore: release v0.52.7
    - chore(deps): bump DavidAnson/markdownlint-cli2-action from 21 to 22
    - chore(deps): bump actions/cache from 4 to 5
    - chore(deps): bump actions/upload-artifact from 5 to 6

# [Version 0.52.9] - 2025-12-25

### Features

    - feat: Refactor and expand slash command handling with new diagnostics, skills, tools, workspace, and context commands.
    - feat: Implement terminal setup wizard with support for multiple terminals and features, and update LLM provider models.
    - feat: add code intelligence tool with LSP-like navigation features

### Bug Fixes

    - fix: disable npm publishing in release.sh
    - fix: remove npm installation due to GitHub Actions costs
    - fix: rename npm package from vtcode-bin to vtcode

### Documentation

    - docs: update installation instructions and scripts for npm package

### Chores

    - chore(deps): bump the all-rust-deps group with 21 updates
    - chore: release v0.52.8
    - chore: update npm version to 0.52.8
    - chore: release v0.52.7
    - chore(deps): bump DavidAnson/markdownlint-cli2-action from 21 to 22
    - chore(deps): bump actions/cache from 4 to 5
    - chore(deps): bump actions/upload-artifact from 5 to 6

# [Version 0.52.5] - 2025-12-24

### Bug Fixes

    - fix: update release workflow to handle npm publishing correctly
    - fix: unignore .github directory to enable GitHub Actions CI/CD workflows

### Documentation

    - docs: update changelog for v0.52.4 [skip ci]

### Chores

    - chore: update npm package.json to v0.52.4 [skip ci]
    - chore: release v0.52.4
    - chore: update npm package.json to v0.52.3 [skip ci]

# [Version 0.52.4] - 2025-12-24

### Features

    - feat: Add new Z.AI GLM models, refine reasoning support, and update Hugging Face model naming conventions.
    - feat: reimplement HuggingFace LLM provider with dedicated logic to handle its unique API behaviors and compatibility.
    - feat: Add Hugging Face integration documentation and update tool policies to include git and cargo commands while removing some mcp time-related tools.

### Bug Fixes

    - fix: unignore .github directory to enable GitHub Actions CI/CD workflows

### Documentation

    - docs: update changelog for v0.52.3 [skip ci]

### Chores

    - chore: update npm package.json to v0.52.3 [skip ci]
    - chore: release v0.52.3
    - chore: update npm package.json to v0.52.2 [skip ci]

# [Version 0.52.3] - 2025-12-24

### Features

    - feat: Add new Z.AI GLM models, refine reasoning support, and update Hugging Face model naming conventions.
    - feat: reimplement HuggingFace LLM provider with dedicated logic to handle its unique API behaviors and compatibility.
    - feat: Add Hugging Face integration documentation and update tool policies to include git and cargo commands while removing some mcp time-related tools.
    - feat: Add MiniMax model support to the Anthropic provider and adjust its API base URL.
    - feat: Reorganize Hugging Face model identifiers and enhance Anthropic model validation
    - feat: Add Hugging Face provider support and update configuration
    - feat: Update model provider to OpenAI and enhance Responses API handling
    - feat: Expand Hugging Face model support and update provider implementation
    - feat: Update model references and configuration for Z.AI GLM-4.7
    - feat: Add missing OpenRouter model entries and update reasoning handling
    - feat: Enhance OpenAI responses handling with tool call parsing and sampling parameters
    - feat: Update tool policies and add new Grok models to configuration
    - feat: Add Z.AI GLM-4.7 model to models.json and update constants
    - feat: Include Claude agent configurations and GitHub workflows in version control, and update existing agent definitions, skills, commands, hooks, and CI/CD configurations.
    - feat: Introduce agent giving-up reasoning detection and constructive responses, and set `execute_code` tool policy to prompt.

### Bug Fixes

    - fix: add missing package-lock.json      r npm CI workflow

### Documentation

    - docs: update changelog for v0.52.2 [skip ci]

### Chores

    - chore: update npm package.json to v0.52.2 [skip ci]
    - chore: release v0.52.2
    - chore: remove temporary file `temp_check.rs`
    - chore: update npm package.json to v0.52.1 [skip ci]

# [Version 0.52.2] - 2025-12-24

### Features

    - feat: Add MiniMax model support to the Anthropic provider and adjust its API base URL.
    - feat: Reorganize Hugging Face model identifiers and enhance Anthropic model validation
    - feat: Add Hugging Face provider support and update configuration
    - feat: Update model provider to OpenAI and enhance Responses API handling
    - feat: Expand Hugging Face model support and update provider implementation
    - feat: Update model references and configuration for Z.AI GLM-4.7
    - feat: Add missing OpenRouter model entries and update reasoning handling
    - feat: Enhance OpenAI responses handling with tool call parsing and sampling parameters
    - feat: Update tool policies and add new Grok models to configuration
    - feat: Add Z.AI GLM-4.7 model to models.json and update constants
    - feat: Include Claude agent configurations and GitHub workflows in version control, and update existing agent definitions, skills, commands, hooks, and CI/CD configurations.
    - feat: Introduce agent giving-up reasoning detection and constructive responses, and set `execute_code` tool policy to prompt.
    - feat: add keyboard protocol configuration and documentation for enhanced keyboard event handling

### Bug Fixes

    - fix: add missing package-lock.json      r npm CI workflow

### Refactors

    - refactor: simplify configuration handling and update tool permissions in multiple files
    - refactor: apply clippy fixes for code quality improvements
    - refactor: clean up whitespace and formatting across multiple files for improved readability

### Documentation

    - docs: update changelog for v0.52.1 [skip ci]

### Tests

    - test: add missing fields to LLMRequest initializers

### Chores

    - chore: remove temporary file `temp_check.rs`
    - chore: update npm package.json to v0.52.1 [skip ci]
    - chore: release v0.52.1
    - chore: update npm package.json to v0.52.0 [skip ci]

# [Version 0.52.1] - 2025-12-23

### Features

    - feat: add keyboard protocol configuration and documentation for enhanced keyboard event handling
    - feat: Introduce advanced LLM parameters, add default implementations for LLMRequest, Message, and ToolChoice, and remove nextest.toml.

### Refactors

    - refactor: simplify configuration handling and update tool permissions in multiple files
    - refactor: apply clippy fixes for code quality improvements
    - refactor: clean up whitespace and formatting across multiple files for improved readability
    - refactor: Migrate testing from `cargo nextest` to `cargo test` and enhance Anthropic LLM configuration with new parameters.

### Documentation

    - docs: update changelog for v0.52.0 [skip ci]
    - docs: Add guidelines for git operations in AGENTS.md and update LLM provider configuration in vtcode.toml
    - docs: Replace all cargo nextest references with cargo test across documentation and agent rules

### Tests

    - test: add missing fields to LLMRequest initializers

### Chores

    - chore: update npm package.json to v0.52.0 [skip ci]
    - chore: release v0.52.0
    - chore: update npm package.json to v0.51.2 [skip ci]

# [Version 0.52.0] - 2025-12-23

### Features

    - feat: Introduce advanced LLM parameters, add default implementations for LLMRequest, Message, and ToolChoice, and remove nextest.toml.
    - feat: Add new model constants for grok-4-1-fast and grok-code-fast-1

### Refactors

    - refactor: Migrate testing from `cargo nextest` to `cargo test` and enhance Anthropic LLM configuration with new parameters.

### Documentation

    - docs: Add guidelines for git operations in AGENTS.md and update LLM provider configuration in vtcode.toml
    - docs: Replace all cargo nextest references with cargo test across documentation and agent rules
    - docs: update changelog for v0.51.2 [skip ci]

### Chores

    - chore: update npm package.json to v0.51.2 [skip ci]
    - chore: release v0.51.2
    - chore: update npm package.json to v0.51.1 [skip ci]

# [Version 0.51.2] - 2025-12-22

### Features

    - feat: Add new model constants for grok-4-1-fast and grok-code-fast-1
    - feat: Implement search and filter functionality for the TUI configuration palette.
    - feat: Introduce a TUI config palette, refactor rendering logic, and enable dynamic theme application.

### Documentation

    - docs: update changelog for v0.51.1 [skip ci]
    - docs: update changelog for v0.51.0 [skip ci]

### Chores

    - chore: update npm package.json to v0.51.1 [skip ci]
    - chore: release v0.51.1
    - chore: release v0.51.0
    - chore: update npm package.json to v0.50.13 [skip ci]

# [Version 0.51.1] - 2025-12-22

### Features

    - feat: Implement search and filter functionality for the TUI configuration palette.
    - feat: Introduce a TUI config palette, refactor rendering logic, and enable dynamic theme application.
    - feat: enhance session logging functionality and update default model
    - feat: integrate SessionWidget into main render function
    - feat: add buffer-based widgets for input, modal, and slash
    - feat: create ratatui widget foundation
    - feat: implement centralized panic handling for TUI applications
    - feat: add better panic handling with debug mode support
    - feat: enhance list rendering with highlight symbol and repeat option
    - feat: implement XDG Base Directory Specification for configuration and data storage

### Bug Fixes

    - fix: redirect terminal commands from stdout to stderr for TUI functionality
    - fix: change terminal output from stderr to stdout for ModernTui
    - fix: reorder MCP time policies and update tool policy documentation
    - fix: align OpenAI Responses API implementation with official spec

### Refactors

    - refactor: streamline widget block creation and layout definitions in TUI components

### Documentation

    - docs: update changelog for v0.51.0 [skip ci]
    - docs: update changelog for v0.50.13 [skip ci]

### Chores

    - chore: release v0.51.0
    - chore: update npm package.json to v0.50.13 [skip ci]
    - chore: release v0.50.13
    - chore: update npm package.json to v0.50.12 [skip ci]

# [Version 0.51.0] - 2025-12-22

### Features

    - feat: Introduce a TUI config palette, refactor rendering logic, and enable dynamic theme application.
    - feat: enhance session logging functionality and update default model
    - feat: integrate SessionWidget into main render function
    - feat: add buffer-based widgets for input, modal, and slash
    - feat: create ratatui widget foundation
    - feat: implement centralized panic handling for TUI applications
    - feat: add better panic handling with debug mode support
    - feat: enhance list rendering with highlight symbol and repeat option
    - feat: implement XDG Base Directory Specification for configuration and data storage

### Bug Fixes

    - fix: redirect terminal commands from stdout to stderr for TUI functionality
    - fix: change terminal output from stderr to stdout for ModernTui
    - fix: reorder MCP time policies and update tool policy documentation
    - fix: align OpenAI Responses API implementation with official spec

### Refactors

    - refactor: streamline widget block creation and layout definitions in TUI components

### Documentation

    - docs: update changelog for v0.50.13 [skip ci]

### Chores

    - chore: update npm package.json to v0.50.13 [skip ci]
    - chore: release v0.50.13
    - chore: update npm package.json to v0.50.12 [skip ci]

# [Version 0.50.13] - 2025-12-21

### Features

    - feat: enhance session logging functionality and update default model
    - feat: integrate SessionWidget into main render function
    - feat: add buffer-based widgets for input, modal, and slash
    - feat: create ratatui widget foundation
    - feat: implement centralized panic handling for TUI applications
    - feat: add better panic handling with debug mode support
    - feat: enhance list rendering with highlight symbol and repeat option
    - feat: implement XDG Base Directory Specification for configuration and data storage
    - feat: Enhance tool execution error handling and implement planning mode warnings
    - feat: Implement TUI-aware tool approval prompts and human-in-the-loop notification bell.
    - feat: add plan phase management and update tool registry for planning mode
    - feat: add HITL notification bell configuration and implement terminal bell notification for approvals
    - feat: Implement pre-flight LLM request and tool definition validation, and ensure `mark_tool_loop_limit_hit` is idempotent.
    - feat: improve release process by adding Linux build automation and related documentation.

### Bug Fixes

    - fix: redirect terminal commands from stdout to stderr for TUI functionality
    - fix: change terminal output from stderr to stdout for ModernTui
    - fix: reorder MCP time policies and update tool policy documentation
    - fix: align OpenAI Responses API implementation with official spec

### Performance Improvements

    - perf: optimize rate limiting with a read-lock fast path and refactor tool execution retry delays using constant values.

### Refactors

    - refactor: streamline widget block creation and layout definitions in TUI components

### Documentation

    - docs: update changelog for v0.50.12 [skip ci]

### Chores

    - chore: update npm package.json to v0.50.12 [skip ci]
    - chore: release v0.50.12
    - chore: update npm package.json to v0.50.11 [skip ci]

# [Version 0.50.12] - 2025-12-20

### Features

    - feat: Enhance tool execution error handling and implement planning mode warnings
    - feat: Implement TUI-aware tool approval prompts and human-in-the-loop notification bell.
    - feat: add plan phase management and update tool registry for planning mode
    - feat: add HITL notification bell configuration and implement terminal bell notification for approvals
    - feat: Implement pre-flight LLM request and tool definition validation, and ensure `mark_tool_loop_limit_hit` is idempotent.
    - feat: improve release process by adding Linux build automation and related documentation.
    - feat: Add Linux build and release support, fix npm publish, and improve release asset uploads and install script error handling.

### Performance Improvements

    - perf: optimize rate limiting with a read-lock fast path and refactor tool execution retry delays using constant values.

### Documentation

    - docs: update changelog for v0.50.11 [skip ci]

### Chores

    - chore: update npm package.json to v0.50.11 [skip ci]
    - chore: release v0.50.11
    - chore: update VSCode extension package.json to v0.50.10 [skip ci]
    - chore: update npm package.json to v0.50.10 [skip ci]

# [Version 0.50.11] - 2025-12-20

### Features

    - feat: Add Linux build and release support, fix npm publish, and improve release asset uploads and install script error handling.
    - feat: Add npm publishing troubleshooting guide and authentication setup script, and automate binary stub creation in the release process.
    - feat: Enable manual versioned builds in the release workflow and significantly enhance the install script with improved dependency/platform detection, asset verification, and a cargo fallback.

### Bug Fixes

    - fix: resolve GitHub release binary upload failures and enhance release script verification with new documentation.

### Documentation

    - docs: update changelog for v0.50.10 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.50.10 [skip ci]
    - chore: update npm package.json to v0.50.10 [skip ci]
    - chore: release v0.50.10
    - chore: update VSCode extension package.json to v0.50.9 [skip ci]
    - chore: update npm package.json to v0.50.9 [skip ci]

# [Version 0.50.10] - 2025-12-20

### Features

    - feat: Add npm publishing troubleshooting guide and authentication setup script, and automate binary stub creation in the release process.
    - feat: Enable manual versioned builds in the release workflow and significantly enhance the install script with improved dependency/platform detection, asset verification, and a cargo fallback.
    - feat: Implement agent task retry with exponential backoff and render tool follow-up prompts.
    - feat: Enhance skill validation and file reference checks for Agent Skills compliance
    - feat: Implement Agent Skills specification by adding `compatibility` and `metadata` fields to skill manifests, updating `allowed-tools` to a space-delimited string, and clarifying skill loading behavior.
    - feat: Enhance tool execution policy with granular user confirmation, auto-acceptance, and feedback capabilities.
    - feat: extract anthropic config, reduce configuration complexity, document experimental features
    - feat: remove reinforcement learning and optimization modules and configurations.
    - feat: Add context-aware prompt enrichment (vibe coding) with new context modules and wizard modal interaction events.
    - feat: Add full-auto mode support and update tool policies for improved automation
    - feat: Implement per-tool rate limiting and refactor agent tool execution state management.
    - feat: implement circuit breaker pattern for MCP client failures and optimize tool inventory management
    - feat: add code reviewer and commit message generator skills

### Bug Fixes

    - fix: resolve GitHub release binary upload failures and enhance release script verification with new documentation.
    - fix: update tool policies and configuration settings for improved performance
    - fix: remove external editor keybinding (Control+E)
    - fix: prevent arrow keys from triggering external editor launch

### Refactors

    - refactor: remove router configuration and related core logic

### Documentation

    - docs: update changelog for v0.50.9 [skip ci]
    - docs: update changelog for v0.50.8 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.50.9 [skip ci]
    - chore: update npm package.json to v0.50.9 [skip ci]
    - chore: release v0.50.9
    - chore: release v0.50.8
    - chore: remove AI model routing configuration from TOML files
    - chore: update VSCode extension package.json to v0.50.7 [skip ci]
    - chore: update npm package.json to v0.50.7 [skip ci]

# [Version 0.50.9] - 2025-12-20

### Features

    - feat: Implement agent task retry with exponential backoff and render tool follow-up prompts.
    - feat: Enhance skill validation and file reference checks for Agent Skills compliance
    - feat: Implement Agent Skills specification by adding `compatibility` and `metadata` fields to skill manifests, updating `allowed-tools` to a space-delimited string, and clarifying skill loading behavior.
    - feat: Enhance tool execution policy with granular user confirmation, auto-acceptance, and feedback capabilities.
    - feat: extract anthropic config, reduce configuration complexity, document experimental features
    - feat: remove reinforcement learning and optimization modules and configurations.
    - feat: Add context-aware prompt enrichment (vibe coding) with new context modules and wizard modal interaction events.
    - feat: Add full-auto mode support and update tool policies for improved automation
    - feat: Implement per-tool rate limiting and refactor agent tool execution state management.
    - feat: implement circuit breaker pattern for MCP client failures and optimize tool inventory management
    - feat: add code reviewer and commit message generator skills

### Bug Fixes

    - fix: update tool policies and configuration settings for improved performance
    - fix: remove external editor keybinding (Control+E)
    - fix: prevent arrow keys from triggering external editor launch

### Refactors

    - refactor: remove router configuration and related core logic

### Documentation

    - docs: update changelog for v0.50.8 [skip ci]
    - docs: update changelog for v0.50.7 [skip ci]
    - docs: update changelog for v0.50.6 [skip ci]

### Chores

    - chore: release v0.50.8
    - chore: remove AI model routing configuration from TOML files
    - chore: update VSCode extension package.json to v0.50.7 [skip ci]
    - chore: update npm package.json to v0.50.7 [skip ci]
    - chore: release v0.50.7
    - chore: release v0.50.6
    - chore: update VSCode extension package.json to v0.50.5 [skip ci]
    - chore: update npm package.json to v0.50.5 [skip ci]

# [Version 0.50.8] - 2025-12-20

### Features

    - feat: Implement Agent Skills specification by adding `compatibility` and `metadata` fields to skill manifests, updating `allowed-tools` to a space-delimited string, and clarifying skill loading behavior.
    - feat: Enhance tool execution policy with granular user confirmation, auto-acceptance, and feedback capabilities.
    - feat: extract anthropic config, reduce configuration complexity, document experimental features
    - feat: remove reinforcement learning and optimization modules and configurations.
    - feat: Add context-aware prompt enrichment (vibe coding) with new context modules and wizard modal interaction events.
    - feat: Add full-auto mode support and update tool policies for improved automation
    - feat: Implement per-tool rate limiting and refactor agent tool execution state management.
    - feat: implement circuit breaker pattern for MCP client failures and optimize tool inventory management
    - feat: add code reviewer and commit message generator skills

### Bug Fixes

    - fix: update tool policies and configuration settings for improved performance
    - fix: remove external editor keybinding (Control+E)
    - fix: prevent arrow keys from triggering external editor launch

### Refactors

    - refactor: remove router configuration and related core logic

### Documentation

    - docs: update changelog for v0.50.7 [skip ci]
    - docs: update changelog for v0.50.6 [skip ci]

### Chores

    - chore: remove AI model routing configuration from TOML files
    - chore: update VSCode extension package.json to v0.50.7 [skip ci]
    - chore: update npm package.json to v0.50.7 [skip ci]
    - chore: release v0.50.7
    - chore: release v0.50.6
    - chore: update VSCode extension package.json to v0.50.5 [skip ci]
    - chore: update npm package.json to v0.50.5 [skip ci]

# [Version 0.50.7] - 2025-12-19

### Features

    - feat: Add success indicators to renderer on exit commands and session end
    - feat: Update tool policy to prompt for file creation and execution, enhance session state management, and adjust LLM provider settings in configuration.
    - feat: Enhance skill definitions with new metadata fields, improve LLM provider support, and refine TUI components.
    - feat: Implement shell command policy checking with regex and glob patterns and add new metadata fields to skill definitions.
    - feat: enhance line ending handling in patch operations and tests

### Bug Fixes

    - fix: Correct test expectations for token threshold boundaries
    - fix: handle errors in AtomicWriter creation and improve diff operations tests

### Refactors

    - refactor: remove unnecessary whitespace in diff and test files

### Documentation

    - docs: update changelog for v0.50.6 [skip ci]
    - docs: update changelog for v0.50.5 [skip ci]

### Chores

    - chore: release v0.50.6
    - chore: update VSCode extension package.json to v0.50.5 [skip ci]
    - chore: update npm package.json to v0.50.5 [skip ci]
    - chore: release v0.50.5
    - chore: update VSCode extension package.json to v0.50.4 [skip ci]
    - chore: update npm package.json to v0.50.4 [skip ci]

# [Version 0.50.6] - 2025-12-19

### Features

    - feat: Add success indicators to renderer on exit commands and session end
    - feat: Update tool policy to prompt for file creation and execution, enhance session state management, and adjust LLM provider settings in configuration.
    - feat: Enhance skill definitions with new metadata fields, improve LLM provider support, and refine TUI components.
    - feat: Implement shell command policy checking with regex and glob patterns and add new metadata fields to skill definitions.
    - feat: enhance line ending handling in patch operations and tests

### Bug Fixes

    - fix: Correct test expectations for token threshold boundaries
    - fix: handle errors in AtomicWriter creation and improve diff operations tests

### Refactors

    - refactor: remove unnecessary whitespace in diff and test files

### Documentation

    - docs: update changelog for v0.50.5 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.50.5 [skip ci]
    - chore: update npm package.json to v0.50.5 [skip ci]
    - chore: release v0.50.5
    - chore: update VSCode extension package.json to v0.50.4 [skip ci]
    - chore: update npm package.json to v0.50.4 [skip ci]

# [Version 0.50.5] - 2025-12-19

### Features

    - feat: Add success indicators to renderer on exit commands and session end
    - feat: Update tool policy to prompt for file creation and execution, enhance session state management, and adjust LLM provider settings in configuration.
    - feat: Enhance skill definitions with new metadata fields, improve LLM provider support, and refine TUI components.
    - feat: Implement shell command policy checking with regex and glob patterns and add new metadata fields to skill definitions.
    - feat: enhance line ending handling in patch operations and tests
    - feat: add Gemini 3 Flash Preview model and update configurations

### Bug Fixes

    - fix: Correct test expectations for token threshold boundaries
    - fix: handle errors in AtomicWriter creation and improve diff operations tests

### Refactors

    - refactor: remove unnecessary whitespace in diff and test files

### Documentation

    - docs: update changelog for v0.50.4 [skip ci]
    - docs: update changelog for v0.50.3 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.50.4 [skip ci]
    - chore: update npm package.json to v0.50.4 [skip ci]
    - chore: release v0.50.4
    - chore: release v0.50.3
    - chore: update VSCode extension package.json to v0.50.2 [skip ci]
    - chore: update npm package.json to v0.50.2 [skip ci]

# [Version 0.50.4] - 2025-12-18

### Features

    - feat: add Gemini 3 Flash Preview model and update configurations

### Documentation

    - docs: update changelog for v0.50.3 [skip ci]
    - docs: update changelog for v0.50.2 [skip ci]

### Chores

    - chore: release v0.50.3
    - chore: update VSCode extension package.json to v0.50.2 [skip ci]
    - chore: update npm package.json to v0.50.2 [skip ci]
    - chore: release v0.50.2
    - chore: update VSCode extension package.json to v0.50.1 [skip ci]
    - chore: update npm package.json to v0.50.1 [skip ci]

# [Version 0.50.3] - 2025-12-18

### Features

    - feat: add Gemini 3 Flash Preview model and update configurations

### Documentation

    - docs: update changelog for v0.50.2 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.50.2 [skip ci]
    - chore: update npm package.json to v0.50.2 [skip ci]
    - chore: release v0.50.2
    - chore: update VSCode extension package.json to v0.50.1 [skip ci]
    - chore: update npm package.json to v0.50.1 [skip ci]

# [Version 0.50.2] - 2025-12-16

### Bug Fixes

    - fix: correct logical operator for XAI provider model check

### Documentation

    - docs: update changelog for v0.50.1 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.50.1 [skip ci]
    - chore: update npm package.json to v0.50.1 [skip ci]
    - chore: release v0.50.1
    - chore: update VSCode extension package.json to v0.50.0 [skip ci]
    - chore: update npm package.json to v0.50.0 [skip ci]

# [Version 0.50.1] - 2025-12-16

### Features

    - feat: implement rate limiting for tool calls and add Nemotron-3-Nano model support

### Bug Fixes

    - fix: correct logical operator for XAI provider model check

### Documentation

    - docs: update changelog for v0.50.0 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.50.0 [skip ci]
    - chore: update npm package.json to v0.50.0 [skip ci]
    - chore: release v0.50.0
    - chore: update VSCode extension package.json to v0.49.8 [skip ci]
    - chore: update npm package.json to v0.49.8 [skip ci]

# [Version 0.50.0] - 2025-12-16

### Features

    - feat: implement rate limiting for tool calls and add Nemotron-3-Nano model support

### Documentation

    - docs: update changelog for v0.49.8 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.49.8 [skip ci]
    - chore: update npm package.json to v0.49.8 [skip ci]
    - chore: release v0.49.8
    - chore: update VSCode extension package.json to v0.49.7 [skip ci]
    - chore: update npm package.json to v0.49.7 [skip ci]

# [Version 0.49.8] - 2025-12-16

### Documentation

    - docs: update changelog for v0.49.7 [skip ci]
    - docs: update changelog for v0.49.6 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.49.7 [skip ci]
    - chore: update npm package.json to v0.49.7 [skip ci]
    - chore: release v0.49.7
    - chore: release v0.49.6
    - chore: update VSCode extension package.json to v0.49.5 [skip ci]
    - chore: update npm package.json to v0.49.5 [skip ci]

# [Version 0.49.7] - 2025-12-15

### Bug Fixes

    - fix: include templates directory in package for crates.io publishing

### Documentation

    - docs: update changelog for v0.49.6 [skip ci]
    - docs: update changelog for v0.49.5 [skip ci]
    - docs: update changelog for v0.49.4 [skip ci]
    - docs: update changelog for v0.49.3 [skip ci]
    - docs: update changelog for v0.49.2 [skip ci]

### Chores

    - chore: release v0.49.6
    - chore: update VSCode extension package.json to v0.49.5 [skip ci]
    - chore: update npm package.json to v0.49.5 [skip ci]
    - chore: release v0.49.5
    - chore: release v0.49.4
    - chore: release v0.49.3
    - chore: release v0.49.2
    - chore: update VSCode extension package.json to v0.49.1 [skip ci]
    - chore: update npm package.json to v0.49.1 [skip ci]

# [Version 0.49.6] - 2025-12-15

### Bug Fixes

    - fix: include templates directory in package for crates.io publishing

### Documentation

    - docs: update changelog for v0.49.5 [skip ci]
    - docs: update changelog for v0.49.4 [skip ci]
    - docs: update changelog for v0.49.3 [skip ci]
    - docs: update changelog for v0.49.2 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.49.5 [skip ci]
    - chore: update npm package.json to v0.49.5 [skip ci]
    - chore: release v0.49.5
    - chore: release v0.49.4
    - chore: release v0.49.3
    - chore: release v0.49.2
    - chore: update VSCode extension package.json to v0.49.1 [skip ci]
    - chore: update npm package.json to v0.49.1 [skip ci]

# [Version 0.49.5] - 2025-12-14

### Bug Fixes

    - fix: include templates directory in package for crates.io publishing

### Documentation

    - docs: update changelog for v0.49.4 [skip ci]
    - docs: update changelog for v0.49.3 [skip ci]
    - docs: update changelog for v0.49.2 [skip ci]
    - docs: update changelog for v0.49.1 [skip ci]

### Chores

    - chore: release v0.49.4
    - chore: release v0.49.3
    - chore: release v0.49.2
    - chore: update VSCode extension package.json to v0.49.1 [skip ci]
    - chore: update npm package.json to v0.49.1 [skip ci]
    - chore: release v0.49.1
    - chore: update npm package.json to v0.49.0 [skip ci]

# [Version 0.49.4] - 2025-12-14

### Documentation

    - docs: update changelog for v0.49.3 [skip ci]
    - docs: update changelog for v0.49.2 [skip ci]
    - docs: update changelog for v0.49.1 [skip ci]

### Chores

    - chore: release v0.49.3
    - chore: release v0.49.2
    - chore: update VSCode extension package.json to v0.49.1 [skip ci]
    - chore: update npm package.json to v0.49.1 [skip ci]
    - chore: release v0.49.1
    - chore: update npm package.json to v0.49.0 [skip ci]

# [Version 0.49.3] - 2025-12-14

### Documentation

    - docs: update changelog for v0.49.2 [skip ci]
    - docs: update changelog for v0.49.1 [skip ci]

### Chores

    - chore: release v0.49.2
    - chore: update VSCode extension package.json to v0.49.1 [skip ci]
    - chore: update npm package.json to v0.49.1 [skip ci]
    - chore: release v0.49.1
    - chore: update npm package.json to v0.49.0 [skip ci]

# [Version 0.49.2] - 2025-12-14

### Documentation

    - docs: update changelog for v0.49.1 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.49.1 [skip ci]
    - chore: update npm package.json to v0.49.1 [skip ci]
    - chore: release v0.49.1
    - chore: update npm package.json to v0.49.0 [skip ci]

### Added

-   **Comprehensive Skills Location System**: Implemented multi-location skill discovery with precedence handling
    -   VT Code User Skills (`~/.vtcode/skills/`) - Highest precedence
    -   VT Code Project Skills (`.vtcode/skills/`) - Project-specific skills
    -   Pi Framework Skills (`~/.pi/skills/`, `.pi/skills/`)
    -   Claude Code Skills (`~/.claude/skills/`, `.claude/skills/`)
    -   Codex CLI Skills (`~/.codex/skills/`)
-   **Precedence System**: Skills from higher precedence locations override lower precedence skills with the same name
-   **Migration Support**: All existing skills migrated from `.claude/skills` to `.vtcode/skills` with backward compatibility
-   **Enhanced Skill Loader**: Updated loader to integrate with new location system while maintaining backward compatibility

### Changed

-   Updated skills documentation to reflect new location system and precedence handling
-   Enhanced skill discovery to support recursive scanning and proper name collision resolution

## [Version 0.43.0] - 2025-11-09

# [Version 0.49.1] - 2025-12-13

### Bug Fixes

    - fix: update execute_code and skill policies to allow execution
    - fix: remove outdated skill discovery documentation and integrate new skill loading functionality
    - fix: enhance skill discovery and loading functionality for vtcode agent
    - fix: improve skill tool output to include full instructions
    - fix: add missing skill tool function declaration
    - fix: vtcode agent skill discovery using SkillLoader instead of SkillManager

### Documentation

    - docs: update changelog for v0.49.0 [skip ci]
    - docs: add complete skill tool fix summary

### Chores

    - chore: update npm package.json to v0.49.0 [skip ci]
    - chore: release v0.49.0
    - chore: update VSCode extension package.json to v0.48.3 [skip ci]
    - chore: update npm package.json to v0.48.3 [skip ci]

# [Version 0.49.0] - 2025-12-13

### Bug Fixes

    - fix: update execute_code and skill policies to allow execution
    - fix: remove outdated skill discovery documentation and integrate new skill loading functionality
    - fix: enhance skill discovery and loading functionality for vtcode agent
    - fix: improve skill tool output to include full instructions
    - fix: add missing skill tool function declaration
    - fix: vtcode agent skill discovery using SkillLoader instead of SkillManager

### Documentation

    - docs: add complete skill tool fix summary
    - docs: update changelog for v0.48.3 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.48.3 [skip ci]
    - chore: update npm package.json to v0.48.3 [skip ci]
    - chore: release v0.48.3
    - chore: update VSCode extension package.json to v0.48.2 [skip ci]
    - chore: update npm package.json to v0.48.2 [skip ci]

# [Version 0.48.3] - 2025-12-13

### Refactors

    - refactor: update LLMError handling in turn_processing
    - refactor: standardize LLMError structure across providers
    - refactor: enhance Z.AI provider error handling and API key validation
    - refactor: enhance context trimming and session management features
    - refactor: update configuration and documentation for improved clarity and performance

### Documentation

    - docs: update changelog for v0.48.2 [skip ci]
    - docs: update changelog for v0.48.1 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.48.2 [skip ci]
    - chore: update npm package.json to v0.48.2 [skip ci]
    - chore: release v0.48.2
    - chore: release v0.48.1
    - chore: update VSCode extension package.json to v0.48.0 [skip ci]
    - chore: update npm package.json to v0.48.0 [skip ci]

# [Version 0.48.2] - 2025-12-13

### Features

    - feat: update OpenAI provider to support GPT-5.2 and enhance reasoning options
    - feat: enhance response output with reasoning traces
    - feat: implement timeout warning management for tool execution
    - feat: enhance tool execution with rate limiting and workspace management

### Bug Fixes

    - fix: improve error handling in LLM client creation and tool execution

### Refactors

    - refactor: update LLMError handling in turn_processing
    - refactor: standardize LLMError structure across providers
    - refactor: enhance Z.AI provider error handling and API key validation
    - refactor: enhance context trimming and session management features
    - refactor: update configuration and documentation for improved clarity and performance
    - refactor: update GPT-5.2 model identifiers and documentation
    - refactor: streamline error handling and conditional checks in various modules
    - refactor: remove logging statements from UI interaction and session handling
    - refactor: remove outdated vibe_tooling_mapping documentation

### Documentation

    - docs: update changelog for v0.48.1 [skip ci]
    - docs: update changelog for v0.48.0 [skip ci]

### Chores

    - chore: release v0.48.1
    - chore: update VSCode extension package.json to v0.48.0 [skip ci]
    - chore: update npm package.json to v0.48.0 [skip ci]
    - chore: release v0.48.0
    - chore: update VSCode extension package.json to v0.47.16 [skip ci]
    - chore: update npm package.json to v0.47.16 [skip ci]

# [Version 0.48.1] - 2025-12-13

### Features

    - feat: update OpenAI provider to support GPT-5.2 and enhance reasoning options
    - feat: enhance response output with reasoning traces
    - feat: implement timeout warning management for tool execution
    - feat: enhance tool execution with rate limiting and workspace management

### Bug Fixes

    - fix: improve error handling in LLM client creation and tool execution

### Refactors

    - refactor: standardize LLMError structure across providers
    - refactor: enhance Z.AI provider error handling and API key validation
    - refactor: enhance context trimming and session management features
    - refactor: update configuration and documentation for improved clarity and performance
    - refactor: update GPT-5.2 model identifiers and documentation
    - refactor: streamline error handling and conditional checks in various modules
    - refactor: remove logging statements from UI interaction and session handling
    - refactor: remove outdated vibe_tooling_mapping documentation

### Documentation

    - docs: update changelog for v0.48.0 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.48.0 [skip ci]
    - chore: update npm package.json to v0.48.0 [skip ci]
    - chore: release v0.48.0
    - chore: update VSCode extension package.json to v0.47.16 [skip ci]
    - chore: update npm package.json to v0.47.16 [skip ci]

# [Version 0.48.0] - 2025-12-12

### Features

    - feat: update OpenAI provider to support GPT-5.2 and enhance reasoning options
    - feat: enhance response output with reasoning traces
    - feat: implement timeout warning management for tool execution
    - feat: enhance tool execution with rate limiting and workspace management
    - feat: enhance logging and error handling in orchestrator and agent components
    - feat: improve tracing initialization and error handling in main
    - feat: implement idle turn detection and management in task execution
    - feat: improve HTTP client pool handling and enhance caching middleware
    - feat: enhance loop detection and rate limiting in tool execution
    - feat: implement timeout management for streaming and generation requests
    - feat: implement streaming failure management and cooldown mechanism
    - feat: enhance agent logging and improve markdown rendering
    - feat: introduce reinforcement learning optimization and enhance configuration
    - feat: add new dependencies and improve error handling in main
    - feat: enhance grep result optimization and tool registration
    - feat: enhance timeout configuration and adaptive timeout handling
    - feat: add search_replace tool and enhance file operations

### Bug Fixes

    - fix: improve error handling in LLM client creation and tool execution

### Refactors

    - refactor: update GPT-5.2 model identifiers and documentation
    - refactor: streamline error handling and conditional checks in various modules
    - refactor: remove logging statements from UI interaction and session handling
    - refactor: remove outdated vibe_tooling_mapping documentation
    - refactor: enhance loop detection logic and add tests

### Documentation

    - docs: update changelog for v0.47.16 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.47.16 [skip ci]
    - chore: update npm package.json to v0.47.16 [skip ci]
    - chore: release v0.47.16
    - chore: update VSCode extension package.json to v0.47.15 [skip ci]
    - chore: update npm package.json to v0.47.15 [skip ci]

# [Version 0.47.16] - 2025-12-11

### Features

    - feat: enhance logging and error handling in orchestrator and agent components
    - feat: improve tracing initialization and error handling in main
    - feat: implement idle turn detection and management in task execution
    - feat: improve HTTP client pool handling and enhance caching middleware
    - feat: enhance loop detection and rate limiting in tool execution
    - feat: implement timeout management for streaming and generation requests
    - feat: implement streaming failure management and cooldown mechanism
    - feat: enhance agent logging and improve markdown rendering
    - feat: introduce reinforcement learning optimization and enhance configuration
    - feat: add new dependencies and improve error handling in main
    - feat: enhance grep result optimization and tool registration
    - feat: enhance timeout configuration and adaptive timeout handling
    - feat: add search_replace tool and enhance file operations

### Refactors

    - refactor: enhance loop detection logic and add tests
    - refactor: simplify line style selection logic in tool output
    - refactor: streamline initialization and error handling in various modules
    - refactor: simplify ToolCallUpdateFields initialization
    - refactor: update tool policy and improve middleware handling

### Documentation

    - docs: update changelog for v0.47.15 [skip ci]
    - docs: update changelog for v0.47.14 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.47.15 [skip ci]
    - chore: update npm package.json to v0.47.15 [skip ci]
    - chore: release v0.47.15
    - chore: release v0.47.14
    - chore: update VSCode extension package.json to v0.47.13 [skip ci]
    - chore: update npm package.json to v0.47.13 [skip ci]

# [Version 0.47.15] - 2025-12-11

### Refactors

    - refactor: simplify line style selection logic in tool output
    - refactor: streamline initialization and error handling in various modules
    - refactor: simplify ToolCallUpdateFields initialization
    - refactor: update tool policy and improve middleware handling
    - refactor: remove deprecated tools and update tool policies
    - refactor: update tool policy and streamline follow-up handling
    - refactor: enhance PTY command output summarization and follow-up handling
    - refactor: enhance context management and prompt generation
    - refactor: enhance system prompts with improved tool safety and execution guidelines
    - refactor: implement tool execution retry logic and enhance error handling
    - refactor: enhance tool policy and prompt clarity
    - refactor: implement tool denial handling in agent runner
    - refactor: enhance McpAllowListConfig structure and improve rule definitions
    - refactor: improve code clarity and consistency in multiple modules
    - refactor: streamline conditional checks and improve code readability
    - refactor: unify token budget constants and improve context management

### Documentation

    - docs: update changelog for v0.47.14 [skip ci]
    - docs: update changelog for v0.47.13 [skip ci]

### Chores

    - chore: release v0.47.14
    - chore: update VSCode extension package.json to v0.47.13 [skip ci]
    - chore: update npm package.json to v0.47.13 [skip ci]
    - chore: release v0.47.13
    - chore: update VSCode extension package.json to v0.47.12 [skip ci]
    - chore: update npm package.json to v0.47.12 [skip ci]

# [Version 0.47.14] - 2025-12-11

### Refactors

    - refactor: simplify line style selection logic in tool output
    - refactor: streamline initialization and error handling in various modules
    - refactor: simplify ToolCallUpdateFields initialization
    - refactor: update tool policy and improve middleware handling
    - refactor: remove deprecated tools and update tool policies
    - refactor: update tool policy and streamline follow-up handling
    - refactor: enhance PTY command output summarization and follow-up handling
    - refactor: enhance context management and prompt generation
    - refactor: enhance system prompts with improved tool safety and execution guidelines
    - refactor: implement tool execution retry logic and enhance error handling
    - refactor: enhance tool policy and prompt clarity
    - refactor: implement tool denial handling in agent runner
    - refactor: enhance McpAllowListConfig structure and improve rule definitions
    - refactor: improve code clarity and consistency in multiple modules
    - refactor: streamline conditional checks and improve code readability
    - refactor: unify token budget constants and improve context management

### Documentation

    - docs: update changelog for v0.47.13 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.47.13 [skip ci]
    - chore: update npm package.json to v0.47.13 [skip ci]
    - chore: release v0.47.13
    - chore: update VSCode extension package.json to v0.47.12 [skip ci]
    - chore: update npm package.json to v0.47.12 [skip ci]

# [Version 0.47.13] - 2025-12-10

### Features

    - feat: update tool policy and enhance loop detection functionality
    - feat: implement tool call safety validation and execution tracking

### Refactors

    - refactor: remove deprecated tools and update tool policies
    - refactor: update tool policy and streamline follow-up handling
    - refactor: enhance PTY command output summarization and follow-up handling
    - refactor: enhance context management and prompt generation
    - refactor: enhance system prompts with improved tool safety and execution guidelines
    - refactor: implement tool execution retry logic and enhance error handling
    - refactor: enhance tool policy and prompt clarity
    - refactor: implement tool denial handling in agent runner
    - refactor: enhance McpAllowListConfig structure and improve rule definitions
    - refactor: improve code clarity and consistency in multiple modules
    - refactor: streamline conditional checks and improve code readability
    - refactor: unify token budget constants and improve context management
    - refactor: update tool policy and enhance tool validation
    - refactor: improve code formatting and structure across multiple files
    - refactor: implement API failure tracking with exponential backoff; optimize tool caching and navigation
    - refactor: optimize core agent execution and consolidate utility modules
    - refactor: introduce warning and error handling methods in AgentRunner; streamline tool failure logging and path normalization utilities
    - refactor: update model configurations to use OpenRouter for Moonshot models; remove deprecated entries and enhance model support
    - refactor: enhance reasoning model support and update tool policies; remove deprecated Moonshot models

### Documentation

    - docs: update changelog for v0.47.12 [skip ci]
    - docs: update changelog for v0.47.11 [skip ci]
    - docs: update changelog for v0.47.10 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.47.12 [skip ci]
    - chore: update npm package.json to v0.47.12 [skip ci]
    - chore: release v0.47.12
    - chore: release v0.47.11
    - chore: update dependencies and improve code formatting
    - chore: release v0.47.10
    - chore: update VSCode extension package.json to v0.47.9 [skip ci]
    - chore: update npm package.json to v0.47.9 [skip ci]

# [Version 0.47.12] - 2025-12-08

### Features

    - feat: update tool policy and enhance loop detection functionality
    - feat: implement tool call safety validation and execution tracking
    - feat(build): add multi-stage Dockerfile for building and running vtcode

### Refactors

    - refactor: update tool policy and enhance tool validation
    - refactor: improve code formatting and structure across multiple files
    - refactor: implement API failure tracking with exponential backoff; optimize tool caching and navigation
    - refactor: optimize core agent execution and consolidate utility modules
    - refactor: introduce warning and error handling methods in AgentRunner; streamline tool failure logging and path normalization utilities
    - refactor: update model configurations to use OpenRouter for Moonshot models; remove deprecated entries and enhance model support
    - refactor: enhance reasoning model support and update tool policies; remove deprecated Moonshot models
    - refactor: improve code readability by simplifying conditional statements and updating deprecated usages across multiple modules

### Documentation

    - docs: update changelog for v0.47.11 [skip ci]
    - docs: update changelog for v0.47.10 [skip ci]
    - docs: update changelog for v0.47.9 [skip ci]
    - docs: update changelog for v0.47.8 [skip ci]

### Chores

    - chore: release v0.47.11
    - chore: update dependencies and improve code formatting
    - chore: release v0.47.10
    - chore: update VSCode extension package.json to v0.47.9 [skip ci]
    - chore: update npm package.json to v0.47.9 [skip ci]
    - chore: release v0.47.9
    - chore: release v0.47.8
    - chore: update tool policies and optimize configuration loading
    - chore: update VSCode extension package.json to v0.47.7 [skip ci]
    - chore: update npm package.json to v0.47.7 [skip ci]

# [Version 0.47.11] - 2025-12-08

### Features

    - feat: update tool policy and enhance loop detection functionality
    - feat: implement tool call safety validation and execution tracking
    - feat(build): add multi-stage Dockerfile for building and running vtcode

### Refactors

    - refactor: update tool policy and enhance tool validation
    - refactor: improve code formatting and structure across multiple files
    - refactor: implement API failure tracking with exponential backoff; optimize tool caching and navigation
    - refactor: optimize core agent execution and consolidate utility modules
    - refactor: introduce warning and error handling methods in AgentRunner; streamline tool failure logging and path normalization utilities
    - refactor: update model configurations to use OpenRouter for Moonshot models; remove deprecated entries and enhance model support
    - refactor: enhance reasoning model support and update tool policies; remove deprecated Moonshot models
    - refactor: improve code readability by simplifying conditional statements and updating deprecated usages across multiple modules

### Documentation

    - docs: update changelog for v0.47.10 [skip ci]
    - docs: update changelog for v0.47.9 [skip ci]
    - docs: update changelog for v0.47.8 [skip ci]

### Chores

    - chore: update dependencies and improve code formatting
    - chore: release v0.47.10
    - chore: update VSCode extension package.json to v0.47.9 [skip ci]
    - chore: update npm package.json to v0.47.9 [skip ci]
    - chore: release v0.47.9
    - chore: release v0.47.8
    - chore: update tool policies and optimize configuration loading
    - chore: update VSCode extension package.json to v0.47.7 [skip ci]
    - chore: update npm package.json to v0.47.7 [skip ci]

# [Version 0.47.10] - 2025-12-07

### Features

    - feat(build): add multi-stage Dockerfile for building and running vtcode

### Refactors

    - refactor: optimize core agent execution and consolidate utility modules
    - refactor: introduce warning and error handling methods in AgentRunner; streamline tool failure logging and path normalization utilities
    - refactor: update model configurations to use OpenRouter for Moonshot models; remove deprecated entries and enhance model support
    - refactor: enhance reasoning model support and update tool policies; remove deprecated Moonshot models
    - refactor: improve code readability by simplifying conditional statements and updating deprecated usages across multiple modules

### Documentation

    - docs: update changelog for v0.47.9 [skip ci]
    - docs: update changelog for v0.47.8 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.47.9 [skip ci]
    - chore: update npm package.json to v0.47.9 [skip ci]
    - chore: release v0.47.9
    - chore: release v0.47.8
    - chore: update tool policies and optimize configuration loading
    - chore: update VSCode extension package.json to v0.47.7 [skip ci]
    - chore: update npm package.json to v0.47.7 [skip ci]

# [Version 0.47.9] - 2025-12-05

### Features

    - feat: update tool policies and improve code structure with dead code allowances
    - feat(build): add multi-stage Dockerfile for building and running vtcode

### Refactors

    - refactor: improve code readability by simplifying conditional statements and updating deprecated usages across multiple modules
    - refactor: remove unused tools from tool policies

### Documentation

    - docs: update changelog for v0.47.8 [skip ci]
    - docs: update changelog for v0.47.7 [skip ci]

### Chores

    - chore: release v0.47.8
    - chore: update tool policies and optimize configuration loading
    - chore: update VSCode extension package.json to v0.47.7 [skip ci]
    - chore: update npm package.json to v0.47.7 [skip ci]
    - chore: release v0.47.7
    - chore: update VSCode extension package.json to v0.47.6 [skip ci]
    - chore(deps): bump DavidAnson/markdownlint-cli2-action from 20 to 21
    - chore(deps): bump actions/checkout from 5 to 6
    - chore(deps): bump the all-rust-deps group with 15 updates
    - chore: update npm package.json to v0.47.6 [skip ci]

# [Version 0.47.8] - 2025-12-05

### Features

    - feat: update tool policies and improve code structure with dead code allowances
    - feat(build): add multi-stage Dockerfile for building and running vtcode

### Refactors

    - refactor: remove unused tools from tool policies

### Documentation

    - docs: update changelog for v0.47.7 [skip ci]

### Chores

    - chore: update tool policies and optimize configuration loading
    - chore: update VSCode extension package.json to v0.47.7 [skip ci]
    - chore: update npm package.json to v0.47.7 [skip ci]
    - chore: release v0.47.7
    - chore: update VSCode extension package.json to v0.47.6 [skip ci]
    - chore(deps): bump DavidAnson/markdownlint-cli2-action from 20 to 21
    - chore(deps): bump actions/checkout from 5 to 6
    - chore(deps): bump the all-rust-deps group with 15 updates
    - chore: update npm package.json to v0.47.6 [skip ci]

# [Version 0.47.7] - 2025-12-03

### Features

    - feat: update tool policies and improve code structure with dead code allowances
    - feat: add diff suppression logic and constants for large changes

### Performance Improvements

    - perf: optimize memory allocations and pre-allocate buffers in various modules
    - perf: use write! macro in metrics module
    - perf: use write! macro in llm/token_metrics
    - perf: use write! macro in exec modules and utils
    - perf: use write! macro in core token management modules
    - perf: use .to_string() instead of format! for context_size
    - perf: use write!/writeln! macros in tools and prompts modules
    - perf(ui): use write!/writeln! macros in diff_renderer
    - perf: optimize string formatting and use unwrap_or_default
    - perf: remove unnecessary clone() on Copy types
    - perf: use .to_string() directly for Display types instead of .as_str().to_string()
    - perf: eliminate redundant clones in config loader
    - perf: add Vec with_capacity for known-size allocations
    - perf(core): use write! macro instead of format! with push_str
    - perf(core): remove redundant clones and allocations

### Refactors

    - refactor: remove unused tools from tool policies
    - refactor: optimize completion learning modules with .into() patterns
    - refactor: optimize linting and code completion modules
    - refactor: optimize core modules for code quality and allocations
    - refactor: update message type handling and improve system prompt instructions
    - refactor(llm): extract serialize_messages_openai_format helper
    - refactor(llm): add validate_request_common helper
    - refactor(llm): add parse_tool_call and map_finish_reason helpers
    - refactor(llm): extract common provider helpers to reduce duplication
    - refactor: optimize diff rendering and suppression logic

### Documentation

    - docs: update changelog for v0.47.6 [skip ci]
    - docs: add comprehensive optimization report

### Style Changes

    - style: fix clippy warnings (assign_op, unnecessary_cast, collapsible_if, const thread_local)
    - style: remove redundant closures

### Chores

    - chore: update VSCode extension package.json to v0.47.6 [skip ci]
    - chore(deps): bump DavidAnson/markdownlint-cli2-action from 20 to 21
    - chore(deps): bump actions/checkout from 5 to 6
    - chore(deps): bump the all-rust-deps group with 15 updates
    - chore: update npm package.json to v0.47.6 [skip ci]
    - chore: release v0.47.6
    - chore: update VSCode extension package.json to v0.47.5 [skip ci]
    - chore: update npm package.json to v0.47.5 [skip ci]

# [Version 0.47.6] - 2025-11-30

### Features

    - feat: add diff suppression logic and constants for large changes
    - feat: parse and display friendly error messages from Anthropic API responses

### Bug Fixes

    - fix: clear spinner before displaying error message
    - fix: gracefully handle provider API errors without panicking

### Performance Improvements

    - perf: optimize memory allocations and pre-allocate buffers in various modules
    - perf: use write! macro in metrics module
    - perf: use write! macro in llm/token_metrics
    - perf: use write! macro in exec modules and utils
    - perf: use write! macro in core token management modules
    - perf: use .to_string() instead of format! for context_size
    - perf: use write!/writeln! macros in tools and prompts modules
    - perf(ui): use write!/writeln! macros in diff_renderer
    - perf: optimize string formatting and use unwrap_or_default
    - perf: remove unnecessary clone() on Copy types
    - perf: use .to_string() directly for Display types instead of .as_str().to_string()
    - perf: eliminate redundant clones in config loader
    - perf: add Vec with_capacity for known-size allocations
    - perf(core): use write! macro instead of format! with push_str
    - perf(core): remove redundant clones and allocations

### Refactors

    - refactor: optimize completion learning modules with .into() patterns
    - refactor: optimize linting and code completion modules
    - refactor: optimize core modules for code quality and allocations
    - refactor: update message type handling and improve system prompt instructions
    - refactor(llm): extract serialize_messages_openai_format helper
    - refactor(llm): add validate_request_common helper
    - refactor(llm): add parse_tool_call and map_finish_reason helpers
    - refactor(llm): extract common provider helpers to reduce duplication
    - refactor: optimize diff rendering and suppression logic
    - refactor: streamline code formatting and improve readability across multiple files

### Documentation

    - docs: add comprehensive optimization report
    - docs: clarify spinner cleanup implementation details
    - docs: update changelog for v0.47.5 [skip ci]
    - docs: update changes summary with comprehensive error handling improvements

### Style Changes

    - style: fix clippy warnings (assign_op, unnecessary_cast, collapsible_if, const thread_local)
    - style: remove redundant closures

### Chores

    - chore: update VSCode extension package.json to v0.47.5 [skip ci]
    - chore: update npm package.json to v0.47.5 [skip ci]
    - chore: release v0.47.5
    - chore: update VSCode extension package.json to v0.47.4 [skip ci]
    - chore: update npm package.json to v0.47.4 [skip ci]

# [Version 0.47.5] - 2025-11-25

### Features

    - feat: parse and display friendly error messages from Anthropic API responses

### Bug Fixes

    - fix: clear spinner before displaying error message
    - fix: gracefully handle provider API errors without panicking
    - fix: remove unused spawn_session import

### Refactors

    - refactor: streamline code formatting and improve readability across multiple files

### Documentation

    - docs: update changes summary with comprehensive error handling improvements
    - docs: update changelog for v0.47.4 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.47.4 [skip ci]
    - chore: update npm package.json to v0.47.4 [skip ci]
    - chore: release v0.47.4
    - chore: update VSCode extension package.json to v0.47.3 [skip ci]
    - chore: update npm package.json to v0.47.3 [skip ci]

# [Version 0.47.4] - 2025-11-25

### Bug Fixes

    - fix: remove unused spawn_session import

### Documentation

    - docs: update changelog for v0.47.3 [skip ci]
    - docs: update changelog for v0.47.2 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.47.3 [skip ci]
    - chore: update npm package.json to v0.47.3 [skip ci]
    - chore: release v0.47.3
    - chore: release v0.47.2
    - chore: update VSCode extension package.json to v0.47.1 [skip ci]
    - chore: update npm package.json to v0.47.1 [skip ci]

# [Version 0.47.3] - 2025-11-25

### Features

    - feat: Enhance tool policy with pre-approval allowlist, improve file operation error messages, and refine tool declarations.
    - feat: Integrate production-grade tool improvements system
    - feat: Implement animated thinking spinner for user input submission
    - feat: Add comprehensive ANSI escape sequence documentation and a new core utility module for ANSI codes.

### Bug Fixes

    - fix: correct RUSTFLAGS invalid option and align system prompt with actual tool definitions
    - fix: resolve all cargo clippy warnings and update rust toolchain to stable
    - fix: Revise thinking spinner message to use first-person agent voice
    - fix: Move thinking spinner display to after user message in transcript
    - fix: Clear thinking spinner message on all agent response command types

### Refactors

    - refactor: Update ThinkingSpinner struct visibility for better encapsulation
    - refactor: remove redundant reasoning handling, clarify intent

### Documentation

    - docs: update changelog for v0.47.2 [skip ci]
    - docs: update changelog for v0.47.1 [skip ci]
    - docs: update changelog for v0.47.0 [skip ci]

### Chores

    - chore: release v0.47.2
    - chore: update VSCode extension package.json to v0.47.1 [skip ci]
    - chore: update npm package.json to v0.47.1 [skip ci]
    - chore: release v0.47.1
    - chore: release v0.47.0
    - chore: update VSCode extension package.json to v0.46.0 [skip ci]
    - chore: update npm package.json to v0.46.0 [skip ci]

# [Version 0.47.2] - 2025-11-25

### Features

    - feat: Enhance tool policy with pre-approval allowlist, improve file operation error messages, and refine tool declarations.
    - feat: Integrate production-grade tool improvements system
    - feat: Implement animated thinking spinner for user input submission
    - feat: Add comprehensive ANSI escape sequence documentation and a new core utility module for ANSI codes.

### Bug Fixes

    - fix: correct RUSTFLAGS invalid option and align system prompt with actual tool definitions
    - fix: resolve all cargo clippy warnings and update rust toolchain to stable
    - fix: Revise thinking spinner message to use first-person agent voice
    - fix: Move thinking spinner display to after user message in transcript
    - fix: Clear thinking spinner message on all agent response command types

### Refactors

    - refactor: Update ThinkingSpinner struct visibility for better encapsulation
    - refactor: remove redundant reasoning handling, clarify intent

### Documentation

    - docs: update changelog for v0.47.1 [skip ci]
    - docs: update changelog for v0.47.0 [skip ci]

### Chores

    - chore: update VSCode extension package.json to v0.47.1 [skip ci]
    - chore: update npm package.json to v0.47.1 [skip ci]
    - chore: release v0.47.1
    - chore: release v0.47.0
    - chore: update VSCode extension package.json to v0.46.0 [skip ci]
    - chore: update npm package.json to v0.46.0 [skip ci]

# [Version 0.47.1] - 2025-11-23

### Features

    - feat: Enhance tool policy with pre-approval allowlist, improve file operation error messages, and refine tool declarations.
    - feat: Integrate production-grade tool improvements system
    - feat: Implement animated thinking spinner for user input submission
    - feat: Add comprehensive ANSI escape sequence documentation and a new core utility module for ANSI codes.
    - feat: Refactor install script, rename `run_pty_cmd` to `run_terminal_cmd`, and update installation instructions for Homebrew and NPM.

### Bug Fixes

    - fix: correct RUSTFLAGS invalid option and align system prompt with actual tool definitions
    - fix: resolve all cargo clippy warnings and update rust toolchain to stable
    - fix: Revise thinking spinner message to use first-person agent voice
    - fix: Move thinking spinner display to after user message in transcript
    - fix: Clear thinking spinner message on all agent response command types
    - fix: Update public re-export and documentation to reference file_helpers instead of legacy
    - fix: Improve `edit_file` tool's robustness

### Refactors

    - refactor: Update ThinkingSpinner struct visibility for better encapsulation
    - refactor: remove redundant reasoning handling, clarify intent
    - refactor: Rename `legacy` module to `file_helpers` and fix critical `edit_file` bugs related to newline handling, matching, and trailing newlines.
    - refactor(mcp): Clean up unused imports
    - refactor: Rename `run_terminal_cmd` to `run_pty_cmd` across documentation, examples, and tests.

### Documentation

    - docs: update changelog for v0.47.0 [skip ci]
    - docs: update changelog for v0.46.0 [skip ci]
    - docs/mcp: integrate DEPLOYMENT_GUIDE and update INDEX navigation
    - docs/mcp: Add lessons learned - project retrospective
    - docs/mcp: Add team communication kit - ready-to-use materials
    - docs/mcp: Add master index - 00_START_HERE.md
    - docs/mcp: Add implementation guides for immediate team use
    - docs/mcp: Add executive summary document
    - docs/mcp: Add team announcement document
    - docs: Link MCP module docs and add team guide
    - docs/mcp: Add comprehensive INDEX.md for navigation
    - docs/mcp: Complete documentation migration - consolidate and organize
    - docs: Update and expand documentation across various topics, add a new MCP diagnostic guide, and adjust project configurations and dependencies.

### Chores

    - chore: release v0.47.0
    - chore: update VSCode extension package.json to v0.46.0 [skip ci]
    - chore: update npm package.json to v0.46.0 [skip ci]
    - chore: release v0.46.0
    - chore: update VSCode extension package.json to v0.45.6 [skip ci]
    - chore: update npm package.json to v0.45.6 [skip ci]

# [Version 0.47.0] - 2025-11-23

### Features

    - feat: Enhance tool policy with pre-approval allowlist, improve file operation error messages, and refine tool declarations.
    - feat: Integrate production-grade tool improvements system
    - feat: Implement animated thinking spinner for user input submission
    - feat: Add comprehensive ANSI escape sequence documentation and a new core utility module for ANSI codes.
    - feat: Refactor install script, rename `run_pty_cmd` to `run_terminal_cmd`, and update installation instructions for Homebrew and NPM.

### Bug Fixes

    - fix: correct RUSTFLAGS invalid option and align system prompt with actual tool definitions
    - fix: resolve all cargo clippy warnings and update rust toolchain to stable
    - fix: Revise thinking spinner message to use first-person agent voice
    - fix: Move thinking spinner display to after user message in transcript
    - fix: Clear thinking spinner message on all agent response command types
    - fix: Update public re-export and documentation to reference file_helpers instead of legacy
    - fix: Improve `edit_file` tool's robustness

### Refactors

    - refactor: Update ThinkingSpinner struct visibility for better encapsulation
    - refactor: remove redundant reasoning handling, clarify intent
    - refactor: Rename `legacy` module to `file_helpers` and fix critical `edit_file` bugs related to newline handling, matching, and trailing newlines.
    - refactor(mcp): Clean up unused imports
    - refactor: Rename `run_terminal_cmd` to `run_pty_cmd` across documentation, examples, and tests.

### Documentation

    - docs: update changelog for v0.46.0 [skip ci]
    - docs/mcp: integrate DEPLOYMENT_GUIDE and update INDEX navigation
    - docs/mcp: Add lessons learned - project retrospective
    - docs/mcp: Add team communication kit - ready-to-use materials
    - docs/mcp: Add master index - 00_START_HERE.md
    - docs/mcp: Add implementation guides for immediate team use
    - docs/mcp: Add executive summary document
    - docs/mcp: Add team announcement document
    - docs: Link MCP module docs and add team guide
    - docs/mcp: Add comprehensive INDEX.md for navigation
    - docs/mcp: Complete documentation migration - consolidate and organize
    - docs: Update and expand documentation across various topics, add a new MCP diagnostic guide, and adjust project configurations and dependencies.

### Chores

    - chore: update VSCode extension package.json to v0.46.0 [skip ci]
    - chore: update npm package.json to v0.46.0 [skip ci]
    - chore: release v0.46.0
    - chore: update VSCode extension package.json to v0.45.6 [skip ci]
    - chore: update npm package.json to v0.45.6 [skip ci]

# [Version 0.46.0] - 2025-11-21

### Features

    - feat: Refactor install script, rename `run_pty_cmd` to `run_terminal_cmd`, and update installation instructions for Homebrew and NPM.
    - feat: Add new tools to tool-policy and update permissions for fetch and time providers
    - feat: Add default editor fallback (vi on Unix, notepad on Windows) when EDITOR/VISUAL not set
    - feat: Add external editor integration with TUI suspension, alternate screen handling, and stability improvements.
    - feat: update tool policies, add setup script, and enhance README with configuration details

### Bug Fixes

    - fix: Update public re-export and documentation to reference file_helpers instead of legacy
    - fix: Improve `edit_file` tool's robustness
    - fix: Track fire-and-forget tokio::spawn tasks with JoinHandles
    - fix: Apply Ratatui FAQ best practices - fix async/tokio issues
    - fix: Add environment() and path() to EditorBuilder to properly detect and launch editor
    - fix: Remove duplicate test block with non-existent method in zed.rs

### Refactors

    - refactor: Rename `legacy` module to `file_helpers` and fix critical `edit_file` bugs related to newline handling, matching, and trailing newlines.
    - refactor(mcp): Clean up unused imports
    - refactor: Rename `run_terminal_cmd` to `run_pty_cmd` across documentation, examples, and tests.
    - refactor: Remove static default editor, rely on try_common_editors for fallback

### Documentation

    - docs/mcp: integrate DEPLOYMENT_GUIDE and update INDEX navigation
    - docs/mcp: Add lessons learned - project retrospective
    - docs/mcp: Add team communication kit - ready-to-use materials
    - docs/mcp: Add master index - 00_START_HERE.md
    - docs/mcp: Add implementation guides for immediate team use
    - docs/mcp: Add executive summary document
    - docs/mcp: Add team announcement document
    - docs: Link MCP module docs and add team guide
    - docs/mcp: Add comprehensive INDEX.md for navigation
    - docs/mcp: Complete documentation migration - consolidate and organize
    - docs: Update and expand documentation across various topics, add a new MCP diagnostic guide, and adjust project configurations and dependencies.
    - docs: update changelog for v0.45.6 [skip ci]
    - docs: add comprehensive Ratatui improvements summary
    - docs: Add async improvements documentation
    - docs: add Ratatui FAQ integration summary document
    - docs: add Ratatui FAQ-based TUI best practices guides
    - docs: Add External Editor Configuration to docs index

### Chores

    - chore: update VSCode extension package.json to v0.45.6 [skip ci]
    - chore: update npm package.json to v0.45.6 [skip ci]
    - chore: release v0.45.6
    - chore: update VSCode extension package.json to v0.45.5 [skip ci]
    - chore: update npm package.json to v0.45.5 [skip ci]

# [Version 0.45.6] - 2025-11-20

### Features

    - feat: Add new tools to tool-policy and update permissions for fetch and time providers
    - feat: Add default editor fallback (vi on Unix, notepad on Windows) when EDITOR/VISUAL not set
    - feat: Add external editor integration with TUI suspension, alternate screen handling, and stability improvements.
    - feat: update tool policies, add setup script, and enhance README with configuration details
    - feat: VT Code System Prompt v3 - Context Optimized Implementation
    - feat: add Bash tool and remove non-existent run_pty_cmd references
    - feat: implement interactive tree UI for file structure visualization
    - feat: enhance diff display with full-width backgrounds and improve terminal command visibility

### Bug Fixes

    - fix: Track fire-and-forget tokio::spawn tasks with JoinHandles
    - fix: Apply Ratatui FAQ best practices - fix async/tokio issues
    - fix: Add environment() and path() to EditorBuilder to properly detect and launch editor
    - fix: Remove duplicate test block with non-existent method in zed.rs
    - fix: expose shell tool to LLM by setting expose_in_llm to true
    - fix: ensure development tools are always in PATH with fallback paths
    - fix: remove overly complex sandbox cache clearing on PTY retry
    - fix: improve loop detection for repeated tool calls
    - fix: sync embedded asset for generate-agent-file.md
    - fix: remove duplicate user message from conversation history
    - fix: remove duplicate user message in turn loop

### Refactors

    - refactor: Remove static default editor, rely on try_common_editors for fallback
    - refactor: Remove sandbox functionality and streamline shell command
    - refactor: eliminate wrapper layer in execute_shell_command
    - refactor: use pattern matching in execute_shell_command for clarity
    - refactor: simplify execute_shell_command further
    - refactor: dramatically simplify execute_shell_command
    - refactor: simplify execute_shell_command to skip conversion layer
    - refactor: rename bash to shell and mark run_pty_cmd as deprecated
    - refactor: streamline command execution error suggestions and implement unified run command executor
    - refactor: streamline loop detection logic and improve non-interactive handling

### Documentation

    - docs: add comprehensive Ratatui improvements summary
    - docs: Add async improvements documentation
    - docs: add Ratatui FAQ integration summary document
    - docs: add Ratatui FAQ-based TUI best practices guides
    - docs: Add External Editor Configuration to docs index
    - docs: update changelog for v0.45.5 [skip ci]
    - docs: Add implementation completion summary for System Prompt v3
    - docs: fix misleading comment for RUN_PTY_CMD constant
    - docs: add comprehensive PTY fix outcome report with complete analysis
    - docs: add comprehensive PTY shell initialization fix guide
    - docs: add PTY fix outcome report with validation and impact assessment
    - docs: add comprehensive PTY fix summary with problem analysis and solution validation
    - docs: update PTY command execution improvements documentation
    - docs: add embedded assets management guide and pre-commit hook

### Chores

    - chore: update VSCode extension package.json to v0.45.5 [skip ci]
    - chore: update npm package.json to v0.45.5 [skip ci]
    - chore: release v0.45.5
    - chore: standardize default shell in workflow files and set job timeouts
    - chore: update VSCode extension package.json to v0.45.4 and commit changes [skip ci]
    - chore: update npm package.json to v0.45.4 [skip ci]

# [Version 0.45.5] - 2025-11-19

### Features

    - feat: VT Code System Prompt v3 - Context Optimized Implementation
    - feat: add Bash tool and remove non-existent run_pty_cmd references
    - feat: implement interactive tree UI for file structure visualization
    - feat: enhance diff display with full-width backgrounds and improve terminal command visibility

### Bug Fixes

    - fix: expose shell tool to LLM by setting expose_in_llm to true
    - fix: ensure development tools are always in PATH with fallback paths
    - fix: remove overly complex sandbox cache clearing on PTY retry
    - fix: improve loop detection for repeated tool calls
    - fix: sync embedded asset for generate-agent-file.md
    - fix: remove duplicate user message from conversation history
    - fix: remove duplicate user message in turn loop
    - fix: suppress dead_code warnings for intentionally disabled features

### Refactors

    - refactor: Remove sandbox functionality and streamline shell command
    - refactor: eliminate wrapper layer in execute_shell_command
    - refactor: use pattern matching in execute_shell_command for clarity
    - refactor: simplify execute_shell_command further
    - refactor: dramatically simplify execute_shell_command
    - refactor: simplify execute_shell_command to skip conversion layer
    - refactor: rename bash to shell and mark run_pty_cmd as deprecated
    - refactor: streamline command execution error suggestions and implement unified run command executor
    - refactor: streamline loop detection logic and improve non-interactive handling
    - refactor: replace dissimilar with optimized Myers diff algorithm

### Documentation

    - docs: Add implementation completion summary for System Prompt v3
    - docs: fix misleading comment for RUN_PTY_CMD constant
    - docs: add comprehensive PTY fix outcome report with complete analysis
    - docs: add comprehensive PTY shell initialization fix guide
    - docs: add PTY fix outcome report with validation and impact assessment
    - docs: add comprehensive PTY fix summary with problem analysis and solution validation
    - docs: update PTY command execution improvements documentation
    - docs: add embedded assets management guide and pre-commit hook
    - docs: update changelog for v0.45.4 [skip ci]
    - docs: update AGENTS.md with comprehensive agent guide and tool usage guidelines

### Chores

    - chore: standardize default shell in workflow files and set job timeouts
    - chore: update VSCode extension package.json to v0.45.4 and commit changes [skip ci]
    - chore: update npm package.json to v0.45.4 [skip ci]
    - chore: release v0.45.4
    - chore: update npm package.json to v0.45.3 [skip ci]

# [Version 0.45.4] - 2025-11-17

### Bug Fixes

    - fix: suppress dead_code warnings for intentionally disabled features
    - fix: prevent infinite tool loops by using >= instead of >
    - fix: resolve clippy warnings (range_contains, doc comments, identical blocks)
    - fix: improve tool failure handling by tracking failed attempts
    - fix: truncate verbose reasoning output to reduce noise during tool execution
    - fix(llm): update lmstudio provider: remove stale 'For now' comment and simplify validation; update related utility and policy files

### Refactors

    - refactor: replace dissimilar with optimized Myers diff algorithm
    - refactor: organize documentation into docs/phases and docs/scroll subdirectories; consolidate PHASE5 and SCROLL artifacts for better maintainability

### Documentation

    - docs: update AGENTS.md with comprehensive agent guide and tool usage guidelines
    - docs: update changelog for v0.45.3 [skip ci]
    - docs: reorganize root-level docs into docs/ subdirectories per AGENTS.md
    - docs: clarify run_pty_cmd usage for git, cargo, and one-off shell commands

### Style Changes

    - style: apply cargo fmt

### Chores

    - chore: update npm package.json to v0.45.3 [skip ci]
    - chore: release v0.45.3
    - chore: update npm package.json to v0.45.2 [skip ci]

# [Version 0.45.3] - 2025-11-17

### Features

    - feat: implement token-based truncation for tool outputs and update configuration

### Bug Fixes

    - fix: prevent infinite tool loops by using >= instead of >
    - fix: resolve clippy warnings (range_contains, doc comments, identical blocks)
    - fix: improve tool failure handling by tracking failed attempts
    - fix: truncate verbose reasoning output to reduce noise during tool execution
    - fix(llm): update lmstudio provider: remove stale 'For now' comment and simplify validation; update related utility and policy files

### Refactors

    - refactor: organize documentation into docs/phases and docs/scroll subdirectories; consolidate PHASE5 and SCROLL artifacts for better maintainability

### Documentation

    - docs: reorganize root-level docs into docs/ subdirectories per AGENTS.md
    - docs: clarify run_pty_cmd usage for git, cargo, and one-off shell commands
    - docs: update changelog for v0.45.2 [skip ci]
    - docs: update changelog for v0.45.1 [skip ci]

### Style Changes

    - style: apply cargo fmt

### Chores

    - chore: update npm package.json to v0.45.2 [skip ci]
    - chore: release v0.45.2
    - chore: release v0.45.1
    - chore: update npm package.json to v0.45.0 [skip ci]

# [Version 0.45.2] - 2025-11-17

### Features

    - feat: implement token-based truncation for tool outputs and update configuration

### Refactors

    - refactor(runloop): extract tool pipeline into  and add  — reduce run loop complexity

### Documentation

    - docs: update changelog for v0.45.1 [skip ci]
    - docs: update changelog for v0.45.0 [skip ci]

### Chores

    - chore: release v0.45.1
    - chore: update npm package.json to v0.45.0 [skip ci]
    - chore: release v0.45.0
    - chore: update GitHub Actions workflows for improved performance and consistency; adjust dependency management and environment variables
    - chore(runloop): make session.rs minimal exposing slash_commands
    - chore(runloop): remove session.rs contents to extract run loop
    - chore(runloop): Extract run_single_agent_loop_unified to run_loop.rs
    - chore: update npm package.json to v0.44.1 [skip ci]

# [Version 0.45.1] - 2025-11-17

### Features

    - feat: implement token-based truncation for tool outputs and update configuration

### Refactors

    - refactor(runloop): extract tool pipeline into  and add  — reduce run loop complexity

### Documentation

    - docs: update changelog for v0.45.0 [skip ci]

### Chores

    - chore: update npm package.json to v0.45.0 [skip ci]
    - chore: release v0.45.0
    - chore: update GitHub Actions workflows for improved performance and consistency; adjust dependency management and environment variables
    - chore(runloop): make session.rs minimal exposing slash_commands
    - chore(runloop): remove session.rs contents to extract run loop
    - chore(runloop): Extract run_single_agent_loop_unified to run_loop.rs
    - chore: update npm package.json to v0.44.1 [skip ci]

# [Version 0.45.0] - 2025-11-16

### Bug Fixes

    - fix: update Claude model identifiers and descriptions for accuracy

### Refactors

    - refactor(runloop): extract tool pipeline into  and add  — reduce run loop complexity

### Documentation

    - docs: update changelog for v0.44.1 [skip ci]
    - docs: update changelog for v0.44.0 [skip ci]

### Chores

    - chore: update GitHub Actions workflows for improved performance and consistency; adjust dependency management and environment variables
    - chore(runloop): make session.rs minimal exposing slash_commands
    - chore(runloop): remove session.rs contents to extract run loop
    - chore(runloop): Extract run_single_agent_loop_unified to run_loop.rs
    - chore: update npm package.json to v0.44.1 [skip ci]
    - chore: release v0.44.1
    - chore: release v0.44.0
    - chore: update configuration files for VT Code support
    - chore: update npm package.json to v0.43.17 [skip ci]

# [Version 0.44.1] - 2025-11-15

### Bug Fixes

    - fix: update Claude model identifiers and descriptions for accuracy

### Documentation

    - docs: update changelog for v0.44.0 [skip ci]
    - docs: update changelog for v0.43.17 [skip ci]
    - docs: update changelog for v0.43.16 [skip ci]

### Chores

    - chore: release v0.44.0
    - chore: update configuration files for VT Code support
    - chore: update npm package.json to v0.43.17 [skip ci]
    - chore: release v0.43.17
    - chore: release v0.43.16
    - chore: update npm package.json to v0.43.15 [skip ci]

# [Version 0.44.0] - 2025-11-15

### Documentation

    - docs: update changelog for v0.43.17 [skip ci]
    - docs: update changelog for v0.43.16 [skip ci]

### Chores

    - chore: update configuration files for VT Code support
    - chore: update npm package.json to v0.43.17 [skip ci]
    - chore: release v0.43.17
    - chore: release v0.43.16
    - chore: update npm package.json to v0.43.15 [skip ci]

# [Version 0.43.17] - 2025-11-15

### Documentation

    - docs: update changelog for v0.43.16 [skip ci]
    - docs: update changelog for v0.43.15 [skip ci]
    - docs: update changelog for v0.43.14 [skip ci]
    - docs: update changelog for v0.43.13 [skip ci]
    - docs: update changelog for v0.43.12 [skip ci]

### Chores

    - chore: release v0.43.16
    - chore: update npm package.json to v0.43.15 [skip ci]
    - chore: release v0.43.15
    - chore: release v0.43.14
    - chore: release v0.43.13
    - chore: release v0.43.12
    - chore: update npm package.json to v0.43.11 [skip ci]

### Features

    - feat(openai): add `prompt_cache_retention` option in vtcode.toml to control Responses API cache retention (e.g., "24h")

# [Version 0.43.16] - 2025-11-15

### Documentation

    - docs: update changelog for v0.43.15 [skip ci]
    - docs: update changelog for v0.43.14 [skip ci]
    - docs: update changelog for v0.43.13 [skip ci]
    - docs: update changelog for v0.43.12 [skip ci]

### Chores

    - chore: update npm package.json to v0.43.15 [skip ci]
    - chore: release v0.43.15
    - chore: release v0.43.14
    - chore: release v0.43.13
    - chore: release v0.43.12
    - chore: update npm package.json to v0.43.11 [skip ci]

# [Version 0.43.15] - 2025-11-14

### Documentation

    - docs: update changelog for v0.43.14 [skip ci]
    - docs: update changelog for v0.43.13 [skip ci]
    - docs: update changelog for v0.43.12 [skip ci]
    - docs: update changelog for v0.43.11 [skip ci]

### Chores

    - chore: release v0.43.14
    - chore: release v0.43.13
    - chore: release v0.43.12
    - chore: update npm package.json to v0.43.11 [skip ci]
    - chore: release v0.43.11
    - chore: update npm package.json to v0.43.10 [skip ci]

# [Version 0.43.14] - 2025-11-14

### Documentation

    - docs: update changelog for v0.43.13 [skip ci]
    - docs: update changelog for v0.43.12 [skip ci]
    - docs: update changelog for v0.43.11 [skip ci]

### Chores

    - chore: release v0.43.13
    - chore: release v0.43.12
    - chore: update npm package.json to v0.43.11 [skip ci]
    - chore: release v0.43.11
    - chore: update npm package.json to v0.43.10 [skip ci]

# [Version 0.43.13] - 2025-11-14

### Documentation

    - docs: update changelog for v0.43.12 [skip ci]
    - docs: update changelog for v0.43.11 [skip ci]

### Chores

    - chore: release v0.43.12
    - chore: update npm package.json to v0.43.11 [skip ci]
    - chore: release v0.43.11
    - chore: update npm package.json to v0.43.10 [skip ci]

# [Version 0.43.12] - 2025-11-14

### Documentation

    - docs: update changelog for v0.43.11 [skip ci]

### Chores

    - chore: update npm package.json to v0.43.11 [skip ci]
    - chore: release v0.43.11
    - chore: update npm package.json to v0.43.10 [skip ci]

# [Version 0.43.11] - 2025-11-13

### Documentation

    - docs: update changelog for v0.43.10 [skip ci]
    - docs: update changelog for v0.43.9 [skip ci]

### Chores

    - chore: update npm package.json to v0.43.10 [skip ci]
    - chore: release v0.43.10
    - chore: release v0.43.9
    - chore: update npm package.json to v0.43.8 [skip ci]

# [Version 0.43.10] - 2025-11-13

### Features

    - feat(ripgrep): Add automatic installation and management for ripgrep dependency
    - feat(loop_detection): Refactor loop hang detection for improved accuracy and user experience

### Documentation

    - docs: update changelog for v0.43.9 [skip ci]
    - docs: update changelog for v0.43.8 [skip ci]
    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.7 [skip ci]

### Chores

    - chore: release v0.43.9
    - chore: update npm package.json to v0.43.8 [skip ci]
    - chore: release v0.43.8
    - chore: release v0.43.7
    - chore: update npm package.json to v0.43.6 [skip ci]

# [Version 0.43.9] - 2025-11-13

### Features

    - feat(ripgrep): Add automatic installation and management for ripgrep dependency
    - feat(loop_detection): Refactor loop hang detection for improved accuracy and user experience

### Documentation

    - docs: update changelog for v0.43.8 [skip ci]
    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.7 [skip ci]

### Chores

    - chore: update npm package.json to v0.43.8 [skip ci]
    - chore: release v0.43.8
    - chore: release v0.43.7
    - chore: update npm package.json to v0.43.6 [skip ci]

# [Version 0.43.8] - 2025-11-13

### Features

    - feat(ripgrep): Add automatic installation and management for ripgrep dependency
    - feat(loop_detection): Refactor loop hang detection for improved accuracy and user experience
    - feat(web_fetch): Introduce Web Fetch tool with security configurations
    - feat: Implement token-based truncation for tool output rendering
    - feat: Enhance command execution with additional PATH entries and environment variable handling

### Refactors

    - refactor: replace cargo_bin_cmd with assert_cmd in CLI tests and simplify InlineTextStyle initialization
    - refactor: update InlineTextStyle to include bg_color and effects in snapshot tests
    - refactor: update command execution in tests and remove unused imports

### Documentation

    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.6 [skip ci]
    - docs: Add truncation audit and remove unused terminal output line-limit constants

### Chores

    - chore: release v0.43.7
    - chore: update npm package.json to v0.43.6 [skip ci]
    - chore: release v0.43.6
    - chore: update npm package.json to v0.43.5 [skip ci]

# [Version 0.43.7] - 2025-11-13

### Features

    - feat(ripgrep): Add automatic installation and management for ripgrep dependency
    - feat(loop_detection): Refactor loop hang detection for improved accuracy and user experience
    - feat(web_fetch): Introduce Web Fetch tool with security configurations
    - feat: Implement token-based truncation for tool output rendering
    - feat: Enhance command execution with additional PATH entries and environment variable handling

### Refactors

    - refactor: replace cargo_bin_cmd with assert_cmd in CLI tests and simplify InlineTextStyle initialization
    - refactor: update InlineTextStyle to include bg_color and effects in snapshot tests
    - refactor: update command execution in tests and remove unused imports

### Documentation

    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.6 [skip ci]
    - docs: Add truncation audit and remove unused terminal output line-limit constants

### Chores

    - chore: update npm package.json to v0.43.6 [skip ci]
    - chore: release v0.43.6
    - chore: update npm package.json to v0.43.5 [skip ci]

# [Version 0.43.7] - 2025-11-13

### Features

    - feat(ripgrep): Add automatic installation and management for ripgrep dependency
    - feat(loop_detection): Refactor loop hang detection for improved accuracy and user experience
    - feat(web_fetch): Introduce Web Fetch tool with security configurations
    - feat: Implement token-based truncation for tool output rendering
    - feat: Enhance command execution with additional PATH entries and environment variable handling

### Refactors

    - refactor: replace cargo_bin_cmd with assert_cmd in CLI tests and simplify InlineTextStyle initialization
    - refactor: update InlineTextStyle to include bg_color and effects in snapshot tests
    - refactor: update command execution in tests and remove unused imports

### Documentation

    - docs: update changelog for v0.43.7 [skip ci]
    - docs: update changelog for v0.43.6 [skip ci]
    - docs: Add truncation audit and remove unused terminal output line-limit constants

### Chores

    - chore: update npm package.json to v0.43.6 [skip ci]
    - chore: release v0.43.6
    - chore: update npm package.json to v0.43.5 [skip ci]

# [Version 0.43.7] - 2025-11-13

### Features

    - feat(ripgrep): Add automatic installation and management for ripgrep dependency
    - feat(loop_detection): Refactor loop hang detection for improved accuracy and user experience
    - feat(web_fetch): Introduce Web Fetch tool with security configurations
    - feat: Implement token-based truncation for tool output rendering
    - feat: Enhance command execution with additional PATH entries and environment variable handling

### Refactors

    - refactor: replace cargo_bin_cmd with assert_cmd in CLI tests and simplify InlineTextStyle initialization
    - refactor: update InlineTextStyle to include bg_color and effects in snapshot tests
    - refactor: update command execution in tests and remove unused imports

### Documentation

    - docs: update changelog for v0.43.6 [skip ci]
    - docs: Add truncation audit and remove unused terminal output line-limit constants

### Chores

    - chore: update npm package.json to v0.43.6 [skip ci]
    - chore: release v0.43.6
    - chore: update npm package.json to v0.43.5 [skip ci]

# [Version 0.43.6] - 2025-11-12

### Features

    - feat(web_fetch): Introduce Web Fetch tool with security configurations
    - feat: Implement token-based truncation for tool output rendering
    - feat: Enhance command execution with additional PATH entries and environment variable handling

### Refactors

    - refactor: replace cargo_bin_cmd with assert_cmd in CLI tests and simplify InlineTextStyle initialization
    - refactor: update InlineTextStyle to include bg_color and effects in snapshot tests
    - refactor: update command execution in tests and remove unused imports

### Documentation

    - docs: Add truncation audit and remove unused terminal output line-limit constants
    - docs: update changelog for v0.43.5 [skip ci]

### Chores

    - chore: update npm package.json to v0.43.5 [skip ci]
    - chore: release v0.43.5
    - chore: update npm package.json to v0.43.4 [skip ci]

# [Version 0.43.5] - 2025-11-11

### Features

    - feat: Implement permission system with command resolution, audit logging, and caching

### Improvements

    - improve: Enhanced token approximation algorithm with median-based heuristics for fallback tokenization
    - improve: Fixed token counting fallback to use consistent 3.5 chars/token ratio across head/tail sections
    - improve: Eliminated async token counting overhead by using fast character-based fallback estimation
    - improve: Optimized tail content collection from O(n²) string operations to O(n) with Vec collection
    - improve: Added String pre-allocation with capacity to reduce memory allocations during truncation
    - improve: Improved median-based token estimation to handle edge cases (zero word count, whitespace-heavy content)
    - improve: Optimized result assembly with in-place string building and size pre-calculation
    - improve: Increased code fence block display limit from 200 → 500 lines with better truncation messaging
    - improve: Increased diff preview display limit from 300 → 500 lines with improved user guidance
    - improve: Added comprehensive module-level documentation for token-aware truncation strategy
    - improve: Clarified token budget messaging to users about what content is preserved
    - docs: Added TRUNCATION_IMPROVEMENTS.md explaining token-based truncation design and enhancements

### Refactors

    - refactor: Remove unused audit log and history navigation methods
    - refactor: Remove references to ast_grep_search from documentation and tool policies
    - refactor: Phase 2 Step 4 - migrate remaining input methods and word navigation
    - refactor: Phase 2 Step 3 - migrate clear_input() and reset_history_navigation()
    - refactor: Phase 2 Step 2 - add manager sync helper methods
    - refactor: Phase 2 Step 1 - add manager fields to Session struct
    - refactor: extract input history navigation logic fix in InputManager

### Documentation

    - docs: update changelog for v0.43.4 [skip ci]
    - docs: update Phase 2 progress - Step 4 complete with all input methods migrated
    - docs: add VT Code execution policy documentation and update command validation

### Chores

    - chore: update npm package.json to v0.43.4 [skip ci]
    - chore: release v0.43.4
    - chore: update npm package.json to v0.43.3 [skip ci]

# [Version 0.43.4] - 2025-11-10

### Features

    - feat: Implement permission system with command resolution, audit logging, and caching
    - feat: Add Git color configuration support and theme management
    - feat: Add Styling Quick Start Guide and Refactor Completion Report
    - feat: add theme_parser module for Git/LS_COLORS configuration parsing
    - feat: complete phase 1 anstyle integration - effects and background colors
    - feat: Integrate anstyle-parse for ANSI escape sequence handling
    - feat: implement styling refactor - centralize color palettes and style helpers

### Bug Fixes

    - fix: redirect logging to stderr to prevent stdout pollution in install script

### Refactors

    - refactor: Remove unused audit log and history navigation methods
    - refactor: Remove references to ast_grep_search from documentation and tool policies
    - refactor: Phase 2 Step 4 - migrate remaining input methods and word navigation
    - refactor: Phase 2 Step 3 - migrate clear_input() and reset_history_navigation()
    - refactor: Phase 2 Step 2 - add manager sync helper methods
    - refactor: Phase 2 Step 1 - add manager fields to Session struct
    - refactor: extract input history navigation logic fix in InputManager
    - refactor(styling): implement central style helpers and diff color palette
    - refactor: improve styling consistency with bold_color() and ColorPalette
    - refactor: implement styling suggestions from STYLING_REFACTOR_GUIDE
    - refactor: implement styling refactor from guide - centralize color/style management

### Documentation

    - docs: update Phase 2 progress - Step 4 complete with all input methods migrated
    - docs: add VT Code execution policy documentation and update command validation
    - docs: update changelog for v0.43.3 [skip ci]
    - docs: add comprehensive styling documentation index
    - docs: add Phase 2 planning and implementation guides for advanced styling features
    - docs: add session summary for phase 1 styling integration completion
    - docs: add phase 1 completion summary - all criteria met
    - docs: add styling implementation completion status
    - docs: add styling implementation completion status
    - docs: update installation guides with CDN caching troubleshooting and fix details

### Chores

    - chore: update npm package.json to v0.43.3 [skip ci]
    - chore: release v0.43.3
    - chore: update install script to log messages to stderr and bump version to 0.43.2
    - chore: update npm package.json to v0.43.2 [skip ci]

# [Version 0.43.3] - 2025-11-09

### Features

    - feat: Add Git color configuration support and theme management
    - feat: Add Styling Quick Start Guide and Refactor Completion Report
    - feat: add theme_parser module for Git/LS_COLORS configuration parsing
    - feat: complete phase 1 anstyle integration - effects and background colors
    - feat: Integrate anstyle-parse for ANSI escape sequence handling
    - feat: implement styling refactor - centralize color palettes and style helpers

### Bug Fixes

    - fix: redirect logging to stderr to prevent stdout pollution in install script
    - fix: optimize list_files tool for improved pagination and reduce default page size
    - fix: update LLM provider and models to use Ollama
    - fix: revert extension.toml to valid Zed format

### Refactors

    - refactor(styling): implement central style helpers and diff color palette
    - refactor: improve styling consistency with bold_color() and ColorPalette
    - refactor: implement styling suggestions from STYLING_REFACTOR_GUIDE
    - refactor: implement styling refactor from guide - centralize color/style management
    - refactor: integrate CommandBuilder throughout commands module
    - refactor: restructure zed-extension to modular architecture with comprehensive error handling and caching

### Documentation

    - docs: add comprehensive styling documentation index
    - docs: add Phase 2 planning and implementation guides for advanced styling features
    - docs: add session summary for phase 1 styling integration completion
    - docs: add phase 1 completion summary - all criteria met
    - docs: add styling implementation completion status
    - docs: add styling implementation completion status
    - docs: update installation guides with CDN caching troubleshooting and fix details
    - docs: update changelog for v0.43.2 [skip ci]
    - docs: add file listing output behavior pattern to AGENTS.md
    - docs: add comprehensive final improvements summary
    - docs: update STATUS with improvements session results

### Chores

    - chore: update install script to log messages to stderr and bump version to 0.43.2
    - chore: update npm package.json to v0.43.2 [skip ci]
    - chore: release v0.43.2
    - chore: update npm package.json to v0.43.1 [skip ci]

# [Version 0.43.2] - 2025-11-09

### Bug Fixes

    - fix: optimize list_files tool for improved pagination and reduce default page size
    - fix: update LLM provider and models to use Ollama
    - fix: revert extension.toml to valid Zed format

### Refactors

    - refactor: integrate CommandBuilder throughout commands module
    - refactor: restructure zed-extension to modular architecture with comprehensive error handling and caching

### Documentation

    - docs: add file listing output behavior pattern to AGENTS.md
    - docs: add comprehensive final improvements summary
    - docs: update STATUS with improvements session results
    - docs: update changelog for v0.43.1 [skip ci]
    - docs: Add release readiness confirmation document
    - docs: Add release action checklist for v0.43.0
    - docs: Add comprehensive v0.43.0 release summary

### Chores

    - chore: update npm package.json to v0.43.1 [skip ci]
    - chore: release v0.43.1

# [Version 0.43.1] - 2025-11-09

### Features

    - feat: Implement Agent Communication Protocol (ACP) integration

### Documentation

    - docs: Add release readiness confirmation document
    - docs: Add release action checklist for v0.43.0
    - docs: Add comprehensive v0.43.0 release summary
    - docs: Update ACP implementation summary and usage patterns
    - docs: Add ACP next steps and release checklist
    - docs: Add ACP implementation completion summary

### Chores

    - chore: release v0.43.0
    - chore: bump version to 0.43.0 for ACP release
    - chore: update npm package.json to v0.42.20 [skip ci]

### Features

    - feat: Implement Agent Communication Protocol (ACP) integration for multi-agent orchestration
    - feat: Add ACP client with sync/async RPC methods
    - feat: Implement agent discovery and registry system
    - feat: Add type-safe message protocol with correlation ID tracking
    - feat: Create MCP tools: acp_call, acp_discover, acp_health for agent communication
    - feat: Integrate ACP with Zed editor for terminal command execution
    - feat: Support distributed agent workflows via HTTP-based RPC

### Documentation

    - docs: Add comprehensive ACP integration guide
    - docs: Add ACP quick reference for developers
    - docs: Add ACP client API documentation and examples
    - docs: Add implementation completion summary
    - docs: Add release checklist and next steps guide

### Testing

    - test: Add full test coverage for ACP client (6 unit tests)
    - test: Add ACP tool integration tests
    - test: Add distributed workflow example

## [Version 0.42.20] - 2025-11-09

### Features

    - feat: Implement tool approval dialog with enhanced UX and risk assessment
    - feat: Step 8 - Implement tool versioning and compatibility checking
    - feat: Step 7 - Observability & Metrics system for MCP execution
    - feat: implement all 5 MCP code execution steps from Anthropic recommendations
    - feat: Step 2 Phase 2 - IPC handler integration for tool invocation
    - feat: Step 2 - Code executor with SDK generation and IPC

### Bug Fixes

    - fix: Resolve compilation warnings and duplicate test module
    - fix: remove unused import in code_executor

### Documentation

    - docs: update changelog for v0.42.19 [skip ci]
    - docs: update changelog for v0.42.18 [skip ci]
    - docs: add comprehensive tool configuration status document
    - docs: add agent prompt optimization summary
    - docs: Add MCP quick reference guide for fast lookup
    - docs: Add comprehensive MCP implementation status report
    - docs: Complete 9-step MCP code execution roadmap with Steps 8-9 designs
    - docs: Add Step 6 integration testing guide and test scenarios
    - docs: Update Step 2 completion status and add SDK examples

### Chores

    - chore: update npm package.json to v0.42.19 [skip ci]
    - chore: release v0.42.19
    - chore: release v0.42.18
    - chore: finalize tool configuration and system prompt updates
    - chore: update npm package.json to v0.42.17 [skip ci]

# [Version 0.42.19] - 2025-11-08

### Features

    - feat: Step 8 - Implement tool versioning and compatibility checking
    - feat: Step 7 - Observability & Metrics system for MCP execution
    - feat: implement all 5 MCP code execution steps from Anthropic recommendations
    - feat: Step 2 Phase 2 - IPC handler integration for tool invocation
    - feat: Step 2 - Code executor with SDK generation and IPC
    - feat: Add comprehensive timeout implementation summary and configuration details
    - feat: Implement configurable MCP initialization and tool execution timeouts
    - feat: Add OpenRouter Interleaved Thinking Implementation Plan and Quick Reference

### Bug Fixes

    - fix: Resolve compilation warnings and duplicate test module
    - fix: remove unused import in code_executor

### Documentation

    - docs: update changelog for v0.42.18 [skip ci]
    - docs: add comprehensive tool configuration status document
    - docs: add agent prompt optimization summary
    - docs: Add MCP quick reference guide for fast lookup
    - docs: Add comprehensive MCP implementation status report
    - docs: Complete 9-step MCP code execution roadmap with Steps 8-9 designs
    - docs: Add Step 6 integration testing guide and test scenarios
    - docs: Update Step 2 completion status and add SDK examples
    - docs: update changelog for v0.42.17 [skip ci]

### Chores

    - chore: release v0.42.18
    - chore: finalize tool configuration and system prompt updates
    - chore: update npm package.json to v0.42.17 [skip ci]
    - chore: release v0.42.17
    - chore: update documentation and code structure for clarity
    - chore: update npm package.json to v0.42.16 [skip ci]

# [Version 0.42.18] - 2025-11-08

### Features

    - feat: Step 8 - Implement tool versioning and compatibility checking
    - feat: Step 7 - Observability & Metrics system for MCP execution
    - feat: implement all 5 MCP code execution steps from Anthropic recommendations
    - feat: Step 2 Phase 2 - IPC handler integration for tool invocation
    - feat: Step 2 - Code executor with SDK generation and IPC
    - feat: Add comprehensive timeout implementation summary and configuration details
    - feat: Implement configurable MCP initialization and tool execution timeouts
    - feat: Add OpenRouter Interleaved Thinking Implementation Plan and Quick Reference

### Bug Fixes

    - fix: Resolve compilation warnings and duplicate test module
    - fix: remove unused import in code_executor

### Documentation

    - docs: add comprehensive tool configuration status document
    - docs: add agent prompt optimization summary
    - docs: Add MCP quick reference guide for fast lookup
    - docs: Add comprehensive MCP implementation status report
    - docs: Complete 9-step MCP code execution roadmap with Steps 8-9 designs
    - docs: Add Step 6 integration testing guide and test scenarios
    - docs: Update Step 2 completion status and add SDK examples
    - docs: update changelog for v0.42.17 [skip ci]

### Chores

    - chore: finalize tool configuration and system prompt updates
    - chore: update npm package.json to v0.42.17 [skip ci]
    - chore: release v0.42.17
    - chore: update documentation and code structure for clarity
    - chore: update npm package.json to v0.42.16 [skip ci]

# [Version 0.42.17] - 2025-11-08

### Features

    - feat: Add comprehensive timeout implementation summary and configuration details
    - feat: Implement configurable MCP initialization and tool execution timeouts
    - feat: Add OpenRouter Interleaved Thinking Implementation Plan and Quick Reference

### Bug Fixes

    - fix: update tool policies and disable time provider in configuration

### Documentation

    - docs: update changelog for v0.42.16 [skip ci]

### Chores

    - chore: update documentation and code structure for clarity
    - chore: update npm package.json to v0.42.16 [skip ci]
    - chore: release v0.42.16
    - chore: update npm package.json to v0.42.15 [skip ci]

# [Version 0.42.16] - 2025-11-08

### Bug Fixes

    - fix: update tool policies and disable time provider in configuration

### Documentation

    - docs: update changelog for v0.42.15 [skip ci]

### Chores

    - chore: update npm package.json to v0.42.15 [skip ci]
    - chore: release v0.42.15
    - chore: update package name and publishing instructions for npmjs.com and GitHub Packages
    - chore: update npm package.json to v0.42.14 [skip ci]

# [Version 0.42.15] - 2025-11-08

### Bug Fixes

    - fix: revert version in package.json to 0.42.13

### Documentation

    - docs: update changelog for v0.42.14 [skip ci]

### Chores

    - chore: update package name and publishing instructions for npmjs.com and GitHub Packages
    - chore: update npm package.json to v0.42.14 [skip ci]
    - chore: release v0.42.14

# [Version 0.42.14] - 2025-11-08

### Features

    - feat: Remove deprecated tool and add test_tool to policy
    - feat: Implement NPM package publishing for VT Code
    - feat: add configurable LLM generation parameters in vtcode.toml

### Bug Fixes

    - fix: revert version in package.json to 0.42.13

### Documentation

    - docs: update changelog for v0.42.13 [skip ci]

### Chores

    - chore: release v0.42.13

# [Version 0.42.13] - 2025-11-08

### Features

    - feat: Remove deprecated tool and add test_tool to policy
    - feat: Implement NPM package publishing for VT Code
    - feat: add configurable LLM generation parameters in vtcode.toml

### Bug Fixes

    - fix: redirect print functions to stderr to avoid command substitution issues
    - fix: use temporary file approach with awk for changelog updates on macOS
    - fix: use perl instead of awk for changelog updates on macOS
    - fix: use awk instead of sed for changelog updates on macOS
    - fix: escape newlines properly in sed command for macOS

### Refactors

    - refactor: update tool policies and improve MCP tool handling

### Documentation

    - docs: update changelog for v0.42.12 [skip ci]

### Chores

    - chore: release v0.42.12
    - chore: release vscode extension v0.42.18
    - chore: release vscode extension v0.42.17
    - chore: release vscode extension v0.42.16

# [Version 0.42.12] - 2025-11-08

### Features

    - feat: update vtcode.toml configuration for new model provider

### Bug Fixes

    - fix: redirect print functions to stderr to avoid command substitution issues
    - fix: use temporary file approach with awk for changelog updates on macOS
    - fix: use perl instead of awk for changelog updates on macOS
    - fix: use awk instead of sed for changelog updates on macOS
    - fix: escape newlines properly in sed command for macOS
    - fix: update Moonshot model references from KIMI_K2_THINKING_HEAVY to KIMI_K2_THINKING_TURBO

### Refactors

    - refactor: update tool policies and improve MCP tool handling

### Documentation

    - docs: update changelog for v0.42.11 [skip ci]
    - docs: update changelog for v0.42.10 [skip ci]

### Chores

    - chore: release vscode extension v0.42.18
    - chore: release vscode extension v0.42.17
    - chore: release vscode extension v0.42.16
    - chore: release v0.42.11
    - chore: release v0.42.10

# [Version 0.42.11] - 2025-11-07

### Features

    - feat: update vtcode.toml configuration for new model provider
    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: update Moonshot model references from KIMI_K2_THINKING_HEAVY to KIMI_K2_THINKING_TURBO
    - fix: add Debug trait to MessageStyle enum
    - fix: remove jsonschema dependency from mcp-types in Cargo.lock
    - fix: remove mcp-types configuration from release.toml
    - fix: ensure publish is set to false for mcp-types in release.toml
    - fix: update mcp-types version to 0.1.1 in Cargo.lock
    - fix: update mcp-types version to 0.1.1 in Cargo.toml
    - fix: ensure publish is set to false in Cargo.toml
    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Refactors

    - refactor: remove unused app constant and update elicitation capability handling

### Documentation

    - docs: update changelog for v0.42.10 [skip ci]
    - docs: update changelog for v0.42.9 [skip ci]
    - docs: update changelog for v0.42.8 [skip ci]
    - docs: update changelog for v0.42.7 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.5 [skip ci]
    - docs: update changelog for v0.42.4 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]

### Chores

    - chore: release v0.42.10
    - chore: release v0.42.9
    - chore: release v0.42.8
    - chore: release v0.42.7
    - chore: release v0.42.6
    - chore: release v0.42.5
    - chore: release v0.42.4
    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0

# [Version 0.42.10] - 2025-11-07

### Features

    - feat: update vtcode.toml configuration for new model provider
    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: add Debug trait to MessageStyle enum
    - fix: remove jsonschema dependency from mcp-types in Cargo.lock
    - fix: remove mcp-types configuration from release.toml
    - fix: ensure publish is set to false for mcp-types in release.toml
    - fix: update mcp-types version to 0.1.1 in Cargo.lock
    - fix: update mcp-types version to 0.1.1 in Cargo.toml
    - fix: ensure publish is set to false in Cargo.toml
    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Refactors

    - refactor: remove unused app constant and update elicitation capability handling

### Documentation

    - docs: update changelog for v0.42.9 [skip ci]
    - docs: update changelog for v0.42.8 [skip ci]
    - docs: update changelog for v0.42.7 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.5 [skip ci]
    - docs: update changelog for v0.42.4 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]

### Chores

    - chore: release v0.42.9
    - chore: release v0.42.8
    - chore: release v0.42.7
    - chore: release v0.42.6
    - chore: release v0.42.5
    - chore: release v0.42.4
    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0

# [Version 0.42.9] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: add Debug trait to MessageStyle enum
    - fix: remove jsonschema dependency from mcp-types in Cargo.lock
    - fix: remove mcp-types configuration from release.toml
    - fix: ensure publish is set to false for mcp-types in release.toml
    - fix: update mcp-types version to 0.1.1 in Cargo.lock
    - fix: update mcp-types version to 0.1.1 in Cargo.toml
    - fix: ensure publish is set to false in Cargo.toml
    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Refactors

    - refactor: remove unused app constant and update elicitation capability handling

### Documentation

    - docs: update changelog for v0.42.8 [skip ci]
    - docs: update changelog for v0.42.7 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.5 [skip ci]
    - docs: update changelog for v0.42.4 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.8
    - chore: release v0.42.7
    - chore: release v0.42.6
    - chore: release v0.42.5
    - chore: release v0.42.4
    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.8] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: add Debug trait to MessageStyle enum
    - fix: remove jsonschema dependency from mcp-types in Cargo.lock
    - fix: remove mcp-types configuration from release.toml
    - fix: ensure publish is set to false for mcp-types in release.toml
    - fix: update mcp-types version to 0.1.1 in Cargo.lock
    - fix: update mcp-types version to 0.1.1 in Cargo.toml
    - fix: ensure publish is set to false in Cargo.toml
    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Documentation

    - docs: update changelog for v0.42.7 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.5 [skip ci]
    - docs: update changelog for v0.42.4 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.7
    - chore: release v0.42.6
    - chore: release v0.42.5
    - chore: release v0.42.4
    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.7] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: remove jsonschema dependency from mcp-types in Cargo.lock
    - fix: remove mcp-types configuration from release.toml
    - fix: ensure publish is set to false for mcp-types in release.toml
    - fix: update mcp-types version to 0.1.1 in Cargo.lock
    - fix: update mcp-types version to 0.1.1 in Cargo.toml
    - fix: ensure publish is set to false in Cargo.toml
    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Documentation

    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.5 [skip ci]
    - docs: update changelog for v0.42.4 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.6
    - chore: release v0.42.5
    - chore: release v0.42.4
    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.6] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: remove jsonschema dependency from mcp-types in Cargo.lock
    - fix: remove mcp-types configuration from release.toml
    - fix: ensure publish is set to false for mcp-types in release.toml
    - fix: update mcp-types version to 0.1.1 in Cargo.lock
    - fix: update mcp-types version to 0.1.1 in Cargo.toml
    - fix: ensure publish is set to false in Cargo.toml
    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Documentation

    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.5 [skip ci]
    - docs: update changelog for v0.42.4 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.5
    - chore: release v0.42.4
    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.6] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: remove mcp-types configuration from release.toml
    - fix: ensure publish is set to false for mcp-types in release.toml
    - fix: update mcp-types version to 0.1.1 in Cargo.lock
    - fix: update mcp-types version to 0.1.1 in Cargo.toml
    - fix: ensure publish is set to false in Cargo.toml
    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Documentation

    - docs: update changelog for v0.42.6 [skip ci]
    - docs: update changelog for v0.42.5 [skip ci]
    - docs: update changelog for v0.42.4 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.5
    - chore: release v0.42.4
    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.6] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: ensure publish is set to false for mcp-types in release.toml
    - fix: update mcp-types version to 0.1.1 in Cargo.lock
    - fix: update mcp-types version to 0.1.1 in Cargo.toml
    - fix: ensure publish is set to false in Cargo.toml
    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Documentation

    - docs: update changelog for v0.42.5 [skip ci]
    - docs: update changelog for v0.42.4 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.5
    - chore: release v0.42.4
    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.5] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: ensure publish is set to false in Cargo.toml
    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Documentation

    - docs: update changelog for v0.42.4 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.4
    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.4] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Documentation

    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.3
    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.3] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: remove .cargo_vcs_info.json and update vtcode-core dependency version in Cargo.toml
    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Documentation

    - docs: update changelog for v0.42.3 [skip ci]
    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.3] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Bug Fixes

    - fix: update mcp-types dependency path in Cargo.toml and add jsonschema to dependencies

### Documentation

    - docs: update changelog for v0.42.2 [skip ci]
    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.2
    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.2] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Documentation

    - docs: update changelog for v0.42.1 [skip ci]
    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.1
    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.1] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Documentation

    - docs: update changelog for v0.43.0 [skip ci]
    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.43.0] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Documentation

    - docs: update changelog for v0.42.0 [skip ci]
    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.42.0
    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.42.0] - 2025-11-07

### Features

    - feat: add Kimi K2 Thinking model support and update Moonshot provider logic

### Documentation

    - docs: update changelog for v0.41.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.41.0
    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.41.0] - 2025-11-07

### Documentation

    - docs: update changelog for v0.40.1 [skip ci]
    - docs: update changelog for v0.40.0 [skip ci]
    - docs: update changelog for v0.40.1 [skip ci]

### Chores

    - chore: release v0.40.1
    - chore: release v0.40.0
    - chore: release v0.40.1

# [Version 0.40.1] - 2025-11-06

### Features

    - feat: Enhance workspace trust and automation features
    - feat: add workspace trust request functionality
    - feat: Add VT Code Chat extension with MCP integration
    - feat: add experimental smart summarization feature for conversation compression
    - feat: replace tempfile with assert_fs for improved temporary directory handling

### Bug Fixes

    - fix: add wasm32-wasip2 component to toolchain configuration

### Refactors

    - refactor: remove wasm32-wasip2 component from toolchain configuration and clean up test imports
    - refactor: clean up test module by removing unused imports and structures
    - refactor: replace assert_fs::prelude with tempfile::tempdir in tests
    - refactor: remove unused zed-extension files and grammars
    - refactor: update tool policies, exclude zed-extension from workspace, and upgrade zed_extension_api dependency
    - refactor: update tool policy and improve command handling; streamline error messages and enhance telemetry logging
    - refactor: update extension ID and name for consistency
    - refactor: remove unused imports and streamline timeout error handling

### Documentation

    - docs: update changelog for v0.40.0 [skip ci]
    - docs: add troubleshooting section for development installation

### Chores

    - chore: release v0.40.0

# [Version 0.40.0] - 2025-11-06

### Features

    - feat: Enhance workspace trust and automation features
    - feat: add workspace trust request functionality
    - feat: Add VT Code Chat extension with MCP integration
    - feat: add experimental smart summarization feature for conversation compression
    - feat: replace tempfile with assert_fs for improved temporary directory handling

### Bug Fixes

    - fix: add wasm32-wasip2 component to toolchain configuration

### Refactors

    - refactor: remove wasm32-wasip2 component from toolchain configuration and clean up test imports
    - refactor: clean up test module by removing unused imports and structures
    - refactor: replace assert_fs::prelude with tempfile::tempdir in tests
    - refactor: remove unused zed-extension files and grammars
    - refactor: update tool policies, exclude zed-extension from workspace, and upgrade zed_extension_api dependency
    - refactor: update tool policy and improve command handling; streamline error messages and enhance telemetry logging
    - refactor: update extension ID and name for consistency
    - refactor: remove unused imports and streamline timeout error handling

### Documentation

    - docs: add troubleshooting section for development installation
    - docs: update changelog for v0.39.13 [skip ci]

### Chores

    - chore: release v0.39.13

# [Version 0.39.13] - 2025-11-03

### Features

    - feat: disable Docker usage by default in build script and update Cross.toml comments

### Documentation

    - docs: update changelog for v0.39.12 [skip ci]

### Chores

    - chore: release v0.39.12

# [Version 0.39.12] - 2025-11-03

### Features

    - feat: disable Docker usage by default in build script and update Cross.toml comments
    - feat: add initial implementation of VT Code Zed extension with icons, themes, and logging commands

### Documentation

    - docs: update changelog for v0.39.11 [skip ci]

### Chores

    - chore: release v0.39.11
    - chore: update dependencies, enhance README, and add diagnostics commands for Zed extension

# [Version 0.39.11] - 2025-11-03

### Features

    - feat: add initial implementation of VT Code Zed extension with icons, themes, and logging commands

### Refactors

    - refactor: remove npm package support and update installation instructions
    - refactor: update docs.rs URL and improve response handling in release script

### Documentation

    - docs: update changelog for v0.39.10 [skip ci]
    - docs: update changelog for v0.39.9 [skip ci]

### Chores

    - chore: update dependencies, enhance README, and add diagnostics commands for Zed extension
    - chore: release v0.39.10
    - chore: release v0.39.9

# [Version 0.39.10] - 2025-11-03

### Refactors

    - refactor: remove npm package support and update installation instructions
    - refactor: update docs.rs URL and improve response handling in release script

### Documentation

    - docs: update changelog for v0.39.9 [skip ci]
    - docs: update changelog for v0.39.8 [skip ci]

### Chores

    - chore: release v0.39.9
    - chore: release v0.39.8

# [Version 0.39.9] - 2025-11-03

### Refactors

    - refactor: remove npm package support and update installation instructions
    - refactor: update docs.rs URL and improve response handling in release script

### Documentation

    - docs: update changelog for v0.39.8 [skip ci]

### Chores

    - chore: release v0.39.8

# [Version 0.39.8] - 2025-11-03

### Bug Fixes

    - fix: restore npm/package.json file removed in error

### Documentation

    - docs: update changelog for v0.39.7 [skip ci]

### Chores

    - chore: release v0.39.7
    - chore: update npm package to v0.39.7
    - chore: update Cross.toml and release script for improved environment variable handling

# [Version 0.39.7] - 2025-11-03

### Bug Fixes

    - fix: restore npm/package.json file removed in error

### Documentation

    - docs: update changelog for v0.39.6 [skip ci]

### Chores

    - chore: update npm package to v0.39.7
    - chore: update Cross.toml and release script for improved environment variable handling
    - chore: release v0.39.6
    - chore: update npm package to v0.39.6

# [Version 0.39.6] - 2025-11-03

### Features

    - feat: add cross-compilation configuration and documentation
    - feat: Enhance development and release process for VT Code extension

### Bug Fixes

    - fix: update changelog generation to handle date formatting correctly
    - fix: rename VT Code Update Plan tool for consistency
    - fix: update language model tool properties for VT Code Update Plan

### Refactors

    - refactor: remove unused IdeContextBridge and clean up session initialization

### Documentation

    - docs: update changelog for v0.39.5 [skip ci]
    - docs: update changelog for v0.39.4 [skip ci]
    - docs: update changelog for v0.39.3 [skip ci]
    - docs: update homebrew installation to use core tap

### Chores

    - chore: update npm package to v0.39.6
    - chore: release v0.39.5
    - chore: update npm package to v0.39.5
    - chore: release v0.39.4
    - chore: update npm package to v0.39.4
    - chore: release v0.39.3
    - chore: update npm package to v0.39.3
    - chore: update version to 0.1.1 and add release date to changelog
    - chore(deps): bump the cargo-monthly-rollup group across 1 directory with 28 updates

### Features

    - feat: add cross-compilation configuration and documentation|
    - feat: Enhance development and release process for VT Code extension|

### Bug Fixes

    - fix: update changelog generation to handle date formatting correctly|
    - fix: rename VT Code Update Plan tool for consistency|
    - fix: update language model tool properties for VT Code Update Plan|

### Refactors

    - refactor: remove unused IdeContextBridge and clean up session initialization|

### Documentation

    - docs: update changelog for v0.39.4 [skip ci]|
    - docs: update changelog for v0.39.3 [skip ci]|
    - docs: update homebrew installation to use core tap|
    - docs: update changelog for v0.39.2 [skip ci]|

### Chores

    - chore: update npm package to v0.39.5|
    - chore: release v0.39.4|
    - chore: update npm package to v0.39.4|
    - chore: release v0.39.3|
    - chore: update npm package to v0.39.3|
    - chore: update version to 0.1.1 and add release date to changelog|
    - chore(deps): bump the cargo-monthly-rollup group across 1 directory with 28 updates|
    - chore: release v0.39.2|
    - chore: update npm package to v0.39.2|

# [Version 0.39.4] - 2025-11-03$'

'### Features$'
'    - feat: Enhance development and release process for VT Code extension$'

'### Bug Fixes$'
'    - fix: rename VT Code Update Plan tool for consistency
    - fix: update language model tool properties for VT Code Update Plan$'

'### Refactors$'
'    - refactor: remove unused IdeContextBridge and clean up session initialization$'

'### Documentation$'
'    - docs: update changelog for v0.39.3 [skip ci]
    - docs: update homebrew installation to use core tap
    - docs: update changelog for v0.39.2 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.39.4
    - chore: release v0.39.3
    - chore: update npm package to v0.39.3
    - chore: update version to 0.1.1 and add release date to changelog
    - chore(deps: bump the cargo-monthly-rollup group across 1 directory with 28 updates
    - chore: release v0.39.2
    - chore: update npm package to v0.39.2$'

'

# [Version 0.39.3] - 2025-11-03$'

'### Features$'
'    - feat: Enhance development and release process for VT Code extension$'

'### Bug Fixes$'
'    - fix: rename VT Code Update Plan tool for consistency
    - fix: update language model tool properties for VT Code Update Plan$'

'### Documentation$'
'    - docs: update changelog for v0.39.2 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.39.3
    - chore: update version to 0.1.1 and add release date to changelog
    - chore: release v0.39.2
    - chore: update npm package to v0.39.2$'

'

# [Version 0.39.2] - 2025-11-03$'

'### Documentation$'
'    - docs: update changelog for v0.39.1 [skip ci]
    - docs: update tool-policy and extension files to remove quotes from schema_version
    - docs: update zed-acp documentation to clarify top-level metadata requirements in extension manifest
    - docs: update zed-acp documentation to emphasize required schema_version in extension manifest$'

'### Chores$'
'    - chore: update npm package to v0.39.2
    - chore: release v0.39.1
    - chore: update npm package to v0.39.1$'

'

# [Version 0.39.1] - 2025-11-03$'

'### Documentation$'
'    - docs: update tool-policy and extension files to remove quotes from schema_version
    - docs: update zed-acp documentation to clarify top-level metadata requirements in extension manifest
    - docs: update zed-acp documentation to emphasize required schema_version in extension manifest
    - docs: update README and zed-acp documentation to include package.id requirement
    - docs: update changelog for v0.39.0 [skip ci]
    - docs: update changelog for v0.38.2 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.39.1
    - chore: release v0.39.0
    - chore: update npm package to v0.39.0
    - chore: release v0.38.2
    - chore: update npm package to v0.38.2
    - chore: update mcp-types integration and add tests for docs.rs compatibility
    - chore: update dependencies for agent-client-protocol and related packages
    - chore: add sudo to softwareupdate command for OpenSSL installation on macOS
    - chore: enhance OpenSSL installation step for x86_64-apple-darwin target$'

'

# [Version 0.39.0] - 2025-11-03$'

'### Features$'
'    - feat: Add clear screen command to session and implement related functionality$'

'### Documentation$'
'    - docs: update changelog for v0.38.2 [skip ci]
    - docs: update changelog for v0.38.1 [skip ci]
    - docs: update changelog for v0.38.0 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.39.0
    - chore: release v0.38.2
    - chore: update npm package to v0.38.2
    - chore: update mcp-types integration and add tests for docs.rs compatibility
    - chore: update dependencies for agent-client-protocol and related packages
    - chore: add sudo to softwareupdate command for OpenSSL installation on macOS
    - chore: enhance OpenSSL installation step for x86_64-apple-darwin target
    - chore: release v0.38.1
    - chore: update npm package to v0.38.1
    - chore: update CI workflow to use stable Rust toolchain and add markdown linting filter
    - chore: update dependabot configuration to monthly schedule and reduce open pull requests limit
    - chore: release v0.38.0
    - chore: update npm package to v0.38.0$'

'

# [Version 0.38.2] - 2025-11-02$'

'### Features$'
'    - feat: Add clear screen command to session and implement related functionality$'

'### Documentation$'
'    - docs: update changelog for v0.38.1 [skip ci]
    - docs: update changelog for v0.38.0 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.38.2
    - chore: update mcp-types integration and add tests for docs.rs compatibility
    - chore: update dependencies for agent-client-protocol and related packages
    - chore: add sudo to softwareupdate command for OpenSSL installation on macOS
    - chore: enhance OpenSSL installation step for x86_64-apple-darwin target
    - chore: release v0.38.1
    - chore: update npm package to v0.38.1
    - chore: update CI workflow to use stable Rust toolchain and add markdown linting filter
    - chore: update dependabot configuration to monthly schedule and reduce open pull requests limit
    - chore: release v0.38.0
    - chore: update npm package to v0.38.0$'

'

# [Version 0.38.1] - 2025-11-02$'

'### Features$'
'    - feat: Add clear screen command to session and implement related functionality
    - feat: Enhance glob pattern matching to support question mark wildcard
    - feat: Enhance tool policy and add time conversion functions$'

'### Bug Fixes$'
'    - fix: update tool name in test and improve conversation compression logic$'

'### Refactors$'
'    - refactor(file_ops: Optimize file metadata retrieval and reduce unnecessary system calls
    - refactor(sandbox: Improve sandbox configuration and event logging performance
    - refactor(ui: Modernize TUI rendering and improve diff visualization
    - refactor: Improve code formatting and readability in various files
    - refactor: update tool policies for curl and apply_patch, and improve error messages in update checker
    - refactor: update tool policies to allow more actions and improve asset URL resolution
    - refactor: enhance conversation compression logic and message truncation
    - refactor: clean up whitespace and improve code readability$'

'### Documentation$'
'    - docs: update changelog for v0.38.0 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.38.1
    - chore: update CI workflow to use stable Rust toolchain and add markdown linting filter
    - chore: update dependabot configuration to monthly schedule and reduce open pull requests limit
    - chore: release v0.38.0
    - chore: update npm package to v0.38.0$'

'

# [Version 0.38.0] - 2025-11-02$'

'### Features$'
'    - feat: Add clear screen command to session and implement related functionality
    - feat: Enhance glob pattern matching to support question mark wildcard
    - feat: Enhance tool policy and add time conversion functions$'

'### Bug Fixes$'
'    - fix: update tool name in test and improve conversation compression logic
    - fix: correct tool name from run_command to run_pty_cmd$'

'### Refactors$'
'    - refactor(file_ops: Optimize file metadata retrieval and reduce unnecessary system calls
    - refactor(sandbox: Improve sandbox configuration and event logging performance
    - refactor(ui: Modernize TUI rendering and improve diff visualization
    - refactor: Improve code formatting and readability in various files
    - refactor: update tool policies for curl and apply_patch, and improve error messages in update checker
    - refactor: update tool policies to allow more actions and improve asset URL resolution
    - refactor: enhance conversation compression logic and message truncation
    - refactor: clean up whitespace and improve code readability
    - refactor: update LLM provider and model configurations
    - refactor: rename RUN_PTY_CMD to maintain consistency with run_pty_cmd tool$'

'### Documentation$'
'    - docs: update changelog for v0.37.1 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.38.0
    - chore: release v0.37.1
    - chore: update npm package to v0.37.1
    - chore: update dependencies and enhance tool execution reporting$'

'

# [Version 0.37.1] - 2025-10-30$'

'### Features$'
'    - feat: enhance command execution policies and UI interactions
    - feat: Implement task plan management in TUI session$'

'### Bug Fixes$'
'    - fix: correct tool name from run_command to run_pty_cmd
    - fix: add Debug trait to InlineTextStyle for improved logging$'

'### Refactors$'
'    - refactor: update LLM provider and model configurations
    - refactor: rename RUN_PTY_CMD to maintain consistency with run_pty_cmd tool
    - refactor: improve tool summary rendering and clean up unused code
    - refactor: update LLM provider and model configurations
    - refactor: update configuration for LLM provider and model settings
    - refactor: move display_interrupt_notice function to improve code organization$'

'### Documentation$'
'    - docs: update changelog for v0.37.0 [skip ci]
    - docs: update changelog for v0.36.0 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.37.1
    - chore: update dependencies and enhance tool execution reporting
    - chore: release v0.37.0
    - chore: update npm package to v0.37.0
    - chore: release v0.36.0
    - chore: update npm package to v0.36.0$'

'

# [Version 0.37.0] - 2025-10-30$'

'### Features$'
'    - feat: enhance command execution policies and UI interactions
    - feat: Implement task plan management in TUI session
    - feat: add asset synchronization script for managing embedded assets
    - feat: add embedded asset management for prompts and documentation
    - feat: increase max_tool_loops to 100 and add workspace config refresh functionality
    - feat: add templates for agent file generation and VT Code session initiation$'

'### Bug Fixes$'
'    - fix: add Debug trait to InlineTextStyle for improved logging
    - fix: adjust max_tool_loops to 20 and correct prompt file paths$'

'### Refactors$'
'    - refactor: improve tool summary rendering and clean up unused code
    - refactor: update LLM provider and model configurations
    - refactor: update configuration for LLM provider and model settings
    - refactor: move display_interrupt_notice function to improve code organization$'

'### Documentation$'
'    - docs: update changelog for v0.36.0 [skip ci]
    - docs: update changelog for v0.35.19 [skip ci]
    - docs: add asset synchronization guide for managing embedded assets in vtcode-core
    - docs: clean up vtcode_docs_map.md and remove unnecessary newlines in generate-agent-file.md
    - docs: update changelog for v0.35.18 [skip ci]
    - docs: update changelog for v0.35.17 [skip ci]
    - docs: update changelog for v0.35.16 [skip ci]
    - docs: update changelog for v0.35.15 [skip ci]
    - docs: update changelog for v0.35.14 [skip ci]
    - docs: update changelog for v0.35.13 [skip ci]
    - docs: update changelog for v0.35.12 [skip ci]
    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.37.0
    - chore: release v0.36.0
    - chore: update npm package to v0.36.0
    - chore: release v0.35.19
    - chore: update npm package to v0.35.19
    - chore: update vtcode and related packages to v0.35.18
    - chore: release v0.35.18
    - chore: update npm package to v0.35.18
    - chore: release v0.35.17
    - chore: update npm package to v0.35.17
    - chore: release v0.35.16
    - chore: update npm package to v0.35.16
    - chore: release v0.35.15
    - chore: update npm package to v0.35.15
    - chore: release v0.35.14
    - chore: update npm package to v0.35.14
    - chore: release v0.35.13
    - chore: update npm package to v0.35.13
    - chore: release v0.35.12
    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps: bump crossterm from 0.27.0 to 0.28.1$'

'

# [Version 0.36.0] - 2025-10-30$'

'### Features$'
'    - feat: enhance command execution policies and UI interactions
    - feat: Implement task plan management in TUI session
    - feat: add asset synchronization script for managing embedded assets
    - feat: add embedded asset management for prompts and documentation
    - feat: increase max_tool_loops to 100 and add workspace config refresh functionality
    - feat: add templates for agent file generation and VT Code session initiation$'

'### Bug Fixes$'
'    - fix: add Debug trait to InlineTextStyle for improved logging
    - fix: adjust max_tool_loops to 20 and correct prompt file paths$'

'### Refactors$'
'    - refactor: improve tool summary rendering and clean up unused code
    - refactor: update LLM provider and model configurations
    - refactor: update configuration for LLM provider and model settings
    - refactor: move display_interrupt_notice function to improve code organization$'

'### Documentation$'
'    - docs: update changelog for v0.35.19 [skip ci]
    - docs: add asset synchronization guide for managing embedded assets in vtcode-core
    - docs: clean up vtcode_docs_map.md and remove unnecessary newlines in generate-agent-file.md
    - docs: update changelog for v0.35.18 [skip ci]
    - docs: update changelog for v0.35.17 [skip ci]
    - docs: update changelog for v0.35.16 [skip ci]
    - docs: update changelog for v0.35.15 [skip ci]
    - docs: update changelog for v0.35.14 [skip ci]
    - docs: update changelog for v0.35.13 [skip ci]
    - docs: update changelog for v0.35.12 [skip ci]
    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.36.0
    - chore: release v0.35.19
    - chore: update npm package to v0.35.19
    - chore: update vtcode and related packages to v0.35.18
    - chore: release v0.35.18
    - chore: update npm package to v0.35.18
    - chore: release v0.35.17
    - chore: update npm package to v0.35.17
    - chore: release v0.35.16
    - chore: update npm package to v0.35.16
    - chore: release v0.35.15
    - chore: update npm package to v0.35.15
    - chore: release v0.35.14
    - chore: update npm package to v0.35.14
    - chore: release v0.35.13
    - chore: update npm package to v0.35.13
    - chore: release v0.35.12
    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps: bump crossterm from 0.27.0 to 0.28.1$'

'

# [Version 0.35.19] - 2025-10-27$'

'### Features$'
'    - feat: add asset synchronization script for managing embedded assets
    - feat: add embedded asset management for prompts and documentation
    - feat: increase max_tool_loops to 100 and add workspace config refresh functionality
    - feat: add templates for agent file generation and VT Code session initiation$'

'### Bug Fixes$'
'    - fix: adjust max_tool_loops to 20 and correct prompt file paths$'

'### Documentation$'
'    - docs: add asset synchronization guide for managing embedded assets in vtcode-core
    - docs: clean up vtcode_docs_map.md and remove unnecessary newlines in generate-agent-file.md
    - docs: update changelog for v0.35.18 [skip ci]
    - docs: update changelog for v0.35.17 [skip ci]
    - docs: update changelog for v0.35.16 [skip ci]
    - docs: update changelog for v0.35.15 [skip ci]
    - docs: update changelog for v0.35.14 [skip ci]
    - docs: update changelog for v0.35.13 [skip ci]
    - docs: update changelog for v0.35.12 [skip ci]
    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.19
    - chore: update vtcode and related packages to v0.35.18
    - chore: release v0.35.18
    - chore: update npm package to v0.35.18
    - chore: release v0.35.17
    - chore: update npm package to v0.35.17
    - chore: release v0.35.16
    - chore: update npm package to v0.35.16
    - chore: release v0.35.15
    - chore: update npm package to v0.35.15
    - chore: release v0.35.14
    - chore: update npm package to v0.35.14
    - chore: release v0.35.13
    - chore: update npm package to v0.35.13
    - chore: release v0.35.12
    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps: bump crossterm from 0.27.0 to 0.28.1
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.18] - 2025-10-27$'

'### Features$'
'    - feat: add embedded asset management for prompts and documentation
    - feat: increase max_tool_loops to 100 and add workspace config refresh functionality
    - feat: add templates for agent file generation and VT Code session initiation$'

'### Bug Fixes$'
'    - fix: adjust max_tool_loops to 20 and correct prompt file paths$'

'### Documentation$'
'    - docs: update changelog for v0.35.17 [skip ci]
    - docs: update changelog for v0.35.16 [skip ci]
    - docs: update changelog for v0.35.15 [skip ci]
    - docs: update changelog for v0.35.14 [skip ci]
    - docs: update changelog for v0.35.13 [skip ci]
    - docs: update changelog for v0.35.12 [skip ci]
    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.18
    - chore: release v0.35.17
    - chore: update npm package to v0.35.17
    - chore: release v0.35.16
    - chore: update npm package to v0.35.16
    - chore: release v0.35.15
    - chore: update npm package to v0.35.15
    - chore: release v0.35.14
    - chore: update npm package to v0.35.14
    - chore: release v0.35.13
    - chore: update npm package to v0.35.13
    - chore: release v0.35.12
    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.17] - 2025-10-27$'

'### Features$'
'    - feat: increase max_tool_loops to 100 and add workspace config refresh functionality
    - feat: add templates for agent file generation and VT Code session initiation$'

'### Bug Fixes$'
'    - fix: adjust max_tool_loops to 20 and correct prompt file paths$'

'### Documentation$'
'    - docs: update changelog for v0.35.16 [skip ci]
    - docs: update changelog for v0.35.15 [skip ci]
    - docs: update changelog for v0.35.14 [skip ci]
    - docs: update changelog for v0.35.13 [skip ci]
    - docs: update changelog for v0.35.12 [skip ci]
    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.17
    - chore: release v0.35.16
    - chore: update npm package to v0.35.16
    - chore: release v0.35.15
    - chore: update npm package to v0.35.15
    - chore: release v0.35.14
    - chore: update npm package to v0.35.14
    - chore: release v0.35.13
    - chore: update npm package to v0.35.13
    - chore: release v0.35.12
    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.16] - 2025-10-27$'

'### Features$'
'    - feat: add templates for agent file generation and VT Code session initiation$'

'### Bug Fixes$'
'    - fix: adjust max_tool_loops to 20 and correct prompt file paths$'

'### Documentation$'
'    - docs: update changelog for v0.35.15 [skip ci]
    - docs: update changelog for v0.35.14 [skip ci]
    - docs: update changelog for v0.35.13 [skip ci]
    - docs: update changelog for v0.35.12 [skip ci]
    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.16
    - chore: release v0.35.15
    - chore: update npm package to v0.35.15
    - chore: release v0.35.14
    - chore: update npm package to v0.35.14
    - chore: release v0.35.13
    - chore: update npm package to v0.35.13
    - chore: release v0.35.12
    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.15] - 2025-10-27$'

'### Features$'
'    - feat: add templates for agent file generation and VT Code session initiation$'

'### Documentation$'
'    - docs: update changelog for v0.35.14 [skip ci]
    - docs: update changelog for v0.35.13 [skip ci]
    - docs: update changelog for v0.35.12 [skip ci]
    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.15
    - chore: release v0.35.14
    - chore: update npm package to v0.35.14
    - chore: release v0.35.13
    - chore: update npm package to v0.35.13
    - chore: release v0.35.12
    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.14] - 2025-10-27$'

'### Features$'
'    - feat: add templates for agent file generation and VT Code session initiation$'

'### Documentation$'
'    - docs: update changelog for v0.35.13 [skip ci]
    - docs: update changelog for v0.35.12 [skip ci]
    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.14
    - chore: release v0.35.13
    - chore: update npm package to v0.35.13
    - chore: release v0.35.12
    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.13] - 2025-10-27$'

'### Documentation$'
'    - docs: update changelog for v0.35.12 [skip ci]
    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.13
    - chore: release v0.35.12
    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.12] - 2025-10-27$'

'### Documentation$'
'    - docs: update changelog for v0.35.11 [skip ci]
    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.12
    - chore: release v0.35.11
    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.11] - 2025-10-27$'

'### Documentation$'
'    - docs: update changelog for v0.35.10 [skip ci]
    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.11
    - chore: release v0.35.10
    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.10] - 2025-10-27$'

'### Documentation$'
'    - docs: update changelog for v0.35.9 [skip ci]
    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.10
    - chore: release v0.35.9
    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.9] - 2025-10-27$'

'### Documentation$'
'    - docs: update changelog for v0.35.8 [skip ci]
    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.9
    - chore: release v0.35.8
    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.8] - 2025-10-27$'

'### Documentation$'
'    - docs: update changelog for v0.35.7 [skip ci]
    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.8
    - chore: release v0.35.7
    - chore: update npm package to v0.35.7
    - chore(deps-dev: bump @typescript-eslint/eslint-plugin
    - chore(deps: bump reqwest from 0.12.23 to 0.12.24
    - chore(deps: bump is-terminal from 0.4.16 to 0.4.17
    - chore(deps: bump parking_lot from 0.12.4 to 0.12.5
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore(deps-dev: bump mocha from 10.8.2 to 11.7.4 in /vscode-extension
    - chore(deps-dev: bump @types/glob in /vscode-extension
    - chore(deps: bump thiserror from 1.0.69 to 2.0.16
    - chore(deps: bump actions/checkout from 4 to 5
    - chore(deps: bump DavidAnson/markdownlint-cli2-action from 17 to 20
    - chore(deps: bump actions/upload-pages-artifact from 3 to 4$'

'

# [Version 0.35.7] - 2025-10-27$'

'### Documentation$'
'    - docs: update changelog for v0.35.6 [skip ci]
    - docs: update changelog for v0.35.5 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.7
    - chore: release v0.35.6
    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5$'

'

# [Version 0.35.6] - 2025-10-27$'

'### Features$'
'    - feat(minimax: Add MiniMax provider integration and related constants
    - feat: update custom prompt command syntax from /prompts to /prompt
    - feat: Update README and documentation for Cursor and Windsurf support
    - feat: Implement file tree structure for file navigation$'

'### Bug Fixes$'
'    - fix(configuration: Update LLM provider and related settings to use OpenRouter
    - fix(minimax: Correct base URL in MinimaxProvider configuration
    - fix: remove unnecessary newline in CI workflow
    - fix: add permissions section to workflow files
    - fix: add missing API key header in generate_stream method$'

'### Documentation$'
'    - docs: update changelog for v0.35.5 [skip ci]
    - docs: update changelog for v0.35.4 [skip ci]
    - docs: update changelog for v0.35.3 [skip ci]
    - docs: update changelog for v0.35.2 [skip ci]
    - docs: update user guide and changelog with quick access shortcuts and enhancements
    - docs: update changelog for v0.35.1 [skip ci]
    - docs: update changelog for v0.35.0 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.6
    - chore: release v0.35.5
    - chore: update npm package to v0.35.5
    - chore: release v0.35.4
    - chore: update npm package to v0.35.4
    - chore: remove example files for self-update and update informer demo
    - chore: release v0.35.3
    - chore: update npm package to v0.35.3
    - chore: remove VSCode extension publishing step from release script
    - chore: release v0.35.2
    - chore: update npm package to v0.35.2
    - chore: release v0.35.1
    - chore: update npm package to v0.35.1
    - chore: release v0.35.0
    - chore: update npm package to v0.35.0
    - chore: remove .vscodeignore file and update VSIX package$'

'

# [Version 0.35.5] - 2025-10-27$'

'### Features$'
'    - feat(minimax: Add MiniMax provider integration and related constants
    - feat: update custom prompt command syntax from /prompts to /prompt
    - feat: Update README and documentation for Cursor and Windsurf support
    - feat: Implement file tree structure for file navigation$'

'### Bug Fixes$'
'    - fix(configuration: Update LLM provider and related settings to use OpenRouter
    - fix(minimax: Correct base URL in MinimaxProvider configuration
    - fix: remove unnecessary newline in CI workflow
    - fix: add permissions section to workflow files
    - fix: add missing API key header in generate_stream method$'

'### Documentation$'
'    - docs: update changelog for v0.35.4 [skip ci]
    - docs: update changelog for v0.35.3 [skip ci]
    - docs: update changelog for v0.35.2 [skip ci]
    - docs: update user guide and changelog with quick access shortcuts and enhancements
    - docs: update changelog for v0.35.1 [skip ci]
    - docs: update changelog for v0.35.0 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.5
    - chore: release v0.35.4
    - chore: update npm package to v0.35.4
    - chore: remove example files for self-update and update informer demo
    - chore: release v0.35.3
    - chore: update npm package to v0.35.3
    - chore: remove VSCode extension publishing step from release script
    - chore: release v0.35.2
    - chore: update npm package to v0.35.2
    - chore: release v0.35.1
    - chore: update npm package to v0.35.1
    - chore: release v0.35.0
    - chore: update npm package to v0.35.0
    - chore: remove .vscodeignore file and update VSIX package$'

'

# [Version 0.35.4] - 2025-10-27$'

'### Features$'
'    - feat(minimax: Add MiniMax provider integration and related constants
    - feat: update custom prompt command syntax from /prompts to /prompt
    - feat: Update README and documentation for Cursor and Windsurf support
    - feat: Implement file tree structure for file navigation$'

'### Bug Fixes$'
'    - fix(configuration: Update LLM provider and related settings to use OpenRouter
    - fix(minimax: Correct base URL in MinimaxProvider configuration
    - fix: remove unnecessary newline in CI workflow
    - fix: add permissions section to workflow files
    - fix: add missing API key header in generate_stream method$'

'### Documentation$'
'    - docs: update changelog for v0.35.3 [skip ci]
    - docs: update changelog for v0.35.2 [skip ci]
    - docs: update user guide and changelog with quick access shortcuts and enhancements
    - docs: update changelog for v0.35.1 [skip ci]
    - docs: update changelog for v0.35.0 [skip ci]
    - docs: update changelog for v0.35.3 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.4
    - chore: remove example files for self-update and update informer demo
    - chore: release v0.35.3
    - chore: update npm package to v0.35.3
    - chore: remove VSCode extension publishing step from release script
    - chore: release v0.35.2
    - chore: update npm package to v0.35.2
    - chore: release v0.35.1
    - chore: update npm package to v0.35.1
    - chore: release v0.35.0
    - chore: update npm package to v0.35.0
    - chore: remove .vscodeignore file and update VSIX package
    - chore: release v0.35.3
    - chore: update npm package to v0.35.3
    - chore: remove VSCode extension publishing step from release script$'

'

# [Version 0.35.3] - 2025-10-27$'

'### Documentation$'
'    - docs: update changelog for v0.35.2 [skip ci]
    - docs: update user guide and changelog with quick access shortcuts and enhancements$'

'### Chores$'
'    - chore: update npm package to v0.35.3
    - chore: remove VSCode extension publishing step from release script
    - chore: release v0.35.2
    - chore: update npm package to v0.35.2$'

'

# [Version 0.35.2] - 2025-10-27$'

'### Documentation$'
'    - docs: update user guide and changelog with quick access shortcuts and enhancements
    - docs: update changelog for v0.35.1 [skip ci]$'

'### Chores$'
'    - chore: update npm package to v0.35.2
    - chore: release v0.35.1
    - chore: update npm package to v0.35.1$'

'

## [0.35.1] - 2025-10-27

### Features

    - feat: update custom prompt command syntax from /prompts to /prompt
    - feat: Update README and documentation for Cursor and Windsurf support
    - feat: Implement file tree structure for file navigation
    - feat: add simple GitHub Pages workflow for /docs
    - feat: Enhance model picker and dynamic model fetching
    - feat: add synchronous fetching of LMStudio models and improve model selection

### Bug Fixes

    - fix: remove mdbook workflow causing CI failure

### Documentation

    - docs: update changelog for v0.35.0 [skip ci]
    - docs: add lifecycle hooks guide

### Chores

    - chore: update npm package to v0.35.1
    - chore: release v0.35.0
    - chore: update npm package to v0.35.0
    - chore: remove .vscodeignore file and update VSIX package
    - chore(deps-dev): bump esbuild in /vscode-extension
    - chore(deps-dev): bump eslint from 8.57.1 to 9.38.0 in /vscode-extension
    - chore(deps): bump windows-sys from 0.59.0 to 0.61.1
    - chore(deps): bump toml from 0.9.7 to 0.9.8
    - chore(deps): bump tree-sitter-javascript from 0.23.1 to 0.25.0
    - chore(deps): bump dirs from 5.0.1 to 6.0.0
    - chore(deps-dev): bump @types/node in /vscode-extension
    - chore(deps): bump tree-sitter-go from 0.23.4 to 0.25.0
    - chore(deps-dev): bump glob from 10.4.5 to 11.0.3 in /vscode-extension
    - chore(deps): bump actions/cache from 3 to 4
    - chore(deps): bump actions/upload-artifact from 4 to 5
    - chore(deps-dev): bump @typescript-eslint/parser in /vscode-extension
    - chore(deps): bump codecov/codecov-action from 3 to 5
    - chore(deps): bump actions/checkout from 3 to 5
    - chore(deps): bump actions/setup-node from 4 to 6

## [0.35.0] - 2025-10-27

### Features

    - feat: update custom prompt command syntax from /prompts to /prompt
    - feat: Update README and documentation for Cursor and Windsurf support
    - feat: Implement file tree structure for file navigation
    - feat: add simple GitHub Pages workflow for /docs
    - feat: Enhance model picker and dynamic model fetching
    - feat: add synchronous fetching of LMStudio models and improve model selection
    - feat: Add IDE integration and troubleshooting guides to documentation
    - feat: Add VSCode extension publishing support to release script
    - feat: Add initial files for VT Code Companion extension including README, LICENSE, CHANGELOG, and esbuild configuration
    - feat: Add initial package.json for VT Code Companion extension
    - feat(security): Implement comprehensive security documentation and fixes
    - feat: add comprehensive security audit and model documentation

### Bug Fixes

    - fix: remove mdbook workflow causing CI failure

### Refactors

    - refactor: Rename extension from "VT Code Companion" to "VT Code" and update CHANGELOG
    - refactor: use unsafe blocks for environment variable manipulation in tests
    - refactor: remove unused tools and simplify tool policies

### Documentation

    - docs: update changelog for v0.34.0 [skip ci]
    - docs: add lifecycle hooks guide

### Chores

    - chore: update npm package to v0.35.0
    - chore: remove .vscodeignore file and update VSIX package
    - chore(deps-dev): bump esbuild in /vscode-extension
    - chore(deps-dev): bump eslint from 8.57.1 to 9.38.0 in /vscode-extension
    - chore(deps): bump windows-sys from 0.59.0 to 0.61.1
    - chore(deps): bump toml from 0.9.7 to 0.9.8
    - chore(deps): bump tree-sitter-javascript from 0.23.1 to 0.25.0
    - chore(deps): bump dirs from 5.0.1 to 6.0.0
    - chore(deps-dev): bump @types/node in /vscode-extension
    - chore(deps): bump tree-sitter-go from 0.23.4 to 0.25.0
    - chore(deps-dev): bump glob from 10.4.5 to 11.0.3 in /vscode-extension
    - chore(deps): bump actions/cache from 3 to 4
    - chore(deps): bump actions/upload-artifact from 4 to 5
    - chore(deps-dev): bump @typescript-eslint/parser in /vscode-extension
    - chore(deps): bump codecov/codecov-action from 3 to 5
    - chore(deps): bump actions/checkout from 3 to 5
    - chore(deps): bump actions/setup-node from 4 to 6
    - chore: release v0.34.0
    - chore: update npm package to v0.34.0

## [0.34.0] - 2025-10-25

### Features

    - feat: Add IDE integration and troubleshooting guides to documentation
    - feat: Add VSCode extension publishing support to release script
    - feat: Add initial files for VT Code Companion extension including README, LICENSE, CHANGELOG, and esbuild configuration
    - feat: Add initial package.json for VT Code Companion extension
    - feat(security): Implement comprehensive security documentation and fixes
    - feat: add comprehensive security audit and model documentation
    - feat: add changelog generation from commits in release script

### Refactors

    - refactor: Rename extension from "VT Code Companion" to "VT Code" and update CHANGELOG
    - refactor: use unsafe blocks for environment variable manipulation in tests
    - refactor: remove unused tools and simplify tool policies

### Documentation

    - docs: update changelog for v0.33.1 [skip ci]

### Chores

    - chore: update npm package to v0.34.0
    - chore: release v0.33.1
    - chore: update npm package to v0.33.1
    - chore: update README.md for improved installation instructions and feature highlights
    - chore: update CHANGELOG.md with recent enhancements for v0.33.0

## [0.33.1] - 2025-01-30

### Features

-   feat: add changelog generation from commits in release script
-   feat: run doctests separately in publish_extracted_crates.sh
-   feat: add comprehensive plan for open sourcing VT Code core components
-   feat: add demo section with updated demo GIF in README
-   feat: add VT Code VHS showcase and demo files

### Chores

-   chore: update npm package to v0.33.1
-   chore: update README.md for improved installation instructions and feature highlights
-   chore: update CHANGELOG.md with recent enhancements for v0.33.0
-   chore: release v0.33.0
-   chore: update npm package to v0.33.0
-   chore: update package versions to 0.32.0 and adjust dependencies
-   chore: update npm package to v0.32.0
-   chore: update demo GIF for VHS showcase

### Recent Enhancements (v0.33.0 and beyond)

-   **Enhanced Tool Execution & Output Handling**: Improved tool execution with better error handling and output formatting for enhanced reliability and user experience
-   **Enhanced Timeout Detection & Token Budget Management**: Improved timeout handling and more sophisticated token budget management with better attention management for enhanced performance

-   **Improved Output Rendering**: Enhanced syntax highlighting for JSON, XML, and YAML outputs with better error messaging
-   **Enhanced Bash Runner & Telemetry**: Added dry-run capabilities and feature-gated executors for shell operations with integrated telemetry
-   **Ollama Integration Improvements**: Better support for local models with configurable base URLs and improved tool call handling
-   **MCP Protocol & Tool Support**: Enhanced Model Context Protocol integration with improved resource and prompt handling
-   **Configuration System Improvements**: Enhanced configuration handling with better default preservation and schema validation
-   **Component Extraction Strategy**: Continued work on extracting reusable components including vtcode-exec-events, vtcode-bash-runner, vtcode-config, and vtcode-indexer

### Extracted crates release preparation

-   **vtcode-commons 0.1.0** – marks the shared workspace path/telemetry traits crate ready for publishing with repository and
    documentation metadata in `Cargo.toml`.
-   **vtcode-markdown-store 0.1.0** – aligns the markdown-backed storage crate with the initial release version and links to the
    public documentation.
-   **vtcode-indexer 0.1.0** – retags the workspace-friendly indexer for its first standalone release and records the docs.rs URL
    for consumers.
-   **vtcode-bash-runner 0.1.0** – updates the shell execution helper crate to the shared release version, adds licensing
    metadata, and points to hosted documentation.
-   **vtcode-exec-events 0.1.0** – finalizes the telemetry schema crate for release with docs.rs metadata alongside the version
    alignment.

-   Ran `cargo publish --dry-run` for the release candidates (`vtcode-commons`, `vtcode-markdown-store`, `vtcode-indexer`, `vtcode-exec-events`) and confirmed that `vtcode-bash-runner` will package successfully once `vtcode-commons` is available on crates.io.
-   Scheduled the sequential publish order, tagging plan, and post-release dependency bumps in `docs/component_release_plan.md` so the crates can be released without coordination gaps.
-   Scripted the sequential publish workflow in `scripts/publish_extracted_crates.sh` to automate validation, publishing, and tagging steps with optional dry-run rehearsals.

### `vtcode-exec-events`

-   Added schema metadata (`EVENT_SCHEMA_VERSION`) and a `VersionedThreadEvent` wrapper so consumers can negotiate compatibility before processing telemetry streams.
-   Introduced an `EventEmitter` trait with optional `LogEmitter` and `TracingEmitter` adapters to integrate JSON and tracing pipelines without boilerplate.
-   Published JSON helper utilities and optional schema export support to simplify serialization round-trips and documentation workflows.

### `vtcode-bash-runner`

-   Added feature-gated executors for process, pure-Rust, and dry-run operation so adopters can tailor shell execution strategies without forking the runner.F:vtcode-bash-runner/Cargo.toml†L1-L40F:vtcode-bash-runner/src/executor.rs†L1-L356
-   Introduced the `EventfulExecutor` bridge to emit `vtcode-exec-events` telemetry from standalone shell invocations, plus documentation covering the new feature flags and integrations.F:vtcode-bash-runner/src/executor.rs†L358-L470F:docs/vtcode_bash_runner.md†L1-L120F:docs/vtcode_exec_events.md†L1-L160

### **Major Enhancements - Context Engineering & Attention Management** (Phase 1 & 2)

#### Phase 1: Enhanced System Prompts

-   **Explicit Response Framework**: All system prompts now include a clear 5-step framework
    1. Assess the situation - Understand what the user needs
    2. Gather context efficiently - Use search tools before reading files
    3. Make precise changes - Prefer targeted edits over rewrites
    4. Verify outcomes - Test changes appropriately
    5. Confirm completion - Summarize and verify satisfaction
-   **Enhanced Guidelines**: More specific guidance on tool selection, code style preservation, and handling destructive operations
-   **Multi-Turn Coherence**: Explicit guidance on building context across conversation turns
-   **Token Efficiency**: Maintained concise prompts (~280 tokens) while adding structure

**System Prompt Improvements:**

-   Default prompt: Added explicit framework, guidelines, and context management strategies
-   Lightweight prompt: Added minimal 4-step approach for quick tasks
-   Specialized prompt: Added tool selection strategy by phase, advanced guidelines, and multi-turn coherence

#### Phase 2: Dynamic Context Curation

-   **New Module**: `context_curator.rs` - Implements iterative per-turn context selection based on Anthropic's principles
-   **Conversation Phase Detection**: Automatically detects phase (Exploration, Implementation, Validation, Debugging, Unknown)
-   **Phase-Aware Tool Selection**: Dynamically selects relevant tools based on current conversation phase
-   **Priority-Based Context Selection**:

    1. Recent messages (always included, configurable)
    2. Active work context (files being modified)
    3. Decision ledger summary (compact)
    4. Recent errors and resolutions
    5. Relevant tools (phase-aware)

-   **Configurable Curation**: Full control via `[context.curation]` configuration

**Key Features:**

-   Tracks active files and file summaries
-   Maintains recent error context for debugging
-   Selects optimal tools based on conversation phase
-   Respects token budget constraints
-   Integrates with TokenBudgetManager and DecisionTracker

**API:**

```rust
let curator = ContextCurator::new(config, token_budget, decision_ledger);
curator.mark_file_active("src/main.rs".to_string());
curator.add_error(ErrorContext { ... });
let curated = curator.curate_context(&messages, &tools).await?;
```

**Configuration:**

```toml
[context.curation]
enabled = true
max_tokens_per_turn = 100000
preserve_recent_messages = 5
max_tool_descriptions = 10
include_ledger = true
ledger_max_entries = 12
include_recent_errors = true
max_recent_errors = 3
```

#### Token Budget Tracking & Attention Management

-   **New Module**: `token_budget.rs` - Real-time token budget tracking using Hugging Face `tokenizers`
-   **Component-Level Tracking**: Monitor token usage by category (system prompt, messages, tool results, decision ledger)
-   **Configurable Thresholds**: Warning at 75% (customizable via `vtcode.toml`)
-   **Model-Specific Tokenizers**: Support for GPT, Claude, and other models for accurate counting
-   **Automatic Deduction**: Track token removal during context cleanup
-   **Budget Reports**: Generate detailed token usage reports by component
-   **Performance Optimized**: ~10μs per message using Rust-native Hugging Face `tokenizers`
-   **New Method**: `remaining_tokens()` - Get remaining tokens in budget for context curation decisions

**Configuration:**

```toml
[context.token_budget]
enabled = true
model = "gpt-5-nano"
warning_threshold = 0.75
detailed_tracking = false
```

#### Optimized System Prompts & Tool Descriptions

-   **67-82% Token Reduction**: System prompts streamlined from ~600 tokens to ~200 tokens
-   **80% Tool Description Efficiency**: Average tool description reduced from ~400 to ~80 tokens
-   **"Right Altitude" Principles**: Concise, actionable guidance over verbose instructions
-   **Progressive Disclosure**: Emphasize search-first approach with `grep_file`
-   **Clear Tool Purposes**: Eliminated capability overlap in tool descriptions
-   **Token Management Guidance**: Built-in advice for efficient context usage (e.g., `max_results` parameters)

**System Prompt Improvements:**

-   Removed verbose explanations and redundant information
-   Focused on core principles and actionable strategies
-   Added explicit context strategy guidance
-   Emphasized metadata-first, content-second approach

**Tool Description Improvements:**

-   Clear, unambiguous purposes with minimal overlap
-   Token efficiency guidance (e.g., `max_results` limits)
-   Auto-chunking behavior documented
-   Metadata-first approach emphasized

#### Context Engineering Documentation

-   **New Documentation**: `docs/context_engineering.md` - Comprehensive guide to context management
-   **Implementation Summary**: `docs/context_engineering_implementation.md` - Technical details
-   **Best Practices**: User and developer guidelines for efficient context usage
-   **Configuration Examples**: Complete examples for token budget and context management
-   **Performance Metrics**: Token efficiency improvements documented
-   **References**: Links to Anthropic research and related resources

#### Bug Fixes

-   **Fixed MCP Server Initialization**: Removed premature `cleanup_dead_providers()` call that caused `BrokenPipeError` during initialization
-   **MCP Process Management**: Improved connection lifecycle management to prevent pipe closure issues

#### Dependencies

-   **Added**: `tokenizers = "0.15"` for accurate token counting
-   **Updated**: Cargo.lock with new dependencies

#### Release Automation

-   **Cargo Release Integration**: Adopted `cargo release` with a shared workspace configuration (`release.toml`) and updated `scripts/release.sh` to drive changelog-powered GitHub releases, coordinated crates.io publishing, and npm version synchronization.

### **Major Enhancements - Anthropic-Inspired Architecture**

#### Decision Transparency System

-   **New Module**: `decision_tracker.rs` - Complete audit trail of all agent decisions
-   **Real-time Tracking**: Every action logged with reasoning and confidence scores
-   **Transparency Reports**: Live decision summaries and session statistics
-   **Confidence Scoring**: Quality assessment for all agent actions
-   **Context Preservation**: Full conversation context maintained across decisions

#### Error Recovery & Resilience

-   **New Module**: `error_recovery.rs` - Intelligent error handling system
-   **Pattern Detection**: Automatic identification of recurring errors
-   **Context Preservation**: Never lose important information during failures
-   **Recovery Strategies**: Multiple approaches for handling errors gracefully
-   **Error Statistics**: Comprehensive analysis of error patterns and recovery rates

#### Conversation Summarization

-   **New Module**: `conversation_summarizer.rs` - Automatic conversation compression
-   **Intelligent Summaries**: Key decisions, completed tasks, and error patterns
-   **Long Session Support**: Automatic triggers when conversations exceed thresholds
-   **Confidence Scoring**: Quality assessment for summary reliability
-   **Context Efficiency**: Maintain useful context without hitting limits

### **Tool Design Improvements**

#### Enhanced Tool Documentation

-   **Comprehensive Specifications**: Extensive tool descriptions with examples and error cases
-   **Error-Proofing**: Anticipate and prevent common model misunderstandings
-   **Clear Usage Guidelines**: Detailed instructions for each tool parameter
-   **Debugging Support**: Specific guidance for troubleshooting tool failures

#### Improved System Instruction

-   **Model-Driven Control**: Give maximum autonomy to the language model
-   **Thorough Reasoning**: Encourage deep thinking for complex problems
-   **Flexible Methodology**: Adaptable problem-solving approaches
-   **Quality First**: Emphasize correctness over speed

### **Release Automation**

-   **Coordinated Version Bumps**: `scripts/release.sh` now prompts maintainers to bump the `vtagent-core` crate alongside the main binary, keeping release metadata synchronized.

### **Transparency & Observability**

#### Verbose Mode Enhancements

-   **Real-time Decision Tracking**: See exactly why each action is taken
-   **Error Recovery Monitoring**: Observe intelligent error handling
-   **Conversation Summarization Alerts**: Automatic notifications for long sessions
-   **Session Statistics**: Comprehensive metrics and pattern analysis
-   **Pattern Detection**: Automatic identification of recurring issues

#### Session Reporting

-   **Final Transparency Reports**: Complete session summaries with success metrics
-   **Error Recovery Statistics**: Analysis of error patterns and recovery rates
-   **Decision Quality Metrics**: Confidence scores and decision success rates
-   **Context Usage Monitoring**: Automatic warnings for approaching limits

### **Configuration System Improvements**

#### Two-Way Configuration Synchronization

-   **Smart Config Generation**: `vtcode config` now reads existing `vtcode.toml` and preserves customizations
-   **Complete Template Generation**: Ensures all configuration sections are present, even missing ones
-   **Bidirectional Sync**: Generated configs always match your actual configuration state
-   **Fallback Safety**: Uses system defaults when no configuration file exists
-   **TOML Serialization**: Replaced hardcoded templates with proper TOML generation

## [Previous Versions]

### v0.1.0 - Initial Release

-   Basic agent architecture with Gemini integration
-   Core file system tools (list_files, read_file, write_file, edit_file)
-   Interactive chat and specialized workflows
-   Workspace safety and path validation
-   Comprehensive logging and debugging support

## **Performance & Reliability**

### SWE-bench Inspired Improvements

-   **49% Target Achievement**: Architecture designed following Anthropic's breakthrough approach
-   **Error-Proofed Tools**: Extensive validation and error handling
-   **Context Engineering**: Research-preview conversation management techniques
-   **Model Empowerment**: Maximum control given to language models

### Reliability Enhancements

-   **Context Preservation**: Never lose important information during failures
-   **Recovery Strategies**: Multiple approaches for error handling
-   **Pattern Detection**: Automatic identification of recurring issues
-   **Comprehensive Logging**: Full audit trail of all agent actions

## **Technical Improvements**

### Architecture Refactoring

-   **Modular Design**: Separate modules for transparency, error recovery, and summarization
-   **Clean Interfaces**: Well-defined APIs between components
-   **Performance Optimization**: Efficient data structures and algorithms
-   **Error Handling**: Comprehensive error management throughout

### Code Quality

-   **Documentation**: Extensive inline documentation and examples
-   **Type Safety**: Strong typing with comprehensive error handling
-   **Testing**: Unit tests for core functionality
-   **Linting**: Clean, well-formatted code following Rust best practices

## **Key Features Summary**

### New Capabilities

1. **Complete Decision Transparency** - Every action tracked and explained
2. **Intelligent Error Recovery** - Learn from mistakes and adapt strategies

3. **Confidence Scoring** - Quality assessment for all agent actions
4. **Pattern Detection** - Identify and address recurring issues

### Enhanced User Experience

1. **Verbose Mode Overhaul** - Rich transparency and debugging information
2. **Better Error Messages** - Clear, actionable feedback for all failures
3. **Session Insights** - Comprehensive statistics and recommendations
4. **Improved Tool Reliability** - Error-proofed design prevents common issues
5. **Context Management** - Intelligent handling of conversation limits

## **Future Roadmap**

### Planned Enhancements

-   **Multi-file Operations**: Batch processing capabilities
-   **Project Templates**: Predefined scaffolds for common projects
-   **Integration APIs**: REST endpoints for external integration

### Research Areas

-   **Multi-modal Support**: Images, diagrams, and audio processing
-   **Collaborative Workflows**: Enhanced human-agent teaming
-   **Domain Specialization**: Industry-specific optimizations
-   **Performance Benchmarking**: SWE-bench style evaluation capabilities

## **Contributing**

### Development Guidelines

-   **Feature Branches**: Create feature branches for new capabilities
-   **Comprehensive Testing**: Include tests for all new functionality
-   **Documentation Updates**: Update README, BUILD.md, and this CHANGELOG
-   **Code Standards**: Follow established Rust idioms and best practices

### Areas of Interest

-   **Tool Enhancements**: Additional tools for specific use cases
-   **Workflow Patterns**: New specialized workflows and patterns
-   **Performance Optimization**: Further improvements for complex tasks
-   **Documentation**: Tutorials, examples, and user guides

---

## **Related Breakthroughs**

This release incorporates insights from Anthropic's engineering approach that achieved **49% on SWE-bench Verified**, including:

-   **Minimal Scaffolding**: Give maximum control to language models
-   **Error-Proofed Tools**: Extensive documentation and validation
-   **Thorough Reasoning**: Encourage deep thinking for complex problems
-   **Context Preservation**: Never lose important information during failures
-   **Decision Transparency**: Complete audit trail of agent actions

These improvements position vtcode as a state-of-the-art coding assistant with exceptional transparency, reliability, and performance on complex software engineering tasks.
