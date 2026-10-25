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
