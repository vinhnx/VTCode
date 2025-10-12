# Open Source Extraction Opportunities

## Overview
We reviewed the vtcode-core crate to locate self-contained subsystems whose logic could be reused by other Rust CLI or agent projects. The focus was on modules with:

- minimal coupling to Ratatui- or CLI-specific concerns;
- clear public structs or functions that already abstract side effects; and
- limited dependency footprints so they can become standalone crates or reusable modules.

## Candidate Components
| Component | What it does | Why it is reusable |
| --- | --- | --- |
| Markdown storage layer | Persists structured data (project metadata, key-value pairs) in Markdown with embedded JSON/YAML blocks. | General-purpose lightweight persistence that needs only `serde`, `serde_json`, `serde_yaml`, and `indexmap`, making it attractive for tooling that wants human-readable archives. 【F:vtcode-core/src/markdown_storage.rs†L1-L253】 |
| Simple indexer | Recursively scans a workspace, computes hashes, and exposes grep-like search utilities, storing summaries alongside results. | Provides a ready-to-use baseline code intelligence component without embedding dependencies, suitable for editors or bots that want local search. 【F:vtcode-core/src/simple_indexer.rs†L1-L338】 |
| Dot configuration manager | Manages a `$HOME/.vtcode` tree with config serialization, cache cleanup, backups, and disk-usage reporting. | A turnkey dot-folder management layer for any CLI needing persistent settings and caches, built around `toml` and filesystem utilities. 【F:vtcode-core/src/utils/dot_config.rs†L1-L551】 |
| Session archive subsystem | Captures structured chat transcripts with metadata, handles storage location resolution, and provides recent-session queries. | Useful to any conversational agent needing durable JSON snapshots and preview helpers, with only `serde`, `chrono`, and filesystem dependencies. 【F:vtcode-core/src/utils/session_archive.rs†L1-L400】 |
| Tool policy manager | Persists per-tool allow/prompt/deny decisions, integrates MCP allow lists, and applies sensible defaults for trusted utilities. | Offers a reusable consent-tracking framework for tool-enabled agents, mixing UX prompts with persistent policy storage. 【F:vtcode-core/src/tool_policy.rs†L1-L200】 |
| Provider-neutral LLM interface | Defines request/response structures and tool-choice normalization across OpenAI, Anthropic, Gemini, and others. | Captures cross-provider ergonomics (role mapping, tool selection) that other agent runtimes or SDKs could adopt directly. 【F:vtcode-core/src/llm/provider.rs†L1-L200】 |

## Extraction Notes
### Markdown Storage Layer
- **Scope:** `MarkdownStorage`, `SimpleKVStorage`, and `ProjectStorage` already have clean APIs; extraction mainly needs renaming and documentation. 【F:vtcode-core/src/markdown_storage.rs†L13-L253】
- **Dependencies:** `serde`, `serde_json`, `serde_yaml`, `indexmap`, `anyhow`.
- **Work items:** publish as `markdown-ledger` crate, expose optional features for KV or project helpers, add file locking (if concurrent use is expected), and document data layout for compatibility.

### Simple Indexer
- **Scope:** `SimpleIndexer` provides directory walking, hash computation, regex search, and Markdown export of index files. 【F:vtcode-core/src/simple_indexer.rs†L42-L338】
- **Dependencies:** `regex`, `anyhow`, standard library only.
- **Work items:** generalize ignore rules (currently hardcoded for `.`, `target`, `node_modules`), replace Markdown export with pluggable backend trait, and add CLI examples/tests for community adoption.

### Dot Configuration Manager
- **Scope:** `DotManager` orchestrates initialization, `toml` (de)serialization, cache cleanup, disk-usage stats, and backup utilities in one module. 【F:vtcode-core/src/utils/dot_config.rs†L160-L423】
- **Dependencies:** `toml`, `dirs`, `serde`, `thiserror`, `anyhow`-style error semantics.
- **Work items:** parameterize folder name (currently `.vtcode`), split provider-specific defaults behind optional features, and publish as a generic `dot-config-manager` crate with macros for declaring config schemas.

### Session Archive Subsystem
- **Scope:** `SessionArchive` and helpers serialize sessions to JSON files with deterministic filenames, plus preview utilities and listing helpers. 【F:vtcode-core/src/utils/session_archive.rs†L11-L400】
- **Dependencies:** `serde`, `serde_json`, `chrono`, `anyhow`.
- **Work items:** abstract dependency on `DotManager` so consumers can supply their own storage root, add streaming writer support for long sessions, and provide conversion adapters (e.g., Markdown summaries or HTML export).

### Tool Policy Manager
- **Scope:** Persists user decisions, enforces allow lists, and integrates CLI prompts with configurable defaults for safe automation. 【F:vtcode-core/src/tool_policy.rs†L27-L200】
- **Dependencies:** `dialoguer`, `indexmap`, `serde`, `anyhow`.
- **Work items:** separate terminal UI concerns from policy persistence, surface traits for custom prompt backends, and ship sample front-ends (CLI & GUI) under an `agent-tool-policy` crate.

### Provider-Neutral LLM Interface
- **Scope:** Unified request object, tool-choice abstraction, and provider-specific formatting helpers for OpenAI, Anthropic, Gemini, and generic backends. 【F:vtcode-core/src/llm/provider.rs†L57-L200】
- **Dependencies:** `serde`, `async-trait`, `async-stream`, `serde_json`.
- **Work items:** move provider-specific conversions behind feature flags, extract message/role definitions into a core crate, and document compatibility guarantees so other SDKs can build on it.

## Next Steps
1. Prioritize publishing the Markdown storage and DotManager pieces—they need minimal decoupling and would be immediately useful to CLI maintainers.
2. Draft RFC-style docs describing API stability expectations and migration plans before splitting crates, so downstream users understand future compatibility.
3. Establish CI templates (formatting, Clippy, tests) that each extracted crate can inherit from this repository to keep maintenance overhead low.
