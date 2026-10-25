IMPORTANT: https://app.coderabbit.ai/deep-scans?owner=vinhnx&repo=VTCode&repoId=1046809510

---

# Injection

- **Repository:** `vinhnx/VTCode`
- **Severity:** high
- **State:** Open
- **Location:** `crates/codegen/vtcode-config/src/api_keys.rs:131-131`
- **Reachability:** external
- **Exploitability:** moderate

## Description

Startup imports workspace dotenv settings before Copilot command selection. A repository can therefore replace the authentication executable that an ordinary login command launches with the user's filesystem authority.

**Evidence**

- Source/control: A supplied checkout's .env sets VTCODE_COPILOT_COMMAND to an attacker-selected executable and arguments.
- Sink/decision: run_captured_command spawns the resolved authentication program.
- Missing mitigation: Executable selection consumes repository-imported environment values as trusted user configuration.

## Recommendation

Capture trusted command overrides before loading workspace data, and prevent repository dotenv entries from selecting authentication executables. Keep workspace credentials in a separate restricted lookup.

===

# Authorization bypass

- **Repository:** `vinhnx/VTCode`
- **Severity:** high
- **State:** Open
- **Location:** `crates/codegen/vtcode-config/src/api_keys.rs:131-131`
- **CWE:** CWE-807
- **Reachability:** external
- **Exploitability:** moderate

## Description

Workspace dotenv values enter the global environment before configuration selection. A repository-controlled configuration selector is therefore treated as explicit user authorization to load executable provider authentication settings.

Repository dotenv input can select VTCODE_CONFIG_PATH, which startup treats as an explicit trusted configuration choice. This bypasses the restriction on repository-defined authentication commands and allows an ordinary provider request to execute an attacker-selected program.

A workspace .env can supply VTCODE_CONFIG_PATH before startup decides which configuration sources are trusted. The selected file is then accepted as an explicit user opt-in, allowing repository-controlled authentication commands to bypass the policy that normally rejects them and execute with the user's authority.

**Evidence**

- Source/control: A supplied repository's .env selects an accompanying custom-provider configuration through VTCODE_CONFIG_PATH.
- Sink/decision: CustomProviderAuthHandle::fetch_token executes the configured authentication command through command.output().
- Missing mitigation: Configuration selection does not distinguish inherited user settings from values imported out of repository dotenv files before granting the explicit-file trust exception.

## Recommendation

Keep workspace dotenv entries out of the trusted process settings used for configuration selection. Grant explicit-file trust only to a CLI-selected path or a selector captured from the inherited environment before reading workspace files.

Resolve trusted configuration selectors before reading workspace dotenv data, and preserve their provenance. Do not let repository-imported variables select an explicit trusted config file or executable authentication settings.

Capture trusted configuration selectors before loading workspace dotenv, or exclude VTCODE_CONFIG_PATH from those imports. Grant explicit-file trust only when the selector came from a trusted launch-time source.

---

# Sensitive data exposure

- **Repository:** `vinhnx/VTCode`
- **Severity:** high
- **State:** Open
- **Location:** `crates/codegen/vtcode-config/src/api_keys.rs:131-131`
- **CWE:** CWE-200
- **Reachability:** external
- **Exploitability:** moderate

## Description

Repository dotenv values influence the provider destination while credential resolution still supplies the user's private credential. Requests attach that credential without verifying that the destination was selected through trusted user settings.

Repository dotenv values can select OPENAI_BASE_URL, and provider construction treats that lower-trust value as an authorized destination. The provider then attaches the victim's bearer credential and request content to a request sent to the selected server.

**Evidence**

- Source/control: A supplied checkout's .env sets OPENAI_BASE_URL.
- Sink/decision: OpenAI request authorization attaches the private bearer credential to a request whose destination came from the repository-imported endpoint.
- Missing mitigation: Endpoint selection does not preserve the distinction between inherited user configuration and repository dotenv input before attaching private credentials.

## Recommendation

Keep repository dotenv entries out of trusted endpoint settings. Use a separate credential lookup and require trusted user configuration or explicit approval before changing a credential-bearing destination.

Exclude provider destination selectors from workspace dotenv imports or resolve them exclusively from trusted configuration captured beforehand. Attach provider credentials only after establishing that the selected origin is trusted.

---

# Injection

- **Repository:** `vinhnx/VTCode`
- **Severity:** high
- **State:** Open
- **Location:** `crates/codegen/vtcode-config/src/loader/manager.rs:1190-1195`
- **CWE:** CWE-78
- **Reachability:** external
- **Exploitability:** moderate

## Description

Repository configuration can replace the executable used for Copilot authentication. The origin-aware restriction on executable authentication omits auth.copilot.command, allowing repository content to execute a selected program with the victim's authority.

Automatically loaded repository configuration can select the executable and initial arguments used for Copilot login. The origin-aware validator prohibits repository-supplied executable authentication settings in other provider configuration but omits auth.copilot.command, allowing repository content to execute with the user's authority.

**Evidence**

- Source/control: auth.copilot.command in an effective workspace or project configuration layer.
- Sink/decision: run_captured_command spawns the selected program and configured arguments.
- Missing mitigation: The repository-origin executable restriction does not cover Copilot's command setting before authentication consumes it.

## Recommendation

Reject repository-origin auth.copilot.command values during configuration loading. Allow executable replacement only from trusted user/system configuration or an explicit operator-selected configuration.

Add auth.copilot.command to the existing origin-aware restrictions on executable authentication settings. Permit it only from trusted system, user, or explicitly selected configuration, and enforce the restriction before any Copilot process launch.

---

# Path traversal

- **Repository:** `vinhnx/VTCode`
- **Severity:** high
- **State:** Open
- **Location:** `crates/codegen/vtcode-indexer/src/lib.rs:142-142`
- **CWE:** CWE-59
- **Reachability:** external
- **Exploitability:** moderate

## Description

Snapshot persistence opens a predictable temporary pathname with File::create, which follows existing symlinks and truncates their targets. Workspace-controlled filesystem entries can redirect metadata writes outside the intended index directory and corrupt files writable by the victim process.

