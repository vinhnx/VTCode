# Hugging Face Models Quick Reference

## Conversational LLMs

| Model ID | Name | Context | Reasoning | Tools | Structured Output | Best For |
|----------|------|---------|-----------|-------|-------------------|----------|
| `google/gemma-2-2b-it` | Gemma 2 2B IT | 8K | No | Yes | Yes | Fast, lightweight tasks |
| `Qwen/Qwen3-Coder-480B-A35B-Instruct` | Qwen3 Coder 480B | 131K | Yes | Yes | Yes | Advanced coding |
| `openai/gpt-oss-120b` | GPT-OSS 120B | 131K | Yes | Yes | Yes | Top tool calling |
| `zai-org/GLM-4.5` | GLM-4.5 | 131K | Yes | Yes | Yes | General reasoning |
| `Qwen/Qwen3-4B-Thinking-2507` | Qwen3 4B Thinking | 32K | Yes | Yes | Yes | Small reasoning model |
| `Qwen/Qwen2.5-7B-Instruct-1M` | Qwen2.5 7B Instruct | 1M | Yes | Yes | Yes | Very long context |
| `Qwen/Qwen2.5-Coder-32B-Instruct` | Qwen2.5 Coder 32B | 131K | Yes | Yes | Yes | Code generation |
| `deepseek-ai/DeepSeek-R1` | DeepSeek R1 | 131K | Yes | Yes | Yes | Advanced reasoning |
| `deepseek-ai/DeepSeek-V3.2` | DeepSeek V3.2 | 131K | Yes | Yes | Yes | Chat & reasoning |
| `openai/gpt-oss-20b` | GPT-OSS 20B | 131K | Yes | Yes | Yes | Balanced performance |
| `zai-org/GLM-4.6` | GLM-4.6 | 131K | Yes | Yes | Yes | General reasoning |
| `zai-org/GLM-4.7` | GLM-4.7 | 131K | Yes | Yes | Yes | Latest GLM version |
| `moonshotai/Kimi-K2.5` | Kimi K2.5 | 262K | Yes | Yes | Yes | Reasoning tasks |

## Via Novita Provider

| Model ID | Name | Context | Reasoning | Tools | Structured Output | Best For |
|----------|------|---------|-----------|-------|-------------------|----------|
| `MiniMaxAI/MiniMax-M2:novita` | MiniMax-M2 (Novita) | 200K | No | No | No | Text generation |
| `MiniMaxAI/MiniMax-M2.5:novita` | MiniMax-M2.5 (Novita) | 200K | Yes | Yes | Yes | Enhanced reasoning |
| `deepseek-ai/DeepSeek-V3.2:novita` | DeepSeek-V3.2 (Novita) | 160K | Yes | Yes | Yes | Cost-optimized reasoning |
| `XiaomiMiMo/MiMo-V2-Flash:novita` | MiMo-V2-Flash (Novita) | 256K | Yes | Yes | Yes | Fast, long-context |

## Vision-Language Models (VLMs)

| Model ID | Name | Context | Modalities | Reasoning | Tools | Best For |
|----------|------|---------|------------|-----------|-------|----------|
| `zai-org/GLM-4.5V` | GLM-4.5V | 131K | Text + Image | Yes | Yes | Vision reasoning |

## Usage Examples

### Basic Chat Completion

```bash
# Using the default model (GPT-OSS 120B)
vtcode ask "Explain async/await in Rust" --provider huggingface

# Using a specific model
vtcode ask "Write a sorting algorithm" --provider huggingface --model "Qwen/Qwen3-Coder-480B-A35B-Instruct"
```

### Long Context Tasks

```bash
# Use Qwen2.5-7B-Instruct-1M for very long documents
vtcode ask "Analyze this entire codebase" --provider huggingface --model "Qwen/Qwen2.5-7B-Instruct-1M"
```

### Vision Tasks

```bash
# Use GLM-4.5V for image understanding
vtcode ask "What's in this image?" --provider huggingface --model "zai-org/GLM-4.5V" --image screenshot.png
```

### Reasoning Tasks

```bash
# Use DeepSeek R1 for complex reasoning
vtcode ask "Solve this math problem step by step" --provider huggingface --model "deepseek-ai/DeepSeek-R1"
```

### Coding Tasks

