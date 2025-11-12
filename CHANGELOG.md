# Changelog - vtcode

All notable changes to vtcode will be documented in this file.

## [Version 0.43.0] - 2025-11-09
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
    - fix: correct tool name from run_command to run_terminal_cmd$'

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
    - refactor: rename RUN_TERMINAL_CMD to RUN_COMMAND for consistency$'

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
'    - fix: correct tool name from run_command to run_terminal_cmd
    - fix: add Debug trait to InlineTextStyle for improved logging$'

'### Refactors$'
'    - refactor: update LLM provider and model configurations
    - refactor: rename RUN_TERMINAL_CMD to RUN_COMMAND for consistency
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
- feat: add changelog generation from commits in release script
- feat: run doctests separately in publish_extracted_crates.sh
- feat: add comprehensive plan for open sourcing VTCode core components
- feat: add demo section with updated demo GIF in README
- feat: add VT Code VHS showcase and demo files

### Chores
- chore: update npm package to v0.33.1
- chore: update README.md for improved installation instructions and feature highlights
- chore: update CHANGELOG.md with recent enhancements for v0.33.0
- chore: release v0.33.0
- chore: update npm package to v0.33.0
- chore: update package versions to 0.32.0 and adjust dependencies
- chore: update npm package to v0.32.0
- chore: update demo GIF for VHS showcase



### Recent Enhancements (v0.33.0 and beyond)

- **Enhanced Tool Execution & Output Handling**: Improved tool execution with better error handling and output formatting for enhanced reliability and user experience
- **Enhanced Timeout Detection & Token Budget Management**: Improved timeout handling and more sophisticated token budget management with better attention management for enhanced performance

- **Improved Output Rendering**: Enhanced syntax highlighting for JSON, XML, and YAML outputs with better error messaging
- **Enhanced Bash Runner & Telemetry**: Added dry-run capabilities and feature-gated executors for shell operations with integrated telemetry
- **Ollama Integration Improvements**: Better support for local models with configurable base URLs and improved tool call handling
- **MCP Protocol & Tool Support**: Enhanced Model Context Protocol integration with improved resource and prompt handling
- **Configuration System Improvements**: Enhanced configuration handling with better default preservation and schema validation
- **Component Extraction Strategy**: Continued work on extracting reusable components including vtcode-exec-events, vtcode-bash-runner, vtcode-config, and vtcode-indexer

### Extracted crates release preparation

- **vtcode-commons 0.1.0** – marks the shared workspace path/telemetry traits crate ready for publishing with repository and
  documentation metadata in `Cargo.toml`.
- **vtcode-markdown-store 0.1.0** – aligns the markdown-backed storage crate with the initial release version and links to the
  public documentation.
- **vtcode-indexer 0.1.0** – retags the workspace-friendly indexer for its first standalone release and records the docs.rs URL
  for consumers.
- **vtcode-bash-runner 0.1.0** – updates the shell execution helper crate to the shared release version, adds licensing
  metadata, and points to hosted documentation.
- **vtcode-exec-events 0.1.0** – finalizes the telemetry schema crate for release with docs.rs metadata alongside the version
  alignment.

- Ran `cargo publish --dry-run` for the release candidates (`vtcode-commons`, `vtcode-markdown-store`, `vtcode-indexer`, `vtcode-exec-events`) and confirmed that `vtcode-bash-runner` will package successfully once `vtcode-commons` is available on crates.io.
- Scheduled the sequential publish order, tagging plan, and post-release dependency bumps in `docs/component_release_plan.md` so the crates can be released without coordination gaps.
- Scripted the sequential publish workflow in `scripts/publish_extracted_crates.sh` to automate validation, publishing, and tagging steps with optional dry-run rehearsals.

### `vtcode-exec-events`

- Added schema metadata (`EVENT_SCHEMA_VERSION`) and a `VersionedThreadEvent` wrapper so consumers can negotiate compatibility before processing telemetry streams.
- Introduced an `EventEmitter` trait with optional `LogEmitter` and `TracingEmitter` adapters to integrate JSON and tracing pipelines without boilerplate.
- Published JSON helper utilities and optional schema export support to simplify serialization round-trips and documentation workflows.

