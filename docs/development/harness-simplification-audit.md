# Harness simplification audit

This ledger is an initial inventory, not a completed harness audit. Measurements
below are deterministic offline fixtures, not provider token billing or evidence
of model behavior. Do not remove a tool or safety boundary based on this ledger
without tracing its callers and testing its replacement.

## Prompt baseline

Measured with `cargo nextest run --locked -p vtcode-core -E
'test(built_in_composed_prompt_budget_without_workspace_additions) | test(planning_prompt_size_fixed_fixture)' --no-capture`.
The fixtures supply empty user and workspace prompt layers directly. The
profile fixture uses Unix shell guidance, no skills or tool context, and
disables date and working-directory additions. The planning fixture uses a
fixed synthetic catalog and includes a working-directory value.
Token counts use the existing `vtcode_commons::estimate_tokens` estimator.

| Fixture | Lines | Estimated tokens |
| --- | ---: | ---: |
| Default | 48 | 881 |
| Minimal | 38 | 604 |
| Lightweight | 44 | 775 |
| Specialized | 48 | 877 |
| Planning, default density | 64 | 1,857 |
| Planning, minimal density | 65 | 1,949 |

The profile fixture now guards 90 lines and 1,000 estimated tokens; the planning
fixture guards 90 lines and its existing independent token limits. These guards
do not cover all combinations or enforce a limit on provider-specific and
per-turn additions. Workspace system overrides, skill metadata, environment
values, real tool schemas, and user instructions need separate measurement.

`runtime_prompt_budget_with_registry_catalog` additionally composes all four
profiles with the offline registry's progressive catalog, both shell profiles,
execution and planning state, and runtime catalog metadata. It supplies empty
prompt layers and no workspace additions. The execution cases range from 52 to
63 lines and 882 to 1,258 estimated tokens; planning ranges from 61 to 72 lines
and 1,798 to 2,131 estimated tokens. The fixture guards 90 lines and 2,300
estimated tokens. It does not include later per-turn additions or schemas.

## Offline tool catalog baseline

`cargo nextest run --locked -p vtcode-core -E
'test(model_tool_catalog_baseline_by_mode)' --no-capture` measures an offline
`ToolRegistry::new_for_schema` with the interactive surface, CodeSearch
capability, default model capabilities, no controller or matrix access, no MCP,
and the `full_public` default (disabled deferred-loading policy). The planning
flag changes catalog visibility. Sizes below are JSON byte lengths of registry
schema entries and generic `ToolDefinition` values, respectively; provider
formatting, runtime catalog shaping, and transport overhead are not included.

The token estimate below sums each serialized entry's byte length divided by
four, matching the existing schema-budget test's heuristic. It is not a
provider tokenizer or a provider-wire measurement.

| Documentation | Planning | Entries | Schema bytes | Estimated tokens | Model bytes |
| --- | --- | ---: | ---: | ---: | ---: |
| Minimal | No | 5 | 3,059 | 761 | 3,214 |
| Minimal | Yes | 4 | 3,059 | 761 | 3,183 |
| Progressive | No | 5 | 9,903 | 2,472 | 10,058 |
| Progressive | Yes | 4 | 8,021 | 2,002 | 8,145 |
| Full | No | 5 | 9,903 | 2,472 | 10,058 |
| Full | Yes | 4 | 8,021 | 2,002 | 8,145 |

Normal mode exposes `apply_patch`, `exec_command`, `write_stdin`, `search_tools`,
and `record_decision`; planning exposes `exec_command`, `code_search`,
`record_decision`, and `request_user_input`. In the progressive normal fixture,
`exec_command` is the largest entry at 3,546 bytes, followed by `write_stdin`
at 2,148 and `apply_patch` at 2,014. The full and progressive projections match
in size for this fixture, not necessarily for MCP tools or other catalogs.
The stale numeric estimate in `ToolDocumentationMode::Progressive` was removed;
the existing schema-budget test remains the source for its current limit.

The same fixture formats the generated definitions through the public provider
formatter helpers. Values are bytes for the formatted tool JSON only; they do
not include the request envelope, cache markers, model-specific route shaping,
client-local wire filtering, or tokenization by a provider. The Chat formatter
uses the shared OpenAI-compatible helper, not proof of every OpenAI Chat route.

| Documentation | Planning | OpenAI Chat | OpenAI Responses | Anthropic | Gemini |
| --- | --- | ---: | ---: | ---: | ---: |
| Minimal | No | 2,798 | 2,808 | 2,877 | 2,572 |
| Minimal | Yes | 3,031 | 3,039 | 3,067 | 2,737 |
| Progressive | No | 9,642 | 9,652 | 9,721 | 9,416 |
| Progressive | Yes | 7,993 | 8,001 | 8,029 | 7,699 |
| Full | No | 9,642 | 9,652 | 9,721 | 9,416 |
| Full | Yes | 7,993 | 8,001 | 8,029 | 7,699 |

## Full-request census

`cargo nextest run --locked -p vtcode -E 'test(first_request_payload_census)'
--no-capture` builds a representative first turn through `build_turn_request`
(Minimal-mode composed base, three stub tool definitions, one user message,
no skills/MCP/deferral) and pins the emitted first-call composition:

| Part | Bytes | Tokens |
| --- | ---: | ---: |
| System prompt | 5,583 | 1,396 |
| Tool schemas (3 on wire) | 476 | 120 |
| Message history | — | 5 |
| Fixed overhead (system + tools) | — | 1,516 |

