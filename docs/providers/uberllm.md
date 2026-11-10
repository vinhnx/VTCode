# UberLLM Integration Guide

[UberLLM](https://uberllm.dev) is a hosted gateway that serves many chat models (DeepSeek, Qwen, GLM, Kimi, Claude, GPT
and others) behind one API key. The same key works for the OpenAI Chat Completions, OpenAI Responses, and Anthropic
Messages APIs. VT Code can use it through the existing `[[custom_providers]]` support without a dedicated runtime
provider.

## Prerequisites

1. Create an API key at [uberllm.dev/signup](https://uberllm.dev/signup). Usage is prepaid credit, billed per request.
2. Export the key:

   ```bash
   export UBERLLM_API_KEY="..."
   ```

## Configuration

Custom providers are trusted configuration, so VT Code rejects them in a workspace `vtcode.toml`. Add the entry to your
user configuration (`~/.vtcode/vtcode.toml`, or `$VTCODE_HOME/vtcode.toml` when `VTCODE_HOME` is set, or the platform
config directory) or pass a file with `--config path/to/file.toml`. See
[Configuration precedence](../config/CONFIGURATION_PRECEDENCE.md).

```toml
[agent]
provider = "uberllm"
default_model = "deepseek/deepseek-v4.1-flash"

[[custom_providers]]
name = "uberllm"
display_name = "UberLLM"
base_url = "https://api.uberllm.dev/v1"
api_key_env = "UBERLLM_API_KEY"
model = "deepseek/deepseek-v4.1-flash"
models = ["deepseek/deepseek-v4.1-flash", "qwen/qwen3.7-plus", "z-ai/glm-5.1", "moonshotai/kimi-k3"]
context_window = 200000 # Optional; default is 128000 tokens
```

Then run VT Code normally:

```bash
vtcode ask "Summarize this repository"
```

`base_url` must include `/v1`. VT Code appends `/chat/completions`, `/responses`, or `/messages` to that value.

## Model Selection

Model IDs look like `<vendor>/<model>`. The catalog is public, so you can list the IDs, context lengths, and prices
without a key:

```bash
curl https://api.uberllm.dev/v1/models
```

Copy an exact ID from that response, then update both `agent.default_model` and `custom_providers.model`, and add the
ID to `models` to list it in `/model`. VT Code does not import `/v1/models` for custom providers, so the picker shows
only the IDs you list.

Each catalog entry reports `context_length`. Set `context_window` to the smallest value among the models you list, or
set it per model in a profile:

```toml
[custom_providers.profiles."deepseek/deepseek-v4.1-flash"]
context_window = 1048576
```

## Claude Models over Anthropic Messages

Claude models can use the Anthropic Messages API on the same `base_url` and key. Add a profile per model:

```toml
[custom_providers.profiles."anthropic/claude-sonnet-4.6"]
api_format = "anthropic-messages"
context_window = 1000000
```

## Protocol Behavior

- VT Code sends the key as `Authorization: Bearer`.
- Unknown model IDs use OpenAI Chat Completions. Models recognized by VT Code as Responses-capable use `/responses`,
  which UberLLM also serves.
- Streaming and function tools use VT Code's shared OpenAI-compatible paths.
- Failed requests are not billed. Check the remaining credit with `GET https://api.uberllm.dev/v1/balance` and the same
  bearer key.
- Request timeouts use VT Code's global `[timeouts]` configuration.

## Troubleshooting

| Symptom                            | Resolution                                                                                        |
| ---------------------------------- | ------------------------------------------------------------------------------------------------- |
| `uberllm` is missing from `/model` | Confirm the `[[custom_providers]]` block is in your user config, not the workspace `vtcode.toml`. |
| API key not found                  | Export `UBERLLM_API_KEY` in the shell that starts VT Code.                                        |
| HTTP 401                           | The key is wrong or revoked. Create a new one at uberllm.dev.                                     |
| HTTP 402                           | The prepaid balance is used up or the key's spend cap is reached. Check `GET /v1/balance`.        |
| HTTP 404                           | Confirm that `base_url` ends in `/v1`, without `/chat/completions` or `/responses`.               |
| Model not found                    | Copy an exact ID from `GET /v1/models`.                                                           |

## References

- [UberLLM model list and prices](https://uberllm.dev/models)
- [UberLLM docs for agents (llms.txt)](https://uberllm.dev/llms.txt)
- [VT Code custom provider configuration](../config/models.md#custom_providers)
