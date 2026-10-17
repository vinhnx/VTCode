# Documentation Index

Repository-wide entrypoint for VT Code documentation.

Last reviewed: 2026-08-28

## Start Here

- [Community Wiki](https://github.com/vinhnx/VTCode/wiki) - Getting started, configuration, providers, local models,
  skills, MCP, automation, security, and FAQ.
- [Documentation Hub](README.md) - Main user/developer overview.
- [Harness Index](harness/INDEX.md) - Agent operating model, quality scoring, and debt tracking.
- [Zen Alignment](harness/ZEN_ALIGNMENT.md) - Full all-19 principle mapping and rollout.
- [Architecture](ARCHITECTURE.md) - System structure and crate boundaries.
- [Contributing](CONTRIBUTING.md) - Contribution workflow and standards.

## Core Domains

- [Configuration Precedence](config/CONFIGURATION_PRECEDENCE.md) - Runtime config loading order.
- [User Data Directories](guides/user-data-directories.md) - XDG/native paths, environment overrides, permissions, and
  legacy migration.
- [Config Field Reference](config/CONFIG_FIELD_REFERENCE.md) - Field-level schema reference.
- [Tool Specifications](tools/TOOL_SPECS.md) - Canonical tool surface reference.
- [AI Tool Surface Migration](development/ai-tool-surface-migration.md) - Breaking-change notes for the Codex-style
  default tools.
- [Web Search Tool](tools/web_search.md) - Web search configuration and guard rails.
- [Defuddle Fetch](tools/defuddle_fetch.md) - Markdown extraction fallback for complex pages.
- [Scheduled Tasks](user-guide/scheduled-tasks.md) - Reminder and durable scheduler flows.
- [WebMCP Browser Bridge User Guide](user-guide/webmcp.md) - Connect a browser editor to an active VT Code session or a
  bounded standalone workspace bridge.
- [Planning Workflow](guides/planning-workflow.md) - `/plan`, review gate, plan handoff to build/auto agents.
- [Permissions Guide](guides/permissions.md) - Granular agent permissions and rule grammar.
- [Decisions Probe Guide](guides/decisions-probe.md) - Experimental OpenAI Decisions tool-output probe usage.
- [Provider Guides](providers/PROVIDER_GUIDES.md) - LLM provider setup and behavior.
- [Security Model](security/SECURITY_MODEL.md) - Security architecture.
- [Process Hardening](development/PROCESS_HARDENING.md) - Runtime hardening controls.
- [MCP Integration Guide](mcp/MCP_INTEGRATION_GUIDE.md) - MCP integration onboarding.
- [Agent Plugins](guides/agent-plugins.md) - Portable skill + MCP packages, `vtcode plugins` CLI.
- [Testing Guide](development/testing.md) - Test strategy and commands.

## Engineering References

- [Development Setup](development/DEVELOPMENT_SETUP.md) - Canonical contributor setup and local quality loop.
- [C++ Core Guidelines Adoption](development/CPP_CORE_GUIDELINES_ADOPTION.md) - Rules for C/C++ code paths and
  cross-language safety intent.
- [Extension Boundaries](development/EXTENSION_BOUNDARIES.md) - When to use internal Rust traits vs external protocol or
  manifest seams.
- [Language Support Matrix](protocols/LANGUAGE_SUPPORT.md) - Tree-sitter and language support status.
- [Signal Handling](signal_handling.md) - Ctrl+C / SIGINT priority guarantees and emergency exit.
- [Indexer Notes](modules/vtcode_indexer.md) - Indexer behavior and usage.
- [Development Guide](development/README.md) - Local dev workflows.
- [Diff Preview Architecture](development/diff-preview.md) - Shared diff computation, bounded layout, and renderer
  boundaries.
- [HOVER Validation](development/hypothesis-revision-validation.md) - HOVER acceptance, deterministic checks, live
  replay pilot.
- [Roadmap](project/ROADMAP.md) - Planned work.
- [Loop Engineering](loop-engineering.md) - Worktree isolation, propose/verify sub-agents, loop state, cost guardrails.

## Module Documentation

- [vtcode-ui](modules/vtcode_ui.md) - UI framework, design system, theme registry.
- [vtcode-llm](modules/vtcode_llm.md) - LLM provider abstraction and streaming.
- [vtcode-skills](modules/vtcode_skills.md) - Skill discovery, loading, and validation.
- [vtcode-safety](modules/vtcode_safety.md) - Command safety, execution policies, sandboxing.
- [vtcode-a2a](modules/vtcode_a2a.md) - Agent2Agent protocol support.
- [vtcode-mcp](modules/vtcode_mcp.md) - Model Context Protocol integration.
- [vtcode-bash-runner](modules/vtcode_bash_runner.md) - Shell execution sandbox.
- [vtcode-config](modules/vtcode_config_migration.md) - Configuration loading and schema.
- [vtcode-commons](modules/vtcode_commons_reference.md) - Shared utilities.
- [vtcode-exec-events](modules/vtcode_exec_events.md) - ThreadEvent contract and ATIF export.
- [vtcode-indexer](modules/vtcode_indexer.md) - Code indexing and search.
- [vtcode-agent-plugins](modules/vtcode_agent_plugins.md) - Agent Plugins manifest parsing, validation, and discovery.

## Historical and Archive Paths

- `docs/async/` - Retired async reference (see `docs/guides/async-architecture.md`).

When adding implementation summaries or one-off reports, prefer a domain folder or an archive path instead of placing
files at `docs/*.md`.