### `vtcode-bash-runner`

- Added feature-gated executors for process, pure-Rust, and dry-run operation so adopters can tailor shell execution strategies without forking the runner.【F:vtcode-bash-runner/Cargo.toml†L1-L40】【F:vtcode-bash-runner/src/executor.rs†L1-L356】
- Introduced the `EventfulExecutor` bridge to emit `vtcode-exec-events` telemetry from standalone shell invocations, plus documentation covering the new feature flags and integrations.【F:vtcode-bash-runner/src/executor.rs†L358-L470】【F:docs/vtcode_bash_runner.md†L1-L120】【F:docs/vtcode_exec_events.md†L1-L160】

### **Major Enhancements - Context Engineering & Attention Management** (Phase 1 & 2)

#### Phase 1: Enhanced System Prompts

- **Explicit Response Framework**: All system prompts now include a clear 5-step framework
  1. Assess the situation - Understand what the user needs
  2. Gather context efficiently - Use search tools before reading files
  3. Make precise changes - Prefer targeted edits over rewrites
  4. Verify outcomes - Test changes appropriately  
  5. Confirm completion - Summarize and verify satisfaction
- **Enhanced Guidelines**: More specific guidance on tool selection, code style preservation, and handling destructive operations
- **Multi-Turn Coherence**: Explicit guidance on building context across conversation turns
- **Token Efficiency**: Maintained concise prompts (~280 tokens) while adding structure

**System Prompt Improvements:**
- Default prompt: Added explicit framework, guidelines, and context management strategies
- Lightweight prompt: Added minimal 4-step approach for quick tasks
- Specialized prompt: Added tool selection strategy by phase, advanced guidelines, and multi-turn coherence

#### Phase 2: Dynamic Context Curation

- **New Module**: `context_curator.rs` - Implements iterative per-turn context selection based on Anthropic's principles
- **Conversation Phase Detection**: Automatically detects phase (Exploration, Implementation, Validation, Debugging, Unknown)
- **Phase-Aware Tool Selection**: Dynamically selects relevant tools based on current conversation phase
- **Priority-Based Context Selection**:
  1. Recent messages (always included, configurable)
  2. Active work context (files being modified)
  3. Decision ledger summary (compact)
  4. Recent errors and resolutions
  5. Relevant tools (phase-aware)

- **Configurable Curation**: Full control via `[context.curation]` configuration

**Key Features:**
- Tracks active files and file summaries
- Maintains recent error context for debugging
- Selects optimal tools based on conversation phase
- Respects token budget constraints
- Integrates with TokenBudgetManager and DecisionTracker

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

- **New Module**: `token_budget.rs` - Real-time token budget tracking using Hugging Face `tokenizers`
- **Component-Level Tracking**: Monitor token usage by category (system prompt, messages, tool results, decision ledger)
- **Configurable Thresholds**: Warning at 75% (customizable via `vtcode.toml`)
- **Model-Specific Tokenizers**: Support for GPT, Claude, and other models for accurate counting
- **Automatic Deduction**: Track token removal during context cleanup
- **Budget Reports**: Generate detailed token usage reports by component
- **Performance Optimized**: ~10μs per message using Rust-native Hugging Face `tokenizers`
- **New Method**: `remaining_tokens()` - Get remaining tokens in budget for context curation decisions

**Configuration:**
```toml
[context.token_budget]
enabled = true
model = "gpt-5-nano"
warning_threshold = 0.75
detailed_tracking = false
```

#### Optimized System Prompts & Tool Descriptions

- **67-82% Token Reduction**: System prompts streamlined from ~600 tokens to ~200 tokens
- **80% Tool Description Efficiency**: Average tool description reduced from ~400 to ~80 tokens
- **"Right Altitude" Principles**: Concise, actionable guidance over verbose instructions
- **Progressive Disclosure**: Emphasize search-first approach with `grep_file`
- **Clear Tool Purposes**: Eliminated capability overlap in tool descriptions
- **Token Management Guidance**: Built-in advice for efficient context usage (e.g., `max_results` parameters)