**Evidence**

- Source/control: A planted .vtcode/index/.index.md.tmp symlink, or a symlinked component of the index-directory path, in an attacker-supplied workspace.
- Sink/decision: fs::File::create(&temp_path), followed by generated Markdown writes.
- Missing mitigation: Temporary-file creation does not require a fresh file, reject symlinks, or resolve the output relative to a trusted directory handle.

## Recommendation

Create a fresh temporary file exclusively inside a verified index directory, then atomically replace the snapshot. Enforce containment while resolving directory components, and perform replacement and cleanup relative to the trusted directory.

---

# Path traversal

- **Repository:** `vinhnx/VTCode`
- **Severity:** high
- **State:** Open
- **Location:** `crates/codegen/vtcode-indexer/src/markdown_store.rs:196-196`
- **CWE:** CWE-59
- **Reachability:** external
- **Exploitability:** moderate

## Description

Storage writes open paths assembled from logical keys without enforcing containment or rejecting symlinked components. A planted project-record symlink can redirect ordinary project initialization to truncate and overwrite a victim-writable file outside the workspace.

**Evidence**

- Source/control: A workspace-controlled .vtcode/projects/<project-name>.md symlink or symlinked parent directory. Public storage APIs additionally accept logical keys containing absolute or parent path components.
- Sink/decision: write_with_lock opens the resolved pathname, calls file.set_len(0), and writes generated Markdown or supplied cache bytes.
- Missing mitigation: Logical keys are not restricted to single filename components, and the writer does not enforce containment or reject symlink redirection during filesystem resolution.

## Recommendation

Validate logical keys and project names as single normal filename components. Resolve writes relative to a trusted storage-directory handle with containment and symlink restrictions enforced during resolution, then replace files safely.

---

# Injection

- **Repository:** `vinhnx/VTCode`
- **Severity:** high
- **State:** Open
- **Location:** `crates/codegen/vtcode-llm/src/copilot/command.rs:139-141`
- **Reachability:** external
- **Exploitability:** moderate

## Description

Repository configuration can replace the executable and initial arguments launched as the Copilot ACP runtime. The configuration-origin validator omits auth.copilot.command, so starting an authenticated Copilot prompt can execute repository-selected code with the user's operating-system authority.

**Evidence**

- Source/control: auth.copilot.command from an automatically loaded repository configuration layer.
- Sink/decision: spawn_copilot_acp_process calls spawn on the selected program before ACP initialization.
- Missing mitigation: No repository-origin restriction or executable approval runs before the configured ACP program is launched.

## Recommendation

Reject repository-origin auth.copilot.command through the existing configuration-origin validator before provider construction. Apply that restriction consistently to authentication and ACP launches, including configuration reloads.

--

# Security misconfiguration

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-acp/src/zed/agent/mod.rs:81-84`
- **CWE:** CWE-693
- **Reachability:** external
- **Exploitability:** difficult

## Description

ACP omits the effective sandbox configuration when initializing its local tool registry. Approved local commands consequently launch without the operator-requested sandbox, allowing project code to access host resources that the configured policy should restrict.

**Evidence**

- Source/control: Lower-trust project code executed by an otherwise permitted, client-approved command during an ACP session.
- Sink/decision: Local pipe-session creation using an unwrapped command and sandbox_active false.
- Missing mitigation: The local ACP registry never receives the configured sandbox policy. Command approval does not enforce filesystem or network restrictions on the launched process and its descendants.

## Recommendation

Pass the effective sandbox configuration through ACP startup and apply it before local tools become available. Refuse execution when a requested restrictive sandbox cannot be installed, instead of launching the command without isolation.

---

# Authorization bypass

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-acp/src/zed/agent/session_state.rs:572-592`
- **CWE:** CWE-863
- **Reachability:** external
- **Exploitability:** difficult

## Description

ACP constructs the session-selected provider without the configured provider whitelist. A connected client can therefore route conversation content to a usable provider that the operator explicitly excluded.

**Evidence**

- Source/control: The connected ACP peer supplies the provider selection through session/set_config_option.
- Sink/decision: build_session_provider calls create_provider_with_config, whose factory invocation uses an empty whitelist; run_prompt subsequently submits the session messages.
- Missing mitigation: The ACP bridge drops the effective whitelist before session handling and provider creation. Registry membership is checked instead of whether the operator permits that provider.

## Recommendation

Carry the effective whitelist into the ACP agent, filter provider selections through it, and enforce it again when constructing providers for prompts and compaction. Reject excluded providers before loading credentials or sending messages.

---

# Authorization bypass

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-acp/src/zed/agent/tool_execution.rs:123-131`
- **CWE:** CWE-863
- **Reachability:** external
- **Exploitability:** difficult

## Description

ACP dispatches model-returned calls without enforcing the selected primary agent's permissions against the concrete operation. Selecting the read-only plan agent also leaves planning enforcement inactive, allowing otherwise permitted mutating commands to execute under a read-only selection.

**Evidence**

- Source/control: Prompt text, embedded resources, and linked workspace content enter model context; model output supplies tool names and arguments.
- Sink/decision: execute_descriptor dispatches catalog-resolved calls without the selected agent's policy, including local execution with a default execution snapshot.
- Missing mitigation: Advertising restrictions are not repeated as an authoritative check before execution. The selected plan agent's read-only policy is also not bound to the execution context.

## Recommendation

Evaluate the selected agent's effective permissions against each concrete call before dispatch. Bind read-only planning enforcement to the session's selected agent and apply it to both local execution and client terminal requests, independently of client approval.

---

# Authorization bypass

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-acp/src/zed/agent/tool_execution.rs:211-215`
- **CWE:** CWE-863
- **Reachability:** external
- **Exploitability:** difficult

## Description

ACP lets model arguments select client terminal execution before the local command-policy check. Consequently, commands configured as always denied can be sent for execution through terminal/create.

**Evidence**

