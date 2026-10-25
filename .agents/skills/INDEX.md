# Skills Index

This file lists all available skills for dynamic discovery.
Use `read_file` on individual skill directories for full documentation.

## Available Skills (74 total)

### Traditional Skills

| Name | Description | Type |
|------|-------------|------|
| `adding-llm-providers` | Adds or updates VT Code LLM providers and model catalog entries. Use when adding a new LLM provider, adding models to an existing provider, or changing provider defaults/model picker support. | Skill |
| `adding-workspace-crate` | Adds a new crate to the VT Code workspace with all required metadata, documentation, release script integration, and crates.io compatibility. Use when creating a new workspace crate, extracting code into a new crate, or splitting an existing crate. | Skill |
| `agentic-testing` | Risk-first testing nudge for agentic coding. Use when writing or reviewing tests to avoid panic-only and identical-fixture traps. | Skill |
| `android-cli` | Orchestrates Android development tasks including project creation, deployment, SDK management, and environment diagnostics using the `android` command-line tool. | Skill |
| `ast-grep` | Use for ast-grep project setup, rule authoring, rule debugging, and structural search workflows. | Skill |
| `audit-module-agents` | Audit and update per-module AGENTS.md files when making significant changes to a crate. Use after adding new modules, changing conventions, discovering gotchas, or modifying public APIs. | Skill |
| `charon-rust-analysis` | Deep semantic analysis of Rust crates with Charon. Use when a question needs what rustc actually computes — real symbol resolution, drop glue and drop order, repr layout and its stability guarantees, monomorphization, trait selection, or unwind paths — beyond what the syntactic code_search/ast-grep surface can prove. | Skill |
| `cmd-analyze` | Perform comprehensive codebase analysis and generate reports (usage: /analyze [full\|security\|performance]) | Skill |
| `cmd-command` | Run a terminal command (usage: /command <program> [args...]) | Skill |
| `cmd-review` | Review the current diff or selected files (usage: /review [instructions \| --last-diff \| --target <expr> \| --file <path> \| files...] [--style <style>]) | Skill |
| `codemod` | Use Codemod CLI whenever the user wants to migrate, upgrade, update, or refactor a codebase in a repeatable way. This includes framework migrations, library upgrades, version bump migrations, API surface changes, deprecations, and large-scale mechanical edits. First search the Codemod Registry for an existing package, prefer deterministic codemods before open-ended AI rewrites, run dry-runs before apply, and create a codemod package only when no suitable package exists. | Skill |
| `evaluate-crate` | Evaluate whether a Rust crate, library, or external resource is useful for the vtcode project. Use when the user shares a crate URL, article, or reference and asks "can we use this to improve vtcode". | Skill |
| `generate-product-spec-patch` | Generate a numbered patch file (product-spec-patch-NNN.md) to surgically update a finalized product-spec.md when the backend contract, API field, enum value, error response, validation rule, or product requirement changes. Use this whenever the BE changes something that affects the spec, error messages change, a field type updates, an enum is renamed, or the spec falls out of sync with the implementation. Triggers on: "patch the spec", "BE changed X", "contract changed", "update product-spec", field type change, enum rename, response schema mismatch, or "spec is out of sync". | Skill |
| `jira-to-product-spec` | Generate product spec from Jira/Confluence PRD. Outputs 9-section spec: Metadata, Business Context, Functional Requirements (FR-CORE-001+), Acceptance Criteria (20+), User Flows (4+ with ASCII diagrams), [CONDITIONAL] API Contracts (grounded in backend-conventions.md), Platform Implementation (3 bullets per platform), Dependencies & Open Items (design conflicts + open questions sorted by criticality), and Appendix. Assumption-free: every claim is either verbatim from PRD, user-confirmed via HITL, or surfaced as a Q-XXX open question. Use when user mentions 'product spec', 'write spec', 'generate spec', 'specification', or provides Jira/Confluence URL. Includes 3 enforcement gates to prevent API hallucinations (verify conventions → validate checklist → pre-gen approval). | Skill |
| `mcp-apps` | Build MCP Apps (SEP-1865, `io.modelcontextprotocol/ui`) — MCP servers that declare `_meta.ui.resourceUri` on a tool and serve a `ui://` resource whose content is `text/html;profile=mcp-app`, rendered in a host iframe and driven over `postMessage` JSON-RPC. Use when creating, testing, or reasoning about an interactive MCP UI, or when a user asks for a visual/form/dashboard MCP tool. | Skill |
| `mimo-browser-use` | Prefer this skill for web search, opening URLs, reading articles, filling forms, and page UI. Controls IAB (default), Chrome, Edge, Brave, Chromium, or raw CDP via the shared js MCP for navigation, inspection, clicking, typing, tabs, downloads, dialogs, and screenshots. | Skill |
| `pr-code-review` | Spin up a PR code review sub-agent. Ranks bugs critical/high/medium/low, eliminates false positives, writes consolidated report, applies KISS/DRY fixes (main logic only, skip tests), re-reviews after every change, loops until done. Use when the user asks to review a PR, triage bugs, or run a code review fix loop. | Skill |
| `prd-diff-review` | Audit a product-spec.md against its original Jira/Confluence source to find hallucinations, intentional PIC additions, and unverified claims. Use after generating a PRD when you want to verify it against the source, surface what was added vs sourced, or identify invented details. Triggers on: 'review prd vs jira', 'audit the prd', 'check for hallucinations', 'diff spec against source', 'verify prd requirements', or /prd-diff-review. Then approve, remove, or modify flagged items, and optionally apply changes back to the spec. | Skill |
| `profiling-guide` | Apply Rust profiling-guide lessons to VT Code layer by layer (wall-clock, HTTP, I/O, locks, channels, allocs, CPU) using hotpath instrumentation, audits, and deterministic signals. Use when profiling, hunting bottlenecks, or wiring new hotpath telemetry. | Skill |
| `read-and-apply` | Read an external article, book chapter, or technical resource and apply its lessons to improve the vtcode codebase. Use when the user shares a URL or pastes article text and says 'read and apply' or 'improve vtcode based on this'. | Skill |
| `rust-skills` | 193 Rust coding rules across 14 categories. Use when writing, reviewing, or refactoring Rust code. Covers ownership, errors, async, API design, memory, performance, testing, and anti-patterns. | Skill |
| `skill-creator` | Guide for creating effective skills. This skill should be used when users want to create a new skill (or update an existing skill) that extends VT Code's capabilities with specialized knowledge, workflows, or tool integrations. | Skill |
| `skill-installer` | Install VT Code skills into ~/.agents/skills from a curated list or a GitHub repo path. Use when a user asks to list installable skills, install a curated skill, or install a skill from another repo, including private repos. | Skill |
| `source-command-fix-failing-tests` | Placeholder: the fix-failing-tests agent is not currently implemented in this workspace. | Skill |
| `verification-discipline` | Apply Creusot's deductive-verification discipline — explicit contracts, executable invariants, absence of overflow and panics, justified unsafe/trusted assumptions, termination, and views-as-models — to ordinary Rust without the Creusot/Why3 toolchain. Use when hardening Rust invariants and contracts, auditing overflow/panic/unsafe assumptions, or codifying verification-grade rigor (contract docs, debug_assert, checked/saturating arithmetic) in a codebase. | Skill |
| `vtcode-check` | Run the full quality gate for the vtcode project, automating execution of ./scripts/check.sh. | Skill |
| `vtcode-clippy` | Run clippy on the vtcode project to ensure code quality follows strict linting rules. | Skill |
| `vtcode-dev-helper` | Development helper for the vtcode project. Use to run the quality gate, workspace tests, clippy, and other development tasks following project standards. | Skill |
| `vtcode-hawk` | Run hawk analysis on the workspace to find dead public code and unnecessary visibility. | Skill |
| `vtcode-test` | Run tests for the vtcode project, prioritizing cargo nextest for speed. | Skill |

