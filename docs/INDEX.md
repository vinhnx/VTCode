# Documentation Index

Repository-wide entrypoint for VT Code documentation.

Last reviewed: 2026-02-16

## Start Here

- [Documentation Hub](README.md) - Main user/developer overview.
- [Harness Index](harness/INDEX.md) - Agent operating model, quality scoring, and debt tracking.
- [Architecture](ARCHITECTURE.md) - System structure and crate boundaries.
- [Contributing](CONTRIBUTING.md) - Contribution workflow and standards.

## Core Domains

- [Configuration Precedence](config/CONFIGURATION_PRECEDENCE.md) - Runtime config loading order.
- [Config Field Reference](config/CONFIG_FIELD_REFERENCE.md) - Field-level schema reference.
- [Provider Guides](providers/PROVIDER_GUIDES.md) - LLM provider setup and behavior.
- [Security Model](security/SECURITY_MODEL.md) - Security architecture.
- [Process Hardening](development/PROCESS_HARDENING.md) - Runtime hardening controls.
- [MCP Start Here](mcp/00_START_HERE.md) - MCP integration onboarding.
- [Subagents Guide](subagents/SUBAGENTS.md) - Subagent types and configuration.
- [Testing Guide](development/testing.md) - Test strategy and commands.

## Engineering References

- [Language Support Matrix](protocols/LANGUAGE_SUPPORT.md) - Tree-sitter and language support status.
- [Indexer Notes](modules/vtcode_indexer.md) - Indexer behavior and usage.
- [Development Guide](development/README.md) - Local dev workflows.
- [Roadmap](project/ROADMAP.md) - Planned work.

## Historical and Archive Paths

- `docs/mcp/archive/` - Historical MCP implementation reports.
- `docs/async/` - Async migration implementation logs.
- `docs/vscode-extension-improve-docs/` - VS Code extension review artifacts.

When adding implementation summaries or one-off reports, prefer a domain folder or an archive path instead of placing files at `docs/*.md`.
