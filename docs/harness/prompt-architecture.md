# Prompt Architecture

## Cache-stable segments

Each harness segment freezes one `SessionRequestEnvelope`: a segment ID, the system prompt, a canonically ordered tool
catalog, and the instruction digest. Ordinary turns only append messages. Compaction or a change to the catalog epoch,
primary agent, model, provider, mode, or instruction digest starts a new segment and therefore one intentional cache
miss.

Session-start time is rendered once into the cached system prompt. Later time must come from tool output. Approval and
sandbox policy values are execution gateway state and must not be rendered into prompts or tool descriptions.

VT Code's prompt system follows the four pillars of _Section 18.3 (Prompt Architecture)_ of the agentic-AI guide:

1. **System-prompt design** (18.3.1) — persona, capabilities, constraints, universal runtime guidance, and output format
   assembled once per segment.
2. **Dynamic assembly** (18.3.2) — modular, cache-friendly composition at segment creation from the base prompt,
   instruction appendix, runtime contract, recovery mode, harness limits, tool catalog, and primary-agent state. Later
   runtime changes are appended as context messages.
3. **Few-shot management** (18.3.3) — keyword-tagged examples loaded from disk, token-budgeted via
   `vtcode_commons::tokens::estimate_tokens`, and appended as a synthetic system context message when relevant; they do
   not rewrite the immutable prompt prefix.
4. **Tool descriptions** (18.3.4) — every LLM-visible tool must include when-to-use guidance, when-NOT-to-use guidance,
   and a constraints cue (rate-limit, max size, side-effect, permission). The
   `tool_descriptions_satisfy_documented_contract` test in
   `crates/codegen/vtcode-core/src/tools/registry/builtins.rs::tests` enforces this contract at `cargo test` time.

## Assembly order

At segment creation, `request_builder::build_prompt_output` produces the immutable `system_prompt` in this order:

1. **Base system prompt** — `vtcode_core::prompts::system::generate_system_instruction` (with
   `Default / Minimal / Lightweight / Specialized` variants cached via `OnceLock`). Each cached profile includes the
   deterministic compiled runtime-guidance section from `prompts/runtime_guidance.rs` exactly once.
2. **INSTRUCTIONS** appendix — `AGENTS.md`, `CLAUDE.md`, and other project-scoped instruction files discovered by
   `project_doc::build_instruction_appendix_with_context`.
3. **Runtime mode contract** — full-auto / planning / request_user_input flags
   (`runtime_contract::append_runtime_mode_sections`).
4. **Auto-permission notice** — only when `auto_permission && !planning`.
5. **Active Primary Agent Skills** — when a primary agent is active.
6. **Harness Limits** — `harness_limits::upsert_harness_limits_section`.
7. **Recovery Mode** — not a system-prompt section: when `tool_free_recovery` is active, the recovery contract rides as
   a request-only tail-position turn-scoped system message (`recovery_mode_directive`, pushed in `build_turn_request`,
   never persisted to canonical history), keeping the system prompt and tool catalog byte-identical to tool-enabled
   turns for prefix-cache reuse (`tool_choice: none` prevents calls).
8. **Runtime Tool Catalog** — `append_runtime_tool_prompt_sections_for_profile` / `_for_model` with the
   planning/capability filtered `SessionToolCatalogSnapshot`.
9. **GitHub Copilot Client Tools** — only for the Copilot provider.
10. **Active Primary Agent Runtime State** — model, reasoning effort, instructions, and `### Memory Appendix` if the
    agent has memory.
11. **[Few-Shot Examples]** — never part of the system prompt. When relevant and budget allows, the block is persisted
    in history once per user turn, directly after the user message (see below).

## Prompt style

The compiled base prompt (`prompts/system.rs`, `prompts/runtime_guidance.rs`, `prompts/guidelines.rs`) is written to
work unchanged on every provider, including small local models.

