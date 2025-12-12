# Using GPT-5.2

## Summary

-   Model ID: `gpt-5.2` (alias: `gpt-5.2-2025-12-11`).
-   Best general-purpose OpenAI model; upgrades over GPT-5.1 in reasoning, instruction following, vision, code generation, tool calling, and context management (compaction).
-   Supports Responses API features: reasoning effort (adds `xhigh`), verbosity, prompt caching/compaction, allowed tools, custom tools, and first-person tool preambles.

## Model lineup

-   `gpt-5.2` (primary id; legacy alias `gpt-5.2-2025-12-11`): complex reasoning, broad knowledge, code-heavy or multi-step agent tasks.
-   `gpt-5.2-pro`: higher compute for harder problems, more consistent depth.
-   `gpt-5.2-chat-latest`: ChatGPT-facing chat default.
-   `gpt-5.1-codex-max`: coding-optimized variant with `xhigh` reasoning and compaction; keep for coding-only workloads.
-   `gpt-5-mini` / `gpt-5-nano`: cost/speed-optimized options.

## What's new vs GPT-5.1

-   New `xhigh` reasoning effort level and concise reasoning summaries.
-   Built-in compaction for better long-running and tool-heavy flows.
-   Default reasoning effort is `none` (lower latency); increase gradually to `medium`/`high`/`xhigh` when needed.
-   Verbosity still `low | medium | high` (defaults to `medium`); lower for terse outputs.

## Parameters and compatibility

-   Reasoning effort: `none | low | medium | high | xhigh` (default `none`).
-   Verbosity: `text.verbosity` = `low | medium | high` (default `medium`).
-   Temperature/top_p/logprobs are supported only when `reasoning.effort = none` (OpenAI API rule).
-   Works best on the Responses API (pass previous CoT via `previous_response_id` for tool chains).

## Tooling highlights

-   `apply_patch` tool: structured diffs for multi-step edits.
-   Shell tool supported.
-   Custom tools: `type: custom` for freeform inputs; can attach CFG grammars to constrain outputs.
-   Allowed tools: restrict active tools via `tool_choice.allowed_tools`.
-   Preambles: have the model restate the goal in first person with one short action-first line (verb + target + tool), outline the steps, and narrate progress without using a "Preamble:" prefix for better traceability.

## Quick Requests API examples

Minimal reasoning (default `none`) with the Responses API:

```bash
curl --request POST https://api.openai.com/v1/responses \
  --header "Authorization: Bearer $OPENAI_API_KEY" \
  --header "Content-type: application/json" \
  --data '{
    "model": "gpt-5.2",
    "input": "Summarize the design constraints for the VTCode agent.",
    "reasoning": { "effort": "none" },
    "text": { "verbosity": "medium" }
  }'
```

Custom tool with allowed tools:

```json
{
    "model": "gpt-5.2",
    "input": "Use code_exec to compute the median of [1,2,3,10].",
    "tools": [
        {
            "type": "custom",
            "name": "code_exec",
            "description": "Executes arbitrary python code"
        }
    ],
    "tool_choice": {
        "type": "allowed_tools",
        "mode": "auto",
        "tools": [{ "type": "function", "name": "code_exec" }]
    }
}
```

## Migration notes

-   Drop-in replacement for `gpt-5.1` in most cases.
-   Use Responses API to carry CoT across turns; improves latency and cache hits.
-   For o3 → GPT-5.2: start with `reasoning.effort=medium`, raise to `high/xhigh` if needed.
-   For GPT-4.1 → GPT-5.2: start at `reasoning.effort=none`, then increase selectively.
-   Mini/nano replacements: use `gpt-5-mini` or `gpt-5-nano` with prompt tuning.

## Prompting tips

-   Keep verbosity at `medium` or `high` for rich code output; use `low` for concise SQL/snippets.
-   With `reasoning.effort=none`, explicitly ask the model to outline steps before answering when you need depth.
-   Enable first-person preambles (goal → steps → progress, no prefix) when chaining tools to surface intent between calls.

## References

-   GPT-5.2 prompting guide: https://cookbook.openai.com/examples/gpt-5/gpt-5-2_prompting_guide
-   GPT-5 family new features: https://cookbook.openai.com/examples/gpt-5/gpt-5_new_params_and_tools
-   Responses vs Chat Completions: https://platform.openai.com/docs/guides/migrate-to-responses
