# VT Code Project Info

## What is VT Code?

VT Code is an open-source coding agent with LLM-native code understanding and robust shell safety. It supports multiple LLM providers with automatic failover and efficient context management.

## Architecture

- Cargo workspace with multiple crates
- `vtcode-core/` for reusable agent/runtime logic (agent loop, tools, prompts, orchestration)
- `vtcode-llm/` for LLM provider abstraction, client implementations, and streaming
- `vtcode-skills/` for skill types, discovery, loading, and validation
- `src/` for the CLI executable
- `vtcode-ui/` for the terminal UI, design, and theme
- `docs/` for architecture guides and developer docs
