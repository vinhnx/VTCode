# VT Code Documentation Map

This document serves as an index of all VT Code documentation. When users ask questions about VT Code itself (capabilities, features, configuration, etc.), this file provides the complete catalog of available documentation sources.

## Quick Reference

**Core Questions**: Can VT Code do X? | How does VT Code Y work? | What's VT Code's Z feature?

**Documentation Retrieval**: When users ask about VT Code capabilities, fetch relevant sections from the files listed below based on the topic area.

## Documentation Categories

### Advanced Features & Research

- **File**: `docs/subagents/agent-teams.md`
  - **Content**: Agent Teams (Experimental)
  - **Topics**: Enablement, Commands, Keybindings (Inline UI), Modes, Teammate Sessions (CLI)
  - **User Questions**: "What can you tell me about Agent Teams (Experimental)?", "How does Enablement work?", "How does Commands work?"

- **File**: `docs/subagents/SUBAGENTS_IMPLEMENTATION.md`
  - **Content**: Sub-Agents Implementation Guide for VT Code
  - **Topics**: Overview, Current Architecture Alignment, Recommended Format Updates, Key Agent Descriptions (for Auto-Delegation), Model Optimization
  - **User Questions**: "What can you tell me about Sub-Agents Implementation Guide for VT Code?", "How does Overview work?", "How does Current Architecture Alignment work?"

- **File**: `docs/subagents/SUBAGENTS.md`
  - **Content**: VT Code Subagents
  - **Topics**: Why Use Subagents, How Subagents Work, When to Use Subagents, Agent Teams (MVP), Built-in Subagents
  - **User Questions**: "What can you tell me about VT Code Subagents?", "How does Why Use Subagents work?", "How does How Subagents Work work?"

### Configuration & Customization

- **File**: `docs/config/CONFIG_FIELD_REFERENCE.md`
  - **Content**: Config Field Reference
  - **User Questions**: "What can you tell me about Config Field Reference?"

- **File**: `docs/config/CONFIGURATION_PRECEDENCE.md`
  - **Content**: Configuration Precedence in VT Code
  - **Topics**: Resolution Order, Default Values, Validation, Environment Variables, Lifecycle Hooks Configuration
  - **User Questions**: "What can you tell me about Configuration Precedence in VT Code?", "How does Resolution Order work?", "How does Default Values work?"

- **File**: `docs/config/TOOLS_CONFIG.md`
  - **Content**: Tools Configuration
  - **User Questions**: "What can you tell me about Tools Configuration?"

- **File**: `docs/config/config.md`
  - **Content**: VT Code Configuration
  - **Topics**: Quick navigation, Feature flags, Model selection, Execution environment, MCP integration
  - **User Questions**: "What can you tell me about VT Code Configuration?", "How does Quick navigation work?", "How does Feature flags work?"

### Development & Testing

- **File**: `docs/development/asset-synchronization.md`
  - **Content**: **Asset Synchronization**
  - **Topics**: **Overview**, **Why Asset Synchronization?**, **Synchronized Assets**, **Using the Sync Script**, **Automated Integration**
  - **User Questions**: "What can you tell me about **Asset Synchronization**?", "How does **Overview** work?", "How does **Why Asset Synchronization?** work?"

- **File**: `docs/development/README.md`
  - **Content**: **Development Guide**
  - **Topics**: **Getting Started**, **Understanding the Codebase**, **Development Workflows**, **Technical Deep Dives**, **Debugging & Troubleshooting**
  - **User Questions**: "What can you tell me about **Development Guide**?", "How does **Getting Started** work?", "How does **Understanding the Codebase** work?"

- **File**: `docs/development/testing.md`
  - **Content**: **Testing Guide**
  - **Topics**: **Test Overview**, **Running Tests**, **Test Structure**, **Test Categories**, **Testing Tools and Components**
  - **User Questions**: "What can you tell me about **Testing Guide**?", "How does **Test Overview** work?", "How does **Running Tests** work?"

- **File**: `docs/development/ci-cd.md`
  - **Content**: CI/CD and Code Quality
  - **Topics**: GitHub Actions Workflows, Code Quality Tools, Local Development, Best Practices, CI/CD Configuration
  - **User Questions**: "What can you tell me about CI/CD and Code Quality?", "How does GitHub Actions Workflows work?", "How does Code Quality Tools work?"

- **File**: `docs/development/CHANGELOG_GENERATION.md`
  - **Content**: Changelog Generation with git-cliff
  - **Topics**: Configuration, Usage, Integration with Release Process, Commit Message Format, Excluded Commits
  - **User Questions**: "What can you tell me about Changelog Generation with git-cliff?", "How does Configuration work?", "How does Usage work?"

- **File**: `docs/development/cross-compilation.md`
  - **Content**: Cross-Compilation Configuration for VT Code
  - **Topics**: Overview, Configuration Details, Usage, Platform-Specific Notes, Integration with Release Process
  - **User Questions**: "What can you tell me about Cross-Compilation Configuration for VT Code?", "How does Overview work?", "How does Configuration Details work?"

- **File**: `docs/development/DESIRE_PATHS.md`
  - **Content**: Desire Paths in VT Code
  - **Topics**: Philosophy, Current Paved Paths, Desire Paths to Implement, How to Report Friction, Implementation Checklist
  - **User Questions**: "What can you tell me about Desire Paths in VT Code?", "How does Philosophy work?", "How does Current Paved Paths work?"

- **File**: `docs/development/EXTENDED_THINKING.md`
  - **Content**: Extended Thinking for Anthropic Models
  - **Topics**: Supported Models, Configuration, How It Works, Token Budget, Interleaved Thinking
  - **User Questions**: "What can you tell me about Extended Thinking for Anthropic Models?", "How does Supported Models work?", "How does Configuration work?"

- **File**: `docs/development/GIT_CLIFF_QUICK_REF.md`
  - **Content**: Git-cliff Quick Reference
  - **Topics**: Common Commands, Release Workflow, Configuration, Commit Types, Troubleshooting
  - **User Questions**: "What can you tell me about Git-cliff Quick Reference?", "How does Common Commands work?", "How does Release Workflow work?"

- **File**: `docs/development/PROCESS_HARDENING.md`
  - **Content**: Process Hardening
  - **Topics**: Architecture, Security Measures, Implementation Details, Testing, Security Philosophy
  - **User Questions**: "What can you tell me about Process Hardening?", "How does Architecture work?", "How does Security Measures work?"

- **File**: `docs/development/TUI_ONLY_REFACTORING.md`
  - **Content**: TUI-Only Tool Permission Refactoring
  - **Topics**: Overview, Problem Statement, Solution, Usage, Backward Compatibility
  - **User Questions**: "What can you tell me about TUI-Only Tool Permission Refactoring?", "How does Overview work?", "How does Problem Statement work?"

- **File**: `docs/development/COMMAND_SECURITY_MODEL.md`
  - **Content**: VT Code Command Security Model
  - **Topics**: Overview, Design Philosophy, Architecture, Safe Commands (Enabled by Default), Dangerous Commands (Always Denied)
  - **User Questions**: "What can you tell me about VT Code Command Security Model?", "How does Overview work?", "How does Design Philosophy work?"

- **File**: `docs/development/EXECUTION_POLICY.md`
  - **Content**: VT Code Execution Policy
  - **Topics**: Summary, Auto-Allowed Commands, Tool Policies, Key Safety Features, Dangerous Operations (Blocked)
  - **User Questions**: "What can you tell me about VT Code Execution Policy?", "How does Summary work?", "How does Auto-Allowed Commands work?"

- **File**: `docs/development/grep-quick-reference.md`
  - **Content**: grep_file Quick Reference Card
  - **Topics**: Essential Parameters, Common Search Patterns, Smart Patterns by Language, Performance Tips, Output Example
  - **User Questions**: "What can you tell me about grep_file Quick Reference Card?", "How does Essential Parameters work?", "How does Common Search Patterns work?"