### Built-In Command Skills

| Name | Description | Slash Alias |
|------|-------------|-------------|
| `cmd-agent` | Manage subagents and delegated child threads (usage: /agent [list\|threads\|inspect <id>\|close <id>\|create [project\|user] [name]\|edit [name]\|delete <name>]) | `/agent` |
| `cmd-auth` | Show authentication status for providers (usage: /auth [provider]) | `/auth` |
| `cmd-clear` | Clear visible screen (usage: /clear [new]) | `/clear` |
| `cmd-compact` | Compact the current conversation immediately or manage the saved manual compaction prompt | `/compact` |
| `cmd-config` | Browse settings and run workspace panels: memory, permissions, model, ide, tasks, jobs, log, subprocess, notify, checkup, or reset the active layer (usage: /config [memory\|permissions\|model\|ide\|tasks\|jobs\|log\|subprocess\|notify\|checkup\|<path>\|reset]) | `/config` |
| `cmd-copy` | Copy the latest complete assistant reply to clipboard | `/copy` |
| `cmd-docs` | Open vtcode documentation in web browser | `/docs` |
| `cmd-donate` | Support the project by buying the author a coffee | `/donate` |
| `cmd-exit` | Exit the session | `/exit` |
| `cmd-explain` | Explain recorded execution without a model call | `/explain` |
| `cmd-fast` | Toggle native OpenAI ultrafast or Claude fast at premium pricing (usage: /fast [on\|off]) | `/fast` |
| `cmd-feedback` | Open a new GitHub issue to report a bug or request a feature | `/feedback` |
| `cmd-files` | Browse and select files from workspace (usage: /files [filter]) | `/files` |
| `cmd-fork` | Fork an archived session into a new thread (usage: /fork [limit] [--all]) | `/fork` |
| `cmd-help` | Show slash command help | `/help` |
| `cmd-history` | Open command history picker (usage: /history, same as Ctrl+R) | `/history` |
| `cmd-init` | Guided workspace setup for vtcode.toml, AGENTS.md, and indexing (usage: /init [--force]) | `/init` |
| `cmd-local` | Manage local inference servers: Ollama, LM Studio, llama.cpp (usage: /local [action] [provider]) | `/local` |
| `cmd-login` | Authenticate with OpenAI, OpenRouter, or GitHub Copilot (usage: /login [provider]) | `/login` |
| `cmd-logout` | Clear stored provider authentication (usage: /logout [provider]) | `/logout` |
| `cmd-mcp` | Open interactive MCP manager (usage: /mcp, optional subcommands still supported) | `/mcp` |
| `cmd-mode` | Switch the active agent mode (usage: /mode [agent-name]) | `/mode` |
| `cmd-model` | Launch the interactive model picker (includes reasoning effort) | `/model` |
| `cmd-new` | Start a new session | `/new` |
| `cmd-pause` | Pause the active turn at the next safe boundary | `/pause` |
| `cmd-plan` | Start or continue the planning workflow with an optional task prompt (usage: /plan [task]) | `/plan` |
| `cmd-plugin` | Open interactive Agent Plugins manager (usage: /plugin, /plugin manager) | `/plugin` |
| `cmd-redo` | Restore files and conversation from before the last rewind | `/redo` |
| `cmd-refresh-oauth` | Refresh stored provider credentials when supported (usage: /refresh-oauth [provider]) | `/refresh-oauth` |
| `cmd-resume` | List archived sessions when idle; resume the active turn while it is paused | `/resume` |
| `cmd-rewind` | Open the rewind picker or restore a specific checkpoint (usage: /rewind [turn] [conversation\|code\|both]) | `/rewind` |
| `cmd-secret` | Manage provider API keys in your OS keyring (usage: /secret [list\|status [provider] [key-name]\|add <provider> [key-name]\|delete <provider> [key-name]\|migrate [provider]\|help]) | `/secret` |
| `cmd-share` | Export the current session as JSON, Markdown, or self-contained HTML timeline (usage: /share [json\|markdown\|md\|html\|--format <fmt>]) | `/share` |
| `cmd-skills` | Open interactive skills manager (usage: /skills, /skills manager) | `/skills` |
| `cmd-status` | Show model, provider, workspace, and tool status | `/status` |
| `cmd-statusline` | Set up a custom status line with target selection (usage: /statusline [instructions...]) | `/statusline` |
| `cmd-stop` | Stop the active turn immediately | `/stop` |
| `cmd-terminal-setup` | Configure terminal for VT Code (multiline, copy/paste, shell, themes) | `/terminal-setup` |
| `cmd-theme` | Switch UI theme (usage: /theme <theme-id>) | `/theme` |
| `cmd-title` | Configure the terminal title items interactively | `/title` |
| `cmd-transcript` | Inspect or manage the bounded TUI transcript snapshot (usage: /transcript [stats\|clear\|export [path]]) | `/transcript` |
| `cmd-update` | Check for new VT Code releases and install updates (usage: /update [check\|install] [--force], or run `vtcode update` from the CLI) | `/update` |
| `cmd-vim` | Toggle Vim-style prompt editing for this session (usage: /vim [on\|off]) | `/vim` |
| `cmd-webmcp` | Manage the opt-in authenticated browser editor bridge (usage: /webmcp [help\|status\|tools\|roots\|pair [--replace] <origin>\|unpair]) | `/webmcp` |