- Source/control: The model-returned exec_command call supplies the command and selects PTY execution through mode or tty.
- Sink/decision: launch_client_terminal passes the parsed executable and arguments to ConnectionHandle::create_terminal without command-policy admission.
- Missing mitigation: The effective command policy is enforced only by the local execution path. Backend selection can therefore remove a mandatory deny decision.

## Recommendation

Evaluate the concrete executable and arguments against the effective command policy before choosing an execution backend. Reject denied invocations before sending terminal/create, even when the client approves the call.

---

# Sensitive data exposure

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-auth/src/credentials/mod.rs:107-111`
- **Reachability:** external
- **Exploitability:** difficult

## Description

Credential storage joins two validated identity components with an underscore that is also allowed within those components. Different provider/key identities can consequently share a stored secret, violating the API's documented profile isolation.

**Evidence**

- Source/control: Caller-supplied provider and key_name arguments to the exported credential APIs.
- Sink/decision: The account string constructed by CustomApiKeyStorage::for_identity selects the keyring entry and encrypted credential file.
- Missing mitigation: The backend account does not preserve the boundary between provider and key name, and stored values carry no independently checked tuple identity.

## Recommendation

Encode the normalized tuple unambiguously, using length-prefixed components or a structured representation before hashing. Migrate ambiguous existing entries conservatively rather than assigning them automatically to either profile.

---

# Sensitive data exposure

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-auth/src/mcp_oauth.rs:197-197`
- **CWE:** CWE-200
- **Reachability:** external
- **Exploitability:** difficult

## Description

MCP credential lookup uses a provider name without checking the OAuth identity or protected resource associated with the stored token. Repository configuration can replace that provider's destinations, causing the application to send an existing credential to a different server.

**Evidence**

- Source/control: A repository-controlled vtcode.toml or workspace configuration layer defining an enabled same-name HTTP MCP provider.
- Sink/decision: Provider-name credential lookup authorizes reuse; rmcp_client.rs:394-396 attaches the access token to the replacement endpoint, and mcp_oauth.rs:309-321 sends a refresh token to the replacement token URL.
- Missing mitigation: Stored tokens lack an independently checked binding to their OAuth server, client, and protected resource. Repository-origin MCP endpoint changes are not rejected before credential retrieval and transmission.

## Recommendation

Store and verify the OAuth server, client, and protected-resource identity before reusing or refreshing a token. Reject repository-origin changes to globally authenticated MCP destinations unless the user explicitly approves the new credential binding.

---

# Injection

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-bash-runner/src/executor.rs:201-201`
- **CWE:** CWE-78
- **Reachability:** external
- **Exploitability:** difficult

## Description

The Windows runner inserts caller-supplied strings into executable PowerShell source while escaping only ASCII apostrophes. Typographic single quotation marks can change the string boundary and allow input intended as data to execute commands with the process's authority.

**Evidence**

- Source/control: The pattern supplied to exported BashRunner::grep; filesystem path arguments also use the affected quoting strategy.
- Sink/decision: ProcessCommandExecutor passes the generated Windows script to powershell -Command.
- Missing mitigation: The generated script does not keep arbitrary caller strings separate from PowerShell syntax, and its literal escaping does not cover PowerShell's recognized quotation delimiters.

## Recommendation

Use a fixed PowerShell script and pass untrusted values through a data-binding mechanism that does not evaluate them as source. If literal generation is retained, use quoting that covers every delimiter PowerShell recognizes.

---

# Path traversal

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-bash-runner/src/runner.rs:203-203`
- **CWE:** CWE-22
- **Reachability:** external
- **Exploitability:** difficult

## Description

The Windows removal command passes a checked literal path through PowerShell's wildcard-sensitive -Path parameter. Wildcard expansion can select a different filesystem resource after containment validation, allowing deletion outside the configured workspace.

**Evidence**

- Source/control: A caller-selected removal path under a workspace whose pathname contains PowerShell wildcard syntax.
- Sink/decision: Remove-Item -Path interprets the validated pathname as a pattern when the process executor evaluates the generated script.
- Missing mitigation: The final filesystem operation does not retain the literal resource identity that passed authorization. The runner neither uses -LiteralPath nor confines the resources selected by wildcard expansion.

## Recommendation

Use -LiteralPath for removal and other existing-path operations that must preserve literal filesystem identity. Audit each remaining PowerShell path parameter so the resource consumed by the operation is the same resource that passed containment checks.

---

# Path traversal

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-bash-runner/src/executor.rs:335-335`
- **CWE:** CWE-61
- **Reachability:** external
- **Exploitability:** difficult

## Description

The pure-Rust copy backend follows descendant filesystem links after the runner has checked only the top-level paths. A permitted recursive copy can consequently read or overwrite files outside the workspace through attacker-supplied links.

**Evidence**

- Source/control: Lower-trust symlinks within an authorized source directory or an existing destination directory.
- Sink/decision: fs::copy follows descendant source or destination links without verifying workspace containment.
- Missing mitigation: Recursive filesystem operations do not preserve the workspace boundary for each discovered source and destination. Both directory traversal and file copying can follow links beyond the authorized roots.

## Recommendation

Perform recursive operations through directory handles constrained beneath the workspace and reject escaping links on both sides. Define explicit symlink-copy behavior and avoid implicitly dereferencing links after authorization.

---

# Path traversal

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-bash-runner/src/runner.rs:203-203`
- **CWE:** CWE-22
- **Reachability:** external
- **Exploitability:** difficult

## Description

The Windows removal command passes a checked literal path through PowerShell's wildcard-sensitive -Path parameter. Wildcard expansion can select a different filesystem resource after containment validation, allowing deletion outside the configured workspace.

**Evidence**

- Source/control: A caller-selected removal path under a workspace whose pathname contains PowerShell wildcard syntax.
- Sink/decision: Remove-Item -Path interprets the validated pathname as a pattern when the process executor evaluates the generated script.
- Missing mitigation: The final filesystem operation does not retain the literal resource identity that passed authorization. The runner neither uses -LiteralPath nor confines the resources selected by wildcard expansion.

## Recommendation