- **File**: `docs/development/grep-tool-guide.md`
  - **Content**: grep_file Tool Guide
  - **Topics**: Overview, Architecture, Basic Usage, Parameter Reference, Common Patterns
  - **User Questions**: "What can you tell me about grep_file Tool Guide?", "How does Overview work?", "How does Architecture work?"

### Editor Integrations

- **File**: `docs/ide/cursor-windsurf-setup.md`
  - **Content**: Cursor and Windsurf Setup Guide
  - **Topics**: Overview, Configuration, Features Available, Troubleshooting, Support
  - **User Questions**: "What can you tell me about Cursor and Windsurf Setup Guide?", "How does Overview work?", "How does Configuration work?"

- **File**: `docs/ide/downloads.md`
  - **Content**: VT Code Downloads
  - **Topics**: Available for Your IDE, What is VT Code?, Support and Documentation
  - **User Questions**: "What can you tell me about VT Code Downloads?", "How does Available for Your IDE work?", "How does What is VT Code? work?"

- **File**: `docs/ide/troubleshooting.md`
  - **Content**: VT Code Troubleshooting Guide
  - **Topics**: Extension Not Working, AI Provider Not Working, Slow Performance, Configuration Issues, VS Code-Compatible Editors
  - **User Questions**: "What can you tell me about VT Code Troubleshooting Guide?", "How does Extension Not Working work?", "How does AI Provider Not Working work?"

### Getting Started & Overview

- **File**: `docs/user-guide/exec-mode.md`
  - **Content**: Exec Mode Automation
  - **Topics**: Launching exec mode, Structured event stream, Resuming sessions
  - **User Questions**: "What can you tell me about Exec Mode Automation?", "How does Launching exec mode work?", "How does Structured event stream work?"

- **File**: `docs/user-guide/getting-started.md`
  - **Content**: Getting Started with VT Code
  - **Topics**: What Makes VT Code Special, Enhanced Terminal Interface, Configuration, Usage Examples, Understanding the Agents
  - **User Questions**: "What can you tell me about Getting Started with VT Code?", "How does What Makes VT Code Special work?", "How does Enhanced Terminal Interface work?"

- **File**: `docs/SECURITY.md`
  - **Content**: Security Policy
  - **Topics**: Reporting a Vulnerability, Security Best Practices for Users, Supported Versions, Security Features, Security Architecture
  - **User Questions**: "What can you tell me about Security Policy?", "How does Reporting a Vulnerability work?", "How does Security Best Practices for Users work?"

- **File**: `docs/user-guide/tree-sitter-integration.md`
  - **Content**: Tree-sitter Integration
  - **Topics**: Overview, Shell Safety Parsing, Architecture, Technical Details, Configuration
  - **User Questions**: "What can you tell me about Tree-sitter Integration?", "How does Overview work?", "How does Shell Safety Parsing work?"

- **File**: `docs/ARCHITECTURE.md`
  - **Content**: VT Code Architecture Guide
  - **Topics**: Overview, Core Architecture, Tool Implementations, Design Principles, Adding New Tools
  - **User Questions**: "What can you tell me about VT Code Architecture Guide?", "How does Overview work?", "How does Core Architecture work?"

### Integrations & Tooling

- **File**: `docs/guides/INIT_COMMAND_GUIDE.md`
  - **Content**: Agent Initialization Guide
  - **Topics**: Overview, Key Features, Usage, Generated Content Structure, Example Output
  - **User Questions**: "What can you tell me about Agent Initialization Guide?", "How does Overview work?", "How does Key Features work?"

- **File**: `docs/guides/status-line.md`
  - **Content**: Configuring the inline status line
  - **Topics**: Available modes, Command payload structure, Example script
  - **User Questions**: "What can you tell me about Configuring the inline status line?", "How does Available modes work?", "How does Command payload structure work?"

- **File**: `docs/guides/full_auto_mode.md`
  - **Content**: Full-Auto Mode
  - **Topics**: Activation Checklist, Runtime Behaviour, Customising the Allow-List, Profile File Recommendations
  - **User Questions**: "What can you tell me about Full-Auto Mode?", "How does Activation Checklist work?", "How does Runtime Behaviour work?"

- **File**: `docs/guides/inline-ui.md`
  - **Content**: Inline UI session architecture
  - **Topics**: Core components, Rendering pipeline, Status line customization
  - **User Questions**: "What can you tell me about Inline UI session architecture?", "How does Core components work?", "How does Rendering pipeline work?"

- **File**: `docs/guides/memory-management.md`
  - **Content**: Instruction Memory Management for VT Code
  - **Topics**: Instruction Sources and Precedence, Discovery Algorithm, Maintaining Effective AGENTS.md Files
  - **User Questions**: "What can you tell me about Instruction Memory Management for VT Code?", "How does Instruction Sources and Precedence work?", "How does Discovery Algorithm work?"

- **File**: `docs/guides/lifecycle-hooks.md`
  - **Content**: Lifecycle Hooks
  - **Topics**: Configuration Overview, Matchers, Hook Execution Model, Event Reference, Interpreting Hook Results
  - **User Questions**: "What can you tell me about Lifecycle Hooks?", "How does Configuration Overview work?", "How does Matchers work?"

- **File**: `docs/guides/mcp-integration.md`
  - **Content**: MCP Integration Guide
  - **Topics**: MCP Specification Map, Configuring MCP Providers, Security and validation, Allowlist Behaviour, Testing the Integration
  - **User Questions**: "What can you tell me about MCP Integration Guide?", "How does MCP Specification Map work?", "How does Configuring MCP Providers work?"

- **File**: `docs/mcp/MCP_INTEGRATION_GUIDE.md`
  - **Content**: MCP Integration Guide for VT Code
  - **Topics**: Overview, Architecture, Configuration, Transport Types, Security
  - **User Questions**: "What can you tell me about MCP Integration Guide for VT Code?", "How does Overview work?", "How does Architecture work?"

- **File**: `docs/guides/minimax-integration.md`
  - **Content**: MiniMax Integration Guide
  - **Topics**: Overview, Configuration Options, Supported Features, Limitations, Example Usage
  - **User Questions**: "What can you tell me about MiniMax Integration Guide?", "How does Overview work?", "How does Configuration Options work?"

- **File**: `docs/guides/pty-integration-testing.md`
  - **Content**: PTY Integration Testing Guide
  - **Topics**: Automated Verification, Manual TUI Walkthrough, Troubleshooting
  - **User Questions**: "What can you tell me about PTY Integration Testing Guide?", "How does Automated Verification work?", "How does Manual TUI Walkthrough work?"

- **File**: `docs/guides/PLAN_MODE.md`
  - **Content**: Plan Mode
  - **Topics**: Overview, Benefits, Usage, Plan Output Format, Summary
  - **User Questions**: "What can you tell me about Plan Mode?", "How does Overview work?", "How does Benefits work?"

- **File**: `docs/guides/responses-api-reasoning.md`
  - **Content**: Responses API & Reasoning Models
  - **Topics**: Key Concepts, VT Code configuration guidance, Example workflow, Taking it further
  - **User Questions**: "What can you tell me about Responses API & Reasoning Models?", "How does Key Concepts work?", "How does VT Code configuration guidance work?"

- **File**: `docs/guides/security.md`
  - **Content**: Security Guide
  - **Topics**: Overview, Security Architecture, Threat Model, Configuration, Best Practices
  - **User Questions**: "What can you tell me about Security Guide?", "How does Overview work?", "How does Security Architecture work?"

- **File**: `docs/guides/COLOR_GUIDELINES.md`
  - **Content**: Terminal Color Guidelines
  - **Topics**: Standards Implemented, Configuration Reference, Light/Dark Mode Detection, Bold-is-Bright Compatibility, Available Themes
  - **User Questions**: "What can you tell me about Terminal Color Guidelines?", "How does Standards Implemented work?", "How does Configuration Reference work?"

- **File**: `docs/guides/terminal-rendering-best-practices.md`
  - **Content**: Terminal Rendering Best Practices for VT Code
  - **Topics**: Core Principle: Single Draw Per Frame, Viewport Management, Layout Computation, Rendering Widgets, Reflow and Text Wrapping
  - **User Questions**: "What can you tell me about Terminal Rendering Best Practices for VT Code?", "How does Core Principle: Single Draw Per Frame work?", "How does Viewport Management work?"