## Quick Reference

### adding-llm-providers

Adds or updates VT Code LLM providers and model catalog entries. Use when adding a new LLM provider, adding models to an existing provider, or changing provider defaults/model picker support.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/adding-llm-providers/SKILL.md`

### adding-workspace-crate

Adds a new crate to the VT Code workspace with all required metadata, documentation, release script integration, and crates.io compatibility. Use when creating a new workspace crate, extracting code into a new crate, or splitting an existing crate.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/adding-workspace-crate/SKILL.md`

### agentic-testing

Risk-first testing nudge for agentic coding. Use when writing or reviewing tests to avoid panic-only and identical-fixture traps.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/agentic-testing/SKILL.md`

### android-cli

Orchestrates Android development tasks including project creation, deployment, SDK management, and environment diagnostics using the `android` command-line tool.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/.agents/skills/android-cli/SKILL.md`

### ast-grep

Use for ast-grep project setup, rule authoring, rule debugging, and structural search workflows.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Library/Application Support/com.vinhnx.vtcode/skills/.system/ast-grep/SKILL.md`

### audit-module-agents

Audit and update per-module AGENTS.md files when making significant changes to a crate. Use after adding new modules, changing conventions, discovering gotchas, or modifying public APIs.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/audit-module-agents/SKILL.md`

