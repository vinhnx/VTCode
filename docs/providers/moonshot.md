# Moonshot AI Integration

Moonshot AI exposes an OpenAI-compatible chat completions API at `https://api.moonshot.cn/v1`. The provider supports both the general `moonshot-v1` family and the limited-time **Kimi K2 Turbo Preview** promotion described in the official documentation.

## Supported models

| Identifier | Context window | Notes |
| --- | --- | --- |
| `moonshot-v1-8k` | 8K tokens | Fastest Moonshot v1 tier |
| `moonshot-v1-32k` | 32K tokens | Balanced general-purpose tier |
| `moonshot-v1-128k` | 128K tokens | Long-context flagship tier |
| `kimi-k2-0711-preview` | 8K tokens | Early preview of the Kimi K2 reasoning model |
| `kimi-k2-turbo-preview` | 8K tokens | Promotional Turbo Preview (50% off during the published campaign) |

Moonshot’s API currently accepts standard Chat Completions payloads, including tool calling definitions. The limited-time Turbo preview promotion details are published at <https://platform.moonshot.ai/docs/promotion#kimi-k2-model-limited-time-promotion>.

## Authentication

Set the `MOONSHOT_API_KEY` environment variable before launching VTCode:

```bash
export MOONSHOT_API_KEY="your-moonshot-key"
```

You can also specify a provider entry in `vtcode.toml` or `~/.vtcode/config.toml` and supply the key inline if necessary.

## Configuration snippet

```toml
[agent]
provider = "moonshot"
default_model = "moonshot-v1-32k"
api_key_env = "MOONSHOT_API_KEY"

[prompt_cache.providers.moonshot]
# Prompt caching is advisory only at the moment.
enabled = false
```

## Usage notes

* Streaming and tool calling follow the same semantics as OpenAI’s Chat Completions API.
* The provider currently does not expose explicit prompt cache controls; the VTCode integration records the preference but does not forward cache directives yet.
* Reasoning-effort overrides are disabled until the public API documents such parameters for Moonshot models.