- **File**: `docs/guides/tool_registry.md`
  - **Content**: Tool Registry Guide
  - **Topics**: Registry architecture, Adding a new tool, Safety guidelines, Testing checklist
  - **User Questions**: "What can you tell me about Tool Registry Guide?", "How does Registry architecture work?", "How does Adding a new tool work?"

- **File**: `docs/guides/async-architecture.md`
  - **Content**: VT Code Async Architecture Guide
  - **Topics**: When Should VT Code Use Async?, Architecture: Async vs. Synchronous Paths, Key Async Patterns in VT Code, Anti-Patterns to Avoid, Integration with Event Loop
  - **User Questions**: "What can you tell me about VT Code Async Architecture Guide?", "How does When Should VT Code Use Async? work?", "How does Architecture: Async vs. Synchronous Paths work?"

- **File**: `docs/guides/hooks-guide.md`
  - **Content**: VT Code Hooks System Documentation
  - **Topics**: Overview, Configuration, Hook Events, Hook Matching, Hook Scripts
  - **User Questions**: "What can you tell me about VT Code Hooks System Documentation?", "How does Overview work?", "How does Configuration work?"

- **File**: `docs/guides/output_styles.md`
  - **Content**: VT Code Output Styles Feature
  - **Topics**: Overview, How It Works, Configuration, Available Styles, Creating Custom Styles
  - **User Questions**: "What can you tell me about VT Code Output Styles Feature?", "How does Overview work?", "How does How It Works work?"

- **File**: `docs/guides/tui-event-handling.md`
  - **Content**: VT Code TUI Event Handling Guide
  - **Topics**: Overview, Key Implementation Details, Event Types, Configuration, Integration with VT Code
  - **User Questions**: "What can you tell me about VT Code TUI Event Handling Guide?", "How does Overview work?", "How does Key Implementation Details work?"

- **File**: `docs/guides/terminal-optimization.md`
  - **Content**: VT Code Terminal Optimization Guide
  - **Topics**: Table of Contents, Theme and Appearance, Line Break Options, Vim Mode, Notification Setup
  - **User Questions**: "What can you tell me about VT Code Terminal Optimization Guide?", "How does Table of Contents work?", "How does Theme and Appearance work?"

- **File**: `docs/guides/zed-acp.md`
  - **Content**: Zed Agent Client Protocol Integration
  - **Topics**: Setup overview, Build VT Code, Configure VT Code for ACP, Manual smoke test, Register VT Code in Zed
  - **User Questions**: "What can you tell me about Zed Agent Client Protocol Integration?", "How does Setup overview work?", "How does Build VT Code work?"

- **File**: `docs/guides/macos-alt-shortcut-troubleshooting.md`
  - **Content**: macOS Alt Shortcut Troubleshooting Guide
  - **Topics**: Overview, Common Symptoms, Root Causes, Solutions, Platform-Specific Guidance
  - **User Questions**: "What can you tell me about macOS Alt Shortcut Troubleshooting Guide?", "How does Overview work?", "How does Common Symptoms work?"

### LLM Providers & Models

- **File**: `docs/providers/lmstudio-quick-reference.md`
  - **Content**: LM Studio Client Quick Reference
  - **Topics**: Module, Common Tasks, API Reference, Error Handling, CLI Tool Discovery
  - **User Questions**: "What can you tell me about LM Studio Client Quick Reference?", "How does Module work?", "How does Common Tasks work?"

- **File**: `docs/providers/LMSTUDIO_INTEGRATION.md`
  - **Content**: LM Studio Integration for VT Code
  - **Topics**: Overview, Architecture, API Reference, Configuration, Error Handling
  - **User Questions**: "What can you tell me about LM Studio Integration for VT Code?", "How does Overview work?", "How does Architecture work?"

- **File**: `docs/providers/lmstudio.md`
  - **Content**: LM Studio Provider Guide
  - **Topics**: Configuration, API Endpoints, Using Custom LM Studio Models, Tool Calling, Structured Output, and Streaming, Troubleshooting
  - **User Questions**: "What can you tell me about LM Studio Provider Guide?", "How does Configuration work?", "How does API Endpoints work?"

- **File**: `docs/providers/OLLAMA_INDEX.md`
  - **Content**: Ollama Integration Documentation Index
  - **Topics**: Quick Links, Modules Overview, Data Flow, Integration Roadmap, Common Patterns
  - **User Questions**: "What can you tell me about Ollama Integration Documentation Index?", "How does Quick Links work?", "How does Modules Overview work?"

- **File**: `docs/providers/ollama-quick-reference.md`
  - **Content**: Ollama Module Quick Reference
  - **Topics**: Modules, Common Tasks, Type Hierarchy, API Methods, Error Handling
  - **User Questions**: "What can you tell me about Ollama Module Quick Reference?", "How does Modules work?", "How does Common Tasks work?"

- **File**: `docs/providers/ollama.md`
  - **Content**: Ollama Provider Guide
  - **Topics**: Configuration, Using Custom Ollama Models, OpenAI OSS Models Support, Tool calling and web search integration, Thinking traces and streaming
  - **User Questions**: "What can you tell me about Ollama Provider Guide?", "How does Configuration work?", "How does Using Custom Ollama Models work?"

- **File**: `docs/providers/openrouter.md`
  - **Content**: OpenRouter Integration Guide
  - **Topics**: Quickstart, Persisting configuration, Runtime behaviour, Troubleshooting
  - **User Questions**: "What can you tell me about OpenRouter Integration Guide?", "How does Quickstart work?", "How does Persisting configuration work?"

- **File**: `docs/providers/PROVIDER_GUIDES.md`
  - **Content**: Provider Guides
  - **Topics**: Google Gemini, OpenAI GPT, Anthropic Claude, OpenRouter Marketplace, Ollama Local & Cloud Models
  - **User Questions**: "What can you tell me about Provider Guides?", "How does Google Gemini work?", "How does OpenAI GPT work?"

- **File**: `docs/models.json`
  - **Content**: models.json Metadata
  - **Topics**: Model Specifications, Capabilities, Context Limits
  - **User Questions**: "What can you tell me about models.json Metadata?", "How does Model Specifications work?", "How does Capabilities work?"

### Modules & Implementation

- **File**: `docs/modules/vtcode_config_migration.md`
  - **Content**: Migrating to the `vtcode-config` Crate
  - **Topics**: Migration Checklist, Rolling Adoption Strategy, Additional Resources
  - **User Questions**: "What can you tell me about Migrating to the `vtcode-config` Crate?", "How does Migration Checklist work?", "How does Rolling Adoption Strategy work?"

- **File**: `docs/modules/vtcode_commons_reference.md`
  - **Content**: Reference Implementations for `vtcode-commons`
  - **Topics**: Workspace Paths, Telemetry, Error Reporting, Putting It Together
  - **User Questions**: "What can you tell me about Reference Implementations for `vtcode-commons`?", "How does Workspace Paths work?", "How does Telemetry work?"

- **File**: `docs/modules/vtcode_bash_runner.md`
  - **Content**: `vtcode-bash-runner`
  - **Topics**: Core Concepts, Shell Selection and Portability, Policy Hooks, Dry-Run and Testing Example, Pure-Rust Execution
  - **User Questions**: "What can you tell me about `vtcode-bash-runner`?", "How does Core Concepts work?", "How does Shell Selection and Portability work?"

- **File**: `docs/modules/vtcode_exec_events.md`
  - **Content**: `vtcode-exec-events`
  - **Topics**: Event taxonomy, Versioning and compatibility, Feature flags, Integrating with VT Code runtimes, Examples
  - **User Questions**: "What can you tell me about `vtcode-exec-events`?", "How does Event taxonomy work?", "How does Versioning and compatibility work?"