### charon-rust-analysis

Deep semantic analysis of Rust crates with Charon. Use when a question needs what rustc actually computes — real symbol resolution, drop glue and drop order, repr layout and its stability guarantees, monomorphization, trait selection, or unwind paths — beyond what the syntactic code_search/ast-grep surface can prove.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/charon-rust-analysis/SKILL.md`

### cmd-agent

Manage subagents and delegated child threads (usage: /agent [list|threads|inspect <id>|close <id>|create [project|user] [name]|edit [name]|delete <name>])

- **Type**: Built-in command skill
- **Slash Alias**: `/agent`
- **Usage**: `/agent [list|threads|inspect <id>|close <id>|create [project|user] [name]|edit [name]|delete <name>]`

### cmd-analyze

Perform comprehensive codebase analysis and generate reports (usage: /analyze [full|security|performance])

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/cmd-analyze/SKILL.md`

### cmd-auth

Show authentication status for providers (usage: /auth [provider])

- **Type**: Built-in command skill
- **Slash Alias**: `/auth`
- **Usage**: `/auth [provider]`

### cmd-clear

Clear visible screen (usage: /clear [new])

- **Type**: Built-in command skill
- **Slash Alias**: `/clear`
- **Usage**: `/clear [new]`

### cmd-command

Run a terminal command (usage: /command <program> [args...])

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/cmd-command/SKILL.md`

### cmd-compact

Compact the current conversation immediately or manage the saved manual compaction prompt

- **Type**: Built-in command skill
- **Slash Alias**: `/compact`
- **Usage**: `/compact [--instructions <text>] [--max-output-tokens <n>] [--reasoning-effort <none|minimal|low|medium|high|xhigh>] [--verbosity <low|medium|high>] [--native-only] | /compact edit-prompt | /compact reset-prompt`

### cmd-config

Browse settings and run workspace panels: memory, permissions, model, ide, tasks, jobs, log, subprocess, notify, checkup, or reset the active layer (usage: /config [memory|permissions|model|ide|tasks|jobs|log|subprocess|notify|checkup|<path>|reset])

- **Type**: Built-in command skill
- **Slash Alias**: `/config`
- **Usage**: `/config [memory|permissions|model|ide|tasks|jobs|log|subprocess|notify|checkup|<path>|reset]`

### cmd-copy

Copy the latest complete assistant reply to clipboard

- **Type**: Built-in command skill
- **Slash Alias**: `/copy`
- **Usage**: `/copy`

### cmd-docs

Open vtcode documentation in web browser

- **Type**: Built-in command skill
- **Slash Alias**: `/docs`
- **Usage**: `/docs`

### cmd-donate

Support the project by buying the author a coffee

- **Type**: Built-in command skill
- **Slash Alias**: `/donate`
- **Usage**: `/donate`

### cmd-exit

Exit the session

- **Type**: Built-in command skill
- **Slash Alias**: `/exit`
- **Usage**: `/exit`

### cmd-explain

Explain recorded execution without a model call

- **Type**: Built-in command skill
- **Slash Alias**: `/explain`
- **Usage**: `/explain [--scope task|session] [--details|diagram|--web|--export html]`

### cmd-fast

Toggle native OpenAI ultrafast or Claude fast at premium pricing (usage: /fast [on|off])

- **Type**: Built-in command skill
- **Slash Alias**: `/fast`
- **Usage**: `/fast [on|off]`

### cmd-feedback

Open a new GitHub issue to report a bug or request a feature

- **Type**: Built-in command skill
- **Slash Alias**: `/feedback`
- **Usage**: `/feedback`

### cmd-files

Browse and select files from workspace (usage: /files [filter])

- **Type**: Built-in command skill
- **Slash Alias**: `/files`
- **Usage**: `/files [filter]`

### cmd-fork

Fork an archived session into a new thread (usage: /fork [limit] [--all])

- **Type**: Built-in command skill
- **Slash Alias**: `/fork`
- **Usage**: `/fork [limit] [--all]`

### cmd-help

Show slash command help

- **Type**: Built-in command skill
- **Slash Alias**: `/help`
- **Usage**: `/help [command]`

### cmd-history

Open command history picker (usage: /history, same as Ctrl+R)

- **Type**: Built-in command skill
- **Slash Alias**: `/history`
- **Usage**: `/history`

### cmd-init

Guided workspace setup for vtcode.toml, AGENTS.md, and indexing (usage: /init [--force])

- **Type**: Built-in command skill
- **Slash Alias**: `/init`
- **Usage**: `/init [--force]`

### cmd-local

Manage local inference servers: Ollama, LM Studio, llama.cpp (usage: /local [action] [provider])

- **Type**: Built-in command skill
- **Slash Alias**: `/local`
- **Usage**: `/local [status|start|stop|configure|troubleshoot] [ollama|lmstudio|llamacpp]`

### cmd-login

Authenticate with OpenAI, OpenRouter, or GitHub Copilot (usage: /login [provider])

- **Type**: Built-in command skill
- **Slash Alias**: `/login`
- **Usage**: `/login [provider]`

### cmd-logout

Clear stored provider authentication (usage: /logout [provider])

- **Type**: Built-in command skill
- **Slash Alias**: `/logout`
- **Usage**: `/logout [provider]`

### cmd-mcp

Open interactive MCP manager (usage: /mcp, optional subcommands still supported)

- **Type**: Built-in command skill
- **Slash Alias**: `/mcp`
- **Usage**: `/mcp [status|list|tools|refresh|config|config edit|repair|diagnose|login <name>|logout <name>]`

### cmd-mode

Switch the active agent mode (usage: /mode [agent-name])

- **Type**: Built-in command skill
- **Slash Alias**: `/mode`
- **Usage**: `/mode [agent-name]`

### cmd-model

Launch the interactive model picker (includes reasoning effort)

- **Type**: Built-in command skill
- **Slash Alias**: `/model`
- **Usage**: `/model (alias: /models)`

### cmd-new

Start a new session

- **Type**: Built-in command skill
- **Slash Alias**: `/new`
- **Usage**: `/new`

### cmd-pause

Pause the active turn at the next safe boundary

- **Type**: Built-in command skill
- **Slash Alias**: `/pause`
- **Usage**: `/pause`

### cmd-plan

Start or continue the planning workflow with an optional task prompt (usage: /plan [task])

- **Type**: Built-in command skill
- **Slash Alias**: `/plan`
- **Usage**: `/plan [task]`

### cmd-plugin

Open interactive Agent Plugins manager (usage: /plugin, /plugin manager)

- **Type**: Built-in command skill
- **Slash Alias**: `/plugin`
- **Usage**: `/plugin [manager|list|info <name>|add <source> [--name <id>]|remove <name>|validate <path>|refresh|help]`

### cmd-redo

Restore files and conversation from before the last rewind

- **Type**: Built-in command skill
- **Slash Alias**: `/redo`
- **Usage**: `/redo`

### cmd-refresh-oauth

Refresh stored provider credentials when supported (usage: /refresh-oauth [provider])

- **Type**: Built-in command skill
- **Slash Alias**: `/refresh-oauth`
- **Usage**: `/refresh-oauth [provider]`

### cmd-resume

List archived sessions when idle; resume the active turn while it is paused

- **Type**: Built-in command skill
- **Slash Alias**: `/resume`
- **Usage**: `/resume [limit|--all]`

### cmd-review

Review the current diff or selected files (usage: /review [instructions | --last-diff | --target <expr> | --file <path> | files...] [--style <style>])

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/cmd-review/SKILL.md`