- **Plain prose with reasons.** State each rule as a full sentence and give the reason when it is not obvious ("confirm
  destructive actions ... since lost work may be unrecoverable"). Do not use capitalized emphasis such as MUST, NEVER,
  or CRITICAL; the runtime-guidance test rejects it.
- **One home per rule.** Universal rules (scope, grounding, verification honesty, delegation, safety, tool-failure
  recovery, waiting on `next_wait_args` instead of polling, `spool_path` paging and `preview_budget_exhausted` handling,
  progress updates) live only in `RUNTIME_GUIDANCE_SECTION`, which every profile includes and which is re-added when a
  workspace `system.md` replaces the base. State the harness carries across compaction lives in `SHARED_CONTRACT_LINES`.
  Extended style for the Default, Lightweight, and Specialized profiles lives in `DEFAULT_SPECIFIC_LINES`. Operating
  deltas describe only mode mechanics (core tools, Planning workflow, tracker). Per-tool mechanics, including the single
  `start_planning` line, live in the `## Active Tools` section from `prompts/guidelines.rs`, and `[Harness Limits]`
  states limit values, their exemptions, and how to work within each limit; neither restates a universal rule.
  `static_prompts::tests::shared_contract_lines_and_runtime_bullets_appear_exactly_once_per_profile` asserts that every
  `SHARED_CONTRACT_LINES` entry, every profile-specific contract line, and every Runtime Guidance bullet appears exactly
  once in each profile, and `guidelines::tests::universal_rules_have_one_home_across_composed_prompt_sections` composes
  each profile with the Default, Minimal, and Planning tool guidance plus Harness Limits and asserts each universal rule
  appears exactly once.
- **Provider-agnostic.** No model or provider names, no references to provider-specific features such as thinking blocks
  or context-clearing parameters, and no formatting that depends on one vendor's renderer.
- **Budgets with justification.** Each profile and section has a token or character bound in its tests. When a bound is
  raised, the constant or assertion carries a one-line comment saying why and what it measured. Current base sizes
  (cl100k estimate): Minimal about 525, Lightweight about 730, Default about 860, Specialized about 875, Runtime
  Guidance about 440.

## Few-shot management (Section 18.3.3)

### Authoring examples

Examples live as Markdown files under:

- `<workspace>/.vtcode/prompts/examples/*.md`
- `<canonical user config directory>/prompts/examples/*.md` (legacy user prompt files are migrated there)

The filename stem is the example id. The file body uses YAML frontmatter for metadata:

```markdown
---
id: read-then-edit-large-file
tags: [read, edit, patch, large-file, refactor]
summary: Inspect a large file in targeted shell slices before editing; use apply_patch for multi-hunk changes.
---
# User
<the user query>

# Assistant
<the expected tool sequence and rationale>
```

`tags` and `summary` are optional. `tags` drive the keyword selector; `summary` is appended to the prompt as a one-line
caption above the body. Workspace examples take precedence over user-global examples on id collision.

### Selection

For each turn, the harness:

1. Reads the most recent user message and uses it as the selection query.
2. Loads the example store from disk via `FewShotStore::load`.
3. Scores each example: +1.0 per exact tag/word match, +0.5 per tag that appears as a substring of the query.
4. Sorts by score desc, then id asc (stable ordering).
5. Walks in order, appending until the running total exceeds [`DEFAULT_FEW_SHOT_BUDGET_TOKENS`] (default 800 tokens,
   ~10% of an 8K context window).
6. Renders the chosen examples as a `[Few-Shot Examples]` block and, at the first request of the turn only, persists it
   in canonical history directly after the user message (`llm_request/request_context.rs`). Every later request of the
   turn, and every later turn, replays it unchanged at that position, so requests stay append-only for prompt caching
   and for models that bind replayed thinking to the exact prior prefix (Claude Sonnet 5.5, Claude Opus 5.5, Claude
   Fable 5.1).
7. Shapes the persisted block per route: routes with turn-scoped system messages send it as `role: "system"` with
   `clear_at: "next_user_message"`, so it stops applying once the next user turn arrives; other routes receive it as a
   user-role context message, because their adapters fold mid-history system messages into the top-level system prompt
   and would rewrite the cached system prefix whenever the selection changes. Earlier turns' blocks stay in history
   (bounded by the per-turn budget) until compaction removes them.

The selection is keyword-based and runs in-process without an embedding provider. Embedding-based selection is the
documented next step (see Section 18.3.3); layering it on top will not require API changes.

### Guard rails

- **Recovery mode skips few-shot.** When `tool_free_recovery` is active the model is in "summarize from evidence" mode
  and adding examples would distract. The few-shot block is omitted.
- **Empty stores are silent.** If no examples are present, no `[Few-Shot Examples]` block is added — no overhead for
  users who don't ship examples.
- **Token count is honest.** `FewShotExample::token_count` is computed via the `cl100k_base` BPE tokenizer in
  `vtcode_commons::tokens::estimate_tokens`. Budget enforcement uses the same tokenizer, so the budget is accurate for
  OpenAI models and within ~10% for Anthropic / Gemini.

### Adding a new example

1. Choose a stable, hyphenated id (used in logs and prompts).
2. Author the `.md` file with frontmatter and body.
3. Run `cargo test -p vtcode-core prompts::few_shot::tests` to confirm the example loads and selects correctly.
4. Optional: add a `FewShotStore::from_examples(...)` unit test that asserts the example is selected by a representative
   query.

## Tool description contract (Section 18.3.4)

Every LLM-visible tool whose description is not in the allowlist must include:

- A **verb cue** — `Use`, `Create`, `List`, `Fetch`, etc. — so the model recognizes the action the tool performs.
- A **constraint cue** — e.g. `max ...`, `rate-limit`, `session`, `timeout`, `requires approval`, `inherits` — so the
  model knows the limits and side effects. Prohibition phrasing such as `Do NOT ...` or `Avoid ...` does not satisfy the
  rule; models that follow tool descriptions literally over-apply it, so state the concrete limit instead.

Descriptions must be 40-1500 characters. Tools exempted from the constraint requirement are single-action or read-only
helpers (`request_user_input`, `search_tools`, `code_search`, etc.) where the model can safely call them without
explicit guard-rails. The allowlist holds registration names only; aliases such as `cron_list` are never checked. See
the test for the full allowlist and cue vocabulary.

Run `cargo test -p vtcode-core tools::registry::builtins::tests::tool_descriptions_satisfy_documented_contract` to
validate any description change before merging.

[`DEFAULT_FEW_SHOT_BUDGET_TOKENS`]: ../crates/codegen/vtcode-core/src/prompts/few_shot.rs