The test guards ceilings (system 3,000, tools 300, history 50, overhead 3,500)
as regression tripwires, not budgets. Per-provider formatted tool bytes are
additionally bounded to 0.5–2x of the internal model-tool JSON in
`model_tool_catalog_baseline_by_mode`, so context-pressure accounting (which
measures pre-format definitions) cannot silently diverge from wire reality.

## Ownership and findings

| Area | Evidence and impact | Status |
| --- | --- | --- |
| Static profiles | `prompts/static_prompts.rs` composes shared runtime guidance, contract lines, and mode deltas; cached prefixes are shared. | Baseline measured; no text removed. |
| System composition | `prompts/system/compose.rs` applies workspace layers and adds shell, skills, tool, and environment sections; its existing token budget applies to the combined sections. | `SystemPromptReport` now carries `layer_token_estimate` (estimator run on the exact applied layer text) alongside the combined total; over-budget warnings and logs report both. Layers stay untrimmable and sections/digest behavior is unchanged. Boundary: prompt-file bodies count as layers; rendered skill metadata stays built-in. |
| Turn assembly | `src/agent/runloop/unified/turn/turn_processing/llm_request/prompt_assembly.rs` adds runtime state and validates catalog alignment. Recovery uses a request-only directive to retain the system prefix. | Full-request census baselined above; preserve cache and alignment contracts. |
| Prompt section ownership | Runtime assembly previously removed `## Active Tools`, `[Harness Limits]`, and `[Deferred Tools]` by heading after composing workspace layers, so user-authored sections could be removed. The catalog-metadata guard additionally parsed the whole prompt, so a user-authored `[Runtime Tool Catalog]` block was validated as runtime attestation whenever the runtime appended none (cache-shaping mode, tool-incapable providers). | Interactive and headless runtime assembly now appends fresh owned sections without removing the existing prefix, and the metadata guard parses only the runtime-owned suffix (same boundary fresh assembly uses for removal). Older whole-string rebuild APIs retain their compatibility behavior with a zero boundary. First match wins within the suffix, so forged markers in later deferred content cannot shadow the runtime block. Residual note: the guard is warn-and-continue (`rebuild_once_on_alignment_mismatch` always returns `Ok`), so a foreign block cost one wasted rebuild per turn, never a failed turn; every production mismatch fire was a false positive because runtime metadata is written from the compared snapshot. |
| Tool registration | `tools/registry/pack.rs` and `pack_impls.rs` register shipped builtins (8 packs: hitl, planning, multi_agent, search, web, shell, internal_pty, editing); `builtins/tests.rs` reads those registrations. Aliases can remain execution compatibility paths even when absent from schemas. | Per-surface inventory pinned by `model_surface_inventory_by_surface` (Interactive/AgentRunner: 5 normal, 4 planning; ACP: 3 normal, 2 planning; `grep_file`/`read_file`/`list_files` absent everywhere). No registered model-facing tool qualifies for removal: the Interactive surface is already minimal. `grep_file` (unregistered phantom name) removed from readonly agent specs and stale docs; `GrepSearchManager` backend kept (live `code_search` dependency). Residual: `exec_pty_cmd`/`run_pty_cmd` duplicate hidden routes, `ToolPresence::grep_file` dead flag, alias sprawl, and the text-channel grep dead end need separate tracing. |
| Wire accounting | `llm_request/metrics.rs` estimates schema tokens from serialized internal `ToolDefinition` values before provider formatting; `request_builder.rs` records the result for context accounting. Provider formatters produce different JSON. | Corrected comments that called this an exact wire measurement; representative requests now measured (see census), formatted bytes bounded to 0.5–2x of internal JSON; MCP/deferred/controller configurations remain. |
| Routing, context, execution | Public route lookup resolves aliases before dispatch; structured-request retry and fetch-budget classification previously compared the unresolved name. An alias such as `fetch_url` could reach the fetch handler without its shared origin budget or two-attempt limit. | Classify those policies through the existing public route lookup while retaining the requested name for safety and errors. Dispatch-level budget and retry regressions cover canonical, alias, and namespaced forms. History and cancellation remain to audit. |
| Request history cleanup | `context_manager.rs` checked only concatenated text to detect empty messages. Image-only and file-only user messages therefore disappeared from the provider-facing request, even though durable history remained intact. | Inspect content parts and retain non-text attachments. A regression covers images, files, blank text, ordering, and preservation of source history. |
| Decision guidance | `prompts/guidelines/mod.rs` overlaps the `record_decision` description in `tools/registry/pack_impls.rs`. The tool is not in the always-eager set in `session_tool_catalog.rs`; hosted search may defer it, while minimal descriptions omit the ordinary-command qualification. | Retained pending evidence that discovery and short descriptions preserve the guidance's effect. `decision_tool_hosted_deferral_and_minimal_description` guards these boundaries, including direct exposure for the small client-local fixture. |

Model-surface inventory is complete: no registered model-facing tool qualifies
for removal under the current evidence, so no migration is proposed. The phantom
`grep_file` name was dropped from agent specs and user-facing docs per the earlier
simplification decision; its history-compat match arms stay. Before any future
removal, trace integrations and callers and assess sandbox, approval, history,
and cancellation behavior. Compare after measurements against this baseline.