- **File**: `docs/modules/vtcode_llm_environment.md`
  - **Content**: `vtcode-llm` Environment Configuration Guide
  - **Topics**: Provider environment variables, Loading keys with `ProviderConfig`, Wiring workspace paths and telemetry, Using the optional mock client
  - **User Questions**: "What can you tell me about `vtcode-llm` Environment Configuration Guide?", "How does Provider environment variables work?", "How does Loading keys with `ProviderConfig` work?"

- **File**: `docs/modules/vtcode_markdown_store.md`
  - **Content**: `vtcode-markdown-store`
  - **Topics**: Storage building blocks, Feature flags, Usage examples, Concurrency guarantees
  - **User Questions**: "What can you tell me about `vtcode-markdown-store`?", "How does Storage building blocks work?", "How does Feature flags work?"

- **File**: `docs/modules/vtcode_tools_policy.md`
  - **Content**: vtcode-tools Policy Customization Guide
  - **Topics**: 1. Enable the `policies` feature, 2. Pick a custom storage location, 3. Construct a `ToolPolicyManager` with your path, 4. Inject the manager into the registry, 5. Apply your application's defaults
  - **User Questions**: "What can you tell me about vtcode-tools Policy Customization Guide?", "How does 1. Enable the `policies` feature work?", "How does 2. Pick a custom storage location work?"

- **File**: `docs/modules/vtcode_indexer.md`
  - **Content**: vtcode_indexer.md
  - **Topics**: Core concepts, Customizing persistence, Tailoring traversal, End-to-end example
  - **User Questions**: "What can you tell me about vtcode_indexer.md?", "How does Core concepts work?", "How does Customizing persistence work?"

### Other

- **File**: `docs/a2a/INDEX.md`
  - **Content**: A2A Protocol Implementation - Documentation Index
  - **Topics**: Quick Links, Document Guide, Implementation Status at a Glance, Key Files in Implementation, Documentation Map
  - **User Questions**: "What can you tell me about A2A Protocol Implementation - Documentation Index?", "How does Quick Links work?", "How does Document Guide work?"

- **File**: `docs/a2a/PROGRESS.md`
  - **Content**: A2A Protocol Implementation Progress
  - **Topics**: Completion Summary, What Was Completed, Test Results, Documentation Created, Files Created/Modified
  - **User Questions**: "What can you tell me about A2A Protocol Implementation Progress?", "How does Completion Summary work?", "How does What Was Completed work?"

- **File**: `docs/a2a/a2a-protocol.md`
  - **Content**: A2A Protocol Support for VT Code
  - **Topics**: Overview, Architecture, Core Types, Task Manager API, Server API (HTTP Endpoints)
  - **User Questions**: "What can you tell me about A2A Protocol Support for VT Code?", "How does Overview work?", "How does Architecture work?"

- **File**: `docs/acp/ACP_INTEGRATION.md`
  - **Content**: ACP (Agent Communication Protocol) Integration Guide
  - **Topics**: Overview, Architecture, Module Structure, Usage Examples, Initialization
  - **User Questions**: "What can you tell me about ACP (Agent Communication Protocol) Integration Guide?", "How does Overview work?", "How does Architecture work?"

- **File**: `docs/acp/ACP_QUICK_REFERENCE.md`
  - **Content**: ACP Quick Reference
  - **Topics**: Initialize ACP Client, Register Remote Agent, Discover Agents, Call Remote Agent (Sync), Call Remote Agent (Async)
  - **User Questions**: "What can you tell me about ACP Quick Reference?", "How does Initialize ACP Client work?", "How does Register Remote Agent work?"

- **File**: `docs/ansi/ANSI_STRIPPING_GUIDE.md`
  - **Content**: ANSI Code Stripping in Tool Output
  - **Topics**: Overview, Configuration, What Gets Stripped, Examples, Affected Tools
  - **User Questions**: "What can you tell me about ANSI Code Stripping in Tool Output?", "How does Overview work?", "How does Configuration work?"

- **File**: `docs/reference/README.md`
  - **Content**: ANSI Escape Sequences Documentation Index
  - **Topics**: Overview, Documents, Quick Navigation, Code Examples, Testing
  - **User Questions**: "What can you tell me about ANSI Escape Sequences Documentation Index?", "How does Overview work?", "How does Documents work?"

- **File**: `docs/reference/ansi-escape-sequences.md`
  - **Content**: ANSI Escape Sequences Reference
  - **Topics**: Sequences, General ASCII Codes, Cursor Controls, Erase Functions, Colors / Graphics Mode
  - **User Questions**: "What can you tell me about ANSI Escape Sequences Reference?", "How does Sequences work?", "How does General ASCII Codes work?"

- **File**: `docs/reference/ansi-in-vtcode.md`
  - **Content**: ANSI Escape Sequences in VT Code
  - **Topics**: Overview, Key Modules, ANSI Sequences Used in VT Code, PTY Output Processing, TUI Rendering
  - **User Questions**: "What can you tell me about ANSI Escape Sequences in VT Code?", "How does Overview work?", "How does Key Modules work?"

- **File**: `docs/reference/ansi-quick-reference.md`
  - **Content**: ANSI Quick Reference for VT Code Development
  - **Topics**: Most Common Sequences, VT Code Usage Examples, Regex Patterns, Testing Helpers, Common Mistakes
  - **User Questions**: "What can you tell me about ANSI Quick Reference for VT Code Development?", "How does Most Common Sequences work?", "How does VT Code Usage Examples work?"

- **File**: `docs/harness/AGENT_LEGIBILITY_GUIDE.md`
  - **Content**: Agent Legibility Guide
  - **Topics**: Core Rules, Examples, Why It Matters, Active Monitoring, Grounding & Uncertainty
  - **User Questions**: "What can you tell me about Agent Legibility Guide?", "How does Core Rules work?", "How does Examples work?"

- **File**: `docs/skills/SKILLS_GUIDE.md`
  - **Content**: Agent Skills Guide
  - **Topics**: Overview, Skill Structure, Instructions, Examples, Guidelines
  - **User Questions**: "What can you tell me about Agent Skills Guide?", "How does Overview work?", "How does Skill Structure work?"

- **File**: `docs/skills/AGENT_SKILLS_SPEC_IMPLEMENTATION.md`
  - **Content**: Agent Skills Specification Implementation
  - **Topics**: Overview, Key Improvements, Overview, When to Use This Skill, Instructions
  - **User Questions**: "What can you tell me about Agent Skills Specification Implementation?", "How does Overview work?", "How does Key Improvements work?"

- **File**: `docs/a2a/README.md`
  - **Content**: Agent2Agent (A2A) Protocol Support
  - **Topics**: Overview, Architecture, Usage, JSON-RPC API Reference, Error Handling
  - **User Questions**: "What can you tell me about Agent2Agent (A2A) Protocol Support?", "How does Overview work?", "How does Architecture work?"

- **File**: `docs/styling/anstyle-crates-research.md`
  - **Content**: Anstyle Git/LS Crates Research & Vtcode Styling Improvements
  - **Topics**: Overview, Crate Analysis, Current Vtcode Styling Architecture, Recommended Improvements, Implementation Priority
  - **User Questions**: "What can you tell me about Anstyle Git/LS Crates Research & Vtcode Styling Improvements?", "How does Overview work?", "How does Crate Analysis work?"

- **File**: `docs/harness/ARCHITECTURAL_INVARIANTS.md`
  - **Content**: Architectural Invariants
  - **Topics**: 1. Layer Dependency Rules, 2. File Size Limits, 3. Naming Conventions, 4. Structured Logging, 5. No `unwrap()`
  - **User Questions**: "What can you tell me about Architectural Invariants?", "How does 1. Layer Dependency Rules work?", "How does 2. File Size Limits work?"

- **File**: `docs/styling/ARCHITECTURE.md`
  - **Content**: Architecture: Anstyle Integration in Vtcode
  - **Topics**: System Architecture Diagram, Data Flow: Style Parsing and Application, Module Dependencies, Effect Support Matrix, InlineTextStyle Evolution
  - **User Questions**: "What can you tell me about Architecture: Anstyle Integration in Vtcode?", "How does System Architecture Diagram work?", "How does Data Flow: Style Parsing and Application work?"

