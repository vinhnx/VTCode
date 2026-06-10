# VT Code Project Info

## What is VT Code?

VT Code is an open-source coding agent with LLM-native code understanding and robust shell safety. It supports multiple LLM providers with automatic failover and efficient context management.

## Architecture

- Cargo workspace with multiple crates
- `vtcode-core/` for reusable agent/runtime logic
- `src/` for the CLI executable
- `vtcode-ui/` for the terminal UI, design, and theme
- `docs/` for architecture guides and developer docs
