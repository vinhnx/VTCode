# NVIDIA NIM Provider Guide

VT Code supports NVIDIA NIM through NVIDIA's OpenAI-compatible Chat Completions endpoint.

## Setup

Set an API key from the [NVIDIA Build platform](https://build.nvidia.com/) and select the provider:

```bash
export NVIDIA_API_KEY="nvapi-..."
vtcode --provider nvidia --model nvidia/nemotron-3-ultra-550b-a55b ask "Review this repository"
```

The default endpoint is `https://integrate.api.nvidia.com/v1`. Set `NVIDIA_BASE_URL` when routing through a compatible
gateway or proxy.

Equivalent `vtcode.toml` configuration:

```toml
[agent]
provider = "nvidia"
default_model = "nvidia/nemotron-3-ultra-550b-a55b"
```

## Curated models

These models appear in the NVIDIA section of the `/model` picker:

| Model                                |   Context | Notes                                  |
| ------------------------------------ | --------: | -------------------------------------- |
| `nvidia/nemotron-3-ultra-550b-a55b`  | 1,000,000 | Default flagship agentic model         |
| `nvidia/nemotron-3-super-120b-a12b`  | 1,000,000 | Efficient long-context reasoning       |
| `nvidia/nemotron-3-nano-30b-a3b`     | 1,000,000 | Lower-cost reasoning and tool use      |
| `z-ai/glm-5.2`                       | 1,000,000 | GLM-5.2 served by NVIDIA NIM           |
| `deepseek-ai/deepseek-v4-flash-0731` | 1,000,000 | DeepSeek V4 Flash served by NVIDIA NIM |

NVIDIA's catalog is larger than this curated list. An explicitly configured model ID is forwarded without local
allowlist rejection:

```bash
vtcode --provider nvidia --model nvidia/any-catalog-model ask "Explain this code"
```

## Reasoning and tools

VT Code maps any selected reasoning effort other than `none` to NVIDIA's `chat_template_kwargs.enable_thinking = true`;
no effort or `none` disables thinking. When tools are present, VT Code also sends
`chat_template_kwargs.force_nonempty_content = true`, as required by NVIDIA's Nemotron tool-calling format. Streaming
usage totals and `reasoning_content` are preserved in both streaming and non-streaming responses.

## References

- [NVIDIA API catalog](https://build.nvidia.com/llms.txt)
- [Nemotron 3 Ultra model card](https://build.nvidia.com/nvidia/nemotron-3-ultra-550b-a55b/modelcard)
- [NVIDIA NIM API reference](https://docs.nvidia.com/nim/large-language-models/latest/api-reference.html)