Use -LiteralPath for removal and other existing-path operations that must preserve literal filesystem identity. Audit each remaining PowerShell path parameter so the resource consumed by the operation is the same resource that passed containment checks.

===

# Security misconfiguration

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-config/src/api_keys.rs:131-131`
- **CWE:** CWE-15
- **Reachability:** external
- **Exploitability:** difficult

## Description

Repository dotenv entries configure the updater's proxy and TLS trust roots before its clients are constructed. This lets repository data determine which remote party is trusted to provide release metadata, executable archives, and checksum metadata.

**Evidence**

- Source/control: A supplied checkout's .env selects proxy and certificate settings, with an accompanying attacker-controlled CA file. It can also select the updater configuration root.
- Sink/decision: Authenticated release-metadata requests can disclose the existing GitHub token; after update approval, self_replace installs an executable obtained through the attacker-controlled transport.
- Missing mitigation: Updater routing and certificate trust are derived from the global environment after repository dotenv import, without preserving which settings came from the user.

## Recommendation

Load workspace credentials into a scoped map instead of modifying the process environment. Construct updater clients and resolve updater configuration using trusted settings captured before workspace files are read.

===

# Authorization bypass

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-core/src/core/agent/runner/tool_access.rs:109-119`
- **CWE:** CWE-863
- **Reachability:** external
- **Exploitability:** difficult

## Description

Delegated runners omit their stored local permission policy when deciding whether to admit a tool call. A model-generated operation can therefore execute despite an explicit child-local deny when global and sandbox controls permit it.

**Evidence**

- Source/control: Lower-trust model-generated tool names and arguments in a delegated child carrying a supported SubagentSpec.permissions policy.
- Sink/decision: Global-only permission admission followed by prepared tool execution.
- Missing mitigation: The delegated runner does not attach or evaluate the resolved child policy at concrete tool admission. Tool-name filtering cannot enforce argument-specific or resource-specific local denies.

## Recommendation

Attach the resolved child permission policy to AgentRunner and combine it with global rules for advertisement and every concrete admission. Enforce child-local denies before execution while retaining global and sandbox restrictions.

--

# Denial of service

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-core/src/instructions.rs:1085-1088`
- **CWE:** CWE-407
- **Reachability:** external
- **Exploitability:** moderate

## Description

Rule matching recursively constructs and retains every distinct pattern obtained by removing recursive-directory components. Distinct intervening literals make the number of variants exponential, allowing a small repository rule to exhaust the agent process's memory or stall prompt construction.

**Evidence**

- Source/control: The paths frontmatter of a repository-supplied .vtcode/rules/\*.md file.
- Sink/decision: collect_zero_directory_variants allocates shortened strings, retains them in both collections, and recursively generates further variants.
- Missing mitigation: The matcher has no limit on variant count or retained bytes before recursive expansion. Prompt-size limits do not protect this earlier operation.

## Recommendation

Evaluate recursive-directory matching without enumerating component subsets. As an interim fix, reject patterns with excessive recursive components and enforce a strict work budget before allocating variants.

---

# Sensitive data exposure

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-core/src/instructions.rs:1253-1256`
- **CWE:** CWE-201
- **Reachability:** external
- **Exploitability:** difficult

## Description

The instruction import allowlist includes the user's entire home directory and is shared by repository-authored instructions. A repository author can select a readable private home file for import, causing its contents to enter a model request despite the documented restriction to workspace and VT Code user-config roots.

**Evidence**

- Source/control: Inline import paths in repository-authored AGENTS.md, CLAUDE.md, or loaded workspace rule files.
- Sink/decision: The broad root is accepted by resolve_import_path; expand_instruction_contents reads the selected file at line 911 and returns its contents for system-prompt assembly.
- Missing mitigation: Repository-selected imports are not confined to the documented workspace and VT Code user-config directories. The home-directory root grants repository content authority to read unrelated private files.

## Recommendation

Remove the entire home directory from allowed import roots and retain only the documented canonical workspace and VT Code user-config roots. Apply that boundary before reading an imported file, and require explicit authorization for any supported import outside it.

=---

# Sensitive data exposure

- **Repository:** `vinhnx/VTCode`
- **Severity:** medium
- **State:** Open
- **Location:** `crates/codegen/vtcode-core/src/llm/lightweight_routing.rs:264-264`
- **CWE:** CWE-201
- **Reachability:** external
- **Exploitability:** difficult

## Description

Auxiliary provider construction drops the explicitly configured Anthropic endpoint while reusing the primary provider's credential. Refinement requests and alternate-model tool-output summaries can consequently send private content and a gateway credential to an endpoint outside the configured destination.

**Evidence**

- Source/control: Submitted prompt text or tool-output excerpts, together with the runtime credential for an explicitly configured native Anthropic-compatible gateway.
- Sink/decision: AnthropicProvider::send_request posts the auxiliary request body and x-api-key to the independently resolved endpoint.
- Missing mitigation: Auxiliary clients do not inherit the endpoint associated with the credential they reuse. Provider-name equality is insufficient to preserve the configured destination.

## Recommendation

Build auxiliary clients from the same effective endpoint and authentication configuration as the primary client, changing only the intended model settings. Preserve the explicit Anthropic base URL for initial and fallback requests before attaching credentials.

===

Inspect code changes from the last 24 hours for high-confidence risks involving runtime failures, data loss, authorization bypasses, resource leaks, or cross-platform compatibility, and attach code and commit/diff evidence. Perform read-only analysis using only verifiable repository facts; state when evidence is insufficient, do not speculate, and do not modify code or external state. Also scan for large and monolith files and module and plan deduplication and refactor and extract reusable components.

---

Audit the codebase for oversized files, monolithic modules, duplicated logic, and tightly coupled components. Identify refactoring opportunities, extract reusable components, and improve modularity and separation of concerns. Prioritize by complexity, duplication, maintainability, and impact. Produce a phased refactoring plan with file references, proposed module boundaries, and risks. Follow KISS/DRY, preserve existing behavior and public APIs, and avoid unnecessary abstractions.

---