### cmd-rewind

Open the rewind picker or restore a specific checkpoint (usage: /rewind [turn] [conversation|code|both])

- **Type**: Built-in command skill
- **Slash Alias**: `/rewind`
- **Usage**: `/rewind [turn] [conversation|code|both]`

### cmd-secret

Manage provider API keys in your OS keyring (usage: /secret [list|status [provider] [key-name]|add <provider> [key-name]|delete <provider> [key-name]|migrate [provider]|help])

- **Type**: Built-in command skill
- **Slash Alias**: `/secret`
- **Usage**: `/secret [list|status [provider] [key-name]|add <provider> [key-name]|delete <provider> [key-name]|migrate [provider]|help]`

### cmd-share

Export the current session as JSON, Markdown, or self-contained HTML timeline (usage: /share [json|markdown|md|html|--format <fmt>])

- **Type**: Built-in command skill
- **Slash Alias**: `/share`
- **Usage**: `/share [json|markdown|md|html|--format <fmt>]`

### cmd-skills

Open interactive skills manager (usage: /skills, /skills manager)

- **Type**: Built-in command skill
- **Slash Alias**: `/skills`
- **Usage**: `/skills [manager|list|search|create|load|unload|info|use|validate|package|regenerate-index|help]`

### cmd-status

Show model, provider, workspace, and tool status

