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

#### Extraction considerations
- The structs already hide filesystem interactions, so the primary work is trimming vtcode-specific helper methods (e.g., project metadata conventions) into optional features.
- Storage previously assumed exclusive file access; advisory locking would still be useful for multi-process access, but the new atomic writer eliminates most partial-write risks.
- Markdown pages mix fenced JSON/YAML blocks with inline prose—documenting that layout is important so external consumers can extend parsers without guessing the schema.

#### Next steps
- Rename the crate to something neutral (e.g., `markdown-ledger`) and add cargo features (`kv`, `project`) that gate the higher-level convenience layers.
- Write migration notes explaining file naming, top-level headings, and how record IDs map to filenames for interoperability.
- Add examples covering initialization, read/modify/write cycles, and integration with async runtimes so adopters can evaluate ergonomics quickly.

#### Progress
- Added `MarkdownStorageOptions` so adopters can toggle JSON/YAML/raw sections or switch file extensions without reimplementing the storage layer, and persist records atomically to stabilize on-disk formats before extraction. 【F:vtcode-core/src/markdown_storage.rs†L15-L244】

### Simple Indexer
- **Scope:** `SimpleIndexer` provides directory walking, hash computation, regex search, and Markdown export of index files. 【F:vtcode-core/src/simple_indexer.rs†L42-L338】
- **Dependencies:** `regex`, `anyhow`, standard library only.

#### Extraction considerations
- Ignore rules are embedded in the implementation and reference vtcode's defaults; expose a configuration struct or trait so downstream tools can supply per-project filters.
- The Markdown exporter couples indexing and reporting—factor it into a trait (`IndexSink`) so alternate stores (SQLite, JSONL, in-memory) can be plugged in without forking.
- Hashing currently uses simple path-based deduplication; consider optional features for content fingerprinting or incremental updates before declaring a stable API.

#### Next steps
- Split the walker, search, and export routines into dedicated modules with unit tests to make the public surface easier to understand for crate users.
- Provide CLI snippets or `examples/` programs showing how to build a lightweight “search” command on top of the library API.
- Investigate adding a `send` + `sync` friendly iterator API so async consumers can stream matches to UI layers without blocking.

#### Progress
- Added `SimpleIndexerOptions` with configurable hidden/ignored directory handling and a pluggable `IndexSink` trait (with a default markdown implementation) so external tools can reuse the walker while supplying custom storage backends. 【F:vtcode-core/src/simple_indexer.rs†L70-L360】

### Dot Configuration Manager
- **Scope:** `DotManager` orchestrates initialization, `toml` (de)serialization, cache cleanup, disk-usage stats, and backup utilities in one module. 【F:vtcode-core/src/utils/dot_config.rs†L160-L423】
- **Dependencies:** `toml`, `dirs`, `serde`, `thiserror`, `anyhow`-style error semantics.

#### Extraction considerations
- The manager assumes the root directory is `.vtcode`; promote this to a configurable parameter and ship ergonomic builders for custom product names.
- Several helper methods reference vtcode provider defaults—those should become optional submodules gated behind features so the core crate stays neutral.
- Backup and cleanup routines rely on synchronous filesystem traversal; document expected directory structures and size estimation strategies for users with large caches.

#### Next steps
- Publish as `dot-config-manager` with derive/attribute macros that let consumers declare typed config files and upgrade hooks.
- Add high-level guides explaining recommended directory layout (`config/`, `cache/`, `sessions/`) and how to integrate with CI secrets management.
- Provide integration tests that exercise initialization, upgrade migrations, and corruption recovery so maintainers can trust the crate in automation contexts.
- Begin surfacing configurable constructors (e.g., `DotManager::with_product_name` and `DotManager::with_home_dir`) so host applications can control the root directory without forking. 【F:vtcode-core/src/utils/dot_config.rs†L172-L214】

### Session Archive Subsystem
- **Scope:** `SessionArchive` and helpers serialize sessions to JSON files with deterministic filenames, plus preview utilities and listing helpers. 【F:vtcode-core/src/utils/session_archive.rs†L11-L400】
- **Dependencies:** `serde`, `serde_json`, `chrono`, `anyhow`.