Inspect code changes from the last 24 hours for high-confidence risks involving runtime failures, data loss, authorization bypasses, resource leaks, or cross-platform compatibility, and attach code and commit/diff evidence. Perform read-only analysis using only verifiable repository facts; state when evidence is insufficient, do not speculate, and do not modify code or external state. Also scan for large and monolith files and module and plan deduplication and refactor and extract reusable components. Also check context management and proper resource cleanup. Check for wasted, redundant, or inefficient tools calls and suggest optimizations where applicable. Check recent sessions for any anomalies or unusual patterns that could indicate potential issues.

--

gui
gpui
gpui-kit

--

computer use https://developers.openai.com/api/docs/guides/tools-computer-use

---

## Goal

Continue building VT Code Desktop into a production-ready, full-featured native application using **GPUI and gpui-kit**, with the quality and polish of OpenAI Codex and Claude Desktop.

Build a distinctive VT Code experience grounded in the project's existing design system, architecture, principles, and agent capabilities.

## Instructions

1. **Resume progress:** Inspect the repository, recent commits, current implementation, unfinished work, and previous decisions. Continue from the latest state, preserving existing functionality.

2. **Apply relevant skills:** Discover and use applicable skills, project guidance, and framework best practices. Review the latest GPUI and gpui-kit documentation before making architectural or UI decisions.

3. **Evaluate the current implementation:** Audit architecture, component reuse, navigation, layouts, interaction patterns, accessibility, performance, and desktop-native behavior. Identify missing functionality and opportunities to improve the user experience.

4. **Design a cohesive experience:** Follow VT Code's design system and principles. Create a distinctive, consistent interface with polished interactions, thoughtful information hierarchy, keyboard-first workflows, responsive layouts, and reusable gpui-kit components.

5. **Build complete workflows:** Progress toward a fully functional coding-agent desktop application, including conversation management, streaming responses, tool execution, approvals, diffs, file navigation, workspace management, settings, and agent activity. Reuse VT Code's existing core capabilities wherever possible.

6. **Implement improvements:** Prioritize unfinished features, critical gaps, and high-impact UX improvements. Recommend the simplest practical approach, then implement it. Continue into other valuable areas without stopping at analysis or planning.

7. **Maintain engineering quality:** Apply DRY, KISS, and separation of concerns. Prefer reusable components, clear state management, lightweight abstractions, and idiomatic Rust. Avoid unnecessary dependencies, duplication, and architectural complexity.

8. **Verify outcomes:** Run relevant checks and tests, fix issues, and validate UI behavior, performance, accessibility, and integration with VT Code's existing systems.

## Execution Principles

- Think broadly about the complete desktop experience before choosing implementation details.
- Use Codex and Claude Desktop as quality references while establishing VT Code's own visual identity.
- Favor working, end-to-end functionality over placeholders or disconnected UI mockups.
- Preserve architectural consistency and avoid unnecessary rewrites.
- Make incremental, verifiable improvements with clear priorities.
- Continue autonomously through the next actionable improvements.
- Report completed changes, verification results, remaining gaps, and recommended next priorities.

## References

- GPUI: https://docs.rs/gpui/latest/gpui/
- gpui-kit: https://gpui-kit.com/llms.txt
- VT Code: https://github.com/vinhnx/VTCode

**Start now:** Inspect the current project state, recover the latest development context, identify the highest-impact unfinished work, and begin implementation. Continue improving the application toward a cohesive, production-ready desktop experience.

--

research with https://docs.typesafe.ai/primitives/choice Jev Choice for VT Code integration

The existing OpenAI Decisions integration is a good foundation to learn from. My recommendation is to keep it for the injection probe and evaluate Jev first for skill suggestion, then memory classification.

I inspected the current source and ran its focused provider tests: 12 passed; 1,236 excluded by the filter.

What VT Code already implements

The current probe (src/agent/runloop/unified/auto_permission/probe.rs:156) classifies tool output as SAFE or SUSPECT:

- Direct OpenAI API-key authentication at the standard endpoint; opt-in for ordinary interactive sessions.
- Evidence limited to 2,400 tool-output characters plus bounded recent user context.
- Four seconds for Decisions, with one generation fallback sharing an eight-second deadline.
- Cancellation, per-turn dispatch limits, transient status, and per-attempt usage accounting.
- Suspicious results produce an advisory warning. Failed classification produces no warning; permission and sandbox enforcement remain separate.

The transport (crates/codegen/vtcode-llm/src/providers/openai/provider/decisions.rs:40) rejects redirects, limits responses to 16 KiB, avoids application retries, and excludes evidence and credentials from errors.

How Jev compares

| Aspect               | Existing OpenAI integration                 | Jev Choice                                             |
| -------------------- | ------------------------------------------- | ------------------------------------------------------ |
| Decision shape       | Exactly one text Choice question            | Multiple questions against shared state                |
| Request              | input, question array, choices              | state, keyed questions, criteria                       |
| Response             | Answer array; probability entries           | Keyed answers; probability map                         |
| Confidence           | Validated and retained; unused by the probe | Available for application-specific abstention          |
| Runtime attachment   | Capability of the active chat provider      | Could be an independently configured auxiliary service |
| Best immediate reuse | Existing injection probe                    | Broader bounded classification tasks                   |

Jev’s batching could help when several independent classifications use the same evidence. This is a documented capability, not demonstrated VT Code latency or cost savings. Choice documentation

One important distinction: Jev’s confidence depends on the selected probability and the number of options. Thresholds need calibration per task and candidate set; they should not be copied from another backend. Confidence formula

What to reuse—and improve

Reuse the existing transport safeguards, cancellation, bounded evidence, unknown-cost handling, and deterministic fallback patterns.

Avoid copying its provider coupling: Jev should be selectable independently of the main chat model, without pretending to support text generation. Start with a small auxiliary client rather than a general decision framework.

Also strengthen response consistency before using probabilities for routing. The current answer validator (crates/codegen/vtcode-llm/src/provider/decisions.rs:94) checks categories, duplicates, and numeric ranges, but does not check that probabilities sum to approximately one or that choice has the highest probability. Existing tests deliberately accept a label that disagrees with the distribution. That is a concrete limitation to address when generalizing this contract.