**System Prompt Improvements:**
- Removed verbose explanations and redundant information
- Focused on core principles and actionable strategies
- Added explicit context strategy guidance
- Emphasized metadata-first, content-second approach

**Tool Description Improvements:**
- Clear, unambiguous purposes with minimal overlap
- Token efficiency guidance (e.g., `max_results` limits)
- Auto-chunking behavior documented
- Metadata-first approach emphasized

#### Context Engineering Documentation

- **New Documentation**: `docs/context_engineering.md` - Comprehensive guide to context management
- **Implementation Summary**: `docs/context_engineering_implementation.md` - Technical details
- **Best Practices**: User and developer guidelines for efficient context usage
- **Configuration Examples**: Complete examples for token budget and context management
- **Performance Metrics**: Token efficiency improvements documented
- **References**: Links to Anthropic research and related resources

#### Bug Fixes

- **Fixed MCP Server Initialization**: Removed premature `cleanup_dead_providers()` call that caused `BrokenPipeError` during initialization
- **MCP Process Management**: Improved connection lifecycle management to prevent pipe closure issues

#### Dependencies

- **Added**: `tokenizers = "0.15"` for accurate token counting
- **Updated**: Cargo.lock with new dependencies

#### Release Automation

- **Cargo Release Integration**: Adopted `cargo release` with a shared workspace configuration (`release.toml`) and updated `scripts/release.sh` to drive changelog-powered GitHub releases, coordinated crates.io publishing, and npm version synchronization.

### **Major Enhancements - Anthropic-Inspired Architecture**

#### Decision Transparency System

- **New Module**: `decision_tracker.rs` - Complete audit trail of all agent decisions
- **Real-time Tracking**: Every action logged with reasoning and confidence scores
- **Transparency Reports**: Live decision summaries and session statistics
- **Confidence Scoring**: Quality assessment for all agent actions
- **Context Preservation**: Full conversation context maintained across decisions

#### Error Recovery & Resilience

- **New Module**: `error_recovery.rs` - Intelligent error handling system
- **Pattern Detection**: Automatic identification of recurring errors
- **Context Preservation**: Never lose important information during failures
- **Recovery Strategies**: Multiple approaches for handling errors gracefully
- **Error Statistics**: Comprehensive analysis of error patterns and recovery rates

#### Conversation Summarization

- **New Module**: `conversation_summarizer.rs` - Automatic conversation compression
- **Intelligent Summaries**: Key decisions, completed tasks, and error patterns
- **Long Session Support**: Automatic triggers when conversations exceed thresholds
- **Confidence Scoring**: Quality assessment for summary reliability
- **Context Efficiency**: Maintain useful context without hitting limits

### **Tool Design Improvements**

#### Enhanced Tool Documentation

- **Comprehensive Specifications**: Extensive tool descriptions with examples and error cases
- **Error-Proofing**: Anticipate and prevent common model misunderstandings
- **Clear Usage Guidelines**: Detailed instructions for each tool parameter
- **Debugging Support**: Specific guidance for troubleshooting tool failures

#### Improved System Instruction

- **Model-Driven Control**: Give maximum autonomy to the language model
- **Thorough Reasoning**: Encourage deep thinking for complex problems
- **Flexible Methodology**: Adaptable problem-solving approaches
- **Quality First**: Emphasize correctness over speed

### **Release Automation**

- **Coordinated Version Bumps**: `scripts/release.sh` now prompts maintainers to bump the `vtagent-core` crate alongside the main binary, keeping release metadata synchronized.

### **Transparency & Observability**

#### Verbose Mode Enhancements

- **Real-time Decision Tracking**: See exactly why each action is taken
- **Error Recovery Monitoring**: Observe intelligent error handling
- **Conversation Summarization Alerts**: Automatic notifications for long sessions
- **Session Statistics**: Comprehensive metrics and pattern analysis
- **Pattern Detection**: Automatic identification of recurring issues

#### Session Reporting

- **Final Transparency Reports**: Complete session summaries with success metrics
- **Error Recovery Statistics**: Analysis of error patterns and recovery rates
- **Decision Quality Metrics**: Confidence scores and decision success rates
- **Context Usage Monitoring**: Automatic warnings for approaching limits

### **Configuration System Improvements**

