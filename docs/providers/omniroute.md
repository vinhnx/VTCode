# OmniRoute Integration Guide

[OmniRoute](https://github.com/diegosouzapw/OmniRoute) exposes an
OpenAI-compatible gateway, so VT Code can use it through the existing
`[[custom_providers]]` support without a dedicated runtime provider.

## Prerequisites

1. Start OmniRoute and confirm that its API is reachable.
2. If API-key authentication is enabled, store the key in your environment:

```bash
export OMNIROUTE_API_KEY="your-omniroute-key"
```

## Configuration

Add the gateway to your workspace `vtcode.toml`:

```toml
# Optional governance guard: only expose this provider in VT Code.
providers_whitelist = ["omniroute"]

[agent]
provider = "omniroute"
default_model = "auto"
reasoning_effort = "none"

[[custom_providers]]
name = "omniroute"
display_name = "OmniRoute"
base_url = "http://localhost:20128/v1"
api_key_env = "OMNIROUTE_API_KEY"
model = "auto"
```

Then run VT Code normally:

```bash
vtcode ask "Summarize this repository"
```

`base_url` must include `/v1`. VT Code appends `/chat/completions` or
`/responses` to that value, depending on the selected model and API capability.
Using the server root would send requests to the wrong path.

## Model Selection

`auto` delegates model selection and provider fallback to OmniRoute. To pin a
specific model, query the gateway catalog and copy an exact model ID:

```bash
curl http://localhost:20128/v1/models \
  -H "Authorization: Bearer ${OMNIROUTE_API_KEY}"
```

Update both `agent.default_model` and `custom_providers.model` with the chosen
ID. VT Code does not dynamically import `/v1/models` for custom providers. To
offer several known IDs in `/model`, add them explicitly:

```toml
[[custom_providers]]
name = "omniroute"
display_name = "OmniRoute"
base_url = "http://localhost:20128/v1"
api_key_env = "OMNIROUTE_API_KEY"
model = "auto"
models = ["auto", "auto/coding"]
```

## Protocol Behavior

- VT Code sends the configured key as a Bearer token.
- Unknown model IDs such as `auto` use OpenAI Chat Completions.
- Models recognized by VT Code as Responses-capable use `/responses`; if that
  endpoint rejects an optional Responses request, VT Code falls back to Chat
  Completions for custom providers.
- Streaming and function tools use VT Code's shared OpenAI-compatible paths.
- Reasoning controls depend on the selected model. Leave `reasoning_effort`
  disabled for `auto`, or enable it only when the pinned model supports the
  corresponding OpenAI-compatible fields.
- Request timeouts use VT Code's global `[timeouts]` configuration.
- Cross-provider failover is handled by the OmniRoute model or combo selected
  in `default_model`; VT Code treats OmniRoute as one custom endpoint.

## Remote and Container Setups

`localhost` refers to the machine or container running VT Code. When OmniRoute
runs elsewhere, replace the host while preserving the `/v1` suffix:

```toml
base_url = "http://omniroute-host:20128/v1"
```

## Troubleshooting

| Symptom | Resolution |
| --- | --- |
| `Unknown provider: omniroute` | Confirm that the `[[custom_providers]]` block is in the active `vtcode.toml`. |
| API key not found | Export `OMNIROUTE_API_KEY` or store it with `vtcode secret add omniroute --key-name OMNIROUTE_API_KEY`. |
| HTTP 404 | Confirm that `base_url` ends in `/v1`, without `/chat/completions` or `/responses`. |
| Model not found | Copy an exact ID from `GET /v1/models`, or use `auto`. |
| VT Code cannot connect from a container | Use a hostname reachable from that container instead of `localhost`. |

## References

- [OmniRoute repository](https://github.com/diegosouzapw/OmniRoute)
- [VT Code custom provider configuration](../config/config.md#custom_providers)
