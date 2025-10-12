# VTCode Component Extraction Plan

## Purpose
This document captures the results of a quick architectural survey of VTCode with the goal of identifying subsystems that could be extracted into standalone crates for open-source reuse. The focus is on components that already expose clear boundaries, have minimal coupling to the TUI, and would benefit the wider Rust agent ecosystem.

## Evaluation Approach
- Reviewed `vtcode-core` modules to map existing responsibilities and public APIs.
- Prioritized subsystems that already encapsulate well-defined responsibilities and expose reusable traits or data structures.
- Noted required decoupling work (configuration, logging, storage paths) needed before a clean extraction.

## Candidate Crates Overview
| Candidate crate | Source modules | Core capability | Key dependencies | Reuse potential |
| --- | --- | --- | --- | --- |
| `vtcode-llm` | `vtcode-core/src/llm` | Unified async client abstraction over Gemini, OpenAI, Anthropic, xAI, DeepSeek, and Z.AI providers (streaming, function calling, retries). | `anyhow`, `futures`, provider SDKs, config loader. | High – provides a ready-made multi-provider facade with streaming and function-call support.【F:vtcode-core/src/llm/mod.rs†L1-L160】|
| `vtcode-tools` | `vtcode-core/src/tools` | Registry-driven tool execution framework with safety policies, PTY integration, AST/grep search utilities. | `async_trait`, `serde_json`, `tokio`, tree-sitter crates. | High – modular tool runtime could power other agents or CLI automation surfaces.【F:vtcode-core/src/tools/mod.rs†L1-L160】|
| `vtcode-config` | `vtcode-core/src/config/loader/mod.rs` plus `config` submodules | Typed loader for TOML configuration with defaults covering agent, tools, security, UI, MCP/ACP, telemetry, syntax highlighting. | `serde`, `toml`, `anyhow`. | Medium – valuable for other terminal agents; requires separating VTCode-specific defaults and paths.【F:vtcode-core/src/config/loader/mod.rs†L1-L200】|
| `vtcode-markdown-store` | `vtcode-core/src/markdown_storage.rs`, `project.rs` | Markdown-backed storage, project management, simple cache/kv utilities. | `serde_json`, `serde_yaml`, `indexmap`. | Medium – lightweight alternative to database-backed state useful for offline tooling.【F:vtcode-core/src/markdown_storage.rs†L1-L200】【F:vtcode-core/src/project.rs†L1-L200】|
| `vtcode-indexer` | `vtcode-core/src/simple_indexer.rs` | Regex-powered file indexer with on-disk markdown snapshots and search helpers. | `regex`, filesystem APIs. | Medium – simple workspace index ideal for scripting or other agents.【F:vtcode-core/src/simple_indexer.rs†L1-L200】|
| `vtcode-bash-runner` | `vtcode-core/src/bash_runner.rs` | Safe wrapper around common shell commands (cd/ls/mkdir/rm/cp/mv/grep) with contextual error handling. | `std::process`, `anyhow`. | Low/Medium – helpful for sandboxed automation or testing harnesses.【F:vtcode-core/src/bash_runner.rs†L1-L200】|
| `vtcode-exec-events` | `vtcode-core/src/exec/events.rs` | Structured event schema for autonomous execution telemetry (thread lifecycle, command/file updates, error reporting). | `serde`. | Medium – reusable telemetry schema for orchestrating multi-step agent runs.【F:vtcode-core/src/exec/events.rs†L1-L200】|

## Detailed Extraction Notes

### `vtcode-llm`
**What it offers:**
- Common `AnyClient` interface with helper constructors for every supported provider, including streaming and function calling flows.【F:vtcode-core/src/llm/mod.rs†L1-L160】
- Consistent error taxonomy for auth, rate limits, provider, and network failures.

**Decoupling tasks:**
- Extract provider-specific clients behind feature flags to keep the crate lightweight.
- Replace direct references to `vtcode_core::utils::dot_config::ProviderConfigs` with a provider-agnostic config trait.
- Document environment variable expectations and provide mock implementations for testing.

### `vtcode-tools`
**What it offers:**
- Registry pattern (`ToolRegistry`, `ToolRegistration`) with async execution and serde-based parameter schemas.【F:vtcode-core/src/tools/mod.rs†L18-L159】
- Rich catalogue of built-in tools (bash, AST-grep, srgn, curl, planners) bundled behind module boundaries.
- Safety policies: workspace path validation, command allow/deny lists, execution logging.【F:vtcode-core/src/tools/mod.rs†L42-L85】