Where Jev could help elsewhere

| Priority | Integration                | Concrete use and boundary                                                                                                                                |
| -------- | -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1        | Skill suggestion           | Supplement keyword matching with semantic selection. Preserve explicit requests and multi-skill behavior; uncertain results produce no suggestion.       |
| 2        | Memory-fact classification | Classify each grounded candidate as preference, repository_fact, or discard. Keep provenance and memory-write validation authoritative.                  |
| 3        | Tool discovery reranking   | Rerank a local BM25 shortlist. Return existing results on timeout; keep the current synchronous search interface free of network waits.                  |
| 4        | Failure-category selection | After deterministic checks, select a known diagnostic category or abstain. Rust renders established guidance; the classifier does not authorize retries. |

Memory is particularly interesting because the existing implementation (crates/codegen/vtcode-core/src/persistent_memory/llm_ops.rs:164) combines classification and sentence rewriting. Jev could handle classification, but rewriting still needs a generative model. Splitting them only pays off if evaluation shows useful savings or better decisions.

Likewise, failure diagnosis currently generates observed, likely_cause, and next_action. Jev fits selection among known categories, rather than replacing that entire feature.

Recommended next step: compare Jev skill suggestions against current matching on labeled VT Code requests, including paraphrases, overlapping skills, multiple applicable skills, and no-match cases. Keep the OpenAI probe unchanged during that experiment.

No source changes or live model calls were made. The passing tests establish provider-contract and transport behavior, not classification accuracy.

---

hook the desktop <-> cli <-> webmcp apps ecosystem together for consistent behavior

===

destkop: remove the light/dark switch button

====

MOST IMPORATANT. MAKE THE destkop APP LOOK LESS ELECTRON FEEL. and has aesthetic and modern feel.

---

https://claude.com/resources/articles/the-anatomy-of-effective-commerce-agents

---

https://claude.com/resources/articles/building-agents-when-to-fold-subagents-into-your-main-agent-loop

---

https://modelcontextprotocol.io/extensions/apps/overview

## MCP Apps and WebMCP — are they related?

Short answer: no. They are two different protocols that happen to both put a browser in the loop, and the only thing they share is the word "browser". Conflating them costs real time.

### What each one is