#### Extraction considerations
- The archive currently shells out to `DotManager` for locating directories; swap this for a trait so host applications can mount archives in custom paths or remote stores.
- Session filenames encode timestamps and slugs—stabilize that pattern (documented regex, version field) to preserve compatibility across crate releases.
- Preview helpers assume terminal-width constraints; decouple formatting so GUI/web consumers can render transcripts differently.

#### Next steps
- Add a streaming writer that appends events as they occur to reduce memory pressure for long conversations.
- Provide adapters that render Markdown, HTML, or summary JSON to widen reuse in dashboards and reports.
- Ship migration tools for consolidating legacy vtcode archives into the new layout so existing users can adopt the crate.

#### Progress
- Added the `SessionDirectoryResolver` abstraction plus default and fixed-directory implementations so host applications can supply custom archive roots while retaining the legacy DotManager-backed default. 【F:vtcode-core/src/utils/session_archive.rs†L17-L168】

### Tool Policy Manager
- **Scope:** Persists user decisions, enforces allow lists, and integrates CLI prompts with configurable defaults for safe automation. 【F:vtcode-core/src/tool_policy.rs†L27-L200】
- **Dependencies:** `dialoguer`, `indexmap`, `serde`, `anyhow`.

#### Extraction considerations
- Policy persistence is cleanly separated, but the prompt layer depends on `dialoguer`; introduce traits for confirmation prompts so the crate works in headless environments.
- MCP allow-list integration references vtcode constants; move the defaults into data files or feature-gated modules so adopters can inject their own lists.
- Consider thread-safe storage (RwLock/Arc) if agents want to mutate policies from concurrent tasks.

#### Next steps
- Publish as `agent-tool-policy` with examples showing CLI, web, and automated decision providers implementing the prompt trait.
- Add auditing hooks that emit events whenever a policy changes, enabling observability integrations for large deployments.
- Document recommended UX patterns (initial onboarding, transient approvals, time-based expirations) so consumers can replicate vtcode's guardrails.

#### Progress
- Introduced a `ToolPromptBackend` trait and default dialoguer implementation so callers can swap interactive prompts for automated or headless approvals while preserving policy persistence semantics, plus regression tests validating custom backends. 【F:vtcode-core/src/tool_policy.rs†L89-L205】【F:vtcode-core/src/tool_policy.rs†L598-L676】【F:vtcode-core/src/tool_policy.rs†L970-L1024】

### Provider-Neutral LLM Interface
- **Scope:** Unified request object, tool-choice abstraction, and provider-specific formatting helpers for OpenAI, Anthropic, Gemini, and generic backends. 【F:vtcode-core/src/llm/provider.rs†L57-L200】
- **Dependencies:** `serde`, `async-trait`, `async-stream`, `serde_json`.

#### Extraction considerations
- Provider adapters rely on crate-wide feature flags; repackage them into additive features so consumers pay only for the APIs they use.
- Tool-choice normalization ties into vtcode's tool registry; define a `ToolCall` trait with conversions so other runtimes can map to their internal representations.
- Streaming utilities assume tokio + async-stream—document supported runtimes and consider optional support for `futures`-only environments.

#### Next steps
- Split foundational message/role types into a `llm-interface-core` module with serde-compatible structs that providers can embed.
- Publish provider crates (`llm-openai-adapter`, `llm-anthropic-adapter`, etc.) that depend on the core module and manage API quirks behind feature flags.
- Author compatibility charts showing which provider features (tool calling, JSON mode, streaming) are supported, plus examples demonstrating cross-provider fallbacks.

#### Progress
- Extracted shared request, message, tool definition, and response types into a new `interface` module that exposes serde-friendly structs while keeping the provider trait focused on transport specifics, with re-exports to preserve existing call sites for future crate extraction. 【F:vtcode-core/src/llm/interface.rs†L1-L491】【F:vtcode-core/src/llm/provider.rs†L1-L118】

## Next Steps
1. Prioritize publishing the Markdown storage and DotManager pieces—they need minimal decoupling and would be immediately useful to CLI maintainers.
2. Draft RFC-style docs describing API stability expectations and migration plans before splitting crates, so downstream users understand future compatibility.
3. Establish CI templates (formatting, Clippy, tests) that each extracted crate can inherit from this repository to keep maintenance overhead low.
