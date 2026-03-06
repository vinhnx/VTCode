# Responses API & Reasoning Models

VT Code routes OpenAI Responses models, including the GPT-5 family plus `o3` and `o4-mini`, through the Responses API. This guide focuses on the parts that matter in VT Code: reasoning continuity across tool calls, cache-friendly request shaping, encrypted reasoning for stateless workflows, and the config needed to turn those features on.

## Key Concepts

| Concept                 | Description                                                                                                                                               |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Reasoning items**     | Internal chain-of-thought tokens exposed as IDs in the Responses API output. Reusing them keeps tool-enabled turns coherent and helps downstream caching. |
| **Reasoning summaries** | Short, user-visible explanations of what the model computed. VT Code requests summaries automatically for OpenAI Responses reasoning models and folds returned text into normal reasoning output. |
| **Encrypted reasoning** | A stateless, compliance-friendly variant where the API returns encrypted tokens that your sidecar can return verbatim without persisting data.            |

## VT Code configuration guidance

1. **Choose reasoning effort by task shape**: Set `reasoning_effort` inside `vtcode.toml` based on the task, not by defaulting to the highest setting. For execution-heavy and latency-sensitive work, `none` or `low` is usually enough; `medium` or `high` is better for research-heavy or conflict-resolution work; `xhigh` should stay reserved for long-horizon agentic tasks where evals justify the extra cost and latency.

    ```toml
    reasoning_effort = "none"
    ```

2. **Surface reasoning summaries**: VT Code automatically requests `reasoning.summary = "auto"` for OpenAI reasoning models. Returned summary text is folded into the agent’s normal reasoning output and logs, so no extra toggle is required.

3. **Preserve reasoning items across API calls**: VT Code keeps continuity in two ways. It stores `previous_response_id` for OpenAI/OpenResponses sessions, and it also preserves structured reasoning items in assistant `reasoning_details` so tool loops can replay them when the next request is built. That matches OpenAI’s guidance to pass `previous_response_id` or reinsert reasoning items explicitly.

4. **Use hybrid continuity + server-side compaction**: VT Code keeps Responses-style continuity (`previous_response_id`) for OpenAI/OpenResponses providers and enables compaction via `context_management` on `/responses` requests when `agent.harness.auto_compaction_enabled = true`.

5. **Use encrypted reasoning for ZDR-style compliance**: If you are restricted from storing model state, enable the Responses API flags directly in `vtcode.toml`:

    ```toml
    [provider.openai]
    responses_store = false
    responses_include = ["reasoning.encrypted_content"]
    ```

    The Responses API will return encrypted reasoning state inside each reasoning item, and VT Code will pass that state back on the next OpenAI Responses request. No raw reasoning needs to be persisted locally to preserve continuity.

6. **Cache-friendly prompts and continuity**: The Responses API differentiates cached and uncached tokens. Longer prompts (>= 1,024 tokens) benefit from returning everything, including reasoning items, so the cache can match on both the request and internal context. Higher cache hit ratios reduce costs and latency for GPT-5-family models, especially during long-running agent loops.

    Tip: VT Code sends a stable OpenAI routing key per conversation by default via `prompt_cache_key_mode = "session"` under `[prompt_cache.providers.openai]`. Keep this at `session` for better cache locality; set `off` only when you explicitly want to disable key-based routing. You can also instruct the Responses API to retain cached prefixes for longer by setting `prompt_cache_retention` on the request. VT Code exposes this setting as `# prompt_cache_retention = "24h"` (commented out by default). Using a longer retention can reduce costs and latency for frequently repeated prompts in GPT-5.4 and related Responses models. The value must be in the format `<number>[s|m|h|d]` (e.g., `24h`) and is restricted to a minimum of `1s` and a maximum of `30d`.

    Example: Enable 24h retention using CLI config overrides for a Responses model:

    ```bash
    vtcode --model gpt-5 --config prompt_cache.providers.openai.prompt_cache_retention=24h ask "Explain this function"
    ```

    To list the models known to support the OpenAI Responses API, run:

    ```bash
    vtcode models list --provider openai
    ```

7. **Function calling etiquette**: Ensure any VT Code tool definitions expose their JSON schema via the `function` payload. The Responses API requires each tool message to include a `tool_call_id`, and VT Code already handles this when serializing `ToolDefinition`s.

8. **Reasoning visibility**: When troubleshooting, inspect `.vtcode/logs/trajectory.jsonl` for `reasoning` entries and correlate them with the configured `reasoning_effort`.

9. **Auto-compaction settings**: Auto compaction is disabled by default. Turn it on explicitly when you want long-session coherence via Responses `context_management`:

    ```toml
    [agent.harness]
    auto_compaction_enabled = true
    # Optional explicit threshold; if omitted VT Code uses ~90% of model context.
    auto_compaction_threshold_tokens = 200000
    ```

    VT Code applies this only on compatible Responses providers/endpoints.

## Example workflow

1. VT Code sends a Responses API request for an OpenAI reasoning model with tools serialized through the shared helper.
2. The response includes tool call instructions plus a reasoning item. VT Code records the response id and preserves any structured reasoning items in message history.
3. Tool outputs are emitted as `function_call_output` messages with the original `tool_call_id`, then the next request is issued with the preserved continuity state.
4. If encrypted reasoning is enabled, the returned `encrypted_content` is replayed automatically on the next OpenAI Responses request.

## Taking it further

-   Combine reasoning summary output with VT Code’s status line badge and telemetry for interactive tracing.
-   Use reasoning effort tiers together with the status line’s `runtime.reasoning_effort` so your shell hook can show when the agent is "thinking" harder.
-   Keep `.vtcode/logs/trajectory.jsonl` for post-run analysis and to debug why a tool call required an extra turn.

Following these practices keeps VT Code aligned with current OpenAI Responses guidance, delivering better continuity, lower-cost cached prompts, and stronger long-horizon agent behavior.