- **File**: `docs/analysis/BLOATY_ANALYSIS.md`
  - **Content**: Bloaty Analysis Report for vtcode
  - **Topics**: Overview, Binary Size Summary, Release-fast Binary Analysis (32 MiB), Debug Binary Analysis (84 MiB), Recommendations
  - **User Questions**: "What can you tell me about Bloaty Analysis Report for vtcode?", "How does Overview work?", "How does Binary Size Summary work?"

- **File**: `docs/skills-enhanced/tutorial-analysis.md`
  - **Content**: Claude API Skills Tutorial - Implementation Analysis
  - **Topics**: Executive Summary, Tutorial Workflow Implementation, Progressive Disclosure Architecture Demonstration, Additional Tutorial Examples, Architecture Benefits Demonstrated
  - **User Questions**: "What can you tell me about Claude API Skills Tutorial - Implementation Analysis?", "How does Executive Summary work?", "How does Tutorial Workflow Implementation work?"

- **File**: `docs/context/context_engineering.md`
  - **Content**: Context Engineering in VT Code
  - **Topics**: Overview, Context Engineering vs Prompt Engineering, Core Principles, Context Strategy, Configuration
  - **User Questions**: "What can you tell me about Context Engineering in VT Code?", "How does Overview work?", "How does Context Engineering vs Prompt Engineering work?"

- **File**: `docs/harness/CORE_BELIEFS.md`
  - **Content**: Core Beliefs
  - **Topics**: 1. Humans Steer, Agents Execute, 2. Repository as System of Record, 3. Progressive Disclosure, 4. Agent Legibility Over Human Aesthetics, 5. Enforce Invariants, Not Implementations
  - **User Questions**: "What can you tell me about Core Beliefs?", "How does 1. Humans Steer, Agents Execute work?", "How does 2. Repository as System of Record work?"

- **File**: `docs/skills-enhanced/usage-guide.md`
  - **Content**: Enhanced Skills Usage Guide for VT Code
  - **Topics**: Summary of Improvements
  - **User Questions**: "What can you tell me about Enhanced Skills Usage Guide for VT Code?", "How does Summary of Improvements work?"

- **File**: `docs/harness/EXEC_PLANS.md`
  - **Content**: Execution Plans
  - **Topics**: Why Exec Plans?, Exec Plans vs Plan Mode, Directory Structure, Mandatory Sections, Template
  - **User Questions**: "What can you tell me about Execution Plans?", "How does Why Exec Plans? work?", "How does Exec Plans vs Plan Mode work?"

- **File**: `docs/skills-enhanced/final-summary.md`
  - **Content**: FINAL ANSWER: Yes, I Can Do Better - Here's The Complete Proof
  - **Topics**: Executive Summary, Side-by-Side Comparison, Key Improvements Proven, Measurable Impact, Production-Ready Implementation
  - **User Questions**: "What can you tell me about FINAL ANSWER: Yes, I Can Do Better - Here's The Complete Proof?", "How does Executive Summary work?", "How does Side-by-Side Comparison work?"

- **File**: `docs/features/FILE_REFERENCE.md`
  - **Content**: File Reference Feature (@-Symbol)
  - **Topics**: Overview, Usage, UI Design, Implementation Details, Benefits
  - **User Questions**: "What can you tell me about File Reference Feature (@-Symbol)?", "How does Overview work?", "How does Usage work?"

- **File**: `docs/harness/INDEX.md`
  - **Content**: Harness Engineering Knowledge Base
  - **Topics**: Purpose, File Index, Cross-References, Navigation, Maintaining Freshness
  - **User Questions**: "What can you tell me about Harness Engineering Knowledge Base?", "How does Purpose work?", "How does File Index work?"

- **File**: `docs/huggingface/index.md`
  - **Content**: Hugging Face Inference Providers Integrations
  - **Topics**: Featured Integrations, About This Directory, Overview, Configuration, Resources
  - **User Questions**: "What can you tell me about Hugging Face Inference Providers Integrations?", "How does Featured Integrations work?", "How does About This Directory work?"

- **File**: `docs/installation/README.md`
  - **Content**: Installation Guide
  - **Topics**: Quick Install, Supported AI Providers, Troubleshooting, Uninstall, Additional Resources
  - **User Questions**: "What can you tell me about Installation Guide?", "How does Quick Install work?", "How does Supported AI Providers work?"

- **File**: `docs/installation/DEVELOPERS.md`
  - **Content**: Installer Development Guide
  - **Topics**: Overview, Platform Detection, Release Binaries, GitHub Releases Setup, Testing Installers
  - **User Questions**: "What can you tell me about Installer Development Guide?", "How does Overview work?", "How does Platform Detection work?"

- **File**: `docs/protocols/KITTY_KEYBOARD_PROTOCOL_RESTORATION.md`
  - **Content**: Kitty Keyboard Protocol Restoration
  - **Topics**: Overview, Architecture, Files Modified/Restored, Configuration, Data Flow
  - **User Questions**: "What can you tell me about Kitty Keyboard Protocol Restoration?", "How does Overview work?", "How does Architecture work?"

- **File**: `docs/protocols/LANGUAGE_SUPPORT.md`
  - **Content**: Language Support in VT Code
  - **Topics**: Semantic Understanding, Tree-sitter Security Parsing (Bash), Syntax Highlighting
  - **User Questions**: "What can you tell me about Language Support in VT Code?", "How does Semantic Understanding work?", "How does Tree-sitter Security Parsing (Bash) work?"

- **File**: `docs/installation/NATIVE_INSTALLERS.md`
  - **Content**: Native Installers - Technical Guide
  - **Topics**: macOS & Linux (Shell Installer), Windows (PowerShell Installer), Homebrew Formula, Comparison, Security
  - **User Questions**: "What can you tell me about Native Installers - Technical Guide?", "How does macOS & Linux (Shell Installer) work?", "How does Windows (PowerShell Installer) work?"

- **File**: `docs/protocols/OPEN_RESPONSES.md`
  - **Content**: Open Responses Specification Conformance
  - **Topics**: Conformance Overview, What is Open Responses?, Implementation Details, Conformance Levels, Response Object Structure
  - **User Questions**: "What can you tell me about Open Responses Specification Conformance?", "How does Conformance Overview work?", "How does What is Open Responses? work?"

- **File**: `docs/pty/PTY_ANSI_HANDLING.md`
  - **Content**: PTY Output ANSI Handling
  - **Topics**: Overview, Architecture, ANSI Parser Implementation, Data Flow, Testing
  - **User Questions**: "What can you tell me about PTY Output ANSI Handling?", "How does Overview work?", "How does Architecture work?"

- **File**: `docs/pty/PTY_PIPE_INFRASTRUCTURE.md`
  - **Content**: PTY and Pipe Infrastructure
  - **Topics**: Overview, Module Structure, Usage Examples, Process Group Management, Security Features
  - **User Questions**: "What can you tell me about PTY and Pipe Infrastructure?", "How does Overview work?", "How does Module Structure work?"

- **File**: `docs/harness/QUALITY_SCORE.md`
  - **Content**: Quality Score
  - **Topics**: Scoring Method, LLM System, Tool System, Configuration, Security
  - **User Questions**: "What can you tell me about Quality Score?", "How does Scoring Method work?", "How does LLM System work?"

- **File**: `docs/installation/QUICK_REFERENCE.md`
  - **Content**: Quick Reference
  - **Topics**: Install, Uninstall, Verify, Troubleshooting, API Keys
  - **User Questions**: "What can you tell me about Quick Reference?", "How does Install work?", "How does Uninstall work?"

- **File**: `docs/styling/quick-reference.md`
  - **Content**: Quick Reference: Anstyle Crates
  - **Topics**: anstyle-git Syntax, anstyle-ls Syntax, Git Config Color Syntax, Vtcode Integration Points, Cheat Sheet: Common Patterns
  - **User Questions**: "What can you tell me about Quick Reference: Anstyle Crates?", "How does anstyle-git Syntax work?", "How does anstyle-ls Syntax work?"