- **Type**: Built-in command skill
- **Slash Alias**: `/status`
- **Usage**: `/status`

### cmd-statusline

Set up a custom status line with target selection (usage: /statusline [instructions...])

- **Type**: Built-in command skill
- **Slash Alias**: `/statusline`
- **Usage**: `/statusline [instructions...]`

### cmd-stop

Stop the active turn immediately

- **Type**: Built-in command skill
- **Slash Alias**: `/stop`
- **Usage**: `/stop`

### cmd-terminal-setup

Configure terminal for VT Code (multiline, copy/paste, shell, themes)

- **Type**: Built-in command skill
- **Slash Alias**: `/terminal-setup`
- **Usage**: `/terminal-setup`

### cmd-theme

Switch UI theme (usage: /theme <theme-id>)

- **Type**: Built-in command skill
- **Slash Alias**: `/theme`
- **Usage**: `/theme [theme-id]`

### cmd-title

Configure the terminal title items interactively

- **Type**: Built-in command skill
- **Slash Alias**: `/title`
- **Usage**: `/title`

### cmd-transcript

Inspect or manage the bounded TUI transcript snapshot (usage: /transcript [stats|clear|export [path]])

- **Type**: Built-in command skill
- **Slash Alias**: `/transcript`
- **Usage**: `/transcript [stats|clear|export [path]]`

