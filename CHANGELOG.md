# Changelog - vtcode

All notable changes to vtcode will be documented in this file.

## [Unreleased] - 2025-12-14
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
- **Comprehensive Skills Location System**: Implemented multi-location skill discovery with precedence handling
  - VTCode User Skills (`~/.vtcode/skills/`) - Highest precedence
  - VTCode Project Skills (`.vtcode/skills/`) - Project-specific skills  
  - Pi Framework Skills (`~/.pi/skills/`, `.pi/skills/`)
  - Claude Code Skills (`~/.claude/skills/`, `.claude/skills/`)
  - Codex CLI Skills (`~/.codex/skills/`)
- **Precedence System**: Skills from higher precedence locations override lower precedence skills with the same name
- **Migration Support**: All existing skills migrated from `.claude/skills` to `.vtcode/skills` with backward compatibility
- **Enhanced Skill Loader**: Updated loader to integrate with new location system while maintaining backward compatibility

### Changed
- Updated skills documentation to reflect new location system and precedence handling
- Enhanced skill discovery to support recursive scanning and proper name collision resolution

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
    - chore: update configuration files for VTCode support
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
    - chore: update configuration files for VTCode support
    - chore: update npm package.json to v0.43.17 [skip ci]
    - chore: release v0.43.17
    - chore: release v0.43.16
    - chore: update npm package.json to v0.43.15 [skip ci]
# [Version 0.44.0] - 2025-11-15


### Documentation
    - docs: update changelog for v0.43.17 [skip ci]
    - docs: update changelog for v0.43.16 [skip ci]


### Chores
    - chore: update configuration files for VTCode support
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
    - feat: Add VTCode Chat extension with MCP integration
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
    - feat: Add VTCode Chat extension with MCP integration
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
    - feat: Enhance development and release process for VTCode extension

### Bug Fixes

    - fix: update changelog generation to handle date formatting correctly
    - fix: rename VTCode Update Plan tool for consistency
    - fix: update language model tool properties for VTCode Update Plan

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
    - feat: Enhance development and release process for VTCode extension|

### Bug Fixes

    - fix: update changelog generation to handle date formatting correctly|
    - fix: rename VTCode Update Plan tool for consistency|
    - fix: update language model tool properties for VTCode Update Plan|

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
'    - feat: Enhance development and release process for VTCode extension$'

'### Bug Fixes$'
'    - fix: rename VTCode Update Plan tool for consistency
    - fix: update language model tool properties for VTCode Update Plan$'

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
'    - feat: Enhance development and release process for VTCode extension$'

'### Bug Fixes$'
'    - fix: rename VTCode Update Plan tool for consistency
    - fix: update language model tool properties for VTCode Update Plan$'

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
    - feat: Add initial files for VTCode Companion extension including README, LICENSE, CHANGELOG, and esbuild configuration
    - feat: Add initial package.json for VTCode Companion extension
    - feat(security): Implement comprehensive security documentation and fixes
    - feat: add comprehensive security audit and model documentation

### Bug Fixes

    - fix: remove mdbook workflow causing CI failure

### Refactors

    - refactor: Rename extension from "VTCode Companion" to "VTCode" and update CHANGELOG
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
    - feat: Add initial files for VTCode Companion extension including README, LICENSE, CHANGELOG, and esbuild configuration
    - feat: Add initial package.json for VTCode Companion extension
    - feat(security): Implement comprehensive security documentation and fixes
    - feat: add comprehensive security audit and model documentation
    - feat: add changelog generation from commits in release script

### Refactors

    - refactor: Rename extension from "VTCode Companion" to "VTCode" and update CHANGELOG
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
-   feat: add comprehensive plan for open sourcing VTCode core components
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