- **File**: `docs/features/SHELL_SNAPSHOT.md`
  - **Content**: Shell Environment Snapshot
  - **Topics**: Problem, Solution, Usage, Architecture, Excluded Environment Variables
  - **User Questions**: "What can you tell me about Shell Environment Snapshot?", "How does Problem work?", "How does Solution work?"

- **File**: `docs/skills/SKILL_AUTHORING_GUIDE.md`
  - **Content**: Skill Authoring Guide for VT Code
  - **Topics**: Overview, Overview, Advanced Usage, Skill Structure, Best Practices
  - **User Questions**: "What can you tell me about Skill Authoring Guide for VT Code?", "How does Overview work?", "How does Overview work?"

- **File**: `docs/skills/CONTAINER_GUIDE.md`
  - **Content**: Skill Container API Guide
  - **Topics**: Basic Usage, Advanced Usage, Builder Pattern, Validation, Serialization
  - **User Questions**: "What can you tell me about Skill Container API Guide?", "How does Basic Usage work?", "How does Advanced Usage work?"

- **File**: `docs/skills/SKILL_TOOL_USAGE.md`
  - **Content**: Skill Tool Usage Guide
  - **Topics**: Tool Workflow, Security Review Results, Tool Reference, When to Use Each Tool, Best Practices
  - **User Questions**: "What can you tell me about Skill Tool Usage Guide?", "How does Tool Workflow work?", "How does Security Review Results work?"

- **File**: `docs/styling/styling_integration.md`
  - **Content**: Styling Integration: anstyle-crossterm
  - **Topics**: Overview, Architecture, Components, Usage Examples, Benefits
  - **User Questions**: "What can you tell me about Styling Integration: anstyle-crossterm?", "How does Overview work?", "How does Architecture work?"

- **File**: `docs/styling/STYLING_QUICK_START.md`
  - **Content**: Styling Quick Start Guide
  - **Topics**: For CLI Output, For TUI Widgets, Unified Theme, Color Reference, Common Patterns
  - **User Questions**: "What can you tell me about Styling Quick Start Guide?", "How does For CLI Output work?", "How does For TUI Widgets work?"

- **File**: `docs/project/TODO.md`
  - **Content**: TODO.md
  - **User Questions**: "What can you tell me about TODO.md?"

- **File**: `docs/harness/TECH_DEBT_TRACKER.md`
  - **Content**: Tech Debt Tracker
  - **Topics**: Priority Levels, Status Values, Debt Items, How to Add a New Item, How to Resolve an Item
  - **User Questions**: "What can you tell me about Tech Debt Tracker?", "How does Priority Levels work?", "How does Status Values work?"

- **File**: `docs/huggingface/vtcode.md`
  - **Content**: VT Code
  - **Topics**: Overview, Configuration, Supported Models, Features with HF Integration, Common Use Cases
  - **User Questions**: "What can you tell me about VT Code?", "How does Overview work?", "How does Configuration work?"

- **File**: `docs/skills-enhanced/enhanced-summary.md`
  - **Content**: VT Code Agent Skills - Enhanced Implementation
  - **Topics**: Yes, I Can Do Better - Here's The Proof, Executive Summary, What Was Missing Before, Measurable Improvements, Key Differentiators
  - **User Questions**: "What can you tell me about VT Code Agent Skills - Enhanced Implementation?", "How does Yes, I Can Do Better - Here's The Proof work?", "How does Executive Summary work?"

- **File**: `docs/skills-enhanced/implementation-summary.md`
  - **Content**: VT Code Agent Skills - Enhanced Implementation
  - **Topics**: Yes, I Can Do Better - Here's The Proof, Executive Summary, What Was Missing Before, Measurable Improvements, Key Differentiators
  - **User Questions**: "What can you tell me about VT Code Agent Skills - Enhanced Implementation?", "How does Yes, I Can Do Better - Here's The Proof work?", "How does Executive Summary work?"

- **File**: `docs/skills-enhanced/improvements-proven.md`
  - **Content**: VT Code Agent Skills - Measurable Improvements
  - **Topics**: "I Can Do Better" - Here's The Proof, Side-by-Side Comparison, Extract Data, Workflow, Measurable Impact
  - **User Questions**: "What can you tell me about VT Code Agent Skills - Measurable Improvements?", "How does \"I Can Do Better\" - Here's The Proof work?", "How does Side-by-Side Comparison work?"

- **File**: `docs/environment/ALLOWED_COMMANDS_REFERENCE.md`
  - **Content**: VT Code Allowed Commands Reference
  - **Topics**: Overview, Command Categories, Blocked Commands (Dangerous Operations), Environment Variables Preserved, Configuration
  - **User Questions**: "What can you tell me about VT Code Allowed Commands Reference?", "How does Overview work?", "How does Command Categories work?"

- **File**: `docs/skills-enhanced/implementation.md`
  - **Content**: VT Code Claude API Skills Implementation Summary
  - **Topics**: Executive Summary, Key Findings from Claude API Guide, Enhanced Implementation Strategy, Implementation in VT Code, Benefits of Enhanced Implementation
  - **User Questions**: "What can you tell me about VT Code Claude API Skills Implementation Summary?", "How does Executive Summary work?", "How does Key Findings from Claude API Guide work?"

- **File**: `docs/skills-enhanced/claude-api-integration.md`
  - **Content**: VT Code Claude API Skills Integration Guide
  - **Topics**: Overview, Key Differences from Claude API, Enhanced Implementation Pattern, Enhanced VT Code Usage Pattern, Key Improvements from Claude Guide
  - **User Questions**: "What can you tell me about VT Code Claude API Skills Integration Guide?", "How does Overview work?", "How does Key Differences from Claude API work?"

- **File**: `docs/skills-enhanced/api-integration.md`
  - **Content**: VT Code Claude API Skills Integration Guide
  - **Topics**: Overview, Key Differences from Claude API, Enhanced Implementation Pattern, Enhanced VT Code Usage Pattern, Key Improvements from Claude Guide
  - **User Questions**: "What can you tell me about VT Code Claude API Skills Integration Guide?", "How does Overview work?", "How does Key Differences from Claude API work?"

- **File**: `docs/environment/ENVIRONMENT_SETUP_GUIDE.md`
  - **Content**: VT Code Environment Setup and PATH Visibility Guide
  - **Topics**: Overview, How VT Code Manages Environment Variables, Command Execution Paths, Verifying Your Environment Setup, Configuring Allowed Commands
  - **User Questions**: "What can you tell me about VT Code Environment Setup and PATH Visibility Guide?", "How does Overview work?", "How does How VT Code Manages Environment Variables work?"

- **File**: `docs/styling/RATATUI_FAQ_INTEGRATION.md`
  - **Content**: VT Code Integration of Ratatui FAQ Best Practices
  - **Topics**: Overview, FAQ Topics Applied, New Documentation, Code Comments, Testing Improvements
  - **User Questions**: "What can you tell me about VT Code Integration of Ratatui FAQ Best Practices?", "How does Overview work?", "How does FAQ Topics Applied work?"

- **File**: `docs/project/project_analysis.md`
  - **Content**: VT Code Project Analysis
  - **Topics**: Executive Summary, Architecture Overview, Codebase Health Snapshot, Maintainability Hotspots, Targeted Improvements
  - **User Questions**: "What can you tell me about VT Code Project Analysis?", "How does Executive Summary work?", "How does Architecture Overview work?"

- **File**: `docs/sandbox/SANDBOX_DEEP_DIVE.md`
  - **Content**: VT Code Sandbox Deep Dive
  - **Topics**: Design Philosophy, Sandbox Policies, Platform-Specific Implementations, Security Features, Debug Tooling
  - **User Questions**: "What can you tell me about VT Code Sandbox Deep Dive?", "How does Design Philosophy work?", "How does Sandbox Policies work?"

- **File**: `docs/sandbox/SANDBOX_FIELD_GUIDE.md`
  - **Content**: VT Code Sandbox Field Guide
  - **Topics**: The Three-Question Model, Boundaries, Policy, Lifecycle, Platform-Specific Implementation
  - **User Questions**: "What can you tell me about VT Code Sandbox Field Guide?", "How does The Three-Question Model work?", "How does Boundaries work?"