### cmd-update

Check for new VT Code releases and install updates (usage: /update [check|install] [--force], or run `vtcode update` from the CLI)

- **Type**: Built-in command skill
- **Slash Alias**: `/update`
- **Usage**: `/update [check|install] [--force]`

### cmd-vim

Toggle Vim-style prompt editing for this session (usage: /vim [on|off])

- **Type**: Built-in command skill
- **Slash Alias**: `/vim`
- **Usage**: `/vim [on|off]`

### cmd-webmcp

Manage the opt-in authenticated browser editor bridge (usage: /webmcp [help|status|tools|roots|pair [--replace] <origin>|unpair])

- **Type**: Built-in command skill
- **Slash Alias**: `/webmcp`
- **Usage**: `/webmcp [help|status|tools|roots|pair [--replace] <origin>|unpair]`

### codemod

Use Codemod CLI whenever the user wants to migrate, upgrade, update, or refactor a codebase in a repeatable way. This includes framework migrations, library upgrades, version bump migrations, API surface changes, deprecations, and large-scale mechanical edits. First search the Codemod Registry for an existing package, prefer deterministic codemods before open-ended AI rewrites, run dry-runs before apply, and create a codemod package only when no suitable package exists.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/codemod/SKILL.md`

### evaluate-crate

Evaluate whether a Rust crate, library, or external resource is useful for the vtcode project. Use when the user shares a crate URL, article, or reference and asks "can we use this to improve vtcode".

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/evaluate-crate/SKILL.md`

### generate-product-spec-patch

Generate a numbered patch file (product-spec-patch-NNN.md) to surgically update a finalized product-spec.md when the backend contract, API field, enum value, error response, validation rule, or product requirement changes. Use this whenever the BE changes something that affects the spec, error messages change, a field type updates, an enum is renamed, or the spec falls out of sync with the implementation. Triggers on: "patch the spec", "BE changed X", "contract changed", "update product-spec", field type change, enum rename, response schema mismatch, or "spec is out of sync".

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/.agents/skills/generate-product-spec-patch/SKILL.md`

### jira-to-product-spec

Generate product spec from Jira/Confluence PRD. Outputs 9-section spec: Metadata, Business Context, Functional Requirements (FR-CORE-001+), Acceptance Criteria (20+), User Flows (4+ with ASCII diagrams), [CONDITIONAL] API Contracts (grounded in backend-conventions.md), Platform Implementation (3 bullets per platform), Dependencies & Open Items (design conflicts + open questions sorted by criticality), and Appendix. Assumption-free: every claim is either verbatim from PRD, user-confirmed via HITL, or surfaced as a Q-XXX open question. Use when user mentions 'product spec', 'write spec', 'generate spec', 'specification', or provides Jira/Confluence URL. Includes 3 enforcement gates to prevent API hallucinations (verify conventions → validate checklist → pre-gen approval).

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/.agents/skills/jira-to-product-spec/SKILL.md`

### mcp-apps

Build MCP Apps (SEP-1865, `io.modelcontextprotocol/ui`) — MCP servers that declare `_meta.ui.resourceUri` on a tool and serve a `ui://` resource whose content is `text/html;profile=mcp-app`, rendered in a host iframe and driven over `postMessage` JSON-RPC. Use when creating, testing, or reasoning about an interactive MCP UI, or when a user asks for a visual/form/dashboard MCP tool.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/mcp-apps/SKILL.md`

### mimo-browser-use

Prefer this skill for web search, opening URLs, reading articles, filling forms, and page UI. Controls IAB (default), Chrome, Edge, Brave, Chromium, or raw CDP via the shared js MCP for navigation, inspection, clicking, typing, tabs, downloads, dialogs, and screenshots.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/.agents/skills/mimo-browser-use/mimo-browser-use/SKILL.md`