|                | MCP Apps (SEP-1865)                                          | WebMCP (this repo's `vtcode-webmcp`)                                                  |
| -------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------- |
| Role           | External MCP **server** ships an interactive UI              | VT Code itself **serves** a bridge to its own browser app                             |
| Direction      | Server → Host → user                                         | Host (VT Code) → browser client                                                       |
| Transport      | `postMessage` JSON-RPC inside a sandboxed iframe             | Axum WebSocket `/webmcp` + Streamable HTTP `/mcp`, loopback-only, bearer/pairing auth |
| Content type   | `text/html;profile=mcp-app` declared by a third-party server | None. Custom `BridgeRequest`/`BridgeResponse` enum; never serves files over HTTP      |
| Unit of work   | A tool result rendered as a view                             | A workspace operation (read, propose patch, apply, run checks)                        |
| Trust boundary | Untrusted third-party HTML, deny-by-default CSP              | Trusted first-party app, authenticated pair, origin-bound                             |

The trust models are opposite. MCP Apps exists to safely render HTML we do not trust; WebMCP exists to let a client we do trust operate the workspace we control. WebMCP's crate rules explicitly forbid adding a file-serving route, which is exactly what an app host needs.

### What would actually be shared

Only two things, and neither is a renderer:

1. **The event feed.** WebMCP already replays `VersionedThreadEvent` through `event_hub.rs` with bounded replay and slow-client handling. That is the correct pipe if a view ever needs to observe a live session.
2. **The approval path.** WebMCP already routes mutations through `ApprovalRequired` and an authoritative diff. Any view-triggered mutation should reuse that, not invent a second one.

### What would genuinely collide

- **Two RPC dialects.** WebMCP uses a versioned custom enum; MCP Apps mandates JSON-RPC 2.0 with `ui/`-prefixed methods. Merging them means one of the two stops being spec-compliant. Keep them separate.
- **Origin policy.** WebMCP binds exact origins and forbids file routes. An app host needs an opaque or host-controlled origin for third-party HTML. Same crate, contradictory invariants.

Conclusion: an app host, if it ever exists, is a **consumer of the WebMCP bridge**, not an addition to the WebMCP crate.

### What VT Code has today

- `resources/list` and `resources/read` are implemented in `vtcode-mcp` and have **zero callers** outside that crate. The path is dead-ended; nothing in core or the TUI consumes it.
- `_meta` on tool **definitions** is silently dropped at `parse_mcp_tool` (`crates/common/vtcode-utility-tool-specs/src/mcp_tool.rs:12-20`) and again at `McpToolInfo`. `_meta.ui.resourceUri` is structurally unrepresentable.
- `_meta` on tool **results** is deliberately stripped at result, content-block, and embedded-resource levels (`crates/codegen/vtcode-mcp/src/client.rs:1031-1047`), guarded by a test and an AGENTS.md rule. That rule is about results only; definition `_meta` is not covered.
- There is no webview anywhere: the TUI is ratatui text, the GPUI desktop app drops HTML in `markdown.rs`, and no `wry`/`cef`/`webkit` dependency exists in the lockfile.
- `rmcp 3.5.0` already exposes `ClientCapabilities.extensions` (SEP-1724) and `Tool.meta`, so the protocol layer needs no new dependency.

### Recommendation, simplest first

**Do now — plumbing and detection, no UI.** About a day, no new dependencies, no policy reversal:

1. Carry definition `_meta` through `ParsedMcpTool` → `McpToolInfo` → registry metadata, filtered to the `ui` key only. Do not pass arbitrary `_meta` into prompts; it is server-controlled text.
2. Advertise `io.modelcontextprotocol/ui` in `ClientCapabilities.extensions` with `mimeTypes: ["text/html;profile=mcp-app"]` **only when a configured mode says VT Code can render apps**, and never otherwise. Advertising a capability we cannot honor is worse than not having it: servers branch on it and ship HTML we then drop.
3. When a tool result carries a `ui`-linked tool and the mode is off, render the existing resource row plus an explicit `[app: ui://…]` marker with a hint that the host cannot render it. The user learns the app exists instead of seeing a blank `[resource: …]` line.
4. Add a `vtcode mcp apps` (or `mcp resources`) CLI subcommand to list `ui://` resources and probe them. Today there is no way to see what a server offers — the `mcp` CLI only manages config. This is the cheapest real value and validates the plumbing against live servers.

**Do next, if the probe step proves demand — rendering, one surface only.** Pick the desktop GPUI app or the browser, not the TUI. The TUI cannot render HTML and a text-mode approximation of an interactive app is a bad product. The browser route reuses the WebMCP event feed; the desktop route needs a webview engine that does not exist yet, which is a licensing, bundling, and sandbox-review project on its own.

**Do not do.** Do not implement an MCP Apps host inside `vtcode-webmcp`. It inverts that crate's trust model and contradicts its no-file-serving rule. Do not build a TUI renderer for apps. Do not strip `_meta` more aggressively to "simplify" — the existing result-level rule is correct for the model context boundary and should stay.

### Open questions

- Is there a real server in use today that ships a `ui://` app? Without one, step 4 is the only step with observable payoff.
- If rendering lands in the desktop app, is a bundled WebKit view acceptable, or does the app stay component-only? This decides whether apps are ever a first-class desktop feature.
- Does advertising the extension to servers change tool registration in ways that break the text fallback? Needs a live multi-provider check before touching `build_initialize_params`.

No source changes yet. The only thing implemented from this is the `mcp-apps` authoring skill at `.agents/skills/mcp-apps/SKILL.md`, which teaches an agent to build a spec-correct app server and to gate on host capability rather than assume it.

---

# MCP Apps implementation plan

Ordered as increments. Each one ships independently, has its own acceptance test, and leaves the tree green. Stop after any increment if the next one no longer pays for itself.

Ground rules for every increment:

- The TUI never renders HTML. Any increment that claims to "support MCP Apps" in the terminal is wrong by construction.
- Do not weaken the result-level `_meta` stripping in `format_tool_result`. That boundary protects the model context and already has a regression test.
- `_meta` is server-controlled text. Anything read from it is bounded and never promoted into prompt text without the existing untrusted-description wrapping.

---

## Increment 0 — Discoverability only (no protocol change)

**Goal:** make `ui://` capability visible without touching the initialize handshake. Lowest risk, only work that can ship unconditionally.

1. Add `McpUiToolMeta` to `crates/codegen/vtcode-mcp/src/types.rs`:

    ```rust
    pub struct McpUiToolMeta {
        pub resource_uri: String,
        pub visibility: Vec<McpToolVisibility>, // Model | App
        pub legacy_flat: bool,                  // came from _meta["ui/resourceUri"]
    }
    ```

2. Extend `ParsedMcpTool` (`crates/common/vtcode-utility-tool-specs/src/mcp_tool.rs:4-20`) with `ui: Option<McpUiToolMeta>`. Parse `_meta.ui.resourceUri` first, fall back to the deprecated `_meta["ui/resourceUri"]`, record which one matched. Reject values that are not `ui://` prefixed — a non-`ui://` URI is a server bug, not a feature.
3. Extend `McpToolInfo` with `ui: Option<McpUiToolMeta>`. This touches every construction site; they are: `provider.rs:617`, `conversion.rs:65`, `mcp_facade.rs:112`, `code_executor.rs:735`, plus test fixtures in `client.rs:1236`, `tool_discovery.rs:290`, `tool_discovery_cache.rs:549`, `registry/tests.rs` (5 sites), `tools/mcp.rs:212`, `mcp_support_overview.rs:312`, `tests/mcp_basic_test.rs:145`. Default `None` everywhere except `provider.rs`.
4. Surface the URI in the generic renderer (`src/agent/runloop/tool_output/mcp.rs:92-101`) where `resource`/`resource_link` blocks are handled. When the URI starts with `ui://`, prefer `[app: <uri>]` over `[resource: <uri>]` and append `type: text/html;profile=mcp-app` as today.
5. Renderer needs the tool's UI metadata, not just the result payload. The result envelope carries no linkage, so pass it through `ToolOutputItem` or look it up by tool name at render time. **Decision required:** lookup by name is simpler and needs no contract change; the envelope change is more correct but touches `vtcode-exec-events::ThreadOutputItem`, so prefer lookup first.

**Acceptance:** a server that declares `_meta.ui.resourceUri` produces an `[app: ui://…]` row; a server that does not is byte-identical to today. Regression test alongside `render_tool_output_mcp_resources_and_media_preserve_content_order` (`src/agent/runloop/tool_output/tests.rs:1486`).

**Risks:** none material. Worst case is a slightly uglier row.

---

## Increment 1 — Orientation surface (CLI)

**Goal:** let a user see what a server actually offers. Fixes the gap that `vtcode mcp` only manages config and cannot inspect a live server.

1. New subcommand in `crates/codegen/vtcode-core/src/mcp/cli.rs:24` `McpCommands`:

    ```
    vtcode mcp resources [--provider <name>] [--json] [--ui-only]
    ```

    Implementation calls the already-exported `McpClient::list_resources` (`client.rs:212`), which today has **zero callers**. Add `vtcode mcp tools --json` at the same time if it is cheap — same discovery path.

2. Optionally `vtcode mcp app <uri>` to `resources/read` and print a summary: MIME type, byte size, declared CSP domains, permissions, `prefersBorder`. Never execute or render it.
3. Update the `mcp` CLI docs table and `docs/guides/mcp-integration.md`.

**Acceptance:** against a live server, the command lists `ui://` resources with their declared CSP. This is also the answer to "is there a real app in the wild?" — the plan's own open question.

**Why this comes before rendering:** it costs an afternoon, has immediate utility, and tells you whether increment 2+ is worth building at all.

---

## Increment 2 — Capability advertisement (gated)

**Goal:** tell servers we can render, but only when we actually can.

1. Add config in `McpUiConfig` (`crates/codegen/vtcode-config/src/mcp.rs:237-265`):

    ```toml
    [mcp.ui]
    apps = "off"        # off | detect | render
    ```

    `off` (default) and `detect` do **not** advertise. Only `render` advertises, and `render` must be rejected at config load until increment 3 exists.

2. In `build_initialize_params` (`client.rs:983-1001`), when mode is `render`, insert into `ClientCapabilities.extensions`:

    ```json
    "io.modelcontextprotocol/ui": { "mimeTypes": ["text/html;profile=mcp-app"] }
    ```

    `rmcp 3.5.0` already models this (`ClientCapabilities.extensions: Option<ExtensionCapabilities>`, SEP-1724) — no dependency change.

3. Apply the same in the reconnect path (`provider.rs:584-588`) and pool path (`connection_pool.rs:375-378`), which currently pass `ClientCapabilities::default()` and would otherwise silently lose the extension.

**Acceptance:** with `apps = render` the initialize request carries the extension; with `off` the request is byte-identical to today. Assert on the serialized request, not on internal state.

**Hard rule:** never advertise speculatively. A server that sees the capability registers a UI tool and returns HTML we discard — strictly worse than not advertising.

---

## Increment 3 — Rendering, one surface only

**Prerequisite:** increments 0-2 shipped and increment 1 proved at least one real `ui://` server.

### 3a. Choice of surface

| Surface                 | Cost                                                                       | Verdict                                                                       |
| ----------------------- | -------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| TUI (ratatui)           | Impossible without a terminal image protocol and a DOM                     | **No.** A text approximation of an interactive app is worse than the fallback |
| Desktop (GPUI)          | Needs `wry`/WebKit — no webview dependency exists anywhere in the lockfile | Deferred; licensing, bundling, and sandbox review are a project of their own  |
| Browser (WebMCP bridge) | Reuses an existing authenticated, origin-bound channel                     | **Yes**, if any surface                                                       |

### 3b. If browser: the shape

Do **not** add app hosting to `vtcode-webmcp`. That crate's rules forbid file-serving routes and bind exact origins, which are the opposite invariants from hosting untrusted third-party HTML. Instead:

1. A separate static app shell (new directory beside `apps/webmcp`) hosts the sandbox proxy iframe on its own origin.
2. VT Code serves the `ui://` HTML over the existing authenticated bridge as a **new `BridgeRequest` variant** (e.g. `app.resource`), returning HTML plus declared CSP. The bridge stays file-serving-free because the payload comes from an MCP provider under allowlist control, not from the workspace.
3. The shell enforces the spec's deny-by-default CSP and the double-iframe sandbox. Implement `ui/initialize`, `ui/notifications/initialized`, `tool-input`, `tool-result`, `tool-cancelled`, `resource-teardown`, and proxy `tools/call` / `resources/read`.
4. Reuse the existing `ApprovalRequired` path for anything mutating. Do not invent a second approval mechanism.

### 3c. Proxying rules

- `visibility: ["app"]` tools: strip from the model-visible catalog at registration time; accept from the view only on the same server connection; always reject cross-server.
- Enforce the resource allowlist on view-initiated `resources/read`.
- Honor `ui.csp` as a ceiling only. Undeclared origins must be blocked, verified by test.

**Acceptance:** a real example server (`ext-apps/examples/qr-server` or `get-time`) renders and round-trips a tool call. Then a negative test: request an undeclared origin and confirm it is blocked.

---

## Increment 4 — Event feed and context (only if 3b ships)

- Feed live session events to the view through `event_hub.rs` (`VersionedThreadEvent`, already bounded).
- Implement `ui/update-model-context` with **last-write-wins per settled state**, debounced. It overwrites, so a naive implementation floods the context.
- Implement `ui/message` behind consent; it appends to the conversation.

---

## Sequencing summary

| #   | Increment                               | Depends on | Effort   | Reversible        |
| --- | --------------------------------------- | ---------- | -------- | ----------------- |
| 0   | `ui` metadata plumbing + `[app: …]` row | —          | ~1 day   | Yes               |
| 1   | `vtcode mcp resources` / `app <uri>`    | —          | ~0.5 day | Yes               |
| 2   | Gated capability advertisement          | 0          | ~0.5 day | Yes               |
| 3   | Browser rendering host                  | 1, 2       | weeks    | Yes, config-gated |
| 4   | Event feed, model context               | 3          | days     | Yes               |

Increments 0-2 are conservative and independently valuable even if nothing else follows. Increment 3 is the real commitment; do not start it before increment 1 confirms a real server exists.

---

## Assumptions to challenge

1. **"Users want interactive apps in a terminal IDE."** Unverified. The TUI cannot render HTML; the desktop app has no webview. The realistic audience is the browser integration only. If the answer is "no audience", the correct outcome is increment 0 + 1 and stop.
2. **"Advertising the extension is free."** It is not. Servers gate tool registration on it and may return tool variants with different descriptions and schemas, which changes the model-visible catalog.
3. **"`_meta` stripping blocks this work."** It does not. The stripping is result-level; tool-definition `_meta` is untouched by it. Increment 0 needs no policy reversal — verified against `crates/codegen/vtcode-mcp/AGENTS.md`.
4. **"WebMCP is the natural home."** Opposite trust model. It is a _channel_, not a _host_.
5. **"This is a security feature."** MCP Apps is a sandboxing _requirement_, not a security _gain_. Today we never render third-party HTML, so the current attack surface is the simplest possible one. Increment 3 enlarges it. That trade must be explicit before implementation.

## Non-goals

- Rendering apps in the TUI.
- Implementing an MCP Apps host inside `vtcode-webmcp`.
- Relaxing result-level `_meta` stripping.
- Bundling a webview engine into the desktop app before increment 3 is justified.
- Faithfully implementing `ui/request-display-mode`, `pip`, or `ui/open-link` before there is a surface that can honor them.