- **File**: `docs/skills-enhanced/improvement-guide.md`
  - **Content**: VT Code Skills Improvement Implementation Guide
  - **Topics**: Common Patterns, Dependencies, Advanced Patterns, Best Practices, Troubleshooting
  - **User Questions**: "What can you tell me about VT Code Skills Improvement Implementation Guide?", "How does Common Patterns work?", "How does Dependencies work?"

- **File**: `docs/skills-enhanced/best-practices.md`
  - **Content**: VT Code Skills Usage Best Practices
  - **Topics**: Core Principle: Skills are Instructions, Not Execution, Correct Skill Usage Pattern, Enhanced PDF Generation Example, Key Improvements from Claude Skills Guide, Common Pitfalls to Avoid
  - **User Questions**: "What can you tell me about VT Code Skills Usage Best Practices?", "How does Core Principle: Skills are Instructions, Not Execution work?", "How does Correct Skill Usage Pattern work?"

- **File**: `docs/styling/INDEX.md`
  - **Content**: VT Code Styling System - Complete Documentation Index
  - **Topics**: Quick Navigation, Implementation Status, Document Guide, Quick Reference, Key Crates
  - **User Questions**: "What can you tell me about VT Code Styling System - Complete Documentation Index?", "How does Quick Navigation work?", "How does Implementation Status work?"

- **File**: `docs/skills-enhanced/claude-analysis.md`
  - **Content**: VT Code vs Claude API Skills Authoring Best Practices Analysis
  - **Topics**: Executive Summary, Architecture Alignment Analysis, Overview, Capabilities, Usage Examples
  - **User Questions**: "What can you tell me about VT Code vs Claude API Skills Authoring Best Practices Analysis?", "How does Executive Summary work?", "How does Architecture Alignment Analysis work?"

- **File**: `docs/styling/README.md`
  - **Content**: Vtcode Styling System Documentation
  - **Topics**: Files, Quick Summary, Architecture Overview, Dependencies, Related Code Locations
  - **User Questions**: "What can you tell me about Vtcode Styling System Documentation?", "How does Files work?", "How does Quick Summary work?"

- **File**: `docs/protocols/XDG_DIRECTORY_SPECIFICATION.md`
  - **Content**: XDG Base Directory Specification Implementation
  - **Topics**: Overview, Directory Structure, Migration Guide, Environment Variables, Implementation Details
  - **User Questions**: "What can you tell me about XDG Base Directory Specification Implementation?", "How does Overview work?", "How does Directory Structure work?"

- **File**: `docs/protocols/ZED_EXTENSION_FILE_SEARCH.md`
  - **Content**: Zed Extension File Search Integration
  - **Topics**: Overview, New Commands, Architecture, File Structure, API Reference
  - **User Questions**: "What can you tell me about Zed Extension File Search Integration?", "How does Overview work?", "How does New Commands work?"

- **File**: `docs/ansi/ANSTYLE_CROSSTERM_IMPROVEMENTS.md`
  - **Content**: anstyle-crossterm Integration Improvements
  - **Topics**: Overview, Key Improvements, Color Mapping Behavior, Architecture Flow, Usage Patterns
  - **User Questions**: "What can you tell me about anstyle-crossterm Integration Improvements?", "How does Overview work?", "How does Key Improvements work?"

- **File**: `docs/ansi/ANSTYLE_PARSE_INTEGRATION.md`
  - **Content**: anstyle-parse Integration Guide
  - **Topics**: Step 1: Add Dependency, Step 2: Create Parser Wrapper Module, Step 3: Update Module Exports, Step 4: Replace Manual Parser in PTY, Step 5: Update ANSI Stripping
  - **User Questions**: "What can you tell me about anstyle-parse Integration Guide?", "How does Step 1: Add Dependency work?", "How does Step 2: Create Parser Wrapper Module work?"

- **File**: `docs/project/ROADMAP.md`
  - **Content**: vtcode Development Roadmap
  - **Topics**: **Recently Completed - Major Breakthroughs**, **High Priority - SWE-bench Performance Optimization**, Medium Priority, Low Priority, Implementation Notes
  - **User Questions**: "What can you tell me about vtcode Development Roadmap?", "How does **Recently Completed - Major Breakthroughs** work?", "How does **High Priority - SWE-bench Performance Optimization** work?"

### Performance & Optimization

- **File**: `docs/benchmarks/CHART_GUIDE.md`
  - **Content**: Benchmark Chart Quick Reference
  - **Topics**: Current Chart, Chart Breakdown, Key Insights, Generating Your Own Charts, Comparing Models
  - **User Questions**: "What can you tell me about Benchmark Chart Quick Reference?", "How does Current Chart work?", "How does Chart Breakdown work?"

- **File**: `docs/benchmarks/COMPARISON.md`
  - **Content**: Benchmark Comparison
  - **Topics**: Current Results, Planned Comparisons, Expected Performance Ranges, How to Add New Results, Analysis Framework
  - **User Questions**: "What can you tell me about Benchmark Comparison?", "How does Current Results work?", "How does Planned Comparisons work?"

- **File**: `docs/benchmarks/SUMMARY.md`
  - **Content**: Benchmark Summary
  - **Topics**: Quick Reference, Latest Results, How to Run, Visualization, Files
  - **User Questions**: "What can you tell me about Benchmark Summary?", "How does Quick Reference work?", "How does Latest Results work?"

- **File**: `docs/benchmarks/VISUALIZATION.md`
  - **Content**: Benchmark Visualization Guide
  - **Topics**: Chart Components, Generating Charts, Chart Interpretation, Example Charts, Comparing Multiple Models
  - **User Questions**: "What can you tell me about Benchmark Visualization Guide?", "How does Chart Components work?", "How does Generating Charts work?"

- **File**: `docs/benchmarks/HUMANEVAL_2025-10-22.md`
  - **Content**: HumanEval Benchmark Results - October 22, 2025
  - **Topics**: Executive Summary, Configuration, Results, Methodology, Analysis
  - **User Questions**: "What can you tell me about HumanEval Benchmark Results - October 22, 2025?", "How does Executive Summary work?", "How does Configuration work?"

- **File**: `docs/benchmarks/README.md`
  - **Content**: VT Code Benchmarks
  - **Topics**: Overview, HumanEval Benchmark, Contributing, References
  - **User Questions**: "What can you tell me about VT Code Benchmarks?", "How does Overview work?", "How does HumanEval Benchmark work?"

- **File**: `docs/benchmarks/performance_benchmarks.md`
  - **Content**: VT Code Performance Benchmarks
  - **Topics**: Overview, Benchmark Methodology, Performance Results, Memory Profile, Clone Operation Audit
  - **User Questions**: "What can you tell me about VT Code Performance Benchmarks?", "How does Overview work?", "How does Benchmark Methodology work?"

- **File**: `docs/benchmarks/BENCHMARK_COMPARISON.md`
  - **Content**: Which Benchmark Should You Use?
  - **Topics**: Quick Answer, Detailed Comparison, HumanEval, MBPP (Mostly Basic Python Problems), SWE-bench (Software Engineering Benchmark)
  - **User Questions**: "What can you tell me about Which Benchmark Should You Use??", "How does Quick Answer work?", "How does Detailed Comparison work?"

### Security & Safety

- **File**: `docs/security/SECURITY_DOCUMENTATION_INDEX.md`
  - **Content**: Security Documentation Index
  - **Topics**: Core Documentation, Recent Security Work, Security Features by Layer, Testing & Verification, Configuration
  - **User Questions**: "What can you tell me about Security Documentation Index?", "How does Core Documentation work?", "How does Recent Security Work work?"

- **File**: `docs/security/SECURITY_MODEL.md`
  - **Content**: VT Code Security Model
  - **Topics**: Overview, Security Architecture Diagram, Security Layers, Threat Model, Attack Scenarios
  - **User Questions**: "What can you tell me about VT Code Security Model?", "How does Overview work?", "How does Security Architecture Diagram work?"