### pr-code-review

Spin up a PR code review sub-agent. Ranks bugs critical/high/medium/low, eliminates false positives, writes consolidated report, applies KISS/DRY fixes (main logic only, skip tests), re-reviews after every change, loops until done. Use when the user asks to review a PR, triage bugs, or run a code review fix loop.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/pr-code-review/SKILL.md`

### prd-diff-review

Audit a product-spec.md against its original Jira/Confluence source to find hallucinations, intentional PIC additions, and unverified claims. Use after generating a PRD when you want to verify it against the source, surface what was added vs sourced, or identify invented details. Triggers on: 'review prd vs jira', 'audit the prd', 'check for hallucinations', 'diff spec against source', 'verify prd requirements', or /prd-diff-review. Then approve, remove, or modify flagged items, and optionally apply changes back to the spec.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/.agents/skills/prd-diff-review/SKILL.md`

### profiling-guide

Apply Rust profiling-guide lessons to VT Code layer by layer (wall-clock, HTTP, I/O, locks, channels, allocs, CPU) using hotpath instrumentation, audits, and deterministic signals. Use when profiling, hunting bottlenecks, or wiring new hotpath telemetry.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/profiling-guide/SKILL.md`

### read-and-apply

Read an external article, book chapter, or technical resource and apply its lessons to improve the vtcode codebase. Use when the user shares a URL or pastes article text and says 'read and apply' or 'improve vtcode based on this'.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/read-and-apply/SKILL.md`

### rust-skills

193 Rust coding rules across 14 categories. Use when writing, reviewing, or refactoring Rust code. Covers ownership, errors, async, API design, memory, performance, testing, and anti-patterns.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/rust-skills/SKILL.md`

### skill-creator

Guide for creating effective skills. This skill should be used when users want to create a new skill (or update an existing skill) that extends VT Code's capabilities with specialized knowledge, workflows, or tool integrations.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Library/Application Support/com.vinhnx.vtcode/skills/.system/skill-creator/SKILL.md`

### skill-installer

Install VT Code skills into ~/.agents/skills from a curated list or a GitHub repo path. Use when a user asks to list installable skills, install a curated skill, or install a skill from another repo, including private repos.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Library/Application Support/com.vinhnx.vtcode/skills/.system/skill-installer/SKILL.md`

### source-command-fix-failing-tests

Placeholder: the fix-failing-tests agent is not currently implemented in this workspace.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/source-command-fix-failing-tests/SKILL.md`

### verification-discipline

Apply Creusot's deductive-verification discipline — explicit contracts, executable invariants, absence of overflow and panics, justified unsafe/trusted assumptions, termination, and views-as-models — to ordinary Rust without the Creusot/Why3 toolchain. Use when hardening Rust invariants and contracts, auditing overflow/panic/unsafe assumptions, or codifying verification-grade rigor (contract docs, debug_assert, checked/saturating arithmetic) in a codebase.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/verification-discipline/SKILL.md`

### vtcode-check

Run the full quality gate for the vtcode project, automating execution of ./scripts/check.sh.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/vtcode-check/SKILL.md`

### vtcode-clippy

Run clippy on the vtcode project to ensure code quality follows strict linting rules.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/vtcode-clippy/SKILL.md`

### vtcode-dev-helper

Development helper for the vtcode project. Use to run the quality gate, workspace tests, clippy, and other development tasks following project standards.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/vtcode-dev-helper/SKILL.md`

### vtcode-hawk

Run hawk analysis on the workspace to find dead public code and unnecessary visibility.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/vtcode-hawk/SKILL.md`

### vtcode-test

Run tests for the vtcode project, prioritizing cargo nextest for speed.

- **Type**: Skill
- **Path**: `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.agents/skills/vtcode-test/SKILL.md`


---
*Generated automatically. Do not edit manually.*