```bash
# Use Qwen3 Coder for advanced coding
vtcode ask "Refactor this function" --provider huggingface --model "Qwen/Qwen3-Coder-480B-A35B-Instruct"

# Use Qwen2.5 Coder for code generation
vtcode ask "Generate a REST API" --provider huggingface --model "Qwen/Qwen2.5-Coder-32B-Instruct"
```

### Via Novita Provider

```bash
# Cost-optimized reasoning with DeepSeek-V3.2
vtcode ask "Solve this problem" --provider huggingface --model "deepseek-ai/DeepSeek-V3.2:novita"

# Fast reasoning with ultra-long context (256K)
vtcode ask "Analyze this entire codebase" --provider huggingface --model "XiaomiMiMo/MiMo-V2-Flash:novita"

# MiniMax-M2 for text generation
vtcode ask "Write a blog post" --provider huggingface --model "MiniMaxAI/MiniMax-M2:novita"
```

## Model Selection Guide

### Choose by Use Case

- **Best Overall**: `openai/gpt-oss-120b` - Great tool calling, balanced performance
- **Best for Coding**: `Qwen/Qwen3-Coder-480B-A35B-Instruct` - Specialized for code
- **Best for Reasoning**: `deepseek-ai/DeepSeek-R1` or `deepseek-ai/DeepSeek-V3.2:novita` - Advanced reasoning
- **Best for Long Context**: `XiaomiMiMo/MiMo-V2-Flash:novita` (256K) or `Qwen/Qwen2.5-7B-Instruct-1M` (1M)
- **Best for Vision**: `zai-org/GLM-4.5V` - Vision-language model
- **Fastest**: `google/gemma-2-2b-it` - Small, fast, lightweight
- **Best Value**: `deepseek-ai/DeepSeek-V3.2:novita` - Cost-optimized reasoning on Novita
- **Best Small Reasoning**: `Qwen/Qwen3-4B-Thinking-2507` - Compact with reasoning

### Choose by Context Window

- **1M tokens**: Qwen2.5-7B-Instruct-1M
- **256K tokens**: XiaomiMiMo/MiMo-V2-Flash:novita
- **200K tokens**: MiniMaxAI/MiniMax-M2:novita
- **131K tokens**: Most models (recommended for general use)
- **32K tokens**: Qwen3-4B-Thinking-2507
- **8K tokens**: Gemma 2 2B IT

## Configuration

### Environment Setup

```bash
# Set your Hugging Face token
export HF_TOKEN="hf_..."

# Optional: Override base URL
export HUGGINGFACE_BASE_URL="https://router.huggingface.co/v1"
```

### Model ID Format

HuggingFace model IDs support optional provider selection suffixes:

```bash
# Standard format (auto-selects default or first available provider)
zai-org/GLM-4.7
deepseek-ai/DeepSeek-R1

# With provider suffix (explicit provider selection)
MiniMaxAI/MiniMax-M2:novita          # Force Novita provider
deepseek-ai/DeepSeek-V3.2:novita    # Force Novita provider  
XiaomiMiMo/MiMo-V2-Flash:novita     # Force Novita provider
deepseek-ai/DeepSeek-R1:fastest     # Select fastest provider
openai/gpt-oss-120b:cheapest        # Select cheapest provider
```

**Provider Suffixes:**
- `:provider-name` - Force specific provider (e.g., `:novita`, `:together`, `:groq`)
- `:fastest` - Select provider with highest throughput
- `:cheapest` - Select provider with lowest cost
- No suffix - Auto-select available provider (recommended)

### vtcode.toml

```toml
[llm]
provider = "huggingface"
model = "openai/gpt-oss-120b"  # or any model from the table above

[llm.huggingface]
api_key = "${HF_TOKEN}"
# base_url = "https://router.huggingface.co/v1"  # optional override
```

## API Features

All models support:
- Streaming - Real-time token streaming via SSE
- Tool Calling - Function calling and parallel execution
- Structured Output - JSON mode via `response_format`

Most models support:
- Reasoning - Advanced reasoning capabilities (11/13 models)

Some models support:
- Vision - Image inputs (GLM-4.5V only)
- Ultra-Long Context - 1M tokens (Qwen2.5-7B-Instruct-1M only)

## Links

- [Chat Completion API Docs](https://huggingface.co/docs/inference-providers/tasks/chat-completion)
- [API Playground](https://huggingface.co/playground)
- [Model Hub](https://huggingface.co/models?inference=warm&pipeline_tag=text-generation&sort=trending)
- [Provider Selection Guide](../HUGGINGFACE_PROVIDER_SELECTION.md)
