# LiteLLM Provider Guide

LiteLLM is an OpenAI-compatible proxy that provides a unified interface to **100+ LLM providers** (OpenAI, Anthropic, Bedrock, Vertex AI, Azure, Hugging Face, Together AI, vLLM, and more). VT Code connects to it via the standard `/chat/completions` endpoint.

## Quick Start

### 1. Install & Start LiteLLM

```bash
pip install 'litellm[proxy]'

# Start with a specific model
litellm --model openai/gpt-4o
# → Proxy running on http://0.0.0.0:4000
```

### 2. Configure VT Code

**Option A — Environment variables:**

```bash
export LITELLM_API_KEY="your-proxy-key"   # Optional if proxy has no auth
export LITELLM_BASE_URL="http://localhost:4000"  # Default, can omit
```

**Option B — `vtcode.toml`:**

```toml
[agent]
provider = "litellm"
api_key_env = "LITELLM_API_KEY"
default_model = "gpt-4o"  # Model name as configured in LiteLLM
```

### 3. Run VT Code

```bash
vtcode --provider litellm --model gpt-4o
```

## Configuration

| Setting | Default | Description |
|---------|---------|-------------|
| `LITELLM_API_KEY` | *(empty)* | API key for proxy auth (optional for local) |
| `LITELLM_BASE_URL` | `http://localhost:4000` | LiteLLM proxy endpoint |

### Model Naming

The model name you pass to VT Code is forwarded directly to LiteLLM. Use the model names as defined in your LiteLLM `config.yaml`:

```yaml
# litellm_config.yaml
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: sk-...
  - model_name: claude-sonnet
    litellm_params:
      model: anthropic/claude-sonnet-4-6
      api_key: sk-ant-...
```

Then use those aliases in VT Code:

```bash
vtcode --provider litellm --model gpt-4o
vtcode --provider litellm --model claude-sonnet
```

### Using the `litellm/` Prefix

VT Code auto-detects LiteLLM when the model slug starts with `litellm/`:

```bash
vtcode --model litellm/my-model
```

## Features

- **Streaming**: Full SSE streaming support via `/chat/completions`
- **Tool calling**: OpenAI-compatible function calling forwarded through the proxy
- **Any model**: LiteLLM routes to the underlying provider — use any model your proxy supports
- **Load balancing**: LiteLLM can balance across multiple deployments of the same model
- **Cost tracking**: LiteLLM provides usage and spend tracking across all providers

## Troubleshooting

### Connection Refused

Ensure LiteLLM is running and accessible:

```bash
curl http://localhost:4000/health
```

### Authentication Errors

If your LiteLLM proxy requires a virtual key:

```bash
export LITELLM_API_KEY="sk-your-virtual-key"
```

### Model Not Found

Verify the model is configured in your LiteLLM `config.yaml`:

```bash
curl http://localhost:4000/models
```

## References

- [LiteLLM Documentation](https://docs.litellm.ai/)
- [LiteLLM Proxy Quick Start](https://docs.litellm.ai/docs/proxy/quick_start)
- [Supported Providers](https://docs.litellm.ai/docs/providers)
