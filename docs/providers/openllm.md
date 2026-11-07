# OpenLLM Integration Guide

[OpenLLM](https://www.openllm.sh) routes API keys, OAuth subscription accounts, and local models through one
format-agnostic endpoint. It runs as a local daemon on your machine and extends to the cloud. VT Code can use it through
the existing `[[custom_providers]]` support without a dedicated runtime provider.

OpenLLM serves the full `/v1/` API surface and also provides an MCP server. See the
[OpenLLM documentation](https://docs.openllm.sh).

## Prerequisites

1. Install OpenLLM and sign in. The installer sets up the `openllm` CLI and the local daemon:

   ```bash
   curl -fsSL https://www.openllm.sh/install | bash
   openllm login
   ```

2. Confirm the daemon is running. It listens on `127.0.0.1:8787` by default:

   ```bash
   curl http://127.0.0.1:8787/status
   ```

3. Export a placeholder key. VT Code requires a non-empty key for every custom provider. The local daemon uses your
   saved OpenLLM login and ignores any bearer token that does not start with `sk-llm-`, so the placeholder is never
   checked:

   ```bash
   export OPENLLM_API_KEY="local"
   ```

## Configuration

Custom providers are trusted configuration, so VT Code rejects them in a workspace `vtcode.toml`. Add the entry to your
user configuration (`~/.vtcode/vtcode.toml` or the platform config directory) or pass a file with
`--config path/to/file.toml`. See [Configuration precedence](../config/CONFIGURATION_PRECEDENCE.md).

```toml
[agent]
provider = "openllm"
default_model = "plus"

[[custom_providers]]
name = "openllm"
display_name = "OpenLLM"
base_url = "http://127.0.0.1:8787/v1"
api_key_env = "OPENLLM_API_KEY"
model = "plus"
models = ["ultra", "plus", "lite"]
# context_window = 256000 # Optional; default is 128000 tokens
```

Then run VT Code normally:

```bash
vtcode ask "Summarize this repository"
```

`base_url` must include `/v1`. VT Code appends `/chat/completions` or `/responses` to that value.

## Model Selection

`ultra`, `plus`, and `lite` are OpenLLM's built-in aliases: most capable, everyday, and fast and low cost. Each resolves
to a chain of models from the accounts you have connected. They exist only for providers you have, and OpenLLM lets you
disable them, so check that they appear in your catalog:

```bash
curl http://127.0.0.1:8787/v1/models
```

To pin a specific model, copy an exact ID from that response (IDs look like `<provider>/<model>`), then update both
`agent.default_model` and `custom_providers.model`, and add the ID to `models` to list it in `/model`. VT Code does not
import `/v1/models` for custom providers, so the picker shows only the IDs you list.

## Cloud Origin

To use OpenLLM without a local daemon, for example in CI or on a remote host, change the base URL and set a real key.
Create the key in the OpenLLM dashboard under **Devices & keys**:

```toml
[[custom_providers]]
name = "openllm"
display_name = "OpenLLM"
base_url = "https://www.openllm.sh/v1"
api_key_env = "OPENLLM_API_KEY"
model = "plus"
models = ["ultra", "plus", "lite"]
```

```bash
export OPENLLM_API_KEY="sk-llm-..."
```

Models served from API keys or included inference work from the cloud origin directly. Subscription and local models
are tunneled to one of your online devices, and fail with HTTP 403 `subscription_requires_daemon` or
`local_requires_daemon` when no device is online.

## Protocol Behavior

- VT Code sends the key as `Authorization: Bearer`.
- Alias IDs such as `plus` are unknown to VT Code's model catalog, so they use OpenAI Chat Completions.
- Streaming and function tools use VT Code's shared OpenAI-compatible paths.
- Fallback inside an alias chain happens before the first output token. The `x-openllm-resolved-model` response header
  names the model that answered.
- Custom providers default to a 128K context window. Set `context_window` in tokens if your routed models support more.
- Request timeouts use VT Code's global `[timeouts]` configuration.

## Troubleshooting

| Symptom                                    | Resolution                                                                                        |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------- |
| `openllm` is missing from `/model`         | Confirm the `[[custom_providers]]` block is in your user config, not the workspace `vtcode.toml`. |
| API key not found                          | Export `OPENLLM_API_KEY`. For the local daemon any non-empty placeholder works.                   |
| Connection refused on `127.0.0.1:8787`     | Start the daemon with `openllm start`, or check `OPENLLM_DAEMON_PORT` if you changed the port.    |
| HTTP 401 with `[openllm] API key required` | The daemon has no saved login. Run `openllm login`.                                               |
| HTTP 404                                   | Confirm that `base_url` ends in `/v1`, without `/chat/completions` or `/responses`.               |
| Model not found                            | Copy an exact ID from `GET /v1/models`. The `ultra`, `plus`, and `lite` aliases may be disabled.  |

## References

- [OpenLLM documentation](https://docs.openllm.sh)
- [OpenLLM API reference](https://www.openllm.sh/api/swagger/ui)
- [VT Code custom provider configuration](../config/models.md#custom_providers)
