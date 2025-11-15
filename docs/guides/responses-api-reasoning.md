# Responses API & Reasoning Models

VT Code already routes GPT-5.1 (Codex) and other reasoning-focused models through OpenAI’s Responses API. This guide explains how to unlock the performance and transparency benefits described in the OpenAI recipe for reasoning models and to keep VT Code’s agent workflows aligned with those best practices.

## Key Concepts

| Concept                 | Description                                                                                                                                               |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Reasoning items**     | Internal chain-of-thought tokens exposed as IDs in the Responses API output. Reusing them keeps tool-enabled turns coherent and helps downstream caching. |
| **Reasoning summaries** | Short, user-visible explanations of what the model computed. VT Code surfaces them when `reasoning_summary` is enabled so you can audit agent decisions.  |
| **Encrypted reasoning** | A stateless, compliance-friendly variant where the API returns encrypted tokens that your sidecar can return verbatim without persisting data.            |

## VT Code configuration guidance

1. **Activate reasoning effort**: Set `reasoning_effort` inside `vtcode.toml` to `medium` or `high` when running more complex tasks (`minimal`/`low` are also accepted). Higher effort levels instruct GPT-5.1 and similar models to spend more tokens on thinking, tooling, and structured reasoning.

    ```toml
    reasoning_effort = "medium"
    ```

2. **Surface reasoning summaries**: VT Code exposes the `reasoning_summary` payload (if available) in structured logs and the status line. Ensure `runtime.reasoning_effort` is populated so the frontend can render reasoning-context cues.

3. **Preserve reasoning items across API calls**: When VT Code issues function calls, the Responses API automatically keeps the `output` payload (which includes reasoning items), and we append it back into the context before reissuing the request. This mirrors the guidance to pass `previous_response_id` or to reinsert reasoning components so that subsequent calls continue where the model left off.

4. **Use encrypted reasoning for ZDR-style compliance**: If you are restricted from storing model state, add the following flags when calling OpenAI via `vtcode.toml` overrides or CLI hacks (managed through `[router]` overrides if necessary):

    ```json
    "include": ["reasoning.encrypted_content"],
    "store": false
    ```

    The Responses API will return an encrypted chain of thought inside each reasoning item, and VT Code will replay that token whenever the next turn is dispatched. No reasoning data is stored on disk, yet each step still benefits from the prior reasoning trace.

5. **Cache-friendly prompts**: The Responses API differentiates cached and uncached tokens. Longer prompts (>= 1,024 tokens) benefit from returning everything—including reasoning items—so the cache can match on both the request and internal context. Higher cache hit ratios reduce costs and latency for `o4-mini`, `o3`, and GPT-5-series models.

    Tip: You can instruct the Responses API to retain cached prefixes for longer by setting `prompt_cache_retention` on the request. VT Code exposes this setting in `vtcode.toml` under `[prompt_cache.providers.openai]` as `# prompt_cache_retention = "24h"` (commented out by default). Using a longer retention can reduce costs and latency for frequently repeated prompts in GPT-5.1 if set. The value must be in the format `<number>[s|m|h|d]` (e.g., `24h`) and is restricted to a minimum of `1s` and a maximum of `30d`.

    Example: Enable 24h retention using CLI config overrides for a Responses model:

    ```bash
    vtcode --model gpt-5 --config prompt_cache.providers.openai.prompt_cache_retention=24h ask "Explain this function"
    ```

    To list the models known to support the OpenAI Responses API, run:

    ```bash
    vtcode models list --provider openai
    ```

6. **Function calling etiquette**: Ensure any VT Code tool definitions expose their JSON schema via the `function` payload. The Responses API requires each tool message to include a `tool_call_id`, and VT Code already handles this when serializing `ToolDefinition`s. Reinjecting reasoning summaries into `context` keeps every tool loop consistent with the `responses` guidance.

7. **Reasoning visibility**: When troubleshooting, inspect `.vtcode/logs/trajectory.jsonl` for `reasoning` or `reasoning_summary` entries. The agent’s telemetry also logs `reasoning_effort` (see the inline status line guide) so you can correlate agent decisions with expectation-aligned reasoning levels.

## Example workflow

1. VT Code sends a Responses API request with `reasoning_effort = "medium"` and `tools` serialized through the shared helper.
2. The response includes tool call instructions plus a reasoning item (e.g., `rs_6820f...`). VT Code appends the entire payload back into the context so the next call reuses the reasoning ID.
3. If you requested encrypted reasoning, the response contains `encrypted_content`; VT Code feeds this string into the next request’s `input` block exactly as received.
4. Final completion is returned along with `reasoning_summary` text that can be surfaced to the end user for transparency.

## Taking it further

-   Combine reasoning summary output with VT Code’s status line badge and telemetry for interactive tracing.
-   Use reasoning effort tiers together with the status line’s `runtime.reasoning_effort` so your shell hook can show when the agent is "thinking" harder.
-   Keep `.vtcode/logs/trajectory.jsonl` for post-run analysis and to debug why a tool call required an extra turn.

Following these practices keeps VT Code aligned with OpenAI’s latest reasoning model guidance, delivering better intelligence, lower-cost cached prompts, and compliant encrypted workflows.
