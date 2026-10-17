# NVIDIA NIM Quick Reference

| Setting           | Value                                 |
| ----------------- | ------------------------------------- |
| Provider          | `nvidia`                              |
| API key           | `NVIDIA_API_KEY`                      |
| Default endpoint  | `https://integrate.api.nvidia.com/v1` |
| Endpoint override | `NVIDIA_BASE_URL`                     |
| Default model     | `nvidia/nemotron-3-ultra-550b-a55b`   |
| Context metadata  | 1,000,000 tokens                      |

```bash
export NVIDIA_API_KEY="nvapi-..."
vtcode --provider nvidia --model nvidia/nemotron-3-nano-30b-a3b ask "Summarize this change"
```

NVIDIA uses OpenAI-compatible Chat Completions. VT Code supports streaming, function tools, structured output,
configurable thinking, stream usage, and `reasoning_content` extraction. Explicit NVIDIA model IDs are accepted even
when they are not in the curated picker list.