**Decoupling tasks:**
- Isolate tree-sitter parsers and heavy dependencies into optional features.
- Move VTCode-specific policy wiring (config structs, telemetry hooks) into adapters so the crate exports clean traits.
- Provide integration examples demonstrating registry setup and tool execution from a headless context.

### `vtcode-config`
**What it offers:**
- A single `VTCodeConfig` struct with defaults covering agent tuning, tool permissions, PTY behavior, telemetry, syntax highlighting, automation, MCP/ACP, and caching.【F:vtcode-core/src/config/loader/mod.rs†L88-L167】
- Bootstrap helpers that generate config files and `.gitignore` entries for new workspaces.【F:vtcode-core/src/config/loader/mod.rs†L169-L200】

**Decoupling tasks:**
- Split the monolithic config into layered modules (`agent`, `tools`, `telemetry`, etc.) so downstream projects can pick subsets.
- Allow callers to inject their own default paths and theme lists instead of writing into `.vtcode` directories.
- Publish serde schemas and conversion utilities for forward compatibility.

### `vtcode-markdown-store`
**What it offers:**
- `MarkdownStorage` abstraction that serializes structs into Markdown with JSON/YAML blocks and reloads them via serde.【F:vtcode-core/src/markdown_storage.rs†L13-L155】
- Higher-level utilities: `SimpleKVStorage`, `ProjectStorage`, and `ProjectData` for human-readable project metadata.【F:vtcode-core/src/markdown_storage.rs†L158-L200】
- `SimpleProjectManager` wrapping storage with helper methods to create/list/update projects and locate per-project directories.【F:vtcode-core/src/project.rs†L10-L140】
- `SimpleCache` for filesystem-backed caching with contextual errors.【F:vtcode-core/src/project.rs†L142-L200】

**Decoupling tasks:**
- Extract `.vtcode` directory assumptions; allow callers to pass storage paths or use temp dirs.
- Add file locking or atomic writes if concurrent agents will reuse the crate.
- Expose serde feature gates for consumers who only need KV or project metadata pieces.

### `vtcode-indexer`
**What it offers:**
- Pure-Rust file indexer with metadata hashing, language detection via extension, and regex-powered search/find helpers.【F:vtcode-core/src/simple_indexer.rs†L16-L200】
- Stores snapshots as Markdown alongside an in-memory cache, making results auditable and git-friendly.

**Decoupling tasks:**
- Replace Markdown persistence with pluggable storage (trait) so consumers can target SQLite, S3, etc.
- Add filtering hooks (ignore globs, binary detection) before walking directories.
- Publish CLI examples showing indexing and search flows to demonstrate reuse outside VTCode.

### `vtcode-bash-runner`
**What it offers:**
- Friendly wrapper for common shell actions (cd/ls/pwd/mkdir/rm/cp/mv/grep) with canonicalized paths and rich error messages.【F:vtcode-core/src/bash_runner.rs†L11-L200】

**Decoupling tasks:**
- Harden for cross-platform support (Windows fallback to `cmd` / PowerShell or rely on busybox).
- Inject command execution strategy (pure Rust alternatives) for sandboxed environments.
- Provide dry-run mode to log commands without executing them.

### `vtcode-exec-events`
**What it offers:**
- Serde-tagged enums describing thread lifecycle, command execution, file change summaries, and token usage metrics.【F:vtcode-core/src/exec/events.rs†L3-L123】

**Decoupling tasks:**
- Document event semantics and versioning policy so downstream consumers can rely on backward compatibility.
- Offer feature flags for optional payloads (e.g., exclude file diffs to reduce payload size).

## Cross-Cutting Recommendations
- Establish a shared `vtcode-commons` crate for base error types, path utilities, and configuration traits referenced by multiple extracted crates.
- Adopt semantic versioning per crate and generate docs via `cargo doc` before publishing to crates.io.
- Ensure all extracted crates include focused integration tests and, where applicable, minimal examples under `examples/` demonstrating standalone use.

## Next Steps
1. Prototype extraction of `vtcode-llm` and `vtcode-tools`, since they offer the highest immediate reuse value.
2. Define a migration checklist (tests, documentation, CI) for each crate to keep the releases consistent.
3. Evaluate licensing compatibility of bundled dependencies (tree-sitter grammars, provider SDKs) before publishing.
4. Communicate roadmap in the main README and invite community feedback once the first crate lands on crates.io.