#### Two-Way Configuration Synchronization

- **Smart Config Generation**: `vtcode config` now reads existing `vtcode.toml` and preserves customizations
- **Complete Template Generation**: Ensures all configuration sections are present, even missing ones
- **Bidirectional Sync**: Generated configs always match your actual configuration state
- **Fallback Safety**: Uses system defaults when no configuration file exists
- **TOML Serialization**: Replaced hardcoded templates with proper TOML generation

## [Previous Versions]

### v0.1.0 - Initial Release

- Basic agent architecture with Gemini integration
- Core file system tools (list_files, read_file, write_file, edit_file)
- Interactive chat and specialized workflows
- Workspace safety and path validation
- Comprehensive logging and debugging support

## **Performance & Reliability**

### SWE-bench Inspired Improvements

- **49% Target Achievement**: Architecture designed following Anthropic's breakthrough approach
- **Error-Proofed Tools**: Extensive validation and error handling
- **Context Engineering**: Research-preview conversation management techniques
- **Model Empowerment**: Maximum control given to language models

### Reliability Enhancements

- **Context Preservation**: Never lose important information during failures
- **Recovery Strategies**: Multiple approaches for error handling
- **Pattern Detection**: Automatic identification of recurring issues
- **Comprehensive Logging**: Full audit trail of all agent actions

## **Technical Improvements**

### Architecture Refactoring

- **Modular Design**: Separate modules for transparency, error recovery, and summarization
- **Clean Interfaces**: Well-defined APIs between components
- **Performance Optimization**: Efficient data structures and algorithms
- **Error Handling**: Comprehensive error management throughout

### Code Quality

- **Documentation**: Extensive inline documentation and examples
- **Type Safety**: Strong typing with comprehensive error handling
- **Testing**: Unit tests for core functionality
- **Linting**: Clean, well-formatted code following Rust best practices

## **Key Features Summary**

### New Capabilities

1. **Complete Decision Transparency** - Every action tracked and explained
2. **Intelligent Error Recovery** - Learn from mistakes and adapt strategies

4. **Confidence Scoring** - Quality assessment for all agent actions
5. **Pattern Detection** - Identify and address recurring issues

### Enhanced User Experience

1. **Verbose Mode Overhaul** - Rich transparency and debugging information
2. **Better Error Messages** - Clear, actionable feedback for all failures
3. **Session Insights** - Comprehensive statistics and recommendations
4. **Improved Tool Reliability** - Error-proofed design prevents common issues
5. **Context Management** - Intelligent handling of conversation limits

## **Future Roadmap**

### Planned Enhancements

- **Multi-file Operations**: Batch processing capabilities
- **Project Templates**: Predefined scaffolds for common projects
- **Integration APIs**: REST endpoints for external integration


### Research Areas

- **Multi-modal Support**: Images, diagrams, and audio processing
- **Collaborative Workflows**: Enhanced human-agent teaming
- **Domain Specialization**: Industry-specific optimizations
- **Performance Benchmarking**: SWE-bench style evaluation capabilities

## **Contributing**

### Development Guidelines

- **Feature Branches**: Create feature branches for new capabilities
- **Comprehensive Testing**: Include tests for all new functionality
- **Documentation Updates**: Update README, BUILD.md, and this CHANGELOG
- **Code Standards**: Follow established Rust idioms and best practices

### Areas of Interest

- **Tool Enhancements**: Additional tools for specific use cases
- **Workflow Patterns**: New specialized workflows and patterns
- **Performance Optimization**: Further improvements for complex tasks
- **Documentation**: Tutorials, examples, and user guides

---

## **Related Breakthroughs**

This release incorporates insights from Anthropic's engineering approach that achieved **49% on SWE-bench Verified**, including:

- **Minimal Scaffolding**: Give maximum control to language models
- **Error-Proofed Tools**: Extensive documentation and validation
- **Thorough Reasoning**: Encourage deep thinking for complex problems
- **Context Preservation**: Never lose important information during failures
- **Decision Transparency**: Complete audit trail of agent actions

These improvements position vtcode as a state-of-the-art coding assistant with exceptional transparency, reliability, and performance on complex software engineering tasks.