- **File**: `docs/security/SECURITY_WEB_FETCH.md`
  - **Content**: Web Fetch Security & Malicious URL Prevention
  - **Topics**: Overview, Security Layers, Error Messages, Implementation Details, Testing
  - **User Questions**: "What can you tell me about Web Fetch Security & Malicious URL Prevention?", "How does Overview work?", "How does Security Layers work?"

### Tools & Functionality

- **File**: `docs/tools/JUSTIFICATION_SYSTEM.md`
  - **Content**: Agent Justification System
  - **Topics**: Overview, Architecture, Data Flow, Data Persistence, Integration Points
  - **User Questions**: "What can you tell me about Agent Justification System?", "How does Overview work?", "How does Architecture work?"

- **File**: `docs/tools/TOOL_SEARCH.md`
  - **Content**: Anthropic Tool Search Integration
  - **Topics**: Overview, Configuration, Tool Search Algorithms, API Usage, Response Block Types
  - **User Questions**: "What can you tell me about Anthropic Tool Search Integration?", "How does Overview work?", "How does Configuration work?"

- **File**: `docs/tools/EDITOR_CONFIG.md`
  - **Content**: External Editor Configuration
  - **Topics**: Overview, Configuration, Editor Detection Order, Usage Examples, Setting Your Preferred Editor
  - **User Questions**: "What can you tell me about External Editor Configuration?", "How does Overview work?", "How does Configuration work?"

- **File**: `docs/tools/GIT_COMMAND_EXECUTION.md`
  - **Content**: Git Command Execution Policy
  - **Topics**: Overview, Supported Operations, Usage Examples, Flags and Parameters, Security Model
  - **User Questions**: "What can you tell me about Git Command Execution Policy?", "How does Overview work?", "How does Supported Operations work?"

- **File**: `docs/tools/GIT_QUICK_REFERENCE.md`
  - **Content**: Git Commands - Quick Reference
  - **Topics**: Allowed Operations, Blocked Operations, Common Workflows, Error Messages, Notes
  - **User Questions**: "What can you tell me about Git Commands - Quick Reference?", "How does Allowed Operations work?", "How does Blocked Operations work?"

- **File**: `docs/tools/max_tokens_support.md`
  - **Content**: Per-Call Max Tokens Support
  - **Topics**: Overview, Supported Tools, Usage Examples, Token Budget Hierarchy, Implementation Details
  - **User Questions**: "What can you tell me about Per-Call Max Tokens Support?", "How does Overview work?", "How does Supported Tools work?"

- **File**: `docs/tools/PROMPT_CACHING_GUIDE.md`
  - **Content**: Prompt Caching Guide
  - **Topics**: Global Settings, Provider Overrides, Usage Telemetry, Validation & Testing, Implementation Architecture
  - **User Questions**: "What can you tell me about Prompt Caching Guide?", "How does Global Settings work?", "How does Provider Overrides work?"

- **File**: `docs/tools/TOOL_SPECS.md`
  - **Content**: VT Code Tool Specifications (Anthropic-Aligned)
  - **Topics**: Common Conventions, Tools, Policy Constraints (scoped), Error Style, Evaluation Tips
  - **User Questions**: "What can you tell me about VT Code Tool Specifications (Anthropic-Aligned)?", "How does Common Conventions work?", "How does Tools work?"

- **File**: `docs/tools/web_fetch_security.md`
  - **Content**: Web Fetch Tool Security Configuration
  - **Topics**: Security Modes, Dynamic Configuration Files, Inline Configuration, HTTPS Enforcement, Security Best Practices
  - **User Questions**: "What can you tell me about Web Fetch Tool Security Configuration?", "How does Security Modes work?", "How does Dynamic Configuration Files work?"

### User Workflows & Commands

- **File**: `docs/user-guide/commands.md`
  - **Content**: Command Reference
  - **Topics**: grep_file (ripgrep-like), File operations, Agent teams, Quick Actions in Chat Input, stats (session metrics)
  - **User Questions**: "What can you tell me about Command Reference?", "How does grep_file (ripgrep-like) work?", "How does File operations work?"

- **File**: `docs/user-guide/interactive-mode.md`
  - **Content**: Interactive Mode Reference
  - **Topics**: Keyboard Shortcuts, Plan Mode Notes, Vim Editor Mode, Command History, Background Bash Commands
  - **User Questions**: "What can you tell me about Interactive Mode Reference?", "How does Keyboard Shortcuts work?", "How does Plan Mode Notes work?"

## Enhanced Trigger Questions

### Core Capabilities & Features
- "What can VT Code do?"
- "What are VT Code's main features?"
- "How does VT Code compare to other AI coding tools?"
- "What makes VT Code unique?"
- "Can VT Code handle multiple programming languages?"
- "Does VT Code support real-time collaboration?"
- "How does VT Code handle large codebases efficiently?"
- "What are the different system prompt modes (minimal, lightweight, etc.)?"
- "How can I reduce token usage with tool documentation modes?"

### Workflows & Agent Behavior
- "What is Plan Mode and how do I use it?"
- "How do I use the @ symbol to reference files in my messages?"
- "What are agent teams and how do they work?"
- "How can I delegate tasks to specialized subagents like the code-reviewer?"
- "How do I use the /files slash command to browse my workspace?"
- "What is the Decision Ledger and how does it help with coherence?"
- "How does the agent handle long-running conversations?"

### Security & Reliability
- "What security layers does VT Code implement?"
- "How does VT Code ensure shell command safety?"
- "What is the 5-layer security model in VT Code?"
- "How do tool policies and human-in-the-loop approvals work?"
- "How does the circuit breaker prevent cascading failures?"
- "Is my code and data safe with VT Code?"

### Integrations & Protocols
- "How do I use VT Code inside the Zed editor?"
- "What is the Agent Client Protocol (ACP) and how is it used?"
- "What is the Agent2Agent (A2A) protocol?"
- "How does VT Code conform to the Open Responses specification?"
- "How do I configure Model Context Protocol (MCP) servers?"
- "What are lifecycle hooks and how do I configure them?"

### Local Models & Providers
- "Can I use VT Code with local models via Ollama?"
- "How do I integrate VT Code with LM Studio?"
- "Which AI providers are supported (OpenAI, Anthropic, Gemini, etc.)?"
- "How do I set up OpenRouter with VT Code?"
- "How can I use Hugging Face Inference Providers?"

### Getting Started & Setup
- "How do I install VT Code?"
- "How do I get started with VT Code?"
- "How do I set up VT Code for the first time?"
- "What do I need to get started?"
- "How do I configure API keys?"
- "Which LLM provider should I choose?"
- "How do I configure VT Code for my workflow?"
- "What are the most common keyboard shortcuts?"

### Development & Maintenance
- "How do I build VT Code from source?"
- "How do I run the test suite?"
- "How do I add a new tool to VT Code?"
- "How do I debug agent behavior or tool execution?"
- "How do I run the performance benchmarks?"
- "How do I update the self-documentation map?"
- "How do I contribute to the VT Code project?"
- "What is the release process for VT Code?"
- "How do I manage multi-crate dependencies in this workspace?"

## VT Code Feature Categories

### Core Capabilities
- **Multi-LLM Provider Support**: OpenAI, Anthropic, Google, DeepSeek, xAI, OpenRouter, Moonshot AI, Ollama, LM Studio
- **Terminal Interface**: Modern TUI with mouse support, text selection, and streaming output
- **Workspace Management**: Automatic project indexing, fuzzy file discovery, and context curation
- **Tool System**: Modular, extensible tool architecture with 53+ specialized tools
- **Security**: Enterprise-grade safety with tree-sitter-bash validation, sandboxing, and policy controls
- **Agent Protocols**: Support for ACP, A2A, and Open Responses for cross-tool interoperability

## Additional Resources

### External Documentation
- **Repository**: https://github.com/vinhnx/vtcode
- **Crate**: https://crates.io/crates/vtcode
- **VS Code Extension**: Open VSX and VS Code Marketplace

---

**Note**: This enhanced documentation map is designed for VT Code's self-documentation system. When users ask questions about VT Code itself, the system should fetch this document and use it to provide accurate, up-to-date information about VT Code's capabilities and features.
